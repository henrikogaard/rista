//! Live-preview emphasis in the source editor: markdown syntax markers
//! dim (`fade_out`), inline content picks up real styling — bold, italic,
//! strike, heading weight, link accent — so Source mode reads closer to
//! the rendered note without breaking the mono grid.
//!
//! Ranges are UTF-8 byte offsets into the editor buffer. The
//! `TextDecorationCollection` tracks edits itself; we just rebuild the
//! set on each debounced preview sync.

use gpui_kit::component::input::TextDecoration;
use gpui_kit::*;
use markdown::mdast;
use std::ops::Range;

/// Syntax-marker fade — `**`, `#`, `>`, `[](…)`, `---` delimiters.
const MARKER_FADE: f32 = 0.55;
/// Syntax-marker fade when cursor-aware reveal is active and the marker is on the caret line.
const ACTIVE_LINE_MARKER_FADE: f32 = 0.32;
/// Syntax-marker fade when cursor-aware reveal is active and the marker is off the caret line.
const OFF_LINE_MARKER_FADE: f32 = 0.80;
/// Whole-node fades — YAML frontmatter, images, thematic breaks.
const BLOCK_FADE: f32 = 0.45;
/// Text of a completed `- [x]` task.
const DONE_FADE: f32 = 0.4;
/// Focus mode: whole blocks away from the cursor.
const FOCUS_FADE: f32 = 0.6;

fn line_span_at(text: &str, offset: usize) -> Range<usize> {
    let clamped = offset.min(text.len());
    let start = text[..clamped].rfind('\n').map_or(0, |i| i + 1);
    let end = text[clamped..]
        .find('\n')
        .map_or(text.len(), |i| clamped + i);
    start..end
}

fn marker_fade_for(range: &Range<usize>, active_line: Option<&Range<usize>>) -> f32 {
    match active_line {
        None => MARKER_FADE,
        Some(line) => {
            if range.start <= line.end && range.end >= line.start {
                ACTIVE_LINE_MARKER_FADE
            } else {
                OFF_LINE_MARKER_FADE
            }
        }
    }
}

fn fade(amount: f32) -> HighlightStyle {
    HighlightStyle {
        fade_out: Some(amount),
        ..Default::default()
    }
}

fn node_range(node: &mdast::Node) -> Option<Range<usize>> {
    node.position().map(|p| p.start.offset..p.end.offset)
}

/// Byte gaps of `node` not covered by any direct child — the syntax
/// markers around inline content (`**`, `](…)`, `> `, `- `, `# `).
/// For `*a *b* c*`, emphasis affixes are the two outer `*`; the inner
/// Emphasis is a child, so its own markers stay content to it.
fn affixes(node: &mdast::Node) -> Vec<Range<usize>> {
    let Some(range) = node_range(node) else {
        return Vec::new();
    };
    let mut spans: Vec<Range<usize>> = node
        .children()
        .map(|c| c.iter().filter_map(node_range).collect())
        .unwrap_or_default();
    spans.sort_by_key(|r| r.start);
    let mut gaps = Vec::new();
    let mut at = range.start;
    for span in spans {
        if span.start > at {
            gaps.push(at..span.start);
        }
        at = at.max(span.end);
    }
    if at < range.end {
        gaps.push(at..range.end);
    }
    gaps
}

/// The complement of [`affixes`]: merged child ranges — where inline
/// styling should land.
fn content_spans(node: &mdast::Node) -> Vec<Range<usize>> {
    let range = match node_range(node) {
        Some(r) => r,
        None => return Vec::new(),
    };
    let mut spans: Vec<Range<usize>> = node
        .children()
        .map(|c| c.iter().filter_map(node_range).collect())
        .unwrap_or_default();
    spans.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for span in spans {
        if span.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.end >= span.start => last.end = last.end.max(span.end),
            _ => out.push(span),
        }
    }
    if out.is_empty() {
        vec![range]
    } else {
        out
    }
}

fn mark(out: &mut Vec<TextDecoration>, range: Range<usize>, style: HighlightStyle) {
    if !range.is_empty() {
        out.push(TextDecoration::new(range, style));
    }
}

fn mark_spans(out: &mut Vec<TextDecoration>, spans: Vec<Range<usize>>, style: HighlightStyle) {
    for span in spans {
        mark(out, span, style);
    }
}

