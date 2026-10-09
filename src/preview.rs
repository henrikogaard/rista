//! Preview: the reference editor-flavored preprocessing before TextView's markdown parse,
//! plus a callout block plugin.
//!
//! Transforms:
//! - `[[Note]]` / `[[Note|alias]]` → `[label](wiki:Note)` — clicks handled by the
//!   workspace and resolved against the vault index.
//! - `![[img.png]]` → image embed resolved to a `file://` URI via the vault's
//!   image index; `![[img|300]]` / `![[img|300x200]]` carry an the reference editor size
//!   suffix through a `rista:` title marker that [`SizedImagePlugin`] renders
//!   at the requested dimensions. Unresolved embeds become wikilinks instead.
//! - `^block-id` markers → stripped (they are anchors, not content).
//! - `%%` comment regions → stripped (hidden markup, spans lines).
//! - `#tag` → `[#tag](tag:tag)` — clickable, opens project search.
//! - `> [!type]` the reference editor callouts → parsed by [`CalloutPlugin`] at render time.
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
    pub properties_visibility: crate::settings::PropertiesVisibility,
    pub language: crate::settings::Language,
    pub vault: Entity<crate::vault::Vault>,
    pub workspace: WeakEntity<crate::app::Workspace>,
    pub views: EmbedViews,
    /// The note being rendered — bound as `this` in ```` ```base ````
    /// embeds. Transclusions re-bind it to the embedded file.
    pub doc_path: Option<PathBuf>,
    pub depth: usize,
}

/// The per-document embed map shared with [`PreviewCtx::views`].
pub type EmbedViews = std::sync::Arc<
    std::sync::Mutex<std::collections::HashMap<u64, Entity<crate::bases::BaseView>>>,
>;

/// Markdown extensions Rísta renders with. `doc_anchored` is true only
/// when the text being parsed is the document itself — nested fragment
/// renderers (callout/task/transclude bodies) pass false so plugins that
/// write back into the source (task checkboxes) stay doc-level only.
pub fn extensions(
    folds: &CalloutFolds,
    ctx: Option<&PreviewCtx>,
    doc_anchored: bool,
) -> MarkdownExtensions {
    let ext = MarkdownExtensions::default()
        .frontmatter()
        .plugin(PropertiesPlugin {
            folds: folds.clone(),
            ctx: ctx.cloned(),
        })
        .plugin(LocalImagePlugin)
        .plugin(NoteIconPlugin)
        .plugin(CalloutPlugin::new(folds.clone(), ctx.cloned()))
        .plugin(LinkCardPlugin)
        .plugin(FootnoteRefPlugin { ctx: ctx.cloned() })
        .plugin(FootnoteDefPlugin {
            folds: folds.clone(),
            ctx: ctx.cloned(),
        });
    let ext = match ctx {
        Some(ctx) => ext
            .plugin(WikiLinkPlugin { ctx: ctx.clone() })
            .plugin(BaseEmbedPlugin { ctx: ctx.clone() })
            .plugin(ColumnBlockPlugin {
                ctx: ctx.clone(),
                folds: folds.clone(),
            })
            .plugin(TranscludePlugin {
                ctx: ctx.clone(),
                folds: folds.clone(),
            })
            .plugin(TaskListPlugin {
                ctx: Some(ctx.clone()),
                folds: folds.clone(),
                doc_anchored,
            }),
        None => ext,
    };
    // Cell-click editing writes back into the source document, so it
    // only intercepts tables at the top level — nested fragments keep
    // the built-in table rendering.
    let ext = match (doc_anchored, ctx) {
        (true, Some(ctx)) => ext.plugin(TablePlugin {
            ctx: ctx.clone(),
            folds: folds.clone(),
        }),
        _ => ext,
    };
    ext.plugin(MathPlugin).plugin(MathBlockPlugin)
}

/// Rewrite the reference editor syntax into CommonMark for the preview pipeline.
pub fn navigation_blocks(source: &str) -> Vec<(usize, String)> {
    fn collect(node: &markdown::mdast::Node, out: &mut Vec<(usize, String)>) {
        use markdown::mdast::Node;
        if matches!(node, Node::Heading(_) | Node::Paragraph(_) | Node::Code(_)) {
            if let Some(position) = node.position() {
                let text = node.to_string();
                if !text.trim().is_empty() {
                    out.push((position.start.offset, text));
                }
            }
        } else {
            for child in node.children().into_iter().flatten() {
                collect(child, out);
            }
        }
    }
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    let mut out = Vec::new();
    if let Ok(node) = markdown::to_mdast(source, &options) {
        collect(&node, &mut out);
    }
    out
}

pub fn match_navigation_blocks(blocks: &[(usize, String)], rendered: &str) -> Vec<(usize, usize)> {
    let mut cursor = 0;
    blocks
        .iter()
        .filter_map(|(source, text)| {
            let start = rendered[cursor..].find(text.as_str())? + cursor;
            cursor = start + text.len();
            Some((*source, start))
        })
        .collect()
}

pub fn headings(source: &str) -> Vec<(usize, usize, String)> {
    fn collect(node: &markdown::mdast::Node, out: &mut Vec<(usize, usize, String)>) {
        if let markdown::mdast::Node::Heading(heading) = node {
            if let Some(position) = &heading.position {
                out.push((
                    position.start.line,
                    heading.depth as usize,
                    node.to_string(),
                ));
            }
        }
        for child in node.children().into_iter().flatten() {
            collect(child, out);
        }
    }
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    let mut out = Vec::new();
    if let Ok(node) = markdown::to_mdast(source, &options) {
        collect(&node, &mut out);
    }
    out
}

/// Obsidian's highlight-color emoji, in menu order: `==🔴text==`.
pub const HIGHLIGHT_COLORS: [(&str, &str); 6] = [
    ("🔴", "Red"),
    ("🟠", "Orange"),
    ("🟡", "Yellow"),
    ("🟢", "Green"),
    ("🔵", "Blue"),
    ("🟣", "Purple"),
];

/// `<mark>` backgrounds for `HIGHLIGHT_COLORS`, as `#rrggbbaa`: theme
/// colors, translucent so the text stays readable. Empty = the plain
/// highlight color.
#[derive(Clone, Default)]
pub struct MarkColors([String; 6]);

impl MarkColors {
    pub fn from_theme(theme: &gpui_kit::component::theme::ThemeColor) -> Self {
        Self(highlight_palette(theme).map(|c| {
            let c = c.to_rgb();
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            format!("#{:02x}{:02x}{:02x}59", byte(c.r), byte(c.g), byte(c.b))
        }))
    }
}

/// The theme colors behind `HIGHLIGHT_COLORS`, opaque. There is no
/// orange token, so orange sits halfway between red and yellow.
pub fn highlight_palette(theme: &gpui_kit::component::theme::ThemeColor) -> [Hsla; 6] {
    let (red, yellow) = (theme.red.to_rgb(), theme.yellow.to_rgb());
    let orange = Rgba {
        r: (red.r + yellow.r) / 2.0,
        g: (red.g + yellow.g) / 2.0,
        b: (red.b + yellow.b) / 2.0,
        a: 1.0,
    };
    [
        theme.red,
        orange.into(),
        theme.yellow,
        theme.green,
        theme.blue,
        theme.magenta,
    ]
}

pub fn preprocess(
    source: &str,
    doc_path: &Path,
    vault_root: Option<&Path>,
    image_resolver: &dyn Fn(&str) -> Option<PathBuf>,
    marks: &MarkColors,
) -> String {
    preprocess_mapped(source, doc_path, vault_root, image_resolver, marks).0
}

