//! Mermaid diagrams for the preview — merman parses, lays out and draws
//! SVG in pure Rust; resvg rasterizes it with the system fonts. Offline.
//! Rendering runs on the background executor; the preview shows the
//! source until the diagram is ready, then refreshes.

use gpui_kit::component::theme::ThemeColor;
use gpui_kit::{App, Hsla, Image, ImageFormat};
use merman::svg::{
    CssOverridePolicy, HostTheme, HostThemeAppearance, Presentation, SvgOutputPolicy,
    SvgPipelinePreset, ThemeRole,
};
use merman::{Engine, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

/// A rendered diagram. Sizes are logical pixels.
pub struct DiagramImage {
    pub image: Arc<Image>,
    pub width: f32,
    pub height: f32,
}

pub enum Diagram {
    Ready(Arc<DiagramImage>),
    Failed(String),
    /// Still rendering — the preview refreshes when it's done.
    Pending,
}

/// Theme colors as the hex strings merman takes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiagramTheme {
    dark: bool,
    canvas: String,
    surface: String,
    surface_alt: String,
    text: String,
    border: String,
    line: String,
    note_background: String,
    note_border: String,
    series: [String; 6],
}

impl DiagramTheme {
    /// The diagram sits on the preview surface (`group_box`); nodes use
    /// the muted fill, lines the muted text color.
    pub fn from_theme(theme: &ThemeColor, dark: bool) -> Self {
        Self {
            dark,
            canvas: hex(theme.group_box),
            surface: hex(theme.muted),
            surface_alt: hex(theme.border),
            text: hex(theme.foreground),
            border: hex(theme.muted_foreground),
            line: hex(theme.muted_foreground),
            note_background: hex(mix(theme.warning, theme.group_box, 0.25)),
            note_border: hex(theme.warning),
            series: [
                hex(theme.blue),
                hex(theme.green),
                hex(theme.yellow),
                hex(theme.magenta),
                hex(theme.cyan),
                hex(theme.red),
            ],
        }
    }
}

fn hex(color: Hsla) -> String {
    let c = color.to_rgb();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}

/// `amount` of `a` over `b`, opaque.
fn mix(a: Hsla, b: Hsla, amount: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let lerp = |x: f32, y: f32| x * amount + y * (1.0 - amount);
    gpui_kit::Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: 1.0,
    }
    .into()
}

type Key = (String, DiagramTheme, u32);

#[derive(Default)]
struct Cache {
    done: HashMap<Key, Result<Arc<DiagramImage>, String>>,
    /// `done` keys oldest first — the oldest go once it's full.
    order: VecDeque<Key>,
    pending: HashSet<Key>,
}

