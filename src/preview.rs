//! Preview: Obsidian-flavored preprocessing before TextView's markdown parse,
//! plus a callout block plugin.
//!
//! Transforms:
//! - `[[Note]]` / `[[Note|alias]]` → `[label](wiki:Note)` — clicks handled by the
//!   workspace and resolved against the vault index.
//! - `![[img.png]]` → image embed resolved to a `file://` URI via the vault's
//!   image index; unresolved embeds become wikilinks instead.
//! - `^block-id` markers → stripped (they are anchors, not content).
//! - `> [!type]` Obsidian callouts → parsed by [`CalloutPlugin`] at render time.

use gpui_kit::base::StyledExt;
use gpui_kit::component::text::FrontmatterPlugin;
use gpui_kit::component::text::{
    MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use markdown::mdast;
use std::path::{Path, PathBuf};

/// Markdown extensions Rísta renders with.
pub fn extensions() -> MarkdownExtensions {
    MarkdownExtensions::default()
        .frontmatter()
        .plugin(FrontmatterPlugin::new())
        .plugin(CalloutPlugin)
}

/// Rewrite Obsidian syntax into CommonMark for the preview pipeline.
pub fn preprocess(
    source: &str,
    doc_dir: &Path,
    image_resolver: &dyn Fn(&str) -> Option<PathBuf>,
) -> String {
    let mut out = String::with_capacity(source.len() + 128);
    let mut in_fence = false;
    let mut fence_marker = "";

    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        // Fenced code blocks are untouched.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let marker = &trimmed[..3];
            if !in_fence {
                in_fence = true;
                fence_marker = marker;
            } else if marker == fence_marker {
                in_fence = false;
            }
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }

        let line = rewrite_line(line, doc_dir, image_resolver);
        out.push_str(&line);
    }
    out
}

