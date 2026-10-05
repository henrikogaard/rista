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
/// Whole-node fades — YAML frontmatter, images, thematic breaks.
const BLOCK_FADE: f32 = 0.45;
/// Text of a completed `- [x]` task.
const DONE_FADE: f32 = 0.4;

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

/// `[[note]]` / `![[image]]` inside a Text node (CommonMark doesn't
/// know them) — accent the whole span, dim the bracket pairs.
fn wikilinks(text: &str, base: usize, out: &mut Vec<TextDecoration>, accent: HighlightStyle) {
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
        mark(out, start..base + at + 2, fade(MARKER_FADE));
        mark(out, base + close..end, fade(MARKER_FADE));
        i = close + 2;
    }
}

fn walk(node: &mdast::Node, text: &str, out: &mut Vec<TextDecoration>, theme: &component::Theme) {
    match node {
        mdast::Node::Strong(_) => mark_spans(
            out,
            content_spans(node),
            HighlightStyle {
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            },
        ),
        mdast::Node::Emphasis(_) => mark_spans(
            out,
            content_spans(node),
            HighlightStyle {
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
        ),
        mdast::Node::Delete(_) => mark_spans(
            out,
            content_spans(node),
            HighlightStyle {
                strikethrough: Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                }),
                ..Default::default()
            },
        ),
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
                mark(out, range.start..range.start + head, fade(MARKER_FADE));
                mark(out, range.end - tail..range.end, fade(MARKER_FADE));
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
                    mark_spans(out, affixes(node), fade(MARKER_FADE));
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
            mark_spans(out, affixes(node), fade(MARKER_FADE));
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
                            mark(out, affix.start..affix.start + b, fade(MARKER_FADE));
                            mark(out, affix.start + b + 3..affix.end, fade(MARKER_FADE));
                            continue;
                        }
                        _ => {}
                    }
                }
                mark(out, affix, fade(MARKER_FADE));
            }
        }
        mdast::Node::Blockquote(_) => {
            mark_spans(out, affixes(node), fade(MARKER_FADE));
        }
        mdast::Node::ThematicBreak(_) | mdast::Node::Break(_) => {
            if let Some(range) = node_range(node) {
                mark(out, range, fade(MARKER_FADE));
            }
        }
        _ => {}
    }
    if let Some(children) = node.children() {
        for child in children {
            walk(child, text, out, theme);
        }
    }
}

/// `- [ ]`/`- [x]` line pass — mdast marks the item but not the
/// bracket's position. `[x]` accents the marker and fades the rest
/// of the line, mirroring the preview's strike-through. The marker
/// also lands inside the ListItem's marker-affix fade — an explicit
/// `fade_out: 0` overrides it (first style on a property wins).
fn task_markers(text: &str, out: &mut Vec<TextDecoration>, accent: HighlightStyle) {
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
                        at + b..at + b + 3,
                        HighlightStyle {
                            fade_out: Some(0.),
                            ..accent
                        },
                    ),
                    Some(b"x]") | Some(b"X]") => {
                        mark(
                            out,
                            at + b..at + b + 3,
                            HighlightStyle {
                                fade_out: Some(0.),
                                ..accent
                            },
                        );
                        mark(out, at + b + 3..at + seg.len(), fade(DONE_FADE));
                    }
                    _ => {}
                }
            }
        }
        at += line.len();
    }
}

/// All decorations for `text`, styled against the active theme.
pub fn markdown_decorations(text: &str, theme: &component::Theme) -> Vec<TextDecoration> {
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
    // Task markers first: the line pass owns `[ ]`/`[x]` styling, so the
    // ListItem affix fade in `walk` carves those brackets out — combining
    // same-property decorations resolves nondeterministically, so passes
    // must never emit overlapping fades/colors.
    task_markers(text, &mut out, accent);
    // `[[wikilink]]` spans live inside mdast Text nodes — one textual
    // pass over the raw buffer covers them wherever they appear.
    wikilinks(text, 0, &mut out, accent);
    if let Ok(root) = markdown::to_mdast(text, &options) {
        walk(&root, text, &mut out, theme);
    }
    out
}