/// Rendered diagrams kept at once.
const CACHE_SIZE: usize = 256;

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The diagram for `source`, starting a background render on a miss.
pub fn diagram(source: &str, theme: &DiagramTheme, scale: f32, cx: &mut App) -> Diagram {
    let key: Key = (source.to_string(), theme.clone(), scale.to_bits());
    {
        let Ok(mut cache) = cache().lock() else {
            return Diagram::Failed("Diagram cache unavailable".into());
        };
        match cache.done.get(&key) {
            Some(Ok(image)) => return Diagram::Ready(image.clone()),
            Some(Err(error)) => return Diagram::Failed(error.clone()),
            None => {}
        }
        if !cache.pending.insert(key.clone()) {
            return Diagram::Pending;
        }
    }
    let task = cx.background_executor().spawn({
        let (source, theme) = (source.to_string(), theme.clone());
        async move { render(&source, &theme, scale).map(Arc::new) }
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        if let Ok(mut cache) = cache().lock() {
            cache.pending.remove(&key);
            while cache.done.len() >= CACHE_SIZE {
                let Some(oldest) = cache.order.pop_front() else {
                    break;
                };
                cache.done.remove(&oldest);
            }
            cache.order.push_back(key.clone());
            cache.done.insert(key, result);
        }
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
    Diagram::Pending
}

/// Mermaid source → PNG at `scale` (device pixels per logical pixel).
pub fn render(source: &str, theme: &DiagramTheme, scale: f32) -> Result<DiagramImage, String> {
    let svg = svg(source, theme)?;
    let options = resvg::usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
    let (width, height) = (tree.size().width(), tree.size().height());
    let scale = raster_scale(width, height, scale)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(
        (width * scale).ceil().max(1.0) as u32,
        (height * scale).ceil().max(1.0) as u32,
    )
    .ok_or("Diagram too large")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png().map_err(|e| e.to_string())?;
    Ok(DiagramImage {
        image: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
        width,
        height,
    })
}

/// Longest side and pixel count a diagram rasterizes to.
const MAX_SIDE: f32 = 8192.0;
const MAX_PIXELS: f32 = 24_000_000.0;

/// `scale`, lowered so a `width`×`height` diagram stays within
/// `MAX_SIDE` and `MAX_PIXELS` — the pixmap can't report a failed
/// allocation, so huge diagrams must not reach it.
fn raster_scale(width: f32, height: f32, scale: f32) -> Result<f32, String> {
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return Err("Diagram has no size".into());
    }
    let fit = scale
        .min(MAX_SIDE / width)
        .min(MAX_SIDE / height)
        .min((MAX_PIXELS / (width * height)).sqrt());
    // Below a quarter scale the text is unreadable anyway.
    if fit < 0.25 {
        return Err(format!(
            "Diagram too large to draw ({}×{} px)",
            width.round(),
            height.round()
        ));
    }
    Ok(fit)
}

/// The system font database, loaded once.
fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn svg(source: &str, theme: &DiagramTheme) -> Result<String, String> {
    let host = HostTheme::new()
        .with_appearance(if theme.dark {
            HostThemeAppearance::Dark
        } else {
            HostThemeAppearance::Light
        })
        .try_with_font_family("Helvetica Neue, Helvetica, Arial, sans-serif")
        .and_then(|t| t.try_with_role(ThemeRole::Canvas, &theme.canvas))
        .and_then(|t| t.try_with_role(ThemeRole::Surface, &theme.surface))
        .and_then(|t| t.try_with_role(ThemeRole::SurfaceAlt, &theme.surface_alt))
        .and_then(|t| t.try_with_role(ThemeRole::Text, &theme.text))
        .and_then(|t| t.try_with_role(ThemeRole::Border, &theme.border))
        .and_then(|t| t.try_with_role(ThemeRole::Line, &theme.line))
        .and_then(|t| t.try_with_role(ThemeRole::NoteBackground, &theme.note_background))
        .and_then(|t| t.try_with_role(ThemeRole::NoteBorder, &theme.note_border))
        .and_then(|t| t.try_with_role(ThemeRole::NoteText, &theme.text))
        .and_then(|t| t.try_with_series_palette(theme.series.clone()))
        .map_err(|e| e.to_string())?;
    let output = SvgOutputPolicy {
        // resvg can't draw the HTML labels the parity preset emits.
        preset: SvgPipelinePreset::ResvgSafe,
        css_override_policy: CssOverridePolicy::StripExistingImportant,
        // merman paints white when this is unset; use the preview surface.
        root_background_color: Some(theme.canvas.clone()),
        ..SvgOutputPolicy::default()
    };
    let resolved = Presentation::new().with_theme(host).resolve();
    let renderer = Renderer::new().with_engine(resolved.materialize_engine(Engine::new()));
    let request = SvgRequest {
        pipeline: Some(output.pipeline()),
        presentation: resolved.render_policy(),
        ..Default::default()
    };
    match renderer
        .render(RenderRequest::svg(source, OperationControl::new(), request))
        .map_err(|e| e.to_string())?
    {
        RenderOutput::Svg(Some(svg)) => Ok(svg.svg().to_string()),
        _ => Err("No Mermaid diagram found".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{raster_scale, render, DiagramTheme};

    fn theme() -> DiagramTheme {
        let theme = gpui_kit::component::theme::ThemeColor {
            group_box: gpui_kit::rgb(0x1e1e20).into(),
            foreground: gpui_kit::rgb(0xe5e5ea).into(),
            ..Default::default()
        };
        DiagramTheme::from_theme(&theme, true)
    }

    #[test]
    fn renders_common_diagrams() {
        for source in [
            "graph TD; A[Start]-->B{Ok?}; B-->|yes|C[Done]; B-->|no|D[Retry]",
            "sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi",
            "pie title Pets\n  \"Dogs\" : 79\n  \"Cats\" : 17",
        ] {
            let image = render(source, &theme(), 1.0).unwrap_or_else(|e| panic!("{source}: {e}"));
            assert!(image.width > 10.0 && image.height > 10.0);
        }
    }

    #[test]
    fn bad_syntax_is_an_error() {
        assert!(render("graph TD; A[Start", &theme(), 1.0).is_err());
        assert!(render("not a diagram", &theme(), 1.0).is_err());
    }

    #[test]
    fn huge_diagrams_are_scaled_down_or_refused() {
        assert_eq!(raster_scale(800.0, 600.0, 2.0), Ok(2.0));
        let fit = raster_scale(20_000.0, 400.0, 2.0).unwrap();
        assert!(20_000.0 * fit <= 8192.0);
        let fit = raster_scale(6000.0, 6000.0, 2.0).unwrap();
        assert!(6000.0 * fit * 6000.0 * fit <= 24_000_000.0);
        assert!(raster_scale(100_000.0, 100_000.0, 2.0).is_err());
        assert!(raster_scale(0.0, 10.0, 2.0).is_err());
    }
}
