//! Frontmatter properties: an key/value editor that
//! round-trips through the note's `--- YAML ---` block, plus a vault-wide
//! tag index (`tags:` frontmatter + inline `#tags`).

use crate::document::Document;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState, DateTime};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable, WindowExt};
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

#[derive(Clone, Copy, PartialEq, Debug)]
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

/// A note's first tag as written — the first `tags:` entry, else the
/// first inline `#tag`. The graph groups tag colors by it.
pub fn first_tag(text: &str) -> Option<String> {
    frontmatter_tags(text)
        .into_iter()
        .next()
        .or_else(|| inline_tags(text).into_iter().next())
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

/// `cssclasses:`/`cssclass:` frontmatter — the per-note styling
/// hook. Accepts a YAML list or a space/comma-separated string.
pub fn frontmatter_cssclasses(text: &str) -> Vec<String> {
    fn split(s: &str) -> Vec<String> {
        s.split(|c: char| c.is_whitespace() || c == ',')
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    }
    properties(text)
        .into_iter()
        .find(|(key, _)| key == "cssclasses" || key == "cssclass")
        .map(|(_, value)| match value {
            Value::Sequence(items) => items.iter().flat_map(|i| split(&scalar_text(i))).collect(),
            other => split(&scalar_text(&other)),
        })
        .unwrap_or_default()
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
// Vault property schema — inferred controls and value suggestions.
// ------------------------------------------------------------------

/// The control a property row offers, inferred from how the vault uses the key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropertyControl {
    /// A plain text field.
    Plain,
    /// A true/false toggle.
    Bool,
    /// A date picker.
    Date,
    /// A text field with the key's known values offered as chips.
    Select,
}

/// Per-key controls and value suggestions, inferred from the vault's notes.
#[derive(Clone, Debug, Default)]
pub struct PropertySchema {
    /// Every key seen in frontmatter, sorted.
    pub keys: Vec<String>,
    controls: BTreeMap<String, PropertyControl>,
    suggestions: BTreeMap<String, Vec<String>>,
}

/// Distinct values offered before a text key stops looking like a choice.
const MAX_SUGGESTIONS: usize = 12;
/// Values longer than this are free text, not choices.
const MAX_SUGGESTION_CHARS: usize = 40;

impl PropertySchema {
    /// The control to use for `key`, falling back to the current value's own
    /// kind when the vault has not seen the key before.
    pub fn control_for(&self, key: &str, value: &Value) -> PropertyControl {
        if let Some(control) = self.controls.get(key) {
            return *control;
        }
        match value {
            Value::Bool(_) => PropertyControl::Bool,
            Value::String(text) if parse_iso_date(text).is_some() => PropertyControl::Date,
            _ => PropertyControl::Plain,
        }
    }

    /// Known values for a text key, for use as chips. Empty when unknown.
    pub fn suggestions_for(&self, key: &str) -> &[String] {
        self.suggestions.get(key).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Controls and suggestions inferred from the notes' frontmatter.
pub fn vault_schema(notes: &[PathBuf]) -> PropertySchema {
    let mut tallies: BTreeMap<String, KindTally> = BTreeMap::new();
    for path in notes {
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for (key, value) in properties(&text) {
            tallies.entry(key).or_default().observe(&value);
        }
    }
    let mut schema = PropertySchema::default();
    for (key, tally) in tallies {
        schema.controls.insert(key.clone(), tally.control());
        if let Some(values) = tally.suggestions() {
            schema.suggestions.insert(key.clone(), values);
        }
        schema.keys.push(key);
    }
    schema
}

/// `YYYY-MM-DD` or `YYYY/MM/DD`, the shapes date properties use.
fn parse_iso_date(text: &str) -> Option<chrono::NaiveDate> {
    let text = text.trim();
    ["%Y-%m-%d", "%Y/%m/%d"]
        .into_iter()
        .find_map(|format| chrono::NaiveDate::parse_from_str(text, format).ok())
}

#[derive(Default)]
struct KindTally {
    bools: usize,
    dates: usize,
    texts: usize,
    scalars: usize,
    values: BTreeMap<String, usize>,
}

impl KindTally {
    fn observe(&mut self, value: &Value) {
        match value {
            Value::Bool(_) => self.bools += 1,
            Value::String(text) => {
                if parse_iso_date(text).is_some() {
                    self.dates += 1;
                } else {
                    self.texts += 1;
                }
                if text.chars().count() <= MAX_SUGGESTION_CHARS {
                    *self.values.entry(text.clone()).or_default() += 1;
                }
            }
            Value::Null => {}
            Value::Number(_) | Value::Sequence(_) | Value::Mapping(_) | Value::Tagged(_) => {
                self.scalars += 1
            }
        }
    }

    fn control(&self) -> PropertyControl {
        let (count, control) = [
            (self.bools, PropertyControl::Bool),
            (self.dates, PropertyControl::Date),
            (self.texts, PropertyControl::Select),
            (self.scalars, PropertyControl::Plain),
        ]
        .into_iter()
        .max_by_key(|(count, _)| *count)
        .unwrap();
        if count == 0 {
            return PropertyControl::Plain;
        }
        match control {
            PropertyControl::Select if !(2..=MAX_SUGGESTIONS).contains(&self.values.len()) => {
                PropertyControl::Plain
            }
            control => control,
        }
    }

    fn suggestions(&self) -> Option<Vec<String>> {
        if self.values.is_empty() || self.values.len() > MAX_SUGGESTIONS {
            return None;
        }
        let mut ranked = self
            .values
            .iter()
            .map(|(value, count)| (value.clone(), *count))
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        Some(ranked.into_iter().map(|(value, _)| value).collect())
    }
}

/// The parse kind a control writes back as: booleans stay booleans, dates and
/// selects are strings (or lists), everything else keeps the value's own kind.
fn kind_for(control: PropertyControl, value: &Value) -> PropKind {
    match control {
        PropertyControl::Bool => PropKind::Bool,
        PropertyControl::Date => PropKind::Text,
        PropertyControl::Select => match value {
            Value::Sequence(_) => PropKind::List,
            _ => PropKind::Text,
        },
        PropertyControl::Plain => kind_of(value),
    }
}

/// The parse kind for a row: an existing value keeps its own YAML type, and a
/// key with no value yet takes the control's kind.
fn row_kind(control: PropertyControl, value: &Value) -> PropKind {
    match value {
        Value::Null => kind_for(control, value),
        _ => kind_of(value),
    }
}

// ------------------------------------------------------------------
// Properties panel — rows of typed key/value controls, applied on commit.
// ------------------------------------------------------------------

struct PropRow {
    key: Entity<InputState>,
    value: Entity<InputState>,
    kind: PropKind,
    control: PropertyControl,
    date_state: Option<Entity<DatePickerState>>,
}

pub struct PropertiesPanel {
    doc: Entity<Document>,
    rows: Vec<PropRow>,
    schema: PropertySchema,
    _subscriptions: Vec<Subscription>,
}

impl PropertiesPanel {
    fn new(
        doc: Entity<Document>,
        schema: PropertySchema,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = doc.read(cx).editor.read(cx).value().to_string();
        let mut subscriptions = Vec::new();
        let rows = properties(&text)
            .into_iter()
            .map(|(key, value)| {
                let control = schema.control_for(&key, &value);
                // Keep an existing value's own YAML type; infer the type from
                // the control only when the note has no value yet. Otherwise a
                // mixed-type key (e.g. one note with `status: 42`) would be
                // rewritten as a string just by applying the dialog.
                let kind = row_kind(control, &value);
                Self::make_row(
                    &key,
                    &display_value(&value),
                    kind,
                    control,
                    window,
                    cx,
                    &mut subscriptions,
                )
            })
            .collect();
        Self {
            doc,
            rows,
            schema,
            _subscriptions: subscriptions,
        }
    }

    fn make_row(
        key: &str,
        value: &str,
        kind: PropKind,
        control: PropertyControl,
        window: &mut Window,
        cx: &mut Context<Self>,
        subscriptions: &mut Vec<Subscription>,
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
        let date_state = (control == PropertyControl::Date).then(|| {
            let state = cx.new(|cx| DatePickerState::new(window, cx));
            if let Some(date) = parse_iso_date(value) {
                state.update(cx, |state, cx| state.set_date(date, window, cx));
            }
            subscriptions.push(cx.subscribe_in(&state, window, {
                let value_input = value_input.clone();
                move |_panel, _state, event: &DatePickerEvent, window, cx| {
                    if let DatePickerEvent::Change(DateTime::Single(Some(when))) = event {
                        let iso = when.date().format("%Y-%m-%d").to_string();
                        value_input.update(cx, |input, cx| input.set_value(iso, window, cx));
                    }
                }
            }));
            state
        });
        PropRow {
            key: key_input,
            value: value_input,
            kind,
            control,
            date_state,
        }
    }

    fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut subscriptions = Vec::new();
        let row = Self::make_row(
            "",
            "",
            PropKind::Text,
            PropertyControl::Plain,
            window,
            cx,
            &mut subscriptions,
        );
        self.rows.push(row);
        self._subscriptions.extend(subscriptions);
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
        let mut list = v_flex().w_full().gap_2();
        for (ix, row) in self.rows.iter().enumerate() {
            let key_text = row.key.read(cx).value().trim().to_string();
            let key_suggestions = if key_text.is_empty() {
                self.schema.keys.clone()
            } else {
                Vec::new()
            };
            let select_suggestions = if row.control == PropertyControl::Select {
                self.schema.suggestions_for(&key_text).to_vec()
            } else {
                Vec::new()
            };

            let mut value_cell = v_flex().w_full().gap_1();
            match row.control {
                PropertyControl::Bool => {
                    let on = row
                        .value
                        .read(cx)
                        .value()
                        .trim()
                        .eq_ignore_ascii_case("true");
                    let target = row.value.clone();
                    value_cell = value_cell.child(
                        Button::new(("prop-bool", ix))
                            .small()
                            .icon(if on {
                                assets::IconName::Check
                            } else {
                                assets::IconName::X
                            })
                            .label(if on { "true" } else { "false" })
                            .on_click({
                                let this = this.clone();
                                move |_, window, cx| {
                                    let next = if on { "false" } else { "true" };
                                    target
                                        .update(cx, |input, cx| input.set_value(next, window, cx));
                                    // The label and `on` are captured per render;
                                    // repaint so the next click toggles back.
                                    this.update(cx, |_panel, cx| cx.notify());
                                }
                            }),
                    );
                }
                PropertyControl::Date => {
                    if let Some(state) = &row.date_state {
                        value_cell = value_cell.child(div().w_full().child(DatePicker::new(state)));
                    }
                }
                PropertyControl::Select | PropertyControl::Plain => {
                    value_cell = value_cell.child(Input::new(&row.value).appearance(true));
                }
            }
            if !select_suggestions.is_empty() {
                let target = row.value.clone();
                value_cell = value_cell.child(
                    h_flex().w_full().flex_wrap().gap_1().children(
                        select_suggestions
                            .into_iter()
                            .enumerate()
                            .map(move |(chip_ix, value)| {
                                let target = target.clone();
                                let picked = value.clone();
                                Button::new(("prop-value-chip", ix * 100 + chip_ix))
                                    .ghost()
                                    .xsmall()
                                    .label(picked.clone())
                                    .on_click(move |_, window, cx| {
                                        target.update(cx, |input, cx| {
                                            input.set_value(picked.clone(), window, cx)
                                        });
                                    })
                                    .into_any_element()
                            }),
                    ),
                );
            }

            let mut key_cell = v_flex()
                .w(px(150.))
                .gap_1()
                .child(Input::new(&row.key).appearance(true));
            if !key_suggestions.is_empty() {
                let target = row.key.clone();
                key_cell = key_cell.child(
                    h_flex().w_full().flex_wrap().gap_1().children(
                        key_suggestions
                            .into_iter()
                            .take(MAX_SUGGESTIONS)
                            .enumerate()
                            .map(move |(chip_ix, value)| {
                                let target = target.clone();
                                let picked = value.clone();
                                Button::new(("prop-key-chip", ix * 100 + chip_ix))
                                    .ghost()
                                    .xsmall()
                                    .label(picked.clone())
                                    .on_click(move |_, window, cx| {
                                        target.update(cx, |input, cx| {
                                            input.set_value(picked.clone(), window, cx)
                                        });
                                    })
                                    .into_any_element()
                            }),
                    ),
                );
            }

            list = list.child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_start()
                    .child(key_cell)
                    .child(div().flex_1().child(value_cell))
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
                    .child("Booleans toggle; dates use a picker; lists are comma-separated."),
            )
    }
}

/// Open the properties dialog for `doc`, using `schema` for typed controls.
pub fn open_properties(
    doc: Entity<Document>,
    schema: PropertySchema,
    window: &mut Window,
    cx: &mut App,
) {
    let panel = cx.new(|cx| PropertiesPanel::new(doc, schema, window, cx));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title("Properties")
            .w(px(560.))
            .overlay_closable(true)
            .child(panel.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::{row_kind, vault_schema, PropKind, PropertyControl};
    use serde_yaml::Value;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_vault() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rista-properties-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn schema_infers_bool_date_and_select_controls() {
        let root = temp_vault();
        std::fs::write(
            root.join("a.md"),
            "---\ndone: true\ndue: 2026-01-02\nstatus: Draft\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(
            root.join("b.md"),
            "---\ndone: false\ndue: 2026-02-03\nstatus: Done\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(root.join("c.md"), "---\nstatus: Draft\n---\nbody\n").unwrap();
        let schema = vault_schema(&[root.join("a.md"), root.join("b.md"), root.join("c.md")]);
        assert_eq!(
            schema.keys,
            vec!["done".to_string(), "due".to_string(), "status".to_string()]
        );
        assert_eq!(
            schema.control_for("done", &Value::Bool(true)),
            PropertyControl::Bool
        );
        assert_eq!(
            schema.control_for("due", &Value::String("2026-01-02".into())),
            PropertyControl::Date
        );
        assert_eq!(
            schema.control_for("status", &Value::String("Draft".into())),
            PropertyControl::Select
        );
        assert_eq!(
            schema.suggestions_for("status").to_vec(),
            vec!["Draft".to_string(), "Done".to_string()]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn many_unique_values_stay_plain() {
        let root = temp_vault();
        let paths = (0..20)
            .map(|index| {
                let path = root.join(format!("n{index}.md"));
                std::fs::write(&path, format!("---\ntitle: Note {index}\n---\nbody\n")).unwrap();
                path
            })
            .collect::<Vec<_>>();
        let schema = vault_schema(&paths);
        assert_eq!(
            schema.control_for("title", &Value::String("Note".into())),
            PropertyControl::Plain
        );
        assert!(schema.suggestions_for("title").is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unknown_keys_fall_back_to_the_value_kind() {
        let schema = vault_schema(&[]);
        assert_eq!(
            schema.control_for("drafted", &Value::Bool(true)),
            PropertyControl::Bool
        );
        assert_eq!(
            schema.control_for("when", &Value::String("2026-03-04".into())),
            PropertyControl::Date
        );
        assert_eq!(
            schema.control_for("note", &Value::String("free".into())),
            PropertyControl::Plain
        );
    }

    #[test]
    fn existing_values_keep_their_own_kind() {
        // A Select control must not retype an existing number.
        assert_eq!(
            row_kind(PropertyControl::Select, &Value::Number(42i64.into())),
            PropKind::Number
        );
        // A Bool control must not retype an existing string.
        assert_eq!(
            row_kind(PropertyControl::Bool, &Value::String("true".into())),
            PropKind::Text
        );
        // A key with no value yet follows the control.
        assert_eq!(
            row_kind(PropertyControl::Bool, &Value::Null),
            PropKind::Bool
        );
        assert_eq!(
            row_kind(PropertyControl::Select, &Value::Null),
            PropKind::Text
        );
    }
}
