//! Vault: an opened folder. Owns the file tree, the flattened note index,
//! and the filesystem watcher that keeps both honest.

use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState};
use gpui_kit::*;
use notify::{RecursiveMode, Watcher};
use regex::{Regex, RegexBuilder};

use crate::settings::{LinkFormat, TreeSort};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", "dist", ".build"];

/// Emitted after a filesystem burst settles — the tree was already refreshed.
pub enum VaultEvent {
    TreeExpansion,
    /// The file index was refreshed. `changed` carries the watcher's
    /// paths for incremental refreshes; `None` means a full rebuild, so
    /// consumers should treat every path as changed.
    Files {
        changed: Option<Vec<PathBuf>>,
    },
    /// Star/unstar flipped — `.base` `file.starred` rows recompute.
    Starred,
}

impl EventEmitter<VaultEvent> for Vault {}

pub struct Vault {
    pub root: Option<PathBuf>,
    pub tree: Entity<TreeState>,
    /// Flattened list of every `.md`, `.base`, and `.canvas` file in the vault,
    /// sorted.
    pub notes: Vec<PathBuf>,
    /// Image files indexed by lowercase file name — the reference editor resolves
    /// `![[name.png]]` vault-wide, so basename is the key.
    /// Shared behind one `Rc`: resolvers handed out to documents stay live
    /// and see files added later (paste/drop, watcher refreshes).
    pub images: Rc<RefCell<std::collections::HashMap<String, PathBuf>>>,
    attachments: Rc<RefCell<std::collections::HashMap<String, PathBuf>>>,
    /// `(tag, note_count)` pairs, rebuilt with the index — completions read
    /// this snapshot instead of re-parsing every note per keystroke.
    pub tags: Vec<(String, usize)>,
    /// Open `- [ ]` checkboxes vault-wide — the sidebar Tasks index.
    /// Rebuilt with the note index.
    pub tasks: Vec<crate::properties::VaultTask>,
    search_paths: Arc<Vec<PathBuf>>,
    /// Lowercase `aliases:` frontmatter values → the note declaring them,
    /// so `[[Alias]]` resolves. Rebuilt with the index.
    pub aliases: std::collections::HashMap<String, PathBuf>,
    /// `folder/note` (extensionless, lowercase) → note — the full-path
    /// branch of wikilink resolution without a linear scan.
    by_rel: std::collections::HashMap<String, PathBuf>,
    /// Lowercase file stem → note — basename resolution. First note wins
    /// on duplicates, matching the old `.find()` scan order.
    by_stem: std::collections::HashMap<String, PathBuf>,
    stem_counts: std::collections::HashMap<String, usize>,
    /// Parsed per-note content, keyed by stamp — a refresh re-reads only
    /// files whose stamp (mtime + size) changed. Feeds the tag, task,
    /// alias, and backlink indexes without touching disk.
    content_cache: std::collections::HashMap<PathBuf, (ContentStamp, NoteContent)>,
    /// Every indexed path — notes, images, attachments — for deciding
    /// whether a watcher event can refresh incrementally or needs a
    /// full rebuild.
    known_paths: std::collections::HashSet<PathBuf>,
    /// Bumped whenever the index content is rebuilt — cached derived
    /// views key off it.
    index_epoch: u64,
    /// Starred notes (absolute path strings) — mirrors
    /// `Settings::starred` so `.base` `file.starred` can read it
    /// without touching the workspace borrow.
    pub starred: std::collections::BTreeSet<String>,
    watcher: Option<notify::RecommendedWatcher>,
    pending_events: usize,
    /// Folder ids the user expanded — reapplied to rebuilt trees so
    /// watcher refreshes don't collapse the sidebar.
    expanded: std::collections::BTreeSet<String>,
    all_items: Vec<TreeItem>,
    pub explorer_query: String,
    pub explorer_filter: crate::explorer::FileFilter,
    /// File ordering inside each folder — dirs stay alphabetical.
    pub tree_sort: TreeSort,
    pub show_other_files: bool,
    pub link_format: LinkFormat,
    /// Templates folder relative to the root — its notes stay visible
    /// and linkable but their scaffolding (`{{cursor}}` tasks, tags,
    /// aliases) doesn't pollute the vault indexes. Mirrors
    /// `Settings::templates_dir`, set by the workspace on open.
    pub templates_dir: String,
    ignore_filters: IgnoreFilters,
    _tree_sub: Subscription,
}

#[derive(Clone, Default)]
struct IgnoreFilters {
    paths: Vec<String>,
    regexes: Vec<Regex>,
}

impl IgnoreFilters {
    fn from_settings(filters: &[String]) -> Self {
        let mut compiled = Self::default();
        for filter in filters.iter().take(1_000) {
            if let Some(pattern) = filter.strip_prefix('/').and_then(|s| s.strip_suffix('/')) {
                let mut builder = RegexBuilder::new(pattern);
                builder.size_limit(256 * 1024).dfa_size_limit(256 * 1024);
                if let Ok(regex) = builder.build() {
                    compiled.regexes.push(regex);
                }
            } else if !filter.is_empty()
                && filter.len() <= 4096
                && !filter.starts_with('/')
                && !filter.contains('\\')
                && !filter.contains('*')
                && !filter.contains('?')
                && !Path::new(filter).components().any(|component| {
                    matches!(
                        component,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                })
            {
                compiled.paths.push(filter.trim_matches('/').to_string());
            }
        }
        compiled
    }

    fn matches(&self, root: &Path, path: &Path) -> bool {
        let Some(relative) = path.strip_prefix(root).ok().and_then(Path::to_str) else {
            return false;
        };
        let relative = relative.replace('\\', "/");
        self.paths
            .iter()
            .any(|ignored| relative == *ignored || relative.starts_with(&format!("{ignored}/")))
            || self.regexes.iter().any(|regex| regex.is_match(&relative))
    }
}

impl Vault {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let tree_sub = cx.subscribe(&tree, |this, _tree, event, cx| {
            match event {
                TreeEvent::Expanded(id) => {
                    this.expanded.insert(id.to_string());
                }
                TreeEvent::Collapsed(id) => {
                    this.expanded.remove(id.as_str());
                }
            };
            cx.emit(VaultEvent::TreeExpansion);
        });
        Self {
            root: None,
            tree,
            notes: Vec::new(),
            images: Rc::new(RefCell::new(std::collections::HashMap::new())),
            attachments: Rc::new(RefCell::new(std::collections::HashMap::new())),
            watcher: None,
            pending_events: 0,
            tags: Vec::new(),
            tasks: Vec::new(),
            search_paths: Arc::new(Vec::new()),
            aliases: std::collections::HashMap::new(),
            by_rel: std::collections::HashMap::new(),
            by_stem: std::collections::HashMap::new(),
            stem_counts: std::collections::HashMap::new(),
            content_cache: std::collections::HashMap::new(),
            known_paths: std::collections::HashSet::new(),
            index_epoch: 0,
            starred: std::collections::BTreeSet::new(),
            expanded: Default::default(),
            all_items: Vec::new(),
            explorer_query: String::new(),
            explorer_filter: Default::default(),
            tree_sort: TreeSort::default(),
            show_other_files: false,
            link_format: LinkFormat::Shortest,
            templates_dir: "templates".to_string(),
            ignore_filters: IgnoreFilters::default(),
            _tree_sub: tree_sub,
        }
    }

    pub fn is_open(&self) -> bool {
        self.root.is_some()
    }

    pub(crate) fn set_ignore_filters(&mut self, filters: &[String]) {
        self.ignore_filters = IgnoreFilters::from_settings(filters);
    }

    pub(crate) fn set_link_format(&mut self, format: crate::settings::LinkFormat) {
        self.link_format = format;
    }