pub fn preprocess_mapped(
    source: &str,
    doc_path: &Path,
    vault_root: Option<&Path>,
    image_resolver: &dyn Fn(&str) -> Option<PathBuf>,
    marks: &MarkColors,
) -> (String, Vec<usize>) {
    let doc_dir = doc_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let doc_dir = doc_dir.as_path();
    let mut out = String::with_capacity(source.len() + 128);
    let mut in_fence = false;
    let mut fence_marker = "";
    let mut in_frontmatter = false;
    let mut in_comment = false;
    let mut in_columns = false;
    let mut line_offsets = Vec::new();

    for (index, line) in source.split_inclusive('\n').enumerate() {
        line_offsets.push(out.len());
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

        // `::: columns` fenced divs (the the reference editor Columns plugin's
        // syntax) → a `columns` code fence the ColumnBlockPlugin
        // splits into side-by-side nested markdown. `::: column`
        // lines are column separators; a bare `:::` closes.
        if in_columns {
            if trimmed.trim_end() == ":::" {
                in_columns = false;
                out.push_str("```\n");
            } else if colon_fence(trimmed).is_some_and(|n| n == "column") {
                out.push_str(":::colsep:::\n");
            } else {
                out.push_str(&rewrite_line(
                    line,
                    doc_dir,
                    doc_path,
                    vault_root,
                    image_resolver,
                    marks,
                    &mut in_comment,
                ));
            }
            continue;
        }
        if colon_fence(trimmed).is_some_and(|n| n == "columns") {
            in_columns = true;
            out.push_str("```columns\n");
            continue;
        }

        let line = rewrite_line(
            line,
            doc_dir,
            doc_path,
            vault_root,
            image_resolver,
            marks,
            &mut in_comment,
        );
        out.push_str(&line);
    }
    if in_columns {
        out.push_str("```\n");
    }
    line_offsets.push(out.len());
    (out, line_offsets)
}

/// `::: name` fence marker — accepts `::: columns`, `:::columns` and
/// Pandoc-style `::: {.columns}`. A bare `:::` yields `None`.
#[cfg(test)]
mod navigation_tests {
    use super::{
        headings, match_navigation_blocks, navigation_blocks, preprocess_mapped, slice_section,
    };

    #[test]
    fn block_embeds_resolve_inline_and_standalone_ids() {
        let text = "intro\n\npara one\npara two ^p1\n\n> quote a\n> quote b\n\n^q1\n\nafter";
        assert_eq!(
            slice_section(text, "^p1").as_deref(),
            Some("para one\npara two")
        );
        assert_eq!(
            slice_section(text, "^q1").as_deref(),
            Some("> quote a\n> quote b")
        );
        let tail = "| a |\n|---|\n| 1 |\n\n^t1";
        assert_eq!(
            slice_section(tail, "^t1").as_deref(),
            Some("| a |\n|---|\n| 1 |")
        );
    }
    use std::path::Path;

    #[test]
    fn source_blocks_map_to_rendered_text_not_markdown_offsets() {
        let source = "# Øgård\n\n**Bold** and [a link](note.md).\n\n## Øgård\n";
        let rendered = "Øgård\nBold and a link.\nØgård\n";
        let mapped = match_navigation_blocks(&navigation_blocks(source), rendered);
        assert_eq!(
            mapped,
            vec![
                (0, 0),
                (
                    source.find("**Bold").unwrap(),
                    rendered.find("Bold").unwrap()
                ),
                (
                    source.rfind("##").unwrap(),
                    rendered.rfind("Øgård").unwrap()
                )
            ]
        );
    }

    #[test]
    fn heading_outline_handles_setext_fences_and_frontmatter() {
        assert_eq!(
            headings("---\nlabel: x\n---\nTitle\n=====\n~~~\n# Not a heading\n~~~\n## Real"),
            vec![(4, 1, "Title".into()), (9, 2, "Real".into())]
        );
    }

    #[test]
    fn preview_offsets_survive_expansion_hidden_comments_and_unicode() {
        let (text, offsets) = preprocess_mapped(
            "[[a|A]]\n%% hidden %%\n## Øgård\n",
            Path::new("/vault/note.md"),
            Some(Path::new("/vault")),
            &|_| None,
            &Default::default(),
        );
        assert!(text[offsets[2]..].starts_with("## Øgård"));
        assert_eq!(offsets[3], text.len());
        assert!(offsets.iter().all(|&at| text.is_char_boundary(at)));
    }

    #[test]
    fn colored_highlights_use_the_theme_color_and_hide_the_emoji() {
        let theme = gpui_kit::component::theme::ThemeColor {
            red: gpui_kit::rgb(0xff0000).into(),
            yellow: gpui_kit::rgb(0xffff00).into(),
            ..Default::default()
        };
        let marks = super::MarkColors::from_theme(&theme);
        let render =
            |src: &str| super::preprocess(src, Path::new("/v/n.md"), None, &|_| None, &marks);
        assert_eq!(
            render("a ==🔴hot== b"),
            "a <mark color=\"#ff000059\">hot</mark> b"
        );
        assert_eq!(render("==🟠mid=="), "<mark color=\"#ff800059\">mid</mark>");
        assert_eq!(render("==plain=="), "<mark>plain</mark>");
        let unthemed = super::preprocess(
            "==🟣idea==",
            Path::new("/v/n.md"),
            None,
            &|_| None,
            &Default::default(),
        );
        assert_eq!(unthemed, "<mark>idea</mark>");
    }
}

