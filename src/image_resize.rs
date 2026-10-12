use gpui_kit::component::{tooltip::Tooltip, ActiveTheme};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct ImagePreview {
    document: WeakEntity<crate::document::Document>,
    source: Arc<str>,
    revision: u64,
    spans: Arc<Vec<SourceImage>>,
    views: Arc<Mutex<HashMap<usize, Entity<ResizeImage>>>>,
}

#[derive(Clone)]
pub(crate) struct ResizeAnchor {
    pub(crate) source: Range<usize>,
    pub(crate) original: String,
    pub(crate) revision: u64,
}

impl ResizeAnchor {
    pub(crate) fn replacement(
        &self,
        current: &str,
        revision: u64,
        size: Option<(u32, u32)>,
    ) -> Option<String> {
        if self.revision != revision
            || current.get(self.source.clone()) != Some(self.original.as_str())
        {
            return None;
        }
        ImageSyntax::parse(&self.original)?.resized(&self.original, size)
    }
}

impl ImagePreview {
    pub(crate) fn new(
        document: WeakEntity<crate::document::Document>,
        source: &str,
        revision: u64,
        spans: Vec<SourceImage>,
    ) -> Self {
        Self {
            document,
            source: Arc::from(source),
            revision,
            spans: Arc::new(spans),
            views: Arc::default(),
        }
    }

    pub(crate) fn anchor(&self, preview: Range<usize>, rendered: &str) -> Option<ResizeAnchor> {
        let index = self
            .spans
            .binary_search_by_key(&preview.start, |span| span.preview.start)
            .ok()?;
        let span = &self.spans[index];
        if span.preview != preview || span.rendered != rendered {
            return None;
        }
        Some(ResizeAnchor {
            source: span.source.clone(),
            original: self.source.get(span.source.clone())?.to_string(),
            revision: self.revision,
        })
    }

    pub(crate) fn view(
        &self,
        anchor: ResizeAnchor,
        url: String,
        size: Option<(f32, Option<f32>)>,
        context: &crate::preview::PreviewCtx,
        cx: &mut App,
    ) -> Option<Entity<ResizeImage>> {
        let mut views = self.views.lock().ok()?;
        Some(
            views
                .entry(anchor.source.start)
                .or_insert_with(|| {
                    cx.new(|cx| ResizeImage {
                        document: self.document.clone(),
                        workspace: context.workspace.clone(),
                        anchor,
                        url,
                        size,
                        local_size: None,
                        drag: None,
                        bounds: std::rc::Rc::new(std::cell::Cell::new(Bounds::default())),
                        focus: cx.focus_handle(),
                        language: context.language,
                    })
                })
                .clone(),
        )
    }
}

struct Drag {
    origin: Point<Pixels>,
    initial: (f32, f32),
    moved: bool,
}

pub(crate) struct ResizeImage {
    document: WeakEntity<crate::document::Document>,
    workspace: WeakEntity<crate::app::Workspace>,
    anchor: ResizeAnchor,
    url: String,
    size: Option<(f32, Option<f32>)>,
    local_size: Option<(u32, u32)>,
    drag: Option<Drag>,
    bounds: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
    language: crate::settings::Language,
}