fn mark_affix(
    out: &mut Vec<TextDecoration>,
    range: Range<usize>,
    active_line: Option<&Range<usize>>,
) {
    if !range.is_empty() {
        let amount = marker_fade_for(&range, active_line);
        mark(out, range, fade(amount));
    }
}

fn mark_affixes(
    out: &mut Vec<TextDecoration>,
    spans: Vec<Range<usize>>,
    active_line: Option<&Range<usize>>,
) {
    for span in spans {
        mark_affix(out, span, active_line);
    }
}

/// `[[note]]` / `![[image]]` inside a Text node (CommonMark doesn't
/// know them) — accent the whole span, dim the bracket pairs.
fn wikilinks(
    text: &str,
    base: usize,
    out: &mut Vec<TextDecoration>,
    accent: HighlightStyle,
    active_line: Option<&Range<usize>>,
) {
    let mut i = 0;
    let bytes = text.as_bytes();
    while i + 1 < bytes.len() {
        let bang = bytes[i] == b'!';
        let at = if bang { i + 1 } else { i };
        if bytes[at] != b'[' || bytes.get(at + 1) != Some(&b'[') {
            i += 1;
            continue;
        }
        let Some(close) = text[at + 2..].find("]]").map(|j| at + 2 + j) else {
            break;
        };
        let start = base + i;
        let end = base + close + 2;
        mark(out, start..end, accent);
        mark_affix(out, start..base + at + 2, active_line);
        mark_affix(out, base + close..end, active_line);
        i = close + 2;
    }
}

fn walk(
    node: &mdast::Node,
    text: &str,
    out: &mut Vec<TextDecoration>,
    theme: &component::Theme,
    active_line: Option<&Range<usize>>,
) {
    match node {
        mdast::Node::Strong(_) => {
            mark_spans(
                out,
                content_spans(node),
                HighlightStyle {
                    font_weight: Some(FontWeight::SEMIBOLD),
                    ..Default::default()
                },
            );
            mark_affixes(out, affixes(node), active_line);
        }
        mdast::Node::Emphasis(_) => {
            mark_spans(
                out,
                content_spans(node),
                HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                },
            );
            mark_affixes(out, affixes(node), active_line);
        }
        mdast::Node::Delete(_) => {
            mark_spans(
                out,
                content_spans(node),
                HighlightStyle {
                    strikethrough: Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    }),
                    ..Default::default()
                },
            );
            mark_affixes(out, affixes(node), active_line);
        }
        mdast::Node::InlineCode(_) => {
            if let Some(range) = node_range(node) {
                mark(
                    out,
                    range.clone(),
                    HighlightStyle {
                        background_color: Some(theme.secondary),
                        ..Default::default()
                    },
                );
                // A leaf — dim the backtick runs at each edge by hand.
                let seg = &text[range.clone()];
                let head = seg.len() - seg.trim_start_matches('`').len();
                let tail = seg.len() - seg.trim_end_matches('`').len();
                mark_affix(out, range.start..range.start + head, active_line);
                mark_affix(out, range.end - tail..range.end, active_line);
            }
        }
        mdast::Node::Link(_) | mdast::Node::LinkReference(_) => {
            // `- [x]` parses as a shortcut link reference — `task_markers`
            // owns that bracket, and same-property overlaps inside one
            // collection resolve nondeterministically, so skip it here.
            if let Some(range) = node_range(node) {
                if !matches!(&text[range], "[ ]" | "[x]" | "[X]") {
                    mark_spans(
                        out,
                        content_spans(node),
                        HighlightStyle {
                            color: Some(theme.info),
                            ..Default::default()
                        },
                    );
                    mark_affixes(out, affixes(node), active_line);
                }
            }
        }
        mdast::Node::Image(_) | mdast::Node::ImageReference(_) => {
            if let Some(range) = node_range(node) {
                mark(out, range, fade(BLOCK_FADE));
            }
        }
        mdast::Node::Heading(_) => {
            if let Some(range) = node_range(node) {
                mark(
                    out,
                    range,
                    HighlightStyle {
                        font_weight: Some(FontWeight::BOLD),
                        ..Default::default()
                    },
                );
            }
            mark_affixes(out, affixes(node), active_line);
        }
        mdast::Node::Yaml(_) | mdast::Node::Toml(_) => {
            if let Some(range) = node_range(node) {
                mark(out, range, fade(BLOCK_FADE));
            }
        }
        mdast::Node::Code(_) => {
            if let Some(range) = node_range(node) {
                mark(
                    out,
                    range,
                    HighlightStyle {
                        background_color: Some(theme.secondary),
                        ..Default::default()
                    },
                );
            }
        }
        mdast::Node::ListItem(_) => {
            // `- ` bullets dim; a task item's affix also covers the
            // `[ ]`/`[x]` bracket — carve it out so the accent from
            // `task_markers` never fights a same-property fade.
            for affix in affixes(node) {
                let seg = &text[affix.clone()];
                if let Some(b) = seg.find('[') {
                    match seg.as_bytes().get(b + 1..b + 3) {
                        Some(b" ]") | Some(b"x]") | Some(b"X]") => {
                            mark_affix(out, affix.start..affix.start + b, active_line);
                            mark_affix(out, affix.start + b + 3..affix.end, active_line);
                            continue;
                        }
                        _ => {}
                    }
                }
                mark_affix(out, affix, active_line);
            }
        }
        mdast::Node::Blockquote(_) => {
            // Callouts `> [!type]` tint the whole quote and accent the
            // `[!type]` token, matching the preview's colored blocks.
            let callout = node
                .children()
                .and_then(|c| c.first())
                .and_then(node_range)
                .filter(|r| text[r.clone()].trim_start().starts_with("[!"));
            if let (Some(range), Some(head)) = (node_range(node), callout) {
                mark(
                    out,
                    range,
                    HighlightStyle {
                        background_color: Some(theme.secondary),
                        ..Default::default()
                    },
                );
                let seg = &text[head.clone()];
                if let Some(end) = seg.find(']') {
                    mark(
                        out,
                        head.start..head.start + end + 1,
                        HighlightStyle {
                            color: Some(theme.info),
                            font_weight: Some(FontWeight::SEMIBOLD),
                            ..Default::default()
                        },
                    );
                }
            }
            mark_affixes(out, affixes(node), active_line);
        }
        mdast::Node::ThematicBreak(_) | mdast::Node::Break(_) => {
            if let Some(range) = node_range(node) {
                mark_affix(out, range, active_line);
            }
        }
        _ => {}
    }
    if let Some(children) = node.children() {
        for child in children {
            walk(child, text, out, theme, active_line);
        }
    }
}