fn colon_fence(line: &str) -> Option<String> {
    let rest = line.trim_end().strip_prefix(":::")?;
    let name = rest
        .trim()
        .trim_start_matches('{')
        .trim_start_matches('.')
        .trim_end_matches('}')
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn rewrite_line(
    line: &str,
    doc_dir: &Path,
    doc_path: &Path,
    vault_root: Option<&Path>,
    image_resolver: &dyn Fn(&str) -> Option<PathBuf>,
    marks: &MarkColors,
    in_comment: &mut bool,
) -> String {
    let mut out = String::with_capacity(line.len() + 32);
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_code = false;

    while i < bytes.len() {
        let ch = line[i..].chars().next().unwrap();
        // `%%` comment regions — the hidden markup. An open
        // comment swallows everything up to the next `%%`, even across
        // lines; `%%` inside inline code is literal.
        if *in_comment {
            if line[i..].starts_with("%%") {
                *in_comment = false;
                i += 2;
            } else {
                i += ch.len_utf8();
            }
            continue;
        }
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
        if line[i..].starts_with("%%") {
            *in_comment = true;
            i += 2;
            continue;
        }

        if ch == ':' && (i == 0 || bytes[i - 1] != b'\\') {
            if let Some((len, path)) = crate::note_icons::shortcode(&line[i..]) {
                out.push_str(&format!("![{}](rista-icon:{path})", &line[i..i + len]));
                i += len;
                continue;
            }
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
                // `#anchor` stays on the target so the click can jump to
                // the heading; the label shows just the note name — or
                // the anchor itself for same-note `[[#heading]]` links.
                let label = label.split('#').next().unwrap_or(label);
                let label = if label.is_empty() {
                    inner.trim_start_matches('#')
                } else {
                    label
                };
                // `#` anchors can carry spaces — a bare URL with a
                // space isn't a valid link destination, so wrap in <>.
                if target.contains(' ') {
                    out.push_str(&format!("[{}](<wiki:{}>)", label, target));
                } else {
                    out.push_str(&format!("[{}](wiki:{})", label, target));
                }
                i += end + 2;
                continue;
            }
        } else if ch == '#' {
            // `#tag` → `[#tag](tag:tag)` — clickable, opens project
            // search for the tag. Tag chars: word chars, `-`, `/`
            // (nested); the first must be a letter or `_` (`#123` and
            // `## headings` aren't tags). Needs whitespace or start
            // before it.
            let prev_ok = i == 0
                || bytes[i - 1].is_ascii_whitespace()
                || (bytes[i - 1].is_ascii_punctuation() && bytes[i - 1] != b'#');
            let rest = &line[i + 1..];
            let first = rest.chars().next().unwrap_or(' ');
            let tag_len: usize = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '/')
                .map(char::len_utf8)
                .sum();
            if prev_ok && tag_len > 0 && (first.is_alphabetic() || first == '_') {
                let tag = &rest[..tag_len];
                // A tag ending in `/` (`#a/`) isn't a tag.
                if !tag.ends_with('/') {
                    out.push_str(&format!("[#{tag}](tag:{tag})"));
                    i += 1 + tag_len;
                    continue;
                }
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
        } else if ch == '[' {
            // `[label](target)` / `![label](target)` — the reference editor treats
            // document-relative destinations as vault paths. `.md`/
            // `.base`/extensionless targets become `wiki:` note links
            // (create-on-click like `[[…]]`), other existing files
            // become `file://` links, `![](a.md)` transcludes the note,
            // and a bare `(#anchor)` jumps to a heading in this note.
            let image = i > 0 && bytes[i - 1] == b'!';
            if let Some(end) = line[i..].find("](") {
                let label = &line[i + 1..i + end];
                if let Some((url, tail, used)) =
                    rewrite_link_dest(&line[i + end + 2..], image, doc_dir, doc_path, vault_root)
                {
                    out.push_str(&format!("[{label}]({url}{tail})"));
                    i += end + 2 + used;
                    continue;
                }
            }
        } else if line[i..].starts_with("==") {
            // `==highlight==` — the mark syntax; renders through
            // the inline-HTML path as <mark>. The content must be
            // non-empty and not start with `=` (keeps `===` and `====`
            // runs literal). A leading color emoji (`==🔴text==`) picks
            // the theme color and is hidden.
            if let Some(end) = line[i + 2..].find("==") {
                let inner = &line[i + 2..i + 2 + end];
                if !inner.trim().is_empty() && !inner.starts_with('=') {
                    let colored =
                        HIGHLIGHT_COLORS
                            .iter()
                            .enumerate()
                            .find_map(|(ix, (emoji, _))| {
                                let text = inner.strip_prefix(emoji)?;
                                Some((&marks.0[ix], text))
                            });
                    match colored {
                        Some((color, text)) if !color.is_empty() => {
                            out.push_str(&format!("<mark color=\"{color}\">{text}</mark>"))
                        }
                        Some((_, text)) => out.push_str(&format!("<mark>{text}</mark>")),
                        None => out.push_str(&format!("<mark>{inner}</mark>")),
                    }
                    i += 2 + end + 2;
                    continue;
                }
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
    // `.md`/`.base`, or no extension at all — the reference editor wiki targets
    // are extensionless note names. The `#anchor`/`#View` suffix is
    // stripped first so `![[db.base#Board]]` still reads as a note.
    let file_part = target.split('#').next().unwrap_or(target);
    let looks_like_note = file_part
        .rsplit('.')
        .next()
        .map(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("base"))
        .unwrap_or_default()
        || !file_part.contains('.');
    if looks_like_note {
        // Note transclusion — TranscludePlugin renders the note inline.
        // `<>`-wrapped so targets with spaces stay a single URL. A
        // numeric `|height` suffix rides along
        // on the target for `.base` embeds.
        let height = parse_size_suffix(suffix)
            .map(|(w, h)| format!("|{}", h.unwrap_or(w) as u32))
            .unwrap_or_default();
        return format!("![](<transclude:{target}{height}>)");
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
        // Unresolved embeds aren't note links — a distinct scheme keeps a
        // click from creating `img.png.md` in `open_wikilink`.
        None => format!("[{}](missing:{})", alt, target),
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

/// Split the `(...)` destination of a `[label](…)` link/image into
/// (raw target, tail like ` "title"`, bytes consumed through `)`).
/// Handles `<>`-wrapped targets and quoted titles.
fn split_link_dest(s: &str) -> Option<(&str, &str, usize)> {
    let lead = s.len() - s.trim_start().len();
    let s = &s[lead..];
    if let Some(a) = s.strip_prefix('<') {
        let gt = a.find('>')?;
        let after = &a[gt + 1..];
        let close = after.find(')')?;
        let tail = &after[..close];
        let t = tail.trim();
        if t.is_empty() || t.starts_with('"') || t.starts_with('\'') {
            return Some((&a[..gt], tail, lead + 1 + gt + 1 + close + 1));
        }
        return None;
    }
    let paren = s.find(')')?;
    let inner = &s[..paren];
    let w = inner.find(char::is_whitespace).unwrap_or(inner.len());
    let (raw, tail) = (&inner[..w], &inner[w..]);
    let t = tail.trim();
    if t.is_empty() || t.starts_with('"') || t.starts_with('\'') {
        Some((raw, tail, lead + paren + 1))
    } else {
        None
    }
}

/// Resolve `.`/`..` components without touching the filesystem.
fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// Vault-root-relative path as a forward-slash string.
fn vault_rel(path: &Path, root: &Path) -> Option<String> {
    path.strip_prefix(root).ok().map(|p| {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    })
}

/// `<>`-wrap a rewritten destination when it can't be a bare URL.
fn wrap_dest(url: String) -> String {
    if url
        .chars()
        .any(|c| c.is_whitespace() || c == '(' || c == ')')
    {
        format!("<{url}>")
    } else {
        url
    }
}

/// Rewrite a relative markdown link/image destination the way the reference editor
/// resolves it: note-ish targets (`.md`, `.base`, extensionless) become
/// `wiki:` links against the vault, `![](note.md)` becomes a
/// transclusion, other existing files become `file://` links, and a
/// bare `#anchor` links to a heading in this note. Returns
/// (new destination, preserved title tail, bytes consumed).
fn rewrite_link_dest<'a>(
    s: &'a str,
    image: bool,
    doc_dir: &Path,
    doc_path: &Path,
    vault_root: Option<&Path>,
) -> Option<(String, &'a str, usize)> {
    let (raw, tail, used) = split_link_dest(s)?;
    if raw.is_empty() {
        return None;
    }
    // `scheme:`/`//host`/`/abs` destinations are untouched.
    let head = raw.split(['/', '#', '?']).next().unwrap_or(raw);
    if raw.contains("://") || head.contains(':') || raw.starts_with(['/', '\\']) {
        return None;
    }
    let (target, anchor) = match raw.split_once('#') {
        Some((t, a)) => (t, Some(percent_decode(a))),
        None => (raw, None),
    };
    let decoded = percent_decode(target);
    // `[x](#heading)` — same-document heading link.
    if target.is_empty() {
        let anchor = anchor.as_deref().filter(|a| !a.is_empty())?;
        let stem = doc_path.file_stem()?.to_str()?;
        return Some((wrap_dest(format!("wiki:{stem}#{anchor}")), tail, used));
    }
    let ext = decoded
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_note = ext == "md" || ext == "base" || !decoded.contains('.');
    let joined = normalize_path(&doc_dir.join(&decoded));
    let rel = vault_root.and_then(|r| vault_rel(&joined, r));
    if image {
        if is_note {
            let mut url = format!("transclude:{}", rel?);
            if let Some(a) = anchor {
                url.push('#');
                url.push_str(&a);
            }
            return Some((wrap_dest(url), tail, used));
        }
        return joined.exists().then(|| (file_url(&joined), tail, used));
    }
    if let Some(rel) = rel.filter(|_| is_note) {
        let mut url = format!("wiki:{rel}");
        if let Some(a) = anchor {
            url.push('#');
            url.push_str(&a);
        }
        return Some((wrap_dest(url), tail, used));
    }
    joined.exists().then(|| (file_url(&joined), tail, used))
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

struct NoteIconPlugin;

impl MarkdownPlugin for NoteIconPlugin {
    fn name(&self) -> &str {
        "note-icon"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Image(image) = node else {
            return None;
        };
        let path = image.url.strip_prefix("rista-icon:")?;
        let expected = crate::note_icons::shortcode(&image.alt)?.1;
        (path == expected).then(|| MarkdownNode::new("note-icon", expected).text(image.alt.clone()))
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<InlineElement> {
        let path = node.data::<String>()?;
        Some(
            InlineElement::new(
                svg()
                    .path(path.clone())
                    .size(context.font_size())
                    .text_color(context.text_style().color),
            )
            .with_baseline(context.font_size() * 0.85),
        )
    }
}

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
// the convention shared by the banner/cover plugins.
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
// Callouts: `> [!note]` / `> [!warning]-` admonitions.
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

/// Frontmatter rendered the reference-editor style: a compact, collapsible
/// "Properties" strip instead of a raw YAML table. Properties are the
/// data behind bases, so they stay visible — just not noisy.
struct PropertiesPlugin {
    folds: CalloutFolds,
    ctx: Option<PreviewCtx>,
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
                    .filter_map(|(k, v)| {
                        let key = k.as_str()?.to_string();
                        let links = prop_links(v);
                        Some((
                            key.clone(),
                            prop_edit_text(v),
                            prop_display(v),
                            links.clone(),
                            prop_kind(&key, v, links.is_some()),
                        ))
                    })
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
            .data::<(
                Vec<(
                    String,
                    String,
                    String,
                    Option<Vec<String>>,
                    assets::IconName,
                )>,
                usize,
            )>()
            .expect("properties node data");
        let theme = cx.theme();
        if entries.is_empty() {
            return div().into_any_element();
        }
        let visibility = self
            .ctx
            .as_ref()
            .map(|ctx| ctx.properties_visibility)
            .unwrap_or_default();
        let language = self
            .ctx
            .as_ref()
            .map(|ctx| ctx.language)
            .unwrap_or_default();
        if visibility == crate::settings::PropertiesVisibility::Hidden {
            return div().into_any_element();
        }
        let default_folded = visibility == crate::settings::PropertiesVisibility::Collapsed;
        let folded = self.folds.is_folded(*key, default_folded);
        let folds = self.folds.clone();
        let key = *key;
        // Rows open the property-edit dialog only on the document's own
        // strip — transcluded/fragment renders have no write target.
        let edit_ctx = self
            .ctx
            .as_ref()
            .filter(|ctx| ctx.depth == 0)
            .map(|ctx| ctx.workspace.clone());
        let mut rows = v_flex().w_full();
        if !folded {
            for (ix, (k, edit, v, links, kind)) in entries.iter().enumerate() {
                let mut key_cell = h_flex()
                    .id(("property-key", ix))
                    .w(px(96.))
                    .flex_none()
                    .gap_1()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .truncate()
                    .child({
                        // Click the type glyph → type picker;
                        // stop_propagation keeps the row's edit click.
                        let mut icon_cell = div().id(("property-type", ix)).child(
                            Icon::new(*kind)
                                .size(px(11.))
                                .text_color(theme.muted_foreground),
                        );
                        if let Some(workspace) = edit_ctx.clone() {
                            let k = k.clone();
                            let edit = edit.clone();
                            icon_cell =
                                icon_cell.cursor_pointer().on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    let Some(ws) = workspace.upgrade() else {
                                        return;
                                    };
                                    ws.update(cx, |ws, cx| {
                                        ws.show_property_type_picker(
                                            k.clone(),
                                            edit.clone(),
                                            window,
                                            cx,
                                        );
                                    });
                                });
                        }
                        icon_cell
                    })
                    .child(k.clone());
                let mut value_cell = div().flex_1().text_xs().truncate();
                match links {
                    // All-link values render as accent chips that open
                    // the note — the key cell keeps the edit click.
                    Some(targets) => {
                        let workspace = edit_ctx.clone();
                        let mut chips = h_flex().flex_1().flex_wrap().gap_1().items_center();
                        for (jx, target) in targets.iter().enumerate() {
                            let label = target.rsplit('/').next().unwrap_or(target).to_string();
                            let mut chip = div()
                                .id(("property-link", ix * 100 + jx))
                                .text_color(theme.info)
                                .child(label);
                            if let Some(workspace) = workspace.clone() {
                                let target = target.clone();
                                chip = chip
                                    .cursor_pointer()
                                    .hover(|c| c.underline())
                                    .on_click({
                                        let workspace = workspace.clone();
                                        let target = target.clone();
                                        move |ev, window, cx| {
                                            let Some(workspace) = workspace.upgrade() else {
                                                return;
                                            };
                                            workspace.update(cx, |workspace, cx| {
                                                if ev.modifiers().platform {
                                                    workspace
                                                        .open_wikilink_new_tab(&target, window, cx);
                                                } else {
                                                    workspace.open_wikilink(&target, window, cx);
                                                }
                                            });
                                        }
                                    })
                                    // Same page-preview hover as body wikilinks.
                                    .on_mouse_move({
                                        let workspace = workspace.clone();
                                        let target = target.clone();
                                        move |ev: &gpui::MouseMoveEvent, _window, cx| {
                                            let Some(ws) = workspace.upgrade() else {
                                                return;
                                            };
                                            ws.update(cx, |ws, cx| {
                                                if let Some(path) = ws
                                                    .vault_entity()
                                                    .read(cx)
                                                    .resolve_wikilink(&target)
                                                {
                                                    ws.peek_at(
                                                        crate::app::PeekKind::Note(path),
                                                        ev.position,
                                                        cx,
                                                    );
                                                }
                                            });
                                        }
                                    })
                                    .on_hover({
                                        let target = target.clone();
                                        move |hovered: &bool, _window, cx| {
                                            if *hovered {
                                                return;
                                            }
                                            let Some(ws) = workspace.upgrade() else {
                                                return;
                                            };
                                            ws.update(cx, |ws, cx| {
                                                if let Some(path) = ws
                                                    .vault_entity()
                                                    .read(cx)
                                                    .resolve_wikilink(&target)
                                                {
                                                    ws.hide_peek(
                                                        &crate::app::PeekKind::Note(path.clone()),
                                                        cx,
                                                    );
                                                }
                                            });
                                        }
                                    });
                            }
                            chips = chips.child(chip);
                            if jx + 1 < targets.len() {
                                chips = chips
                                    .child(div().text_color(theme.muted_foreground).child(","));
                            }
                        }
                        value_cell = div().flex_1().text_xs().child(chips);
                        if let Some(workspace) = edit_ctx.clone() {
                            let k = k.clone();
                            let edit = edit.clone();
                            key_cell = key_cell.cursor_pointer().hover(|c| c.underline()).on_click(
                                move |_, window, cx| {
                                    let Some(workspace) = workspace.upgrade() else {
                                        return;
                                    };
                                    workspace.update(cx, |workspace, cx| {
                                        workspace.edit_active_property(
                                            k.clone(),
                                            edit.clone(),
                                            window,
                                            cx,
                                        );
                                    });
                                },
                            );
                        }
                    }
                    None => {
                        if k == "tags" {
                            // Tag values render as `#x` chips — a click
                            // opens project search, like inline tags.
                            let mut chips = h_flex().flex_1().flex_wrap().gap_1().items_center();
                            for (jx, tag) in v
                                .split(',')
                                .map(|t| t.trim().trim_start_matches('#'))
                                .filter(|t| !t.is_empty())
                                .enumerate()
                            {
                                let mut chip = div()
                                    .id(("property-tag", ix * 100 + jx))
                                    .text_color(theme.info)
                                    .child(format!("#{tag}"));
                                if let Some(workspace) = edit_ctx.clone() {
                                    let tag = tag.to_string();
                                    chip = chip.cursor_pointer().hover(|c| c.underline()).on_click(
                                        move |_, window, cx| {
                                            // Don't bubble into the row's
                                            // property-edit click.
                                            cx.stop_propagation();
                                            let Some(ws) = workspace.upgrade() else {
                                                return;
                                            };
                                            crate::search::open_project_search_for(
                                                ws.clone(),
                                                Some(&format!("#{tag}")),
                                                window,
                                                cx,
                                            );
                                        },
                                    );
                                }
                                chips = chips.child(chip);
                            }
                            value_cell = div().flex_1().text_xs().child(chips);
                        } else if *kind == assets::IconName::SquareCheck {
                            // Bool props get the ☐/☑ glyph .base cells use —
                            // the row's toggle-on-click stays the same.
                            value_cell = value_cell
                                .text_color(theme.foreground)
                                .child(if v == "true" { "☑" } else { "☐" });
                        } else {
                            value_cell = value_cell.text_color(theme.foreground).child(v.clone());
                        }
                    }
                }
                let mut row = h_flex()
                    .id(("property-row", ix))
                    .w_full()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border.opacity(0.5))
                    .child(key_cell)
                    .child(value_cell);
                if links.is_none() {
                    if let Some(workspace) = edit_ctx.clone() {
                        let k = k.clone();
                        let edit = edit.clone();
                        row = row
                            .cursor_pointer()
                            .hover(|row| row.bg(theme.accent.opacity(0.4)))
                            .on_click(move |_, window, cx| {
                                let Some(workspace) = workspace.upgrade() else {
                                    return;
                                };
                                workspace.update(cx, |workspace, cx| {
                                    workspace.edit_active_property(
                                        k.clone(),
                                        edit.clone(),
                                        window,
                                        cx,
                                    );
                                });
                            });
                    }
                }
                rows = rows.child(row);
            }
            if let Some(workspace) = edit_ctx.clone() {
                rows = rows.child(
                    div()
                        .id("property-add")
                        .w_full()
                        .px_3()
                        .py_1()
                        .border_t_1()
                        .border_color(theme.border.opacity(0.5))
                        .cursor_pointer()
                        .hover(|row| row.bg(theme.accent.opacity(0.4)))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(language.text("+ Add property", "+ Legg til egenskap")),
                        )
                        .on_click(move |_, window, cx| {
                            let Some(workspace) = workspace.upgrade() else {
                                return;
                            };
                            workspace.update(cx, |workspace, cx| {
                                workspace.show_add_property_dialog(window, cx);
                            });
                        }),
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
                        folds.toggle(key, default_folded);
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
                            .child(language.text("Properties", "Egenskaper")),
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

/// Edit-box text for a frontmatter property — scalars as-is, lists in
/// YAML flow form (`[a, b]`) so committing parses back to a sequence.
fn prop_edit_text(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Sequence(items) => {
            let items = items
                .iter()
                .map(|item| match item {
                    serde_yaml::Value::String(s) if s.contains(',') => {
                        format!("{:?}", s)
                    }
                    other => prop_edit_text(other),
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{items}]")
        }
        other => serde_yaml::to_string(other)
            .map(|s| s.trim().to_string())
            .unwrap_or_default(),
    }
}

/// `[[wikilink]]` targets when EVERY scalar in the value is a link —
/// `related: "[[a]]"` or a list of links renders as clickable chips;
/// a mixed `["[[a]]", "plain text"]` stays plain text.
fn prop_links(v: &serde_yaml::Value) -> Option<Vec<String>> {
    fn target(s: &str) -> Option<String> {
        s.trim()
            .strip_prefix("![[")
            .or_else(|| s.trim().strip_prefix("[["))
            .and_then(|s| s.strip_suffix("]]"))
            .map(|t| t.split('|').next().unwrap_or(t).trim().to_string())
            .filter(|t| !t.is_empty())
    }
    match v {
        serde_yaml::Value::String(s) => target(s).map(|t| vec![t]),
        serde_yaml::Value::Sequence(items) if !items.is_empty() => {
            items.iter().map(|i| i.as_str().and_then(target)).collect()
        }
        _ => None,
    }
}

/// Lucide icon for a property's YAML type — the Properties
/// panel marks each row with its type glyph.
fn prop_kind(key: &str, v: &serde_yaml::Value, has_links: bool) -> assets::IconName {
    use assets::IconName as I;
    if key == "tags" {
        return I::Tags;
    }
    if has_links {
        return I::Link;
    }
    match v {
        serde_yaml::Value::Bool(_) => I::SquareCheck,
        serde_yaml::Value::Number(_) => I::Hash,
        serde_yaml::Value::Sequence(_) => I::List,
        serde_yaml::Value::String(s) => {
            let s = s.trim();
            // `YYYY-MM-DD` alone → calendar; a datetime tail → clock.
            let b = s.as_bytes();
            let date = s.len() >= 10
                && b[4] == b'-'
                && b[7] == b'-'
                && s[..4].bytes().all(|c| c.is_ascii_digit())
                && s[5..7].bytes().all(|c| c.is_ascii_digit())
                && s[8..10].bytes().all(|c| c.is_ascii_digit());
            if !date {
                I::Type
            } else if s.len() == 10 {
                I::Calendar
            } else {
                I::Clock
            }
        }
        _ => I::Type,
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
                            .markdown_extensions(extensions(
                                &self.folds,
                                self.ctx.as_ref(),
                                false,
                            )),
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
    // the reference editor fold markers: `[!type]-` starts folded, `[!type]+` starts
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
// bookmark card (rich ) once its OpenGraph metadata has loaded.

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
            // `this` differs per host — identical specs in different
            // notes must not share the cached view entity.
            self.ctx.doc_path.hash(&mut hasher);
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
                            self.ctx.doc_path.clone(),
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
// `::: columns` fenced layout — `preprocess` rewrites the container
// into a `columns` code fence whose `:::colsep:::` lines split the
// columns. Each column renders nested markdown side-by-side.
// ------------------------------------------------------------------

struct ColumnBlock {
    body: String,
}

struct ColumnBlockPlugin {
    ctx: PreviewCtx,
    folds: CalloutFolds,
}

impl MarkdownPlugin for ColumnBlockPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "column-block"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Code(code) = node else {
            return None;
        };
        if code.lang.as_deref() != Some("columns") {
            return None;
        }
        Some(MarkdownNode::new(
            "column-block",
            ColumnBlock {
                body: code.value.clone(),
            },
        ))
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let block = node.data::<ColumnBlock>().expect("column-block node data");
        let mut row = h_flex().w_full().items_start().gap_4().my_2();
        for (ix, segment) in block.body.split(":::colsep:::").enumerate() {
            let nested = self.ctx.clone();
            let segment = segment.trim_matches('\n').to_string();
            row = row.child(
                div().flex_1().min_w_0().child(
                    gpui_kit::component::text::TextView::markdown(
                        SharedString::from(format!("column-{ix}")),
                        segment,
                    )
                    .markdown_extensions(extensions(
                        &self.folds,
                        Some(&nested),
                        false,
                    )),
                ),
            );
        }
        row.into_any_element()
    }
}

// ------------------------------------------------------------------
// Editable tables — doc-level tables render with per-cell click
// targets; a click opens the cell dialog and splices the row's pipe
// segment back into source via `Document::set_table_cell`. Nested
// fragments (columns, callouts, transclusions) keep the built-in
// table: writes must land in the document being previewed, so the
// plugin registers only when `doc_anchored`.
// ------------------------------------------------------------------

struct EditableCell {
    /// Transformed source of the cell's content — nested-rendered.
    display: String,
    /// 1-based source line of the cell's row.
    line: usize,
    /// Cell index within its row.
    col: usize,
}

struct EditableTable {
    /// `rows[0]` is the header row; the rest are body rows.
    rows: Vec<Vec<EditableCell>>,
    /// 1-based source line of the header row (the GFM `---` separator
    /// is always `start + 1`).
    start: usize,
    /// 1-based source line of the last row.
    end: usize,
    /// Column count (header cells).
    cols: usize,
}

struct TablePlugin {
    ctx: PreviewCtx,
    folds: CalloutFolds,
}

impl MarkdownPlugin for TablePlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "editable-table"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Table(table) = node else {
            return None;
        };
        let rows = table
            .children
            .iter()
            .map(|row| {
                let mdast::Node::TableRow(row) = row else {
                    return Vec::new();
                };
                row.children
                    .iter()
                    .enumerate()
                    .filter_map(|(col, cell)| {
                        let line = cell.position()?.start.line;
                        // mdast-gfm's cell span reaches back to the
                        // preceding `|`, and the last cell in a row
                        // swallows the row's trailing pipe too.
                        let raw = cx
                            .node_source(cell)
                            .unwrap_or_default()
                            .trim()
                            .trim_start_matches('|');
                        let display = raw
                            .strip_suffix('|')
                            .filter(|_| !raw.ends_with("\\|"))
                            .unwrap_or(raw)
                            .trim()
                            .to_string();
                        Some(EditableCell { display, line, col })
                    })
                    .collect()
            })
            .collect::<Vec<Vec<EditableCell>>>();
        let start = rows
            .first()
            .and_then(|r| r.first())
            .map(|c| c.line)
            .unwrap_or(0);
        let end = rows
            .last()
            .and_then(|r| r.first())
            .map(|c| c.line)
            .unwrap_or(start);
        let cols = rows.first().map(|r| r.len()).unwrap_or(0);
        Some(MarkdownNode::new(
            "editable-table",
            EditableTable {
                rows,
                start,
                end,
                cols,
            },
        ))
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let table = node.data::<EditableTable>().expect("editable-table data");
        let theme = cx.theme();
        let last = table.rows.len().saturating_sub(1);
        let mut tbl = v_flex()
            .w_full()
            .my_2()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .overflow_hidden();
        for (rix, row) in table.rows.iter().enumerate() {
            let mut r = h_flex().w_full();
            if rix == 0 {
                r = r.bg(theme.table_head);
            }
            if rix < last {
                r = r.border_b_1().border_color(theme.border);
            }
            for (cix, cell) in row.iter().enumerate() {
                let line = cell.line;
                let col = cell.col;
                let workspace = self.ctx.workspace.clone();
                // Header cells render their raw text bolded through the
                // nested markdown; `**` only when there is content.
                let display = if rix == 0 && !cell.display.is_empty() {
                    format!("**{}**", cell.display)
                } else {
                    cell.display.clone()
                };
                r = r.child(
                    div()
                        .id(("table-cell", rix * 4096 + cix))
                        .flex_1()
                        .min_w_0()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted.opacity(0.4)))
                        .on_click(move |_, window, cx| {
                            let _ = workspace.update(cx, |ws, cx| {
                                ws.show_table_cell_dialog(line, col, window, cx)
                            });
                        })
                        .child(
                            gpui_kit::component::text::TextView::markdown(
                                SharedString::from(format!("table-cell-{rix}-{cix}")),
                                display,
                            )
                            .markdown_extensions(extensions(
                                &self.folds,
                                Some(&self.ctx),
                                false,
                            )),
                        ),
                );
            }
            if rix == 0 {
                // Trailing "+" cell on the header — appends an empty
                // column (sep row gets `---`).
                let workspace = self.ctx.workspace.clone();
                let (start, end) = (table.start, table.end);
                r = r.child(
                    div()
                        .id("table-add-col")
                        .w(px(26.))
                        .flex_none()
                        .px_1()
                        .py_1()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted.opacity(0.4)))
                        .child("+")
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = workspace
                                .update(cx, |ws, cx| ws.add_table_col(start, end, window, cx));
                        }),
                );
            }
            tbl = tbl.child(r);
        }
        // "+ New row" footer — splices `|  |  |…` after the table's
        // last source line.
        {
            let workspace = self.ctx.workspace.clone();
            let (end, cols) = (table.end, table.cols);
            tbl = tbl.child(
                div()
                    .id("table-add-row")
                    .w_full()
                    .px_2()
                    .py_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted.opacity(0.4)))
                    .child("+ New row")
                    .on_click(move |_, window, cx| {
                        let _ =
                            workspace.update(cx, |ws, cx| ws.add_table_row(end, cols, window, cx));
                    }),
            );
        }
        tbl.into_any_element()
    }
}

