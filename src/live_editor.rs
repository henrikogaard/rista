//! Native Live Preview (WYSIWYG) Markdown editor for Rísta.
//!
//! Renders every block cleanly (true heading scale, interactive checkboxes,
//! bullets, numbered lists, callouts, editorial tables, images, Mermaid, LaTeX,
//! wikilinks, tags, highlights) and edits any block seamlessly in place without
//! raw Markdown boxes or `# ` / `- [ ] ` prefix clutter, syncing continuously
//! back to the canonical [`Document`] buffer.

use crate::app::Workspace;
use crate::document::Document;
use crate::preview;
use gpui_kit::assets;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    self, Editor, EditorState, Input, InputEvent, InputState, TabSize, TextDecorationCollection,
};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use markdown::mdast;
use std::collections::BTreeSet;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Kind of WYSIWYG block in Live Preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Heading(u8),
    Paragraph,
    BulletItem {
        indent: usize,
        marker: char,
    },
    OrderedItem {
        indent: usize,
        number: u64,
        sep: char,
    },
    TaskItem {
        indent: usize,
        marker: char,
        checked: bool,
    },
    Blockquote {
        callout: Option<String>,
    },
    Code,
    Table,
    ThematicBreak,
    Other,
}

impl BlockKind {
    pub fn is_list_item(&self) -> bool {
        matches!(
            self,
            BlockKind::BulletItem { .. }
                | BlockKind::OrderedItem { .. }
                | BlockKind::TaskItem { .. }
        )
    }

    pub fn indent(&self) -> usize {
        match self {
            BlockKind::BulletItem { indent, .. }
            | BlockKind::OrderedItem { indent, .. }
            | BlockKind::TaskItem { indent, .. } => *indent,
            _ => 0,
        }
    }

    pub fn with_indent(&self, next: usize) -> Self {
        match self {
            BlockKind::BulletItem { marker, .. } => BlockKind::BulletItem {
                indent: next,
                marker: *marker,
            },
            BlockKind::OrderedItem { number, sep, .. } => BlockKind::OrderedItem {
                indent: next,
                number: *number,
                sep: *sep,
            },
            BlockKind::TaskItem {
                marker, checked, ..
            } => BlockKind::TaskItem {
                indent: next,
                marker: *marker,
                checked: *checked,
            },
            other => other.clone(),
        }
    }
}

/// One WYSIWYG block mapped back to its exact byte span in the note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveBlock {
    pub kind: BlockKind,
    /// Exact UTF-8 byte range inside the full canonical note string.
    pub range: Range<usize>,
    /// 1-based starting line number in the full canonical note string.
    pub start_line: usize,
    /// Raw Markdown slice (`&raw[range]`).
    pub source: String,
}

/// Parsed Live Preview representation of a Markdown note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveDocument {
    pub frontmatter: Option<(Range<usize>, String)>,
    pub body_start: usize,
    pub blocks: Vec<LiveBlock>,
    /// Link/image reference definitions (`[id]: url`) collected across the note so
    /// individually rendered block fragments resolve reference links and images.
    pub reference_definitions: String,
}

/// A visible block index plus how many subsequent blocks are folded beneath it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleBlock {
    pub index: usize,
    pub collapsed_count: usize,
}

fn line_number_at(raw: &str, byte_offset: usize) -> usize {
    let clamped = byte_offset.min(raw.len());
    raw[..clamped].bytes().filter(|&b| b == b'\n').count() + 1
}

fn detect_callout_kind(source: &str) -> Option<String> {
    let first = source.lines().next()?.trim_start();
    let after_gt = first.strip_prefix('>')?.trim_start();
    let rest = after_gt.strip_prefix("[!")?;
    let end = rest.find(']')?;
    let kind = rest[..end].trim_end_matches(['+', '-']).to_lowercase();
    (!kind.is_empty() && kind.chars().all(|c| c.is_ascii_alphanumeric())).then_some(kind)
}

/// Parse a single Markdown list line prefix (`- [ ] `, `- `, `* `, `+ `, `1. `, `1) `).
/// Returns `(BlockKind, prefix_byte_len)`.
fn parse_list_line_prefix(line: &str) -> Option<(BlockKind, usize)> {
    let trimmed = line.trim_start_matches(' ');
    let indent = line.len() - trimmed.len();

    for marker in ['-', '*', '+'] {
        if let Some(after_bullet) = trimmed
            .strip_prefix(marker)
            .and_then(|s| s.strip_prefix(' '))
        {
            for (tag, checked) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
                if after_bullet.starts_with(tag) {
                    return Some((
                        BlockKind::TaskItem {
                            indent,
                            marker,
                            checked,
                        },
                        indent + 2 + tag.len(),
                    ));
                }
            }
            // Handle bare `- [ ]` / `- [x]` with no trailing space (empty task item).
            for (tag, checked) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
                if after_bullet == tag {
                    return Some((
                        BlockKind::TaskItem {
                            indent,
                            marker,
                            checked,
                        },
                        indent + 2 + tag.len(),
                    ));
                }
            }
            return Some((BlockKind::BulletItem { indent, marker }, indent + 2));
        }
        // Handle bare `-` / `*` / `+` on an empty bullet line.
        if trimmed.len() == 1 && trimmed.starts_with(marker) {
            return Some((BlockKind::BulletItem { indent, marker }, indent + 1));
        }
    }

    let digits_len = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits_len > 0 {
        let rest = &trimmed[digits_len..];
        for sep in ['.', ')'] {
            if let Some(after_sep) = rest.strip_prefix(sep) {
                if after_sep.starts_with(' ') {
                    if let Ok(number) = trimmed[..digits_len].parse::<u64>() {
                        return Some((
                            BlockKind::OrderedItem {
                                indent,
                                number,
                                sep,
                            },
                            indent + digits_len + 2,
                        ));
                    }
                } else if after_sep.is_empty() {
                    if let Ok(number) = trimmed[..digits_len].parse::<u64>() {
                        return Some((
                            BlockKind::OrderedItem {
                                indent,
                                number,
                                sep,
                            },
                            indent + digits_len + 1,
                        ));
                    }
                }
            }
        }
    }

    None
}

/// Split an `mdast::Node::List` span into individual WYSIWYG item blocks with exact byte offsets.
fn split_list_blocks(raw: &str, list_start: usize, list_end: usize) -> Vec<LiveBlock> {
    let slice = &raw[list_start..list_end];
    let mut items: Vec<(BlockKind, usize, usize)> = Vec::new();
    let mut rel = 0usize;

    for raw_line in slice.split_inclusive('\n') {
        let line_start = list_start + rel;
        let line_no_nl = raw_line.trim_end_matches(['\n', '\r']);
        let line_end = line_start + line_no_nl.len();
        rel += raw_line.len();

        if let Some((kind, _)) = parse_list_line_prefix(line_no_nl) {
            items.push((kind, line_start, line_end));
        } else if let Some(last) = items.last_mut() {
            if !line_no_nl.trim().is_empty() {
                last.2 = line_end;
            }
        }
    }

    if items.is_empty() {
        return vec![LiveBlock {
            kind: BlockKind::Paragraph,
            range: list_start..list_end,
            start_line: line_number_at(raw, list_start),
            source: raw[list_start..list_end].to_string(),
        }];
    }

    items
        .into_iter()
        .filter(|(_, s, e)| s <= e && raw.is_char_boundary(*s) && raw.is_char_boundary(*e))
        .map(|(kind, s, e)| LiveBlock {
            kind,
            range: s..e,
            start_line: line_number_at(raw, s),
            source: raw[s..e].to_string(),
        })
        .collect()
}

/// Parse a Markdown note into frontmatter + WYSIWYG blocks with exact byte offsets.
pub fn parse_live_document(raw: &str) -> LiveDocument {
    let (frontmatter, body_start) = match crate::properties::frontmatter_span(raw) {
        Some(span) => (
            Some((span.clone(), raw[span.clone()].to_string())),
            span.end,
        ),
        None => (None, 0),
    };
    let body = &raw[body_start..];
    let mut constructs = markdown::Constructs::gfm();
    constructs.frontmatter = false;
    let options = markdown::ParseOptions {
        constructs,
        ..markdown::ParseOptions::gfm()
    };

    let mut blocks = Vec::new();
    let mut reference_defs: Vec<&str> = Vec::new();
    if let Ok(mdast::Node::Root(root)) = markdown::to_mdast(body, &options) {
        for child in root.children {
            let Some(pos) = child.position() else {
                continue;
            };
            let start = body_start + pos.start.offset;
            let end = (body_start + pos.end.offset).min(raw.len());
            if start >= end || !raw.is_char_boundary(start) || !raw.is_char_boundary(end) {
                continue;
            }
            if matches!(&child, mdast::Node::List(_)) {
                blocks.extend(split_list_blocks(raw, start, end));
                continue;
            }
            if matches!(&child, mdast::Node::Definition(_)) {
                reference_defs.push(&raw[start..end]);
            }
            let source = raw[start..end].to_string();
            let kind = match &child {
                mdast::Node::Heading(h) => BlockKind::Heading(h.depth),
                mdast::Node::Paragraph(_) => BlockKind::Paragraph,
                mdast::Node::Blockquote(_) => BlockKind::Blockquote {
                    callout: detect_callout_kind(&source),
                },
                mdast::Node::Code(_) => BlockKind::Code,
                mdast::Node::Table(_) => BlockKind::Table,
                mdast::Node::ThematicBreak(_) => BlockKind::ThematicBreak,
                _ => BlockKind::Other,
            };
            blocks.push(LiveBlock {
                kind,
                range: start..end,
                start_line: line_number_at(raw, start),
                source,
            });
        }
    }

    LiveDocument {
        frontmatter,
        body_start,
        blocks,
        reference_definitions: reference_defs.join("\n"),
    }
}

/// Append document-level reference definitions (`[id]: url`) to a standalone block
/// fragment so reference-style links (`[text][id]`) and images (`![alt][id]`) resolve.
pub fn with_reference_definitions(fragment: &str, defs: &str) -> String {
    if defs.is_empty() || !fragment.contains('[') {
        return fragment.to_string();
    }
    format!("{fragment}\n\n{defs}")
}

