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
use gpui_kit::component::text::{
    InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext,
    MarkdownPlugin,
};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use markdown::mdast;
use std::path::{Path, PathBuf};

/// Live handles preview plugins need: the vault they query, the workspace
/// their links/rows open notes into, the per-document map of spec-hash →
/// base view entity (views survive re-renders), and the transclusion
/// recursion depth (capped so cyclic `![[a]]`/`![[b]]` embeds terminate).
#[derive(Clone)]
pub struct PreviewCtx {
    pub vault: Entity<crate::vault::Vault>,
    pub workspace: WeakEntity<crate::app::Workspace>,
    pub views: EmbedViews,
    pub depth: usize,
}

/// The per-document embed map shared with [`PreviewCtx::views`].
pub type EmbedViews = std::sync::Arc<
    std::sync::Mutex<std::collections::HashMap<u64, Entity<crate::bases::BaseView>>>,
>;

/// Markdown extensions Rísta renders with.
pub fn extensions(folds: &CalloutFolds, ctx: Option<&PreviewCtx>) -> MarkdownExtensions {
    let ext = MarkdownExtensions::default()
        .frontmatter()
        .plugin(PropertiesPlugin {
            folds: folds.clone(),
        })
        .plugin(LocalImagePlugin)
        .plugin(CalloutPlugin::new(folds.clone(), ctx.cloned()))
        .plugin(LinkCardPlugin);
    match ctx {
        Some(ctx) => ext
            .plugin(BaseEmbedPlugin { ctx: ctx.clone() })
            .plugin(TranscludePlugin {
                ctx: ctx.clone(),
                folds: folds.clone(),
            }),
        None => ext,
    }
    .plugin(MathPlugin)
    .plugin(MathBlockPlugin)
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
    // `.md`, or no extension at all — Obsidian wiki targets are
    // extensionless note names.
    let looks_like_note = target
        .rsplit('.')
        .next()
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or_default()
        || !target.contains('.');
    if looks_like_note {
        // Note transclusion — TranscludePlugin renders the note inline.
        return format!("![](transclude:{target})");
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

/// Fold state for collapsible callouts, keyed by source offset so it
/// survives preview re-syncs. Shared between the document, the plugin
/// instance and the rendered chevron's click handler.
#[derive(Clone, Default)]
pub struct CalloutFolds(std::sync::Arc<std::sync::Mutex<std::collections::HashMap<usize, bool>>>);

impl CalloutFolds {
    fn is_folded(&self, key: usize, default: bool) -> bool {
        self.0
            .lock()
            .ok()
            .and_then(|m| m.get(&key).copied())
            .unwrap_or(default)
    }

    fn toggle(&self, key: usize, default: bool) {
        if let Ok(mut map) = self.0.lock() {
            let current = map.get(&key).copied().unwrap_or(default);
            map.insert(key, !current);
        }
    }
}

/// Frontmatter rendered Obsidian-style: a compact, collapsible
/// "Properties" strip instead of a raw YAML table. Properties are the
/// data behind bases, so they stay visible — just not noisy.
struct PropertiesPlugin {
    folds: CalloutFolds,
}

impl MarkdownPlugin for PropertiesPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "properties"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Yaml(yaml) = node else {
            return None;
        };
        let entries = serde_yaml::from_str::<serde_yaml::Value>(&yaml.value)
            .ok()
            .and_then(|v| v.as_mapping().cloned())
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| Some((k.as_str()?.to_string(), prop_display(v))))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let key = node.position().map(|p| p.start.offset).unwrap_or_default();
        Some(
            MarkdownNode::new("properties", (entries, key))
                .text("properties")
                .markdown(yaml.value.clone()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (entries, key) = node
            .data::<(Vec<(String, String)>, usize)>()
            .expect("properties node data");
        let theme = cx.theme();
        if entries.is_empty() {
            return div().into_any_element();
        }
        let folded = self.folds.is_folded(*key, false);
        let folds = self.folds.clone();
        let key = *key;
        let mut rows = v_flex().w_full();
        if !folded {
            for (k, v) in entries {
                rows = rows.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_1()
                        .gap_2()
                        .border_t_1()
                        .border_color(theme.border.opacity(0.5))
                        .child(
                            div()
                                .w(px(96.))
                                .flex_none()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(k.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(theme.foreground)
                                .truncate()
                                .child(v.clone()),
                        ),
                );
            }
        }
        v_flex()
            .w_full()
            .my_2()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .overflow_hidden()
            .child(
                h_flex()
                    .id(("properties-header", key))
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .items_center()
                    .cursor_pointer()
                    .on_click(move |_, window, _cx| {
                        folds.toggle(key, false);
                        window.refresh();
                    })
                    .child(
                        Icon::new(if folded {
                            gpui_kit::component::IconName::ChevronRight
                        } else {
                            gpui_kit::component::IconName::ChevronDown
                        })
                        .size_4()
                        .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(theme.muted_foreground)
                            .child("Properties"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("{}", entries.len())),
                    ),
            )
            .child(rows)
            .into_any_element()
    }
}

/// Display value for a frontmatter property — scalars as-is, lists
/// joined, `[[wikilinks]]`/`![[embeds]]` unwrapped to their targets.
fn prop_display(v: &serde_yaml::Value) -> String {
    fn unwrap_link(s: &str) -> String {
        let inner = s
            .trim()
            .strip_prefix("![[")
            .or_else(|| s.trim().strip_prefix("[["))
            .and_then(|s| s.strip_suffix("]]"));
        inner
            .map(|t| {
                t.split('|')
                    .next()
                    .unwrap_or("")
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            })
            .unwrap_or_else(|| s.to_string())
    }
    match v {
        serde_yaml::Value::String(s) => unwrap_link(s),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Sequence(items) => items
            .iter()
            .map(prop_display)
            .collect::<Vec<_>>()
            .join(", "),
        serde_yaml::Value::Null => String::new(),
        other => serde_yaml::to_string(other)
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
    }
}

#[derive(Debug, Clone)]
struct Callout {
    kind: String,
    title: String,
    /// Inner markdown, `>` markers already stripped.
    body: String,
    /// Source offset — fold-state key.
    key: usize,
    /// `+`/`-` marker present (`> [!note]-`): the callout is collapsible.
    /// Value is the default fold when the user hasn't toggled it.
    foldable: Option<bool>,
}

/// Parses `> [!type]` blockquotes into a custom node; other blockquotes pass
/// through to the default renderer.
pub struct CalloutPlugin {
    folds: CalloutFolds,
    ctx: Option<PreviewCtx>,
}

impl CalloutPlugin {
    pub fn new(folds: CalloutFolds, ctx: Option<PreviewCtx>) -> Self {
        Self { folds, ctx }
    }
}

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
        let mut callout = parse_callout(source)?;
        callout.key = node
            .position()
            .map(|position| position.start.offset)
            .unwrap_or_default();
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
        let folded = callout
            .foldable
            .is_some_and(|default| self.folds.is_folded(callout.key, default));

        let folds = self.folds.clone();
        let key = callout.key;
        let default_fold = callout.foldable.unwrap_or_default();
        let mut header = h_flex()
            .id(("callout-header", key))
            .w_full()
            .px_3()
            .py_2()
            .gap_2()
            .items_center();
        if callout.foldable.is_some() {
            header = header
                .cursor_pointer()
                .on_click(move |_, window, _cx| {
                    folds.toggle(key, default_fold);
                    window.refresh();
                })
                .child(
                    Icon::new(if folded {
                        gpui_kit::component::IconName::ChevronRight
                    } else {
                        gpui_kit::component::IconName::ChevronDown
                    })
                    .size_4()
                    .text_color(accent),
                );
        }
        let header = header
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
            );

        v_flex()
            .w_full()
            .rounded(theme.radius)
            .bg(theme.accent)
            .overflow_hidden()
            .child(header)
            .when(!folded && !callout.body.trim().is_empty(), |this| {
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
                            .markdown_extensions(extensions(&self.folds, self.ctx.as_ref())),
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
    // Obsidian fold markers: `[!type]-` starts folded, `[!type]+` starts
    // expanded; without a marker the callout is not collapsible.
    let mut foldable = None;
    if let Some(stripped) = tail.strip_prefix('-') {
        foldable = Some(true);
        tail = stripped.trim_start();
    } else if let Some(stripped) = tail.strip_prefix('+') {
        foldable = Some(false);
        tail = stripped.trim_start();
    }
    // Marker on the kind token itself (`[!note-]`).
    if let Some(stripped) = kind.strip_suffix('-') {
        foldable = Some(true);
        kind = stripped.to_string();
    } else if let Some(stripped) = kind.strip_suffix('+') {
        foldable = Some(false);
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
    Some(Callout {
        kind,
        title,
        body,
        key: 0,
        foldable,
    })
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

// ── Link preview cards ──────────────────────────────────────────────
//
// A paragraph that is exactly one bare `https://…` link renders as a
// bookmark card (Notion-style) once its OpenGraph metadata has loaded.

/// Fetch state for a card, keyed by URL — global since the same link
/// shows the same card in every note.
enum CardFetch {
    Pending,
    Done(Option<LinkCard>),
}

#[derive(Clone, Debug)]
struct LinkCard {
    title: String,
    description: String,
    image: Option<String>,
    site: String,
}

static LINK_CARDS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, CardFetch>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[derive(Debug, Clone)]
struct BareLink {
    url: String,
}

struct LinkCardPlugin;

impl MarkdownPlugin for LinkCardPlugin {
    fn name(&self) -> &str {
        "link-card"
    }

    fn is_block(&self) -> bool {
        true
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Paragraph(paragraph) = node else {
            return None;
        };
        let url = match paragraph.children.as_slice() {
            // A bare URL GFM-autolinked into a Link — visible text must be
            // the URL itself so `[title](url)` stays an inline link.
            [mdast::Node::Link(link)] => match link.children.as_slice() {
                [mdast::Node::Text(text)] if text.value.trim() == link.url.trim() => {
                    link.url.trim()
                }
                _ => return None,
            },
            // Autolink off: the paragraph is one plain-text URL.
            [mdast::Node::Text(text)] => {
                let value = text.value.trim();
                if value.chars().any(char::is_whitespace) {
                    return None;
                }
                value
            }
            _ => return None,
        };
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return None;
        }
        Some(
            MarkdownNode::new(
                "link-card",
                BareLink {
                    url: url.to_string(),
                },
            )
            .text(url.to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let link = node.data::<BareLink>().expect("link card data");
        let url = link.url.clone();

        enum Show {
            Loading,
            Card(Box<LinkCard>),
            Fallback,
        }
        let show = match LINK_CARDS.lock().ok().as_deref() {
            Some(map) => match map.get(&url) {
                Some(CardFetch::Pending) => Show::Loading,
                Some(CardFetch::Done(Some(card))) => Show::Card(Box::new(card.clone())),
                Some(CardFetch::Done(None)) => Show::Fallback,
                None => Show::Loading,
            },
            None => Show::Fallback,
        };
        if matches!(show, Show::Loading) {
            let unseen = LINK_CARDS
                .lock()
                .ok()
                .is_some_and(|m| !m.contains_key(&url));
            if unseen {
                if let Ok(mut map) = LINK_CARDS.lock() {
                    map.insert(url.clone(), CardFetch::Pending);
                }
                queue_card_fetch(&url, cx);
            }
        }
        let theme = cx.theme();

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&url, &mut hasher);
        let key = std::hash::Hasher::finish(&hasher) as usize;
        let open = url.clone();

        match show {
            Show::Loading | Show::Fallback => h_flex()
                .id(("linkcard-pending", key))
                .w_full()
                .px_3()
                .py_2()
                .gap_2()
                .items_center()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.border)
                .cursor_pointer()
                .on_click(move |_, _window, cx| cx.open_url(&open))
                .child(
                    Icon::new(gpui_kit::component::IconName::Globe)
                        .size_4()
                        .text_color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(url),
                )
                .into_any_element(),
            Show::Card(card) => {
                let title = if card.title.is_empty() {
                    url.clone()
                } else {
                    card.title.clone()
                };
                let mut card_body = h_flex()
                    .id(("linkcard", key))
                    .w_full()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .cursor_pointer()
                    .on_click(move |_, _window, cx| cx.open_url(&open))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .px_3()
                            .py_2()
                            .gap_1()
                            .child(div().text_sm().font_semibold().child(title))
                            .when(!card.description.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(card.description.clone()),
                                )
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(card.site.clone()),
                            ),
                    );
                if let Some(image) = card.image.clone() {
                    card_body = card_body.child(
                        gpui::img(image)
                            .w(px(112.))
                            .h_full()
                            .object_fit(gpui::ObjectFit::Cover),
                    );
                }
                card_body.into_any_element()
            }
        }
    }
}

