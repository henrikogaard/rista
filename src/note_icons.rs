//! Local icon shortcodes; source Markdown remains untouched.

use gpui_kit::*;
use std::collections::HashMap;
use std::sync::OnceLock;

pub fn icon_path(name: &str) -> Option<String> {
    static ICONS: OnceLock<HashMap<String, String>> = OnceLock::new();
    let icons = ICONS.get_or_init(|| {
        assets::AllAssets
            .list("icons/")
            .unwrap_or_default()
            .into_iter()
            .filter_map(|path| {
                let stem = std::path::Path::new(path.as_ref()).file_stem()?.to_str()?;
                Some((normalize(stem), path.to_string()))
            })
            .collect()
    });
    let name = name.trim_matches(':');
    let name = name
        .strip_prefix("lucide-")
        .or_else(|| name.strip_prefix("li-"))
        .or_else(|| name.strip_prefix("Li"))?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    icons.get(&normalize(name)).cloned()
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn shortcode(text: &str) -> Option<(usize, String)> {
    let rest = text.strip_prefix(':')?;
    let end = rest.find(':')?;
    let path = icon_path(&rest[..end])?;
    Some((end + 2, path))
}

#[cfg(test)]
mod tests {
    use super::{icon_path, shortcode};

    #[test]
    fn aliases_resolve_to_bundled_icons() {
        assert_eq!(icon_path("LiInbox"), icon_path("lucide-inbox"));
        assert!(icon_path("LiInbox").is_some());
        assert_eq!(icon_path("LiFileText"), icon_path("li-file-text"));
        assert!(shortcode(":LiInbox: Inbox").is_some());
        assert!(icon_path("lucide-missing-icon-xyz").is_none());
        assert!(shortcode(":smile:").is_none());
        assert!(shortcode(":lucide-../inbox:").is_none());
    }

    #[test]
    fn preview_only_rewrites_prose_shortcodes() {
        let source = "---\ntitle: ':LiInbox:'\n---\n# :LiInbox: Inbox\n`:LiInbox:`\n```md\n:LiInbox:\n```\n:lucide-no-such-icon-xyz:\n\\:LiInbox:\n";
        let rendered = crate::preview::preprocess(
            source,
            std::path::Path::new("note.md"),
            None,
            &|_| None,
            &Default::default(),
        );
        assert!(rendered.contains("# ![:LiInbox:](rista-icon:"));
        assert!(rendered.contains("title: ':LiInbox:'"));
        assert!(rendered.contains("`:LiInbox:`"));
        assert!(rendered.contains("```md\n:LiInbox:\n```"));
        assert!(rendered.contains(":lucide-no-such-icon-xyz:"));
        assert!(rendered.contains("\\:LiInbox:"));
    }
}
