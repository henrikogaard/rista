//! Editor completion: `/` at the start of a line opens markdown block
//! templates — rich , plain markdown out. `[[` / `![[` anywhere
//! completes vault notes (and vault images for embeds).

use gpui_kit::component::input::{CompletionProvider, Rope, RopeExt};
use gpui_kit::{App, Entity, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Range, TextEdit,
};

use crate::vault::Vault;

struct SlashItem {
    label: &'static str,
    detail: &'static str,
    keywords: &'static str,
    text: &'static str,
}

const ITEMS: &[SlashItem] = &[
    SlashItem {
        label: "Heading 1",
        detail: "#",
        keywords: "h1 title",
        text: "# ",
    },
    SlashItem {
        label: "Heading 2",
        detail: "##",
        keywords: "h2 section",
        text: "## ",
    },
    SlashItem {
        label: "Heading 3",
        detail: "###",
        keywords: "h3 subsection",
        text: "### ",
    },
    SlashItem {
        label: "Bulleted list",
        detail: "-",
        keywords: "ul unordered",
        text: "- ",
    },
    SlashItem {
        label: "Numbered list",
        detail: "1.",
        keywords: "ol ordered",
        text: "1. ",
    },
    SlashItem {
        label: "To-do",
        detail: "- [ ]",
        keywords: "task checkbox check",
        text: "- [ ] ",
    },
    SlashItem {
        label: "Quote",
        detail: ">",
        keywords: "blockquote",
        text: "> ",
    },
    SlashItem {
        label: "Callout",
        detail: "> [!note]",
        keywords: "note warning tip",
        text: "> [!note] Title\n> ",
    },
    SlashItem {
        label: "Divider",
        detail: "---",
        keywords: "hr rule line",
        text: "---\n",
    },
    SlashItem {
        label: "Code block",
        detail: "```",
        keywords: "fence pre",
        text: "```\n\n```\n",
    },
    SlashItem {
        label: "Table",
        detail: "| |",
        keywords: "grid columns",
        text: "| Column 1 | Column 2 |\n| --- | --- |\n|  |  |\n",
    },
    SlashItem {
        label: "Image",
        detail: "![[image]]",
        keywords: "picture photo embed",
        text: "![[]]",
    },
    SlashItem {
        label: "Wiki link",
        detail: "[[note]]",
        keywords: "link internal",
        text: "[[]]",
    },
    SlashItem {
        label: "Math block",
        detail: "$$",
        keywords: "latex equation tex",
        text: "$$\n\n$$\n",
    },
    SlashItem {
        label: "Base",
        detail: "```base",
        keywords: "database query table cards",
        text: "```base\nfilters:\n  and:\n    - 'file.ext == \"md\"'\nviews:\n  - type: table\n    name: Table\n```\n",
    },
];

/// Completion provider installed on every document editor. The menu opens on
/// `/` (block templates, only when the line so far is exactly `/query`) and
/// on `[[` / `![[` (note/image links resolved against the live vault).
pub struct VaultCompletions {
    vault: Option<Entity<Vault>>,
    /// The note's title, for `{{title}}` in template items.
    title: std::rc::Rc<std::cell::RefCell<String>>,
}

impl VaultCompletions {
    pub fn new(
        vault: Option<Entity<Vault>>,
        title: std::rc::Rc<std::cell::RefCell<String>>,
    ) -> Self {
        Self { vault, title }
    }
}

