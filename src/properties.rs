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

/// The frontmatter body of `text` with `key` set to `value` (`Null`
/// removes the key), serialized back to YAML. `None` when the block
/// would be left empty. Existing keys keep their order; new keys append.
pub fn body_with(text: &str, key: &str, value: Value) -> Option<String> {
    let mut map = Mapping::new();
    for (k, v) in properties(text) {
        map.insert(Value::String(k), v);
    }
    if matches!(value, Value::Null) {
        map.remove(Value::String(key.into()));
    } else {
        map.insert(Value::String(key.into()), value);
    }
    if map.is_empty() {
        None
    } else {
        serde_yaml::to_string(&map).ok()
    }
}

/// Whole-document equivalent of `Document::set_properties` for notes
/// that aren't open in an editor — splices a new frontmatter block into
/// `text` (`None` removes it) and returns the whole file.
pub fn splice_frontmatter(text: &str, body: Option<String>) -> String {
    match (frontmatter_span(text), body) {
        (Some(span), Some(body)) => {
            let block = format!("---\n{}\n---\n", body.trim_end_matches('\n'));
            format!("{}{}{}", &text[..span.start], block, &text[span.end..])
        }
        (Some(mut span), None) => {
            if text[span.end..].starts_with('\n') {
                span.end += 1;
            }
            format!("{}{}", &text[..span.start], &text[span.end..])
        }
        (None, Some(body)) => format!("---\n{}\n---\n\n{text}", body.trim_end_matches('\n')),
        (None, None) => text.to_string(),
    }
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
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
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

/// All of one note's tags — `tags:` frontmatter plus inline `#tag`s,
/// deduplicated, bare names (no `#`). Bases expose this as `file.tags`.
pub fn note_tags(text: &str) -> Vec<String> {
    let mut found = std::collections::BTreeSet::new();
    for tag in frontmatter_tags(text) {
        found.insert(tag);
    }
    for tag in inline_tags(text) {
        found.insert(tag);
    }
    found.into_iter().collect()
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

/// An open `- [ ]` checkbox somewhere in the vault — one sidebar row.
#[derive(Debug, Clone)]
pub struct VaultTask {
    pub path: PathBuf,
    /// 1-based source line — the click target.
    pub line: usize,
    /// Task text after the marker.
    pub text: String,
}

/// All open `- [ ]` checkboxes across the vault, in file order —
/// the sidebar Tasks index. Skips fenced blocks and `%%` comment
/// regions like the tag scan; done markers (`[x]`, `[/]`, …) don't
/// count.
pub fn vault_tasks(notes: &[PathBuf]) -> Vec<VaultTask> {
    let mut out = Vec::new();
    for path in notes {
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut in_fence = false;
        let mut fence_marker = "";
        let mut in_comment = false;
        for (ix, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                let marker = &trimmed[..3];
                if !in_fence {
                    in_fence = true;
                    fence_marker = marker;
                } else if marker == fence_marker {
                    in_fence = false;
                }
                continue;
            }
            if in_fence {
                continue;
            }
            // Inside a `%%` region a `%%` closes it; nothing else counts.
            if in_comment {
                if trimmed.contains("%%") {
                    in_comment = false;
                }
                continue;
            }
            if let Some(text) = task_line_text(trimmed) {
                out.push(VaultTask {
                    path: path.clone(),
                    line: ix + 1,
                    text,
                });
            }
            // An odd number of `%%` on the line leaves a comment open.
            if trimmed.matches("%%").count() % 2 == 1 {
                in_comment = true;
            }
        }
    }
    out
}

/// The text after `- [ ]` / `* [ ]` / `+ [ ]`, or `None` when the line
/// isn't an open task (`[ ]x` without a space isn't a marker either).
fn task_line_text(trimmed: &str) -> Option<String> {
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;
    let text = rest.strip_prefix("[ ]")?;
    if !text.is_empty() && !text.starts_with(' ') {
        return None;
    }
    Some(text.trim().to_string())
}

/// `aliases:` entries from frontmatter — sequence or comma-separated
/// scalar, same shapes `vault_tags` accepts.
pub fn frontmatter_aliases(text: &str) -> Vec<String> {
    properties(text)
        .into_iter()
        .find(|(key, _)| key == "aliases" || key == "alias")
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

/// Inline `#tags` outside fenced code, inline `code` and `%%` comments —
/// same state machine `preview::rewrite_line` uses so the index and the
/// rendered links agree on what counts as a tag.
fn inline_tags(text: &str) -> Vec<String> {
    inline_tag_spans(text)
        .into_iter()
        .map(|(_, tag)| tag)
        .collect()
}

/// Byte range + name of every inline `#tag` — the tag index maps to
/// names; tag renames edit the ranges directly.
fn inline_tag_spans(text: &str) -> Vec<(Range<usize>, String)> {
    let mut tags = Vec::new();
    let mut in_fence = false;
    let mut in_comment = false;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let bytes = line.as_bytes();
        let mut ix = 0;
        let mut in_code = false;
        // `ix` always sits on a char boundary — it advances by len_utf8.
        while ix < bytes.len() {
            let ch = line[ix..].chars().next().unwrap();
            if in_comment {
                if line[ix..].starts_with("%%") {
                    in_comment = false;
                    ix += 1;
                }
                ix += ch.len_utf8();
                continue;
            }
            if ch == '`' {
                in_code = !in_code;
                ix += 1;
                continue;
            }
            if in_code {
                ix += ch.len_utf8();
                continue;
            }
            if line[ix..].starts_with("%%") {
                in_comment = true;
                ix += 2;
                continue;
            }
            // `#anchor`s inside `[[wikilinks]]` and `[l](urls)` are link
            // targets, not tags.
            if line[ix..].starts_with("[[") {
                ix += line[ix + 2..].find("]]").map(|e| 2 + e + 2).unwrap_or(2);
                continue;
            }
            if line[ix..].starts_with("](") {
                ix += line[ix + 2..].find(')').map(|e| 2 + e + 1).unwrap_or(2);
                continue;
            }
            let bounded = ix == 0
                || bytes[ix - 1].is_ascii_whitespace()
                || (bytes[ix - 1].is_ascii_punctuation() && bytes[ix - 1] != b'#');
            if ch == '#' && bounded {
                let rest = &line[ix + 1..];
                let tag_len: usize = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '/')
                    .map(char::len_utf8)
                    .sum();
                let tag = &rest[..tag_len];
                // `#123`-style digits, `#` runs and trailing `/` aren't tags.
                let first = tag.chars().next();
                if !tag.is_empty()
                    && !tag.ends_with('/')
                    && first.is_some_and(|c| c.is_alphabetic() || c == '_')
                {
                    tags.push((offset + ix..offset + ix + 1 + tag_len, tag.to_string()));
                }
                ix += 1 + tag_len;
            } else {
                ix += ch.len_utf8();
            }
        }
        offset += line.len();
    }
    tags
}

