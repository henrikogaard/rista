use base64::Engine as _;
use gpui_kit::{component::ActiveTheme, App, Hsla};
use markdown::mdast::Node;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use unicode_normalization::UnicodeNormalization as _;

#[cfg(target_os = "macos")]
#[path = "pdf_export_macos.rs"]
mod native;
#[cfg(target_os = "macos")]
pub use native::ExportJob;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Paper {
    #[default]
    A4,
    Letter,
}

impl Paper {
    pub fn points(self) -> (f64, f64) {
        match self {
            Self::A4 => (595.276, 841.89),
            Self::Letter => (612., 792.),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Options {
    pub paper: Paper,
    pub margin_mm: f64,
    pub active_theme: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            paper: Paper::A4,
            margin_mm: 20.,
            active_theme: false,
        }
    }
}

impl Options {
    pub fn valid(self) -> bool {
        self.margin_mm.is_finite() && (5.0..=50.0).contains(&self.margin_mm)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportError {
    TooLarge,
    Invalid,
    Image,
    Math,
    Unsupported,
    Render,
    Write,
}

impl ExportError {
    pub fn message(self, language: crate::settings::Language) -> &'static str {
        match self {
            Self::TooLarge => language.text(
                "This note or its images are too large to export.",
                "Notatet eller bildene er for store til å eksporteres.",
            ),
            Self::Invalid => language.text(
                "The note could not be prepared for PDF export.",
                "Notatet kunne ikke klargjøres for PDF-eksport.",
            ),
            Self::Image => language.text(
                "An image could not be embedded. Use local PNG, JPEG, GIF or WebP images.",
                "Et bilde kunne ikke bygges inn. Bruk lokale PNG-, JPEG-, GIF- eller WebP-bilder.",
            ),
            Self::Math => language.text(
                "A math expression could not be rendered. Correct it before exporting.",
                "Et matematikkuttrykk kunne ikke gjengis. Rett det før eksport.",
            ),
            Self::Unsupported => language.text(
                "PDF export is currently available on macOS only.",
                "PDF-eksport er foreløpig bare tilgjengelig på macOS.",
            ),
            Self::Render => language.text(
                "PDF rendering failed or timed out. The destination was not changed.",
                "PDF-gjengivelsen mislyktes eller tok for lang tid. Målfilen ble ikke endret.",
            ),
            Self::Write => language.text(
                "The PDF could not be saved. Check the destination and permissions.",
                "PDF-filen kunne ikke lagres. Kontroller målmappen og tilgangene.",
            ),
        }
    }
}

#[derive(Clone)]
pub struct Style {
    font: String,
    size: f32,
    foreground: Hsla,
    background: Hsla,
    muted: Hsla,
    border: Hsla,
    accent: Hsla,
}

impl Style {
    pub fn new(settings: &crate::settings::Settings, active: bool, cx: &App) -> Self {
        let mut theme = cx.theme().clone();
        if !active {
            theme.apply_config(&theme.light_theme.clone());
        }
        Self {
            font: font_family(&settings.editor_font_family).to_owned(),
            size: settings.editor_font_size.clamp(8., 48.),
            foreground: theme.foreground,
            background: theme.background,
            muted: theme.muted,
            border: theme.border,
            accent: theme.primary,
        }
    }
}

pub fn font_family(requested: &str) -> &str {
    #[cfg(target_os = "macos")]
    if !native::font_is_available(requested) {
        return "Menlo";
    }
    requested
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn css_color(color: Hsla) -> String {
    let rgb = color.to_rgb();
    format!(
        "rgba({},{},{},{})",
        (rgb.r * 255.) as u8,
        (rgb.g * 255.) as u8,
        (rgb.b * 255.) as u8,
        rgb.a
    )
}

fn css_string(value: &str) -> String {
    value
        .chars()
        .map(|c| format!("\\{:x} ", c as u32))
        .collect()
}

pub fn save(path: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    if !bytes.starts_with(b"%PDF-") || bytes.len() > 128 * 1024 * 1024 {
        return Err(ExportError::Render);
    }
    let parent = path.parent().ok_or(ExportError::Write)?;
    let temporary = parent.join(format!(
        ".rista-pdf-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|_| ExportError::Write)?;
    let result = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| {
            drop(file);
            std::fs::rename(&temporary, path)
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|_| ExportError::Write)
}

pub fn html(
    text: &str,
    path: &Path,
    root: Option<&Path>,
    files: &[PathBuf],
    style: &Style,
) -> Result<String, ExportError> {
    if text.len() > 2 * 1024 * 1024 {
        return Err(ExportError::TooLarge);
    }
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    options.constructs.math_flow = true;
    options.constructs.math_text = true;
    let ast = markdown::to_mdast(text, &options).map_err(|_| ExportError::Invalid)?;
    let mut renderer = Renderer {
        path,
        root,
        files,
        style,
        bytes: 0,
        definitions: std::collections::HashMap::new(),
    };
    renderer.definitions(&ast);
    let body = renderer.node(&ast, 0)?;
    let title = escape(
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
    );
    Ok(format!(
        r#"<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>{title}</title><style>
:root{{color-scheme:light}}*{{box-sizing:border-box}}
html,body{{margin:0;padding:0;background:{background};color:{foreground};-webkit-print-color-adjust:exact}}
body{{font-family:'{font}',monospace;font-size:{size}px;line-height:1.55;overflow-wrap:anywhere}}
h1,h2,h3,h4,h5,h6{{line-height:1.2;break-after:avoid;margin:1.2em 0 .5em}}h1{{font-size:2em}}h2{{font-size:1.5em}}
p{{margin:.6em 0;orphans:3;widows:3}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;background:{muted};padding:.8em;border-radius:4px}}
code{{font-family:inherit;font-size:.9em}}blockquote{{border-left:3px solid {border};padding:.3em 1em;margin:1em 0}}.callout{{background:{muted}}}
table{{border-collapse:collapse;width:100%;table-layout:fixed;margin:1em 0}}thead{{display:table-header-group}}tr{{break-inside:avoid}}
th,td{{border:1px solid {border};padding:.4em;text-align:left;overflow-wrap:anywhere}}th{{background:{muted}}}
img{{max-width:100%;height:auto;break-inside:avoid}}.math{{vertical-align:middle}}.display-math{{text-align:center;margin:1em 0;break-inside:avoid}}
a{{color:{accent};text-decoration:underline}}hr{{border:0;border-top:1px solid {border}}}li{{margin:.2em 0}}
</style></head><body>{body}</body></html>"#,
        background = css_color(style.background),
        foreground = css_color(style.foreground),
        font = css_string(&style.font),
        size = style.size,
        muted = css_color(style.muted),
        border = css_color(style.border),
        accent = css_color(style.accent)
    ))
}

struct Renderer<'a> {
    path: &'a Path,
    root: Option<&'a Path>,
    files: &'a [PathBuf],
    style: &'a Style,
    bytes: usize,
    definitions: std::collections::HashMap<String, String>,
}

impl Renderer<'_> {
    fn definitions(&mut self, node: &Node) {
        if let Node::Definition(value) = node {
            self.definitions
                .insert(value.identifier.clone(), value.url.clone());
        }
        if let Some(children) = node.children() {
            for child in children {
                self.definitions(child);
            }
        }
    }

    fn children(&mut self, node: &Node, depth: usize) -> Result<String, ExportError> {
        let mut out = String::new();
        if let Some(children) = node.children() {
            for child in children {
                out.push_str(&self.node(child, depth + 1)?);
            }
        }
        Ok(out)
    }

    fn node(&mut self, node: &Node, depth: usize) -> Result<String, ExportError> {
        if depth > 128 {
            return Err(ExportError::TooLarge);
        }
        let wrap = |tag: &str, body: String| format!("<{tag}>{body}</{tag}>");
        Ok(match node {
            Node::Text(v) => self.wiki_text(&v.value)?,
            Node::InlineCode(v) => wrap("code", escape(&v.value)),
            Node::Code(v) => wrap("pre", wrap("code", escape(&v.value))),
            Node::Html(v) => escape(&v.value),
            Node::Yaml(_) | Node::Toml(_) | Node::Definition(_) => String::new(),
            Node::Heading(v) => wrap(&format!("h{}", v.depth), self.children(node, depth)?),
            Node::Paragraph(_) => wrap("p", self.children(node, depth)?),
            Node::Emphasis(_) => wrap("em", self.children(node, depth)?),
            Node::Strong(_) => wrap("strong", self.children(node, depth)?),
            Node::Delete(_) => wrap("del", self.children(node, depth)?),
            Node::Break(_) => "<br>".into(),
            Node::ThematicBreak(_) => "<hr>".into(),
            Node::Blockquote(_) => {
                let mut body = self.children(node, depth)?;
                if let Some(rest) = body.strip_prefix("<p>[!") {
                    if let Some(end) = rest.find(']') {
                        let label = &rest[..end];
                        let tail = rest[end + 1..].trim_start_matches(['+', '-']);
                        body = format!("<p><strong>{label}</strong>{tail}");
                        format!("<blockquote class=\"callout\">{body}</blockquote>")
                    } else {
                        wrap("blockquote", body)
                    }
                } else {
                    wrap("blockquote", body)
                }
            }
            Node::List(v) => {
                if v.ordered {
                    format!(
                        "<ol start=\"{}\">{}</ol>",
                        v.start.unwrap_or(1),
                        self.children(node, depth)?
                    )
                } else {
                    wrap("ul", self.children(node, depth)?)
                }
            }
            Node::ListItem(v) => {
                let mark = v
                    .checked
                    .map(|checked| if checked { "☑ " } else { "☐ " })
                    .unwrap_or("");
                wrap("li", format!("{mark}{}", self.children(node, depth)?))
            }
            Node::Table(v) => {
                let mut body = String::from("<table>");
                for (ix, row) in v.children.iter().enumerate() {
                    body.push_str(if ix == 0 { "<thead><tr>" } else { "<tr>" });
                    if let Some(cells) = row.children() {
                        for cell in cells {
                            body.push_str(&wrap(
                                if ix == 0 { "th" } else { "td" },
                                self.children(cell, depth + 1)?,
                            ));
                        }
                    }
                    body.push_str(if ix == 0 {
                        "</tr></thead><tbody>"
                    } else {
                        "</tr>"
                    });
                }
                body.push_str("</tbody></table>");
                body
            }
            Node::Image(v) => self.image(&v.url, &v.alt)?,
            Node::ImageReference(v) => {
                let url = self
                    .definitions
                    .get(&v.identifier)
                    .cloned()
                    .ok_or(ExportError::Image)?;
                self.image(&url, &v.alt)?
            }
            Node::Link(v) => Self::link(&v.url, self.children(node, depth)?),
            Node::LinkReference(v) => {
                let url = self
                    .definitions
                    .get(&v.identifier)
                    .cloned()
                    .unwrap_or_default();
                Self::link(&url, self.children(node, depth)?)
            }
            Node::Math(v) => format!(
                "<div class=\"display-math\">{}</div>",
                self.math(&v.value, false)?
            ),
            Node::InlineMath(v) => self.math(&v.value, true)?,
            _ => self.children(node, depth)?,
        })
    }

    fn link(url: &str, body: String) -> String {
        if url.starts_with('#')
            || url::Url::parse(url)
                .is_ok_and(|url| matches!(url.scheme(), "http" | "https" | "mailto"))
        {
            format!("<a href=\"{}\">{body}</a>", escape(url))
        } else {
            body
        }
    }

    fn wiki_text(&mut self, text: &str) -> Result<String, ExportError> {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find("![[") {
            out.push_str(&escape(&rest[..start]));
            let Some(end) = rest[start + 3..].find("]]") else {
                out.push_str(&escape(&rest[start..]));
                return Ok(out);
            };
            let inner = &rest[start + 3..start + 3 + end];
            let target = inner.split('|').next().unwrap_or(inner);
            out.push_str(&self.image(target, target)?);
            rest = &rest[start + 3 + end + 2..];
        }
        out.push_str(&escape(rest));
        Ok(out)
    }

    fn image(&mut self, target: &str, alt: &str) -> Result<String, ExportError> {
        if target.starts_with("//") {
            return Err(ExportError::Image);
        }
        if let Ok(url) = url::Url::parse(target) {
            if url.scheme() != "file" {
                return Err(ExportError::Image);
            }
        }
        let base = url::Url::from_directory_path(self.path.parent().ok_or(ExportError::Image)?)
            .map_err(|_| ExportError::Image)?;
        let path = base
            .join(target)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .filter(|p| p.is_file())
            .or_else(|| {
                self.root
                    .map(|root| root.join(target))
                    .filter(|p| p.is_file())
            })
            .or_else(|| {
                let name = Path::new(target)
                    .file_name()?
                    .to_string_lossy()
                    .nfc()
                    .collect::<String>()
                    .to_lowercase();
                self.files
                    .iter()
                    .find(|p| {
                        p.file_name().is_some_and(|n| {
                            n.to_string_lossy().nfc().collect::<String>().to_lowercase() == name
                        })
                    })
                    .cloned()
            })
            .ok_or(ExportError::Image)?;
        let mime = match crate::file_preview::extension(&path).as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => return Err(ExportError::Image),
        };
        let mut bytes = Vec::new();
        let file = std::fs::File::open(path).map_err(|_| ExportError::Image)?;
        if !file.metadata().map_err(|_| ExportError::Image)?.is_file() {
            return Err(ExportError::Image);
        }
        file.take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ExportError::Image)?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(ExportError::TooLarge);
        }
        self.bytes += bytes.len();
        if self.bytes > 64 * 1024 * 1024 {
            return Err(ExportError::TooLarge);
        }
        Ok(format!(
            "<img alt=\"{}\" src=\"data:{mime};base64,{}\">",
            escape(alt),
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ))
    }

    fn math(&mut self, latex: &str, inline: bool) -> Result<String, ExportError> {
        let math = crate::math::render(
            latex,
            self.style.size * crate::math::SCALE,
            2.,
            self.style.foreground,
            inline,
        )
        .map_err(|_| ExportError::Math)?;
        self.bytes += math.image.bytes().len();
        if self.bytes > 64 * 1024 * 1024 {
            return Err(ExportError::TooLarge);
        }
        Ok(format!("<img class=\"math\" alt=\"{}\" width=\"{}\" height=\"{}\" src=\"data:image/png;base64,{}\">", escape(latex), math.width, math.height, base64::engine::general_purpose::STANDARD.encode(math.image.bytes())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn style() -> Style {
        Style {
            font: "SF Mono".into(),
            size: 14.,
            foreground: gpui_kit::rgb(0x222222).into(),
            background: gpui_kit::rgb(0xffffff).into(),
            muted: gpui_kit::rgb(0xeeeeee).into(),
            border: gpui_kit::rgb(0x888888).into(),
            accent: gpui_kit::rgb(0x333333).into(),
        }
    }
    #[test]
    fn pdf_export_preserves_structure_and_math_without_executing_html() {
        let source = "---\ntitle: private metadata\n---\n# Heading\n\n**Strong** and $x^2$.\n\n> [!NOTE] Callout\n> Body\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n<script>alert(1)</script>";
        let out = html(source, Path::new("/vault/Note.md"), None, &[], &style()).unwrap();
        for expected in [
            "<h1>Heading</h1>",
            "<strong>Strong</strong>",
            "<table>",
            "<th>A</th>",
            "<pre><code>",
            "class=\"callout\"",
            "data:image/png;base64,",
            "&lt;script&gt;",
            "default-src 'none'",
        ] {
            assert!(out.contains(expected), "{expected}");
        }
        assert!(!out.contains("private metadata"));
        assert!(!out.contains("<script>"));
    }
    #[test]
    fn pdf_export_rejects_remote_assets_and_invalid_margins() {
        assert_eq!(
            html(
                "![remote](https://example.org/image.png)",
                Path::new("/vault/Note.md"),
                None,
                &[],
                &style()
            ),
            Err(ExportError::Image)
        );
        for margin_mm in [f64::NAN, f64::INFINITY, 0., 51.] {
            assert!(!Options {
                margin_mm,
                ..Default::default()
            }
            .valid());
        }
        assert!(Options::default().valid());
        assert_eq!(Paper::Letter.points(), (612., 792.));
    }

    #[test]
    fn pdf_export_resolves_nested_unicode_image_basenames() {
        let dir =
            std::env::temp_dir().join(format!("rista-pdf-unicode-test-{}", std::process::id()));
        let assets = dir.join("Assets");
        std::fs::create_dir_all(&assets).unwrap();
        let image = assets.join("Bla\u{030a} chart 314.png");
        let png = crate::math::render("x", 14., 1., style().foreground, true).unwrap();
        std::fs::write(&image, png.image.bytes()).unwrap();
        let out = html(
            "![chart](<Assets/Bla\u{030a} chart 314.png>)\n\n![[Blå chart 314.png]]",
            &dir.join("Note.md"),
            Some(&dir),
            &[image],
            &style(),
        )
        .unwrap();
        assert_eq!(out.matches("src=\"data:image/png;base64,").count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pdf_export_embeds_local_images_and_preserves_destination_on_failure() {
        let dir =
            std::env::temp_dir().join(format!("rista-pdf-export-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let image = dir.join("Blå bilde.png");
        let png = crate::math::render("x", 14., 1., style().foreground, true).unwrap();
        std::fs::write(&image, png.image.bytes()).unwrap();
        let note = dir.join("Note.md");
        let out = html(
            "![[Blå bilde.png]]\n\n![alt](Bl%C3%A5%20bilde.png)",
            &note,
            None,
            &[],
            &style(),
        )
        .unwrap();
        assert_eq!(out.matches("src=\"data:image/png;base64,").count(), 2);
        let target = dir.join("Note.pdf");
        std::fs::write(&target, b"original").unwrap();
        assert_eq!(save(&target, b"not PDF"), Err(ExportError::Render));
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        save(&target, b"%PDF-test").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"%PDF-test");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