// ------------------------------------------------------------------
// `![[note]]` transclusion — note content rendered inline.
// ------------------------------------------------------------------

/// Deeper nesting than this renders as a link — cyclic embeds terminate.
const TRANSCLUDE_MAX_DEPTH: usize = 2;

/// Section slice for `![[note#heading]]` / `![[note#^block]]` embeds:
/// heading anchors return the heading through the line before the next
/// heading of equal-or-higher level; `^block` anchors return the block
/// paragraph with the `^id` marker stripped. Fenced code can't spoof
/// either kind. `None` when the anchor isn't in the text.
/// First and last line of the block a `^id` marker names: the
/// contiguous non-blank lines around a trailing ` ^id`, or — for a lone
/// `^id` line set off by a blank line, the structured-block convention
/// for quotes, tables, and code fences — the block above it.
pub(crate) fn block_lines(lines: &[&str], id: &str) -> Option<(usize, usize)> {
    let marker = format!("^{id}");
    let at = lines.iter().position(|l| {
        let t = l.trim_end();
        t.trim_start() == marker || t.ends_with(&format!(" {marker}"))
    })?;
    let blank = |i: usize| lines[i].trim().is_empty();
    let mut e = at;
    if lines[at].trim() == marker && at > 0 && blank(at - 1) {
        e = (0..at).rev().find(|&i| !blank(i))?;
    } else {
        while e + 1 < lines.len() && !blank(e + 1) {
            e += 1;
        }
    }
    let mut s = e.min(at);
    while s > 0 && !blank(s - 1) {
        s -= 1;
    }
    Some((s, e))
}