fn queue_card_fetch(url: &str, cx: &mut App) {
    let url = url.to_string();
    cx.spawn(async move |cx| {
        let data = cx
            .background_executor()
            .spawn({
                let url = url.clone();
                async move { fetch_link_card(&url) }
            })
            .await;
        if let Ok(mut map) = LINK_CARDS.lock() {
            map.insert(url, CardFetch::Done(data));
        }
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// Fetch `url` and scrape OpenGraph + `<title>` metadata. Blocking —
/// must run on the background executor.
fn fetch_link_card(url: &str) -> Option<LinkCard> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(8)))
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = agent.get(url).call().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.body_mut().read_to_vec().ok()?;
    let html = String::from_utf8_lossy(&bytes[..bytes.len().min(512 * 1024)]);
    let head = &html[..html.find("</head>").unwrap_or(html.len())];

    let mut title = None;
    let mut description = None;
    let mut image = None;
    let mut site = None;
    for tag in head.match_indices("<meta") {
        let rest = &head[tag.0..];
        let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
        let tag = &rest[..end];
        let Some(content) = tag_attr(tag, "content") else {
            continue;
        };
        match tag_attr(tag, "property")
            .or_else(|| tag_attr(tag, "name"))
            .as_deref()
        {
            Some("og:title") => {
                title.get_or_insert(decode_entities(&content));
            }
            Some("og:description") | Some("description") | Some("twitter:description") => {
                description.get_or_insert(decode_entities(&content));
            }
            Some("og:image") | Some("twitter:image") => {
                image.get_or_insert(absolute_url(url, &content));
            }
            Some("og:site_name") => {
                site.get_or_insert(decode_entities(&content));
            }
            _ => {}
        }
    }
    if title.is_none() {
        if let Some(start) = head.find("<title") {
            if let Some(open_end) = head[start..].find('>').map(|e| start + e + 1) {
                if let Some(close) = head[open_end..].find("</title>") {
                    title = Some(decode_entities(head[open_end..open_end + close].trim()));
                }
            }
        }
    }
    let site = site.unwrap_or_else(|| host_of(url));
    let mut description = description.unwrap_or_default();
    if description.len() > 200 {
        description.truncate(197);
        description.push('…');
    }
    Some(LinkCard {
        title: title.unwrap_or_else(|| host_of(url)),
        description,
        image,
        site,
    })
}