/// `- [ ]`/`- [x]` line pass — mdast marks the item but not the
/// bracket's position. `[x]` accents the marker and fades the rest
/// of the line, mirroring the preview's strike-through. The marker
/// also lands inside the ListItem's marker-affix fade — an explicit
/// `fade_out: 0` overrides it (first style on a property wins).
fn task_markers(text: &str, base: usize, out: &mut Vec<TextDecoration>, accent: HighlightStyle) {
    let mut at = 0usize;
    for line in text.split_inclusive('\n') {
        let seg = line.trim_end_matches(['\n', '\r']);
        if let Some(b) = seg.find('[') {
            let prefix = seg[..b].trim_end();
            let bullet = prefix.ends_with('-')
                || prefix.ends_with('*')
                || prefix.ends_with('+')
                || prefix
                    .rsplit(' ')
                    .next()
                    .and_then(|t| t.strip_suffix('.'))
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            if bullet {
                match seg.as_bytes().get(b + 1..b + 3) {
                    Some(b" ]") => mark(
                        out,
                        base + at + b..base + at + b + 3,
                        HighlightStyle {
                            fade_out: Some(0.),
                            ..accent
                        },
                    ),
                    Some(b"x]") | Some(b"X]") => {
                        mark(
                            out,
                            base + at + b..base + at + b + 3,
                            HighlightStyle {
                                fade_out: Some(0.),
                                ..accent
                            },
                        );
                        mark(
                            out,
                            base + at + b + 3..base + at + seg.len(),
                            fade(DONE_FADE),
                        );
                    }
                    _ => {}
                }
            }
        }
        at += line.len();
    }
}

