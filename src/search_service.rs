use crate::properties;
use crate::search_query::{
    compile_query, evaluate_compiled, explain, extract_tags, parse_query, requirements,
    EvaluationOptions, QueryError, QueryRequirements, SearchDocument,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    #[default]
    Relevance,
    Filename,
    Modified,
    Created,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SearchOptions {
    pub match_case: bool,
    pub sort: SearchSort,
    pub limit: usize,
}

#[derive(Clone, Debug)]
pub struct SearchResult {
    pub path: PathBuf,
    pub relative_path: String,
    pub count: usize,
    pub first_line: Option<usize>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total: usize,
    pub more: bool,
    pub explanation: String,
    pub explanation_norwegian: String,
}

/// One file a literal replacement would change.
#[derive(Clone, Debug)]
pub struct ReplacementFile {
    pub path: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub count: usize,
}

/// The result of planning a project-wide replacement.
#[derive(Clone, Debug, Default)]
pub struct ReplacePlan {
    pub files: Vec<ReplacementFile>,
    pub total: usize,
}

impl ReplacePlan {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[derive(Clone, Debug)]
struct CachedFile {
    modified: Option<SystemTime>,
    len: u64,
    content: Arc<str>,
    properties: Option<Vec<(String, serde_yaml::Value)>>,
    tags: Option<Vec<String>>,
}

#[derive(Default)]
struct CacheState {
    entries: HashMap<PathBuf, CachedFile>,
    retained_bytes: usize,
    generation: u64,
}

#[derive(Clone)]
pub struct SearchService {
    cache: Arc<Mutex<CacheState>>,
}

impl Default for SearchService {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchService {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(CacheState::default())),
        }
    }

    pub fn search(
        &self,
        root: &Path,
        paths: &[PathBuf],
        input: &str,
        options: SearchOptions,
    ) -> Result<SearchResponse, QueryError> {
        self.search_cancellable(root, paths, input, options, None)
    }

    pub fn search_cancellable(
        &self,
        root: &Path,
        paths: &[PathBuf],
        input: &str,
        options: SearchOptions,
        cancellation: Option<(Arc<AtomicU64>, u64)>,
    ) -> Result<SearchResponse, QueryError> {
        if is_cancelled(&cancellation) {
            return Ok(SearchResponse::default());
        }
        let query = parse_query(input)?;
        if is_cancelled(&cancellation) {
            return Ok(SearchResponse::default());
        }
        let requirements = requirements(&query);
        let cache_generation = self
            .cache
            .lock()
            .map(|cache| cache.generation)
            .unwrap_or_default();
        if is_cancelled(&cancellation) {
            return Ok(SearchResponse::default());
        }
        let compiled = compile_query(
            &query,
            EvaluationOptions {
                match_case: options.match_case,
            },
        )?;
        if is_cancelled(&cancellation) {
            return Ok(SearchResponse::default());
        }
        self.prune_cache(paths);
        let mut results = Vec::new();
        for path in paths {
            if is_cancelled(&cancellation) {
                return Ok(SearchResponse::default());
            }
            let relative_path = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned();
            let Ok(metadata) = fs::metadata(path) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let is_text = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    ["md", "markdown", "base", "canvas"]
                        .iter()
                        .any(|known| extension.eq_ignore_ascii_case(known))
                });
            let document = if requirements.read_content && is_text {
                self.document(
                    path,
                    &metadata,
                    &relative_path,
                    requirements,
                    cache_generation,
                )
            } else {
                SearchDocument::path_only(path.clone(), relative_path.clone())
                    .with_times(metadata.modified().ok(), metadata.created().ok())
            };
            if is_cancelled(&cancellation) {
                return Ok(SearchResponse::default());
            }
            let evaluation = evaluate_compiled(
                &compiled,
                &document,
                EvaluationOptions {
                    match_case: options.match_case,
                },
            );
            if is_cancelled(&cancellation) {
                return Ok(SearchResponse::default());
            }
            if evaluation.matched {
                results.push(SearchResult {
                    path: path.clone(),
                    relative_path,
                    count: evaluation.occurrences,
                    first_line: (crate::file_preview::extension(path) != "canvas")
                        .then_some(evaluation.first_line)
                        .flatten(),
                    modified: document.modified,
                    created: document.created,
                });
            }
        }
        if is_cancelled(&cancellation) {
            return Ok(SearchResponse::default());
        }
        sort_results(&mut results, options.sort);
        let total = results.len();
        let limit = options.limit.max(1);
        let more = total > limit;
        results.truncate(limit);
        Ok(SearchResponse {
            results,
            total,
            more,
            explanation: explain(&query, false),
            explanation_norwegian: explain(&query, true),
        })
    }

    /// Plan a literal find-and-replace across `paths`. Only files whose raw
    /// text is what search indexes are considered — Markdown and `.base`.
    /// JSON Canvas boards are searched through a projection, so their source
    /// is left untouched.
    pub fn plan_replacements(
        &self,
        paths: &[PathBuf],
        needle: &str,
        replacement: &str,
        match_case: bool,
    ) -> ReplacePlan {
        let mut plan = ReplacePlan::default();
        if needle.is_empty() {
            return plan;
        }
        for path in paths {
            if !is_replaceable(path) {
                continue;
            }
            let Ok(content) = fs::read_to_string(path) else {
                continue;
            };
            let (after, count) = replace_literal(&content, needle, replacement, match_case);
            if count == 0 {
                continue;
            }
            plan.total += count;
            plan.files.push(ReplacementFile {
                path: path.clone(),
                before: content.into_bytes(),
                after: after.into_bytes(),
                count,
            });
        }
        plan
    }

    fn document(
        &self,
        path: &Path,
        metadata: &fs::Metadata,
        relative_path: &str,
        requirements: QueryRequirements,
        cache_generation: u64,
    ) -> SearchDocument {
        let modified = metadata.modified().ok();
        let len = metadata.len();
        let is_canvas = crate::file_preview::extension(path) == "canvas";
        let cached = if modified.is_some() {
            self.cache
                .lock()
                .ok()
                .and_then(|cache| cache.entries.get(path).cloned())
                .filter(|entry| entry.modified == modified && entry.len == len)
                .filter(|_| {
                    fs::metadata(path).ok().is_some_and(|current| {
                        current.len() == len && current.modified().ok() == modified
                    })
                })
        } else {
            None
        };
        if let Some(mut cached) = cached {
            let properties = if requirements.parse_properties && !is_canvas {
                cached
                    .properties
                    .clone()
                    .unwrap_or_else(|| properties::properties(&cached.content))
            } else {
                Vec::new()
            };
            let tags = if requirements.extract_tags && !is_canvas {
                cached.tags.clone().unwrap_or_else(|| {
                    extract_tags(
                        &cached.content,
                        if requirements.parse_properties {
                            &properties
                        } else {
                            &[]
                        },
                    )
                })
            } else {
                Vec::new()
            };
            if requirements.parse_properties && !is_canvas && cached.properties.is_none() {
                cached.properties = Some(properties.clone());
            }
            if requirements.extract_tags && !is_canvas && cached.tags.is_none() {
                cached.tags = Some(tags.clone());
            }
            if !is_canvas && (requirements.parse_properties || requirements.extract_tags) {
                if let Ok(mut cache) = self.cache.lock() {
                    if let Some(entry) = (cache.generation == cache_generation)
                        .then(|| cache.entries.get_mut(path))
                        .flatten()
                    {
                        if entry.modified == cached.modified && entry.len == cached.len {
                            entry.properties = cached.properties;
                            entry.tags = cached.tags;
                        }
                    }
                }
            }
            return SearchDocument::new(path, relative_path, cached.content.to_string())
                .with_metadata(properties, tags)
                .with_times(modified, metadata.created().ok());
        }
        let Ok(content) = fs::read_to_string(path) else {
            return SearchDocument::path_only(path, relative_path)
                .with_times(modified, metadata.created().ok());
        };
        let content = if is_canvas {
            canvas_search_projection(&content)
        } else {
            content
        };
        let properties = if requirements.parse_properties && !is_canvas {
            Some(properties::properties(&content))
        } else {
            None
        };
        let tags = if requirements.extract_tags && !is_canvas {
            Some(extract_tags(
                &content,
                properties.as_deref().unwrap_or_default(),
            ))
        } else {
            None
        };
        let document = SearchDocument::new(path, relative_path, content.clone())
            .with_metadata(
                properties.clone().unwrap_or_default(),
                tags.clone().unwrap_or_default(),
            )
            .with_times(modified, metadata.created().ok());
        let current_metadata = fs::metadata(path).ok();
        let unchanged = current_metadata
            .as_ref()
            .is_some_and(|current| current.len() == len && current.modified().ok() == modified);
        if unchanged && modified.is_some() && content.len() <= MAX_CACHE_BYTES {
            if let Ok(mut cache) = self.cache.lock() {
                insert_cache_if_current(
                    &mut cache,
                    cache_generation,
                    path.to_path_buf(),
                    CachedFile {
                        modified,
                        len,
                        content: Arc::from(content),
                        properties,
                        tags,
                    },
                );
            }
        }
        document
    }

    pub fn invalidate(&self, paths: impl IntoIterator<Item = PathBuf>) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.generation = cache.generation.wrapping_add(1);
            for path in paths {
                if let Some(entry) = cache.entries.remove(&path) {
                    cache.retained_bytes = cache.retained_bytes.saturating_sub(entry.content.len());
                }
            }
        }
    }

    pub fn invalidate_all(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.generation = cache.generation.wrapping_add(1);
            cache.entries.clear();
            cache.retained_bytes = 0;
        }
    }

    fn prune_cache(&self, paths: &[PathBuf]) {
        let paths = paths.iter().collect::<std::collections::HashSet<_>>();
        if let Ok(mut cache) = self.cache.lock() {
            let removed = cache
                .entries
                .keys()
                .filter(|path| !paths.contains(path))
                .cloned()
                .collect::<Vec<_>>();
            for path in removed {
                if let Some(entry) = cache.entries.remove(&path) {
                    cache.retained_bytes = cache.retained_bytes.saturating_sub(entry.content.len());
                }
            }
        }
    }
}