fn tag_attr(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(start) = tag.find(&needle) {
            let rest = &tag[start + needle.len()..];
            if let Some(end) = rest.find(quote) {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

fn host_of(url: &str) -> String {
    url.split('/')
        .nth(2)
        .unwrap_or(url)
        .trim_start_matches("www.")
        .to_string()
}

/// Resolve a possibly-relative og:image against the page URL.
fn absolute_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        href.to_string()
    } else if let Some(rest) = href.strip_prefix("//") {
        format!("https:{rest}")
    } else if href.starts_with('/') {
        let host = base.split('/').take(3).collect::<Vec<_>>().join("/");
        format!("{host}{href}")
    } else {
        let base_dir = base.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(base);
        format!("{base_dir}/{href}")
    }
}

fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}

// ------------------------------------------------------------------
// Inline ```` ```base ```` embeds — a vault database inside a note.
// ------------------------------------------------------------------

struct BaseEmbed {
    spec: String,
}

struct BaseEmbedPlugin {
    ctx: PreviewCtx,
}

impl MarkdownPlugin for BaseEmbedPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "base-embed"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Code(code) = node else {
            return None;
        };
        if code.lang.as_deref() != Some("base") {
            return None;
        }
        Some(MarkdownNode::new(
            "base-embed",
            BaseEmbed {
                spec: code.value.clone(),
            },
        ))
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let embed = node.data::<BaseEmbed>().expect("base-embed node data");
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            embed.spec.hash(&mut hasher);
            hasher.finish()
        };
        let view = {
            let mut views = self.ctx.views.lock().expect("embed views");
            match views.get(&key) {
                Some(view) => view.clone(),
                None => {
                    let view = cx.new(|cx| {
                        crate::bases::BaseView::for_inline(
                            embed.spec.clone(),
                            self.ctx.vault.clone(),
                            self.ctx.workspace.clone(),
                            window,
                            cx,
                        )
                    });
                    views.insert(key, view.clone());
                    view
                }
            }
        };
        div()
            .w_full()
            .h(px(320.))
            .my_2()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .overflow_hidden()
            .child(view)
    }
}