/// `#tag` word pass — the reference editor colors tags; headings are safe (`# ` has
/// a space after the marker). Skips fenced code and `[[link#target]]`
/// targets so no color fights the wikilink accent.
fn tags(text: &str, base: usize, out: &mut Vec<TextDecoration>, color: HighlightStyle) {
    let mut at = 0usize;
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let seg = line.trim_end_matches(['\n', '\r']);
        if seg.trim_start().starts_with("```") {
            fenced = !fenced;
            at += line.len();
            continue;
        }
        if !fenced {
            let bytes = seg.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'#'
                    && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t')
                    && bytes.get(i + 1).is_some_and(|c| c.is_ascii_alphanumeric())
                {
                    // `[[link#target]]` — the wikilink accent owns it.
                    let before = &seg[..i];
                    let in_wiki = match (before.rfind("[["), before.rfind("]]")) {
                        (Some(o), c) => c.is_none_or(|c| o > c),
                        _ => false,
                    };
                    if !in_wiki {
                        let end = bytes[i + 1..]
                            .iter()
                            .position(|c| {
                                !(c.is_ascii_alphanumeric()
                                    || *c == b'_'
                                    || *c == b'-'
                                    || *c == b'/')
                            })
                            .map(|p| i + 1 + p)
                            .unwrap_or(bytes.len());
                        mark(out, base + at + i..base + at + end, color);
                        i = end;
                        continue;
                    }
                }
                i += 1;
            }
        }
        at += line.len();
    }
}

/// `%%` the reference editor comment regions — inline or spanning lines; a lone
/// trailing `%%` comments out to EOF. Fenced code stays literal. The
/// emitted style is fade-only so it composes deterministically with
/// whatever color the other passes put inside a commented span.
fn comments(text: &str, base: usize, out: &mut Vec<TextDecoration>, style: HighlightStyle) {
    let mut open: Option<usize> = None;
    let mut fenced = false;
    let mut at = 0usize;
    for line in text.split_inclusive('\n') {
        let seg = line.trim_end_matches(['\n', '\r']);
        if seg.trim_start().starts_with("```") && open.is_none() {
            fenced = !fenced;
            at += line.len();
            continue;
        }
        if !fenced {
            let bytes = seg.as_bytes();
            let mut i = 0;
            while i + 1 < bytes.len() {
                if bytes[i] == b'%' && bytes[i + 1] == b'%' {
                    match open {
                        None => open = Some(at + i),
                        Some(start) => {
                            mark(out, base + start..base + at + i + 2, style);
                            open = None;
                        }
                    }
                    i += 2;
                    continue;
                }
                i += 1;
            }
        }
        at += line.len();
    }
    // Unclosed `%%` runs to the end of the note,.
    if let Some(start) = open {
        mark(out, base + start..base + text.len(), style);
    }
}

/// `==highlight==` spans get a translucent wash like the preview's
/// `<mark>` — `===` (setext underline) is not a highlight. Fenced code
/// stays literal. A leading color emoji (`==🔴text==`) picks the wash
/// from `colors` (see `preview::HIGHLIGHT_COLORS`).
fn highlights(
    text: &str,
    base: usize,
    out: &mut Vec<TextDecoration>,
    style: HighlightStyle,
    colors: &[gpui_kit::Hsla; 6],
) {
    let mut open: Option<usize> = None;
    let mut fenced = false;
    let mut at = 0usize;
    for line in text.split_inclusive('\n') {
        let seg = line.trim_end_matches(['\n', '\r']);
        if seg.trim_start().starts_with("```") && open.is_none() {
            fenced = !fenced;
            at += line.len();
            continue;
        }
        if !fenced {
            let bytes = seg.as_bytes();
            let mut i = 0;
            while i + 1 < bytes.len() {
                if bytes[i] == b'='
                    && bytes[i + 1] == b'='
                    && bytes.get(i + 2) != Some(&b'=')
                    && (i == 0 || bytes[i - 1] != b'=')
                {
                    match open {
                        None => open = Some(at + i),
                        Some(start) => {
                            let inner = &text[start + 2..];
                            let color = crate::preview::HIGHLIGHT_COLORS
                                .iter()
                                .position(|(emoji, _)| inner.starts_with(emoji))
                                .map(|ix| HighlightStyle {
                                    background_color: Some(colors[ix].opacity(0.25)),
                                    ..style
                                });
                            mark(out, base + start..base + at + i + 2, color.unwrap_or(style));
                            open = None;
                        }
                    }
                    i += 2;
                    continue;
                }
                i += 1;
            }
        }
        at += line.len();
    }
}

