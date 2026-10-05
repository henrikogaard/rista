//! Editor completion: `/` at the start of a line opens markdown block
//! templates — Notion-style, plain markdown out. `[[` / `![[` anywhere
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
];

/// Completion provider installed on every document editor. The menu opens on
/// `/` (block templates, only when the line so far is exactly `/query`) and
/// on `[[` / `![[` (note/image links resolved against the live vault).
pub struct VaultCompletions {
    vault: Option<Entity<Vault>>,
}

impl VaultCompletions {
    pub fn new(vault: Option<Entity<Vault>>) -> Self {
        Self { vault }
    }
}

impl CompletionProvider for VaultCompletions {
    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        // `completions` decides for real — be permissive here so the query
        // keeps updating as the user types `/word` or `[[wor`. `[` is
        // allowed so a batched insert like `[[s` still triggers.
        new_text.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' ' | '/' | '[' | '!' | '.' | '#')
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
        Task::ready(Ok(slash_items(text, offset)))
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

fn slash_items(text: &Rope, offset: usize) -> CompletionResponse {
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
            .collect(),
    )
}