fn slice_section(text: &str, anchor: &str) -> Option<String> {
    let anchor = anchor.trim();
    if anchor.is_empty() {
        return None;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    fn heading_at(l: &str, in_fence: bool) -> Option<(usize, &str)> {
        if in_fence {
            return None;
        }
        let t = l.trim_start();
        let level = t.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&level) && t.chars().nth(level) == Some(' ') {
            Some((level, t[level + 1..].trim()))
        } else {
            None
        }
    }

    // `^block-id` — the block the marker names.
    if let Some(id) = anchor.strip_prefix('^') {
        let marker = format!("^{id}");
        let (s, e) = block_lines(&lines, id)?;
        let mut block = lines[s..=e].to_vec();
        let last = block.len() - 1;
        block[last] = block[last]
            .trim_end()
            .strip_suffix(&marker)
            .map(|s| s.trim_end())
            .unwrap_or(block[last]);
        return Some(block.join("\n"));
    }

    // `#heading` — from the heading through the line before the next
    // heading at the same or shallower level.
    let mut in_fence = false;
    let mut start: Option<(usize, usize)> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if let Some((level, title)) = heading_at(l, in_fence) {
            if title.eq_ignore_ascii_case(anchor) {
                start = Some((i, level));
                break;
            }
        }
    }
    let (i, level) = start?;
    let mut end = lines.len();
    in_fence = false;
    for (j, l) in lines.iter().enumerate().skip(i + 1) {
        let t = l.trim_start();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if let Some((l2, _)) = heading_at(l, in_fence) {
            if l2 <= level {
                end = j;
                break;
            }
        }
    }
    Some(lines[i..end].join("\n"))
}

