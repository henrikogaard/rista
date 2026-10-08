use std::path::{Path, PathBuf};

use gpui_kit::component::tree::TreeItem;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum FileFilter {
    #[default]
    All,
    Notes,
    Images,
    Other,
}

impl FileFilter {
    pub fn accepts(self, path: &Path) -> bool {
        let document = crate::file_preview::is_document(path);
        let image =
            crate::vault::IMAGE_EXTS.contains(&crate::file_preview::extension(path).as_str());
        match self {
            Self::All => true,
            Self::Notes => document,
            Self::Images => image,
            Self::Other => !document && !image,
        }
    }
}

pub fn filtered_tree(
    items: &[TreeItem],
    root: &Path,
    query: &str,
    filter: FileFilter,
) -> Vec<TreeItem> {
    let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
    items
        .iter()
        .filter_map(|item| {
            let path = Path::new(item.id.as_str());
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_lowercase();
            let matches = words.iter().all(|word| relative.contains(word));
            let mut item = TreeItem::new(item.id.clone(), item.label.clone())
                .children(item.children.clone())
                .expanded(item.is_expanded());
            if path.is_dir() {
                item.children = filtered_tree(&item.children, root, query, filter);
                if item.children.is_empty() && !(matches && filter == FileFilter::All) {
                    return None;
                }
                if !words.is_empty() || filter != FileFilter::All {
                    item = item.expanded(true);
                }
            } else if !matches || !filter.accepts(path) {
                return None;
            }
            Some(item)
        })
        .collect()
}

pub fn paths(items: &[TreeItem]) -> Vec<PathBuf> {
    items
        .iter()
        .flat_map(|item| {
            let mut entries = vec![PathBuf::from(item.id.as_str())];
            entries.extend(paths(&item.children));
            entries
        })
        .collect()
}

pub fn move_entry(root: &Path, source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    let root = root.canonicalize()?;
    let source_parent = source
        .parent()
        .ok_or(ErrorKind::InvalidInput)?
        .canonicalize()?;
    let destination_parent = destination
        .parent()
        .ok_or(ErrorKind::InvalidInput)?
        .canonicalize()?;
    let source = source_parent.join(source.file_name().ok_or(ErrorKind::InvalidInput)?);
    let destination =
        destination_parent.join(destination.file_name().ok_or(ErrorKind::InvalidInput)?);
    if !source.starts_with(&root)
        || !destination.starts_with(&root)
        || source == root
        || destination.starts_with(&source)
        || std::fs::symlink_metadata(&source)?.file_type().is_symlink()
    {
        return Err(Error::from(ErrorKind::InvalidInput));
    }
    if std::fs::symlink_metadata(&destination).is_ok() {
        return Err(Error::from(ErrorKind::AlreadyExists));
    }
    std::fs::rename(source, destination)
}

pub fn size_label(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_do_not_overlap() {
        for (path, category) in [
            ("note.MD", FileFilter::Notes),
            ("data.base", FileFilter::Notes),
            ("photo.PNG", FileFilter::Images),
            ("document.pdf", FileFilter::Other),
        ] {
            let path = Path::new(path);
            assert!(category.accepts(path));
            assert_eq!(
                [FileFilter::Notes, FileFilter::Images, FileFilter::Other]
                    .iter()
                    .filter(|f| f.accepts(path))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn moves_can_be_reversed_but_never_overwrite() {
        let root = std::env::temp_dir().join(format!(
            "rista-move-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join("folder")).unwrap();
        let source = root.join("note.md");
        let dest = root.join("folder/note.md");
        std::fs::write(&source, "original").unwrap();
        move_entry(&root, &source, &dest).unwrap();
        move_entry(&root, &dest, &source).unwrap();
        std::fs::write(&dest, "other").unwrap();
        assert_eq!(
            move_entry(&root, &source, &dest).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "original");
        assert!(move_entry(&root, &root.join("folder"), &root.join("folder/child")).is_err());
        let items =
            vec![
                TreeItem::new(root.join("folder").to_string_lossy().to_string(), "folder")
                    .children(vec![TreeItem::new(
                        dest.to_string_lossy().to_string(),
                        "note.md",
                    )]),
            ];
        assert_eq!(
            filtered_tree(&items, &root, "folder note", FileFilter::Notes).len(),
            1
        );
        assert!(filtered_tree(&items, &root, "folder", FileFilter::Images).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