// ------------------------------------------------------------------
// `![[note]]` transclusion — note content rendered inline.
// ------------------------------------------------------------------

/// Deeper nesting than this renders as a link — cyclic embeds terminate.
const TRANSCLUDE_MAX_DEPTH: usize = 2;

struct Transclude {
    target: String,
}

struct TranscludePlugin {
    ctx: PreviewCtx,
    folds: CalloutFolds,
}

impl MarkdownPlugin for TranscludePlugin {
    // Inline: `![[note]]` parses to an image node inside a paragraph.
    fn name(&self) -> &str {
        "transclude"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Image(image) = node else {
            return None;
        };
        let target = image.url.strip_prefix("transclude:")?;
        Some(MarkdownNode::new(
            "transclude",
            Transclude {
                target: target.to_string(),
            },
        ))
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let embed = node.data::<Transclude>().expect("transclude node data");
        let theme = cx.theme();
        let resolved = self.ctx.vault.read(cx).resolve_wikilink(&embed.target);
        let content = resolved
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| match crate::properties::frontmatter_span(&text) {
                Some(span) => text[span.end..].to_string(),
                None => text,
            });

        match (self.ctx.depth < TRANSCLUDE_MAX_DEPTH, content) {
            (true, Some(content)) => {
                let mut nested = self.ctx.clone();
                nested.depth += 1;
                div()
                    .w_full()
                    .my_2()
                    .border_l_2()
                    .border_color(theme.border)
                    .pl_3()
                    .child(
                        gpui_kit::component::text::TextView::markdown("transclude", content)
                            .markdown_extensions(extensions(&self.folds, Some(&nested))),
                    )
                    .into_any_element()
            }
            _ => {
                // Unresolved, empty, or too deep — fall back to the link form.
                let workspace = self.ctx.workspace.clone();
                let target = embed.target.clone();
                div()
                    .id(SharedString::from(format!("transclude-{target}")))
                    .text_sm()
                    .text_color(theme.accent)
                    .cursor_pointer()
                    .child(embed.target.clone())
                    .on_click(move |_, window, cx| {
                        let target = target.clone();
                        let _ =
                            workspace.update(cx, |ws, cx| ws.open_wikilink(&target, window, cx));
                    })
                    .into_any_element()
            }
        }
    }
}