fn canvas_search_projection(raw: &str) -> String {
    crate::canvas::CanvasDocument::parse(raw)
        .map(|canvas| canvas.search_projection())
        .unwrap_or_default()
}

/// Whether a file's raw text is what search indexes, and so can be replaced.
fn is_replaceable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["md", "markdown", "base"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// Replace every non-overlapping occurrence of `needle`, returning the new
/// text and the number of replacements. An empty needle matches nothing.
///
/// Case-insensitive matching lowercases the *whole* strings, exactly like
/// `search_query`, so replacement never touches text the search would not
/// match. Matches are mapped back to original character boundaries with a
/// fold map, so a match that lands inside a multi-character fold (e.g. `İ`)
/// is skipped rather than corrupting the text.
pub fn replace_literal(
    text: &str,
    needle: &str,
    replacement: &str,
    match_case: bool,
) -> (String, usize) {
    if needle.is_empty() {
        return (text.to_string(), 0);
    }
    if match_case {
        let count = text.matches(needle).count();
        return (text.replace(needle, replacement), count);
    }
    let folded_needle = needle.to_lowercase();
    if folded_needle.is_empty() {
        return (text.to_string(), 0);
    }
    let Folded {
        text: haystack,
        char_at,
        starts,
    } = fold_case(text);
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut cursor = 0usize;
    let mut emitted = 0usize;
    while let Some(relative) = haystack[cursor..].find(folded_needle.as_str()) {
        let start = cursor + relative;
        let end = start + folded_needle.len();
        let aligned = starts[start] && (end == haystack.len() || starts[end]);
        if aligned {
            let original_start = char_at[start];
            let original_end = if end == haystack.len() {
                text.len()
            } else {
                char_at[end]
            };
            out.push_str(&text[emitted..original_start]);
            out.push_str(replacement);
            count += 1;
            emitted = original_end;
            cursor = end;
        } else {
            cursor = (start + 1..haystack.len())
                .find(|&index| starts[index])
                .unwrap_or(haystack.len());
        }
        if cursor >= haystack.len() {
            break;
        }
    }
    out.push_str(&text[emitted..]);
    (out, count)
}

/// A matching form of `text` with, for every folded byte, the byte offset of
/// the original character it came from and whether it starts that character's
/// fold.
struct Folded {
    text: String,
    char_at: Vec<usize>,
    starts: Vec<bool>,
}

fn fold_case(text: &str) -> Folded {
    // The boundary map is built per character, but the text matched against
    // uses whole-string lowercasing to agree with `search_query::normalize`
    // (which is context-sensitive — e.g. a Greek final sigma). The two layouts
    // differ only in which lowercase form is chosen, never in length; if a
    // future mapping ever changed the length, fall back to the per-character
    // fold so the map stays valid.
    let mut per_char = String::with_capacity(text.len());
    let mut char_at = Vec::with_capacity(text.len());
    let mut starts = Vec::with_capacity(text.len());
    for (index, ch) in text.char_indices() {
        let begin = per_char.len();
        for lower in ch.to_lowercase() {
            per_char.push(lower);
        }
        for offset in begin..per_char.len() {
            char_at.push(index);
            starts.push(offset == begin);
        }
    }
    let whole = text.to_lowercase();
    let folded = if whole.len() == per_char.len() {
        whole
    } else {
        per_char
    };
    Folded {
        text: folded,
        char_at,
        starts,
    }
}

fn is_cancelled(cancellation: &Option<(Arc<AtomicU64>, u64)>) -> bool {
    cancellation
        .as_ref()
        .is_some_and(|(generation, expected)| generation.load(Ordering::Relaxed) != *expected)
}

fn insert_cache(cache: &mut CacheState, path: PathBuf, entry: CachedFile) {
    if let Some(previous) = cache.entries.remove(&path) {
        cache.retained_bytes = cache.retained_bytes.saturating_sub(previous.content.len());
    }
    cache.retained_bytes += entry.content.len();
    cache.entries.insert(path, entry);
    while cache.retained_bytes > MAX_CACHE_BYTES {
        let Some(path) = cache.entries.keys().next().cloned() else {
            break;
        };
        if let Some(entry) = cache.entries.remove(&path) {
            cache.retained_bytes = cache.retained_bytes.saturating_sub(entry.content.len());
        }
    }
}

fn insert_cache_if_current(
    cache: &mut CacheState,
    generation: u64,
    path: PathBuf,
    entry: CachedFile,
) -> bool {
    if cache.generation != generation {
        return false;
    }
    insert_cache(cache, path, entry);
    true
}

fn sort_results(results: &mut [SearchResult], sort: SearchSort) {
    results.sort_by(|left, right| {
        let ordering = match sort {
            SearchSort::Relevance => right.count.cmp(&left.count),
            SearchSort::Filename => left.path.file_name().cmp(&right.path.file_name()),
            SearchSort::Modified => right.modified.cmp(&left.modified),
            SearchSort::Created => right.created.cmp(&left.created),
        };
        ordering.then_with(|| left.relative_path.cmp(&right.relative_path))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rista-search-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn cap_is_applied_after_sorting() {
        let mut results = vec![
            SearchResult {
                path: "b.md".into(),
                relative_path: "b.md".into(),
                count: 1,
                first_line: None,
                modified: None,
                created: None,
            },
            SearchResult {
                path: "a.md".into(),
                relative_path: "a.md".into(),
                count: 4,
                first_line: None,
                modified: None,
                created: None,
            },
            SearchResult {
                path: "c.md".into(),
                relative_path: "c.md".into(),
                count: 2,
                first_line: None,
                modified: None,
                created: None,
            },
        ];
        sort_results(&mut results, SearchSort::Relevance);
        assert_eq!(results[0].relative_path, "a.md");
    }

    #[test]
    fn replace_literal_honours_case_and_counts() {
        assert_eq!(
            replace_literal("Needle needle NEEDLE", "needle", "thread", false),
            ("thread thread thread".to_string(), 3)
        );
        assert_eq!(
            replace_literal("Needle needle", "needle", "thread", true),
            ("Needle thread".to_string(), 1)
        );
        assert_eq!(
            replace_literal("nothing here", "needle", "thread", false),
            ("nothing here".to_string(), 0)
        );
        assert_eq!(
            replace_literal("abc", "", "x", false),
            ("abc".to_string(), 0)
        );
    }

    #[test]
    fn replace_literal_matches_whole_string_lowercasing() {
        // Agrees with search's `to_lowercase` semantics: 'ß' folds to itself,
        // so only the ASCII occurrence matches.
        assert_eq!(
            replace_literal("Straße STRASSE", "strasse", "x", false),
            ("Straße x".to_string(), 1)
        );
    }

    #[test]
    fn replace_literal_matches_context_sensitive_lowercasing() {
        // Rust's `to_lowercase` yields a final sigma: search matches `ος` in
        // `ΟΣ`, so replacement must too, and leave the non-matching `οσ` alone.
        assert_eq!(
            replace_literal("ΟΣ and οσ", "ος", "x", false),
            ("x and οσ".to_string(), 1)
        );
    }

    #[test]
    fn replace_literal_skips_matches_inside_a_multichar_fold() {
        // 'İ' folds to "i̇"; a bare "i" match inside it is not aligned to a
        // character boundary, so the text is left intact rather than corrupted.
        assert_eq!(
            replace_literal("İstanbul", "i", "x", false),
            ("İstanbul".to_string(), 0)
        );
        assert_eq!(replace_literal("İ", "İ", "x", false), ("x".to_string(), 1));
    }

    #[test]
    fn plan_replacements_collects_notes_and_skips_canvas() {
        let root = test_root();
        let note = root.join("note.md");
        let base = root.join("view.base");
        let board = root.join("board.canvas");
        fs::write(&note, "needle\nneedle\n").unwrap();
        fs::write(&base, "filters: needle\n").unwrap();
        fs::write(&board, r#"{"nodes":[{"text":"needle"}],"edges":[]}"#).unwrap();
        let paths = vec![note.clone(), base.clone(), board.clone()];
        let plan = SearchService::new().plan_replacements(&paths, "needle", "thread", false);
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.total, 3);
        assert!(plan.files.iter().all(|file| file.path != board));
        let note_file = plan.files.iter().find(|file| file.path == note).unwrap();
        assert_eq!(note_file.count, 2);
        assert_eq!(
            String::from_utf8(note_file.after.clone()).unwrap(),
            "thread\nthread\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_invalidation_purges_changed_paths() {
        let service = SearchService::new();
        let path = PathBuf::from("renamed.md");
        insert_cache(
            &mut service.cache.lock().unwrap(),
            path.clone(),
            CachedFile {
                modified: None,
                len: 0,
                content: Arc::from("cached"),
                properties: None,
                tags: None,
            },
        );
        service.invalidate([path.clone()]);
        let cache = service.cache.lock().unwrap();
        assert!(!cache.entries.contains_key(&path));
        assert_eq!(cache.retained_bytes, 0);
    }

    #[test]
    fn edits_and_renames_do_not_reuse_stale_content() {
        let root = test_root();
        let path = root.join("note.md");
        fs::write(&path, "before").unwrap();
        let service = SearchService::new();
        let paths = [path.clone()];
        assert_eq!(
            service
                .search(&root, &paths, "before", SearchOptions::default())
                .unwrap()
                .total,
            1
        );
        fs::write(&path, "after-edit").unwrap();
        assert_eq!(
            service
                .search(&root, &paths, "after-edit", SearchOptions::default())
                .unwrap()
                .total,
            1
        );

        let renamed = root.join("renamed.md");
        fs::rename(&path, &renamed).unwrap();
        let renamed_paths = [renamed.clone()];
        assert_eq!(
            service
                .search(
                    &root,
                    &renamed_paths,
                    "after-edit",
                    SearchOptions::default()
                )
                .unwrap()
                .total,
            1
        );
        let cache = service.cache.lock().unwrap();
        assert!(!cache.entries.contains_key(&path));
        assert!(cache.entries.contains_key(&renamed));
        assert_eq!(
            cache.retained_bytes,
            cache
                .entries
                .values()
                .map(|entry| entry.content.len())
                .sum::<usize>()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_generation_stops_before_scanning() {
        let generation = Arc::new(AtomicU64::new(2));
        let response = SearchService::new()
            .search_cancellable(
                Path::new("/missing"),
                &[PathBuf::from("/missing/note.md")],
                "needle",
                SearchOptions::default(),
                Some((generation, 1)),
            )
            .unwrap();
        assert!(response.results.is_empty());
        assert_eq!(response.total, 0);
    }

    #[test]
    fn canvas_search_uses_card_text_and_omits_source_first_line() {
        let root = test_root();
        let path = root.join("board.canvas");
        fs::write(
            &path,
            r#"{"unknown":"opaque-hidden-needle","nodes":[{"id":"t","type":"text","x":123456789,"y":-987654321,"width":400,"height":300,"text":"card-content-needle"}],"edges":[]}"#,
        )
        .unwrap();
        let service = SearchService::new();
        let paths = [path.clone()];
        let response = service
            .search(
                &root,
                &paths,
                "card-content-needle",
                SearchOptions::default(),
            )
            .unwrap();
        assert_eq!(response.total, 1);
        assert_eq!(response.results[0].first_line, None);
        for hidden in ["opaque-hidden-needle", "123456789", "987654321"] {
            assert_eq!(
                service
                    .search(&root, &paths, hidden, SearchOptions::default())
                    .unwrap()
                    .total,
                0,
                "Canvas search must not index {hidden}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalidation_prevents_old_generation_from_repopulating_cache() {
        let mut cache = CacheState {
            generation: 2,
            ..CacheState::default()
        };
        assert!(!insert_cache_if_current(
            &mut cache,
            1,
            PathBuf::from("note.md"),
            CachedFile {
                modified: None,
                len: 6,
                content: Arc::from("stale!"),
                properties: None,
                tags: None,
            },
        ));
        assert!(cache.entries.is_empty());
        assert_eq!(cache.retained_bytes, 0);
    }

    #[test]
    fn all_matches_are_sorted_before_display_cap() {
        let root = test_root();
        let paths = [
            ("a.md", "needle needle needle"),
            ("b.md", "needle"),
            ("c.md", "needle needle"),
        ]
        .into_iter()
        .map(|(name, content)| {
            let path = root.join(name);
            fs::write(&path, content).unwrap();
            path
        })
        .collect::<Vec<_>>();
        let response = SearchService::new()
            .search(
                &root,
                &paths,
                "needle",
                SearchOptions {
                    limit: 1,
                    ..SearchOptions::default()
                },
            )
            .unwrap();
        assert_eq!(response.total, 3);
        assert!(response.more);
        assert_eq!(response.results[0].relative_path, "a.md");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn plain_text_queries_defer_property_and_markdown_parsing() {
        let root = test_root();
        let path = root.join("note.md");
        fs::write(&path, "---\ntags: [inline]\n---\nneedle #body\n").unwrap();
        let service = SearchService::new();
        service
            .search(
                &root,
                std::slice::from_ref(&path),
                "needle",
                SearchOptions::default(),
            )
            .unwrap();
        let cached = service
            .cache
            .lock()
            .unwrap()
            .entries
            .get(&path)
            .unwrap()
            .clone();
        assert!(cached.properties.is_none());
        assert!(cached.tags.is_none());

        let response = service
            .search(
                &root,
                std::slice::from_ref(&path),
                "tag:inline",
                SearchOptions::default(),
            )
            .unwrap();
        assert_eq!(response.total, 1);
        let cached = service
            .cache
            .lock()
            .unwrap()
            .entries
            .get(&path)
            .unwrap()
            .clone();
        assert!(cached.properties.is_some());
        assert!(cached.tags.is_some());
        fs::remove_dir_all(root).unwrap();
    }
}