    pub fn open(&mut self, root: PathBuf, restored_expanded: Vec<String>, cx: &mut Context<Self>) {
        self.stop_watching();
        self.expanded = restored_expanded.into_iter().collect();
        self.explorer_query.clear();
        self.explorer_filter = Default::default();
        self.root = Some(root);
        self.refresh_tree(cx);
        self.start_watcher(cx);
        cx.emit(VaultEvent::Files { changed: None });
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.stop_watching();
        self.root = None;
        self.all_items.clear();
        self.search_paths = Arc::new(Vec::new());
        self.notes.clear();
        self.attachments.borrow_mut().clear();
        self.by_rel.clear();
        self.by_stem.clear();
        self.stem_counts.clear();
        self.aliases.clear();
        self.content_cache.clear();
        self.known_paths.clear();
        self.index_epoch = self.index_epoch.wrapping_add(1);
        self.tree
            .update(cx, |tree, cx| tree.set_items(Vec::new(), cx));
        cx.emit(VaultEvent::Files { changed: None });
        cx.notify();
    }

    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.all_items = build_items(
            &root,
            &root,
            0,
            self.tree_sort,
            self.show_other_files,
            &self.ignore_filters,
        );
        self.search_paths = Arc::new(crate::explorer::paths(&self.all_items));
        let items = mark_expanded(self.all_items.clone(), &self.expanded);
        let items = crate::explorer::filtered_tree(
            &items,
            &root,
            &self.explorer_query,
            self.explorer_filter,
        );
        let FileIndex {
            notes,
            images,
            attachments,
        } = collect_files(&root, &self.ignore_filters);
        // Template files are scaffolding, not notes — they stay in the
        // tree and resolve as links, but their tags/tasks/aliases don't
        // index (a `- [ ] {{cursor}}` placeholder is not a vault task).
        let tpl_root = root.join(&self.templates_dir);
        // Link-resolution indexes. Note content is cached per note by
        // stamp, so a refresh re-reads only files that changed and the
        // tag/task/alias/backlink indexes rebuild without disk reads.
        self.by_rel.clear();
        self.by_stem.clear();
        self.stem_counts.clear();
        for note in &notes {
            if let Some(rel) = note.strip_prefix(&root).ok().and_then(|rel| rel.to_str()) {
                self.by_rel
                    .insert(rel.trim_end_matches(".md").to_lowercase(), note.clone());
            }
            if let Some(stem) = note.file_stem().and_then(|s| s.to_str()) {
                let stem = stem.to_lowercase();
                *self.stem_counts.entry(stem.clone()).or_default() += 1;
                self.by_stem.entry(stem).or_insert_with(|| note.clone());
            }
        }
        let note_set: std::collections::HashSet<&PathBuf> = notes.iter().collect();
        self.content_cache.retain(|p, _| note_set.contains(p));
        for note in &notes {
            self.refresh_note_content(note, &tpl_root);
        }
        Self::aggregate_content(
            &notes,
            &self.content_cache,
            &mut self.tags,
            &mut self.tasks,
            &mut self.aliases,
        );
        self.known_paths = notes
            .iter()
            .cloned()
            .chain(images.values().cloned())
            .chain(attachments.values().cloned())
            .collect();
        self.index_epoch = self.index_epoch.wrapping_add(1);
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            // A scroll offset that outlives a shrunken entry list leaves the
            // tree rendering whitespace — snap back to the top in that case.
            let top = tree
                .scroll_handle()
                .0
                .borrow()
                .base_handle
                .logical_scroll_top()
                .0;
            if top > 0 && tree.entry(top).is_none() {
                tree.scroll_to_item(0, ScrollStrategy::Top);
            }
        });
        self.notes = notes;
        *self.images.borrow_mut() = images;
        *self.attachments.borrow_mut() = attachments;
        cx.notify();
    }

    /// Public refresh — called after our own writes so the index stays warm.
    pub fn expanded_folders(&self) -> Vec<String> {
        self.expanded.iter().cloned().collect()
    }

    pub(crate) fn restore_expanded_folders(
        &mut self,
        folders: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        self.expanded = folders.into_iter().collect();
        self.filter_tree(cx);
    }

    pub fn explorer_paths(&self) -> Vec<PathBuf> {
        self.search_paths.as_ref().clone()
    }

    pub fn explorer_path_snapshot(&self) -> Arc<Vec<PathBuf>> {
        self.search_paths.clone()
    }

    pub fn attachment_path_snapshot(&self) -> Arc<Vec<PathBuf>> {
        let mut paths: Vec<_> = self.attachments.borrow().values().cloned().collect();
        paths.sort();
        paths.dedup();
        Arc::new(paths)
    }

    pub fn filter_tree(&mut self, cx: &mut Context<Self>) {
        let Some(root) = &self.root else {
            return;
        };
        let items = mark_expanded(self.all_items.clone(), &self.expanded);
        let items = crate::explorer::filtered_tree(
            &items,
            root,
            &self.explorer_query,
            self.explorer_filter,
        );
        self.tree.update(cx, |tree, cx| tree.set_items(items, cx));
        cx.notify();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_tree(cx);
        cx.emit(VaultEvent::Files { changed: None });
    }

    /// Bumped whenever index content is rebuilt — cached derived views
    /// (backlink panes, mentions memos) key off it to know when to
    /// recompute.
    pub fn index_epoch(&self) -> u64 {
        self.index_epoch
    }

    /// Incremental refresh for a watcher burst that only touched
    /// existing files. Re-reads those notes and rebuilds the derived
    /// indexes from the content cache — no directory walk, and
    /// unchanged notes are never re-read.
    fn refresh_changed_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let tpl_root = root.join(&self.templates_dir);
        let mut content_changed = false;
        for path in paths {
            let is_md = matches!(
                crate::file_preview::extension(path).as_str(),
                "md" | "markdown"
            );
            if is_md || crate::canvas::is_canvas(path) {
                content_changed |= self.refresh_note_content(path, &tpl_root);
            }
        }
        if content_changed {
            Self::aggregate_content(
                &self.notes,
                &self.content_cache,
                &mut self.tags,
                &mut self.tasks,
                &mut self.aliases,
            );
            self.index_epoch = self.index_epoch.wrapping_add(1);
        }
        cx.emit(VaultEvent::Files {
            changed: Some(paths.to_vec()),
        });
        cx.notify();
    }

    /// Re-read one note's cached parse when its stamp changed. Returns
    /// whether the cache entry was replaced or dropped.
    fn refresh_note_content(&mut self, note: &Path, tpl_root: &Path) -> bool {
        let is_md = matches!(
            crate::file_preview::extension(note).as_str(),
            "md" | "markdown"
        );
        let is_canvas = crate::canvas::is_canvas(note);
        if note.starts_with(tpl_root) || (!is_md && !is_canvas) {
            return false;
        }
        let stamp = content_stamp(note);
        if let (Some(stamp), Some((cached, _))) = (stamp, self.content_cache.get(note)) {
            if *cached == stamp {
                return false;
            }
        }
        let Ok(text) = std::fs::read_to_string(note) else {
            return self.content_cache.remove(note).is_some();
        };
        let content = parse_note_content(&text, is_canvas);
        if let Some(stamp) = stamp {
            self.content_cache
                .insert(note.to_path_buf(), (stamp, content));
        }
        true
    }

    /// Rebuild the tag, task, and alias indexes from the content cache,
    /// in note order (aliases last-wins like the old per-note inserts).
    fn aggregate_content(
        notes: &[PathBuf],
        cache: &std::collections::HashMap<PathBuf, (ContentStamp, NoteContent)>,
        tags: &mut Vec<(String, usize)>,
        tasks: &mut Vec<crate::properties::VaultTask>,
        aliases: &mut std::collections::HashMap<String, PathBuf>,
    ) {
        let mut counts: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        tasks.clear();
        aliases.clear();
        for note in notes {
            let Some((_, content)) = cache.get(note) else {
                continue;
            };
            for tag in &content.tags {
                *counts.entry(tag.clone()).or_default() += 1;
            }
            for (line, text) in &content.tasks {
                tasks.push(crate::properties::VaultTask {
                    path: note.clone(),
                    line: *line,
                    text: text.clone(),
                });
            }
            for alias in &content.aliases {
                aliases.insert(alias.to_lowercase(), note.clone());
            }
        }
        *tags = counts.into_iter().collect();
    }

    fn start_watcher(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let (tx, rx) = smol::channel::unbounded::<notify::Event>();
        let result =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.send_blocking(event);
                }
            });
        let Ok(mut watcher) = result else {
            return;
        };
        if watcher.watch(&root, RecursiveMode::Recursive).is_err() {
            return;
        }
        self.watcher = Some(watcher);

        // Debounce: a burst of events collapses into one refresh. The
        // event paths decide between an incremental content refresh and
        // a full rebuild, so one edited note no longer re-walks the
        // vault or re-reads its neighbours.
        cx.spawn(async move |this: WeakEntity<Vault>, cx| {
            while let Ok(first) = rx.recv().await {
                // Let the dust settle — saves, renames and builds emit clusters.
                smol::Timer::after(std::time::Duration::from_millis(250)).await;
                // Drain any trailing events.
                let mut batch = vec![first];
                while let Ok(event) = rx.try_recv() {
                    batch.push(event);
                }
                let _ = this.update(&mut *cx, |this, cx| {
                    this.pending_events += 1;
                    this.apply_watcher_batch(&batch, cx);
                });
            }
        })
        .detach();
    }

    /// Route one debounced watcher batch: content-only changes refresh
    /// the touched notes, anything structural (create/remove/rename,
    /// folders, new files) rebuilds the tree and indexes.
    fn apply_watcher_batch(&mut self, batch: &[notify::Event], cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let mut paths: Vec<PathBuf> = Vec::new();
        let mut structural = false;
        for event in batch {
            // Pure reads don't change files.
            if matches!(event.kind, notify::EventKind::Access(_)) {
                continue;
            }
            // An event without paths can't be attributed — rebuild
            // rather than risk a stale index.
            if event.paths.is_empty() {
                structural = true;
                continue;
            }
            for path in &event.paths {
                if paths.contains(path) {
                    continue;
                }
                match classify_path(&root, path, &self.ignore_filters, &self.known_paths) {
                    PathChange::Ignore => {}
                    PathChange::Content => paths.push(path.clone()),
                    PathChange::Structural => {
                        structural = true;
                        paths.push(path.clone());
                    }
                }
            }
        }
        if paths.is_empty() && !structural {
            return;
        }
        if structural {
            self.refresh_tree(cx);
            cx.emit(VaultEvent::Files { changed: None });
        } else {
            self.refresh_changed_paths(&paths, cx);
        }
    }

    fn stop_watching(&mut self) {
        if let Some(mut watcher) = self.watcher.take() {
            if let Some(root) = self.root.clone() {
                let _ = watcher.unwatch(&root);
            }
        }
    }

    /// Resolve a wikilink target (`[[Note]]` or `[[folder/Note]]`) to a path.
    pub fn resolve_wikilink(&self, target: &str) -> Option<PathBuf> {
        let target = target
            .split('#')
            .next()
            .unwrap_or(target)
            .split('|')
            .next()
            .unwrap_or(target);
        let needle = target.trim_end_matches(".md").to_lowercase();
        if needle.is_empty() {
            return None;
        }
        // Full path match first, then basename match, then aliases —
        // O(1) lookups against the index built on the last refresh.
        self.by_rel
            .get(&needle)
            .or_else(|| self.by_stem.get(&needle))
            .or_else(|| self.aliases.get(&needle))
            .cloned()
    }

    pub fn resolve_wikilink_from(&self, target: &str, source: &Path) -> Option<PathBuf> {
        let raw = target.split(['#', '|']).next().unwrap_or(target).trim();
        if raw.starts_with("./") || raw.starts_with("../") {
            return self.resolve_relative_wikilink(raw, source.parent()?);
        }
        self.resolve_wikilink(raw)
    }

    fn resolve_relative_wikilink(&self, target: &str, from_dir: &Path) -> Option<PathBuf> {
        resolve_relative_note(self.root.as_deref()?, from_dir, target)
    }

    pub fn link_target_for(&self, target: &Path, source: &Path) -> Option<String> {
        let root = self.root.as_deref()?;
        let target_rel = without_markdown_extension(target.strip_prefix(root).ok()?);
        let target_text = target_rel.to_string_lossy().replace('\\', "/");
        match self.link_format {
            LinkFormat::Absolute => Some(target_text),
            LinkFormat::Relative => relative_link_path_for(root, source, target),
            LinkFormat::Shortest => {
                let stem = target.file_stem()?.to_string_lossy().to_string();
                let count = self
                    .stem_counts
                    .get(&stem.to_lowercase())
                    .copied()
                    .unwrap_or(0);
                Some(if count <= 1 { stem } else { target_text })
            }
        }
    }

    /// Every note containing a `[[wikilink]]` that resolves to `target`.
    /// Answers from the content cache — no disk reads, so this is cheap
    /// enough for render paths.
    pub fn backlinks_to(&self, target: &Path) -> Vec<PathBuf> {
        let resolve = |link: &str, note: &Path| self.resolve_wikilink_from(link, note);
        let canvas = |note: &Path, content: &NoteContent| {
            let Some((references, text_targets)) = &content.canvas_refs else {
                return Vec::new();
            };
            let from_dir = note.parent().unwrap_or(Path::new(""));
            self.canvas_outgoing_targets(references, text_targets, from_dir)
                .0
        };
        backlinks_from_cache(&self.notes, &self.content_cache, target, &resolve, &canvas)
    }

    /// First line of `note` containing a `[[link]]` that resolves to
    /// `target` — the context snippet shown beside a backlink row.
    pub fn backlink_context(&self, note: &Path, target: &Path) -> Option<String> {
        let text = std::fs::read_to_string(note).ok()?;
        if crate::canvas::is_canvas(note) {
            let canvas = crate::canvas::CanvasDocument::parse(&text).ok()?;
            let from_dir = note.parent().unwrap_or(Path::new(""));
            for node in canvas.nodes() {
                if let Some(reference) = node["file"].as_str() {
                    if self.root.as_deref().is_some_and(|root| {
                        reference
                            .split(['#', '^'])
                            .next()
                            .and_then(|path| crate::canvas::resolve_vault_path(root, path).ok())
                            .as_deref()
                            == Some(target)
                    }) {
                        return Some(node["title"].as_str().unwrap_or(reference).to_string());
                    }
                }
                if let Some(background) = node["background"].as_str() {
                    if self.root.as_deref().is_some_and(|root| {
                        crate::canvas::resolve_vault_path(root, background)
                            .ok()
                            .as_deref()
                            == Some(target)
                    }) {
                        return Some(background.to_string());
                    }
                }
                if node["type"].as_str() == Some("text") {
                    if let Some(content) = node["text"].as_str() {
                        let (resolved, _) =
                            self.outgoing_targets(&local_link_targets(content), from_dir);
                        if resolved.contains(&target.to_path_buf()) {
                            let excerpt = content
                                .lines()
                                .find(|line| !line.trim().is_empty())
                                .unwrap_or(content)
                                .trim();
                            let excerpt = excerpt.chars().take(96).collect::<String>();
                            return Some(excerpt);
                        }
                    }
                }
            }
            return None;
        }
        for line in text.lines() {
            let mut cursor = 0;
            while let Some(at) = line[cursor..].find("[[") {
                let start = cursor + at + 2;
                let Some(end) = line[start..].find("]]") else {
                    break;
                };
                if self
                    .resolve_wikilink_from(&line[start..start + end], note)
                    .as_deref()
                    == Some(target)
                {
                    return Some(line.trim().to_string());
                }
                cursor = start + end + 2;
            }
        }
        None
    }

    /// Line containing the first plain-text `needle` mention in `note`
    /// — the context snippet shown beside an unlinked-mention row.
    /// `needle` must already be lowercase (see `unlinked_mentions`).
    pub fn unlinked_context(&self, note: &Path, needle: &str) -> Option<String> {
        let text = std::fs::read_to_string(note).ok()?;
        let at = plain_mention_offset(&text, needle)?;
        let start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let end = text[at..].find('\n').map(|i| at + i).unwrap_or(text.len());
        Some(text[start..end].trim().to_string())
    }

    /// Notes whose body mentions the target's file stem as plain
    /// text outside `[[...]]` — the "unlinked mentions".
    /// Whole-phrase, case-insensitive; linked mentions don't count.
    pub fn unlinked_mentions(&self, target: &Path) -> Vec<PathBuf> {
        let Some(stem) = target.file_stem().and_then(|s| s.to_str()) else {
            return Vec::new();
        };
        let needle = stem.to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for note in &self.notes {
            if *note == target || note.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(note) else {
                continue;
            };
            if plain_mention_offset(&text, &needle).is_some() {
                out.push(note.clone());
            }
        }
        out
    }

    /// Byte ranges of the inner `target` text of every `[[wikilink]]` /
    /// `![[embed]]` in `text` that resolves to `target` — used by
    /// link-safe rename to rewrite the span in place.
    pub fn link_spans_to(
        &self,
        text: &str,
        from_path: &Path,
        target: &Path,
    ) -> Vec<std::ops::Range<usize>> {
        let mut out = Vec::new();
        let mut cursor = 0;
        while let Some(at) = text[cursor..].find("[[").map(|i| cursor + i) {
            let Some(end) = text[at + 2..].find("]]").map(|i| at + 2 + i) else {
                break;
            };
            if self
                .resolve_link_target_from(&text[at + 2..end], from_path)
                .as_deref()
                == Some(target)
            {
                out.push((at + 2)..end);
            }
            cursor = end + 2;
        }
        out
    }

    /// Everything `text` links at: `[[wiki]]`, `![[embed]]` and
    /// `[label](target)` forms. `(resolved, unresolved)` — resolved
    /// paths deduped in order, unresolvable targets as display strings.
    /// `from_dir` is the note's folder for relative md links.
    pub fn outgoing_from(&self, text: &str, from_dir: &Path) -> (Vec<PathBuf>, Vec<String>) {
        self.outgoing_targets(&local_link_targets(text), from_dir)
    }

    /// `outgoing_from` over already-parsed `local_link_targets` — lets
    /// callers cache the markdown parse and only redo the cheap
    /// resolution.
    pub fn outgoing_targets(
        &self,
        targets: &[(String, bool)],
        from_dir: &Path,
    ) -> (Vec<PathBuf>, Vec<String>) {
        let mut resolved: Vec<PathBuf> = Vec::new();
        let mut unresolved: Vec<String> = Vec::new();
        for (target, wiki) in targets {
            match if *wiki {
                if target.starts_with("./") || target.starts_with("../") {
                    self.resolve_relative_wikilink(target, from_dir)
                } else {
                    self.resolve_link_target(target)
                }
            } else {
                self.md_link_path(target, from_dir)
            } {
                Some(p) => {
                    if !resolved.contains(&p) {
                        resolved.push(p);
                    }
                }
                None => {
                    if !target.is_empty() && !unresolved.contains(target) {
                        unresolved.push(target.clone());
                    }
                }
            }
        }
        (resolved, unresolved)
    }

    pub(crate) fn canvas_outgoing_targets(
        &self,
        file_references: &[String],
        text_targets: &[(String, bool)],
        from_dir: &Path,
    ) -> (Vec<PathBuf>, Vec<String>) {
        let mut resolved = Vec::new();
        let mut unresolved = Vec::new();
        if let Some(root) = &self.root {
            for reference in file_references {
                let target = reference
                    .split('#')
                    .next()
                    .unwrap_or(reference)
                    .split('^')
                    .next()
                    .unwrap_or(reference);
                match crate::canvas::resolve_vault_path(root, target) {
                    Ok(path) if path.is_file() => {
                        if !resolved.contains(&path) {
                            resolved.push(path);
                        }
                    }
                    _ if !unresolved.contains(reference) => unresolved.push(reference.clone()),
                    _ => {}
                }
            }
        } else {
            unresolved.extend(file_references.iter().cloned());
        }
        let (text_resolved, text_unresolved) = self.outgoing_targets(text_targets, from_dir);
        for path in text_resolved {
            if !resolved.contains(&path) {
                resolved.push(path);
            }
        }
        for target in text_unresolved {
            if !unresolved.contains(&target) {
                unresolved.push(target);
            }
        }
        (resolved, unresolved)
    }

    /// Where a markdown link's inner `target` points: `<>`-unwrap,
    /// `%`-decode, drop `#anchor`/`?query`; relative to `from_dir`
    /// first, then the vault root. External URLs and pure anchors
    /// return `None`.
    pub(crate) fn md_link_path(&self, inner: &str, from_dir: &Path) -> Option<PathBuf> {
        let inner = inner.trim();
        let inner = inner
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(inner);
        if inner.contains("://") || inner.starts_with('#') || inner.starts_with("mailto:") {
            return None;
        }
        let raw = inner
            .split('#')
            .next()
            .unwrap_or(inner)
            .split('?')
            .next()
            .unwrap_or(inner);
        if raw.is_empty() {
            return None;
        }
        let decoded = percent_decode(raw);
        let candidate = from_dir.join(&decoded);
        if candidate.is_file() {
            return Some(candidate);
        }
        let candidate = self.root.as_deref()?.join(&decoded);
        candidate.is_file().then_some(candidate)
    }

    /// Note links and attachments, including files hidden from the sidebar.
    pub(crate) fn resolve_link_target(&self, target: &str) -> Option<PathBuf> {
        self.resolve_wikilink(target).or_else(|| {
            let name = target
                .split(['#', '|'])
                .next()
                .unwrap_or(target)
                .trim()
                .to_lowercase();
            self.attachments.borrow().get(&name).cloned()
        })
    }

    fn resolve_link_target_from(&self, target: &str, source: &Path) -> Option<PathBuf> {
        self.resolve_wikilink_from(target, source).or_else(|| {
            if target.starts_with("./") || target.starts_with("../") {
                let root = self.root.as_deref()?.canonicalize().ok()?;
                let parent = source.parent()?.canonicalize().ok()?;
                let candidate = parent.join(target.split(['#', '|']).next().unwrap_or(target));
                let candidate = candidate.canonicalize().ok()?;
                if candidate.starts_with(&root) && candidate.is_file() {
                    return Some(candidate);
                }
            }
            let name = target
                .split(['#', '|'])
                .next()
                .unwrap_or(target)
                .trim()
                .to_lowercase();
            self.attachments.borrow().get(&name).cloned()
        })
    }

    /// Byte ranges of the inner `target` text of every `[label](target)`
    /// / `![alt](target)` link in `text` pointing at `target`. The target
    /// resolves relative to `from_dir` (the containing note's dir) first,
    /// then the vault root; `<>`-wrapped and `%`-escaped forms handled.
    pub fn md_link_spans_to(
        &self,
        text: &str,
        from_dir: &Path,
        target: &Path,
    ) -> Vec<std::ops::Range<usize>> {
        let mut out = Vec::new();
        let mut cursor = 0;
        while let Some(at) = text[cursor..].find("](").map(|i| cursor + i) {
            let Some(end) = text[at + 2..].find(')').map(|i| at + 2 + i) else {
                break;
            };
            if self.md_link_resolves(&text[at + 2..end], from_dir, target) {
                out.push((at + 2)..end);
            }
            cursor = end + 1;
        }
        out
    }

    /// Does a markdown link's inner `target` text resolve to `path`?
    fn md_link_resolves(&self, inner: &str, from_dir: &Path, path: &Path) -> bool {
        let inner = inner.trim();
        let inner = inner
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(inner);
        if inner.contains("://") || inner.starts_with('#') || inner.starts_with("mailto:") {
            return false;
        }
        let raw = inner
            .split('#')
            .next()
            .unwrap_or(inner)
            .split('?')
            .next()
            .unwrap_or(inner);
        let decoded = percent_decode(raw);
        let root = self.root.as_deref().unwrap_or(Path::new(""));
        let want = normalize_path(path);
        [from_dir.join(&decoded), root.join(&decoded)]
            .iter()
            .any(|p| normalize_path(p) == want)
    }

    /// Resolve image embeds by basename or vault-relative path.
    pub fn image_resolver(&self) -> crate::document::ImageResolver {
        Self::image_resolver_for(Rc::clone(&self.attachments))
    }

    fn image_resolver_for(
        attachments: Rc<RefCell<std::collections::HashMap<String, PathBuf>>>,
    ) -> crate::document::ImageResolver {
        Rc::new(move |name: &str| {
            attachments
                .borrow()
                .get(&name.to_lowercase())
                .filter(|path| is_image_file(path))
                .cloned()
        })
    }
}