impl ResizeImage {
    fn commit(&mut self, size: Option<(u32, u32)>, window: &mut Window, cx: &mut Context<Self>) {
        let applied = self
            .document
            .update(cx, |doc, cx| {
                doc.resize_image(&self.anchor, size, window, cx)
            })
            .unwrap_or(false);
        if !applied {
            self.local_size = None;
            let message = self.language.text(
                "Image changed; resize cancelled.",
                "Bildet er endret; skalering avbrutt.",
            );
            let _ = self
                .workspace
                .update(cx, |workspace, cx| workspace.note_status(message, cx));
        }
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(drag) = self.drag.take() {
            if drag.moved {
                self.commit(self.local_size, window, cx);
            }
            cx.notify();
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.drag = None;
        self.local_size = None;
        cx.notify();
    }
}

impl Render for ResizeImage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = self.bounds.clone();
        let view = cx.weak_entity();
        let size = self
            .local_size
            .map(|(w, h)| (w as f32, Some(h as f32)))
            .or(self.size);
        let hint = match self.language {
            crate::settings::Language::Norwegian => {
                "Dra for å endre størrelse. Skift: fri størrelse. Dobbeltklikk: nullstill."
            }
            _ => "Drag to resize. Shift: free resize. Double-click: reset.",
        };
        div()
            .id("resizable-image")
            .track_focus(&self.focus)
            .relative()
            .max_w_full()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish(window, cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.drag.is_some() {
                    this.cancel(cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                img(crate::preview::image_source(&self.url))
                    .max_w_full()
                    .when_some(size, |image, (width, height)| {
                        image.w(px(width)).when_some(height, |image, height| {
                            image
                                .aspect_ratio(width / height)
                                .object_fit(ObjectFit::Cover)
                        })
                    }),
            )
            .child(
                canvas(
                    move |bounds_value, _, _| {
                        bounds.set(bounds_value);
                    },
                    move |_, _, window, _| {
                        let view = view.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            let _ = view.update(cx, |this, cx| {
                                let Some(drag) = &mut this.drag else {
                                    return;
                                };
                                if !event.dragging() {
                                    this.cancel(cx);
                                    return;
                                }
                                let dx = f32::from(event.position.x - drag.origin.x);
                                let dy = f32::from(event.position.y - drag.origin.y);
                                if dx.abs() + dy.abs() < 2. && !drag.moved {
                                    return;
                                }
                                drag.moved = true;
                                this.local_size = Some(dragged_size(
                                    drag.initial,
                                    (dx, dy),
                                    event.modifiers.shift,
                                ));
                                cx.notify();
                                cx.stop_propagation();
                            });
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .id("image-resize-corner")
                    .absolute()
                    .bottom_0()
                    .right_0()
                    .size(px(12.))
                    .cursor(CursorStyle::ResizeUpLeftDownRight)
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().muted_foreground)
                    .rounded(px(2.))
                    .aria_label(hint)
                    .tooltip(move |window, cx| Tooltip::new(hint).build(window, cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.focus.focus(window, cx);
                            if event.click_count == 2 {
                                this.drag = None;
                                this.local_size = None;
                                this.commit(None, window, cx);
                                return;
                            }
                            let bounds = this.bounds.get();
                            let initial =
                                (f32::from(bounds.size.width), f32::from(bounds.size.height));
                            if initial.0 > 0. && initial.1 > 0. {
                                this.drag = Some(Drag {
                                    origin: event.position,
                                    initial,
                                    moved: false,
                                });
                                cx.notify();
                            }
                        }),
                    ),
            )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SourceImage {
    pub(crate) source: Range<usize>,
    pub(crate) preview: Range<usize>,
    pub(crate) rendered: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ImageSyntax {
    pub(crate) len: usize,
    label: Range<usize>,
}

impl ImageSyntax {
    pub(crate) fn parse(source: &str) -> Option<Self> {
        if let Some(rest) = source.strip_prefix("![[") {
            let end = rest.find("]]")? + 3;
            return (end > 3).then_some(Self {
                len: end + 2,
                label: 3..end,
            });
        }
        if !source.starts_with("![") {
            return None;
        }
        let mut depth = 1;
        let mut escaped = false;
        let mut label_end = None;
        for (offset, ch) in source[2..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        label_end = Some(offset + 2);
                        break;
                    }
                }
                _ => {}
            }
        }
        let end = label_end?;
        if source.as_bytes().get(end + 1) != Some(&b'(') {
            return None;
        }
        let mut depth = 1;
        let mut quote = None;
        let mut angle = false;
        let mut escaped = false;
        for (offset, ch) in source[end + 2..].char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if let Some(quote_ch) = quote {
                if ch == quote_ch {
                    quote = None;
                }
            } else if angle {
                if ch == '>' {
                    angle = false;
                }
            } else {
                match ch {
                    '<' => angle = true,
                    '\'' | '"' if source[..end + 2 + offset].ends_with(char::is_whitespace) => {
                        quote = Some(ch)
                    }
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(Self {
                                len: end + 3 + offset,
                                label: 2..end,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn suffix_start(&self, source: &str) -> Option<usize> {
        let label = source.get(self.label.clone())?;
        let (prefix, suffix) = label.rsplit_once('|')?;
        let at = self.label.start + prefix.len();
        let backslashes = source[..at]
            .chars()
            .rev()
            .take_while(|c| *c == '\\')
            .count();
        (backslashes % 2 == 0 && parse_size(suffix).is_some()).then_some(at)
    }

    pub(crate) fn label<'a>(&self, source: &'a str) -> Option<&'a str> {
        source.get(self.label.clone())
    }

    pub(crate) fn size(&self, source: &str) -> Option<(f32, Option<f32>)> {
        parse_size(source.get(self.suffix_start(source)? + 1..self.label.end)?)
    }

    pub(crate) fn resized(&self, source: &str, size: Option<(u32, u32)>) -> Option<String> {
        let original = source.get(..self.len)?;
        let start = self.suffix_start(original).unwrap_or(self.label.end);
        let replacement = match size {
            Some((width, height)) => format!("|{width}x{height}"),
            None => String::new(),
        };
        Some(format!(
            "{}{}{}",
            &original[..start],
            replacement,
            &original[self.label.end..]
        ))
    }
}

pub(crate) fn parse_size(value: &str) -> Option<(f32, Option<f32>)> {
    let (width, height) = value
        .trim()
        .split_once('x')
        .map_or((value.trim(), None), |(w, h)| (w.trim(), Some(h.trim())));
    let width: f32 = width.parse().ok()?;
    let height = height.map(str::parse::<f32>).transpose().ok()?;
    (width.is_finite() && width > 0. && height.is_none_or(|h| h.is_finite() && h > 0.))
        .then_some((width, height))
}

pub(crate) fn dragged_size(initial: (f32, f32), delta: (f32, f32), free: bool) -> (u32, u32) {
    if free {
        return (
            (initial.0 + delta.0).clamp(16., 8192.).round() as u32,
            (initial.1 + delta.1).clamp(16., 8192.).round() as u32,
        );
    }
    let ratio = initial.0.max(1.) / initial.1.max(1.);
    let maximum = 8192_f32.min(8192. * ratio);
    let minimum = 16_f32.max(16. * ratio).min(maximum);
    let width = (initial.0 + delta.0).clamp(minimum, maximum);
    (
        width.round().max(1.) as u32,
        (width / ratio).round().max(1.) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::{dragged_size, parse_size, ImageSyntax, ResizeAnchor};

    #[test]
    fn image_resize_round_trips_without_touching_targets_or_titles() {
        for original in [
            "![[Bilder/ø.png]]",
            "![ø](<Bilder/my photo.png> \"title )\")",
            "![a [nested] label](pic(1).png)",
        ] {
            let syntax = ImageSyntax::parse(original).unwrap();
            assert_eq!(syntax.len, original.len());
            let resized = syntax.resized(original, Some((320, 180))).unwrap();
            assert!(resized.contains("|320x180"));
            assert_eq!(
                ImageSyntax::parse(&resized)
                    .unwrap()
                    .resized(&resized, None)
                    .unwrap(),
                original
            );
        }
    }

    #[test]
    fn image_resize_replaces_only_numeric_suffix() {
        for (original, expected) in [
            ("![[pic.png|200]]", "![[pic.png|320x180]]"),
            ("![caption|200x100](pic.png)", "![caption|320x180](pic.png)"),
            ("![[pic.png|caption]]", "![[pic.png|caption|320x180]]"),
            (
                "![literal\\|200](pic.png)",
                "![literal\\|200|320x180](pic.png)",
            ),
        ] {
            assert_eq!(
                ImageSyntax::parse(original)
                    .unwrap()
                    .resized(original, Some((320, 180)))
                    .unwrap(),
                expected
            );
        }
        assert_eq!(parse_size("inf"), None);
        assert_eq!(parse_size("200xNaN"), None);
    }

    #[test]
    fn image_resize_drag_locks_ratio_unless_shift_is_held() {
        assert_eq!(dragged_size((200., 100.), (100., 200.), false), (300, 150));
        assert_eq!(dragged_size((200., 100.), (100., 200.), true), (300, 300));
        assert_eq!(dragged_size((200., 100.), (-1000., -1000.), true), (16, 16));
        assert_eq!(
            dragged_size((200., 400.), (10000., 0.), false),
            (4096, 8192)
        );
    }

    #[test]
    fn image_resize_stale_anchor_never_rewrites_another_occurrence() {
        let text = "ø ![[p.png]] ![[p.png]]";
        let start = text.rfind("![[").unwrap();
        let anchor = ResizeAnchor {
            source: start..text.len(),
            original: text[start..].into(),
            revision: 7,
        };
        assert_eq!(
            anchor.replacement(text, 7, Some((320, 180))).unwrap(),
            "![[p.png|320x180]]"
        );
        assert!(anchor.replacement(text, 8, Some((320, 180))).is_none());
        assert!(anchor.replacement("changed", 7, Some((320, 180))).is_none());
    }
}