struct Transclude {
    target: String,
    /// `|height` suffix on `.base` embeds — pixel height of the frame.
    height: Option<f32>,
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
        // `![[db.base|400]]` — a trailing `|N` is the embed height.
        let (target, height) = match target.rsplit_once('|') {
            Some((t, h)) => (t.to_string(), h.parse::<f32>().ok()),
            None => (target.to_string(), None),
        };
        Some(MarkdownNode::new(
            "transclude",
            Transclude { target, height },
        ))
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let embed = node.data::<Transclude>().expect("transclude node data");
        let theme = cx.theme().clone();
        let resolved = self.ctx.vault.read(cx).resolve_wikilink(&embed.target);
        let anchor = embed.target.split_once('#').map(|(_, a)| a.to_string());
        // `![[db.base]]` embeds the live base view —
        // keyed by spec text + view anchor so the view survives re-renders.
        if let Some(path) = resolved.as_ref().filter(|p| crate::app::is_base(p)) {
            if let Ok(spec) = std::fs::read_to_string(path) {
                let key = {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    spec.hash(&mut hasher);
                    self.ctx.doc_path.hash(&mut hasher);
                    anchor.hash(&mut hasher);
                    hasher.finish()
                };
                let view = {
                    let mut views = self.ctx.views.lock().expect("embed views");
                    match views.get(&key) {
                        Some(view) => view.clone(),
                        None => {
                            let view = cx.new(|cx| {
                                crate::bases::BaseView::for_inline(
                                    spec.clone(),
                                    self.ctx.vault.clone(),
                                    self.ctx.workspace.clone(),
                                    self.ctx.doc_path.clone(),
                                    window,
                                    cx,
                                )
                            });
                            views.insert(key, view.clone());
                            view
                        }
                    }
                };
                // `![[db.base#View]]` opens on the named view.
                if let Some(anchor) = anchor.as_deref() {
                    view.update(cx, |view, cx| view.select_view_by_name(anchor, cx));
                }
                return div()
                    .w_full()
                    .h(px(embed.height.unwrap_or(320.).clamp(80., 4000.)))
                    .my_2()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .overflow_hidden()
                    .child(view)
                    .into_any_element();
            }
        }
        let content = resolved
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| match crate::properties::frontmatter_span(&text) {
                Some(span) => text[span.end..].to_string(),
                None => text,
            })
            .and_then(|text| match anchor.as_deref() {
                // `![[note#heading]]` / `![[note#^block]]` embed only
                // that section or block; a missing anchor renders the
                // link fallback rather than the whole note.
                Some(a) => slice_section(&text, a),
                None => Some(text),
            });

        match (self.ctx.depth < TRANSCLUDE_MAX_DEPTH, content) {
            (true, Some(content)) => {
                let mut nested = self.ctx.clone();
                nested.depth += 1;
                // `this` inside the transcluded note's embeds is the
                // transcluded file, not the page hosting it.
                nested.doc_path = resolved.clone();
                div()
                    .w_full()
                    .my_2()
                    .border_l_2()
                    .border_color(theme.border)
                    .pl_3()
                    .child(
                        gpui_kit::component::text::TextView::markdown("transclude", content)
                            .markdown_extensions(extensions(&self.folds, Some(&nested), false)),
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
                    .text_color(theme.info)
                    .cursor_pointer()
                    .child(embed.target.clone())
                    .on_click(move |ev, window, cx| {
                        let target = target.clone();
                        let _ = workspace.update(cx, |ws, cx| {
                            if ev.modifiers().platform {
                                ws.open_wikilink_new_tab(&target, window, cx)
                            } else {
                                ws.open_wikilink(&target, window, cx)
                            }
                        });
                    })
                    .into_any_element()
            }
        }
    }
}