/// Sorted, non-overlapping `(byte range, replacement)` edits into a
/// note's text — produced by `link_spans_to` + `retarget_link`.
pub type TextEdits = Vec<(std::ops::Range<usize>, String)>;

pub(crate) fn non_overlapping_text_edits(mut edits: TextEdits) -> TextEdits {
    edits.sort_by_key(|(range, _)| range.start);
    let mut non_overlapping: TextEdits = Vec::with_capacity(edits.len());
    for edit in edits {
        if non_overlapping
            .last()
            .is_some_and(|(previous, _)| previous.end > edit.0.start)
        {
            continue;
        }
        non_overlapping.push(edit);
    }
    non_overlapping
}

/// The new inner text for a `[[...]]`/`![[...]]` whose target was renamed
/// to `new_name` — swaps the last path segment only, keeping any
/// `#anchor` and `|alias` suffix and the target's directory prefix.
/// `new_name` is a stem for notes, a filename for other files.
pub fn retarget_link(inner: &str, new_name: &str) -> String {
    let (pre, rest) = match inner.find(['#', '|']) {
        Some(i) => inner.split_at(i),
        None => (inner, ""),
    };
    let pre = pre.trim();
    let (prefix, base) = match pre.rfind('/') {
        Some(i) => (&pre[..i + 1], &pre[i + 1..]),
        None => ("", pre),
    };
    if base.trim_end_matches(".md").is_empty() {
        return inner.to_string();
    }
    format!("{prefix}{new_name}{rest}")
}