impl CompletionProvider for VaultCompletions {
    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        // `completions` decides for real — be permissive here so the query
        // keeps updating as the user types `/word` or `[[wor`. `[` is
        // allowed so a batched insert like `[[s` still triggers.
        new_text.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '-' | '_' | ' ' | '/' | '[' | '!' | '.' | '#' | '^' | ':')
        })
    }

    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        if let Some(resp) = wiki_items(text, offset, self.vault.as_ref(), cx) {
            return Task::ready(Ok(resp));
        }
        if let Some(resp) = tag_items(text, offset, self.vault.as_ref(), cx) {
            return Task::ready(Ok(resp));
        }
        if let Some(resp) = highlight_color_items(text, offset) {
            return Task::ready(Ok(resp));
        }
        if let Some(resp) = emoji_items(text, offset) {
            return Task::ready(Ok(resp));
        }
        let templates = || {
            let Some(vault) = self.vault.as_ref() else {
                return Vec::new();
            };
            let vault = vault.read(cx);
            let Some(root) = vault.root.as_ref() else {
                return Vec::new();
            };
            let title = self.title.borrow();
            let now = chrono::Local::now().naive_local();
            crate::app::template_files(root, &vault.templates_dir)
                .into_iter()
                .filter_map(|path| {
                    let name = path.file_stem()?.to_string_lossy().to_string();
                    let text = std::fs::read_to_string(&path).ok()?;
                    let ctx = crate::templater::TemplateCtx::new(&title, now);
                    Some((name, crate::templater::expand(&text, &ctx).text))
                })
                .collect()
        };
        Task::ready(Ok(slash_items(text, offset, templates)))
    }
}

