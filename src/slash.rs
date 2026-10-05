//! Slash commands: `/` at the start of a line opens the editor's completion
//! menu with markdown block templates — Notion-style, plain markdown out.

use gpui_kit::component::input::{CompletionProvider, Rope, RopeExt};
use gpui_kit::{App, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Range, TextEdit,
};

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
/// `/` and only offers items when the line so far is exactly `/query`.
pub struct SlashCommands;

impl CompletionProvider for SlashCommands {
    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        // `completions` decides for real — be permissive here so the query
        // keeps updating as the user types `/word`. `/` is allowed so a
        // batched insert like `/h` still triggers.
        new_text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' || c == '/')
    }

    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let _ = cx;
        Task::ready(Ok(items(text, offset)))
    }
}

fn items(text: &Rope, offset: usize) -> CompletionResponse {
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