/// Edits renaming tag `old` → `new` throughout `text`: inline `#old`
/// and `#old/sub` (→ `#new`/`#new/sub`) plus `tags:` frontmatter
/// entries written without the `#`. Tag characters only — `old` must
/// already be a bare tag name.
pub fn tag_rename_edits(text: &str, old: &str, new: &str) -> Vec<(Range<usize>, String)> {
    let mut edits = Vec::new();
    for (span, tag) in inline_tag_spans(text) {
        if tag == old {
            edits.push((span.start + 1..span.end, new.to_string()));
        } else if let Some(suffix) = tag.strip_prefix(&format!("{old}/")) {
            edits.push((span.start + 1..span.end, format!("{new}/{suffix}")));
        }
    }
    // `tags:` frontmatter — same swap on each bare token (the `- `
    // list item or `tags: [a, b]` inline list), nested `old/x` kept.
    if let Some(fm) = frontmatter_span(text) {
        let mut offset = fm.start;
        let mut in_tags = false;
        for line in text[fm].split_inclusive('\n') {
            let t = line.trim_start();
            if in_tags && (t.starts_with("- ") || t.trim_end().is_empty()) {
                tag_token_edits(line, offset, old, new, &mut edits);
            } else if t.starts_with("tags:") {
                in_tags = true;
                tag_token_edits(line, offset, old, new, &mut edits);
            } else if !t.trim_end().is_empty() {
                in_tags = false;
            }
            offset += line.len();
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    edits
}

/// Token-bounded `old`/`old/sub` → `new`/`new/sub` inside `slice` —
/// scans tag characters only, so `,`, `]`, `'` and `- ` boundaries are
/// left alone.
fn tag_token_edits(
    slice: &str,
    base: usize,
    old: &str,
    new: &str,
    out: &mut Vec<(Range<usize>, String)>,
) {
    let mut i = 0;
    while i < slice.len() {
        let start = i;
        while i < slice.len() {
            let c = slice[i..].chars().next().unwrap();
            if !(c.is_alphanumeric() || c == '-' || c == '_' || c == '/') {
                break;
            }
            i += c.len_utf8();
        }
        if i == start {
            i += slice[i..].chars().next().unwrap().len_utf8();
            continue;
        }
        let token = &slice[start..i];
        if let Some(suffix) = token
            .strip_prefix(old)
            .filter(|s| s.is_empty() || s.starts_with('/'))
        {
            out.push((base + start..base + i, format!("{new}{suffix}")));
        }
    }
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
            map.insert(Value::String(key), value_from_text(row.kind, &raw));
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