fn rewrite_line(
    line: &str,
    doc_dir: &Path,
    image_resolver: &dyn Fn(&str) -> Option<PathBuf>,
) -> String {
    let mut out = String::with_capacity(line.len() + 32);
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_code = false;

    while i < bytes.len() {
        let ch = line[i..].chars().next().unwrap();
        if ch == '`' {
            let run = line[i..].chars().take_while(|&c| c == '`').count();
            out.push_str(&line[i..i + run]);
            i += run;
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }

        if line[i..].starts_with("![[") {
            if let Some(end) = line[i..].find("]]") {
                let inner = &line[i + 3..i + end];
                out.push_str(&render_embed(inner, doc_dir, image_resolver));
                i += end + 2;
                continue;
            }
        } else if line[i..].starts_with("[[") {
            if let Some(end) = line[i..].find("]]") {
                let inner = line[i + 2..i + end].trim();
                let (target, label) = match inner.split_once('|') {
                    Some((t, l)) => (t.trim(), l.trim()),
                    None => (inner, inner),
                };
                let label = label.split('#').next().unwrap_or(label);
                let target = target.split('#').next().unwrap_or(target);
                out.push_str(&format!("[{}](wiki:{})", label, target));
                i += end + 2;
                continue;
            }
        } else if ch == '^' {
            // `^block-id` — word chars at line end (or followed by whitespace).
            let rest = &line[i + 1..];
            let id_len: usize = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .map(char::len_utf8)
                .sum();
            let after = &rest[id_len..];
            let at_line_end = after.trim_end().is_empty() || after.starts_with(char::is_whitespace);
            let preceded =
                i == 0 || bytes[i - 1].is_ascii_whitespace() || bytes[i - 1].is_ascii_punctuation();
            if id_len > 0 && at_line_end && preceded {
                // Drop the marker; also eat a trailing space before it.
                if out.ends_with(' ') {
                    out.pop();
                }
                i += 1 + id_len;
                continue;
            }
        }
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn render_embed(
    inner: &str,
    doc_dir: &Path,
    image_resolver: &dyn Fn(&str) -> Option<PathBuf>,
) -> String {
    let (target, alt) = match inner.split_once('|') {
        Some((t, a)) => (t.trim(), a.trim()),
        None => (inner.trim(), inner.trim()),
    };
    let looks_like_note = target
        .rsplit('.')
        .next()
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(!target.contains('.'));
    if looks_like_note {
        // Transclusion is out of scope — surface embeds of notes as links.
        return format!("[{}](wiki:{})", alt, target);
    }
    if let Some(path) = image_resolver(target) {
        return format!("![{}](file://{})", alt, path.display());
    }
    // Try a path relative to the document before giving up.
    let local = doc_dir.join(target);
    if local.exists() {
        return format!("![{}](file://{})", alt, local.display());
    }
    format!("![{}](wiki:{})", alt, target)
}

// ------------------------------------------------------------------
// Callouts: `> [!note]` / `> [!warning]-` Obsidian-style admonitions.
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Callout {
    kind: String,
    title: String,
    /// Inner markdown, `>` markers already stripped.
    body: String,
}

/// Parses `> [!type]` blockquotes into a custom node; other blockquotes pass
/// through to the default renderer.
pub struct CalloutPlugin;

impl MarkdownPlugin for CalloutPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "callout"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        if !matches!(node, mdast::Node::Blockquote(_)) {
            return None;
        }
        let source = cx.node_source(node)?;
        let callout = parse_callout(source)?;
        let text = if callout.title.is_empty() {
            callout.kind.clone()
        } else {
            format!("{} — {}", callout.kind, callout.title)
        };
        Some(
            MarkdownNode::new("callout", callout)
                .text(text)
                .markdown(source.to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let callout = node.data::<Callout>().expect("callout node data");
        let theme = cx.theme();
        let accent = callout_accent(&callout.kind, cx);

        v_flex()
            .w_full()
            .rounded(theme.radius)
            .bg(theme.accent)
            .overflow_hidden()
            .child(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(callout_icon(&callout.kind))
                            .size_4()
                            .text_color(accent),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(accent)
                            .child(callout.title.clone()),
                    ),
            )
            .when(!callout.body.trim().is_empty(), |this| {
                this.child(
                    div()
                        .w_full()
                        .px_3()
                        .pb_2()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(
                            gpui_kit::component::text::TextView::markdown(
                                "callout-body",
                                callout.body.clone(),
                            )
                            .markdown_extensions(extensions()),
                        ),
                )
            })
    }
}

/// Strip `>` quote markers and pull `[!type]`, optional `+`/`-` fold sign and
/// title off the first line.
fn parse_callout(source: &str) -> Option<Callout> {
    let mut inner = String::new();
    for line in source.lines() {
        let stripped = line
            .trim_start()
            .strip_prefix('>')
            .map(|s| s.strip_prefix(' ').unwrap_or(s));
        {
            let s = stripped?;
            inner.push_str(s);
            inner.push('\n');
        }
    }
    let mut lines = inner.lines();
    let first = lines.next()?.trim();
    let rest = first.strip_prefix("[!")?;
    let end = rest.find(']')?;
    let mut kind = rest[..end].to_lowercase();
    let mut tail = rest[end + 1..].trim_start();
    // Fold markers `+`/`-` are honored syntactically but callouts render open.
    if let Some(stripped) = tail.strip_prefix('+').or_else(|| tail.strip_prefix('-')) {
        tail = stripped.trim_start();
    }
    // An optional `-` on the kind token itself (`[!note]-`).
    if let Some(stripped) = kind.strip_suffix('-').or_else(|| kind.strip_suffix('+')) {
        kind = stripped.to_string();
    }
    if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let title = if tail.is_empty() {
        titleize(&kind)
    } else {
        tail.to_string()
    };
    let body = lines.collect::<Vec<_>>().join("\n");
    Some(Callout { kind, title, body })
}

fn titleize(kind: &str) -> String {
    let mut chars = kind.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn callout_icon(kind: &str) -> gpui_kit::component::IconName {
    use gpui_kit::component::IconName::*;
    match kind {
        "note" => Info,
        "abstract" | "summary" | "tldr" => BookOpen,
        "info" => Info,
        "todo" => Check,
        "tip" | "hint" | "important" => Star,
        "success" | "check" | "done" => Check,
        "question" | "help" | "faq" => Info,
        "warning" | "caution" | "attention" => TriangleAlert,
        "failure" | "fail" | "missing" => TriangleAlert,
        "danger" | "error" | "bug" => TriangleAlert,
        "example" => BookOpen,
        "quote" | "cite" => BookOpen,
        _ => Info,
    }
}

fn callout_accent(kind: &str, cx: &App) -> gpui_kit::Hsla {
    let colors = &cx.theme().colors;
    match kind {
        "warning" | "caution" | "attention" => colors.warning,
        "failure" | "fail" | "missing" | "danger" | "error" | "bug" => colors.danger,
        "success" | "check" | "done" => colors.success,
        "tip" | "hint" | "important" => colors.success,
        _ => colors.info,
    }
}
