//! Vault: an opened folder. Owns the file tree, the flattened note index,
//! and the filesystem watcher that keeps both honest.

use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState};
use gpui_kit::*;
use notify::{RecursiveMode, Watcher};

use chrono::NaiveDate;

use crate::settings::TreeSort;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", "dist", ".build"];

/// Emitted after a filesystem burst settles — the tree was already refreshed.
pub enum VaultEvent {
    TreeExpansionChanged,
    FilesChanged,
    /// Star/unstar flipped — `.base` `file.starred` rows recompute.
    StarredChanged,
}

impl EventEmitter<VaultEvent> for Vault {}

pub struct Vault {
    pub root: Option<PathBuf>,
    pub tree: Entity<TreeState>,
    /// Flattened list of every `.md` and `.base` file in the vault,
    /// sorted.
    pub notes: Vec<PathBuf>,
    /// Image files indexed by lowercase file name — the reference editor resolves
    /// `![[name.png]]` vault-wide, so basename is the key.
    /// Shared behind one `Rc`: resolvers handed out to documents stay live
    /// and see files added later (paste/drop, watcher refreshes).
    pub images: Rc<RefCell<std::collections::HashMap<String, PathBuf>>>,
    /// `(tag, note_count)` pairs, rebuilt with the index — completions read
    /// this snapshot instead of re-parsing every note per keystroke.
    pub tags: Vec<(String, usize)>,
    /// Open `- [ ]` checkboxes vault-wide — the sidebar Tasks index.
    /// Rebuilt with the note index.
    pub tasks: Vec<crate::properties::VaultTask>,
    /// Lowercase `aliases:` frontmatter values → the note declaring them,
    /// so `[[Alias]]` resolves. Rebuilt with the index.
    pub aliases: std::collections::HashMap<String, PathBuf>,
    /// `folder/note` (extensionless, lowercase) → note — the full-path
    /// branch of wikilink resolution without a linear scan.
    by_rel: std::collections::HashMap<String, PathBuf>,
    /// Lowercase file stem → note — basename resolution. First note wins
    /// on duplicates, matching the old `.find()` scan order.
    by_stem: std::collections::HashMap<String, PathBuf>,
    /// Per-note mtime → parsed `aliases:` values. Lets a refresh reuse
    /// frontmatter without re-reading every file's body.
    alias_cache: std::collections::HashMap<PathBuf, (std::time::SystemTime, Vec<String>)>,
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
    /// Templates folder relative to the root — its notes stay visible
    /// and linkable but their scaffolding (`{{cursor}}` tasks, tags,
    /// aliases) doesn't pollute the vault indexes. Mirrors
    /// `Settings::templates_dir`, set by the workspace on open.
    pub templates_dir: String,
    _tree_sub: Subscription,
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
            cx.emit(VaultEvent::TreeExpansionChanged);
        });
        Self {
            root: None,
            tree,
            notes: Vec::new(),
            images: Rc::new(RefCell::new(std::collections::HashMap::new())),
            watcher: None,
            pending_events: 0,
            tags: Vec::new(),
            tasks: Vec::new(),
            aliases: std::collections::HashMap::new(),
            by_rel: std::collections::HashMap::new(),
            by_stem: std::collections::HashMap::new(),
            alias_cache: std::collections::HashMap::new(),
            starred: std::collections::BTreeSet::new(),
            expanded: Default::default(),
            all_items: Vec::new(),
            explorer_query: String::new(),
            explorer_filter: Default::default(),
            tree_sort: TreeSort::default(),
            show_other_files: false,
            templates_dir: "templates".to_string(),
            _tree_sub: tree_sub,
        }
    }

    pub fn is_open(&self) -> bool {
        self.root.is_some()
    }

    pub fn open(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.stop_watching();
        self.expanded = crate::settings::Settings::load()
            .expanded_folders
            .get(&root.to_string_lossy().to_string())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        self.explorer_query.clear();
        self.explorer_filter = Default::default();
        self.root = Some(root);
        self.refresh_tree(cx);
        self.start_watcher(cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.stop_watching();
        self.root = None;
        self.all_items.clear();
        self.notes.clear();
        self.by_rel.clear();
        self.by_stem.clear();
        self.aliases.clear();
        self.tree
            .update(cx, |tree, cx| tree.set_items(Vec::new(), cx));
        cx.notify();
    }

    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.all_items = build_items(&root, 0, self.tree_sort, self.show_other_files);
        let items = mark_expanded(self.all_items.clone(), &self.expanded);
        let items = crate::explorer::filtered_tree(
            &items,
            &root,
            &self.explorer_query,
            self.explorer_filter,
        );
        let (notes, images) = collect_files(&root);
        // Template files are scaffolding, not notes — they stay in the
        // tree and resolve as links, but their tags/tasks/aliases don't
        // index (a `- [ ] {{cursor}}` placeholder is not a vault task).
        let tpl_root = root.join(&self.templates_dir);
        let content: Vec<PathBuf> = notes
            .iter()
            .filter(|p| !p.starts_with(&tpl_root))
            .cloned()
            .collect();
        self.tags = crate::properties::vault_tags(&content);
        self.tasks = crate::properties::vault_tasks(&content);
        // Link-resolution indexes + frontmatter aliases. Aliases are
        // cached per note by mtime so a watcher refresh only re-reads
        // files that actually changed instead of the whole vault.
        self.by_rel.clear();
        self.by_stem.clear();
        self.aliases.clear();
        let note_set: std::collections::HashSet<&PathBuf> = notes.iter().collect();
        self.alias_cache.retain(|p, _| note_set.contains(p));
        for note in &notes {
            let in_templates = note.starts_with(&tpl_root);
            if let Some(rel) = note.strip_prefix(&root).ok().and_then(|rel| rel.to_str()) {
                self.by_rel
                    .insert(rel.trim_end_matches(".md").to_lowercase(), note.clone());
            }
            if let Some(stem) = note.file_stem().and_then(|s| s.to_str()) {
                self.by_stem
                    .entry(stem.to_lowercase())
                    .or_insert_with(|| note.clone());
            }
            // `aliases:` only live in markdown frontmatter — `.base`
            // files and templates get indexed above but skip the read.
            if in_templates || note.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let mtime = std::fs::metadata(note).and_then(|m| m.modified()).ok();
            let aliases = match (mtime, self.alias_cache.get(note)) {
                (Some(mt), Some((cached_mt, cached))) if *cached_mt == mt => cached.clone(),
                _ => {
                    let parsed = std::fs::read_to_string(note)
                        .ok()
                        .map(|text| crate::properties::frontmatter_aliases(&text))
                        .unwrap_or_default();
                    if let Some(mt) = mtime {
                        self.alias_cache.insert(note.clone(), (mt, parsed.clone()));
                    }
                    parsed
                }
            };
            for alias in aliases {
                self.aliases.insert(alias.to_lowercase(), note.clone());
            }
        }
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
        cx.notify();
    }

    /// Public refresh — called after our own writes so the index stays warm.
    pub fn expanded_folders(&self) -> Vec<String> {
        self.expanded.iter().cloned().collect()
    }

    pub fn explorer_paths(&self) -> Vec<PathBuf> {
        crate::explorer::paths(&self.all_items)
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
    }

    fn start_watcher(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let (tx, rx) = smol::channel::unbounded::<()>();
        let result =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if res.is_ok() {
                    let _ = tx.send_blocking(());
                }
            });
        let Ok(mut watcher) = result else {
            return;
        };
        if watcher.watch(&root, RecursiveMode::Recursive).is_err() {
            return;
        }
        self.watcher = Some(watcher);

        // Debounce: a burst of events collapses into one refresh.
        cx.spawn(async move |this: WeakEntity<Vault>, cx| {
            while rx.recv().await.is_ok() {
                // Let the dust settle — saves, renames and builds emit clusters.
                smol::Timer::after(std::time::Duration::from_millis(250)).await;
                // Drain any trailing events.
                while rx.try_recv().is_ok() {}
                let _ = this.update(&mut *cx, |this, cx| {
                    this.pending_events += 1;
                    this.refresh_tree(cx);
                    cx.emit(VaultEvent::FilesChanged);
                });
            }
        })
        .detach();
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

    /// Every note containing a `[[wikilink]]` that resolves to `target`.
    /// Scans note bodies — O(vault); callers should cache the result.
    pub fn backlinks_to(&self, target: &Path) -> Vec<PathBuf> {
        let mut links = Vec::new();
        for note in &self.notes {
            if *note == target || note.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(note) else {
                continue;
            };
            let mut cursor = 0;
            while let Some(at) = text[cursor..].find("[[") {
                let start = cursor + at + 2;
                let Some(end) = text[start..].find("]]") else {
                    break;
                };
                if self.resolve_wikilink(&text[start..start + end]).as_deref() == Some(target) {
                    links.push(note.clone());
                    break;
                }
                cursor = start + end + 2;
            }
        }
        links
    }

    /// First line of `note` containing a `[[link]]` that resolves to
    /// `target` — the context snippet shown beside a backlink row.
    pub fn backlink_context(&self, note: &Path, target: &Path) -> Option<String> {
        let text = std::fs::read_to_string(note).ok()?;
        for line in text.lines() {
            let mut cursor = 0;
            while let Some(at) = line[cursor..].find("[[") {
                let start = cursor + at + 2;
                let Some(end) = line[start..].find("]]") else {
                    break;
                };
                if self.resolve_wikilink(&line[start..start + end]).as_deref() == Some(target) {
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
    pub fn link_spans_to(&self, text: &str, target: &Path) -> Vec<std::ops::Range<usize>> {
        let mut out = Vec::new();
        let mut cursor = 0;
        while let Some(at) = text[cursor..].find("[[").map(|i| cursor + i) {
            let Some(end) = text[at + 2..].find("]]").map(|i| at + 2 + i) else {
                break;
            };
            if self.resolve_link_target(&text[at + 2..end]).as_deref() == Some(target) {
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
        let mut resolved: Vec<PathBuf> = Vec::new();
        let mut unresolved: Vec<String> = Vec::new();
        for (target, wiki) in local_link_targets(text) {
            match if wiki {
                self.resolve_link_target(&target)
            } else {
                self.md_link_path(&target, from_dir)
            } {
                Some(p) => {
                    if !resolved.contains(&p) {
                        resolved.push(p);
                    }
                }
                None => {
                    if !target.is_empty() && !unresolved.contains(&target) {
                        unresolved.push(target);
                    }
                }
            }
        }
        (resolved, unresolved)
    }

    /// Where a markdown link's inner `target` points: `<>`-unwrap,
    /// `%`-decode, drop `#anchor`/`?query`; relative to `from_dir`
    /// first, then the vault root. External URLs and pure anchors
    /// return `None`.
    fn md_link_path(&self, inner: &str, from_dir: &Path) -> Option<PathBuf> {
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

    /// `resolve_wikilink` plus embedded-file targets (`![[image.png]]`),
    /// which resolve by basename against the image index.
    fn resolve_link_target(&self, target: &str) -> Option<PathBuf> {
        self.resolve_wikilink(target).or_else(|| {
            let name = target
                .split(['#', '|'])
                .next()
                .unwrap_or(target)
                .trim()
                .to_lowercase();
            self.images.borrow().get(&name).cloned()
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

    /// The daily-note path for `date` — `<daily_dir>/<chrono_fmt>.md`.
    /// `daily_dir` is vault-relative ("" = root); `chrono_fmt` is a
    /// strftime pattern the caller derives from the daily-format
    /// setting.
    pub fn daily_note(
        &self,
        date: NaiveDate,
        daily_dir: &str,
        chrono_fmt: &str,
    ) -> Option<PathBuf> {
        self.root.as_ref().map(|root| {
            root.join(daily_dir)
                .join(format!("{}.md", date.format(chrono_fmt)))
        })
    }

    /// Resolver for `![[image.png]]` embeds — basename lookup, vault-wide.
    pub fn image_resolver(&self) -> crate::document::ImageResolver {
        let images = Rc::clone(&self.images);
        std::rc::Rc::new(move |name: &str| images.borrow().get(&name.to_lowercase()).cloned())
    }
}

/// Sorted, non-overlapping `(byte range, replacement)` edits into a
/// note's text — produced by `link_spans_to` + `retarget_link`.
pub type TextEdits = Vec<(std::ops::Range<usize>, String)>;

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

fn percent_decode(s: &str) -> String {
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

fn build_items(dir: &Path, depth: usize, sort: TreeSort, show_other_files: bool) -> Vec<TreeItem> {
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
        let label = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            dirs.push(
                TreeItem::new(path.to_string_lossy().to_string(), label).children(build_items(
                    &path,
                    depth + 1,
                    sort,
                    show_other_files,
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

fn local_link_targets(text: &str) -> Vec<(String, bool)> {
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
mod link_diagnostics_tests {
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
}

fn collect_files(root: &Path) -> (Vec<PathBuf>, std::collections::HashMap<String, PathBuf>) {
    let mut notes = Vec::new();
    let mut images = std::collections::HashMap::new();
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
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());
            match ext.as_deref() {
                Some("md") | Some("base") => notes.push(path),
                Some(e) if IMAGE_EXTS.contains(&e) => {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        images.entry(name.to_lowercase()).or_insert(path.clone());
                    }
                }
                _ => {}
            }
        }
    }
    notes.sort();
    (notes, images)
}
