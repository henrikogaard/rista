//! Vault: an opened folder. Owns the file tree, the flattened note index,
//! and the filesystem watcher that keeps both honest.

use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState};
use gpui_kit::*;
use notify::{RecursiveMode, Watcher};

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", "dist", ".build"];

/// Emitted after a filesystem burst settles — the tree was already refreshed.
pub enum VaultEvent {
    FilesChanged,
}

impl EventEmitter<VaultEvent> for Vault {}

pub struct Vault {
    pub root: Option<PathBuf>,
    pub tree: Entity<TreeState>,
    /// Flattened list of every `.md` and `.base` file in the vault,
    /// sorted.
    pub notes: Vec<PathBuf>,
    /// Image files indexed by lowercase file name — Obsidian resolves
    /// `![[name.png]]` vault-wide, so basename is the key.
    /// Shared behind one `Rc`: resolvers handed out to documents stay live
    /// and see files added later (paste/drop, watcher refreshes).
    pub images: Rc<RefCell<std::collections::HashMap<String, PathBuf>>>,
    /// `(tag, note_count)` pairs, rebuilt with the index — completions read
    /// this snapshot instead of re-parsing every note per keystroke.
    pub tags: Vec<(String, usize)>,
    watcher: Option<notify::RecommendedWatcher>,
    pending_events: usize,
    /// Folder ids the user expanded — reapplied to rebuilt trees so
    /// watcher refreshes don't collapse the sidebar.
    expanded: std::collections::BTreeSet<String>,
    _tree_sub: Subscription,
}

impl Vault {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tree = cx.new(|cx| TreeState::new(cx));
        let tree_sub = cx.subscribe(&tree, |this, _tree, event, _cx| match event {
            TreeEvent::Expanded(id) => {
                this.expanded.insert(id.to_string());
            }
            TreeEvent::Collapsed(id) => {
                this.expanded.remove(id.as_str());
            }
        });
        Self {
            root: None,
            tree,
            notes: Vec::new(),
            images: Rc::new(RefCell::new(std::collections::HashMap::new())),
            watcher: None,
            pending_events: 0,
            tags: Vec::new(),
            expanded: Default::default(),
            _tree_sub: tree_sub,
        }
    }

    pub fn is_open(&self) -> bool {
        self.root.is_some()
    }

    pub fn open(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.stop_watching();
        self.root = Some(root);
        self.refresh_tree(cx);
        self.start_watcher(cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.stop_watching();
        self.root = None;
        self.notes.clear();
        self.tree
            .update(cx, |tree, cx| tree.set_items(Vec::new(), cx));
        cx.notify();
    }

    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let items = mark_expanded(build_items(&root, 0), &self.expanded);
        let (notes, images) = collect_files(&root);
        self.tags = crate::properties::vault_tags(&notes);
        self.tree.update(cx, |tree, cx| tree.set_items(items, cx));
        self.notes = notes;
        *self.images.borrow_mut() = images;
        cx.notify();
    }

    /// Public refresh — called after our own writes so the index stays warm.
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
        // Full path match first, then basename match.
        self.notes
            .iter()
            .find(|p| {
                p.strip_prefix(self.root.as_deref().unwrap_or(Path::new("")))
                    .ok()
                    .and_then(|rel| rel.to_str())
                    .map(|s| s.trim_end_matches(".md").to_lowercase() == needle)
                    .unwrap_or(false)
            })
            .cloned()
            .or_else(|| {
                self.notes
                    .iter()
                    .find(|p| {
                        p.file_stem()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_lowercase() == needle)
                            .unwrap_or(false)
                    })
                    .cloned()
            })
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

    /// The daily-note path for today: `YYYY-MM-DD.md` at vault root.
    pub fn daily_note(&self) -> Option<PathBuf> {
        self.root
            .as_ref()
            .map(|root| root.join(format!("{}.md", chrono::Local::now().format("%Y-%m-%d"))))
    }

    /// Resolver for `![[image.png]]` embeds — basename lookup, vault-wide.
    pub fn image_resolver(&self) -> crate::document::ImageResolver {
        let images = Rc::clone(&self.images);
        std::rc::Rc::new(move |name: &str| images.borrow().get(&name.to_lowercase()).cloned())
    }
}

fn should_skip(entry: &std::fs::DirEntry) -> bool {
    let name = entry.file_name();
    let name = name.to_string_lossy();
    name.starts_with('.') || (entry.path().is_dir() && SKIP_DIRS.iter().any(|d| name == *d))
}

fn build_items(dir: &Path, depth: usize) -> Vec<TreeItem> {
    if depth > 12 {
        return Vec::new();
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in read.flatten() {
        if should_skip(&entry) {
            continue;
        }
        let path = entry.path();
        let label = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            dirs.push(
                TreeItem::new(path.to_string_lossy().to_string(), label)
                    .children(build_items(&path, depth + 1)),
            );
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let e = e.to_lowercase();
                e == "md" || e == "base" || IMAGE_EXTS.contains(&e.as_str())
            })
            .unwrap_or(false)
        {
            files.push(TreeItem::new(path.to_string_lossy().to_string(), label));
        }
    }
    dirs.sort_by(|a, b| a.label.cmp(&b.label));
    files.sort_by(|a, b| a.label.cmp(&b.label));
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
        .map(|mut item| {
            if expanded.contains(item.id.as_str()) {
                item = item.expanded(true);
            }
            item.children = mark_expanded(std::mem::take(&mut item.children), expanded);
            item
        })
        .collect()
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "avif", "bmp"];

/// Whether the path's extension is one of the vault's image types.
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
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