/// `[[query` / `![[query` completions against the live vault index.
/// Notes insert as `[[stem]]` (or `[[dir/stem]]` when the stem isn't
/// unique); images insert their basename, matching how `preprocess`
/// resolves them vault-wide.
fn wiki_items(
    text: &Rope,
    offset: usize,
    vault: Option<&Entity<Vault>>,
    cx: &mut App,
) -> Option<CompletionResponse> {
    let vault = vault?;
    let point = text.offset_to_point(offset);
    let line = text.slice_line(point.row).to_string();
    let line_start = text.line_start_offset(point.row);
    let prefix = line.get(..point.column.min(line.len())).unwrap_or_default();

    // Rightmost `[[` before the cursor; `![[` is `[[` preceded by `!`
    // (the `!` stays outside the replaced range either way).
    let brackets = prefix.rfind("[[").map(|i| i + 2)?;
    let query = &prefix[brackets..];
    // Inside a closed link, or the opener is really a single `[` (rfind
    // can't see a second `[` — `[[[` still counts).
    if query.contains(']') || prefix[..brackets - 2].ends_with('[') {
        return None;
    }
    let embed = prefix[..brackets - 2].ends_with('!');
    let q = query.to_lowercase();

    // Swallow a `]]` the user already typed so `[[a]]` doesn't become
    // `[[note]]]]`.
    let suffix = line.get(point.column.min(line.len())..).unwrap_or_default();
    let extra = if suffix.starts_with("]]") { 2 } else { 0 };

    let range = Range {
        start: text.offset_to_position(line_start + brackets),
        end: text.offset_to_position(offset + extra),
    };

    let vault = vault.read(cx);

    // `[[note#…` — heading anchors. The note part resolves the same way
    // the link itself does; an empty part (`[[#`) completes headings in
    // the note being edited, matching the reference editor.
    if let Some(hash) = query.find('#') {
        let note_part = &query[..hash];
        let head_q = query[hash + 1..].to_lowercase();
        let src = if note_part.is_empty() {
            Some(text.to_string())
        } else {
            vault
                .resolve_wikilink(note_part)
                .and_then(|note| std::fs::read_to_string(note).ok())
        };
        let mut items: Vec<CompletionItem> = Vec::new();
        if let Some(src) = src {
            if let Some(bid_q) = head_q.strip_prefix('^') {
                // `[[note#^…` — block-id anchors: lines carrying a
                // trailing ` ^id` marker (or a lone `^id` line), same
                // convention `slice_section` resolves.
                let bid_q = bid_q.to_lowercase();
                let mut in_fence = false;
                for (ix, line) in src.lines().enumerate() {
                    let t = line.trim_end();
                    if t.starts_with("```") || t.starts_with("~~~") {
                        in_fence = !in_fence;
                        continue;
                    }
                    if in_fence {
                        continue;
                    }
                    let Some(pos) = t.rfind('^') else {
                        continue;
                    };
                    let id = &t[pos + 1..];
                    if id.is_empty()
                        || !id
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                        || !(pos == 0 || t.as_bytes()[pos - 1] == b' ')
                    {
                        continue;
                    }
                    if !bid_q.is_empty() && !id.to_lowercase().contains(&bid_q) {
                        continue;
                    }
                    items.push(CompletionItem {
                        label: format!("^{id}"),
                        detail: Some(if note_part.is_empty() {
                            "block · this note".to_string()
                        } else {
                            format!("block · {note_part}")
                        }),
                        kind: Some(CompletionItemKind::REFERENCE),
                        sort_text: Some(format!("{:04}", ix)),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range,
                            new_text: format!("{note_part}#^{id}]]"),
                        })),
                        ..Default::default()
                    });
                }
                // Blocks without an id yet: link a content-derived id; the
                // next save writes it into the target (Workspace::
                // add_linked_block_ids).
                for (ix, block) in crate::document::block_candidates(&src)
                    .into_iter()
                    .filter(|b| b.id.is_none())
                    .enumerate()
                {
                    if !bid_q.is_empty() && !block.first_line.to_lowercase().contains(&bid_q) {
                        continue;
                    }
                    let id = block.hash_id;
                    let label: String = block.first_line.chars().take(60).collect();
                    items.push(CompletionItem {
                        label,
                        detail: Some("block · adds an id".to_string()),
                        kind: Some(CompletionItemKind::REFERENCE),
                        sort_text: Some(format!("z{:04}", ix)),
                        filter_text: Some(format!("^{}", block.first_line)),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range,
                            new_text: format!("{note_part}#^{id}]]"),
                        })),
                        ..Default::default()
                    });
                }
            } else {
                let mut in_fence = false;
                for (ix, line) in src.lines().enumerate() {
                    let t = line.trim_start();
                    if t.starts_with("```") || t.starts_with("~~~") {
                        in_fence = !in_fence;
                        continue;
                    }
                    if in_fence || !t.starts_with('#') {
                        continue;
                    }
                    let marks = t.chars().take_while(|c| *c == '#').count();
                    if marks > 6 {
                        continue;
                    }
                    let heading = t[marks..].trim_start();
                    if heading.is_empty()
                        || (!head_q.is_empty() && !heading.to_lowercase().contains(&head_q))
                    {
                        continue;
                    }
                    items.push(CompletionItem {
                        label: heading.to_string(),
                        detail: Some(if note_part.is_empty() {
                            "heading · this note".to_string()
                        } else {
                            format!("heading · {note_part}")
                        }),
                        kind: Some(CompletionItemKind::REFERENCE),
                        sort_text: Some(format!("{:04}", ix)),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range,
                            new_text: format!("{note_part}#{heading}]]"),
                        })),
                        ..Default::default()
                    });
                }
            }
        }
        return Some(CompletionResponse::Array(items));
    }

    let root = vault.root.clone().unwrap_or_default();

    // Stems that appear more than once need their directory in the
    // inserted target to stay unambiguous.
    let mut stem_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for note in &vault.notes {
        if let Some(stem) = note.file_stem().and_then(|s| s.to_str()) {
            *stem_counts.entry(stem.to_lowercase()).or_default() += 1;
        }
    }

    let mut note_rows: Vec<(String, String, String)> = Vec::new();
    for note in &vault.notes {
        let Some(stem) = note.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let rel = note
            .strip_prefix(&root)
            .unwrap_or(note)
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
        let unique = stem_counts.get(&stem.to_lowercase()).copied().unwrap_or(0) <= 1;
        let insert = if unique {
            stem.to_string()
        } else {
            rel.clone()
        };
        note_rows.push((stem.to_string(), rel, insert));
    }

    let mut image_rows: Vec<String> = vault.images.borrow().keys().cloned().collect();
    image_rows.sort();

    let mut items: Vec<CompletionItem> = Vec::new();
    let mut push = |label: String, detail: String, insert: String, kind| {
        let hay = label.to_lowercase();
        if !q.is_empty() && !hay.contains(q.as_str()) {
            return;
        }
        items.push(CompletionItem {
            label,
            detail: Some(detail),
            kind: Some(kind),
            sort_text: Some(if hay.starts_with(q.as_str()) {
                format!("0{}", hay)
            } else {
                format!("1{}", hay)
            }),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: format!("{insert}]]"),
            })),
            ..Default::default()
        });
    };

    if embed {
        for name in &image_rows {
            push(
                name.clone(),
                "image".to_string(),
                name.clone(),
                CompletionItemKind::FILE,
            );
        }
        for (label, rel, insert) in &note_rows {
            push(
                label.clone(),
                rel.clone(),
                insert.clone(),
                CompletionItemKind::REFERENCE,
            );
        }
    } else {
        for (label, rel, insert) in &note_rows {
            push(
                label.clone(),
                rel.clone(),
                insert.clone(),
                CompletionItemKind::REFERENCE,
            );
        }
        for name in &image_rows {
            push(
                name.clone(),
                "image".to_string(),
                name.clone(),
                CompletionItemKind::FILE,
            );
        }
    }
    Some(CompletionResponse::Array(items))
}