/// Like `retarget_link` but for a file that MOVED directories:
/// `new_rel` is the target's vault-root-relative path (no `.md`).
/// Dir-prefixed `[[notes/a]]` gets the new folder (`[[docs/b]]`);
/// a bare `[[a]]` stays bare (`[[b]]`) since it resolves vault-wide.
pub fn retarget_link_full(inner: &str, new_rel: &str) -> String {
    let (pre, rest) = match inner.find(['#', '|']) {
        Some(i) => inner.split_at(i),
        None => (inner, ""),
    };
    let pre = pre.trim();
    let has_dir = pre.contains('/');
    if pre.trim_end_matches(".md").ends_with('/') || pre.is_empty() {
        return inner.to_string();
    }
    let target = if has_dir {
        new_rel.to_string()
    } else {
        new_rel.rsplit('/').next().unwrap_or(new_rel).to_string()
    };
    format!("{target}{rest}")
}

pub fn retarget_link_path(inner: &str, new_path: &str) -> String {
    let (pre, rest) = match inner.find(['#', '|']) {
        Some(i) => inner.split_at(i),
        None => (inner, ""),
    };
    if pre.trim().is_empty() {
        return inner.to_string();
    }
    format!("{new_path}{rest}")
}

pub fn is_relative_link_path(inner: &str) -> bool {
    let trimmed = inner.trim();
    let path = trimmed
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(trimmed);
    path.starts_with("./") || path.starts_with("../")
}

