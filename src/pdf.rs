use crate::{pdf_backend as backend, settings::Language};
use gpui_kit::{
    base::ElementExt,
    component::{
        button::{Button, ButtonVariants},
        h_flex,
        input::{self, Input, InputEvent, InputState},
        v_flex, ActiveTheme, Disableable, Sizable,
    },
    prelude::FluentBuilder,
    *,
};
use std::{
    cell::Cell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

type RasterKey = (usize, u32);

fn top_page_index(scroll: &UniformListScrollHandle) -> usize {
    let state = scroll.0.borrow();
    state.deferred_scroll_to_item.as_ref().map_or_else(
        || state.base_handle.top_item(),
        |deferred| deferred.item_index,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PdfTarget {
    pub file: String,
    pub page: usize,
    pub height: u32,
}

impl PdfTarget {
    pub fn parse(target: &str) -> Option<Self> {
        let target = target.split('|').next()?.trim();
        let (file, options) = target.split_once('#').unwrap_or((target, ""));
        if crate::file_preview::extension(Path::new(file)) != "pdf" || file.contains("://") {
            return None;
        }
        let mut result = Self {
            file: file.to_string(),
            page: 0,
            height: 400,
        };
        for option in options.split(['&', '#']) {
            if let Some((name, value)) = option.split_once('=') {
                if let Ok(value) = value.parse::<u32>() {
                    match name {
                        "page" => result.page = value.saturating_sub(1) as usize,
                        "height" => result.height = value.clamp(120, 1600),
                        _ => {}
                    }
                }
            }
        }
        Some(result)
    }

    pub fn resolve(
        &self,
        document: Option<&Path>,
        root: Option<&Path>,
        files: &[PathBuf],
    ) -> Option<PathBuf> {
        if let Some(path) = document
            .and_then(Path::parent)
            .map(|parent| parent.join(&self.file))
            .filter(|path| path.is_file())
        {
            return Some(path);
        }
        if let Some(path) = root
            .map(|root| root.join(&self.file))
            .filter(|path| path.is_file())
        {
            return Some(path);
        }
        let name = Path::new(&self.file)
            .file_name()?
            .to_string_lossy()
            .to_lowercase();
        files
            .iter()
            .find(|path| {
                path.file_name()
                    .is_some_and(|file| file.to_string_lossy().to_lowercase() == name)
            })
            .cloned()
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-c", input::Copy, Some("PdfViewer"))]);
}

pub struct PdfView {
    path: PathBuf,
    file_stamp: Option<(u64, std::time::SystemTime)>,
    language: Language,
    info: Option<backend::PdfInfo>,
    error: Option<backend::PdfError>,
    generation: u64,
    zoom: f32,
    width: f32,
    initial_page: usize,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    page_input: Entity<InputState>,
    search_input: Entity<InputState>,
    search_open: bool,
    search_busy: bool,
    search_results: backend::SearchResults,
    search_index: usize,
    search_epoch: u64,
    selected: Option<(usize, backend::Selection)>,
    selection_epoch: u64,
    drag: Option<(usize, [f64; 2])>,
    images: HashMap<RasterKey, Result<Arc<Image>, backend::PdfError>>,
    raster_tasks: HashMap<RasterKey, Task<()>>,
    load_task: Option<Task<()>>,
    search_task: Option<Task<()>>,
    selection_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PdfView {
    pub fn new(
        path: PathBuf,
        page: usize,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let page_input = cx.new(|cx| InputState::new(window, cx).placeholder("1"));
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text("Find in PDF", "Finn i PDF"))
        });
        let subscriptions = vec![
            cx.subscribe_in(&page_input, window, |this, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    if let Ok(page) = input.read(cx).value().trim().parse::<usize>() {
                        this.jump(page.saturating_sub(1), cx);
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                        this.focus.focus(window, cx);
                    }
                }
            }),
            cx.subscribe_in(&search_input, window, |this, _, event, _, cx| match event {
                InputEvent::PressEnter { .. } => this.search(cx),
                InputEvent::Change => {
                    this.search_epoch = this.search_epoch.wrapping_add(1);
                    this.search_task = None;
                    this.search_results = backend::SearchResults::default();
                    this.search_busy = false;
                    cx.notify();
                }
                _ => {}
            }),
        ];
        let mut view = Self {
            path,
            file_stamp: None,
            language,
            info: None,
            error: None,
            generation: 0,
            zoom: 1.,
            width: 640.,
            initial_page: page,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            page_input,
            search_input,
            search_open: false,
            search_busy: false,
            search_results: backend::SearchResults::default(),
            search_index: 0,
            search_epoch: 0,
            selected: None,
            selection_epoch: 0,
            drag: None,
            images: HashMap::new(),
            raster_tasks: HashMap::new(),
            load_task: None,
            search_task: None,
            selection_task: None,
            _subscriptions: subscriptions,
        };
        view.reload(cx);
        view
    }

    pub fn set_language(
        &mut self,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.language != language {
            self.language = language;
            self.search_input.update(cx, |input, cx| {
                input.set_placeholder(language.text("Find in PDF", "Finn i PDF"), window, cx)
            });
            cx.notify();
        }
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.file_stamp = file_stamp(&self.path);
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.info = None;
        self.error = None;
        self.images.clear();
        self.raster_tasks.clear();
        self.search_task = None;
        self.selection_task = None;
        self.search_epoch = self.search_epoch.wrapping_add(1);
        self.selection_epoch = self.selection_epoch.wrapping_add(1);
        self.search_busy = false;
        self.search_results = backend::SearchResults::default();
        self.selected = None;
        self.drag = None;
        let response = backend::inspect(self.path.clone());
        self.load_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = response
                .recv()
                .await
                .unwrap_or(Err(backend::PdfError::Unavailable));
            let _ = this.update(&mut *cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Ok(info) => {
                        this.info = Some(info);
                        this.jump(this.initial_page, cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn set_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if self.path != path {
            self.path = path;
            self.reload(cx);
        }
    }

    pub fn check_external(&mut self, cx: &mut Context<Self>) {
        if file_stamp(&self.path) != self.file_stamp {
            self.reload(cx);
        }
    }

    pub fn jump(&mut self, page: usize, cx: &mut Context<Self>) {
        self.initial_page = page;
        if let Some(info) = &self.info {
            self.scroll.scroll_to_item(
                page.min(info.pages.len().saturating_sub(1)),
                ScrollStrategy::Top,
            );
        }
        cx.notify();
    }

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = true;
        let input = self.search_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx))
        });
        cx.notify();
    }

    fn search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().trim().to_string();
        if query.is_empty() || self.info.is_none() {
            return;
        }
        self.search_epoch = self.search_epoch.wrapping_add(1);
        let epoch = self.search_epoch;
        self.search_busy = true;
        self.selected = None;
        let response = backend::search(self.path.clone(), query);
        self.search_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = response
                .recv()
                .await
                .unwrap_or(Err(backend::PdfError::Unavailable));
            let _ = this.update(&mut *cx, |this, cx| {
                if this.search_epoch != epoch {
                    return;
                }
                this.search_busy = false;
                match result {
                    Ok(results) => {
                        this.search_results = results;
                        this.search_index = 0;
                        this.go_to_match(cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn go_to_match(&mut self, cx: &mut Context<Self>) {
        if let Some(found) = self.search_results.matches.get(self.search_index) {
            self.jump(found.page, cx);
        }
    }

    fn copy(&self, cx: &mut Context<Self>) {
        if self.info.as_ref().is_some_and(|info| info.can_copy) {
            if let Some((_, selection)) = &self.selected {
                if !selection.text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(selection.text.clone()));
                }
            }
        }
    }

    fn select(&mut self, page: usize, from: [f64; 2], to: [f64; 2], cx: &mut Context<Self>) {
        self.selection_epoch = self.selection_epoch.wrapping_add(1);
        let epoch = self.selection_epoch;
        let response = backend::select(self.path.clone(), page, from, to);
        self.selection_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = response
                .recv()
                .await
                .unwrap_or(Err(backend::PdfError::Unavailable));
            let _ = this.update(&mut *cx, |this, cx| {
                if this.selection_epoch != epoch {
                    return;
                }
                match result {
                    Ok(selection) => this.selected = Some((page, selection)),
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
    }

    fn request_page(&mut self, page: usize, width: u32, cx: &mut Context<Self>) {
        let key = (page, width);
        if self.images.contains_key(&key)
            || self.raster_tasks.contains_key(&key)
            || self.raster_tasks.len() >= 8
        {
            return;
        }
        let generation = self.generation;
        let response = backend::render_page(self.path.clone(), page, width);
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = response
                .recv()
                .await
                .unwrap_or(Err(backend::PdfError::Unavailable));
            let _ = this.update(&mut *cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.raster_tasks.remove(&key);
                if this.images.len() >= 8 {
                    let current = top_page_index(&this.scroll);
                    if let Some(farthest) = this
                        .images
                        .keys()
                        .max_by_key(|(page, _)| page.abs_diff(current))
                        .copied()
                    {
                        this.images.remove(&farthest);
                    }
                }
                this.images.insert(
                    key,
                    result.map(|image| Arc::new(Image::from_bytes(ImageFormat::Png, image.png))),
                );
                cx.notify();
            });
        });
        self.raster_tasks.insert(key, task);
    }

    fn render_page(
        &mut self,
        page: usize,
        row_height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dimensions = self.info.as_ref().unwrap().pages[page];
        let width = (self.width - 24.).max(80.) * self.zoom;
        let height = width * (dimensions.height / dimensions.width) as f32;
        let raster_width = (width * window.scale_factor()).round().clamp(64., 2048.) as u32;
        self.request_page(page, raster_width, cx);
        let image = self.images.get(&(page, raster_width)).cloned();
        let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let rectangles = self
            .selected
            .as_ref()
            .filter(|(p, _)| *p == page)
            .map(|(_, s)| s.rects.clone())
            .or_else(|| {
                self.search_results
                    .matches
                    .get(self.search_index)
                    .filter(|m| m.page == page)
                    .map(|m| m.rects.clone())
            })
            .unwrap_or_default();
        let content = div()
            .id(("pdf-page", page))
            .relative()
            .w(px(width))
            .h(px(height))
            .on_prepaint({
                let bounds = bounds.clone();
                move |rect, _, _| bounds.set(rect)
            })
            .on_mouse_down(MouseButton::Left, {
                let bounds = bounds.clone();
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window, cx);
                    this.selection_task = None;
                    this.selection_epoch = this.selection_epoch.wrapping_add(1);
                    this.selected = None;
                    this.drag = Some((page, normalized(event.position, bounds.get())));
                    cx.stop_propagation();
                    cx.notify();
                })
            })
            .on_mouse_up(MouseButton::Left, {
                let bounds = bounds.clone();
                cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                    if let Some((start_page, start)) = this.drag.take() {
                        if start_page == page {
                            this.select(page, start, normalized(event.position, bounds.get()), cx);
                        }
                    }
                    cx.stop_propagation();
                })
            })
            .on_mouse_move({
                let bounds = bounds.clone();
                cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        if let Some((start_page, start)) = this.drag {
                            if start_page == page {
                                this.select(
                                    page,
                                    start,
                                    normalized(event.position, bounds.get()),
                                    cx,
                                );
                            }
                        }
                    }
                })
            })
            .cursor_text()
            .child(match image {
                Some(Ok(image)) => img(image).size_full().into_any_element(),
                Some(Err(error)) => div()
                    .p_4()
                    .child(error_message(error, self.language))
                    .into_any_element(),
                None => div()
                    .p_4()
                    .child(self.language.text("Loading page…", "Laster side…"))
                    .into_any_element(),
            })
            .children(rectangles.into_iter().map(|rect| {
                div()
                    .absolute()
                    .left(px(rect.x as f32 * width))
                    .top(px(rect.y as f32 * height))
                    .w(px(rect.width as f32 * width))
                    .h(px(rect.height as f32 * height))
                    .bg(cx.theme().selection.opacity(0.4))
            }));
        v_flex()
            .w_full()
            .h(px(row_height))
            .items_center()
            .pt_2()
            .child(content)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{}", page + 1)),
            )
            .into_any_element()
    }
}