/// `#tag` completion from the vault tag index. A `#` only counts as a
/// tag opener at a word boundary (start of line or after whitespace) —
/// `##` stays a heading marker.
fn tag_items(
    text: &Rope,
    offset: usize,
    vault: Option<&Entity<Vault>>,
    cx: &mut App,
) -> Option<CompletionResponse> {
    let vault = vault?;
    let point = text.offset_to_point(offset);
    let line = text.slice_line(point.row).to_string();
    let line_start = text.line_start_offset(point.row);
    let prefix = line.get(..point.column.min(line.len())).unwrap_or_default();

    // Rightmost `#` whose query so far is tag-shaped.
    let hash = prefix.rfind('#')?;
    if hash > 0 {
        let before = prefix[..hash].chars().last()?;
        if !before.is_whitespace() {
            return None;
        }
    }
    let query = &prefix[hash + 1..];
    if !query
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '/')
    {
        return None;
    }
    let q = query.to_lowercase();
    let range = Range {
        start: text.offset_to_position(line_start + hash + 1),
        end: text.offset_to_position(offset),
    };

    let items: Vec<CompletionItem> = vault
        .read(cx)
        .tags
        .clone()
        .into_iter()
        .filter(|(tag, _)| q.is_empty() || tag.to_lowercase().contains(q.as_str()))
        .map(|(tag, count)| CompletionItem {
            label: tag.clone(),
            detail: Some(format!("{count} note{}", if count == 1 { "" } else { "s" })),
            kind: Some(CompletionItemKind::KEYWORD),
            sort_text: Some(if tag.to_lowercase().starts_with(q.as_str()) {
                format!("0{}", tag.to_lowercase())
            } else {
                format!("1{}", tag.to_lowercase())
            }),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: tag,
            })),
            ..Default::default()
        })
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

/// `:query` → emoji — the emoji-picker core plugin. The `:`
/// must start a token (whitespace or line start before it) so `https:`
/// and `::` never fire; needs ≥2 query chars to keep the popup quiet.
/// `==r…` right after an opening `==` — the highlight colors whose
/// name starts with the typed letters (Obsidian's `==` suggestions).
/// Accepting swaps the letters for the color emoji: `==🔴`.
fn highlight_color_items(text: &Rope, offset: usize) -> Option<CompletionResponse> {
    let point = text.offset_to_point(offset);
    let line = text.slice_line(point.row).to_string();
    let line_start = text.line_start_offset(point.row);
    let prefix = line.get(..point.column.min(line.len()))?;
    let marks = crate::document::highlight_delimiters(prefix);
    if marks.len().is_multiple_of(2) {
        return None;
    }
    let open = *marks.last()? + 2;
    let query = prefix[open..].to_lowercase();
    if query.is_empty() || query.len() > 6 || !query.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let range = Range {
        start: text.offset_to_position(line_start + open),
        end: text.offset_to_position(offset),
    };
    let items: Vec<CompletionItem> = crate::preview::HIGHLIGHT_COLORS
        .iter()
        .filter(|(_, name)| name.to_lowercase().starts_with(&query))
        .map(|(emoji, name)| CompletionItem {
            label: format!("{emoji} {name} highlight"),
            kind: Some(CompletionItemKind::COLOR),
            filter_text: Some(name.to_lowercase()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: emoji.to_string(),
            })),
            ..Default::default()
        })
        .collect();
    (!items.is_empty()).then_some(CompletionResponse::Array(items))
}