/// The new inner text for a `[label](target)` link whose target was
/// renamed — swaps the last path segment, keeps `#anchor`/`?query`
/// suffixes, and preserves the original style: plain, `<>`-wrapped, or
/// `%`-escaped. A new name containing a space wraps in `<>` unless the
/// original was percent-escaped.
pub fn retarget_md_link(inner: &str, new_filename: &str) -> String {
    let trimmed = inner.trim();
    let wrapped = trimmed.starts_with('<');
    let body = trimmed
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(trimmed);
    let (path, rest) = match body.find(['#', '?']) {
        Some(i) => body.split_at(i),
        None => (body, ""),
    };
    let (prefix, base) = match path.rfind('/') {
        Some(i) => (&path[..i + 1], &path[i + 1..]),
        None => ("", path),
    };
    if base.is_empty() {
        return inner.to_string();
    }
    let escaped = base.contains('%');
    let seg = if escaped {
        new_filename.replace(' ', "%20")
    } else {
        new_filename.to_string()
    };
    let body = format!("{prefix}{seg}{rest}");
    if wrapped || (!escaped && new_filename.contains(' ')) {
        format!("<{body}>")
    } else {
        body
    }
}

/// Like `retarget_md_link` but for a file that MOVED directories:
/// replaces the whole path body with `new_rel` (vault-root-relative,
/// extension included) so `[x](notes/a.md)` → `[x](docs/b.md)`.
pub fn retarget_md_link_full(inner: &str, new_rel: &str) -> String {
    let trimmed = inner.trim();
    let wrapped = trimmed.starts_with('<');
    let body = trimmed
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(trimmed);
    let rest = match body.find(['#', '?']) {
        Some(i) => &body[i..],
        None => "",
    };
    if body.is_empty() {
        return inner.to_string();
    }
    let escaped = new_rel.contains('%') || body.contains('%');
    let seg = if escaped {
        new_rel.replace(' ', "%20")
    } else {
        new_rel.to_string()
    };
    let body = format!("{seg}{rest}");
    if wrapped || (!escaped && new_rel.contains(' ')) {
        format!("<{body}>")
    } else {
        body
    }
}

pub fn retarget_md_link_path(inner: &str, new_path: &str) -> String {
    let trimmed = inner.trim();
    let wrapped = trimmed.starts_with('<');
    let body = trimmed
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(trimmed);
    let rest = match body.find(['#', '?']) {
        Some(i) => &body[i..],
        None => "",
    };
    let escaped = body.contains('%');
    let path = if escaped {
        new_path.replace(' ', "%20")
    } else {
        new_path.to_string()
    };
    let body = format!("{path}{rest}");
    if wrapped || (!escaped && new_path.contains(' ')) {
        format!("<{body}>")
    } else {
        body
    }
}

fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn relative_link_path_for(root: &Path, source: &Path, target: &Path) -> Option<String> {
    relative_path_for(root, source, target, true)
}

pub fn relative_markdown_link_path_for(
    root: &Path,
    source: &Path,
    target: &Path,
) -> Option<String> {
    relative_path_for(root, source, target, false)
}

fn relative_path_for(
    root: &Path,
    source: &Path,
    target: &Path,
    strip_markdown_extension: bool,
) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let source = canonicalize_with_missing_tail(&normalize_path(source.parent()?))?;
    let target = canonicalize_with_missing_tail(&normalize_path(target))?;
    (source.starts_with(&root) && target.starts_with(&root))
        .then(|| relative_link_path_from_dirs(&root, &source, &target, strip_markdown_extension))?
}

fn canonicalize_with_missing_tail(path: &Path) -> Option<PathBuf> {
    let mut existing = path;
    let mut missing = Vec::new();
    while std::fs::symlink_metadata(existing).is_err() {
        missing.push(existing.file_name()?.to_os_string());
        existing = existing.parent()?;
    }
    let mut resolved = existing.canonicalize().ok()?;
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    Some(normalize_path(&resolved))
}

fn relative_link_path_from_dirs(
    root: &Path,
    source: &Path,
    target: &Path,
    strip_markdown_extension: bool,
) -> Option<String> {
    let source = source.strip_prefix(root).ok()?;
    let target = target.strip_prefix(root).ok()?;
    let target = if strip_markdown_extension {
        without_markdown_extension(target)
    } else {
        target.to_path_buf()
    };
    let source: Vec<_> = source
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    let target: Vec<_> = target
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    let common = source
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec![".."; source.len() - common];
    parts.extend(target[common..].iter().copied());
    let path = parts.join("/");
    Some(if path.starts_with("../") {
        path
    } else {
        format!("./{path}")
    })
}

pub fn relative_link_edits_for_move(
    root: &Path,
    old_source: &Path,
    new_source: &Path,
    text: &str,
) -> TextEdits {
    let Some(old_source_canonical) = old_source.canonicalize().ok() else {
        return Vec::new();
    };
    let Some(new_source_canonical) = canonicalize_with_missing_tail(&normalize_path(new_source))
    else {
        return Vec::new();
    };
    let Some(old_source_dir) = old_source.parent() else {
        return Vec::new();
    };
    let mut edits = Vec::new();

    let mut cursor = 0;
    while let Some(at) = text[cursor..].find("[[").map(|offset| cursor + offset) {
        let Some(end) = text[at + 2..].find("]]").map(|offset| at + 2 + offset) else {
            break;
        };
        let range = (at + 2)..end;
        let inner = &text[range.clone()];
        if is_relative_link_path(inner) {
            let path = inner
                .trim()
                .strip_prefix('<')
                .and_then(|value| value.strip_suffix('>'))
                .unwrap_or(inner.trim())
                .split(['#', '|'])
                .next()
                .unwrap_or("");
            if let Some(target) = resolve_relative_note(root, old_source_dir, path) {
                let target = if target == old_source_canonical {
                    new_source_canonical.clone()
                } else {
                    target
                };
                if let Some(relative) = relative_link_path_for(root, new_source, &target) {
                    let replacement = retarget_link_path(inner, &relative);
                    if replacement != inner {
                        edits.push((range, replacement));
                    }
                }
            }
        }
        cursor = end + 2;
    }

    let mut cursor = 0;
    while let Some(at) = text[cursor..].find("](").map(|offset| cursor + offset) {
        let Some(end) = text[at + 2..].find(')').map(|offset| at + 2 + offset) else {
            break;
        };
        let range = (at + 2)..end;
        let inner = &text[range.clone()];
        if is_relative_link_path(inner) {
            if let Some(target) = resolve_relative_markdown_path(root, old_source_dir, inner) {
                let target = if target == old_source_canonical {
                    new_source_canonical.clone()
                } else {
                    target
                };
                if let Some(relative) = relative_markdown_link_path_for(root, new_source, &target) {
                    let replacement = retarget_md_link_path(inner, &relative);
                    if replacement != inner {
                        edits.push((range, replacement));
                    }
                }
            }
        }
        cursor = end + 1;
    }

    non_overlapping_text_edits(edits)
}

fn resolve_relative_markdown_path(root: &Path, from_dir: &Path, inner: &str) -> Option<PathBuf> {
    let inner = inner.trim();
    let inner = inner
        .strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .unwrap_or(inner);
    let raw = inner
        .split('#')
        .next()
        .unwrap_or(inner)
        .split('?')
        .next()
        .unwrap_or(inner);
    let root = root.canonicalize().ok()?;
    let from_dir = from_dir.canonicalize().ok()?;
    let target = from_dir.join(percent_decode(raw)).canonicalize().ok()?;
    (from_dir.starts_with(&root) && target.starts_with(&root) && target.is_file()).then_some(target)
}

fn without_markdown_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        let value = path.to_string_lossy();
        PathBuf::from(value.strip_suffix(".md").unwrap_or(&value))
    } else {
        path.to_path_buf()
    }
}

fn resolve_relative_note(root: &Path, from_dir: &Path, target: &str) -> Option<PathBuf> {
    let target = target.split(['#', '|']).next()?.trim();
    let root = root.canonicalize().ok()?;
    let from_dir = from_dir.canonicalize().ok()?;
    if !from_dir.starts_with(&root) {
        return None;
    }
    let mut candidate = from_dir.join(target);
    if !candidate
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".md"))
    {
        candidate = PathBuf::from(format!("{}.md", candidate.to_string_lossy()));
    }
    let candidate = candidate.canonicalize().ok()?;
    (candidate.starts_with(&root)
        && candidate.is_file()
        && candidate.extension().is_some_and(|ext| ext == "md"))
    .then_some(candidate)
}

pub(crate) fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn should_skip(entry: &std::fs::DirEntry) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    name.starts_with('.') || (entry.path().is_dir() && SKIP_DIRS.iter().any(|d| name == *d))
}

fn build_items(
    root: &Path,
    dir: &Path,
    depth: usize,
    sort: TreeSort,
    show_other_files: bool,
    ignore_filters: &IgnoreFilters,
) -> Vec<TreeItem> {
    if depth > 12 {
        return Vec::new();
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in read.flatten() {
        if should_skip(&entry) || entry.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
            continue;
        }
        let path = entry.path();
        if ignore_filters.matches(root, &path) {
            continue;
        }
        let label = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            dirs.push(
                TreeItem::new(path.to_string_lossy().to_string(), label).children(build_items(
                    root,
                    &path,
                    depth + 1,
                    sort,
                    show_other_files,
                    ignore_filters,
                )),
            );
        } else if path.is_file() && crate::file_preview::visible(&path, show_other_files) {
            files.push(TreeItem::new(path.to_string_lossy().to_string(), label));
        }
    }
    dirs.sort_by(|a, b| a.label.cmp(&b.label));
    match sort {
        TreeSort::Size => files.sort_by_key(|f| {
            std::cmp::Reverse(
                std::fs::metadata(f.id.as_str())
                    .map(|m| m.len())
                    .unwrap_or(0),
            )
        }),
        TreeSort::Name => files.sort_by(|a, b| a.label.cmp(&b.label)),
        TreeSort::Type => files.sort_by_key(|f| {
            (
                crate::file_preview::extension(Path::new(f.id.as_str())),
                f.label.to_lowercase(),
                f.id.clone(),
            )
        }),
        TreeSort::Modified => files.sort_by_key(|f| {
            std::cmp::Reverse(
                std::fs::metadata(f.id.as_str())
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH),
            )
        }),
    }
    dirs.extend(files);
    dirs
}