// ------------------------------------------------------------------
// Wikilinks — `[label](wiki:target)` rewritten by `preprocess`. Accent
// when the target resolves, dimmed when it doesn't (the "unresolved" styling); clicks flow through `open_wikilink`, which
// creates the note when it's missing.
// ------------------------------------------------------------------

#[derive(Debug, Clone)]
struct WikiLink {
    target: String,
    label: String,
}

struct WikiLinkPlugin {
    ctx: PreviewCtx,
}

/// Visible text of an mdast node — wiki labels are plain text almost
/// always, but `[[x| *emphatic* ]]` nests inline children.
fn mdast_plain_text(node: &mdast::Node) -> String {
    match node {
        mdast::Node::Text(t) => t.value.clone(),
        mdast::Node::InlineCode(c) => c.value.clone(),
        _ => node
            .children()
            .map(|cs| cs.iter().map(mdast_plain_text).collect::<String>())
            .unwrap_or_default(),
    }
}

impl MarkdownPlugin for WikiLinkPlugin {
    fn name(&self) -> &str {
        "wiki-link"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::Link(link) = node else {
            return None;
        };
        let target = link.url.strip_prefix("wiki:")?.to_string();
        let label = link
            .children
            .iter()
            .map(mdast_plain_text)
            .collect::<String>();
        let label = if label.is_empty() {
            target.clone()
        } else {
            label
        };
        Some(
            MarkdownNode::new(
                "wiki-link",
                WikiLink {
                    target,
                    label: label.clone(),
                },
            )
            .text(label),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let link = node.data::<WikiLink>().expect("wiki-link node data");
        let theme = cx.theme();
        let resolved_path = self.ctx.vault.read(cx).resolve_wikilink(&link.target);
        let resolved = resolved_path.is_some();
        let workspace = self.ctx.workspace.clone();
        let target = link.target.clone();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&link.target, &mut hasher);
        let key = std::hash::Hasher::finish(&hasher) as usize;

        let mut el = div()
            .id(("wiki-link", key))
            .text_sm()
            .text_color(if resolved {
                theme.info
            } else {
                theme.info.opacity(0.45)
            })
            .cursor_pointer()
            .hover(|d| d.underline())
            .child(link.label.clone())
            .on_click(move |ev, window, cx| {
                let target = target.clone();
                let _ = workspace.update(cx, |ws, cx| {
                    if ev.modifiers().platform {
                        ws.open_wikilink_new_tab(&target, window, cx)
                    } else {
                        ws.open_wikilink(&target, window, cx)
                    }
                });
            });
        // Resolved links get the page-preview hover: the card
        // anchors where the cursor first crossed the link and clears
        // on hover-out (or on navigation).
        if let Some(path) = resolved_path {
            el = el
                .on_mouse_move({
                    let workspace = self.ctx.workspace.clone();
                    let path = path.clone();
                    move |ev: &gpui::MouseMoveEvent, _window, cx| {
                        let _ = workspace.update(cx, |ws, cx| {
                            ws.peek_at(crate::app::PeekKind::Note(path.clone()), ev.position, cx)
                        });
                    }
                })
                .on_hover({
                    let workspace = self.ctx.workspace.clone();
                    move |hovered: &bool, _window, cx| {
                        if !*hovered {
                            let path = path.clone();
                            let _ = workspace.update(cx, |ws, cx| {
                                ws.hide_peek(&crate::app::PeekKind::Note(path.clone()), cx)
                            });
                        }
                    }
                });
        }
        el
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

// ------------------------------------------------------------------
// Task lists — `- [ ]`/`- [x]` items render interactive checkboxes
// that splice the marker back into the document. The whole list is
// claimed (mixed task/plain items render together, nested lists flow
// through each item's body TextView).
// ------------------------------------------------------------------

#[derive(Clone)]
struct TaskItem {
    checked: Option<bool>,
    /// 1-based source line of the `- [ ]` marker — write-back anchor.
    /// Lines survive `preprocess` untouched, unlike byte offsets.
    line: usize,
    /// Item body markdown — list marker and `[ ]` stripped.
    body: String,
}

struct TaskList {
    ordered: bool,
    items: Vec<TaskItem>,
}

fn strip_item_marker(src: &str) -> &str {
    let t = src.trim_start();
    if t.len() > 2 && matches!(t.as_bytes()[0], b'-' | b'*' | b'+') && t.as_bytes()[1] == b' ' {
        return &t[2..];
    }
    if let Some(dot) = t.find(". ") {
        if dot < 4 && t[..dot].chars().all(|c| c.is_ascii_digit()) {
            return &t[dot + 2..];
        }
    }
    src
}

struct TaskListPlugin {
    ctx: Option<PreviewCtx>,
    folds: CalloutFolds,
    /// True only when parsing the document itself (not a nested fragment).
    doc_anchored: bool,
}

impl MarkdownPlugin for TaskListPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "task-list"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::List(list) = node else {
            return None;
        };
        if !list
            .children
            .iter()
            .any(|c| matches!(c, mdast::Node::ListItem(i) if i.checked.is_some()))
        {
            return None;
        }
        let mut items = Vec::new();
        for child in &list.children {
            let mdast::Node::ListItem(item) = child else {
                continue;
            };
            let src = cx.node_source(child)?;
            let line = child.position().map(|p| p.start.line).unwrap_or(0);
            let body = if item.checked.is_some() {
                src.find(']')
                    .map(|i| src[i + 1..].trim_start_matches(' ').to_string())
                    .unwrap_or_else(|| src.to_string())
            } else {
                strip_item_marker(src).to_string()
            };
            items.push(TaskItem {
                checked: item.checked,
                line,
                body,
            });
        }
        Some(
            MarkdownNode::new(
                "task-list",
                TaskList {
                    ordered: list.ordered,
                    items,
                },
            )
            .markdown(cx.node_source(node).unwrap_or_default().to_string()),
        )
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let list = node.data::<TaskList>().expect("task-list node data");
        let theme = cx.theme();
        let mut rows = v_flex().w_full().my_1().gap_0p5();
        let mut ordinal = 0u32;
        for (ix, item) in list.items.iter().enumerate() {
            ordinal += 1;
            let marker_el = match item.checked {
                Some(checked) => {
                    let ctx = self.doc_anchored.then(|| self.ctx.clone()).flatten();
                    let line = item.line;
                    let mut cb = div()
                        .id(("task-checkbox", line))
                        .w(px(15.))
                        .h(px(15.))
                        .mt(px(4.))
                        .flex_none()
                        .border_1()
                        .rounded(px(3.));
                    if checked {
                        cb = cb.border_color(theme.accent).bg(theme.accent).child(
                            Icon::new(assets::IconName::Check)
                                .size_3()
                                .text_color(theme.accent_foreground),
                        );
                    } else {
                        cb = cb.border_color(theme.muted_foreground);
                    }
                    if let Some(ctx) = ctx {
                        cb = cb.cursor_pointer().on_click(move |_, window, cx| {
                            let _ = ctx
                                .workspace
                                .update(cx, |ws, cx| ws.toggle_task_pub(line, window, cx));
                        });
                    }
                    cb.into_any_element()
                }
                None if list.ordered => div()
                    .w(px(20.))
                    .flex_none()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(format!("{ordinal}."))
                    .into_any_element(),
                None => div()
                    .w(px(16.))
                    .flex_none()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .text_center()
                    .child("•")
                    .into_any_element(),
            };
            // Checked items strike their first line — `~~` is inline-level
            // so it can't wrap block content; the first line is the visible
            // item text anyway.
            let body = match item.checked {
                Some(true) => {
                    let (first, rest) = item
                        .body
                        .split_once('\n')
                        .map(|(a, b)| (a.to_string(), format!("\n{b}")))
                        .unwrap_or((item.body.clone(), String::new()));
                    format!("~~{first}~~{rest}")
                }
                _ => item.body.clone(),
            };
            rows = rows.child(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_1p5()
                    .child(marker_el)
                    .child(
                        div().flex_1().min_w_0().child(
                            gpui_kit::component::text::TextView::markdown(("task-item", ix), body)
                                .markdown_extensions(extensions(
                                    &self.folds,
                                    self.ctx.as_ref(),
                                    false,
                                )),
                        ),
                    ),
            );
        }
        rows
    }
}

