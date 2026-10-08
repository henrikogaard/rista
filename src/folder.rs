//! Read-only folder pages derived from the vault's ordinary files.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layout {
    List,
    #[default]
    Cards,
    Gallery,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Name,
    Modified,
    Type,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Dashboard {
    pub layout: Layout,
    pub sort: Sort,
    pub pinned: Vec<String>,
    pub tag: String,
    pub status: String,
}

#[derive(Clone, Default)]
pub struct Metadata {
    pub title: String,
    pub description: String,
    pub icon: Option<String>,
    pub tags: Vec<String>,
    pub status: String,
}

impl Metadata {
    pub fn parse(raw: &str, fallback: &str) -> Self {
        let props = crate::properties::properties(raw);
        let text = |key: &str| {
            props
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let body = body(raw);
        let heading = body.lines().find_map(|l| l.strip_prefix("# "));
        let title = text("title");
        let title = if title.is_empty() {
            heading.unwrap_or(fallback).to_string()
        } else {
            title
        };
        let mut icon = text("icon");
        let title = if let Some((len, _)) = crate::note_icons::shortcode(&title) {
            if icon.is_empty() {
                icon = title[..len].to_string();
            }
            title[len..].trim_start().to_string()
        } else {
            title
        };
        let description = text("description");
        let description = if description.is_empty() {
            excerpt(body)
        } else {
            description.chars().take(180).collect()
        };
        let tags = props
            .iter()
            .find(|(k, _)| k == "tags")
            .map(|(_, v)| match v {
                serde_yaml::Value::Sequence(values) => values
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(str::to_string)
                    .collect(),
                serde_yaml::Value::String(value) => value
                    .split(',')
                    .map(str::trim)
                    .map(str::to_string)
                    .collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default();
        Self {
            title,
            description,
            icon: (!icon.is_empty()).then_some(icon),
            tags,
            status: text("status"),
        }
    }

    pub fn matches(&self, query: &str, config: &Dashboard) -> bool {
        let haystack = format!(
            "{} {} {} {}",
            self.title,
            self.description,
            self.tags.join(" "),
            self.status
        )
        .to_lowercase();
        haystack.contains(&query.to_lowercase())
            && (config.tag.is_empty() || self.tags.iter().any(|tag| tag == &config.tag))
            && (config.status.is_empty() || self.status == config.status)
    }
}

fn excerpt(body: &str) -> String {
    fn plain(node: &markdown::mdast::Node, out: &mut String) {
        match node {
            markdown::mdast::Node::Text(text) => out.push_str(&text.value),
            markdown::mdast::Node::InlineCode(code) => out.push_str(&code.value),
            _ => {
                if let Some(children) = node.children() {
                    for child in children {
                        plain(child, out);
                    }
                }
            }
        }
    }
    let Ok(root) = markdown::to_mdast(body, &markdown::ParseOptions::default()) else {
        return String::new();
    };
    let mut text = String::new();
    if let Some(children) = root.children() {
        for paragraph in children
            .iter()
            .filter(|n| matches!(n, markdown::mdast::Node::Paragraph(_)))
            .take(2)
        {
            plain(paragraph, &mut text);
            text.push(' ');
        }
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(180)
        .collect()
}

pub fn body(raw: &str) -> &str {
    crate::properties::frontmatter_span(raw)
        .map(|span| &raw[span.end..])
        .unwrap_or(raw)
}

pub fn introduction_body<'a>(raw: &'a str, title: &str) -> &'a str {
    let body = body(raw).trim_start();
    let first = body.lines().next().unwrap_or("");
    if first.strip_prefix("# ").is_some_and(|h| {
        let h = crate::note_icons::shortcode(h)
            .map(|(len, _)| &h[len..])
            .unwrap_or(h);
        h.trim() == title.trim()
    }) {
        body.strip_prefix(first)
            .unwrap_or(body)
            .trim_start_matches(['\r', '\n'])
    } else {
        body
    }
}

pub fn dashboard(raw: &str) -> Dashboard {
    crate::properties::properties(raw)
        .into_iter()
        .find(|(k, _)| k == "dashboard")
        .and_then(|(_, v)| serde_yaml::from_value(v).ok())
        .unwrap_or_default()
}

pub fn summary(path: &Path) -> String {
    use std::io::Read;
    let mut raw = Vec::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(65536).read_to_end(&mut raw);
    }
    String::from_utf8_lossy(&raw).into_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Folder,
    Note,
    Database,
    Image,
    Other,
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
            if !entry.file_type()?.is_file() {
                continue;
            }
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
                _ => Kind::Other,
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
    fn dashboard_metadata_and_filters_are_portable() {
        assert_eq!(
            Metadata::parse("# Title\n\n**October** · Read [this](note.md).", "fallback")
                .description,
            "October · Read this."
        );
        let raw = "---\ntitle: Home\nicon: LiHouse\ntags: [work, notes]\nstatus: Active\ndashboard:\n  layout: gallery\n  sort: modified\n  pinned: [Plan.md, Ideas.md]\n---\n# Home\n\nIntroduction\n";
        let metadata = Metadata::parse(raw, "fallback");
        assert_eq!(metadata.title, "Home");
        assert_eq!(metadata.icon.as_deref(), Some("LiHouse"));
        assert_eq!(introduction_body(raw, "Home"), "Introduction\n");
        assert!(introduction_body(raw, "Other").starts_with("# Home"));
        let mut config = dashboard(raw);
        assert_eq!(config.layout, Layout::Gallery);
        assert_eq!(config.sort, Sort::Modified);
        assert_eq!(config.pinned, ["Plan.md", "Ideas.md"]);
        assert!(metadata.matches("INTRO", &config));
        config.tag = "work".into();
        config.status = "Active".into();
        assert!(metadata.matches("", &config));
        config.status = "Done".into();
        assert!(!metadata.matches("", &config));
        assert_eq!(dashboard("# plain").layout, Layout::Cards);
    }

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
            "details.txt",
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
                "cover.png",
                "details.txt"
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