/// Root-child holding `cursor` — the one block that stays lit in focus
/// mode. Falls to the next block when the caret sits on an empty line,
/// else the last block at EOF.
fn focus_zone(children: &[mdast::Node], cursor: usize) -> Option<(usize, &mdast::Node)> {
    children
        .iter()
        .enumerate()
        .find(|(_, c)| {
            node_range(c)
                .map(|r| r.start <= cursor && cursor <= r.end)
                .unwrap_or(false)
        })
        .or_else(|| {
            children
                .iter()
                .enumerate()
                .find(|(_, c)| node_range(c).map(|r| r.start > cursor).unwrap_or(false))
        })
        .or_else(|| children.iter().enumerate().next_back())
}

/// All decorations for `text`, styled against the active theme. When
/// `cursor` is given (focus mode), only the block under the caret gets
/// the full treatment — every other top-level block fades wholesale.
#[allow(dead_code)]
pub fn markdown_decorations(
    text: &str,
    theme: &component::Theme,
    cursor: Option<usize>,
) -> Vec<TextDecoration> {
    markdown_decorations_with_caret(text, theme, cursor, None)
}

/// Cursor-aware decorations: when `active_caret` is supplied, syntax affixes
/// (`**`, `# `, `[[`, `]]`, etc.) fade strongly off the active line and reveal
/// crisply on the line currently holding the caret.
pub fn markdown_decorations_with_caret(
    text: &str,
    theme: &component::Theme,
    cursor: Option<usize>,
    active_caret: Option<usize>,
) -> Vec<TextDecoration> {
    let active_line = active_caret.map(|offset| line_span_at(text, offset));
    let mut constructs = markdown::Constructs::gfm();
    constructs.frontmatter = true;
    let options = markdown::ParseOptions {
        constructs,
        ..markdown::ParseOptions::gfm()
    };
    let accent = HighlightStyle {
        color: Some(theme.info),
        ..Default::default()
    };
    let mut out = Vec::new();
    if let Ok(root) = markdown::to_mdast(text, &options) {
        match cursor {
            Some(cursor) => {
                let children = root.children().cloned().unwrap_or_default();
                let zone = focus_zone(&children, cursor).map(|(ix, _)| ix);
                for (ix, child) in children.into_iter().enumerate() {
                    if Some(ix) == zone {
                        if let Some(range) = node_range(&child) {
                            // The lit block: full passes inside its span.
                            let zone_text = &text[range.clone()];
                            task_markers(zone_text, range.start, &mut out, accent);
                            wikilinks(
                                zone_text,
                                range.start,
                                &mut out,
                                accent,
                                active_line.as_ref(),
                            );
                        }
                        walk(&child, text, &mut out, theme, active_line.as_ref());
                    } else if let Some(range) = node_range(&child) {
                        mark(&mut out, range, fade(FOCUS_FADE));
                    }
                }
            }
            None => {
                // Task markers first: the line pass owns `[ ]`/`[x]`
                // styling, so the ListItem affix fade in `walk` carves
                // those brackets out — combining same-property
                // decorations resolves nondeterministically, so passes
                // must never emit overlapping fades/colors.
                task_markers(text, 0, &mut out, accent);
                // `[[wikilink]]` spans live inside mdast Text nodes —
                // one textual pass covers them wherever they appear.
                wikilinks(text, 0, &mut out, accent, active_line.as_ref());
                walk(&root, text, &mut out, theme, active_line.as_ref());
            }
        }
    }
    // Color-only — safe under either mode's fades.
    tags(text, 0, &mut out, accent);
    // Fade-only — composes with whatever styles land inside the span.
    comments(text, 0, &mut out, fade(0.55));
    // `==highlight==` wash — background-only so text colors inside
    // the span keep their own styling.
    highlights(
        text,
        0,
        &mut out,
        HighlightStyle {
            background_color: Some(theme.warning.opacity(0.25)),
            ..Default::default()
        },
        &crate::preview::highlight_palette(theme),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::{line_span_at, marker_fade_for};

    #[test]
    fn active_line_syntax_reveals_crisper_than_inactive_lines() {
        let text = "# Heading\n\nSecond **bold** line";
        let line0 = line_span_at(text, 2);
        let line2 = line_span_at(text, text.len() - 2);
        assert_eq!(&text[line0.clone()], "# Heading");
        assert_eq!(&text[line2.clone()], "Second **bold** line");
        assert!(marker_fade_for(&(0..2), Some(&line0)) < marker_fade_for(&(0..2), Some(&line2)));
        assert!(
            marker_fade_for(&(18..20), Some(&line2)) < marker_fade_for(&(18..20), Some(&line0))
        );
    }
}
