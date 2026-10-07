//! Read-only folder pages derived from the vault's ordinary files.

use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Folder,
    Note,
    Database,
    Image,
}

pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub kind: Kind,
}

pub struct Contents {
    pub entries: Vec<Entry>,
    pub introduction: Option<PathBuf>,
}

pub fn introduction_path(dir: &Path) -> PathBuf {
    dir.join(format!(
        "{}.md",
        dir.file_name().unwrap_or_default().to_string_lossy()
    ))
}

pub fn read(dir: &Path) -> io::Result<Contents> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if crate::vault::should_skip(&entry) || entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        let kind = if entry.file_type()?.is_dir() {
            Kind::Folder
        } else {
            match path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase()
                .as_str()
            {
                "md" | "markdown" => Kind::Note,
                "base" => Kind::Database,
                ext if crate::vault::IMAGE_EXTS.contains(&ext) => Kind::Image,
                _ => continue,
            }
        };
        entries.push(Entry {
            path,
            name: entry.file_name().to_string_lossy().into_owned(),
            kind,
        });
    }
    entries.sort_by(|a, b| {
        (a.kind, a.name.to_lowercase(), &a.name).cmp(&(b.kind, b.name.to_lowercase(), &b.name))
    });
    let conventional = introduction_path(dir);
    let introduction = entries
        .iter()
        .find(|e| e.path == conventional)
        .or_else(|| {
            entries
                .iter()
                .find(|e| e.kind == Kind::Note && e.name.eq_ignore_ascii_case("README.md"))
        })
        .map(|e| e.path.clone());
    Ok(Contents {
        entries,
        introduction,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_contents_are_direct_sorted_objects_and_keep_empty_folders() {
        let dir = std::env::temp_dir().join(format!("rista-folder-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Empty")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        for name in [
            "z.md",
            "A.markdown",
            "Tasks.base",
            "cover.png",
            "ignore.txt",
            "README.md",
        ] {
            std::fs::write(dir.join(name), "").unwrap();
        }
        let contents = read(&dir).unwrap();
        assert_eq!(
            contents
                .entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            [
                "Empty",
                "A.markdown",
                "README.md",
                "z.md",
                "Tasks.base",
                "cover.png"
            ]
        );
        assert_eq!(contents.introduction, Some(dir.join("README.md")));
        let intro = introduction_path(&dir);
        std::fs::write(&intro, "# Introduction").unwrap();
        assert_eq!(read(&dir).unwrap().introduction, Some(intro));
        assert!(read(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
