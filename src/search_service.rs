use crate::properties;
use crate::search_query::{
    compile_query, evaluate_compiled, extract_tags, parse_query, requirements, EvaluationOptions,
    QueryError, QueryRequirements, SearchDocument,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
}

#[derive(Clone, Debug)]
struct CachedFile {
    modified: Option<SystemTime>,
    len: u64,
    content: Arc<str>,
    properties: Option<Vec<(String, serde_yaml::Value)>>,
    tags: Option<Vec<String>>,
}

#[derive(Clone, Default)]
pub struct SearchService {
    cache: Arc<Mutex<HashMap<PathBuf, CachedFile>>>,
}

impl SearchService {
    pub fn new() -> Self {
        Self::default()
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
        let query = parse_query(input)?;
        let requirements = requirements(&query);
        let compiled = compile_query(
            &query,
            EvaluationOptions {
                match_case: options.match_case,
            },
        )?;
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
                    ["md", "markdown", "base"]
                        .iter()
                        .any(|known| extension.eq_ignore_ascii_case(known))
                });
            let document = if requirements.read_content && is_text {
                self.document(path, &metadata, &relative_path, requirements)
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
                    first_line: evaluation.first_line,
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
        })
    }

    fn document(
        &self,
        path: &Path,
        metadata: &fs::Metadata,
        relative_path: &str,
        requirements: QueryRequirements,
    ) -> SearchDocument {
        let modified = metadata.modified().ok();
        let len = metadata.len();
        let cached = if modified.is_some() {
            self.cache
                .lock()
                .ok()
                .and_then(|cache| cache.get(path).cloned())
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
            let properties = if requirements.parse_properties {
                cached
                    .properties
                    .clone()
                    .unwrap_or_else(|| properties::properties(&cached.content))
            } else {
                Vec::new()
            };
            let tags = if requirements.extract_tags {
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
            if requirements.parse_properties && cached.properties.is_none() {
                cached.properties = Some(properties.clone());
            }
            if requirements.extract_tags && cached.tags.is_none() {
                cached.tags = Some(tags.clone());
            }
            if requirements.parse_properties || requirements.extract_tags {
                if let Ok(mut cache) = self.cache.lock() {
                    if let Some(entry) = cache.get_mut(path) {
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
        let properties = if requirements.parse_properties {
            Some(properties::properties(&content))
        } else {
            None
        };
        let tags = if requirements.extract_tags {
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
                cache.insert(
                    path.to_path_buf(),
                    CachedFile {
                        modified,
                        len,
                        content: Arc::from(content),
                        properties,
                        tags,
                    },
                );
                trim_cache(&mut cache);
            }
        }
        document
    }

    pub fn invalidate(&self, paths: impl IntoIterator<Item = PathBuf>) {
        if let Ok(mut cache) = self.cache.lock() {
            for path in paths {
                cache.remove(&path);
            }
        }
    }

    pub fn invalidate_all(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }
}

fn is_cancelled(cancellation: &Option<(Arc<AtomicU64>, u64)>) -> bool {
    cancellation
        .as_ref()
        .is_some_and(|(generation, expected)| generation.load(Ordering::Relaxed) != *expected)
}

fn trim_cache(cache: &mut HashMap<PathBuf, CachedFile>) {
    let mut bytes = cache
        .values()
        .map(|entry| entry.content.len())
        .sum::<usize>();
    while bytes > MAX_CACHE_BYTES {
        let Some(path) = cache.keys().next().cloned() else {
            break;
        };
        if let Some(entry) = cache.remove(&path) {
            bytes = bytes.saturating_sub(entry.content.len());
        }
    }
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
    fn cache_invalidation_purges_changed_paths() {
        let service = SearchService::new();
        let path = PathBuf::from("renamed.md");
        service.cache.lock().unwrap().insert(
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
        assert!(!service.cache.lock().unwrap().contains_key(&path));
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
        let cached = service.cache.lock().unwrap().get(&path).unwrap().clone();
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
        let cached = service.cache.lock().unwrap().get(&path).unwrap().clone();
        assert!(cached.properties.is_some());
        assert!(cached.tags.is_some());
        fs::remove_dir_all(root).unwrap();
    }
}