/// Re-mark folders expanded after a rebuild — `set_items` drops the
/// previous `TreeItem`s and their expansion flags with them.
fn mark_expanded(
    items: Vec<TreeItem>,
    expanded: &std::collections::BTreeSet<String>,
) -> Vec<TreeItem> {
    items
        .into_iter()
        .map(|item| {
            let is_expanded = expanded.contains(item.id.as_str());
            let mut item = item.expanded(is_expanded);
            item.children = mark_expanded(std::mem::take(&mut item.children), expanded);
            item
        })
        .collect()
}

pub(crate) const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "avif", "bmp"];

/// Whether the path's extension is one of the vault's image types.
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Byte offset of the first whole-phrase, case-insensitive
/// occurrence of `needle` outside `[[...]]` spans — shared by
/// unlinked mentions and the link-up action. `needle` must already
/// be lowercase.
pub(crate) fn plain_mention_offset(text: &str, needle: &str) -> Option<usize> {
    // Everything is scanned in lowercase space so positions are
    // comparable; the returned offset is verified against `text`.
    let hay = text.to_lowercase();
    let mut spans = Vec::new();
    let mut cur = 0;
    while let Some(at) = hay[cur..].find("[[") {
        let start = cur + at;
        match hay[start + 2..].find("]]") {
            Some(e) => {
                spans.push(start..(start + 2 + e + 2));
                cur = start + 2 + e + 2;
            }
            None => break,
        }
    }
    let boundary = |c: Option<char>| c.map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false);
    let mut cur = 0;
    while let Some(at) = hay[cur..].find(needle) {
        let start = cur + at;
        let end = start + needle.len();
        let in_link = spans.iter().any(|r| start >= r.start && start < r.end);
        let before = hay[..start].chars().next_back();
        let after = hay[end..].chars().next();
        if !in_link && !boundary(before) && !boundary(after) {
            // Lowercasing can shift byte offsets for exotic Unicode;
            // verify the slice is really the needle before splicing.
            return text
                .get(start..end)
                .filter(|s| s.to_lowercase() == needle)
                .map(|_| start);
        }
        cur = end;
    }
    None
}

/// `(modified, len)` — the content cache's invalidation stamp. Size
/// backs up mtime so a same-timestamp write that changed length still
/// invalidates.
type ContentStamp = (std::time::SystemTime, u64);

fn content_stamp(path: &Path) -> Option<ContentStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Canvas link sources: `(file references, text targets)` — parsed
/// once per change for the backlink scan.
type CanvasRefs = (Vec<String>, Vec<(String, bool)>);

/// Parsed per-note content the vault reuses across refreshes.
#[derive(Default)]
struct NoteContent {
    aliases: Vec<String>,
    /// Deduplicated bare tags (frontmatter + inline).
    tags: Vec<String>,
    /// Open tasks as `(1-based line, text)`.
    tasks: Vec<(usize, String)>,
    /// `[[wikilink]]` target strings in body order — backlinks resolve
    /// against these instead of re-reading the note.
    wiki_targets: Vec<String>,
    /// Canvas `(file references, text targets)`; `Some` only for canvas
    /// documents so backlinks can branch without re-parsing JSON.
    canvas_refs: Option<CanvasRefs>,
}

fn parse_note_content(text: &str, is_canvas: bool) -> NoteContent {
    let mut content = NoteContent::default();
    if is_canvas {
        content.canvas_refs = Some(match crate::canvas::CanvasDocument::parse(text) {
            Ok(canvas) => (canvas.vault_file_references(), canvas.text_link_targets()),
            Err(_) => (Vec::new(), Vec::new()),
        });
    } else {
        content.aliases = crate::properties::frontmatter_aliases(text);
        content.tags = crate::properties::note_tags(text);
        content.tasks = crate::properties::open_task_lines(text);
        content.wiki_targets = wiki_targets(text);
    }
    content
}

/// Cache the same Markdown-aware wiki targets used by outgoing links.
/// Parsing happens once per changed note, not once per backlink query.
fn wiki_targets(text: &str) -> Vec<String> {
    local_link_targets(text)
        .into_iter()
        .filter_map(|(target, wiki)| wiki.then_some(target))
        .collect()
}

/// How one watcher path affects the index.
enum PathChange {
    /// Hidden, ignored, or irrelevant — no index work.
    Ignore,
    /// An indexed file's content changed — incremental refresh.
    Content,
    /// The file set changed (create, remove, rename, folder) — the
    /// tree and indexes need a full rebuild.
    Structural,
}

fn classify_path(
    root: &Path,
    path: &Path,
    ignore_filters: &IgnoreFilters,
    known: &std::collections::HashSet<PathBuf>,
) -> PathChange {
    let Ok(relative) = path.strip_prefix(root) else {
        return PathChange::Ignore;
    };
    // Hidden entries (including `.rista` history) and build folders
    // never enter the tree or indexes.
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return PathChange::Ignore;
        };
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP_DIRS.iter().any(|skip| name == *skip) {
            return PathChange::Ignore;
        }
    }
    if ignore_filters.matches(root, path) {
        return PathChange::Ignore;
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => PathChange::Structural,
        Ok(_) if known.contains(path) => PathChange::Content,
        Ok(_) => PathChange::Structural,
        // Directories aren't in `known`, and a removal event may name
        // only the directory. Rebuild conservatively for missing paths.
        Err(_) => PathChange::Structural,
    }
}

/// Notes whose cached content links to `target`. Wiki resolution and
/// canvas handling come in as closures so the scan is testable without
/// a vault.
fn backlinks_from_cache(
    notes: &[PathBuf],
    cache: &std::collections::HashMap<PathBuf, (ContentStamp, NoteContent)>,
    target: &Path,
    resolve_wiki: &dyn Fn(&str, &Path) -> Option<PathBuf>,
    canvas_targets: &dyn Fn(&Path, &NoteContent) -> Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut links = Vec::new();
    for note in notes {
        if note == target {
            continue;
        }
        let Some((_, content)) = cache.get(note) else {
            continue;
        };
        let linked = if content.canvas_refs.is_some() {
            canvas_targets(note, content)
                .iter()
                .any(|path| path == target)
        } else {
            content
                .wiki_targets
                .iter()
                .any(|link| resolve_wiki(link, note).as_deref() == Some(target))
        };
        if linked {
            links.push(note.clone());
        }
    }
    links
}

pub(crate) fn local_link_targets(text: &str) -> Vec<(String, bool)> {
    use markdown::mdast::Node;
    fn definitions(node: &Node, out: &mut std::collections::HashMap<String, String>) {
        if let Node::Definition(def) = node {
            out.entry(def.identifier.clone())
                .or_insert_with(|| def.url.clone());
        }
        for child in node.children().into_iter().flatten() {
            definitions(child, out);
        }
    }
    fn walk(
        node: &Node,
        defs: &std::collections::HashMap<String, String>,
        out: &mut Vec<(String, bool)>,
    ) {
        let url = match node {
            Node::Link(link) => Some(&link.url),
            Node::Image(image) => Some(&image.url),
            Node::LinkReference(link) => defs.get(&link.identifier),
            Node::ImageReference(image) => defs.get(&image.identifier),
            _ => None,
        };
        if let Some(url) = url.filter(|url| {
            !url.is_empty() && !url.starts_with('#') && !url.starts_with("//") && !url.contains(':')
        }) {
            out.push((url.clone(), false));
        }
        if let Node::Text(text) = node {
            let mut rest = text.value.as_str();
            while let Some((_, tail)) = rest.split_once("[[") {
                let Some((target, tail)) = tail.split_once("]]") else {
                    break;
                };
                let target = target.split('|').next().unwrap_or("").trim();
                if !target.is_empty() && !target.starts_with('#') {
                    out.push((target.to_string(), true));
                }
                rest = tail;
            }
        }
        for child in node.children().into_iter().flatten() {
            walk(child, defs, out);
        }
    }
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    let Ok(root) = markdown::to_mdast(text, &options) else {
        return Vec::new();
    };
    let mut defs = std::collections::HashMap::new();
    definitions(&root, &mut defs);
    let mut out = Vec::new();
    walk(&root, &defs, &mut out);
    out
}