// ------------------------------------------------------------------
// Footnotes — `[^label]` refs render as a small accent marker, and
// `[^label]:` definitions render as a muted labelled block in place.
// ------------------------------------------------------------------

struct FootnoteRefPlugin {
    ctx: Option<PreviewCtx>,
}

impl MarkdownPlugin for FootnoteRefPlugin {
    fn name(&self) -> &str {
        "footnote-ref"
    }

    fn parse(&self, node: &mdast::Node, _cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::FootnoteReference(r) = node else {
            return None;
        };
        // Offset keeps element ids unique when the same label is
        // referenced twice.
        let offset = node.position().map(|p| p.start.offset).unwrap_or(0);
        Some(MarkdownNode::new(
            "footnote-ref",
            (
                r.label.clone().unwrap_or_else(|| r.identifier.clone()),
                offset,
            ),
        ))
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (label, offset) = node
            .data::<(String, usize)>()
            .expect("footnote-ref data")
            .clone();
        let workspace = self.ctx.as_ref().map(|ctx| ctx.workspace.clone());
        div()
            .id(format!("fnref-{offset}"))
            .text_xs()
            .text_color(cx.theme().info)
            .child(format!("[{label}]"))
            .when_some(workspace, |this, workspace| {
                this.on_mouse_move({
                    let label = label.clone();
                    let workspace = workspace.clone();
                    move |ev, _window, cx| {
                        if let Some(ws) = workspace.upgrade() {
                            ws.update(cx, |ws, cx| {
                                ws.peek_footnote(label.clone(), ev.position, cx);
                            });
                        }
                    }
                })
                .on_hover({
                    let label = label.clone();
                    move |hovered: &bool, _window, cx| {
                        if *hovered {
                            return;
                        }
                        if let Some(ws) = workspace.upgrade() {
                            ws.update(cx, |ws, cx| {
                                ws.hide_footnote_peek(&label, cx);
                            });
                        }
                    }
                })
            })
    }
}

struct FootnoteDefPlugin {
    ctx: Option<PreviewCtx>,
    folds: CalloutFolds,
}

impl MarkdownPlugin for FootnoteDefPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "footnote-def"
    }

    fn parse(&self, node: &mdast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let mdast::Node::FootnoteDefinition(d) = node else {
            return None;
        };
        let label = d.label.clone().unwrap_or_else(|| d.identifier.clone());
        // Body = the definition's source minus its `[^label]:` marker —
        // only the first line carries it; continuations stay verbatim.
        let src = cx.node_source(node).unwrap_or_default();
        let body = src
            .find(']')
            .map(|i| {
                src[i + 1..]
                    .trim_start_matches(':')
                    .trim_start()
                    .to_string()
            })
            .unwrap_or_else(|| src.to_string());
        Some(MarkdownNode::new("footnote-def", (label, body)))
    }

    fn render(&self, node: &MarkdownNode, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (label, body) = node.data::<(String, String)>().expect("footnote-def data");
        let theme = cx.theme();
        let nested = self.ctx.clone();
        h_flex()
            .w_full()
            .items_start()
            .my_0p5()
            .child(
                div()
                    .w(px(30.))
                    .flex_none()
                    .pt(px(2.))
                    .text_xs()
                    .text_color(theme.info)
                    .child(format!("[{label}]")),
            )
            .child(
                div().flex_1().min_w_0().text_xs().child(
                    gpui_kit::component::text::TextView::markdown(
                        SharedString::from(format!("footnote-{label}")),
                        body.clone(),
                    )
                    .markdown_extensions(extensions(
                        &self.folds,
                        nested.as_ref(),
                        false,
                    )),
                ),
            )
    }
}