/// Extract clean editable heading text from either ATX (`## Title`, `## Title ##`)
/// or Setext (`Title\n=====`, `Sub\n-----`) heading source.
fn strip_heading_syntax(source: &str) -> String {
    let trimmed = source.trim_start();
    if trimmed.starts_with('#') {
        let after_hashes = trimmed.trim_start_matches('#');
        let body = after_hashes.strip_prefix(' ').unwrap_or(after_hashes);
        let trimmed_end = body.trim_end();
        let without_hashes = trimmed_end.trim_end_matches('#');
        if without_hashes.len() < trimmed_end.len()
            && (without_hashes.is_empty() || without_hashes.ends_with([' ', '\t']))
        {
            return without_hashes.trim_end().to_string();
        }
        return body.to_string();
    }
    let lines: Vec<&str> = source.lines().collect();
    if lines.len() >= 2 {
        let last = lines[lines.len() - 1].trim();
        let is_setext_underline = !last.is_empty()
            && (last.bytes().all(|b| b == b'=') || last.bytes().all(|b| b == b'-'));
        if is_setext_underline {
            return lines[..lines.len() - 1]
                .iter()
                .map(|line| line.trim())
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    source.to_string()
}

/// Strip block-level Markdown syntax prefixes (`# `, Setext underlines, `- [ ] `, `- `, `1. `, `> `)
/// so the user edits clean WYSIWYG text directly without seeing raw prefix syntax.
pub fn block_editable_body(kind: &BlockKind, source: &str) -> String {
    match kind {
        BlockKind::Heading(_) => strip_heading_syntax(source),
        BlockKind::BulletItem { .. }
        | BlockKind::OrderedItem { .. }
        | BlockKind::TaskItem { .. } => {
            let first_line = source.lines().next().unwrap_or("");
            if let Some((_, prefix_len)) = parse_list_line_prefix(first_line) {
                source.get(prefix_len..).unwrap_or("").to_string()
            } else {
                source.to_string()
            }
        }
        BlockKind::Blockquote { callout: None } => source
            .lines()
            .map(|line| {
                let t = line.trim_start();
                t.strip_prefix("> ")
                    .or_else(|| t.strip_prefix('>'))
                    .unwrap_or(line)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => source.to_string(),
    }
}

/// Re-serialize clean WYSIWYG body text back into exact canonical Markdown for `kind`.
pub fn serialize_block_markdown(kind: &BlockKind, body: &str) -> String {
    match kind {
        BlockKind::Heading(level) => {
            let hashes = "#".repeat((*level).clamp(1, 6) as usize);
            if body.is_empty() {
                format!("{hashes} ")
            } else {
                format!("{hashes} {body}")
            }
        }
        BlockKind::BulletItem { indent, marker } => {
            format!("{}{} {}", " ".repeat(*indent), marker, body)
        }
        BlockKind::OrderedItem {
            indent,
            number,
            sep,
        } => {
            format!("{}{}{sep} {}", " ".repeat(*indent), number, body)
        }
        BlockKind::TaskItem {
            indent,
            marker,
            checked,
        } => {
            let box_ch = if *checked { 'x' } else { ' ' };
            format!("{}{} [{}] {}", " ".repeat(*indent), marker, box_ch, body)
        }
        BlockKind::Blockquote { callout: None } => {
            if body.is_empty() {
                "> ".to_string()
            } else {
                body.split('\n')
                    .map(|line| {
                        if line.is_empty() {
                            ">".to_string()
                        } else {
                            format!("> {line}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        _ => body.to_string(),
    }
}

/// Live Markdown block shortcuts while typing:
/// `# `..`###### ` -> Heading, `- ` / `* ` -> Bullet, `[] ` / `[ ] ` -> Task,
/// `1. ` -> Ordered list, `> ` -> Blockquote.
/// Returns `(new_kind, stripped_body)`.
pub fn detect_live_block_transform(kind: &BlockKind, body: &str) -> Option<(BlockKind, String)> {
    match kind {
        BlockKind::Paragraph => {
            // Headings `# ` .. `###### `
            let hashes = body.bytes().take_while(|&b| b == b'#').count();
            if (1..=6).contains(&hashes) && body.as_bytes().get(hashes) == Some(&b' ') {
                let rest = body[hashes + 1..].to_string();
                return Some((BlockKind::Heading(hashes as u8), rest));
            }
            // Quick task triggers `[] `, `[ ] `, `[x] `, `[X] `
            for (prefix, checked) in [
                ("[] ", false),
                ("[ ] ", false),
                ("[x] ", true),
                ("[X] ", true),
            ] {
                if let Some(rest) = body.strip_prefix(prefix) {
                    return Some((
                        BlockKind::TaskItem {
                            indent: 0,
                            marker: '-',
                            checked,
                        },
                        rest.to_string(),
                    ));
                }
            }
            // Standard list/task prefixes `- [ ] `, `- `, `* `, `+ `, `1. `
            if let Some((list_kind, prefix_len)) = parse_list_line_prefix(body) {
                if body.len() >= prefix_len && body[..prefix_len].ends_with(' ') {
                    return Some((list_kind, body[prefix_len..].to_string()));
                }
            }
            // Blockquote `> `
            if let Some(rest) = body.strip_prefix("> ") {
                return Some((BlockKind::Blockquote { callout: None }, rest.to_string()));
            }
            None
        }
        BlockKind::BulletItem { indent, marker } => {
            // Typing `[] ` or `[ ] ` inside a bullet upgrades it to a task item!
            for (prefix, checked) in [
                ("[] ", false),
                ("[ ] ", false),
                ("[x] ", true),
                ("[X] ", true),
            ] {
                if let Some(rest) = body.strip_prefix(prefix) {
                    return Some((
                        BlockKind::TaskItem {
                            indent: *indent,
                            marker: *marker,
                            checked,
                        },
                        rest.to_string(),
                    ));
                }
            }
            None
        }
        _ => None,
    }
}

/// Filter blocks through folded heading sections while keeping any section containing
/// `active_range` open so the user's cursor is never hidden.
pub fn visible_blocks(
    blocks: &[LiveBlock],
    folded_lines: &BTreeSet<usize>,
    active_range: Option<&Range<usize>>,
) -> Vec<VisibleBlock> {
    let mut out = Vec::with_capacity(blocks.len());
    let mut i = 0;
    while i < blocks.len() {
        let block = &blocks[i];
        if let BlockKind::Heading(level) = block.kind {
            if folded_lines.contains(&block.start_line) {
                let mut end = i + 1;
                while end < blocks.len() {
                    if let BlockKind::Heading(next_level) = blocks[end].kind {
                        if next_level <= level {
                            break;
                        }
                    }
                    end += 1;
                }
                let active_inside = end > i + 1
                    && active_range.is_some_and(|act| {
                        let sec_start = blocks[i].range.end;
                        let sec_end = blocks[end - 1].range.end;
                        act.start >= sec_start && act.start <= sec_end
                    });
                if !active_inside && end > i + 1 {
                    out.push(VisibleBlock {
                        index: i,
                        collapsed_count: end - (i + 1),
                    });
                    i = end;
                    continue;
                }
            }
        }
        out.push(VisibleBlock {
            index: i,
            collapsed_count: 0,
        });
        i += 1;
    }
    out
}

/// Ordered visual rows in Live Preview derived purely from byte ranges in `raw`.
/// Because `Active` is matched strictly by byte span (`active_range`), a block can
/// never render twice or drift below/above its real position in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderSlot {
    Active {
        range: Range<usize>,
        kind: BlockKind,
    },
    Block {
        index: usize,
        collapsed_count: usize,
    },
    EmptyLine {
        offset: usize,
    },
}

fn emit_gap_empty_lines(
    raw: &str,
    gap_start: usize,
    gap_end: usize,
    normal_newlines: usize,
    out: &mut Vec<RenderSlot>,
) {
    let start = gap_start.min(raw.len());
    let end = gap_end.min(raw.len());
    if start >= end {
        return;
    }
    let slice = &raw[start..end];
    let nl_positions: Vec<usize> = slice
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| (b == b'\n').then_some(start + i))
        .collect();
    if nl_positions.len() <= normal_newlines {
        return;
    }
    for &nl_pos in &nl_positions[normal_newlines..] {
        // Each extra blank line sits immediately before its terminating `\n`,
        // so clicking it activates that exact byte position.
        out.push(RenderSlot::EmptyLine { offset: nl_pos });
    }
}

/// Build the exact ordered list of visual rows (inactive blocks, clickable blank lines,
/// and at most one active editor slot) for `raw`.
pub fn build_render_slots(
    raw: &str,
    parsed: &LiveDocument,
    active_range: Option<&Range<usize>>,
    active_kind: Option<&BlockKind>,
    folded_lines: &BTreeSet<usize>,
) -> Vec<RenderSlot> {
    let visible = visible_blocks(&parsed.blocks, folded_lines, active_range);
    let active_norm = active_range.map(|r| {
        let s = r.start.min(raw.len());
        let e = r.end.min(raw.len()).max(s);
        s..e
    });
    let active_kind_owned = active_kind.cloned().unwrap_or(BlockKind::Paragraph);
    // Frontmatter ends with `---\n` (1 `\n` already inside the span); one extra `\n`
    // after it is the normal visual separator before the first body block.
    let leading_normal_nl = if parsed.frontmatter.is_some() { 1 } else { 0 };

    let mut out = Vec::with_capacity(visible.len() + 4);
    let mut prev_end = parsed.body_start.min(raw.len());
    let mut prev_was_list = false;
    let mut seen_any_block = false;
    let mut active_emitted = false;

    for vis in &visible {
        let block = &parsed.blocks[vis.index];
        let base_normal_nl = if !seen_any_block {
            leading_normal_nl
        } else if prev_was_list && block.kind.is_list_item() {
            1
        } else {
            2
        };

        if let Some(act) = active_norm.as_ref() {
            if !active_emitted {
                let is_empty_before_or_at = act.start == act.end && act.start <= block.range.start;
                let is_nonempty_before = act.start < act.end && act.end <= block.range.start;
                let overlaps = act.start < act.end
                    && act.start < block.range.end
                    && block.range.start < act.end;

                if is_empty_before_or_at || is_nonempty_before {
                    let left_normal = if !seen_any_block {
                        leading_normal_nl
                    } else if prev_was_list && active_kind_owned.is_list_item() {
                        1
                    } else {
                        2
                    };
                    emit_gap_empty_lines(raw, prev_end, act.start, left_normal, &mut out);
                    out.push(RenderSlot::Active {
                        range: act.clone(),
                        kind: active_kind_owned.clone(),
                    });
                    active_emitted = true;

                    let right_normal =
                        if active_kind_owned.is_list_item() && block.kind.is_list_item() {
                            1
                        } else {
                            2
                        };
                    emit_gap_empty_lines(raw, act.end, block.range.start, right_normal, &mut out);
                    out.push(RenderSlot::Block {
                        index: vis.index,
                        collapsed_count: vis.collapsed_count,
                    });
                    prev_end = block.range.end;
                    prev_was_list = block.kind.is_list_item();
                    seen_any_block = true;
                    continue;
                }

                if overlaps {
                    emit_gap_empty_lines(
                        raw,
                        prev_end,
                        block.range.start,
                        base_normal_nl,
                        &mut out,
                    );
                    out.push(RenderSlot::Active {
                        range: act.clone(),
                        kind: active_kind_owned.clone(),
                    });
                    active_emitted = true;
                    prev_end = block.range.end.max(act.end);
                    prev_was_list = active_kind_owned.is_list_item();
                    seen_any_block = true;
                    continue;
                }
            } else if act.start < act.end
                && act.start < block.range.end
                && block.range.start < act.end
            {
                // Skip any secondary parsed block already covered by the active range.
                prev_end = prev_end.max(block.range.end);
                continue;
            }
        }

        emit_gap_empty_lines(raw, prev_end, block.range.start, base_normal_nl, &mut out);
        out.push(RenderSlot::Block {
            index: vis.index,
            collapsed_count: vis.collapsed_count,
        });
        prev_end = block.range.end;
        prev_was_list = block.kind.is_list_item();
        seen_any_block = true;
    }

    // Trailing gap / active block at EOF.
    let tail_left_normal = if !seen_any_block {
        leading_normal_nl
    } else if prev_was_list && active_kind_owned.is_list_item() {
        1
    } else {
        2
    };
    if let Some(act) = active_norm.as_ref() {
        if !active_emitted {
            emit_gap_empty_lines(raw, prev_end, act.start, tail_left_normal, &mut out);
            out.push(RenderSlot::Active {
                range: act.clone(),
                kind: active_kind_owned,
            });
            emit_gap_empty_lines(raw, act.end, raw.len(), 2, &mut out);
            return out;
        }
    }
    emit_gap_empty_lines(raw, prev_end, raw.len(), 2, &mut out);
    out
}

/// Returns true when the first block is already an H1 heading matching the filename stem.
pub fn inline_title_duplicated(stem: &str, blocks: &[LiveBlock]) -> bool {
    let Some(first) = blocks.first() else {
        return false;
    };
    if first.kind != BlockKind::Heading(1) {
        return false;
    }
    let heading_text = block_editable_body(&first.kind, &first.source);
    heading_text.trim().eq_ignore_ascii_case(stem.trim())
}

/// Compute a merge of `blocks[idx]`'s body onto the end of `blocks[idx - 1]`.
/// Preserves the previous block's WYSIWYG kind and returns:
/// `(prev_kind, merged_editable_body, full_byte_range_in_raw, caret_offset_in_body)`.
pub fn merge_block_with_previous(
    raw: &str,
    blocks: &[LiveBlock],
    idx: usize,
) -> Option<(BlockKind, String, Range<usize>, usize)> {
    if idx == 0 || idx >= blocks.len() {
        return None;
    }
    let prev = &blocks[idx - 1];
    let curr = &blocks[idx];
    if matches!(
        prev.kind,
        BlockKind::Table | BlockKind::ThematicBreak | BlockKind::Code
    ) {
        return None;
    }
    let full_range = prev.range.start..curr.range.end;
    if full_range.end > raw.len() {
        return None;
    }
    let prev_body = block_editable_body(&prev.kind, &prev.source);
    let curr_body = block_editable_body(&curr.kind, &curr.source);
    let sep = if prev_body.is_empty()
        || curr_body.is_empty()
        || prev_body.ends_with(' ')
        || curr_body.starts_with(' ')
    {
        ""
    } else {
        " "
    };
    let caret = prev_body.len() + sep.len();
    let merged_body = format!("{prev_body}{sep}{curr_body}");
    Some((prev.kind.clone(), merged_body, full_range, caret))
}

/// Toggle inline Markdown delimiters (`**`, `*`, `~~`, `` ` ``, `==`) around `sel`.
pub fn toggle_wrap_selection(
    text: &str,
    sel: Range<usize>,
    marker: &str,
) -> (String, Range<usize>) {
    let mut start = sel.start.min(text.len());
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = sel.end.min(text.len()).max(start);
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    let m = marker.len();
    if start >= m
        && end + m <= text.len()
        && text.get(start - m..start) == Some(marker)
        && text.get(end..end + m) == Some(marker)
    {
        let mut out = String::with_capacity(text.len() - 2 * m);
        out.push_str(&text[..start - m]);
        out.push_str(&text[start..end]);
        out.push_str(&text[end + m..]);
        return (out, (start - m)..(end - m));
    }
    let selected = &text[start..end];
    if selected.len() >= 2 * m && selected.starts_with(marker) && selected.ends_with(marker) {
        let inner = &selected[m..selected.len() - m];
        let mut out = String::with_capacity(text.len() - 2 * m);
        out.push_str(&text[..start]);
        out.push_str(inner);
        out.push_str(&text[end..]);
        return (out, start..(start + inner.len()));
    }
    let mut out = String::with_capacity(text.len() + 2 * m);
    out.push_str(&text[..start]);
    out.push_str(marker);
    out.push_str(selected);
    out.push_str(marker);
    out.push_str(&text[end..]);
    (out, (start + m)..(end + m))
}

/// Coalescing window for rapid same-block typing or deletion bursts in Live Preview.
const LIVE_HISTORY_COALESCE_MS: u64 = 650;
/// Maximum number of full-document snapshots retained in a note's Live Preview history.
const LIVE_HISTORY_MAX_ENTRIES: usize = 200;

/// Kind of edit recorded into [`LiveHistory`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditGroup {
    Typing,
    Deleting,
    Discrete,
}

/// One reversible document + cursor state in Live Preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveSnapshot {
    pub doc_text: String,
    pub active_range: Option<Range<usize>>,
    pub active_kind: Option<BlockKind>,
    pub caret: usize,
}

/// Document-level Undo/Redo history for Live Preview with smart burst coalescing
/// and exact active-block + cursor restoration across block boundaries.
#[derive(Default)]
pub struct LiveHistory {
    undo_stack: Vec<LiveSnapshot>,
    redo_stack: Vec<LiveSnapshot>,
    last_group: Option<EditGroup>,
    last_edit_at: Option<Instant>,
    last_block_start: Option<usize>,
}

impl LiveHistory {
    pub fn record_before_edit(&mut self, before: LiveSnapshot, group: EditGroup, now: Instant) {
        self.redo_stack.clear();
        let block_start = before.active_range.as_ref().map(|r| r.start);
        let should_coalesce = group != EditGroup::Discrete
            && self.last_group == Some(group)
            && block_start.is_some()
            && block_start == self.last_block_start
            && self.last_edit_at.is_some_and(|t| {
                now.saturating_duration_since(t) <= Duration::from_millis(LIVE_HISTORY_COALESCE_MS)
            })
            && !self.undo_stack.is_empty();

        if !should_coalesce && self.undo_stack.last() != Some(&before) {
            if self.undo_stack.len() >= LIVE_HISTORY_MAX_ENTRIES {
                self.undo_stack.remove(0);
            }
            self.undo_stack.push(before);
        }
        self.last_group = Some(group);
        self.last_edit_at = Some(now);
        self.last_block_start = block_start;
    }

    pub fn undo(&mut self, current: LiveSnapshot) -> Option<LiveSnapshot> {
        let prev = self.undo_stack.pop()?;
        if self.redo_stack.len() >= LIVE_HISTORY_MAX_ENTRIES {
            self.redo_stack.remove(0);
        }
        self.redo_stack.push(current);
        self.break_burst();
        Some(prev)
    }

    pub fn redo(&mut self, current: LiveSnapshot) -> Option<LiveSnapshot> {
        let next = self.redo_stack.pop()?;
        if self.undo_stack.len() >= LIVE_HISTORY_MAX_ENTRIES {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(current);
        self.break_burst();
        Some(next)
    }

    pub fn break_burst(&mut self) {
        self.last_group = None;
        self.last_edit_at = None;
        self.last_block_start = None;
    }
}

/// WYSIWYG Live Preview view bound to one [`Document`].
pub struct LiveView {
    workspace: WeakEntity<Workspace>,
    doc: Entity<Document>,
    block_editor: Entity<EditorState>,
    block_decorations: Option<TextDecorationCollection>,
    /// Exact byte span in `Document::editor` currently being edited in `block_editor`.
    /// Empty (`start == end`) represents an active blank line/paragraph at `start`.
    active_range: Option<Range<usize>>,
    active_kind: Option<BlockKind>,
    /// Exact full document text last written by LiveView (eliminates GPUI deferred-event races).
    last_doc_text: String,
    /// Exact editable body text last programmatically loaded into `block_editor`.
    last_block_text: String,
    /// Document-level WYSIWYG Undo/Redo stack preserving block kind and cursor position.
    history: LiveHistory,
    /// Suppresses re-recording into `history` while a fallback `Document::editor` undo/redo runs.
    applying_fallback_history: bool,
    /// True once Live Preview has written edits or restored snapshots into `Document::editor`,
    /// preventing fallback undo/redo from reversing into out-of-sync native editor history.
    canonical_history_diverged: bool,
    title_input: Entity<InputState>,
    editing_title: bool,
    folded_headings: BTreeSet<usize>,
    scroll: ScrollHandle,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for LiveView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl LiveView {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        vault: Option<Entity<crate::vault::Vault>>,
        tab_size: usize,
        doc: Entity<Document>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (title, path, initial_doc_text) = {
            let d = doc.read(cx);
            (
                d.title(),
                d.path.clone(),
                d.editor.read(cx).value().to_string(),
            )
        };
        let completion_title = Rc::new(std::cell::RefCell::new(title.clone()));
        let completion_path = Rc::new(std::cell::RefCell::new(path));

        // Plain-text EditorState (NO `.language("markdown")` tree-sitter code highlighting)
        // so active blocks look like clean proportional WYSIWYG prose with soft inline decorations.
        let block_editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(false)
                .soft_wrap(true)
                .folding(false)
                .tab_size(TabSize {
                    tab_size,
                    hard_tabs: false,
                });
            state.lsp_mut().completion_provider = Some(Rc::new(
                crate::slash::VaultCompletions::new(vault, completion_title, completion_path),
            ));
            state
        });

        let title_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Untitled");
            state.set_value(title, window, cx);
            state
        });

        let block_sub = cx.subscribe_in(
            &block_editor,
            window,
            |this, _editor, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.on_block_editor_change(window, cx),
                InputEvent::Blur => cx.notify(),
                _ => cx.notify(),
            },
        );

        let title_sub = cx.subscribe_in(
            &title_input,
            window,
            |this, _input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    this.commit_inline_title(window, cx);
                    this.activate_block_index(0, true, window, cx);
                }
                InputEvent::Blur if this.editing_title => {
                    this.commit_inline_title(window, cx);
                }
                _ => {}
            },
        );

        let doc_editor = doc.read(cx).editor.clone();
        let doc_sub = cx.subscribe_in(
            &doc_editor,
            window,
            |this, editor, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let current = editor.read(cx).value().to_string();
                if current == this.last_doc_text {
                    // This Change event was produced by LiveView's own write; stay in the block!
                    return;
                }
                let prev_text = std::mem::replace(&mut this.last_doc_text, current);
                if this.applying_fallback_history {
                    this.applying_fallback_history = false;
                } else {
                    // Record external mutations (e.g. inactive checkbox toggle, table edit,
                    // property update) so ⌘Z in Live Preview cleanly reverses them too.
                    this.history.record_before_edit(
                        LiveSnapshot {
                            doc_text: prev_text,
                            active_range: this.active_range.clone(),
                            active_kind: this.active_kind.clone(),
                            caret: 0,
                        },
                        EditGroup::Discrete,
                        Instant::now(),
                    );
                }
                // External edit: reset active block so the AST stays in exact sync with buffer.
                this.active_range = None;
                this.active_kind = None;
                cx.notify();
            },
        );

        Self {
            workspace,
            doc,
            block_editor,
            block_decorations: None,
            active_range: None,
            active_kind: None,
            last_doc_text: initial_doc_text,
            last_block_text: String::new(),
            history: LiveHistory::default(),
            applying_fallback_history: false,
            canonical_history_diverged: false,
            title_input,
            editing_title: false,
            folded_headings: BTreeSet::new(),
            scroll: ScrollHandle::new(),
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![block_sub, title_sub, doc_sub],
        }
    }

    /// Replace `range` inside the canonical `Document::editor` and snapshot the resulting
    /// text in `self.last_doc_text` so GPUI's queued `InputEvent::Change` never kicks the
    /// user out of the active WYSIWYG block.
    fn write_doc_range(
        &mut self,
        range: Range<usize>,
        replacement: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let mut new_range = range.start..(range.start + replacement.len());
        self.doc.update(cx, |doc, cx| {
            doc.editor.update(cx, |ed, cx| {
                let len = ed.value().len();
                let start = range.start.min(len);
                let end = range.end.min(len);
                new_range = start..(start + replacement.len());
                ed.set_selected_range(start..end, cx);
                ed.replace(replacement, window, cx);
            });
        });
        self.last_doc_text = self.doc.read(cx).editor.read(cx).value().to_string();
        self.canonical_history_diverged = true;
        new_range
    }

    /// Programmatically load clean editable body text into `block_editor` without triggering
    /// a redundant echo back to `Document::editor`.
    fn load_block_editor(
        &mut self,
        body: String,
        caret: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let caret = caret.min(body.len());
        self.last_block_text = body.clone();
        self.block_editor.update(cx, |ed, cx| {
            ed.set_value(body, window, cx);
            ed.set_selected_range(caret..caret, cx);
            ed.focus(window, cx);
        });
        self.refresh_block_decorations(cx);
    }

    pub fn focus_block_for_line(
        &mut self,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw = self.doc.read(cx).editor.read(cx).value().to_string();
        let parsed = parse_live_document(&raw);
        if parsed.blocks.is_empty() {
            self.append_and_focus_block(window, cx);
            return;
        }
        let target = parsed
            .blocks
            .iter()
            .enumerate()
            .rfind(|(_, b)| b.start_line <= line)
            .map(|(ix, _)| ix)
            .unwrap_or(0);
        self.folded_headings.clear();
        self.activate_block_index(target, true, window, cx);
    }

    pub fn focus_active_or_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_range.is_some() {
            self.block_editor.update(cx, |ed, cx| ed.focus(window, cx));
        } else {
            self.focus_handle.focus(window, cx);
        }
    }

    fn refresh_block_decorations(&mut self, cx: &mut Context<Self>) {
        if matches!(self.active_kind, Some(BlockKind::Code)) {
            if let Some(collection) = &self.block_decorations {
                collection.set(Vec::new(), cx);
            }
            return;
        }
        let items = self.block_editor.update(cx, |state, cx| {
            let text = state.value().to_string();
            let caret = state.cursor();
            crate::decorations::markdown_decorations_with_caret(
                &text,
                cx.theme(),
                None,
                Some(caret),
            )
        });
        match &self.block_decorations {
            Some(collection) => collection.set(items, cx),
            None => {
                let collection = self.block_editor.update(cx, |state, cx| {
                    state.create_decorations_collection(items, cx)
                });
                self.block_decorations = Some(collection);
            }
        }
    }

    fn current_snapshot(&self, cx: &App) -> LiveSnapshot {
        let doc_text = self.doc.read(cx).editor.read(cx).value().to_string();
        let caret = if self.active_range.is_some() {
            self.block_editor.read(cx).cursor()
        } else {
            0
        };
        LiveSnapshot {
            doc_text,
            active_range: self.active_range.clone(),
            active_kind: self.active_kind.clone(),
            caret,
        }
    }

    fn record_discrete_snapshot(&mut self, cx: &App) {
        let snap = self.current_snapshot(cx);
        self.history
            .record_before_edit(snap, EditGroup::Discrete, Instant::now());
    }

    fn restore_snapshot(
        &mut self,
        snap: LiveSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_doc_text = snap.doc_text.clone();
        self.canonical_history_diverged = true;
        self.doc.update(cx, |doc, cx| {
            doc.replace_without_history(snap.doc_text.clone(), window, cx);
        });
        let max_len = snap.doc_text.len();
        let restored_range = snap.active_range.map(|r| {
            let s = r.start.min(max_len);
            let e = r.end.min(max_len).max(s);
            s..e
        });
        self.active_range = restored_range.clone();
        self.active_kind = snap.active_kind.clone();
        if let (Some(range), Some(kind)) = (restored_range, snap.active_kind) {
            if snap.doc_text.is_char_boundary(range.start)
                && snap.doc_text.is_char_boundary(range.end)
            {
                let body = if range.is_empty() {
                    String::new()
                } else {
                    block_editable_body(&kind, &snap.doc_text[range.start..range.end])
                };
                self.load_block_editor(body, snap.caret, window, cx);
            } else {
                self.active_range = None;
                self.active_kind = None;
                self.focus_handle.focus(window, cx);
            }
        } else {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn perform_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.current_snapshot(cx);
        if let Some(prev) = self.history.undo(current) {
            self.restore_snapshot(prev, window, cx);
            return;
        }
        if self.canonical_history_diverged {
            return;
        }
        if let Some(ws) = self.workspace.upgrade() {
            self.applying_fallback_history = true;
            let doc = self.doc.clone();
            ws.update(cx, |ws, cx| ws.queue_preview_history_pub(doc, false, cx));
        }
    }

    pub(crate) fn perform_redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.current_snapshot(cx);
        if let Some(next) = self.history.redo(current) {
            self.restore_snapshot(next, window, cx);
            return;
        }
        if self.canonical_history_diverged {
            return;
        }
        if let Some(ws) = self.workspace.upgrade() {
            self.applying_fallback_history = true;
            let doc = self.doc.clone();
            ws.update(cx, |ws, cx| ws.queue_preview_history_pub(doc, true, cx));
        }
    }

    fn activate_block_index(
        &mut self,
        idx: usize,
        caret_at_start: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw = self.doc.read(cx).editor.read(cx).value().to_string();
        let parsed = parse_live_document(&raw);
        let Some(block) = parsed.blocks.get(idx) else {
            return;
        };
        let body = block_editable_body(&block.kind, &block.source);
        let caret = if caret_at_start { 0 } else { body.len() };
        self.history.break_burst();
        self.active_range = Some(block.range.clone());
        self.active_kind = Some(block.kind.clone());
        self.load_block_editor(body, caret, window, cx);
        cx.notify();
    }

    fn activate_empty_line(&mut self, offset: usize, window: &mut Window, cx: &mut Context<Self>) {
        let len = self.doc.read(cx).editor.read(cx).value().len();
        let pos = offset.min(len);
        self.history.break_burst();
        self.active_range = Some(pos..pos);
        self.active_kind = Some(BlockKind::Paragraph);
        self.load_block_editor(String::new(), 0, window, cx);
        cx.notify();
    }

    fn deactivate_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history.break_burst();
        self.active_range = None;
        self.active_kind = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn on_block_editor_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(range) = self.active_range.clone() else {
            return;
        };
        let Some(mut kind) = self.active_kind.clone() else {
            return;
        };
        let (mut body, new_caret) = {
            let ed = self.block_editor.read(cx);
            (ed.value().to_string(), ed.cursor())
        };
        if body == self.last_block_text {
            return;
        }

        let prev_len = self.last_block_text.len();
        let new_len = body.len();
        let before_caret = if new_len >= prev_len {
            new_caret.saturating_sub(new_len - prev_len)
        } else {
            (new_caret + (prev_len - new_len)).min(prev_len)
        };
        let before_snapshot = LiveSnapshot {
            doc_text: self.last_doc_text.clone(),
            active_range: Some(range.clone()),
            active_kind: Some(kind.clone()),
            caret: before_caret,
        };
        let is_live_transform = detect_live_block_transform(&kind, &body).is_some()
            || (kind == BlockKind::Paragraph && body == "--- ");
        let group = if is_live_transform || new_len > prev_len + 4 {
            EditGroup::Discrete
        } else if new_len < prev_len {
            EditGroup::Deleting
        } else {
            EditGroup::Typing
        };
        self.history
            .record_before_edit(before_snapshot, group, Instant::now());

        // Live WYSIWYG block shortcut transforms (`# `, `- `, `[ ] `, `1. `, `> `):
        if let Some((next_kind, stripped_body)) = detect_live_block_transform(&kind, &body) {
            kind = next_kind;
            body = stripped_body;
            self.active_kind = Some(kind.clone());
            self.load_block_editor(body.clone(), body.len(), window, cx);
        } else {
            self.last_block_text = body.clone();
        }

        // Horizontal rule shortcut (`--- ` on a paragraph):
        if kind == BlockKind::Paragraph && body == "--- " {
            let raw = self.doc.read(cx).editor.read(cx).value().to_string();
            let after_slice = &raw[range.end.min(raw.len())..];
            let trailing = if after_slice.starts_with('\n')
                && !after_slice.starts_with("\n\n")
                && after_slice.len() > 1
            {
                "\n"
            } else {
                ""
            };
            let new_range = self.write_doc_range(range, format!("---\n\n{trailing}"), window, cx);
            let next_start = new_range.start + 5;
            self.active_range = Some(next_start..next_start);
            self.active_kind = Some(BlockKind::Paragraph);
            self.load_block_editor(String::new(), 0, window, cx);
            cx.notify();
            return;
        }

        let serialized = serialize_block_markdown(&kind, &body);
        // When typing the first character onto a previously empty line that is followed
        // by a single `\n` before another block, insert an extra `\n` after it so the
        // new block stays cleanly separated instead of merging into the next block.
        let needs_trailing_sep = if range.start == range.end && !serialized.is_empty() {
            let raw = self.doc.read(cx).editor.read(cx).value().to_string();
            let after_slice = &raw[range.end.min(raw.len())..];
            after_slice.starts_with('\n')
                && !after_slice.starts_with("\n\n")
                && after_slice.len() > 1
        } else {
            false
        };
        let to_write = if needs_trailing_sep {
            format!("{serialized}\n")
        } else {
            serialized.clone()
        };
        let written = self.write_doc_range(range, to_write, window, cx);
        self.active_range = Some(written.start..(written.start + serialized.len()));
        self.refresh_block_decorations(cx);
        cx.notify();
    }

    fn append_and_focus_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.doc.read(cx).editor.read(cx).value().to_string();
        let parsed = parse_live_document(&raw);
        if parsed.blocks.is_empty() && raw[parsed.body_start.min(raw.len())..].trim().is_empty() {
            let pos = raw.len();
            self.active_range = Some(pos..pos);
            self.active_kind = Some(BlockKind::Paragraph);
            self.load_block_editor(String::new(), 0, window, cx);
            cx.notify();
            return;
        }
        let suffix = if raw.ends_with("\n\n") {
            ""
        } else if raw.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let insert_at = raw.len();
        if !suffix.is_empty() {
            self.write_doc_range(insert_at..insert_at, suffix.to_string(), window, cx);
        }
        let start = insert_at + suffix.len();
        self.active_range = Some(start..start);
        self.active_kind = Some(BlockKind::Paragraph);
        self.load_block_editor(String::new(), 0, window, cx);
        cx.notify();
    }

    fn unwrap_list_item_to_paragraph(
        &mut self,
        range: Range<usize>,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw = self.doc.read(cx).editor.read(cx).value().to_string();
        let before_slice = &raw[..range.start.min(raw.len())];
        let after_slice = &raw[range.end.min(raw.len())..];
        let needs_leading_nl = before_slice.ends_with('\n') && !before_slice.ends_with("\n\n");
        let needs_trailing_nl = after_slice.starts_with('\n')
            && !after_slice.starts_with("\n\n")
            && after_slice.len() > 1;
        let mut replacement = String::new();
        if needs_leading_nl {
            replacement.push('\n');
        }
        let body_offset = replacement.len();
        replacement.push_str(&body);
        if needs_trailing_nl {
            replacement.push('\n');
        }
        let written = self.write_doc_range(range, replacement, window, cx);
        let body_start = written.start + body_offset;
        let body_end = body_start + body.len();
        self.active_kind = Some(BlockKind::Paragraph);
        self.active_range = Some(body_start..body_end);
        self.load_block_editor(body, 0, window, cx);
        cx.notify();
    }

    fn toggle_active_task_checkbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(BlockKind::TaskItem {
            indent,
            marker,
            checked,
        }) = self.active_kind.clone()
        else {
            return;
        };
        let Some(range) = self.active_range.clone() else {
            return;
        };
        let next_kind = BlockKind::TaskItem {
            indent,
            marker,
            checked: !checked,
        };
        self.record_discrete_snapshot(cx);
        let body = self.block_editor.read(cx).value().to_string();
        let serialized = serialize_block_markdown(&next_kind, &body);
        let new_range = self.write_doc_range(range, serialized, window, cx);
        self.active_kind = Some(next_kind);
        self.active_range = Some(new_range);
        cx.notify();
    }

    fn handle_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(kind) = self.active_kind.clone() else {
            return false;
        };
        // Code blocks & raw tables keep native Enter for normal multi-line editing.
        if matches!(kind, BlockKind::Code | BlockKind::Table) {
            return false;
        }
        let Some(range) = self.active_range.clone() else {
            return false;
        };
        let (body, sel) = {
            let ed = self.block_editor.read(cx);
            (ed.value().to_string(), ed.selected_range())
        };
        let before = body[..sel.start.min(body.len())].to_string();
        let after = body[sel.end.min(body.len())..].to_string();

        // List / Task items:
        // - Empty item -> exit the list cleanly into a plain Paragraph in place,
        //   keeping blank-line separation from neighboring list items.
        // - Non-empty item -> split into the next matching list item on the next line.
        if kind.is_list_item() {
            self.record_discrete_snapshot(cx);
            if body.trim().is_empty() {
                self.unwrap_list_item_to_paragraph(range, String::new(), window, cx);
                return true;
            }
            let next_kind = match &kind {
                BlockKind::BulletItem { indent, marker } => BlockKind::BulletItem {
                    indent: *indent,
                    marker: *marker,
                },
                BlockKind::TaskItem { indent, marker, .. } => BlockKind::TaskItem {
                    indent: *indent,
                    marker: *marker,
                    checked: false,
                },
                BlockKind::OrderedItem {
                    indent,
                    number,
                    sep,
                } => BlockKind::OrderedItem {
                    indent: *indent,
                    number: number + 1,
                    sep: *sep,
                },
                _ => BlockKind::Paragraph,
            };
            let first_md = serialize_block_markdown(&kind, &before);
            let second_md = serialize_block_markdown(&next_kind, &after);
            let combined = format!("{first_md}\n{second_md}");
            let written = self.write_doc_range(range, combined, window, cx);
            let next_start = written.start + first_md.len() + 1;
            let next_end = written.end;
            self.active_range = Some(next_start..next_end);
            self.active_kind = Some(next_kind);
            self.load_block_editor(after, 0, window, cx);
            cx.notify();
            return true;
        }

        // Plain blockquote: Enter on an empty trailing line exits to a Paragraph below;
        // otherwise keep writing inside the quote.
        if matches!(kind, BlockKind::Blockquote { callout: None }) {
            if before.ends_with('\n') && after.is_empty() {
                self.record_discrete_snapshot(cx);
                let kept = before.trim_end_matches('\n');
                let first_md = serialize_block_markdown(&kind, kept);
                let combined = format!("{first_md}\n\n");
                let written = self.write_doc_range(range, combined, window, cx);
                self.active_range = Some(written.end..written.end);
                self.active_kind = Some(BlockKind::Paragraph);
                self.load_block_editor(String::new(), 0, window, cx);
                cx.notify();
                return true;
            }
            return false;
        }

        // Callouts keep multi-line editing inside the callout unless double-Enter at end.
        if matches!(kind, BlockKind::Blockquote { callout: Some(_) }) {
            if before.ends_with('\n') && after.is_empty() {
                self.record_discrete_snapshot(cx);
                let kept = before.trim_end_matches('\n');
                let combined = format!("{kept}\n\n");
                let written = self.write_doc_range(range, combined, window, cx);
                self.active_range = Some(written.end..written.end);
                self.active_kind = Some(BlockKind::Paragraph);
                self.load_block_editor(String::new(), 0, window, cx);
                cx.notify();
                return true;
            }
            return false;
        }

        // Empty Paragraph: insert a single newline so every Enter visibly creates one
        // new blank line and moves the cursor down by one line without ever drifting.
        self.record_discrete_snapshot(cx);
        if kind == BlockKind::Paragraph && body.is_empty() {
            let written = self.write_doc_range(range, "\n".to_string(), window, cx);
            self.active_range = Some(written.end..written.end);
            self.active_kind = Some(BlockKind::Paragraph);
            self.load_block_editor(String::new(), 0, window, cx);
            cx.notify();
            return true;
        }

        // Pressing Enter at column 0 of a non-empty Paragraph pushes it down by 1 blank line.
        if kind == BlockKind::Paragraph && before.is_empty() && !after.is_empty() {
            let written =
                self.write_doc_range(range.start..range.start, "\n".to_string(), window, cx);
            let shift = written.len();
            self.active_range = Some((range.start + shift)..(range.end + shift));
            self.active_kind = Some(BlockKind::Paragraph);
            self.load_block_editor(after, 0, window, cx);
            cx.notify();
            return true;
        }

        // Heading or non-empty Paragraph: split cleanly into a new Paragraph block below
        // while keeping clean `\n\n` separation from any following block.
        let raw = self.doc.read(cx).editor.read(cx).value().to_string();
        let after_slice = &raw[range.end.min(raw.len())..];
        let trailing_nl = if after_slice.is_empty() || after_slice.starts_with("\n\n") {
            ""
        } else if after_slice.starts_with('\n') {
            if after_slice.len() > 1 {
                "\n"
            } else {
                ""
            }
        } else {
            "\n\n"
        };
        let first_md = serialize_block_markdown(&kind, &before);
        let after_clean = if matches!(kind, BlockKind::Heading(_)) {
            after.trim_start().to_string()
        } else {
            after
        };
        let combined = format!("{first_md}\n\n{after_clean}{trailing_nl}");
        let written = self.write_doc_range(range, combined, window, cx);
        let next_start = written.start + first_md.len() + 2;
        let next_end = next_start + after_clean.len();
        self.active_range = Some(next_start..next_end);
        self.active_kind = Some(BlockKind::Paragraph);
        self.load_block_editor(after_clean, 0, window, cx);
        cx.notify();
        true
    }

    fn handle_backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let sel = self.block_editor.read(cx).selected_range();
        if !sel.is_empty() || sel.start != 0 {
            return false;
        }
        let Some(kind) = self.active_kind.clone() else {
            return false;
        };
        let Some(range) = self.active_range.clone() else {
            return false;
        };

        // 1. Backspace at column 0 of a Heading, List/Task item, or plain Quote unwraps
        // its block type into a plain Paragraph while keeping all text intact!
        if kind.is_list_item() {
            self.record_discrete_snapshot(cx);
            let body = self.block_editor.read(cx).value().to_string();
            self.unwrap_list_item_to_paragraph(range, body, window, cx);
            return true;
        }
        if matches!(
            kind,
            BlockKind::Heading(_) | BlockKind::Blockquote { callout: None }
        ) {
            self.record_discrete_snapshot(cx);
            let body = self.block_editor.read(cx).value().to_string();
            let written = self.write_doc_range(range, body.clone(), window, cx);
            self.active_kind = Some(BlockKind::Paragraph);
            self.active_range = Some(written);
            self.load_block_editor(body, 0, window, cx);
            cx.notify();
            return true;
        }

        // 2. Paragraph at column 0: pull up through extra blank lines first, then merge
        // cleanly onto the previous block!
        let raw = self.doc.read(cx).editor.read(cx).value().to_string();
        let parsed = parse_live_document(&raw);
        let start = range.start.min(raw.len());
        let end = range.end.min(raw.len()).max(start);

        let prev_block_ix = parsed.blocks.iter().rposition(|b| b.range.end <= start);
        let prev_end = prev_block_ix
            .map(|i| parsed.blocks[i].range.end)
            .unwrap_or_else(|| parsed.body_start.min(raw.len()));
        let normal_nl = if prev_block_ix.is_some() {
            2
        } else if parsed.frontmatter.is_some() {
            1
        } else {
            0
        };
        let gap_nl_count = if prev_end < start {
            raw[prev_end..start].bytes().filter(|&b| b == b'\n').count()
        } else {
            0
        };

        // If there is an extra visible blank line immediately above, delete 1 `\n`
        // to move the cursor (or paragraph) up by one line cleanly.
        if gap_nl_count > normal_nl && start > 0 && raw.as_bytes()[start - 1] == b'\n' {
            self.record_discrete_snapshot(cx);
            self.write_doc_range((start - 1)..start, String::new(), window, cx);
            self.active_range = Some((start - 1)..(end - 1));
            cx.notify();
            return true;
        }

        // Empty active line directly after a previous block: remove the separator we
        // created on Enter and return focus to the end of the previous block.
        if start == end {
            let Some(prev_ix) = prev_block_ix else {
                return false;
            };
            self.record_discrete_snapshot(cx);
            let prev_end = parsed.blocks[prev_ix].range.end;
            if start > prev_end {
                self.write_doc_range(prev_end..start, String::new(), window, cx);
            }
            self.activate_block_index(prev_ix, false, window, cx);
            return true;
        }

        // Non-empty paragraph directly after a previous block: merge onto previous block.
        let Some(curr_ix) = parsed
            .blocks
            .iter()
            .position(|b| b.range.start < end && start < b.range.end)
        else {
            return false;
        };
        let Some((prev_kind, merged_body, full_range, caret)) =
            merge_block_with_previous(&raw, &parsed.blocks, curr_ix)
        else {
            return false;
        };
        self.record_discrete_snapshot(cx);
        let serialized = serialize_block_markdown(&prev_kind, &merged_body);
        let written = self.write_doc_range(full_range, serialized, window, cx);
        self.active_range = Some(written);
        self.active_kind = Some(prev_kind);
        self.load_block_editor(merged_body, caret, window, cx);
        cx.notify();
        true
    }

    fn handle_arrow_boundary(
        &mut self,
        up: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(active_range) = self.active_range.clone() else {
            return false;
        };
        let (pos, line_count) = {
            let ed = self.block_editor.read(cx);
            let val = ed.value();
            let lines = val.as_ref().split('\n').count().max(1);
            (ed.cursor_position(), lines)
        };
        if up && pos.line > 0 {
            return false;
        }
        if !up && (pos.line as usize + 1) < line_count {
            return false;
        }
        let raw = self.doc.read(cx).editor.read(cx).value().to_string();
        let parsed = parse_live_document(&raw);
        let slots = build_render_slots(
            &raw,
            &parsed,
            Some(&active_range),
            self.active_kind.as_ref(),
            &self.folded_headings,
        );
        let Some(slot_ix) = slots
            .iter()
            .position(|s| matches!(s, RenderSlot::Active { .. }))
        else {
            return false;
        };
        let target_ix = if up {
            slot_ix.checked_sub(1)
        } else if slot_ix + 1 < slots.len() {
            Some(slot_ix + 1)
        } else {
            None
        };
        let Some(target) = target_ix.and_then(|i| slots.get(i)) else {
            return false;
        };
        match target {
            RenderSlot::Block { index, .. } => {
                self.activate_block_index(*index, !up, window, cx);
                true
            }
            RenderSlot::EmptyLine { offset } => {
                self.activate_empty_line(*offset, window, cx);
                true
            }
            RenderSlot::Active { .. } => false,
        }
    }

    fn handle_indent(
        &mut self,
        outdent: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(kind) = self.active_kind.clone() else {
            return false;
        };
        if !kind.is_list_item() {
            return false;
        }
        let Some(range) = self.active_range.clone() else {
            return false;
        };
        let current = kind.indent();
        let next = if outdent {
            current.saturating_sub(2)
        } else {
            (current + 2).min(12)
        };
        if next == current {
            return true;
        }
        self.record_discrete_snapshot(cx);
        let next_kind = kind.with_indent(next);
        let body = self.block_editor.read(cx).value().to_string();
        let serialized = serialize_block_markdown(&next_kind, &body);
        let written = self.write_doc_range(range, serialized, window, cx);
        self.active_kind = Some(next_kind);
        self.active_range = Some(written);
        cx.notify();
        true
    }

    fn handle_paste(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        for entry in &item.entries {
            if let ClipboardEntry::String(text) = entry {
                let url = text.text.trim();
                let is_url = (url.starts_with("https://") || url.starts_with("http://"))
                    && !url.chars().any(char::is_whitespace)
                    && url.len() < 2048;
                if is_url {
                    let sel = self.block_editor.read(cx).selected_range();
                    if !sel.is_empty() {
                        let current = self.block_editor.read(cx).value().to_string();
                        let label = current[sel.clone()].to_string();
                        let replacement = format!("[{label}]({url})");
                        let end = sel.start + replacement.len();
                        self.block_editor.update(cx, |ed, cx| {
                            ed.set_selected_range(sel, cx);
                            ed.replace(replacement, window, cx);
                            ed.set_selected_range(end..end, cx);
                        });
                        return true;
                    }
                }
            }
        }
        false
    }

    fn apply_inline_wrap(&mut self, marker: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.record_discrete_snapshot(cx);
        let (text, sel) = {
            let ed = self.block_editor.read(cx);
            (ed.value().to_string(), ed.selected_range())
        };
        let (updated, new_sel) = toggle_wrap_selection(&text, sel, marker);
        self.last_block_text = updated.clone();
        self.block_editor.update(cx, |ed, cx| {
            ed.set_value(updated.clone(), window, cx);
            ed.set_selected_range(new_sel, cx);
            ed.focus(window, cx);
        });
        if let (Some(range), Some(kind)) = (self.active_range.clone(), self.active_kind.clone()) {
            let serialized = serialize_block_markdown(&kind, &updated);
            let written = self.write_doc_range(range, serialized, window, cx);
            self.active_range = Some(written);
        }
        self.refresh_block_decorations(cx);
        cx.notify();
    }

    fn apply_link_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (text, sel) = {
            let ed = self.block_editor.read(cx);
            (ed.value().to_string(), ed.selected_range())
        };
        if sel.is_empty() {
            return;
        }
        let selected = &text[sel.clone()];
        let replacement = format!("[[{selected}]]");
        let end = sel.start + replacement.len();
        self.block_editor.update(cx, |ed, cx| {
            ed.set_selected_range(sel, cx);
            ed.replace(replacement, window, cx);
            ed.set_selected_range(end..end, cx);
            ed.focus(window, cx);
        });
    }

    fn toggle_heading_block(&mut self, level: u8, window: &mut Window, cx: &mut Context<Self>) {
        let Some(range) = self.active_range.clone() else {
            return;
        };
        self.record_discrete_snapshot(cx);
        let next_kind = if self.active_kind == Some(BlockKind::Heading(level)) {
            BlockKind::Paragraph
        } else {
            BlockKind::Heading(level)
        };
        let body = self.block_editor.read(cx).value().to_string();
        let serialized = serialize_block_markdown(&next_kind, &body);
        let written = self.write_doc_range(range, serialized, window, cx);
        self.active_kind = Some(next_kind);
        self.active_range = Some(written);
        self.block_editor.update(cx, |ed, cx| ed.focus(window, cx));
        cx.notify();
    }

    fn begin_inline_title_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stem = self.doc.read(cx).title();
        self.editing_title = true;
        self.title_input.update(cx, |input, cx| {
            input.set_value(stem, window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn commit_inline_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_title = false;
        let next_name = self.title_input.read(cx).value().trim().to_string();
        let current_stem = self.doc.read(cx).title();
        if next_name.is_empty() || next_name == current_stem {
            cx.notify();
            return;
        }
        let path = self.doc.read(cx).path.clone();
        if let Some(ws) = self.workspace.upgrade() {
            ws.update(cx, |ws, cx| {
                ws.rename_note_inline(path, next_name, window, cx);
            });
        }
        cx.notify();
    }

    fn render_selection_toolbar(
        &self,
        place_below: bool,
        language: crate::settings::Language,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let sel = self.block_editor.read(cx).selected_range();
        if sel.is_empty() || self.active_range.is_none() {
            return None;
        }
        let selected_text = {
            let v = self.block_editor.read(cx).value();
            v.as_ref()
                .get(sel.clone())
                .map(str::to_string)
                .unwrap_or_default()
        };
        let theme = cx.theme().colors;
        Some(
            h_flex()
                .absolute()
                .left_0()
                .when(place_below, |el| el.bottom(px(-36.)))
                .when(!place_below, |el| el.top(px(-36.)))
                .px_1p5()
                .py_0p5()
                .gap_0p5()
                .items_center()
                .rounded(px(8.))
                .bg(theme.popover)
                .border_1()
                .border_color(theme.border.opacity(0.85))
                .shadow_md()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Button::new("live-fmt-bold")
                        .ghost()
                        .xsmall()
                        .label("B")
                        .tooltip(language.text("Bold", "Fet"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_inline_wrap("**", window, cx);
                        })),
                )
                .child(
                    Button::new("live-fmt-italic")
                        .ghost()
                        .xsmall()
                        .label("I")
                        .tooltip(language.text("Italic (⌘I)", "Kursiv (⌘I)"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_inline_wrap("*", window, cx);
                        })),
                )
                .child(
                    Button::new("live-fmt-strike")
                        .ghost()
                        .xsmall()
                        .label("S")
                        .tooltip(language.text("Strikethrough", "Gjennomstreking"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_inline_wrap("~~", window, cx);
                        })),
                )
                .child(
                    Button::new("live-fmt-code")
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::Code)
                        .tooltip(language.text("Inline code", "Kode"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_inline_wrap("`", window, cx);
                        })),
                )
                .child(
                    Button::new("live-fmt-highlight")
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::Highlighter)
                        .tooltip(language.text("Highlight", "Utheving"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_inline_wrap("==", window, cx);
                        })),
                )
                .child(
                    Button::new("live-fmt-link")
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::Link2)
                        .tooltip(language.text("Wikilink", "Wikilenke"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.apply_link_wrap(window, cx);
                        })),
                )
                .child(
                    Button::new("live-fmt-h2")
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::Heading2)
                        .tooltip(language.text("Heading 2", "Overskrift 2"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_heading_block(2, window, cx);
                        })),
                )
                .child(
                    div()
                        .w(px(1.))
                        .h(px(14.))
                        .mx_0p5()
                        .bg(theme.border.opacity(0.6)),
                )
                .child(
                    Button::new("live-fmt-agent")
                        .ghost()
                        .xsmall()
                        .icon(assets::IconName::MessageSquareQuote)
                        .label(language.text("Ask Agent", "Spør agent"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(ws) = this.workspace.upgrade() {
                                ws.update(cx, |ws, cx| {
                                    ws.ask_agent_about_selection(selected_text.clone(), window, cx);
                                });
                            }
                        })),
                ),
        )
    }

    fn handle_link_click(
        workspace: WeakEntity<Workspace>,
        link: SharedString,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(ws) = workspace.upgrade() else {
            return;
        };
        let url = link.to_string();
        if let Some(target) = url.strip_prefix("wiki:") {
            let target = target.to_string();
            ws.update(cx, |this, cx| {
                this.open_wikilink(&target, window, cx);
            });
        } else if let Some(target) = url.strip_prefix("missing:") {
            let target = target.to_string();
            ws.update(cx, |this, cx| {
                this.note_status(format!("No file named “{}”", target), cx);
            });
        } else if let Some(tag) = url.strip_prefix("tag:") {
            let query = format!("#{tag}");
            crate::search::open_project_search_for(ws, Some(query.as_str()), window, cx);
        } else {
            cx.open_url(&url);
        }
    }

    fn render_list_gutter(
        &self,
        kind: &BlockKind,
        actual_line: Option<usize>,
        active: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = cx.theme();
        match kind {
            BlockKind::TaskItem { checked, .. } => {
                let checked = *checked;
                let mut cb = div()
                    .id((
                        "live-task-cb",
                        actual_line.unwrap_or(0) * 2 + usize::from(active),
                    ))
                    .w(px(15.))
                    .h(px(15.))
                    .mt(px(5.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .rounded(px(3.5))
                    .cursor_pointer();
                if checked {
                    cb = cb.border_color(theme.accent).bg(theme.accent).child(
                        Icon::new(assets::IconName::Check)
                            .size_3()
                            .text_color(theme.accent_foreground),
                    );
                } else {
                    cb = cb.border_color(theme.muted_foreground.opacity(0.8));
                }
                if active {
                    cb = cb
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.toggle_active_task_checkbox(window, cx);
                        }));
                } else if let Some(line) = actual_line {
                    let ws = self.workspace.clone();
                    cb = cb
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = ws.update(cx, |ws, cx| ws.toggle_task_pub(line, window, cx));
                        });
                }
                Some(cb.into_any_element())
            }
            BlockKind::BulletItem { .. } => Some(
                div()
                    .w(px(16.))
                    .mt(px(2.))
                    .flex_none()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .text_center()
                    .child("•")
                    .into_any_element(),
            ),
            BlockKind::OrderedItem { number, .. } => Some(
                div()
                    .min_w(px(20.))
                    .mt(px(2.))
                    .flex_none()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(format!("{number}."))
                    .into_any_element(),
            ),
            _ => None,
        }
    }

    fn render_active_block(
        &self,
        slot_ix: usize,
        kind: &BlockKind,
        body_font_size: f32,
        body_font: Option<SharedString>,
        language: crate::settings::Language,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().colors;
        let (font_size, weight, is_code, is_callout, is_quote, my_px) = match kind {
            BlockKind::Heading(1) => (
                body_font_size + 11.5,
                Some(FontWeight::BOLD),
                false,
                false,
                false,
                4.0,
            ),
            BlockKind::Heading(2) => (
                body_font_size + 6.5,
                Some(FontWeight::BOLD),
                false,
                false,
                false,
                4.0,
            ),
            BlockKind::Heading(3) => (
                body_font_size + 3.0,
                Some(FontWeight::SEMIBOLD),
                false,
                false,
                false,
                3.0,
            ),
            BlockKind::Heading(_) => (
                body_font_size + 1.0,
                Some(FontWeight::SEMIBOLD),
                false,
                false,
                false,
                2.0,
            ),
            BlockKind::Code => (
                (body_font_size - 1.5).max(12.),
                None,
                true,
                false,
                false,
                4.0,
            ),
            BlockKind::Blockquote { callout: Some(_) } => {
                (body_font_size, None, false, true, false, 4.0)
            }
            BlockKind::Blockquote { callout: None } => {
                (body_font_size, None, false, false, true, 3.0)
            }
            _ => (body_font_size, None, false, false, false, 1.0),
        };
        let toolbar = self.render_selection_toolbar(slot_ix == 0, language, cx);
        let editor_height = {
            let val = self.block_editor.read(cx).value();
            let line_h = (font_size * 1.5).ceil();
            let col_chars = ((640.0 / (font_size * 0.54)).floor() as usize).max(24);
            let visual_lines: usize = val
                .as_ref()
                .split('\n')
                .map(|line| line.chars().count().max(1).div_ceil(col_chars))
                .sum::<usize>()
                .max(1);
            // `Editor` (Size::Medium) adds 8px top + 8px bottom internal padding;
            // we include those 16px in the inner height and cancel them with -8px
            // negative margins below so active text aligns pixel-for-pixel with
            // rendered inactive blocks (zero horizontal or vertical jump).
            (visual_lines as f32 * line_h + 16.0).clamp(line_h + 16.0, 580.0)
        };

        let editor = Editor::new(&self.block_editor)
            .appearance(false)
            .bordered(false)
            .h(px(editor_height))
            .text_size(px(font_size))
            .line_height(relative(1.5))
            .when_some(weight, |el, w| el.font_weight(w))
            .when(!is_code, |el| {
                el.font_family(body_font.unwrap_or_else(|| cx.theme().font_family.clone()))
            });

        let indent_px = (kind.indent() as f32) * 10.0;
        let list_gutter = self.render_list_gutter(kind, None, true, cx);

        let editor_wrap = div()
            .id("live-active-editor-wrap")
            .flex_1()
            .min_w_0()
            .ml(px(-12.))
            .mr(px(-10.))
            .mt(px(-8.))
            .mb(px(-8.))
            .key_context("RistaLiveBlock")
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .capture_action::<input::Undo>(cx.listener(|this, _, window, cx| {
                this.perform_undo(window, cx);
                cx.stop_propagation();
            }))
            .capture_action::<input::Redo>(cx.listener(|this, _, window, cx| {
                this.perform_redo(window, cx);
                cx.stop_propagation();
            }))
            .capture_action::<input::Enter>(cx.listener(
                |this, enter: &input::Enter, window, cx| {
                    if !enter.shift && this.handle_enter(window, cx) {
                        cx.stop_propagation();
                    }
                },
            ))
            .capture_action::<input::Backspace>(cx.listener(|this, _, window, cx| {
                if this.handle_backspace(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action::<input::IndentInline>(cx.listener(|this, _, window, cx| {
                if this.handle_indent(false, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action::<input::OutdentInline>(cx.listener(|this, _, window, cx| {
                if this.handle_indent(true, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action::<input::Paste>(cx.listener(|this, _, window, cx| {
                if this.handle_paste(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                match ev.keystroke.key.as_str() {
                    "escape" => {
                        this.deactivate_block(window, cx);
                        cx.stop_propagation();
                    }
                    "up" if !ev.keystroke.modifiers.shift
                        && this.handle_arrow_boundary(true, window, cx) =>
                    {
                        cx.stop_propagation();
                    }
                    "down"
                        if !ev.keystroke.modifiers.shift
                            && this.handle_arrow_boundary(false, window, cx) =>
                    {
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .on_drop::<PathBuf>(cx.listener(|this, path: &PathBuf, window, cx| {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !stem.is_empty() {
                    let link = format!("[[{stem}]]");
                    this.block_editor.update(cx, |ed, cx| {
                        ed.replace(link, window, cx);
                    });
                }
            }))
            .child(editor);

        let block_surface = h_flex()
            .w_full()
            .items_start()
            .gap_1p5()
            .when(indent_px > 0.0, |row| row.pl(px(indent_px)))
            .when(is_code, |box_el| {
                box_el
                    .rounded(px(8.))
                    .px_3()
                    .py_2()
                    .bg(theme.muted.opacity(0.65))
                    .border_1()
                    .border_color(theme.border.opacity(0.75))
            })
            .when(is_callout, |box_el| {
                box_el
                    .rounded(px(8.))
                    .px_3()
                    .py_2()
                    .bg(theme
                        .blue
                        .opacity(if cx.theme().is_dark() { 0.1 } else { 0.06 }))
                    .border_1()
                    .border_color(theme.blue.opacity(0.32))
            })
            .when(is_quote, |box_el| {
                box_el
                    .pl_3()
                    .py_0p5()
                    .border_l_2()
                    .border_color(theme.border.opacity(0.85))
            })
            .when_some(list_gutter, |row, g| row.child(g))
            .child(editor_wrap);

        v_flex()
            .id(("live-active-block", slot_ix))
            .relative()
            .w_full()
            .my(px(my_px))
            .child(block_surface)
            .when_some(toolbar, |container, bar| container.child(bar))
            .into_any_element()
    }
}

impl Render for LiveView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(workspace) = self.workspace.upgrade() else {
            return div().size_full().into_any_element();
        };

        let (
            raw,
            doc_path,
            doc_title,
            banner,
            folds,
            embeds,
            query_embeds,
            pdf_embeds,
            images,
            style,
            note_classes,
            mentions_count,
            mentions_open,
            mentions_paths,
        ) = {
            let doc = self.doc.read(cx);
            (
                doc.editor.read(cx).value().to_string(),
                doc.path.clone(),
                doc.title(),
                doc.banner.clone(),
                doc.callout_folds.clone(),
                doc.base_embeds.clone(),
                doc.query_embeds.clone(),
                doc.pdf_embeds.clone(),
                doc.image_preview.clone(),
                doc.resolved_style.clone(),
                doc.css_classes.clone(),
                doc.linked_mentions.len(),
                doc.mentions_open,
                doc.linked_mentions.clone(),
            )
        };

        let (
            vault,
            vault_root,
            settings_language,
            properties_visibility,
            readable_setting,
            ui_font_size,
            enabled_style_snippets,
        ) = {
            let ws = workspace.read(cx);
            let s = ws.settings();
            (
                ws.vault_entity().clone(),
                ws.vault_entity().read(cx).root.clone(),
                s.language,
                s.properties_visibility,
                s.readable_width,
                s.ui_font_size,
                s.enabled_style_snippets.clone(),
            )
        };

        let readable = if note_classes.iter().any(|c| c == "wide") {
            false
        } else if note_classes
            .iter()
            .any(|c| c == "narrow" || c == "readable")
            || style.readable_width.is_some()
        {
            true
        } else {
            readable_setting
        };
        let readable_width = style.readable_width.unwrap_or(720.0);
        let body_font_size = style.typography.body_size.unwrap_or(ui_font_size + 1.5);
        let theme = cx.theme().colors;
        let mark_colors = preview::MarkColors::from_theme(cx.theme());

        let parsed = parse_live_document(&raw);
        let slots = build_render_slots(
            &raw,
            &parsed,
            self.active_range.as_ref(),
            self.active_kind.as_ref(),
            &self.folded_headings,
        );
        let show_inline_title = !inline_title_duplicated(&doc_title, &parsed.blocks);

        let image_resolver = {
            let vault_ref = vault.read(cx);
            let images_map = vault_ref.images.clone();
            let root_for_img = vault_root.clone();
            move |name: &str| -> Option<PathBuf> {
                let candidate = PathBuf::from(name);
                if candidate.is_absolute() && candidate.is_file() {
                    return Some(candidate);
                }
                if let Some(root) = &root_for_img {
                    let joined = root.join(name);
                    if joined.is_file() {
                        return Some(joined);
                    }
                }
                images_map.borrow().get(&name.to_lowercase()).cloned()
            }
        };

        let base_ctx = preview::PreviewCtx {
            properties_visibility,
            language: settings_language,
            vault: vault.clone(),
            workspace: self.workspace.clone(),
            views: embeds,
            query_views: query_embeds,
            pdf_views: pdf_embeds,
            images: Some(images),
            doc_path: Some(doc_path.clone()),
            depth: 0,
            line_offset: 0,
            live_mode: true,
            style: style.clone(),
            catalog: cx
                .try_global::<crate::style_snippets::StyleSnippetCatalog>()
                .map(|c| c.snapshot.clone()),
            enabled_style_snippets,
        };

        let headings = preview::headings(&raw);
        let show_toc_rail = headings.len() >= 2;
        let view_entity = cx.entity();

        let mut column = v_flex()
            .w_full()
            .min_h_full()
            .when(readable, |el| el.max_w(px(readable_width)).mx_auto())
            .px_6()
            .pt_4()
            .pb_8();

        // ── Inline Editable Title (when the note doesn't already start with a matching H1) ──
        if show_inline_title {
            if self.editing_title {
                column = column.child(
                    div()
                        .id("live-title-edit-wrap")
                        .w_full()
                        .mb_2()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            Input::new(&self.title_input)
                                .appearance(false)
                                .bordered(false)
                                .text_size(px(body_font_size + 12.))
                                .font_bold(),
                        ),
                );
            } else {
                column = column.child(
                    div()
                        .id("live-inline-title")
                        .w_full()
                        .mb_2()
                        .cursor_text()
                        .rounded(px(6.))
                        .hover(|s| s.bg(theme.muted.opacity(0.25)))
                        .text_size(px(body_font_size + 12.))
                        .font_bold()
                        .text_color(theme.foreground)
                        .child(doc_title.clone())
                        .on_click(cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.begin_inline_title_edit(window, cx);
                        })),
                );
            }
        }

        // ── Interactive Properties Card (hiding raw YAML frontmatter) ──
        if properties_visibility != crate::settings::PropertiesVisibility::Hidden {
            if let Some((_, fm_yaml)) = &parsed.frontmatter {
                let fm_ctx = base_ctx.clone();
                let fm_view =
                    gpui_kit::base::TextView::markdown("live-properties", fm_yaml.clone())
                        .style(preview::text_style(&fm_ctx, cx))
                        .markdown_extensions(preview::extensions(&folds, Some(&fm_ctx), true))
                        .selectable(false)
                        .w_full();
                column = column.child(
                    div()
                        .id("live-properties-wrap")
                        .w_full()
                        .mb_2()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(fm_view),
                );
            } else {
                column = column.child(
                    div()
                        .id("live-properties-empty")
                        .w_full()
                        .mb_2()
                        .py_1()
                        .cursor_pointer()
                        .rounded(px(6.))
                        .hover(|row| row.bg(theme.muted.opacity(0.3)))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground.opacity(0.75))
                                .child(
                                    settings_language.text("+ Add property", "+ Legg til egenskap"),
                                ),
                        )
                        .on_click({
                            let ws = self.workspace.clone();
                            move |_, window, cx| {
                                cx.stop_propagation();
                                if let Some(ws) = ws.upgrade() {
                                    ws.update(cx, |ws, cx| ws.show_add_property_dialog(window, cx));
                                }
                            }
                        }),
                );
            }
        }

        // ── Empty Note Starter ──
        if parsed.blocks.is_empty() && self.active_range.is_none() {
            column = column.child(
                div()
                    .id("live-empty-starter")
                    .w_full()
                    .py_3()
                    .cursor_text()
                    .text_size(px(body_font_size))
                    .text_color(theme.muted_foreground.opacity(0.55))
                    .child(settings_language.text(
                        "Start writing, or type / for commands…",
                        "Begynn å skrive, eller skriv / for kommandoer…",
                    ))
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        this.append_and_focus_block(window, cx);
                    })),
            );
        }

        // ── Live Blocks (matched strictly by byte span; never duplicated) ──
        let empty_line_h = (body_font_size * 1.5).ceil();
        for (slot_ix, slot) in slots.iter().enumerate() {
            let (block_ix, collapsed_count) = match slot {
                RenderSlot::Active { kind, .. } => {
                    column = column.child(self.render_active_block(
                        slot_ix,
                        kind,
                        body_font_size,
                        style.typography.body_font.clone().map(SharedString::from),
                        settings_language,
                        cx,
                    ));
                    continue;
                }
                RenderSlot::EmptyLine { offset } => {
                    let offset = *offset;
                    column = column.child(
                        div()
                            .id(("live-empty-line", slot_ix))
                            .w_full()
                            .h(px(empty_line_h))
                            .rounded(px(6.))
                            .cursor_text()
                            .hover(|s| s.bg(theme.muted.opacity(0.10)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.activate_empty_line(offset, window, cx);
                            })),
                    );
                    continue;
                }
                RenderSlot::Block {
                    index,
                    collapsed_count,
                } => (*index, *collapsed_count),
            };
            let block = &parsed.blocks[block_ix];

            let block_ctx = preview::PreviewCtx {
                line_offset: block.start_line.saturating_sub(1),
                ..base_ctx.clone()
            };
            let ws_for_links = self.workspace.clone();

            // Render list & task items directly with their native interactive gutter +
            // inline-rendered body so active vs inactive states match pixel-for-pixel!
            let block_body_el: AnyElement = if block.kind.is_list_item() {
                let raw_body = block_editable_body(&block.kind, &block.source);
                let display_body = match block.kind {
                    BlockKind::TaskItem { checked: true, .. } if !raw_body.is_empty() => {
                        format!("~~{raw_body}~~")
                    }
                    _ => raw_body,
                };
                let display_with_defs =
                    with_reference_definitions(&display_body, &parsed.reference_definitions);
                let preprocessed = preview::preprocess(
                    &display_with_defs,
                    &doc_path,
                    vault_root.as_deref(),
                    &image_resolver,
                    &mark_colors,
                );
                let text_el = gpui_kit::base::TextView::markdown(
                    SharedString::from(format!("live-item-{}-{}", block_ix, block.start_line)),
                    preprocessed,
                )
                .style(preview::text_style(&block_ctx, cx))
                .when_some(style.typography.body_font.clone(), |el, font| {
                    el.font_family(font)
                })
                .text_size(px(body_font_size))
                .when_some(style.typography.line_height, |el, lh| {
                    el.line_height(relative(lh))
                })
                .markdown_extensions(preview::extensions(&folds, Some(&block_ctx), false))
                .selectable(false)
                .on_link_click(move |link, _ev, window, cx| {
                    Self::handle_link_click(ws_for_links.clone(), link.clone(), window, cx);
                })
                .w_full();
                let gutter =
                    self.render_list_gutter(&block.kind, Some(block.start_line), false, cx);
                let indent_px = (block.kind.indent() as f32) * 10.0;
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_1p5()
                    .when(indent_px > 0.0, |r| r.pl(px(indent_px)))
                    .when_some(gutter, |r, g| r.child(g))
                    .child(div().flex_1().min_w_0().child(text_el))
                    .into_any_element()
            } else {
                let source_for_render =
                    if matches!(block.kind, BlockKind::Code | BlockKind::ThematicBreak) {
                        block.source.clone()
                    } else {
                        with_reference_definitions(&block.source, &parsed.reference_definitions)
                    };
                let preprocessed = preview::preprocess(
                    &source_for_render,
                    &doc_path,
                    vault_root.as_deref(),
                    &image_resolver,
                    &mark_colors,
                );
                gpui_kit::base::TextView::markdown(
                    SharedString::from(format!("live-block-{}-{}", block_ix, block.start_line)),
                    preprocessed,
                )
                .style(preview::text_style(&block_ctx, cx))
                .when_some(style.typography.body_font.clone(), |el, font| {
                    el.font_family(font)
                })
                .text_size(px(body_font_size))
                .when_some(style.typography.line_height, |el, lh| {
                    el.line_height(relative(lh))
                })
                .markdown_extensions(preview::extensions(&folds, Some(&block_ctx), true))
                .selectable(false)
                .on_link_click(move |link, _ev, window, cx| {
                    Self::handle_link_click(ws_for_links.clone(), link.clone(), window, cx);
                })
                .w_full()
                .into_any_element()
            };

            let is_heading = matches!(block.kind, BlockKind::Heading(_));
            let is_table = matches!(block.kind, BlockKind::Table);
            let heading_line = block.start_line;
            let is_folded = is_heading && self.folded_headings.contains(&heading_line);
            let heading_level = match block.kind {
                BlockKind::Heading(lvl) => Some(lvl),
                _ => None,
            };
            let fold_top = match heading_level {
                Some(1) => px(10.),
                Some(2) => px(8.),
                _ => px(5.),
            };

            let mut row = h_flex()
                .id(("live-block-row", block_ix))
                .group(SharedString::from(format!("live-blk-{block_ix}")))
                .relative()
                .w_full()
                .items_start()
                .rounded(px(6.))
                .py_0p5()
                .cursor_text()
                .hover(|s| s.bg(theme.muted.opacity(0.14)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.activate_block_index(block_ix, false, window, cx);
                }));

            if is_heading {
                row = row.child(
                    div()
                        .id(("live-heading-fold", heading_line))
                        .absolute()
                        .left(px(-22.))
                        .top(fold_top)
                        .w(px(18.))
                        .h(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_color(theme.muted_foreground.opacity(if is_folded {
                            0.85
                        } else {
                            0.4
                        }))
                        .hover(|s| s.bg(theme.muted.opacity(0.5)).text_color(theme.foreground))
                        .child(
                            Icon::new(if is_folded {
                                assets::IconName::ChevronRight
                            } else {
                                assets::IconName::ChevronDown
                            })
                            .size_3(),
                        )
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !this.folded_headings.remove(&heading_line) {
                                this.folded_headings.insert(heading_line);
                            }
                            cx.notify();
                        })),
                );
            }

            row = row.child(div().flex_1().min_w_0().child(block_body_el));

            if is_folded && collapsed_count > 0 {
                row = row.child(
                    div()
                        .id(("live-fold-badge", heading_line))
                        .ml_2()
                        .mt_2()
                        .px_1p5()
                        .py_0p5()
                        .flex_shrink_0()
                        .rounded(px(5.))
                        .bg(theme.muted.opacity(0.75))
                        .text_size(px(10.5))
                        .text_color(theme.muted_foreground)
                        .cursor_pointer()
                        .child(format!("+{collapsed_count}"))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.folded_headings.remove(&heading_line);
                            cx.notify();
                        })),
                );
            } else if is_table {
                row = row.child(
                    div().absolute().top(px(14.)).right(px(-26.)).child(
                        Button::new(("live-table-raw", block_ix))
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Code)
                            .tooltip(
                                settings_language
                                    .text("Edit table Markdown", "Rediger tabell-Markdown"),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.activate_block_index(block_ix, false, window, cx);
                            })),
                    ),
                );
            }

            column = column.child(row);
        }

        // ── Click-below-to-write full-height tail canvas ──
        column = column.child(
            div()
                .id("live-tail-canvas")
                .w_full()
                .flex_1()
                .min_h(px(280.))
                .cursor_text()
                .on_click(cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    this.append_and_focus_block(window, cx);
                })),
        );

        let doc_for_mentions = self.doc.clone();
        let ws_for_mentions = self.workspace.clone();
        let root_for_mentions = vault_root.unwrap_or_default();

        v_flex()
            .size_full()
            .relative()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context("RistaLivePreview")
            .on_action(cx.listener(|this, _: &input::Undo, window, cx| {
                this.perform_undo(window, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|this, _: &input::Redo, window, cx| {
                this.perform_redo(window, cx);
                cx.stop_propagation();
            }))
            .when_some(style.colors.get("background").copied(), |el, bg| el.bg(bg))
            .when_some(banner, |el, b| el.child(crate::app::render_banner_pub(&b)))
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("live-preview-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.active_range.is_some() {
                                    this.deactivate_block(window, cx);
                                }
                            }))
                            .child(column),
                    )
                    .vertical_scrollbar(&self.scroll)
                    .when(show_toc_rail, |container| {
                        container.child(
                            v_flex()
                                .id("live-toc-rail")
                                .absolute()
                                .top(px(28.))
                                .right(px(10.))
                                .gap(px(5.))
                                .items_end()
                                .py_2()
                                .px_1()
                                .children(headings.iter().take(24).enumerate().map(
                                    |(ix, (line, level, _))| {
                                        let line = *line;
                                        let width = match level {
                                            1 => px(16.),
                                            2 => px(12.),
                                            _ => px(8.),
                                        };
                                        let is_first = ix == 0;
                                        let live_view = view_entity.clone();
                                        div()
                                            .id(("live-toc-tick", ix))
                                            .h(px(6.))
                                            .flex()
                                            .items_center()
                                            .cursor_pointer()
                                            .on_click(move |_, window, cx| {
                                                cx.stop_propagation();
                                                live_view.update(cx, |this, cx| {
                                                    this.focus_block_for_line(line, window, cx);
                                                });
                                            })
                                            .child(div().w(width).h(px(2.)).rounded(px(1.)).bg(
                                                if is_first {
                                                    theme.red.opacity(0.85)
                                                } else {
                                                    theme.muted_foreground.opacity(0.32)
                                                },
                                            ))
                                    },
                                )),
                        )
                    }),
            )
            .when(mentions_count > 0, |this| {
                this.child(
                    v_flex()
                        .w_full()
                        .when(readable, |el| el.max_w(px(readable_width)).mx_auto())
                        .px_6()
                        .py_2()
                        .border_t_1()
                        .border_color(theme.border.opacity(0.5))
                        .gap_1()
                        .child(
                            div()
                                .id("live-mentions-toggle")
                                .cursor_pointer()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .hover(|s| s.text_color(theme.foreground))
                                .child(format!(
                                    "{} {} {}",
                                    if mentions_open { "▾" } else { "›" },
                                    mentions_count,
                                    if mentions_count == 1 {
                                        settings_language.text("linked mention", "tilkoblet omtale")
                                    } else {
                                        settings_language
                                            .text("linked mentions", "tilkoblede omtaler")
                                    }
                                ))
                                .on_click(move |_, _, cx| {
                                    doc_for_mentions.update(cx, |doc, cx| {
                                        doc.mentions_open = !doc.mentions_open;
                                        cx.notify();
                                    });
                                }),
                        )
                        .when(mentions_open, |container| {
                            container.child(v_flex().gap_0p5().pl_2().children(
                                mentions_paths.into_iter().enumerate().map(|(ix, path)| {
                                    let label = path
                                        .strip_prefix(&root_for_mentions)
                                        .unwrap_or(&path)
                                        .to_string_lossy()
                                        .to_string();
                                    let open = path.clone();
                                    let ws = ws_for_mentions.clone();
                                    div()
                                        .id(("live-mention", ix))
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(5.))
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .hover(|s| {
                                            s.bg(theme.muted.opacity(0.45))
                                                .text_color(theme.foreground)
                                        })
                                        .child(label)
                                        .on_click(move |_, window, cx| {
                                            if let Some(ws) = ws.upgrade() {
                                                ws.update(cx, |ws, cx| {
                                                    ws.open_document(open.clone(), window, cx);
                                                });
                                            }
                                        })
                                }),
                            ))
                        }),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        block_editable_body, build_render_slots, detect_live_block_transform,
        inline_title_duplicated, merge_block_with_previous, parse_live_document,
        serialize_block_markdown, toggle_wrap_selection, visible_blocks, BlockKind, EditGroup,
        LiveHistory, LiveSnapshot, RenderSlot,
    };
    use std::collections::BTreeSet;
    use std::time::{Duration, Instant};

    #[test]
    fn parse_live_document_splits_list_and_task_items_with_exact_byte_ranges() {
        let raw = "---\ntags: [a]\n---\n\n# Title\n\nHello **world**\n\n> [!info] Note\n> Body\n\n- [ ] Task one\n- [x] Task two\n  - Nested bullet\n";
        let doc = parse_live_document(raw);
        assert!(doc.frontmatter.is_some());
        assert_eq!(doc.blocks.len(), 6);
        assert_eq!(doc.blocks[0].kind, BlockKind::Heading(1));
        assert_eq!(&raw[doc.blocks[0].range.clone()], "# Title");
        assert_eq!(doc.blocks[0].start_line, 5);
        assert_eq!(doc.blocks[1].kind, BlockKind::Paragraph);
        assert_eq!(
            doc.blocks[2].kind,
            BlockKind::Blockquote {
                callout: Some("info".to_string())
            }
        );
        assert_eq!(
            doc.blocks[3].kind,
            BlockKind::TaskItem {
                indent: 0,
                marker: '-',
                checked: false
            }
        );
        assert_eq!(&raw[doc.blocks[3].range.clone()], "- [ ] Task one");
        assert_eq!(
            doc.blocks[4].kind,
            BlockKind::TaskItem {
                indent: 0,
                marker: '-',
                checked: true
            }
        );
        assert_eq!(&raw[doc.blocks[4].range.clone()], "- [x] Task two");
        assert_eq!(
            doc.blocks[5].kind,
            BlockKind::BulletItem {
                indent: 2,
                marker: '-'
            }
        );
        assert_eq!(&raw[doc.blocks[5].range.clone()], "  - Nested bullet");
    }

    #[test]
    fn editable_body_and_serialization_strip_and_restore_block_prefixes() {
        let h = BlockKind::Heading(2);
        assert_eq!(block_editable_body(&h, "## Release plan"), "Release plan");
        assert_eq!(
            serialize_block_markdown(&h, "Release plan"),
            "## Release plan"
        );

        let t = BlockKind::TaskItem {
            indent: 2,
            marker: '-',
            checked: false,
        };
        assert_eq!(
            block_editable_body(&t, "  - [ ] Review [[Docs]]"),
            "Review [[Docs]]"
        );
        assert_eq!(
            serialize_block_markdown(&t, "Review [[Docs]]"),
            "  - [ ] Review [[Docs]]"
        );

        let b = BlockKind::BulletItem {
            indent: 0,
            marker: '-',
        };
        assert_eq!(block_editable_body(&b, "- Item"), "Item");
        assert_eq!(serialize_block_markdown(&b, "Item"), "- Item");

        let q = BlockKind::Blockquote { callout: None };
        assert_eq!(
            block_editable_body(&q, "> Line 1\n> Line 2"),
            "Line 1\nLine 2"
        );
        assert_eq!(
            serialize_block_markdown(&q, "Line 1\nLine 2"),
            "> Line 1\n> Line 2"
        );
    }

    #[test]
    fn live_block_transforms_convert_markdown_triggers_without_showing_syntax() {
        assert_eq!(
            detect_live_block_transform(&BlockKind::Paragraph, "## "),
            Some((BlockKind::Heading(2), String::new()))
        );
        assert_eq!(
            detect_live_block_transform(&BlockKind::Paragraph, "- "),
            Some((
                BlockKind::BulletItem {
                    indent: 0,
                    marker: '-'
                },
                String::new()
            ))
        );
        assert_eq!(
            detect_live_block_transform(&BlockKind::Paragraph, "[] "),
            Some((
                BlockKind::TaskItem {
                    indent: 0,
                    marker: '-',
                    checked: false
                },
                String::new()
            ))
        );
        assert_eq!(
            detect_live_block_transform(&BlockKind::Paragraph, "3. "),
            Some((
                BlockKind::OrderedItem {
                    indent: 0,
                    number: 3,
                    sep: '.'
                },
                String::new()
            ))
        );
        assert_eq!(
            detect_live_block_transform(&BlockKind::Paragraph, "> "),
            Some((BlockKind::Blockquote { callout: None }, String::new()))
        );
        assert_eq!(
            detect_live_block_transform(
                &BlockKind::BulletItem {
                    indent: 2,
                    marker: '-'
                },
                "[ ] "
            ),
            Some((
                BlockKind::TaskItem {
                    indent: 2,
                    marker: '-',
                    checked: false
                },
                String::new()
            ))
        );
    }

    #[test]
    fn visible_blocks_folds_section_until_equal_or_higher_heading() {
        let raw = "# Top\n\nIntro\n\n## Section A\n\nPara A\n\n### Sub A\n\nSub para\n\n## Section B\n\nPara B\n";
        let doc = parse_live_document(raw);
        let section_a_line = doc.blocks[2].start_line;
        let folded = BTreeSet::from([section_a_line]);

        let vis = visible_blocks(&doc.blocks, &folded, None);
        assert_eq!(vis.len(), 5);
        assert_eq!(vis[2].index, 2);
        assert_eq!(vis[2].collapsed_count, 3);
        assert_eq!(vis[3].index, 6);

        let active_range = doc.blocks[4].range.clone();
        let vis_active = visible_blocks(&doc.blocks, &folded, Some(&active_range));
        assert_eq!(vis_active.len(), doc.blocks.len());
    }

    #[test]
    fn build_render_slots_never_duplicates_active_block_or_drifts_past_following_blocks() {
        let raw = "paragraph one\n\nparagraph two\n\n\n\nwewe\n\nasdasds\n\n\n\ndd\n\ndddd\n\nd\n";
        let doc = parse_live_document(raw);
        let asdasds_start = raw.find("asdasds").unwrap();
        let active_range = asdasds_start..(asdasds_start + "asdasds".len());
        let slots = build_render_slots(
            raw,
            &doc,
            Some(&active_range),
            Some(&BlockKind::Paragraph),
            &BTreeSet::new(),
        );

        // Exactly one Active slot, positioned right after `wewe` and BEFORE `dd`, `dddd`, `d`.
        let active_positions: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| matches!(s, RenderSlot::Active { .. }).then_some(i))
            .collect();
        assert_eq!(active_positions.len(), 1);
        let act_pos = active_positions[0];

        // None of the Block slots may refer to `asdasds`.
        for slot in &slots {
            if let RenderSlot::Block { index, .. } = slot {
                assert_ne!(doc.blocks[*index].source, "asdasds");
            }
        }
        // `dd` must appear AFTER the active slot.
        let dd_pos = slots
            .iter()
            .position(|s| matches!(s, RenderSlot::Block { index, .. } if doc.blocks[*index].source == "dd"))
            .unwrap();
        assert!(act_pos < dd_pos);
    }

    #[test]
    fn build_render_slots_emits_visible_empty_lines_on_repeated_enters() {
        // After "hello\n\n" (normal break) + two extra Enters -> "\n\n\n\n" with active at EOF:
        let raw = "hello\n\n\n\n";
        let doc = parse_live_document(raw);
        let eof = raw.len();
        let slots = build_render_slots(
            raw,
            &doc,
            Some(&(eof..eof)),
            Some(&BlockKind::Paragraph),
            &BTreeSet::new(),
        );
        // Block("hello"), EmptyLine, EmptyLine, Active(eof..eof)
        assert_eq!(slots.len(), 4);
        assert!(matches!(slots[0], RenderSlot::Block { index: 0, .. }));
        assert!(matches!(slots[1], RenderSlot::EmptyLine { .. }));
        assert!(matches!(slots[2], RenderSlot::EmptyLine { .. }));
        assert!(matches!(slots[3], RenderSlot::Active { .. }));

        // Pressing Enter between two paragraphs (`hello\n\n\n\ndd` with active at 7..7)
        // produces ZERO stray empty lines around the active slot!
        let mid = "hello\n\n\n\ndd";
        let mid_doc = parse_live_document(mid);
        let mid_slots = build_render_slots(
            mid,
            &mid_doc,
            Some(&(7..7)),
            Some(&BlockKind::Paragraph),
            &BTreeSet::new(),
        );
        assert_eq!(mid_slots.len(), 3);
        assert!(matches!(mid_slots[0], RenderSlot::Block { index: 0, .. }));
        assert!(matches!(mid_slots[1], RenderSlot::Active { .. }));
        assert!(matches!(mid_slots[2], RenderSlot::Block { index: 1, .. }));
    }

    #[test]
    fn merge_block_with_previous_preserves_wysiwyg_kind_and_caret() {
        let raw = "# Heading\n\nSecond paragraph";
        let doc = parse_live_document(raw);
        let (kind, body, range, caret) = merge_block_with_previous(raw, &doc.blocks, 1).unwrap();
        assert_eq!(kind, BlockKind::Heading(1));
        assert_eq!(body, "Heading Second paragraph");
        assert_eq!(range, 0..raw.len());
        assert_eq!(caret, "Heading ".len());
    }

    #[test]
    fn toggle_wrap_selection_wraps_and_unwraps_cleanly() {
        let (wrapped, sel) = toggle_wrap_selection("hello world", 6..11, "**");
        assert_eq!(wrapped, "hello **world**");
        assert_eq!(sel, 8..13);

        let (unwrapped, sel2) = toggle_wrap_selection(&wrapped, sel, "**");
        assert_eq!(unwrapped, "hello world");
        assert_eq!(sel2, 6..11);
    }

    #[test]
    fn inline_title_duplication_matches_case_insensitively() {
        let doc = parse_live_document("# Website Relaunch\n\nBody");
        assert!(inline_title_duplicated("Website relaunch", &doc.blocks));
        assert!(!inline_title_duplicated("Other Note", &doc.blocks));
    }

    #[test]
    fn live_history_coalesces_typing_bursts_and_reverses_across_blocks() {
        let mut h = LiveHistory::default();
        let t0 = Instant::now();
        let snap = |text: &str, range: std::ops::Range<usize>, caret: usize| LiveSnapshot {
            doc_text: text.to_string(),
            active_range: Some(range),
            active_kind: Some(BlockKind::Paragraph),
            caret,
        };

        // Rapid typing burst in block 0..: "a", "ab", "abc" coalesces to 1 undo step.
        h.record_before_edit(snap("", 0..0, 0), EditGroup::Typing, t0);
        h.record_before_edit(
            snap("a", 0..1, 1),
            EditGroup::Typing,
            t0 + Duration::from_millis(80),
        );
        h.record_before_edit(
            snap("ab", 0..2, 2),
            EditGroup::Typing,
            t0 + Duration::from_millis(160),
        );

        // Discrete Enter creates its own checkpoint.
        h.record_before_edit(
            snap("abc", 0..3, 3),
            EditGroup::Discrete,
            t0 + Duration::from_millis(200),
        );

        // Typing in the new block (5..): starts a separate burst.
        h.record_before_edit(
            snap("abc\n\n", 5..5, 0),
            EditGroup::Typing,
            t0 + Duration::from_millis(250),
        );
        h.record_before_edit(
            snap("abc\n\nx", 5..6, 1),
            EditGroup::Typing,
            t0 + Duration::from_millis(320),
        );

        // Undo 1: reverts "xy" back to empty 2nd paragraph.
        let s1 = h.undo(snap("abc\n\nxy", 5..7, 2)).unwrap();
        assert_eq!(s1.doc_text, "abc\n\n");
        assert_eq!(s1.active_range, Some(5..5));

        // Undo 2: reverts Enter back to "abc|".
        let s2 = h.undo(s1).unwrap();
        assert_eq!(s2.doc_text, "abc");
        assert_eq!(s2.caret, 3);

        // Undo 3: reverts whole typing burst back to "".
        let s3 = h.undo(s2).unwrap();
        assert_eq!(s3.doc_text, "");
        assert!(h.undo(s3.clone()).is_none());

        // Redo walks forward through all 3 states cleanly.
        let r1 = h.redo(s3).unwrap();
        assert_eq!(r1.doc_text, "abc");
        let r2 = h.redo(r1).unwrap();
        assert_eq!(r2.doc_text, "abc\n\n");
        let r3 = h.redo(r2).unwrap();
        assert_eq!(r3.doc_text, "abc\n\nxy");
        assert!(h.redo(r3.clone()).is_none());

        // New edit after Undo clears the redo branch.
        let back = h.undo(r3).unwrap();
        h.record_before_edit(back, EditGroup::Typing, t0 + Duration::from_secs(2));
        assert!(h.redo(snap("abc\n\nz", 5..6, 1)).is_none());
    }

    #[test]
    fn toggle_wrap_selection_handles_multibyte_utf8_neighbors_without_panicking() {
        let (wrapped, sel) = toggle_wrap_selection("你好世界", 3..9, "*");
        assert_eq!(wrapped, "你*好世*界");
        assert_eq!(sel, 4..10);

        let (unwrapped, sel2) = toggle_wrap_selection(&wrapped, sel, "*");
        assert_eq!(unwrapped, "你好世界");
        assert_eq!(sel2, 3..9);

        // Two-byte ASCII marker next to 3-byte / 4-byte UTF-8 chars also stays safe.
        let (bolded, sel3) = toggle_wrap_selection("🎉你好🎉", 4..10, "**");
        assert_eq!(bolded, "🎉**你好**🎉");
        assert_eq!(sel3, 6..12);
    }

    #[test]
    fn block_editable_body_strips_setext_heading_underlines_and_closing_hashes() {
        let raw = "Main Title\n=====\n\nSubheading\n-----\n\n## ATX Closed ##";
        let doc = parse_live_document(raw);
        assert_eq!(doc.blocks.len(), 3);
        assert_eq!(doc.blocks[0].kind, BlockKind::Heading(1));
        assert_eq!(
            block_editable_body(&doc.blocks[0].kind, &doc.blocks[0].source),
            "Main Title"
        );
        assert_eq!(
            serialize_block_markdown(&doc.blocks[0].kind, "Main Title!"),
            "# Main Title!"
        );

        assert_eq!(doc.blocks[1].kind, BlockKind::Heading(2));
        assert_eq!(
            block_editable_body(&doc.blocks[1].kind, &doc.blocks[1].source),
            "Subheading"
        );

        assert_eq!(doc.blocks[2].kind, BlockKind::Heading(2));
        assert_eq!(
            block_editable_body(&doc.blocks[2].kind, &doc.blocks[2].source),
            "ATX Closed"
        );
    }

    #[test]
    fn reference_definitions_are_preserved_for_fragment_rendering() {
        let raw = "See [Read more][guide] and ![Diagram][fig].\n\n[guide]: https://example.com\n[fig]: https://example.com/fig.png";
        let doc = parse_live_document(raw);
        assert!(doc
            .reference_definitions
            .contains("[guide]: https://example.com"));
        assert!(doc
            .reference_definitions
            .contains("[fig]: https://example.com/fig.png"));

        let fragment =
            super::with_reference_definitions(&doc.blocks[0].source, &doc.reference_definitions);
        let ast = markdown::to_mdast(&fragment, &markdown::ParseOptions::gfm()).unwrap();
        let markdown::mdast::Node::Root(root) = ast else {
            panic!("expected root");
        };
        let markdown::mdast::Node::Paragraph(para) = &root.children[0] else {
            panic!("expected paragraph");
        };
        assert!(para
            .children
            .iter()
            .any(|n| matches!(n, markdown::mdast::Node::LinkReference(_))));
        assert!(para
            .children
            .iter()
            .any(|n| matches!(n, markdown::mdast::Node::ImageReference(_))));
        assert!(root.children.iter().any(
            |n| matches!(n, markdown::mdast::Node::Definition(d) if d.url == "https://example.com")
        ));
        assert!(root.children.iter().any(|n| matches!(
            n,
            markdown::mdast::Node::Definition(d) if d.url == "https://example.com/fig.png"
        )));
    }
}