// ------------------------------------------------------------------
// Math — `$$...$$` blocks and `$...$` inline, typeset as Unicode.
// Not a full TeX engine: covers the everyday set (greek, scripts,
// fractions, roots, operators, common symbols) and falls back to the
// raw source for anything unrecognized.
// ------------------------------------------------------------------

struct Math {
    source: String,
}

struct MathPlugin;

impl MarkdownPlugin for MathPlugin {
    fn name(&self) -> &str {
        "math"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        match node {
            mdast::Node::InlineMath(math) => Some(
                MarkdownNode::new(
                    "math",
                    Math {
                        source: math.value.clone(),
                    },
                )
                .text(tex_to_unicode(&math.value)),
            ),
            _ => None,
        }
    }
}

/// Block math — `$$...$$` rendered as a centered display line.
struct MathBlockPlugin;

impl MarkdownPlugin for MathBlockPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "math-block"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Math(math) = node else {
            return None;
        };
        Some(MarkdownNode::new(
            "math-block",
            Math {
                source: math.value.clone(),
            },
        ))
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let math = node.data::<Math>().expect("math node data");
        let theme = cx.theme();
        div()
            .w_full()
            .py_2()
            .px_4()
            .my_1()
            .flex()
            .justify_center()
            .child(
                div()
                    .text_base()
                    .italic()
                    .text_color(theme.foreground)
                    .child(tex_to_unicode(&math.source)),
            )
    }
}