#[cfg(test)]
mod content_index_tests {
    use super::{
        backlinks_from_cache, classify_path, content_stamp, parse_note_content, wiki_targets,
        ContentStamp, IgnoreFilters, NoteContent, PathChange, Vault,
    };
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rista-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn stamp() -> ContentStamp {
        (SystemTime::UNIX_EPOCH, 0)
    }

    #[test]
    fn wiki_targets_skip_metadata_fences_and_inline_code() {
        let text = "---\nrelated: \"[[Frontmatter]]\"\n---\n\
            [[Alpha|alias]] and ![[Beta#Head]] and [[#section]] and `[[Code]]`\n\
            \n```\n[[Fenced]]\n```\n[[Gamma#h]] and [[Delta|d#h]]\n";
        assert_eq!(
            wiki_targets(text),
            vec!["Alpha", "Beta#Head", "Gamma#h", "Delta"]
        );
    }

    #[test]
    fn parse_note_content_collects_tags_tasks_aliases_and_wiki_links() {
        let text = "---\ntags: [a, b]\naliases: [Zeta]\n---\n- [ ] open\n- [x] done\n#inline and [[Link]]\n";
        let content = parse_note_content(text, false);
        assert_eq!(
            content.tags,
            vec!["a".to_string(), "b".to_string(), "inline".to_string()]
        );
        assert_eq!(content.aliases, vec!["Zeta".to_string()]);
        assert_eq!(content.tasks, vec![(5, "open".to_string())]);
        assert_eq!(content.wiki_targets, vec!["Link".to_string()]);
        assert!(content.canvas_refs.is_none());
    }

    #[test]
    fn canvas_content_records_references_without_note_indexes() {
        let text = r#"{"nodes":[{"type":"file","file":"Note.md"}],"edges":[]}"#;
        let content = parse_note_content(text, true);
        assert!(content.canvas_refs.is_some());
        assert!(content.tags.is_empty());
        assert!(content.aliases.is_empty());
        assert!(content.tasks.is_empty());
    }

