//! Preview: Obsidian-flavored preprocessing before TextView's markdown parse,
//! plus a callout block plugin.
//!
//! Transforms:
//! - `[[Note]]` / `[[Note|alias]]` → `[label](wiki:Note)` — clicks handled by the
//!   workspace and resolved against the vault index.
//! - `![[img.png]]` → image embed resolved to a `file://` URI via the vault's
//!   image index; `![[img|300]]` / `![[img|300x200]]` carry an Obsidian size
//!   suffix through a `rista:` title marker that [`SizedImagePlugin`] renders
//!   at the requested dimensions. Unresolved embeds become wikilinks instead.
//! - `^block-id` markers → stripped (they are anchors, not content).
//! - `> [!type]` Obsidian callouts → parsed by [`CalloutPlugin`] at render time.
//! - `banner:`/`cover:`/`banner_y`/`banner_icon` frontmatter → [`BannerSpec`],
//!   rendered by the workspace above the preview.

use gpui_kit::base::StyledExt;
use gpui_kit::component::text::FrontmatterPlugin;
use gpui_kit::component::text::{
    InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
    MarkdownPlugin,
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
        .plugin(LocalImagePlugin)
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
    let mut in_frontmatter = false;

    for (index, line) in source.split_inclusive('\n').enumerate() {
        let trimmed = line.trim_start();
        // YAML frontmatter is untouched — wikilinks in `banner:` etc. are data.
        if index == 0 && trimmed.trim_end() == "---" {
            in_frontmatter = true;
        }
        if in_frontmatter {
            out.push_str(line);
            if index > 0 && (trimmed.trim_end() == "---" || trimmed.trim_end() == "...") {
                in_frontmatter = false;
            }
            continue;
        }
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
    let inner = inner.trim();
    let (target, suffix) = match inner.split_once('|') {
        Some((t, s)) => (t.trim(), s.trim()),
        None => (inner, ""),
    };
    let looks_like_note = target
        .rsplit('.')
        .next()
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(!target.contains('.'));
    if looks_like_note {
        // Transclusion is out of scope — surface embeds of notes as links.
        let label = if suffix.is_empty() { target } else { suffix };
        return format!("[{}](wiki:{})", label, target);
    }
    // `![[img|300]]` width, `![[img|300x200]]` w×h; anything else is a caption.
    let size = parse_size_suffix(suffix);
    let alt = if suffix.is_empty() || size.is_some() {
        file_stem(target)
    } else {
        suffix.to_string()
    };
    let resolved = image_resolver(target).or_else(|| {
        // Try a path relative to the document before giving up.
        let local = doc_dir.join(target);
        local.exists().then_some(local)
    });
    match resolved {
        Some(path) => {
            // The size rides on the title so SizedImagePlugin can pick it up
            // after the markdown parse.
            let marker = match size {
                Some((w, Some(h))) => format!(" \"rista:{w}x{h}\""),
                Some((w, None)) => format!(" \"rista:{w}\""),
                None => String::new(),
            };
            format!("![{}]({}{})", alt, file_url(&path), marker)
        }
        // Unresolved: render as a wikilink pill, like Obsidian's "missing" chip.
        None => format!("[{}](wiki:{})", alt, target),
    }
}

fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// `300` → width only, `300x200` → width × height.
fn parse_size_suffix(s: &str) -> Option<(f32, Option<f32>)> {
    if s.is_empty() {
        return None;
    }
    let (w, h) = match s.split_once('x') {
        Some((w, h)) => (w.trim(), Some(h.trim())),
        None => (s.trim(), None),
    };
    let w: f32 = w.parse().ok()?;
    let h = match h {
        Some(h) => Some(h.parse::<f32>().ok()?),
        None => None,
    };
    (w > 0. && h.is_none_or(|h| h > 0.)).then_some((w, h))
}

/// Percent-encode just enough of a path for a well-formed `file://` URI.
fn file_url(path: &Path) -> String {
    let mut out = String::from("file://");
    for byte in path.to_string_lossy().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------------
// Local images: gpui's default image node only loads http(s) URIs, so every
// `file://` image is rendered here via `Resource::Path`. A `rista:` title
// marker (from `![[img|300]]` / `|300x200` embeds) sets explicit dimensions.
// ------------------------------------------------------------------

struct LocalImage {
    url: String,
    /// `(width, height)` from the `rista:` marker, when present.
    size: Option<(f32, Option<f32>)>,
}

pub struct LocalImagePlugin;

impl MarkdownPlugin for LocalImagePlugin {
    fn name(&self) -> &str {
        "local-image"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Image(image) = node else {
            return None;
        };
        let size = image
            .title
            .as_deref()
            .and_then(|t| t.strip_prefix("rista:"))
            .and_then(parse_size_suffix);
        if size.is_none() && !image.url.starts_with("file://") {
            return None;
        }
        Some(MarkdownNode::new(
            "local-image",
            LocalImage {
                url: image.url.clone(),
                size,
            },
        ))
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        _context: &InlineRenderContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let image = node.data::<LocalImage>()?;
        let mut element = img(image_source(&image.url));
        match image.size {
            Some((width, Some(height))) => {
                element = element
                    .w(px(width))
                    .h(px(height))
                    .object_fit(ObjectFit::Cover);
            }
            Some((width, None)) => element = element.w(px(width)),
            // Unsized: natural dimensions, capped at the column width.
            None => element = element.max_w_full(),
        }
        Some(InlineElement::new(
            div()
                .rounded(cx.theme().radius)
                .overflow_hidden()
                .child(element),
        ))
    }
}

fn image_source(url: &str) -> ImageSource {
    if let Some(path) = url.strip_prefix("file://") {
        return PathBuf::from(percent_decode(path)).into();
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string().into();
    }
    PathBuf::from(url).into()
}

// ------------------------------------------------------------------
// Banner: `banner:`/`cover:`/`banner_y:`/`banner_icon:` frontmatter keys,
// the convention shared by Obsidian's banner/cover plugins.
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BannerSpec {
    pub source: BannerSource,
    /// Vertical focal point, 0.0 top – 1.0 bottom. `banner_y` accepts both the
    /// 0–1 and 0–100 conventions.
    pub y: Option<f32>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BannerSource {
    File(PathBuf),
    Remote(String),
}

pub fn banner_spec(
    source: &str,
    doc_dir: &Path,
    image_resolver: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<BannerSpec> {
    let props = frontmatter_pairs(source);
    let value = props
        .iter()
        .find(|(key, _)| key == "banner" || key == "cover")
        .map(|(_, value)| value)?;
    let source = resolve_banner_value(value, doc_dir, image_resolver)?;
    let y = props
        .iter()
        .find(|(key, _)| key == "banner_y")
        .and_then(|(_, value)| value.parse::<f32>().ok())
        .map(|v| if v > 1.0 { v / 100.0 } else { v }.clamp(0.0, 1.0));
    let icon = props
        .iter()
        .find(|(key, _)| key == "banner_icon")
        .map(|(_, value)| value.clone());
    Some(BannerSpec { source, y, icon })
}

/// Top-level `key: value` scalars from the YAML frontmatter block.
fn frontmatter_pairs(source: &str) -> Vec<(String, String)> {
    let mut lines = source.lines();
    if lines.next().map(|line| line.trim()) != Some("---") {
        return Vec::new();
    }
    let mut pairs = Vec::new();
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            break;
        }
        // Nested YAML (lists, maps) is skipped — banners are scalars.
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        pairs.push((key.to_string(), value.to_string()));
    }
    pairs
}

fn resolve_banner_value(
    value: &str,
    doc_dir: &Path,
    image_resolver: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<BannerSource> {
    let value = value.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        return Some(BannerSource::Remote(value.to_string()));
    }
    // Accept `![[img.png]]`, `[[img.png]]`, or a bare relative path.
    let name = value
        .strip_prefix("![[")
        .and_then(|v| v.strip_suffix("]]"))
        .or_else(|| value.strip_prefix("[[").and_then(|v| v.strip_suffix("]]")))
        .unwrap_or(value)
        .split('|')
        .next()
        .unwrap_or(value)
        .trim();
    if name.is_empty() {
        return None;
    }
    image_resolver(name)
        .or_else(|| {
            let local = doc_dir.join(name);
            local.exists().then_some(local)
        })
        .map(BannerSource::File)
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
