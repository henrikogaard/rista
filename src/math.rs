//! LaTeX math for the preview — KaTeX-compatible typesetting by RaTeX,
//! rendered offline (the KaTeX fonts are embedded) to PNG and cached.

use gpui_kit::{Hsla, Image, ImageFormat};
use ratex_layout::{layout, to_display_list, LayoutOptions};
use ratex_render::{render_to_png, RenderOptions};
use ratex_types::color::Color;
use ratex_types::math_style::MathStyle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// A typeset expression. Sizes are logical pixels.
pub struct MathImage {
    pub image: Arc<Image>,
    pub width: f32,
    pub height: f32,
    /// Distance from the top edge to the baseline.
    pub baseline: f32,
}

type Key = (String, u32, u32, u32, bool);
type Cache = Mutex<HashMap<Key, Result<Arc<MathImage>, String>>>;

/// Math size relative to the surrounding text — KaTeX's 1.21em.
pub const SCALE: f32 = 1.21;

/// Space around the glyphs, in logical pixels.
const PAD: f32 = 2.0;

/// Typeset `latex` at `font_px` in `color` — `inline` for `$…$`
/// (text style), else display style for `$$…$$`. Errors carry the
/// parser's message; results are cached per source, size, color and
/// scale.
pub fn render(
    latex: &str,
    font_px: f32,
    scale: f32,
    color: Hsla,
    inline: bool,
) -> Result<Arc<MathImage>, String> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let rgb = color.to_rgb();
    let packed = u32::from_be_bytes([
        (rgb.r * 255.0) as u8,
        (rgb.g * 255.0) as u8,
        (rgb.b * 255.0) as u8,
        0,
    ]);
    let key = (
        latex.to_string(),
        font_px.to_bits(),
        scale.to_bits(),
        packed,
        inline,
    );
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return hit;
    }
    let result = typeset(latex, font_px, scale, [rgb.r, rgb.g, rgb.b], inline).map(Arc::new);
    if let Ok(mut cache) = cache.lock() {
        // Plenty for a session of editing; cheap to rebuild.
        if cache.len() > 2048 {
            cache.clear();
        }
        cache.insert(key, result.clone());
    }
    result
}

fn typeset(
    latex: &str,
    font_px: f32,
    scale: f32,
    rgb: [f32; 3],
    inline: bool,
) -> Result<MathImage, String> {
    let nodes = ratex_parser::parse(latex).map_err(|e| e.message)?;
    let boxed = layout(
        &nodes,
        &LayoutOptions {
            style: if inline {
                MathStyle::Text
            } else {
                MathStyle::Display
            },
            color: Color::rgb(rgb[0], rgb[1], rgb[2]),
            ..Default::default()
        },
    );
    let list = to_display_list(&boxed);
    if list.width <= 0.0 {
        return Err("Empty expression".into());
    }
    let png = render_to_png(
        &list,
        &RenderOptions {
            font_size: font_px,
            padding: PAD,
            device_pixel_ratio: scale,
            background_color: Color::new(0.0, 0.0, 0.0, 0.0),
            ..Default::default()
        },
    )?;
    Ok(MathImage {
        image: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
        width: list.width as f32 * font_px + 2.0 * PAD,
        height: (list.height + list.depth) as f32 * font_px + 2.0 * PAD,
        baseline: list.height as f32 * font_px + PAD,
    })
}

#[cfg(test)]
mod tests {
    use super::render;

    fn white() -> gpui_kit::Hsla {
        gpui_kit::rgb(0xffffff).into()
    }

    #[test]
    fn typesets_display_and_inline_math() {
        let display = render(
            r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}",
            16.0,
            2.0,
            white(),
            false,
        )
        .unwrap();
        let inline = render(r"\sum_{i=1}^{n} i", 16.0, 2.0, white(), true).unwrap();
        assert!(display.width > 0.0 && display.height > inline.height);
        assert!(inline.baseline > 0.0 && inline.baseline < inline.height);
        // Cached: the same source and style returns the same image.
        let again = render(r"\sum_{i=1}^{n} i", 16.0, 2.0, white(), true).unwrap();
        assert!(std::sync::Arc::ptr_eq(&inline, &again));
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        assert!(render(r"\frac{a", 16.0, 1.0, white(), true).is_err());
        assert!(render(r"\unknowncmd", 16.0, 1.0, white(), true).is_err());
        assert!(render("", 16.0, 1.0, white(), true).is_err());
    }
}