fn emoji_items(text: &Rope, offset: usize) -> Option<CompletionResponse> {
    let point = text.offset_to_point(offset);
    let line = text.slice_line(point.row).to_string();
    let line_start = text.line_start_offset(point.row);
    let prefix = line.get(..point.column.min(line.len())).unwrap_or_default();

    let colon = prefix.rfind(':')?;
    if prefix[..colon]
        .chars()
        .last()
        .is_some_and(|c| !c.is_whitespace())
    {
        return None;
    }
    let query = &prefix[colon + 1..];
    if query.len() < 2
        || !query
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let q = query.to_lowercase();
    let range = Range {
        start: text.offset_to_position(line_start + colon),
        end: text.offset_to_position(offset),
    };

    let mut hits: Vec<&(&str, &str)> = crate::emoji::EMOJI
        .iter()
        .filter(|(name, _)| name.contains(q.as_str()))
        .collect();
    // Prefix matches float to the top; keep the menu short.
    hits.sort_by(|(a, _), (b, _)| {
        (!a.starts_with(q.as_str()), *a).cmp(&(!b.starts_with(q.as_str()), *b))
    });
    let items: Vec<CompletionItem> = hits
        .into_iter()
        .take(24)
        .map(|(name, ch)| CompletionItem {
            label: format!("{ch} :{name}:"),
            detail: Some(name.replace('_', " ")),
            kind: Some(CompletionItemKind::TEXT),
            sort_text: Some((*name).to_string()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: (*ch).to_string(),
            })),
            ..Default::default()
        })
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

/// `/query` at line start: the built-in blocks, then the vault's
/// templates (`templates` is only called once the line qualifies).
fn slash_items(
    text: &Rope,
    offset: usize,
    templates: impl FnOnce() -> Vec<(String, String)>,
) -> CompletionResponse {
    let point = text.offset_to_point(offset);
    let line = text.slice_line(point.row).to_string();
    let line_start = text.line_start_offset(point.row);
    let prefix = line.get(..point.column.min(line.len())).unwrap_or_default();

    let Some(query) = prefix.strip_prefix('/') else {
        return CompletionResponse::Array(vec![]);
    };
    if !query
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ')
    {
        return CompletionResponse::Array(vec![]);
    }
    let q = query.to_lowercase();
    let range = Range {
        start: text.offset_to_position(line_start),
        end: text.offset_to_position(offset),
    };

    CompletionResponse::Array(
        ITEMS
            .iter()
            .filter(|item| {
                q.is_empty()
                    || item.label.to_lowercase().contains(&q)
                    || item.keywords.contains(q.as_str())
            })
            .map(|item| CompletionItem {
                label: item.label.to_string(),
                detail: Some(item.detail.to_string()),
                kind: Some(CompletionItemKind::SNIPPET),
                filter_text: Some(format!("{} {}", item.label, item.keywords)),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: item.text.to_string(),
                })),
                ..Default::default()
            })
            .chain(
                templates()
                    .into_iter()
                    .filter(|(name, _)| {
                        q.is_empty()
                            || name.to_lowercase().contains(&q)
                            || "template".contains(q.as_str())
                    })
                    .map(|(name, text)| CompletionItem {
                        filter_text: Some(format!("{name} template")),
                        label: name,
                        detail: Some("Template".to_string()),
                        kind: Some(CompletionItemKind::FILE),
                        text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                            range,
                            new_text: text,
                        })),
                        ..Default::default()
                    }),
            )
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::highlight_color_items;
    use gpui_kit::component::input::Rope;
    use lsp_types::CompletionResponse;

    fn labels(text: &str) -> Vec<String> {
        match highlight_color_items(&Rope::from(text), text.len()) {
            Some(CompletionResponse::Array(items)) => items.into_iter().map(|i| i.label).collect(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn highlight_colors_follow_an_opening_delimiter() {
        assert_eq!(labels("note ==r"), ["🔴 Red highlight"]);
        assert_eq!(labels("==B"), ["🔵 Blue highlight"]);
        assert!(labels("==rea").is_empty());
        assert!(labels("==done== r").is_empty());
        assert!(labels("`==` r").is_empty());
        assert!(labels("==").is_empty());
    }
}