impl Focusable for PdfView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PdfView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let count = self.info.as_ref().map(|info| info.pages.len()).unwrap_or(0);
        let current = top_page_index(&self.scroll).min(count.saturating_sub(1));
        let result_count = self.search_results.matches.len();
        let toolbar = h_flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_1()
            .child(
                Button::new("pdf-prev")
                    .ghost()
                    .small()
                    .label("‹")
                    .tooltip(language.text("Previous page", "Forrige side"))
                    .disabled(current == 0)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.jump(current.saturating_sub(1), cx)),
                    ),
            )
            .child(Input::new(&self.page_input).w(px(48.)))
            .child(div().text_xs().child(format!("{} / {count}", current + 1)))
            .child(
                Button::new("pdf-next")
                    .ghost()
                    .small()
                    .label("›")
                    .tooltip(language.text("Next page", "Neste side"))
                    .disabled(current + 1 >= count)
                    .on_click(cx.listener(move |this, _, _, cx| this.jump(current + 1, cx))),
            )
            .child(
                Button::new("pdf-zoom-out")
                    .ghost()
                    .small()
                    .label("−")
                    .tooltip(language.text("Zoom out", "Zoom ut"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = (this.zoom / 1.25).max(0.5);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("pdf-fit")
                    .ghost()
                    .small()
                    .label(format!("{}%", (self.zoom * 100.).round() as u32))
                    .tooltip(language.text("Fit width", "Tilpass bredde"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = 1.;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("pdf-zoom-in")
                    .ghost()
                    .small()
                    .label("+")
                    .tooltip(language.text("Zoom in", "Zoom inn"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.zoom = (this.zoom * 1.25).min(3.);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("pdf-find")
                    .ghost()
                    .small()
                    .label(language.text("Find", "Finn"))
                    .on_click(cx.listener(|this, _, window, cx| this.focus_search(window, cx))),
            )
            .child(
                Button::new("pdf-copy")
                    .ghost()
                    .small()
                    .label(language.text("Copy", "Kopier"))
                    .disabled(
                        self.selected
                            .as_ref()
                            .is_none_or(|(_, s)| s.text.is_empty()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
            );
        let view = cx.entity();
        let max_ratio = self
            .info
            .as_ref()
            .map(|info| {
                info.pages
                    .iter()
                    .map(|s| s.height / s.width)
                    .fold(1.0, f64::max)
            })
            .unwrap_or(1.);
        let row_height = ((self.width - 24.).max(80.) * self.zoom * max_ratio as f32) + 32.;
        v_flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .track_focus(&self.focus)
            .key_context("PdfViewer")
            .on_action(
                cx.listener(|this, _: &input::Search, window, cx| this.focus_search(window, cx)),
            )
            .on_action(cx.listener(|this, _: &input::Copy, _, cx| this.copy(cx)))
            .child(toolbar)
            .when(self.search_open, |column| {
                column.child(
                    h_flex()
                        .flex_wrap()
                        .gap_1()
                        .p_1()
                        .child(Input::new(&self.search_input).w(px(160.)))
                        .child(
                            Button::new("pdf-search-submit")
                                .ghost()
                                .small()
                                .label(language.text("Find", "Finn"))
                                .disabled(self.search_busy)
                                .on_click(cx.listener(|this, _, _, cx| this.search(cx))),
                        )
                        .child(div().text_xs().child(if self.search_busy {
                            language.text("Searching…", "Søker…").to_string()
                        } else {
                            format!(
                                "{}/{}{}",
                                if result_count == 0 {
                                    0
                                } else {
                                    self.search_index + 1
                                },
                                result_count,
                                if self.search_results.truncated {
                                    "+"
                                } else {
                                    ""
                                }
                            )
                        }))
                        .child(
                            Button::new("pdf-search-next")
                                .ghost()
                                .small()
                                .label("›")
                                .tooltip(language.text("Next match", "Neste treff"))
                                .disabled(result_count == 0)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.search_results.matches.is_empty() {
                                        this.search_index = (this.search_index + 1)
                                            % this.search_results.matches.len();
                                        this.go_to_match(cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new("pdf-search-close")
                                .ghost()
                                .small()
                                .label("×")
                                .tooltip(language.text("Close search", "Lukk søk"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.search_open = false;
                                    this.focus.focus(window, cx);
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when_some(self.error, |column, error| {
                column.child(
                    v_flex()
                        .p_2()
                        .gap_2()
                        .text_sm()
                        .child(error_message(error, language))
                        .child(
                            Button::new("pdf-open-external")
                                .ghost()
                                .small()
                                .label(language.text("Open in default app", "Åpne i standardappen"))
                                .on_click({
                                    let path = self.path.clone();
                                    move |_, _, _| crate::app::open_in_default_app(&path)
                                }),
                        ),
                )
            })
            .when(self.info.is_none() && self.error.is_none(), |column| {
                column.child(
                    div()
                        .p_4()
                        .child(language.text("Opening PDF…", "Åpner PDF…")),
                )
            })
            .child(
                div()
                    .id("pdf-pages-scroll")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_x_scroll()
                    .on_prepaint({
                        let view = cx.entity();
                        move |bounds, window, cx| {
                            let width = f32::from(bounds.size.width);
                            if width > 0. && (view.read(cx).width - width).abs() > 1. {
                                let view = view.clone();
                                window.defer(cx, move |_, cx| {
                                    view.update(cx, |this, cx| {
                                        this.width = width;
                                        cx.notify();
                                    })
                                });
                            }
                        }
                    })
                    .child(
                        uniform_list("pdf-pages", count, move |range, window, cx| {
                            view.update(cx, |this, cx| {
                                range
                                    .map(|page| this.render_page(page, row_height, window, cx))
                                    .collect::<Vec<_>>()
                            })
                        })
                        .h_full()
                        .w(px(self
                            .width
                            .max((self.width - 24.).max(80.) * self.zoom + 24.)))
                        .track_scroll(&self.scroll),
                    ),
            )
    }
}

fn normalized(point: Point<Pixels>, bounds: Bounds<Pixels>) -> [f64; 2] {
    [
        ((point.x - bounds.origin.x) / bounds.size.width.max(px(1.))).clamp(0., 1.) as f64,
        ((point.y - bounds.origin.y) / bounds.size.height.max(px(1.))).clamp(0., 1.) as f64,
    ]
}

fn file_stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()?))
}

fn error_message(error: backend::PdfError, language: Language) -> &'static str {
    use backend::PdfError::*;
    match error {
        Unavailable => language.text(
            "Could not read this PDF. Check that the file is available.",
            "Kunne ikke lese PDF-filen. Kontroller at filen er tilgjengelig.",
        ),
        Invalid => language.text(
            "This PDF could not be opened.",
            "Denne PDF-filen kunne ikke åpnes.",
        ),
        Locked => language.text(
            "This PDF requires a password. Open it in your default PDF app.",
            "Denne PDF-filen krever passord. Åpne den i standardappen for PDF.",
        ),
        TooLarge => language.text(
            "This PDF exceeds the viewer’s size limit.",
            "Denne PDF-filen overskrider størrelsesgrensen for visning.",
        ),
        PageMissing => language.text(
            "This page is unavailable.",
            "Denne siden er ikke tilgjengelig.",
        ),
        CopyDenied => language.text(
            "Copying is not permitted by this PDF.",
            "Denne PDF-filen tillater ikke kopiering.",
        ),
        Unsupported => language.text(
            "In-app PDF viewing is currently available on macOS.",
            "PDF-visning i appen er foreløpig tilgjengelig på macOS.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::PdfTarget;

    #[test]
    fn pdf_targets_keep_local_names_and_parse_bounded_options() {
        assert_eq!(
            PdfTarget::parse("folder/Årsrapport.PDF#page=3&height=400|Alias"),
            Some(PdfTarget {
                file: "folder/Årsrapport.PDF".into(),
                page: 2,
                height: 400,
            })
        );
        let target = PdfTarget::parse("file.pdf#page=0#height=99999").unwrap();
        assert_eq!((target.page, target.height), (0, 1600));
        assert_eq!(PdfTarget::parse("file.pdf#page=wrong").unwrap().page, 0);
        assert!(PdfTarget::parse("note.md#page=3").is_none());
        assert!(PdfTarget::parse("https://example.com/file.pdf").is_none());
    }

    #[test]
    fn pdf_resolution_prefers_note_relative_then_vault_path() {
        let root = std::env::temp_dir().join(format!("rista-pdf-resolve-{}", std::process::id()));
        std::fs::create_dir_all(root.join("notes")).unwrap();
        let local = root.join("notes/report.pdf");
        let at_root = root.join("report.pdf");
        std::fs::write(&local, b"local").unwrap();
        std::fs::write(&at_root, b"root").unwrap();
        let target = PdfTarget::parse("report.pdf#page=2").unwrap();
        let doc = root.join("notes/Note.md");
        assert_eq!(
            target.resolve(Some(&doc), Some(&root), std::slice::from_ref(&at_root)),
            Some(local.clone())
        );
        std::fs::remove_file(&local).unwrap();
        assert_eq!(target.resolve(Some(&doc), Some(&root), &[]), Some(at_root));
        std::fs::remove_dir_all(root).unwrap();
    }
}
