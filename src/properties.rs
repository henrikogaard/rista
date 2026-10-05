//! Frontmatter properties: an Obsidian-style key/value editor that
//! round-trips through the note's `--- YAML ---` block, plus a vault-wide
//! tag index (`tags:` frontmatter + inline `#tags`).

use crate::document::Document;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, WindowExt};
use gpui_kit::*;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

// ------------------------------------------------------------------
// Frontmatter parsing / serialization
// ------------------------------------------------------------------

/// Byte range covering the whole `---\n…\n---` frontmatter block:
/// the opening delimiter through the closing delimiter's newline.
pub fn frontmatter_span(text: &str) -> Option<Range<usize>> {
    let first_nl = text.find('\n')?;
    if text[..first_nl].trim_end() != "---" {
        return None;
    }
    let mut offset = first_nl + 1;
    for line in text[offset..].split_inclusive('\n') {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            return Some(0..offset + line.len());
        }
        offset += line.len();
    }
    None
}

/// YAML body inside the frontmatter block (between the delimiters).
fn frontmatter_yaml(text: &str) -> Option<String> {
    let span = frontmatter_span(text)?;
    let mut body = String::new();
    let mut seen_open = false;
    for line in text[span].lines() {
        let trimmed = line.trim_end();
        if !seen_open {
            seen_open = trimmed == "---";
            continue;
        }
        if trimmed == "---" || trimmed == "..." {
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    Some(body)
}

/// Ordered top-level `key → value` pairs from the frontmatter block.
pub fn properties(text: &str) -> Vec<(String, Value)> {
    frontmatter_yaml(text)
        .and_then(|body| serde_yaml::from_str::<Mapping>(&body).ok())
        .map(|map| {
            map.into_iter()
                .filter_map(|(key, value)| key.as_str().map(|k| (k.to_string(), value.clone())))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq)]
enum PropKind {
    Text,
    Bool,
    Number,
    List,
    Other,
}

fn kind_of(value: &Value) -> PropKind {
    match value {
        Value::Bool(_) => PropKind::Bool,
        Value::Number(_) => PropKind::Number,
        Value::Sequence(_) => PropKind::List,
        Value::String(_) | Value::Null => PropKind::Text,
        _ => PropKind::Other,
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => serde_yaml::to_string(other)
            .map(|s| s.trim_end().to_string())
            .unwrap_or_default(),
    }
}

/// Display form: lists become `a, b, c`, scalars their plain text.
fn display_value(value: &Value) -> String {
    match value {
        Value::Sequence(items) => items.iter().map(scalar_text).collect::<Vec<_>>().join(", "),
        other => scalar_text(other),
    }
}

/// Parse edited text back into a YAML value of the original kind.
/// Values that no longer parse as their kind fall back to strings.
fn value_from_text(kind: PropKind, raw: &str) -> Value {
    let raw = raw.trim();
    match kind {
        PropKind::Bool => raw
            .parse::<bool>()
            .map(Value::Bool)
            .unwrap_or_else(|_| Value::String(raw.into())),
        PropKind::Number => raw
            .parse::<i64>()
            .map(|n| Value::Number(n.into()))
            .or_else(|_| raw.parse::<f64>().map(|n| Value::Number(n.into())))
            .unwrap_or_else(|_| Value::String(raw.into())),
        PropKind::List => Value::Sequence(
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|item| {
                    serde_yaml::from_str::<Value>(item)
                        .unwrap_or_else(|_| Value::String(item.to_string()))
                })
                .collect(),
        ),
        PropKind::Other => {
            serde_yaml::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.into()))
        }
        PropKind::Text => Value::String(raw.into()),
    }
}

// ------------------------------------------------------------------
// Vault tag index — frontmatter `tags:` plus inline `#tags`.
// ------------------------------------------------------------------

/// `(tag, notes_containing_it)` pairs, sorted by tag name.
pub fn vault_tags(notes: &[PathBuf]) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for path in notes {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut found = std::collections::BTreeSet::new();
        for tag in frontmatter_tags(&text) {
            found.insert(tag);
        }
        for tag in inline_tags(&text) {
            found.insert(tag);
        }
        for tag in found {
            *counts.entry(tag).or_default() += 1;
        }
    }
    counts.into_iter().collect()
}