/// Render a TeX-ish expression to Unicode. Anything we don't recognize is
/// passed through raw — the result degrades gracefully rather than hiding.
fn tex_to_unicode(src: &str) -> String {
    const MACROS: &[(&str, &str)] = &[
        ("alpha", "α"),
        ("beta", "β"),
        ("gamma", "γ"),
        ("delta", "δ"),
        ("epsilon", "ε"),
        ("zeta", "ζ"),
        ("eta", "η"),
        ("theta", "θ"),
        ("iota", "ι"),
        ("kappa", "κ"),
        ("lambda", "λ"),
        ("mu", "μ"),
        ("nu", "ν"),
        ("xi", "ξ"),
        ("pi", "π"),
        ("rho", "ρ"),
        ("sigma", "σ"),
        ("tau", "τ"),
        ("phi", "φ"),
        ("chi", "χ"),
        ("psi", "ψ"),
        ("omega", "ω"),
        ("Gamma", "Γ"),
        ("Delta", "Δ"),
        ("Theta", "Θ"),
        ("Lambda", "Λ"),
        ("Xi", "Ξ"),
        ("Pi", "Π"),
        ("Sigma", "Σ"),
        ("Phi", "Φ"),
        ("Psi", "Ψ"),
        ("Omega", "Ω"),
        ("infty", "∞"),
        ("partial", "∂"),
        ("nabla", "∇"),
        ("pm", "±"),
        ("mp", "∓"),
        ("times", "×"),
        ("div", "÷"),
        ("cdot", "·"),
        ("ast", "∗"),
        ("circ", "∘"),
        ("bullet", "•"),
        ("le", "≤"),
        ("leq", "≤"),
        ("ge", "≥"),
        ("geq", "≥"),
        ("ne", "≠"),
        ("neq", "≠"),
        ("approx", "≈"),
        ("equiv", "≡"),
        ("sim", "∼"),
        ("simeq", "≃"),
        ("propto", "∝"),
        ("ll", "≪"),
        ("gg", "≫"),
        ("in", "∈"),
        ("notin", "∉"),
        ("ni", "∋"),
        ("subset", "⊂"),
        ("supset", "⊃"),
        ("subseteq", "⊆"),
        ("supseteq", "⊇"),
        ("cup", "∪"),
        ("cap", "∩"),
        ("setminus", "∖"),
        ("emptyset", "∅"),
        ("forall", "∀"),
        ("exists", "∃"),
        ("nexists", "∄"),
        ("land", "∧"),
        ("wedge", "∧"),
        ("lor", "∨"),
        ("vee", "∨"),
        ("lnot", "¬"),
        ("neg", "¬"),
        ("oplus", "⊕"),
        ("otimes", "⊗"),
        ("perp", "⊥"),
        ("parallel", "∥"),
        ("angle", "∠"),
        ("to", "→"),
        ("rightarrow", "→"),
        ("gets", "←"),
        ("leftarrow", "←"),
        ("Rightarrow", "⇒"),
        ("Leftarrow", "⇐"),
        ("Leftrightarrow", "⇔"),
        ("mapsto", "↦"),
        ("implies", "⟹"),
        ("iff", "⟺"),
        ("uparrow", "↑"),
        ("downarrow", "↓"),
        ("sum", "∑"),
        ("prod", "∏"),
        ("int", "∫"),
        ("iint", "∬"),
        ("oint", "∮"),
        ("lim", "lim"),
        ("log", "log"),
        ("ln", "ln"),
        ("exp", "exp"),
        ("sin", "sin"),
        ("cos", "cos"),
        ("tan", "tan"),
        ("min", "min"),
        ("max", "max"),
        ("sup", "sup"),
        ("inf", "inf"),
        ("det", "det"),
        ("arg", "arg"),
        ("deg", "deg"),
        ("dim", "dim"),
        ("ker", "ker"),
        ("gcd", "gcd"),
        ("Pr", "Pr"),
        ("ell", "ℓ"),
        ("hbar", "ℏ"),
        ("hslash", "ℏ"),
        ("Re", "ℜ"),
        ("Im", "ℑ"),
        ("aleph", "ℵ"),
        ("wp", "℘"),
        ("prime", "′"),
        ("degree", "°"),
        ("angle", "∠"),
        ("ldots", "…"),
        ("cdots", "⋯"),
        ("dots", "…"),
        ("vdots", "⋮"),
        ("ddots", "⋱"),
        ("quad", "  "),
        ("qquad", "    "),
        (",", " "),
        (";", " "),
        ("!", ""),
        (" ", " "),
        ("left", ""),
        ("right", ""),
        ("big", ""),
        ("Big", ""),
        ("displaystyle", ""),
        ("limits", ""),
        ("mathop", ""),
    ];
    const SUPERS: &[(&str, &str)] = &[
        ("0", "⁰"),
        ("1", "¹"),
        ("2", "²"),
        ("3", "³"),
        ("4", "⁴"),
        ("5", "⁵"),
        ("6", "⁶"),
        ("7", "⁷"),
        ("8", "⁸"),
        ("9", "⁹"),
        ("+", "⁺"),
        ("-", "⁻"),
        ("=", "⁼"),
        ("(", "⁽"),
        (")", "⁾"),
        ("n", "ⁿ"),
        ("i", "ⁱ"),
        ("x", "ˣ"),
        ("T", "ᵀ"),
    ];
    const SUBS: &[(&str, &str)] = &[
        ("0", "₀"),
        ("1", "₁"),
        ("2", "₂"),
        ("3", "₃"),
        ("4", "₄"),
        ("5", "₅"),
        ("6", "₆"),
        ("7", "₇"),
        ("8", "₈"),
        ("9", "₉"),
        ("+", "₊"),
        ("-", "₋"),
        ("=", "₌"),
        ("(", "₍"),
        (")", "₎"),
        ("a", "ₐ"),
        ("e", "ₑ"),
        ("o", "ₒ"),
        ("x", "ₓ"),
        ("i", "ᵢ"),
        ("r", "ᵣ"),
        ("u", "ᵤ"),
        ("v", "ᵥ"),
        ("n", "ₙ"),
        ("k", "ₖ"),
        ("j", "ⱼ"),
        ("t", "ₜ"),
        ("p", "ₚ"),
        ("s", "ₛ"),
    ];

    fn group(src: &str, i: &mut usize) -> String {
        // `{...}` or the next single token.
        let rest = &src[*i..];
        if rest.starts_with('{') {
            *i += 1;
            let mut depth = 1;
            let start = *i;
            while *i < src.len() {
                match src[*i..].chars().next() {
                    Some('{') => depth += 1,
                    Some('}') => {
                        depth -= 1;
                        if depth == 0 {
                            let inner = &src[start..*i];
                            *i += 1;
                            return inner.to_string();
                        }
                    }
                    _ => {}
                }
                *i += src[*i..].chars().next().map(char::len_utf8).unwrap_or(1);
            }
            return src[start..].to_string();
        }
        let ch_len = rest.chars().next().map(char::len_utf8).unwrap_or(0);
        *i += ch_len;
        rest[..ch_len].to_string()
    }

    fn script(inner: &str, table: &[(&str, &str)]) -> String {
        let mut out = String::with_capacity(inner.len());
        for c in inner.chars() {
            let mut buf = [0; 4];
            let key = c.encode_utf8(&mut buf) as &str;
            match table.iter().find(|(k, _)| k == &key) {
                Some((_, v)) => out.push_str(v),
                None => out.push(c),
            }
        }
        out
    }

    fn render(src: &str) -> String {
        let mut out = String::with_capacity(src.len() * 2);
        let mut i = 0;
        while i < src.len() {
            let rest = &src[i..];
            if let Some(rest) = rest.strip_prefix('\\') {
                // Longest macro name wins; \frac/\sqrt take arguments.
                let name_end = rest
                    .find(|c: char| !c.is_ascii_alphabetic() && c != '*')
                    .map(|p| p + 1)
                    .unwrap_or(rest.len() + 1);
                let name = &rest[..(name_end - 1).min(rest.len())];
                let name = if name.is_empty() {
                    &rest[..1.min(rest.len())]
                } else {
                    name
                };
                i += name.len() + 1;
                match name {
                    "frac" | "dfrac" | "tfrac" | "cfrac" => {
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        let num = group(src, &mut i);
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        let den = group(src, &mut i);
                        out.push_str(&render(&num));
                        out.push('⁄');
                        out.push_str(&render(&den));
                    }
                    "sqrt" => {
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        // Optional root index [n] — only simple digits map.
                        if src[i..].starts_with('[') {
                            if let Some(close) = src[i..].find(']') {
                                let idx = &src[i + 1..i + close];
                                out.push_str(&script(idx, SUPERS));
                                i += close + 1;
                            }
                        }
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        let inner = group(src, &mut i);
                        out.push('√');
                        out.push('(');
                        out.push_str(&render(&inner));
                        out.push(')');
                    }
                    "text" | "mathrm" | "mathbf" | "mathit" | "mathsf" | "operatorname"
                    | "boldsymbol" | "emph" => {
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        let inner = group(src, &mut i);
                        out.push_str(&inner);
                    }
                    "mathbb" | "mathcal" | "mathfrak" => {
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        let inner = group(src, &mut i);
                        const DOUBLE: &[(&str, &str)] = &[
                            ("R", "ℝ"),
                            ("N", "ℕ"),
                            ("Z", "ℤ"),
                            ("Q", "ℚ"),
                            ("C", "ℂ"),
                            ("H", "ℍ"),
                            ("P", "ℙ"),
                            ("D", "𝔻"),
                        ];
                        let mapped: String = inner
                            .chars()
                            .map(|c| {
                                DOUBLE
                                    .iter()
                                    .find(|(k, _)| k == &c.to_string().as_str())
                                    .map(|(_, v)| v.to_string())
                                    .unwrap_or_else(|| c.to_string())
                            })
                            .collect();
                        out.push_str(&mapped);
                    }
                    "hat" | "bar" | "vec" | "dot" | "ddot" | "tilde" => {
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        let inner = render(&group(src, &mut i));
                        let mark = match name {
                            "hat" => '̂',
                            "bar" => '̄',
                            "vec" => '⃗',
                            "dot" => '̇',
                            "ddot" => '̈',
                            _ => '̃',
                        };
                        out.push_str(&inner);
                        out.push(mark);
                    }
                    "overline" => {
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        let inner = render(&group(src, &mut i));
                        out.push_str(&inner);
                        out.push('‾');
                    }
                    "underline" => {
                        while src[i..].starts_with(' ') {
                            i += 1;
                        }
                        let inner = render(&group(src, &mut i));
                        out.push_str(&inner);
                        out.push('̲');
                    }
                    _ => {
                        match MACROS.iter().find(|(k, _)| *k == name) {
                            Some((_, v)) => out.push_str(v),
                            None => {
                                // Unknown macro — drop the backslash, keep the name.
                                out.push_str(name);
                            }
                        }
                    }
                }
                continue;
            }
            match rest.chars().next() {
                Some('^') => {
                    i += 1;
                    let inner = group(src, &mut i);
                    out.push_str(&script(&inner, SUPERS));
                }
                Some('_') => {
                    i += 1;
                    let inner = group(src, &mut i);
                    out.push_str(&script(&inner, SUBS));
                }
                Some('{') => {
                    let inner = group(src, &mut i);
                    out.push_str(&render(&inner));
                }
                Some(c) => {
                    i += c.len_utf8();
                    out.push(c);
                }
                None => break,
            }
        }
        out
    }

    render(src.trim())
}
