//! Ordered bookmarks, legacy-star migration, and vault bookmark import.

use crate::search_service::SearchSort;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(flatten)]
    pub kind: BookmarkKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BookmarkKind {
    File {
        path: PathBuf,
        #[serde(default)]
        anchor: Option<String>,
        #[serde(default)]
        occurrence: usize,
    },
    Folder {
        path: PathBuf,
        #[serde(default)]
        root: Option<PathBuf>,
    },
    Search {
        root: PathBuf,
        query: String,
        #[serde(default)]
        match_case: bool,
        #[serde(default)]
        sort: SearchSort,
    },
    Graph {
        root: PathBuf,
        #[serde(default)]
        center: Option<PathBuf>,
    },
    Base {
        path: PathBuf,
        view: String,
    },
    Group {
        items: Vec<Bookmark>,
        #[serde(default)]
        collapsed: bool,
    },
}

impl BookmarkKind {
    pub fn file(path: PathBuf) -> Self {
        Self::File {
            path,
            anchor: None,
            occurrence: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bookmarks {
    pub items: Vec<Bookmark>,
    next_id: u64,
    migrated_stars: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    Root,
    Before(u64),
    After(u64),
    Into(u64),
}

pub(crate) fn capture_heading(source: &str, cursor: usize) -> Option<(String, usize)> {
    let mut cursor = cursor.min(source.len());
    while !source.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let caret_line = source.as_bytes()[..cursor]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    let headings = crate::preview::headings(source);
    let (heading_line, _, raw) = headings
        .iter()
        .rev()
        .find(|(line, _, _)| *line <= caret_line)?;
    let heading = raw.trim().to_owned();
    let occurrence = headings
        .iter()
        .filter(|(line, _, raw)| {
            *line <= *heading_line && raw.trim().eq_ignore_ascii_case(&heading)
        })
        .count()
        .saturating_sub(1);
    Some((heading, occurrence))
}

pub(crate) fn resolve_heading(source: &str, heading: &str, occurrence: usize) -> Option<usize> {
    crate::preview::headings(source)
        .into_iter()
        .filter(|(_, _, raw)| raw.trim().eq_ignore_ascii_case(heading))
        .nth(occurrence)
        .map(|(line, _, _)| line)
}

#[derive(Default)]
pub struct ImportResult {
    pub items: Vec<Bookmark>,
    pub imported: usize,
    pub skipped: usize,
}

impl Bookmarks {
    fn allocate(&mut self, kind: BookmarkKind, title: Option<String>) -> Bookmark {
        fn maximum(items: &[Bookmark]) -> u64 {
            items
                .iter()
                .map(|item| match &item.kind {
                    BookmarkKind::Group { items, .. } => item.id.max(maximum(items)),
                    _ => item.id,
                })
                .max()
                .unwrap_or(0)
        }
        self.next_id = self
            .next_id
            .max(maximum(&self.items))
            .checked_add(1)
            .expect("bookmark id space");
        Bookmark {
            id: self.next_id,
            title,
            kind,
        }
    }

    pub fn add(
        &mut self,
        kind: BookmarkKind,
        title: Option<String>,
        parent: Option<u64>,
    ) -> Option<u64> {
        if let Some(parent) = parent {
            if !matches!(
                self.find(parent).map(|item| &item.kind),
                Some(BookmarkKind::Group { .. })
            ) {
                return None;
            }
        }
        let item = self.allocate(kind, title);
        let id = item.id;
        match parent {
            Some(parent) => {
                if let BookmarkKind::Group { items, collapsed } = &mut self.find_mut(parent)?.kind {
                    *collapsed = false;
                    items.push(item);
                }
            }
            None => self.items.push(item),
        }
        Some(id)
    }

    pub fn find(&self, id: u64) -> Option<&Bookmark> {
        find(&self.items, id)
    }

    pub fn find_mut(&mut self, id: u64) -> Option<&mut Bookmark> {
        find_mut(&mut self.items, id)
    }

    pub fn find_kind(&self, kind: &BookmarkKind) -> Option<u64> {
        fn search(items: &[Bookmark], kind: &BookmarkKind) -> Option<u64> {
            items.iter().find_map(|item| {
                if &item.kind == kind {
                    return Some(item.id);
                }
                match &item.kind {
                    BookmarkKind::Group { items, .. } => search(items, kind),
                    _ => None,
                }
            })
        }
        search(&self.items, kind)
    }

    pub fn remove(&mut self, id: u64) -> Option<Bookmark> {
        take(&mut self.items, id)
    }

    pub fn move_to(&mut self, id: u64, target: DropTarget) -> bool {
        let Some(item) = self.find(id) else {
            return false;
        };
        let target_id = match target {
            DropTarget::Root => None,
            DropTarget::Before(id) | DropTarget::After(id) | DropTarget::Into(id) => Some(id),
        };
        if let Some(target_id) = target_id {
            if id == target_id || self.find(target_id).is_none() {
                return false;
            }
            if let BookmarkKind::Group { items, .. } = &item.kind {
                if find(items, target_id).is_some() {
                    return false;
                }
            }
            if matches!(target, DropTarget::Into(_))
                && !matches!(
                    self.find(target_id).map(|item| &item.kind),
                    Some(BookmarkKind::Group { .. })
                )
            {
                return false;
            }
        }
        let item = self.remove(id).expect("validated bookmark");
        match target {
            DropTarget::Root => self.items.push(item),
            DropTarget::Into(id) => {
                if let BookmarkKind::Group { items, collapsed } =
                    &mut self.find_mut(id).expect("validated group").kind
                {
                    *collapsed = false;
                    items.push(item);
                }
            }
            DropTarget::Before(id) | DropTarget::After(id) => {
                let (items, position) =
                    siblings_mut(&mut self.items, id).expect("validated target");
                items.insert(
                    position + usize::from(matches!(target, DropTarget::After(_))),
                    item,
                );
            }
        }
        true
    }

    pub fn migrate_stars(&mut self, starred: &[String]) {
        if self.migrated_stars {
            return;
        }
        for path in starred {
            let kind = BookmarkKind::file(PathBuf::from(path));
            if self.find_kind(&kind).is_none() {
                self.add(kind, None, None);
            }
        }
        self.migrated_stars = true;
    }

    pub fn starred_paths(&self) -> Vec<String> {
        fn collect(items: &[Bookmark], paths: &mut Vec<String>) {
            for item in items {
                match &item.kind {
                    BookmarkKind::File {
                        path, anchor: None, ..
                    } => {
                        let path = path.to_string_lossy().to_string();
                        if !paths.contains(&path) {
                            paths.push(path);
                        }
                    }
                    BookmarkKind::Group { items, .. } => collect(items, paths),
                    _ => {}
                }
            }
        }
        let mut paths = Vec::new();
        collect(&self.items, &mut paths);
        paths
    }

    pub fn repoint(&mut self, source: &Path, destination: &Path) {
        fn visit(items: &mut [Bookmark], source: &Path, destination: &Path) {
            for item in items {
                match &mut item.kind {
                    BookmarkKind::File { path, .. } | BookmarkKind::Base { path, .. } => {
                        crate::explorer::repoint_path(path, source, destination);
                    }
                    BookmarkKind::Folder { path, root } => {
                        crate::explorer::repoint_path(path, source, destination);
                        if let Some(root) = root {
                            crate::explorer::repoint_path(root, source, destination);
                        }
                    }
                    BookmarkKind::Search { root, .. } => {
                        crate::explorer::repoint_path(root, source, destination);
                    }
                    BookmarkKind::Graph { root, center } => {
                        crate::explorer::repoint_path(root, source, destination);
                        if let Some(center) = center {
                            crate::explorer::repoint_path(center, source, destination);
                        }
                    }
                    BookmarkKind::Group { items, .. } => visit(items, source, destination),
                }
            }
        }
        visit(&mut self.items, source, destination);
    }

    pub fn import_vault(
        &mut self,
        root: &Path,
        raw: &str,
    ) -> Result<ImportResult, serde_json::Error> {
        #[derive(Deserialize)]
        struct Source {
            items: Vec<serde_json::Value>,
        }
        let source: Source = serde_json::from_str(raw)?;
        let mut result = ImportResult::default();
        for value in source.items {
            if let Some(item) = self.import_item(root, &value, &mut result, 0) {
                result.items.push(item);
            }
        }
        Ok(result)
    }

    fn import_item(
        &mut self,
        root: &Path,
        value: &serde_json::Value,
        result: &mut ImportResult,
        depth: usize,
    ) -> Option<Bookmark> {
        let mut parse = || -> Option<Bookmark> {
            if depth > 32 {
                return None;
            }
            let title = value
                .get("title")
                .and_then(|s| s.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned);
            let kind = match value.get("type")?.as_str()? {
                "group" => {
                    let mut items = Vec::new();
                    for child in value.get("items")?.as_array()? {
                        if let Some(item) = self.import_item(root, child, result, depth + 1) {
                            items.push(item);
                        }
                    }
                    BookmarkKind::Group {
                        items,
                        collapsed: false,
                    }
                }
                "file" | "folder" => {
                    let relative = Path::new(value.get("path")?.as_str()?);
                    if relative
                        .components()
                        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
                    {
                        return None;
                    }
                    let path = root.join(relative);
                    if value.get("type")?.as_str()? == "folder" {
                        BookmarkKind::Folder {
                            path,
                            root: Some(root.to_path_buf()),
                        }
                    } else {
                        if relative.as_os_str().is_empty() {
                            return None;
                        }
                        let anchor = value
                            .get("subpath")
                            .and_then(|s| s.as_str())
                            .map(|s| s.strip_prefix('#').unwrap_or(s))
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned);
                        let is_base = path
                            .extension()
                            .is_some_and(|extension| extension == "base");
                        match (is_base, anchor) {
                            (true, Some(view)) => BookmarkKind::Base { path, view },
                            (_, anchor) => BookmarkKind::File {
                                path,
                                anchor,
                                occurrence: 0,
                            },
                        }
                    }
                }
                "search" => BookmarkKind::Search {
                    root: root.to_path_buf(),
                    query: value.get("query")?.as_str()?.to_owned(),
                    match_case: false,
                    sort: SearchSort::Relevance,
                },
                "graph" => BookmarkKind::Graph {
                    root: root.to_path_buf(),
                    center: None,
                },
                _ => return None,
            };
            Some(self.allocate(kind, title))
        };
        match parse() {
            Some(item) => {
                result.imported += 1;
                Some(item)
            }
            None => {
                result.skipped += 1;
                None
            }
        }
    }
}

fn find(items: &[Bookmark], id: u64) -> Option<&Bookmark> {
    items.iter().find_map(|item| {
        if item.id == id {
            return Some(item);
        }
        match &item.kind {
            BookmarkKind::Group { items, .. } => find(items, id),
            _ => None,
        }
    })
}

fn find_mut(items: &mut [Bookmark], id: u64) -> Option<&mut Bookmark> {
    for item in items {
        if item.id == id {
            return Some(item);
        }
        if let BookmarkKind::Group { items, .. } = &mut item.kind {
            if let Some(found) = find_mut(items, id) {
                return Some(found);
            }
        }
    }
    None
}

fn take(items: &mut Vec<Bookmark>, id: u64) -> Option<Bookmark> {
    if let Some(ix) = items.iter().position(|item| item.id == id) {
        return Some(items.remove(ix));
    }
    for item in items {
        if let BookmarkKind::Group { items, .. } = &mut item.kind {
            if let Some(item) = take(items, id) {
                return Some(item);
            }
        }
    }
    None
}

fn siblings_mut(items: &mut Vec<Bookmark>, id: u64) -> Option<(&mut Vec<Bookmark>, usize)> {
    if let Some(ix) = items.iter().position(|item| item.id == id) {
        return Some((items, ix));
    }
    for item in items {
        if let BookmarkKind::Group { items, .. } = &mut item.kind {
            if let Some(found) = siblings_mut(items, id) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> BookmarkKind {
        BookmarkKind::Group {
            items: Vec::new(),
            collapsed: false,
        }
    }

    #[test]
    fn migration_preserves_missing_stars_and_does_not_resurrect_removed_items() {
        let mut store = Bookmarks::default();
        let stars = vec![
            "/vault/missing.md".into(),
            "/vault/a.md".into(),
            "/vault/a.md".into(),
        ];
        store.migrate_stars(&stars);
        assert_eq!(store.starred_paths(), stars[..2]);
        store.remove(store.items[0].id);
        let mut restored: Bookmarks =
            serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        restored.migrate_stars(&stars);
        assert_eq!(restored.starred_paths(), vec!["/vault/a.md"]);
    }

    #[test]
    fn nested_reordering_rejects_cycles_without_losing_items() {
        let mut store = Bookmarks::default();
        let outer = store.add(group(), Some("Outer".into()), None).unwrap();
        let inner = store
            .add(group(), Some("Inner".into()), Some(outer))
            .unwrap();
        let a = store
            .add(BookmarkKind::file("/a.md".into()), None, Some(inner))
            .unwrap();
        let b = store
            .add(BookmarkKind::file("/b.md".into()), None, None)
            .unwrap();
        let before = serde_json::to_value(&store).unwrap();
        assert!(!store.move_to(outer, DropTarget::Into(inner)));
        assert!(!store.move_to(inner, DropTarget::After(a)));
        assert!(!store.move_to(a, DropTarget::Into(b)));
        assert_eq!(serde_json::to_value(&store).unwrap(), before);
        assert!(store.move_to(b, DropTarget::Before(a)));
        assert_eq!(store.starred_paths(), vec!["/b.md", "/a.md"]);
        assert!(store.move_to(b, DropTarget::After(a)));
        assert_eq!(store.starred_paths(), vec!["/a.md", "/b.md"]);
        assert!(store.move_to(inner, DropTarget::Root));
        assert_eq!(store.items.last().unwrap().id, inner);
    }

    #[test]
    fn folder_moves_repoint_nested_anchors_views_and_scoped_searches() {
        let mut store = Bookmarks::default();
        let parent = store.add(group(), Some("Work".into()), None).unwrap();
        let kinds = [
            BookmarkKind::File {
                path: "/old/n.md".into(),
                anchor: Some("Heading".into()),
                occurrence: 1,
            },
            BookmarkKind::Folder {
                path: "/old/folder".into(),
                root: Some("/old".into()),
            },
            BookmarkKind::Search {
                root: "/old".into(),
                query: "tag:work".into(),
                match_case: true,
                sort: SearchSort::Modified,
            },
            BookmarkKind::Graph {
                root: "/old".into(),
                center: Some("/old/n.md".into()),
            },
            BookmarkKind::Base {
                path: "/old/db.base".into(),
                view: "Cards".into(),
            },
        ];
        for kind in kinds {
            store.add(kind, Some("Custom".into()), Some(parent));
        }
        store.repoint(Path::new("/old"), Path::new("/new"));
        let serialized = serde_json::to_string(&store).unwrap();
        assert!(!serialized.contains("/old"));
        assert_eq!(serialized.matches("Custom").count(), 5);
        let restored: Bookmarks = serde_json::from_str(&serialized).unwrap();
        assert_eq!(store.items, restored.items);
    }

    #[test]
    fn heading_capture_keeps_duplicates_and_literal_hashes() {
        let source = "# Duplicate\n\n# \\# Literal\n\n# Duplicate\n";
        let second_duplicate = source.rfind("# Duplicate").unwrap();
        let (heading, occurrence) = capture_heading(source, second_duplicate).unwrap();
        assert_eq!(heading, "Duplicate");
        assert_eq!(occurrence, 1);
        assert_eq!(resolve_heading(source, &heading, occurrence), Some(5));

        let literal_hash = source.find("# \\# Literal").unwrap();
        let (heading, occurrence) = capture_heading(source, literal_hash).unwrap();
        assert_eq!(heading, "# Literal");
        assert_eq!(occurrence, 0);
        assert_eq!(resolve_heading(source, &heading, occurrence), Some(3));

        let mut store = Bookmarks::default();
        let result = store
            .import_vault(
                Path::new("/vault"),
                r###"{"items":[{"type":"file","path":"literal.md","subpath":"## Literal"}]}"###,
            )
            .unwrap();
        assert!(matches!(
            &result.items[0].kind,
            BookmarkKind::File {
                anchor: Some(anchor),
                ..
            } if anchor == "# Literal"
        ));
    }

    #[test]
    fn vault_import_preserves_groups_targets_titles_and_rejects_escaping_paths() {
        let raw = r##"{"items":[{"type":"group","title":"Work","items":[
            {"type":"file","path":"a.md","subpath":"#Heading","title":"Custom"},
            {"type":"file","path":"a.md","subpath":"#^block"},
            {"type":"folder","path":"Folder"},
            {"type":"search","query":"tag:work"},
            {"type":"graph"},
            {"type":"file","path":"db.base","subpath":"#Cards"},
            {"type":"file","path":"../outside.md"},
            {"type":"file","path":"/absolute.md"},
            {"type":"unknown"}]}]}"##;
        let mut store = Bookmarks::default();
        let result = store.import_vault(Path::new("/vault"), raw).unwrap();
        assert_eq!((result.imported, result.skipped), (7, 3));
        assert_eq!(result.items[0].title.as_deref(), Some("Work"));
        let BookmarkKind::Group { items, .. } = &result.items[0].kind else {
            panic!()
        };
        assert_eq!(items[0].title.as_deref(), Some("Custom"));
        assert!(
            matches!(&items[1].kind, BookmarkKind::File { anchor: Some(anchor), .. } if anchor == "^block")
        );
        assert!(
            matches!(&items[2].kind, BookmarkKind::Folder { path, root: Some(root) } if path == Path::new("/vault/Folder") && root == Path::new("/vault"))
        );
        assert_eq!(
            serde_json::to_value(&items[2]).unwrap()["root"],
            serde_json::json!("/vault")
        );
        let legacy: BookmarkKind = serde_json::from_value(serde_json::json!({
            "type": "folder",
            "path": "/vault/Folder"
        }))
        .unwrap();
        assert!(matches!(legacy, BookmarkKind::Folder { root: None, .. }));
        assert!(matches!(&items[5].kind, BookmarkKind::Base { view, .. } if view == "Cards"));
        let mut ids = items.iter().map(|item| item.id).collect::<Vec<_>>();
        ids.push(result.items[0].id);
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 7);
        assert!(store.items.is_empty());
    }
}