/// `tags:` entries from frontmatter — sequence, `[a, b]`, or
/// comma-separated scalar all count.
fn frontmatter_tags(text: &str) -> Vec<String> {
    properties(text)
        .into_iter()
        .find(|(key, _)| key == "tags")
        .map(|(_, value)| match value {
            Value::Sequence(items) => items.iter().map(scalar_text).collect::<Vec<_>>(),
            other => scalar_text(&other)
                .split(',')
                .map(|s| s.trim().to_string())
                .collect(),
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.is_empty())
        .collect()
}

/// `#tag` tokens outside fenced code blocks. A tag is `#` not preceded
/// by a word character, followed by `[\w/-]+` (obsidian-style nesting).
fn inline_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let bytes = line.as_bytes();
        let mut ix = 0;
        while ix < bytes.len() {
            if bytes[ix] == b'#'
                && (ix == 0 || !is_tag_char(bytes[ix - 1]))
                && ix + 1 < bytes.len()
                && is_tag_char(bytes[ix + 1])
            {
                let start = ix + 1;
                let mut end = start;
                while end < bytes.len() && is_tag_char(bytes[end]) {
                    end += 1;
                }
                let tag = &line[start..end];
                // `#` followed only by digits is a heading anchor/id, not a tag.
                if !tag.bytes().all(|b| b.is_ascii_digit()) {
                    tags.push(tag.to_string());
                }
                ix = end;
            } else {
                ix += 1;
            }
        }
    }
    tags
}

fn is_tag_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'/' || b == b'-'
}

// ------------------------------------------------------------------
// Properties panel — rows of key/value inputs, applied on commit.
// ------------------------------------------------------------------

struct PropRow {
    key: Entity<InputState>,
    value: Entity<InputState>,
    kind: PropKind,
}

pub struct PropertiesPanel {
    doc: Entity<Document>,
    rows: Vec<PropRow>,
}

impl PropertiesPanel {
    fn new(doc: Entity<Document>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = doc.read(cx).editor.read(cx).value().to_string();
        let rows = properties(&text)
            .into_iter()
            .map(|(key, value)| {
                Self::make_row(&key, &display_value(&value), kind_of(&value), window, cx)
            })
            .collect();
        Self { doc, rows }
    }

    fn make_row(
        key: &str,
        value: &str,
        kind: PropKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> PropRow {
        let key_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("name");
            state.set_value(key.to_string(), window, cx);
            state
        });
        let value_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("value");
            state.set_value(value.to_string(), window, cx);
            state
        });
        PropRow {
            key: key_input,
            value: value_input,
            kind,
        }
    }

    fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rows
            .push(Self::make_row("", "", PropKind::Text, window, cx));
        cx.notify();
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut map = Mapping::new();
        for row in &self.rows {
            let key = row.key.read(cx).value().trim().to_string();
            if key.is_empty() {
                continue;
            }
            let raw = row.value.read(cx).value();
            map.insert(Value::String(key.into()), value_from_text(row.kind, &raw));
        }
        let body = if map.is_empty() {
            None
        } else {
            serde_yaml::to_string(&map).ok()
        };
        self.doc
            .update(cx, |doc, cx| doc.set_properties(body, window, cx));
        window.close_dialog(cx);
    }
}

impl Render for PropertiesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let this = cx.entity();
        let mut list = v_flex().w_full().gap_1();
        for (ix, row) in self.rows.iter().enumerate() {
            list = list.child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        div()
                            .w(px(150.))
                            .child(Input::new(&row.key).appearance(true)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&row.value).appearance(true)),
                    )
                    .child(
                        div()
                            .id(("prop-delete", ix))
                            .flex_none()
                            .cursor_pointer()
                            .text_color(theme.muted_foreground)
                            .hover(|s| s.text_color(theme.foreground))
                            .child("×")
                            .on_click({
                                let this = this.clone();
                                move |_, _window, cx| {
                                    this.update(cx, |panel, cx| {
                                        panel.rows.remove(ix);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            );
        }
        v_flex()
            .w_full()
            .gap_2()
            .child(
                gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                    list.max_h(px(320.)),
                ),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .child(
                        Button::new("add-property")
                            .ghost()
                            .icon(assets::IconName::Plus)
                            .label("Add property")
                            .on_click({
                                let this = this.clone();
                                move |_, window, cx| {
                                    this.update(cx, |panel, cx| panel.add_row(window, cx));
                                }
                            }),
                    )
                    .child(
                        Button::new("apply-properties")
                            .primary()
                            .label("Apply")
                            .on_click({
                                let this = this.clone();
                                move |_, window, cx| {
                                    this.update(cx, |panel, cx| panel.apply(window, cx));
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Lists are comma-separated; booleans and numbers keep their type."),
            )
    }
}

/// Open the properties dialog for `doc`.
pub fn open_properties(doc: Entity<Document>, window: &mut Window, cx: &mut App) {
    let panel = cx.new(|cx| PropertiesPanel::new(doc, window, cx));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title("Properties")
            .w(px(520.))
            .overlay_closable(true)
            .child(panel.clone())
    });
}