    #[test]
    fn aggregate_content_keeps_note_order_and_last_alias_wins() {
        let notes = vec![PathBuf::from("/v/a.md"), PathBuf::from("/v/b.md")];
        let mut cache: HashMap<PathBuf, (ContentStamp, NoteContent)> = HashMap::new();
        let a = NoteContent {
            tags: vec!["x".to_string()],
            tasks: vec![(2, "one".to_string())],
            aliases: vec!["Ali".to_string()],
            ..NoteContent::default()
        };
        let b = NoteContent {
            tags: vec!["x".to_string(), "y".to_string()],
            tasks: vec![(1, "two".to_string())],
            aliases: vec!["ali".to_string()],
            ..NoteContent::default()
        };
        cache.insert(notes[0].clone(), (stamp(), a));
        cache.insert(notes[1].clone(), (stamp(), b));

        let mut tags = Vec::new();
        let mut tasks = Vec::new();
        let mut aliases = HashMap::new();
        Vault::aggregate_content(&notes, &cache, &mut tags, &mut tasks, &mut aliases);

        assert_eq!(tags, vec![("x".to_string(), 2), ("y".to_string(), 1)]);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].path, notes[0]);
        assert_eq!(tasks[0].line, 2);
        assert_eq!(tasks[1].path, notes[1]);
        assert_eq!(
            aliases.get("ali"),
            Some(&notes[1].clone()),
            "later notes win the case-folded alias"
        );
    }

    #[test]
    fn backlinks_from_cache_resolves_wiki_targets_and_canvases() {
        let target = PathBuf::from("/v/Target.md");
        let notes = vec![
            PathBuf::from("/v/a.md"),
            PathBuf::from("/v/b.md"),
            PathBuf::from("/v/c.canvas"),
        ];
        let mut cache: HashMap<PathBuf, (ContentStamp, NoteContent)> = HashMap::new();
        let a = NoteContent {
            wiki_targets: vec!["Target".to_string()],
            ..NoteContent::default()
        };
        let b = NoteContent {
            wiki_targets: vec!["Other".to_string()],
            ..NoteContent::default()
        };
        let c = NoteContent {
            canvas_refs: Some((vec!["Target.md".to_string()], Vec::new())),
            ..NoteContent::default()
        };
        cache.insert(notes[0].clone(), (stamp(), a));
        cache.insert(notes[1].clone(), (stamp(), b));
        cache.insert(notes[2].clone(), (stamp(), c));

        let expected = target.clone();
        let resolve = move |link: &str, _note: &Path| (link == "Target").then(|| expected.clone());
        let canvas = |_note: &Path, content: &NoteContent| {
            content
                .canvas_refs
                .as_ref()
                .map(|(references, _)| {
                    references
                        .iter()
                        .map(|reference| PathBuf::from("/v").join(reference))
                        .collect()
                })
                .unwrap_or_default()
        };
        assert_eq!(
            backlinks_from_cache(&notes, &cache, &target, &resolve, &canvas),
            vec![notes[0].clone(), notes[2].clone()]
        );
    }

    #[test]
    fn classify_path_separates_hidden_content_and_structural_events() {
        let root = temp_root("classify");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join("private")).unwrap();
        let known_file = root.join("note.md");
        std::fs::write(&known_file, "body").unwrap();
        let new_file = root.join("new.md");
        std::fs::write(&new_file, "body").unwrap();
        let mut known = HashSet::new();
        known.insert(known_file.clone());

        let filters = IgnoreFilters::from_settings(&["private".to_string()]);
        {
            let classify = |path: &Path| classify_path(&root, path, &filters, &known);
            assert!(matches!(
                classify(&root.join(".git/config")),
                PathChange::Ignore
            ));
            assert!(matches!(
                classify(&root.join("node_modules/x.js")),
                PathChange::Ignore
            ));
            assert!(matches!(
                classify(&root.join("private/note.md")),
                PathChange::Ignore
            ));
            assert!(matches!(classify(&known_file), PathChange::Content));
            assert!(matches!(classify(&new_file), PathChange::Structural));
            assert!(matches!(
                classify(&root.join("sub")),
                PathChange::Structural
            ));
            assert!(matches!(
                classify(&root.join("gone.md")),
                PathChange::Structural
            ));
        }
        known.insert(root.join("gone.md"));
        assert!(matches!(
            classify_path(&root, &root.join("gone.md"), &filters, &known),
            PathChange::Structural
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn content_stamp_tracks_writes_and_missing_files() {
        let root = temp_root("stamp");
        let file = root.join("note.md");
        std::fs::write(&file, "one").unwrap();
        let first = content_stamp(&file).unwrap();
        std::fs::write(&file, "two-and-longer").unwrap();
        assert_ne!(first, content_stamp(&file).unwrap());
        std::fs::remove_file(&file).unwrap();
        assert_eq!(content_stamp(&file), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn removed_directories_require_a_tree_rebuild() {
        let root = temp_root("removed-directories");
        let empty = root.join("empty");
        let populated = root.join("populated");
        std::fs::create_dir(&empty).unwrap();
        std::fs::create_dir(&populated).unwrap();
        let note = populated.join("note.md");
        std::fs::write(&note, "body").unwrap();
        let known = HashSet::from([note]);
        std::fs::remove_dir(&empty).unwrap();
        std::fs::remove_dir_all(&populated).unwrap();
        for directory in [&empty, &populated] {
            assert!(matches!(
                classify_path(&root, directory, &IgnoreFilters::default(), &known),
                PathChange::Structural
            ));
        }
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn cached_backlinks_preserve_markdown_delimiter_semantics() {
        for source in [
            "``[[Code]]`` [[Real]]",
            "literal ` then [[Real]]",
            "```outer\n[[Code]]\n````\n[[Real]]",
            "---\n\n[[Real]]",
            "    [[Code]]\n\n[[Real]]",
        ] {
            assert_eq!(wiki_targets(source), vec!["Real"], "{source:?}");
        }
    }
}

#[cfg(test)]
mod link_diagnostics_tests {
    use std::path::PathBuf;

    #[test]
    fn relative_link_targets_round_trip_for_unicode_paths() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);

        let root = std::env::temp_dir().join(format!(
            "rista-relative-links-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let source_dir = root.join("source/深い");
        let target_dir = root.join("target");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let source = source_dir.join("source.md");
        let target = target_dir.join("Å v1.2.md");
        std::fs::write(&source, "").unwrap();
        std::fs::write(&target, "").unwrap();

        assert_eq!(
            super::relative_link_path_for(&root, &source, &target).as_deref(),
            Some("../../target/Å v1.2")
        );
        assert_eq!(
            super::resolve_relative_note(&root, &source_dir, "../../target/Å v1.2"),
            Some(target.canonicalize().unwrap())
        );
        for reference in [
            "../../target/Å v1.2#section",
            "../../target/Å v1.2#^block|Label",
        ] {
            assert_eq!(
                super::resolve_relative_note(&root, &source_dir, reference),
                Some(target.canonicalize().unwrap())
            );
        }
        assert_eq!(
            super::retarget_link_path("../old/Å v1.2|alias#heading", "../../target/Å v1.2"),
            "../../target/Å v1.2|alias#heading"
        );
        assert!(super::is_relative_link_path(
            "<../target/Å v1.2.md#heading>"
        ));
        assert!(!super::is_relative_link_path("target/Å v1.2.md"));
        assert!(!super::is_relative_link_path("/target/Å v1.2.md"));
        assert_eq!(
            super::relative_link_path_for(&root, &source, &root.join("target/new.v1.2.md"),)
                .as_deref(),
            Some("../../target/new.v1.2")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moved_notes_keep_relative_wiki_and_markdown_targets() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);

        let root = std::env::temp_dir().join(format!(
            "rista-relative-rename-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let old_dir = root.join("old");
        let target_dir = root.join("target");
        let incoming_dir = root.join("incoming");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::create_dir_all(&incoming_dir).unwrap();
        let old_source = old_dir.join("source.md");
        let new_source = root.join("new/deep/source.md");
        let target = target_dir.join("Å v1.2.md");
        let incoming = incoming_dir.join("linker.md");
        std::fs::write(&old_source, "").unwrap();
        std::fs::write(&target, "").unwrap();
        std::fs::write(&incoming, "").unwrap();

        let mut text = concat!(
            "[[../target/Å v1.2#section|alias]]\n",
            "[target](../target/Å%20v1.2.md#section)\n",
            "[[./source|self]]\n",
            "[self](./source.md)\n"
        )
        .to_string();
        let edits = super::relative_link_edits_for_move(&root, &old_source, &new_source, &text);
        for (range, replacement) in edits.into_iter().rev() {
            text.replace_range(range, &replacement);
        }
        assert!(text.contains("[[../../target/Å v1.2#section|alias]]"));
        assert!(text.contains("[target](../../target/Å%20v1.2.md#section)"));
        assert!(text.contains("[[./source|self]]"));
        assert!(text.contains("[self](./source.md)"));

        std::fs::create_dir_all(new_source.parent().unwrap()).unwrap();
        std::fs::write(&new_source, "").unwrap();
        let old_incoming = "../old/source.md#heading";
        assert_eq!(
            super::resolve_relative_markdown_path(&root, incoming.parent().unwrap(), old_incoming,),
            Some(old_source.canonicalize().unwrap())
        );
        std::fs::remove_file(&old_source).unwrap();
        assert_eq!(
            super::resolve_relative_note(
                &root,
                new_source.parent().unwrap(),
                "../../target/Å v1.2"
            ),
            Some(target.canonicalize().unwrap())
        );
        assert_eq!(
            super::resolve_relative_markdown_path(
                &root,
                new_source.parent().unwrap(),
                "../../target/Å%20v1.2.md#section",
            ),
            Some(target.canonicalize().unwrap())
        );
        assert_eq!(
            super::resolve_relative_markdown_path(
                &root,
                new_source.parent().unwrap(),
                "./source.md",
            ),
            Some(new_source.canonicalize().unwrap())
        );

        let incoming_target =
            super::relative_markdown_link_path_for(&root, &incoming, &new_source).unwrap();
        assert!(incoming_target.ends_with(".md"));
        let incoming_retargeted = super::retarget_md_link_path(old_incoming, &incoming_target);
        assert!(incoming_retargeted.ends_with(".md#heading"));
        assert_eq!(
            super::resolve_relative_markdown_path(
                &root,
                incoming.parent().unwrap(),
                &incoming_retargeted,
            ),
            Some(new_source.canonicalize().unwrap())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn attachment_index_includes_non_images_and_drops_removed_files() {
        let root = std::env::temp_dir().join(format!(
            "rista-attachment-index-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("attachments")).unwrap();
        let report = root.join("attachments/QA-report.txt");
        let image = root.join("attachments/image.png");
        let note = root.join("note.md");
        std::fs::write(&report, "Report").unwrap();
        std::fs::write(&image, b"image fixture").unwrap();
        std::fs::write(&note, "[[QA-report.txt]]").unwrap();
        let index = super::collect_files(&root, &super::IgnoreFilters::default());
        assert_eq!(index.attachments.get("qa-report.txt"), Some(&report));
        assert_eq!(
            index.attachments.get("attachments/qa-report.txt"),
            Some(&report)
        );
        assert_eq!(index.attachments.get("image.png"), Some(&image));
        assert_eq!(index.attachments.get("attachments/image.png"), Some(&image));
        assert_eq!(index.images.get("image.png"), Some(&image));
        let resolver = super::Vault::image_resolver_for(std::rc::Rc::new(std::cell::RefCell::new(
            index.attachments.clone(),
        )));
        assert_eq!(resolver("attachments/image.png"), Some(image.clone()));
        assert!(!index.images.contains_key("qa-report.txt"));
        assert_eq!(index.notes, vec![note]);
        std::fs::remove_file(&report).unwrap();
        let refreshed = super::collect_files(&root, &super::IgnoreFilters::default());
        assert!(!refreshed.attachments.contains_key("qa-report.txt"));
        assert!(!refreshed
            .attachments
            .contains_key("attachments/qa-report.txt"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn image_resolver_supports_live_vault_paths_without_basename_collisions() {
        let first = PathBuf::from("/vault/Assets/Grid.png");
        let second = PathBuf::from("/vault/Other/Grid.png");
        let attachments =
            std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::from([
                ("grid.png".to_string(), first.clone()),
                ("assets/grid.png".to_string(), first.clone()),
                ("other/grid.png".to_string(), second.clone()),
                ("report.txt".to_string(), PathBuf::from("/vault/report.txt")),
            ])));
        let resolver = super::Vault::image_resolver_for(attachments.clone());
        assert_eq!(resolver("Grid.png"), Some(first.clone()));
        assert_eq!(resolver("Assets/Grid.png"), Some(first));
        assert_eq!(resolver("Other/Grid.png"), Some(second.clone()));
        assert!(resolver("Missing/Grid.png").is_none());
        assert!(resolver("../Assets/Grid.png").is_none());
        assert!(resolver("report.txt").is_none());

        attachments.borrow_mut().remove("other/grid.png");
        attachments
            .borrow_mut()
            .insert("new/grid.png".to_string(), second.clone());
        assert!(resolver("Other/Grid.png").is_none());
        assert_eq!(resolver("New/Grid.png"), Some(second));
        attachments.borrow_mut().clear();
        assert!(resolver("Grid.png").is_none());
    }

    #[test]
    fn links_and_images_ignore_code_frontmatter_urls_and_anchors() {
        let source = "---\nexample: '[[metadata]]'\n---\n[[Note|Label]] ![[missing.png]] [file](missing.md) ![photo][pic]\n\n[pic]: photo.png\n\n`[[inline]]`\n~~~\n[[code]]\n~~~\n[web](https://example.com) [mail](mailto:a@b.com) [anchor](#here) [[#Heading]]";
        assert_eq!(
            super::local_link_targets(source),
            vec![
                ("Note".into(), true),
                ("missing.png".into(), true),
                ("missing.md".into(), false),
                ("photo.png".into(), false)
            ]
        );
    }

    #[test]
    fn ignore_filters_hide_paths_from_tree_index_and_search_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "rista-ignore-index-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("private")).unwrap();
        let visible = root.join("visible.md");
        let hidden = root.join("private/hidden.md");
        std::fs::write(&visible, "visible").unwrap();
        std::fs::write(&hidden, "hidden").unwrap();
        let filters = super::IgnoreFilters::from_settings(&[
            "private".to_string(),
            "/visible\\.md$/".to_string(),
        ]);
        let index = super::collect_files(&root, &filters);
        assert!(index.notes.is_empty());
        let visible_only = super::IgnoreFilters::from_settings(&["private".to_string()]);
        let index = super::collect_files(&root, &visible_only);
        assert_eq!(index.notes, vec![visible]);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Default)]
struct FileIndex {
    notes: Vec<PathBuf>,
    images: std::collections::HashMap<String, PathBuf>,
    attachments: std::collections::HashMap<String, PathBuf>,
}

fn collect_files(root: &Path, ignore_filters: &IgnoreFilters) -> FileIndex {
    let mut files = FileIndex::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            if should_skip(&entry) {
                continue;
            }
            let path = entry.path();
            if ignore_filters.matches(root, &path) {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());
            match ext.as_deref() {
                Some("md") | Some("base") | Some("canvas") => files.notes.push(path),
                _ => {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        files
                            .attachments
                            .entry(name.to_lowercase())
                            .or_insert_with(|| path.clone());
                        if ext.as_deref().is_some_and(|ext| IMAGE_EXTS.contains(&ext)) {
                            files
                                .images
                                .entry(name.to_lowercase())
                                .or_insert_with(|| path.clone());
                        }
                    }
                    if let Some(relative) = path.strip_prefix(root).ok().and_then(|p| p.to_str()) {
                        files.attachments.insert(
                            relative
                                .replace(std::path::MAIN_SEPARATOR, "/")
                                .to_lowercase(),
                            path,
                        );
                    }
                }
            }
        }
    }
    files.notes.sort();
    files
}
