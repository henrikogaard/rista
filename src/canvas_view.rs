use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{self, Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::component::text::{TextView, TextViewState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
use gpui_kit::prelude::*;
use gpui_kit::*;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::canvas::{CanvasDocument, CanvasFragment, Geometry};
use crate::document::Document;
use crate::settings::Language;

static NEXT_EDIT_SESSION: AtomicU64 = AtomicU64::new(1);

struct InlineEdit {
    node_id: String,
    state: Entity<EditorState>,
    last_revision: u64,
    _change_subscription: Subscription,
    _focus_subscription: Subscription,
}

#[derive(Clone)]
enum Drag {
    Pan {
        last: Point<Pixels>,
    },
    Move {
        ids: Vec<String>,
        last: Point<Pixels>,
        dx: f64,
        dy: f64,
    },
    Resize {
        id: String,
        last: Point<Pixels>,
        dx: f64,
        dy: f64,
    },
    Edge {
        from_id: String,
        last: Point<Pixels>,
    },
    Marquee {
        start: Point<Pixels>,
        end: Point<Pixels>,
        additive: bool,
    },
}

enum FinishedDrag {
    Pan,
    Move {
        ids: Vec<String>,
        dx: f64,
        dy: f64,
    },
    Resize {
        id: String,
        dx: f64,
        dy: f64,
    },
    Edge {
        from_id: String,
    },
    Marquee {
        start: Point<Pixels>,
        end: Point<Pixels>,
        additive: bool,
    },
}

fn take_finished_drag(drag: &mut Option<Drag>) -> Option<FinishedDrag> {
    Some(match drag.take()? {
        Drag::Pan { .. } => FinishedDrag::Pan,
        Drag::Move { ids, dx, dy, .. } => FinishedDrag::Move { ids, dx, dy },
        Drag::Resize { id, dx, dy, .. } => FinishedDrag::Resize { id, dx, dy },
        Drag::Edge { from_id, .. } => FinishedDrag::Edge { from_id },
        Drag::Marquee {
            start,
            end,
            additive,
        } => FinishedDrag::Marquee {
            start,
            end,
            additive,
        },
    })
}

fn canvas_markdown_excerpt(source: &str, subpath: Option<&str>) -> String {
    let selected = subpath.and_then(|subpath| {
        let anchor = subpath.strip_prefix('^').unwrap_or(subpath);
        if subpath.starts_with('^') {
            let lines = source.split('\n').collect::<Vec<_>>();
            let (start, end) = crate::preview::block_lines(&lines, anchor)?;
            let mut block = lines[start..=end]
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            if let Some(last) = block.last_mut() {
                let trimmed = last.trim_end();
                if let Some(text) = trimmed.strip_suffix(&format!("^{anchor}")) {
                    *last = text.trim_end().to_string();
                }
            }
            return Some(block.join("\n"));
        }
        let lines = source.split('\n').collect::<Vec<_>>();
        let mut in_fence = false;
        let mut start = None;
        let mut level = 0usize;
        let mut end = lines.len();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            let heading_level = trimmed
                .chars()
                .take_while(|character| *character == '#')
                .count();
            if !(1..=6).contains(&heading_level) || trimmed.chars().nth(heading_level) != Some(' ')
            {
                continue;
            }
            let title = trimmed[heading_level + 1..].trim();
            if start.is_none() && title.eq_ignore_ascii_case(anchor.trim_start_matches('#')) {
                start = Some(index);
                level = heading_level;
            } else if start.is_some() && heading_level <= level {
                end = index;
                break;
            }
        }
        start.map(|start| {
            lines[start..end]
                .join("\n")
                .trim_end_matches(['\r', '\n'])
                .to_string()
        })
    });
    let content = selected.unwrap_or_else(|| source.to_string());
    if content.chars().count() <= 6_000 {
        return content;
    }
    content.chars().take(6_000).collect()
}

fn canvas_file_reference(node: &serde_json::Value) -> Option<String> {
    let file = node["file"].as_str()?;
    let Some(subpath) = node["subpath"]
        .as_str()
        .filter(|subpath| !subpath.is_empty())
        .filter(|_| !file.contains('#'))
    else {
        return Some(file.to_string());
    };
    Some(format!("{file}#{}", subpath.trim_start_matches('#')))
}

fn canvas_node_matches_search(node: &serde_json::Value, query_folded: &str) -> bool {
    crate::canvas::node_search_text(node)
        .is_some_and(|text| text.to_lowercase().contains(query_folded))
}

#[derive(Clone)]
struct EdgePaint {
    id: String,
    from: Point<f64>,
    to: Point<f64>,
    curved: bool,
    from_arrow: bool,
    to_arrow: bool,
    color: String,
    label: Option<String>,
}

fn paint_edge_arrow(window: &mut Window, tip: Point<Pixels>, from: Point<Pixels>, color: Hsla) {
    let dx = f32::from(tip.x - from.x);
    let dy = f32::from(tip.y - from.y);
    let length = (dx * dx + dy * dy).sqrt().max(1.);
    let (ux, uy) = (dx / length, dy / length);
    let (wx, wy) = (-uy, ux);
    let back = point(tip.x - px(ux * 8.), tip.y - px(uy * 8.));
    let mut arrow = gpui::PathBuilder::fill();
    arrow.move_to(tip);
    arrow.line_to(point(back.x + px(wx * 3.), back.y + px(wy * 3.)));
    arrow.line_to(point(back.x - px(wx * 3.), back.y - px(wy * 3.)));
    arrow.close();
    if let Ok(path) = arrow.build() {
        window.paint_path(path, color);
    }
}

fn edge_curve_controls(from: Point<Pixels>, to: Point<Pixels>) -> (Point<Pixels>, Point<Pixels>) {
    let dx = f32::from(to.x - from.x);
    let sign = if dx < 0. { -1. } else { 1. };
    let control = (dx.abs() * 0.5).max(24.);
    (
        point(from.x + px(control * sign), from.y),
        point(to.x - px(control * sign), to.y),
    )
}

fn segment_distance(point: Point<Pixels>, from: Point<Pixels>, to: Point<Pixels>) -> f32 {
    let (px, py) = (f32::from(point.x), f32::from(point.y));
    let (ax, ay) = (f32::from(from.x), f32::from(from.y));
    let (dx, dy) = (f32::from(to.x - from.x), f32::from(to.y - from.y));
    let length = dx * dx + dy * dy;
    let t = if length <= f32::EPSILON {
        0.
    } else {
        (((px - ax) * dx + (py - ay) * dy) / length).clamp(0., 1.)
    };
    ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
}

pub(crate) struct CanvasView {
    focus_handle: FocusHandle,
    document: WeakEntity<Document>,
    workspace: WeakEntity<crate::app::Workspace>,
    language: Language,
    preview_context: crate::preview::PreviewCtx,
    source_revision: Option<u64>,
    vault_root: Option<PathBuf>,
    source: String,
    model: Option<CanvasDocument>,
    error: Option<String>,
    text_views: HashMap<String, Entity<TextViewState>>,
    text_sources: HashMap<String, String>,
    editing: Option<InlineEdit>,
    file_status: HashMap<String, bool>,
    file_views: HashMap<String, Entity<TextViewState>>,
    file_paths: HashMap<String, PathBuf>,
    file_signatures: HashMap<String, (String, Option<(std::time::SystemTime, u64)>)>,
    group_backgrounds: HashMap<String, PathBuf>,
    background_signatures: HashMap<String, (String, Option<(std::time::SystemTime, u64)>)>,
    selected: HashSet<String>,
    viewport: Point<f64>,
    zoom: f64,
    bounds: std::rc::Rc<Cell<Bounds<Pixels>>>,
    drag: Option<Drag>,
    snap_drag: bool,
    context_position: Option<(i64, i64)>,
    space_down: bool,
    status: Option<String>,
    search_query: Option<String>,
    search_input: Entity<InputState>,
    search_results: Vec<String>,
    search_index: usize,
    _document_subscription: Subscription,
    _vault_subscription: Subscription,
    _search_subscription: Subscription,
    _focus_subscription: Subscription,
}

impl CanvasView {
    pub(crate) fn new(
        document: Entity<Document>,
        workspace: WeakEntity<crate::app::Workspace>,
        vault: Entity<crate::vault::Vault>,
        language: Language,
        properties_visibility: crate::settings::PropertiesVisibility,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (preview_context, vault_root) = {
            let doc = document.read(cx);
            (
                crate::preview::PreviewCtx {
                    properties_visibility,
                    language,
                    vault: vault.clone(),
                    workspace: workspace.clone(),
                    views: doc.base_embeds.clone(),
                    query_views: doc.query_embeds.clone(),
                    pdf_views: doc.pdf_embeds.clone(),
                    doc_path: Some(doc.path.clone()),
                    depth: 0,
                },
                doc.vault_root.clone(),
            )
        };
        let focus_handle = cx.focus_handle();
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text("Find cards", "Finn kort"))
        });
        let search_subscription =
            cx.subscribe_in(&search_input, window, |this, input, event, window, cx| {
                if matches!(event, InputEvent::Change) && this.search_query.is_some() {
                    this.search_query = Some(input.read(cx).value().to_string());
                    this.search_index = 0;
                    this.refresh_search(0, window, cx);
                }
                if let InputEvent::PressEnter { shift, .. } = event {
                    this.refresh_search(if *shift { -1 } else { 1 }, window, cx);
                }
            });
        let focus_subscription = cx.on_focus_out(&focus_handle, window, |this, _, _, cx| {
            this.drag = None;
            this.space_down = false;
            cx.notify();
        });
        let document_subscription = cx.observe(&document, |this, _, cx| {
            this.sync_source(cx);
            cx.notify();
        });
        let vault_subscription = cx.observe(&vault, |this, _, cx| {
            this.refresh_file_previews_from_model(cx);
            this.text_sources.clear();
            this.source_revision = None;
            this.sync_source(cx);
            cx.notify();
        });
        let mut this = Self {
            focus_handle,
            document: document.downgrade(),
            workspace,
            language,
            preview_context,
            source_revision: None,
            vault_root,
            source: String::new(),
            model: None,
            error: None,
            text_views: HashMap::new(),
            text_sources: HashMap::new(),
            editing: None,
            file_status: HashMap::new(),
            file_views: HashMap::new(),
            file_paths: HashMap::new(),
            file_signatures: HashMap::new(),
            group_backgrounds: HashMap::new(),
            background_signatures: HashMap::new(),
            selected: HashSet::new(),
            viewport: point(80., 80.),
            zoom: 1.,
            bounds: std::rc::Rc::new(Cell::new(Bounds::default())),
            drag: None,
            snap_drag: true,
            context_position: None,
            space_down: false,
            status: None,
            search_query: None,
            search_input,
            search_results: Vec::new(),
            search_index: 0,
            _document_subscription: document_subscription,
            _vault_subscription: vault_subscription,
            _search_subscription: search_subscription,
            _focus_subscription: focus_subscription,
        };
        this.sync_source(cx);
        this
    }

    pub(crate) fn begin_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_query = Some(String::new());
        self.search_results.clear();
        self.search_index = 0;
        self.search_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.status = Some(
            self.language
                .text(
                    "Find card: type to search · Enter next · Esc close",
                    "Finn kort: skriv for å søke · Enter neste · Esc lukk",
                )
                .to_string(),
        );
        self.search_input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    fn refresh_search(&mut self, step: i8, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search_query.clone().unwrap_or_default();
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let query_folded = query.to_lowercase();
        self.search_results = if query_folded.is_empty() {
            Vec::new()
        } else {
            model
                .nodes()
                .iter()
                .filter_map(|node| {
                    let id = node["id"].as_str()?;
                    canvas_node_matches_search(node, &query_folded).then(|| id.to_string())
                })
                .collect()
        };
        if !self.search_results.is_empty() {
            if step > 0 {
                self.search_index = (self.search_index + 1) % self.search_results.len();
            } else if step < 0 && self.search_index == 0 {
                self.search_index = self.search_results.len() - 1;
            } else if step < 0 {
                self.search_index -= 1;
            } else {
                self.search_index = 0;
            }
            self.selected.clear();
            self.selected
                .insert(self.search_results[self.search_index].clone());
            self.fit_selection(true, window, cx);
        }
        let status = if self.search_results.is_empty() {
            self.language
                .text("No Canvas cards match", "Ingen Canvas-kort samsvarer")
                .to_string()
        } else {
            format!(
                "{} {}/{} · {}",
                self.language.text("Canvas match", "Canvas-treff"),
                self.search_index + 1,
                self.search_results.len(),
                query
            )
        };
        self.status = Some(status);
        cx.notify();
    }

    fn sync_source(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.upgrade() else {
            return;
        };
        let (revision, root, path) = {
            let document = document.read(cx);
            (
                document.content_revision,
                document.vault_root.clone(),
                document.path.clone(),
            )
        };
        let context_changed =
            self.preview_context.doc_path.as_ref() != Some(&path) || self.vault_root != root;
        if context_changed {
            self.text_sources.clear();
            self.file_signatures.clear();
        }
        if self.source_revision == Some(revision) && !context_changed {
            return;
        }
        let source = document.read(cx).editor.read(cx).value().to_string();
        self.preview_context.doc_path = Some(path);
        self.preview_context.language = self.language;
        if self
            .editing
            .as_ref()
            .is_some_and(|editing| editing.last_revision != revision)
        {
            self.editing = None;
            self.status = Some(
                self.language
                    .text(
                        "Inline editing ended because this Canvas changed elsewhere.",
                        "Redigering direkte i kortet ble avsluttet fordi Canvas-dokumentet ble endret et annet sted.",
                    )
                    .to_string(),
            );
        }
        self.source_revision = Some(revision);
        self.vault_root = root.clone();
        self.source = source.clone();
        match CanvasDocument::parse(&source) {
            Ok(model) => {
                self.error = None;
                let mut text_ids = HashSet::new();
                let mut file_references = Vec::new();
                let mut background_references = Vec::new();
                for node in model.nodes() {
                    let Some(id) = node["id"].as_str() else {
                        continue;
                    };
                    if node["type"].as_str() == Some("text") {
                        if let Some(text) = node["text"].as_str() {
                            text_ids.insert(id.to_string());
                            if self
                                .text_sources
                                .get(id)
                                .is_some_and(|previous| previous == text)
                                || self
                                    .editing
                                    .as_ref()
                                    .is_some_and(|editing| editing.node_id == id)
                            {
                                continue;
                            }
                            let rendered = self.preprocess_card(
                                text,
                                self.preview_context
                                    .doc_path
                                    .as_deref()
                                    .unwrap_or(Path::new("")),
                                cx,
                            );
                            if let Some(state) = self.text_views.get(id) {
                                state.update(cx, |state, cx| state.set_text(&rendered, cx));
                            } else {
                                let state = cx.new(|cx| TextViewState::markdown(&rendered, cx));
                                self.text_views.insert(id.to_string(), state);
                            }
                            self.text_sources.insert(id.to_string(), text.to_string());
                        }
                    }
                    if let Some(path) = canvas_file_reference(node) {
                        file_references.push((id.to_string(), path));
                    }
                    if node["type"].as_str() == Some("group") {
                        if let Some(path) = node["background"].as_str() {
                            background_references.push((id.to_string(), path.to_string()));
                        }
                    }
                }
                self.text_views.retain(|id, _| text_ids.contains(id));
                self.text_sources.retain(|id, _| text_ids.contains(id));
                self.refresh_file_previews(&file_references, root.as_deref(), false, cx);
                self.refresh_group_backgrounds(&background_references, root.as_deref(), false, cx);
                self.selected
                    .retain(|id| model.node(id).is_some() || model.edge(id).is_some());
                self.model = Some(model);
            }
            Err(error) => {
                self.model = None;
                self.error = Some(error);
                self.text_views.clear();
                self.text_sources.clear();
                self.file_status.clear();
                self.file_views.clear();
                self.file_paths.clear();
                self.file_signatures.clear();
                self.group_backgrounds.clear();
                self.background_signatures.clear();
                self.selected.clear();
            }
        }
        cx.notify();
    }

    pub(crate) fn refresh_settings(
        &mut self,
        language: Language,
        visibility: crate::settings::PropertiesVisibility,
        cx: &mut Context<Self>,
    ) {
        self.language = language;
        self.preview_context.language = language;
        self.preview_context.properties_visibility = visibility;
        self.text_sources.clear();
        self.file_signatures.clear();
        self.source_revision = None;
        let view = cx.entity();
        cx.defer(move |cx| {
            view.update(cx, |this, cx| this.sync_source(cx));
        });
    }

    fn preprocess_card(&self, text: &str, path: &Path, cx: &App) -> String {
        let resolver = self.preview_context.vault.read(cx).image_resolver();
        crate::preview::preprocess(
            text,
            path,
            self.vault_root.as_deref(),
            &*resolver,
            &crate::preview::MarkColors::from_theme(cx.theme()),
        )
    }

    fn refresh_file_previews_from_model(&mut self, cx: &mut Context<Self>) {
        let references = self
            .model
            .as_ref()
            .map(|model| {
                model
                    .nodes()
                    .iter()
                    .filter_map(|node| {
                        Some((
                            node["id"].as_str()?.to_string(),
                            canvas_file_reference(node)?,
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let backgrounds = self
            .model
            .as_ref()
            .map(|model| {
                model
                    .nodes()
                    .iter()
                    .filter(|node| node["type"].as_str() == Some("group"))
                    .filter_map(|node| {
                        Some((
                            node["id"].as_str()?.to_string(),
                            node["background"].as_str()?.to_string(),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let root = self.vault_root.clone();
        self.refresh_file_previews(&references, root.as_deref(), true, cx);
        self.refresh_group_backgrounds(&backgrounds, root.as_deref(), true, cx);
    }

    fn refresh_group_backgrounds(
        &mut self,
        references: &[(String, String)],
        root: Option<&Path>,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let mut backgrounds = HashMap::new();
        let mut signatures = HashMap::new();
        for (id, reference) in references {
            let Some(path) =
                root.and_then(|root| crate::canvas::resolve_vault_path(root, reference).ok())
            else {
                continue;
            };
            let metadata = std::fs::metadata(&path).ok();
            let stamp = metadata
                .as_ref()
                .and_then(|metadata| Some((metadata.modified().ok()?, metadata.len())));
            let unchanged =
                self.background_signatures
                    .get(id)
                    .is_some_and(|(previous, previous_stamp)| {
                        previous == reference && *previous_stamp == stamp
                    });
            if unchanged && !force {
                if let Some(previous) = self.group_backgrounds.get(id) {
                    backgrounds.insert(id.clone(), previous.clone());
                }
                signatures.insert(id.clone(), (reference.clone(), stamp));
                continue;
            }
            if !unchanged && self.background_signatures.contains_key(id) {
                let source: Resource = path.clone().into();
                cx.remove_asset::<ImgResourceLoader>(&source);
            }
            let extension = crate::file_preview::extension(&path);
            if metadata.is_some()
                && matches!(
                    extension.as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
                )
                && path.is_file()
            {
                backgrounds.insert(id.clone(), path);
            }
            signatures.insert(id.clone(), (reference.clone(), stamp));
        }
        self.group_backgrounds = backgrounds;
        self.background_signatures = signatures;
    }

    fn refresh_file_previews(
        &mut self,
        references: &[(String, String)],
        root: Option<&Path>,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let mut status = HashMap::new();
        let mut retained = HashSet::new();
        for (id, reference) in references {
            retained.insert(id.clone());
            let (file, subpath) = reference
                .split_once('#')
                .map_or((reference.as_str(), None), |(file, subpath)| {
                    (file, Some(subpath))
                });
            let path = root.and_then(|root| crate::canvas::resolve_vault_path(root, file).ok());
            let stamp = path.as_ref().and_then(|path| {
                let metadata = std::fs::metadata(path).ok()?;
                Some((metadata.modified().ok()?, metadata.len()))
            });
            let available = path.as_ref().is_some_and(|path| path.is_file());
            status.insert(id.clone(), available);
            if !force
                && self
                    .file_signatures
                    .get(id)
                    .is_some_and(|(previous, _)| previous == reference)
            {
                continue;
            }
            if force
                && self
                    .file_signatures
                    .get(id)
                    .is_some_and(|(previous, previous_stamp)| {
                        previous == reference && *previous_stamp == stamp
                    })
            {
                continue;
            }
            self.file_signatures
                .insert(id.clone(), (reference.clone(), stamp));
            let preview = path.as_ref().and_then(|path| {
                let extension = crate::file_preview::extension(path);
                if matches!(extension.as_str(), "md" | "markdown") {
                    let source = std::fs::read_to_string(path).ok()?;
                    Some(canvas_markdown_excerpt(&source, subpath))
                } else if matches!(
                    extension.as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "pdf"
                ) {
                    let name = path.to_string_lossy();
                    let subpath = subpath
                        .map(|subpath| format!("#{subpath}"))
                        .unwrap_or_default();
                    Some(format!("![[{name}{subpath}]]"))
                } else {
                    None
                }
            });
            if let Some(preview) = preview {
                let preview =
                    self.preprocess_card(&preview, path.as_deref().unwrap_or(Path::new("")), cx);
                if let Some(state) = self.file_views.get(id) {
                    state.update(cx, |state, cx| state.set_text(&preview, cx));
                } else {
                    self.file_views.insert(
                        id.clone(),
                        cx.new(|cx| TextViewState::markdown(&preview, cx)),
                    );
                }
                if let Some(path) = path {
                    self.file_paths.insert(id.clone(), path);
                }
            } else {
                self.file_views.remove(id);
                self.file_paths.remove(id);
            }
        }
        self.file_status = status;
        self.file_views.retain(|id, _| retained.contains(id));
        self.file_paths.retain(|id, _| retained.contains(id));
        self.file_signatures.retain(|id, _| retained.contains(id));
    }

    fn apply_model(&mut self, next: CanvasDocument, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_model_group(next, None, window, cx);
    }

    fn apply_model_group(
        &mut self,
        next: CanvasDocument,
        group: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Ok(after) = next.serialize() else {
            self.status = Some(
                self.language
                    .text(
                        "Could not serialize this Canvas change.",
                        "Kunne ikke serialisere denne Canvas-endringen.",
                    )
                    .to_string(),
            );
            cx.notify();
            return;
        };
        let before = self.source.clone();
        if before == after {
            return;
        }
        let Some(document) = self.document.upgrade() else {
            return;
        };
        let (applied, revision) = document.update(cx, |document, cx| {
            let applied = document.apply_canvas_snapshot(&before, &after, group, window, cx);
            (applied, document.content_revision)
        });
        if applied {
            self.source = after;
            self.model = Some(next);
            self.source_revision = None;
            if let Some(editing) = &mut self.editing {
                editing.last_revision = revision;
            }
            self.status = None;
            self.sync_source(cx);
        } else {
            self.source_revision = None;
            self.status = Some(
                self.language
                    .text(
                        "Canvas changed elsewhere; this action was cancelled.",
                        "Canvas ble endret et annet sted; handlingen ble avbrutt.",
                    )
                    .to_string(),
            );
        }
        cx.notify();
    }

    fn begin_inline_edit(&mut self, node_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.model.as_ref().and_then(|model| model.node(node_id)) else {
            return;
        };
        if node["type"].as_str() != Some("text") {
            return;
        }
        let initial = node["text"].as_str().unwrap_or_default().to_string();
        let state = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language("markdown")
                .line_number(false)
                .soft_wrap(true)
                .searchable(false);
            state.set_value(initial, window, cx);
            state
        });
        let group = NEXT_EDIT_SESSION.fetch_add(1, Ordering::Relaxed);
        let id = node_id.to_string();
        let change_subscription =
            cx.subscribe_in(&state, window, move |this, editor, event, window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = editor.read(cx).value().to_string();
                let Some(model) = this.model.clone() else {
                    return;
                };
                let mut next = model;
                if next
                    .set_node_field(&id, "text", serde_json::Value::String(text))
                    .is_ok()
                {
                    this.apply_model_group(next, Some(group), window, cx);
                }
            });
        let focus = state.read(cx).focus_handle(cx);
        let focus_subscription = cx.on_focus_out(&focus, window, |this, _, _, cx| {
            this.editing = None;
            this.source_revision = None;
            this.sync_source(cx);
            cx.notify();
        });
        self.editing = Some(InlineEdit {
            node_id: node_id.to_string(),
            state: state.clone(),
            last_revision: self.source_revision.unwrap_or_default(),
            _change_subscription: change_subscription,
            _focus_subscription: focus_subscription,
        });
        state.update(cx, |editor, cx| editor.focus(window, cx));
        cx.notify();
    }

    fn finish_inline_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.source_revision = None;
        self.sync_source(cx);
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn add_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_text_at(self.center_world(window), window, cx);
    }

    fn add_text_at(&mut self, position: (i64, i64), window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let mut next = model.clone();
        if let Ok(id) = next.add_text(position.0, position.1, "") {
            self.selected.clear();
            self.selected.insert(id);
            self.apply_model(next, window, cx);
        }
    }

    fn add_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_link_at(self.center_world(window), window, cx);
    }

    fn add_link_at(&mut self, position: (i64, i64), window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("https://"));
        let view = cx.entity().downgrade();
        let language = self.language;
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(language.text("Add link", "Legg til lenke"))
                .child(Input::new(&input).appearance(true))
                .on_ok({
                    let input = input.clone();
                    let view = view.clone();
                    move |_, window, cx| {
                        let url = input.read(cx).value().trim().to_string();
                        let view = view.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = view.update(cx, |this, cx| {
                                let Some(mut next) = this.model.clone() else {
                                    return;
                                };
                                match next.add_link(position.0, position.1, &url) {
                                    Ok(id) => {
                                        this.selected.clear();
                                        this.selected.insert(id);
                                        this.apply_model(next, window, cx);
                                    }
                                    Err(_) => this.status_error(
                                        "Enter an HTTP or HTTPS URL.",
                                        "Skriv inn en HTTP- eller HTTPS-adresse.",
                                        cx,
                                    ),
                                }
                            });
                        });
                        true
                    }
                })
        });
    }

    fn add_file_path(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.document.upgrade() else {
            return;
        };
        let root = document.read(cx).vault_root.clone();
        let Some(root) = root.and_then(|root| root.canonicalize().ok()) else {
            self.status_error(
                "Open a vault before adding file cards.",
                "Åpne et hvelv før du legger til filkort.",
                cx,
            );
            return;
        };
        let Some(path) = path
            .canonicalize()
            .ok()
            .filter(|path| path.is_file() && path.starts_with(&root))
        else {
            self.status_error(
                "Only files inside the open vault can be added to a Canvas.",
                "Bare filer i det åpne hvelvet kan legges til i et Canvas.",
                cx,
            );
            return;
        };
        let Ok(relative) = path.strip_prefix(&root) else {
            return;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let position = self.center_world(window);
        let Some(mut next) = self.model.clone() else {
            return;
        };
        match next.add_file(position.0, position.1, &relative) {
            Ok(id) => {
                self.selected.clear();
                self.selected.insert(id);
                self.apply_model(next, window, cx);
            }
            Err(_) => self.status_error(
                "Could not add this file card.",
                "Kunne ikke legge til dette filkortet.",
                cx,
            ),
        }
    }

    fn pick_canvas_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                self.language
                    .text("Add Canvas file card", "Legg til Canvas-filkort")
                    .into(),
            ),
        });
        cx.spawn_in(window, async move |view, window| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = window.update(|window, cx| {
                        let _ = view.update(cx, |this, cx| this.add_file_path(&path, window, cx));
                    });
                }
            }
        })
        .detach();
    }

    fn align_selected(&mut self, alignment: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut model) = self.model.clone() else {
            return;
        };
        match model.align_selection(
            &self.selected.iter().cloned().collect::<Vec<_>>(),
            alignment,
        ) {
            Ok(true) => self.apply_model(model, window, cx),
            Ok(false) => {}
            Err(error) => self.show_error(error, cx),
        }
    }

    fn reorder_selected(&mut self, to_front: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut model) = self.model.clone() else {
            return;
        };
        if model.reorder_selection(&self.selected.iter().cloned().collect::<Vec<_>>(), to_front) {
            self.apply_model(model, window, cx);
        }
    }

    fn open_inspector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.iter().next().cloned() else {
            return;
        };
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let is_edge = model.edge(&id).is_some();
        let data = if is_edge {
            model.edge(&id)
        } else {
            model.node(&id)
        };
        let Some(data) = data else { return };
        let fields: Vec<(&str, &str, &str)> = if is_edge {
            vec![
                ("fromNode", "From card", "Fra kort"),
                ("toNode", "To card", "Til kort"),
                ("fromSide", "From side", "Fra side"),
                ("toSide", "To side", "Til side"),
                ("fromEnd", "From end", "Fra ende"),
                ("toEnd", "To end", "Til ende"),
                ("label", "Label", "Etikett"),
                ("color", "Color (1–6 or #hex)", "Farge (1–6 eller #hex)"),
                ("curve", "Curve", "Kurve"),
            ]
        } else {
            match data["type"].as_str().unwrap_or_default() {
                "link" => vec![
                    ("url", "URL", "URL"),
                    ("color", "Color (1–6 or #hex)", "Farge (1–6 eller #hex)"),
                ],
                "file" => vec![
                    ("file", "Vault file", "Hvelvfil"),
                    (
                        "subpath",
                        "Heading, block or PDF page",
                        "Overskrift, blokk eller PDF-side",
                    ),
                    ("color", "Color (1–6 or #hex)", "Farge (1–6 eller #hex)"),
                ],
                "group" => vec![
                    ("label", "Label", "Etikett"),
                    ("background", "Background file", "Bakgrunnsfil"),
                    ("backgroundStyle", "Background style", "Bakgrunnstype"),
                    ("color", "Color (1–6 or #hex)", "Farge (1–6 eller #hex)"),
                ],
                _ => vec![("color", "Color (1–6 or #hex)", "Farge (1–6 eller #hex)")],
            }
        };
        let inputs = fields
            .iter()
            .map(|(field, _, _)| {
                let input = cx.new(|cx| InputState::new(window, cx));
                let value = data
                    .get(*field)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                input.update(cx, |input, cx| input.set_value(value, window, cx));
                (*field, input)
            })
            .collect::<Vec<_>>();
        let input_entities = inputs
            .iter()
            .map(|(_, input)| input.clone())
            .collect::<Vec<_>>();
        let view = cx.entity().downgrade();
        let language = self.language;
        let choices = fields
            .iter()
            .map(|(field, _, _)| -> Vec<(String, String)> {
                let options: &[(&str, &str, &str)] = match *field {
                    "fromSide" | "toSide" => &[
                        ("", "Automatic", "Automatisk"),
                        ("top", "Top", "Topp"),
                        ("right", "Right", "Høyre"),
                        ("bottom", "Bottom", "Bunn"),
                        ("left", "Left", "Venstre"),
                    ],
                    "fromEnd" => &[
                        ("", "None (default)", "Ingen (standard)"),
                        ("none", "None", "Ingen"),
                        ("arrow", "Arrow", "Pil"),
                    ],
                    "toEnd" => &[
                        ("", "Arrow (default)", "Pil (standard)"),
                        ("none", "None", "Ingen"),
                        ("arrow", "Arrow", "Pil"),
                    ],
                    "curve" => &[
                        ("", "Curved (default)", "Buet (standard)"),
                        ("curved", "Curved", "Buet"),
                        ("straight", "Straight", "Rett"),
                    ],
                    "backgroundStyle" => &[
                        ("", "Cover (default)", "Fyll (standard)"),
                        ("cover", "Cover", "Fyll"),
                        ("ratio", "Fit", "Tilpass"),
                        ("repeat", "Repeat", "Gjenta"),
                    ],
                    _ => &[],
                };
                if matches!(*field, "fromNode" | "toNode") {
                    return model
                        .nodes()
                        .iter()
                        .filter_map(|node| {
                            let id = node["id"].as_str()?;
                            let title = ["text", "label", "file", "url"]
                                .iter()
                                .find_map(|field| node[*field].as_str())
                                .unwrap_or(id)
                                .lines()
                                .next()
                                .unwrap_or(id)
                                .chars()
                                .take(60)
                                .collect::<String>();
                            Some((id.to_string(), title))
                        })
                        .collect();
                }
                options
                    .iter()
                    .map(|(value, en, nb)| {
                        (value.to_string(), self.language.text(en, nb).to_string())
                    })
                    .collect()
            })
            .collect::<Vec<_>>();
        let expected_source = self.source.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let mut body = v_flex()
                .id("canvas-inspector-fields")
                .gap_2()
                .max_h(px(360.))
                .overflow_y_scroll();
            for (index, ((_, en, nb), (_, input))) in fields.iter().zip(&inputs).enumerate() {
                let options = &choices[index];
                let control = if options.is_empty() {
                    Input::new(input).appearance(true).into_any_element()
                } else {
                    let selected = input.read(cx).value().to_string();
                    let label = options
                        .iter()
                        .find(|(value, _)| *value == selected)
                        .map(|(_, label)| label.clone())
                        .unwrap_or(selected);
                    let input = input.clone();
                    let options = options.clone();
                    Button::new(("canvas-inspector-choice", index))
                        .w_full()
                        .label(label)
                        .dropdown_menu(move |mut menu, _, _| {
                            for (value, label) in &options {
                                let value = value.clone();
                                let input = input.clone();
                                menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                                    move |_, window, cx| {
                                        input.update(cx, |input, cx| {
                                            input.set_value(value.clone(), window, cx)
                                        });
                                        window.refresh();
                                    },
                                ));
                            }
                            menu
                        })
                        .into_any_element()
                };
                body = body.child(
                    v_flex()
                        .gap_1()
                        .child(div().text_xs().child(language.text(en, nb).to_string()))
                        .child(control),
                );
            }
            dialog
                .title(language.text("Canvas selection", "Canvas-utvalg"))
                .w(px(420.))
                .child(body)
                .on_ok({
                    let view = view.clone();
                    let id = id.clone();
                    let fields = fields
                        .iter()
                        .map(|(field, _, _)| *field)
                        .collect::<Vec<_>>();
                    let input_entities = input_entities.clone();
                    let expected_source = expected_source.clone();
                    move |_, window, cx| {
                        let values = fields
                            .iter()
                            .zip(input_entities.iter())
                            .map(|(field, input)| (*field, input.read(cx).value().to_string()))
                            .collect::<Vec<_>>();
                        let view = view.clone();
                        let id = id.clone();
                        let expected_source = expected_source.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = view.update(cx, |this, cx| {
                                if this.source != expected_source {
                                    this.status_error(
                                        "Canvas changed; reopen the inspector.",
                                        "Canvas er endret; åpne inspektøren på nytt.",
                                        cx,
                                    );
                                    return false;
                                }
                                let Some(mut next) = this.model.clone() else {
                                    return false;
                                };
                                let mut changed = false;
                                for (field, value) in values {
                                    let current = if is_edge {
                                        next.edge(&id)
                                    } else {
                                        next.node(&id)
                                    }
                                    .and_then(|data| data.get(field))
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or_default();
                                    if current == value {
                                        continue;
                                    }
                                    if field == "backgroundStyle"
                                        && !value.is_empty()
                                        && !matches!(value.as_str(), "cover" | "ratio" | "repeat")
                                    {
                                        this.status_error(
                                            "Choose cover, ratio or repeat.",
                                            "Velg cover, ratio eller repeat.",
                                            cx,
                                        );
                                        return false;
                                    }
                                    if field == "url" && !crate::canvas::valid_external_url(&value)
                                    {
                                        this.status_error(
                                            "Enter an HTTP or HTTPS URL.",
                                            "Skriv inn en HTTP- eller HTTPS-adresse.",
                                            cx,
                                        );
                                        return false;
                                    }
                                    if matches!(field, "background" | "file")
                                        && (value.is_empty() && field == "file"
                                            || !value.is_empty()
                                                && !this.vault_root.as_deref().is_some_and(
                                                    |root| {
                                                        crate::canvas::resolve_vault_path(
                                                            root, &value,
                                                        )
                                                        .is_ok()
                                                    },
                                                ))
                                    {
                                        this.status_error(
                                            "Choose a file inside the vault.",
                                            "Velg en fil i hvelvet.",
                                            cx,
                                        );
                                        return false;
                                    }
                                    let value = if value.is_empty()
                                        && !matches!(field, "url" | "fromNode" | "toNode")
                                    {
                                        serde_json::Value::Null
                                    } else {
                                        serde_json::Value::String(value)
                                    };
                                    let result = if is_edge {
                                        next.set_edge_field(&id, field, value)
                                    } else {
                                        next.set_node_field(&id, field, value)
                                    };
                                    match result {
                                        Ok(value) => changed |= value,
                                        Err(error) => {
                                            this.show_error(error, cx);
                                            return false;
                                        }
                                    }
                                }
                                if changed {
                                    this.apply_model(next, window, cx);
                                }
                                true
                            });
                        });
                        true
                    }
                })
        });
    }

    fn add_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model.clone() else {
            return;
        };
        let selected = self.selected.iter().cloned().collect::<Vec<_>>();
        let mut next = model;
        let label = self.language.text("Group", "Gruppe");
        let result = if selected.is_empty() {
            let (x, y) = self.center_world(window);
            next.add_empty_group(x, y, label)
        } else {
            next.add_group(&selected, label, 24)
        };
        match result {
            Ok(id) => {
                self.selected.clear();
                self.selected.insert(id);
                self.apply_model(next, window, cx);
            }
            Err(error) => self.show_error(error, cx),
        }
    }

    fn selected_text_node(&self) -> Option<(String, String)> {
        if self.selected.len() != 1 {
            return None;
        }
        let id = self.selected.iter().next()?.clone();
        let node = self.model.as_ref()?.node(&id)?;
        (node["type"].as_str() == Some("text"))
            .then(|| (id, node["text"].as_str().unwrap_or_default().to_string()))
    }

    fn convert_selected_text_to_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, text)) = self.selected_text_node() else {
            return;
        };
        let (Some(document), Some(mut next)) = (self.document.upgrade(), self.model.clone()) else {
            return;
        };
        let (root, canvas_path) = {
            let document = document.read(cx);
            (document.vault_root.clone(), document.path.clone())
        };
        let Some(root) = root.and_then(|root| root.canonicalize().ok()) else {
            self.status_error(
                "Text-to-file conversion requires an open vault.",
                "Konvertering av tekst til fil krever et åpent hvelv.",
                cx,
            );
            return;
        };
        let Some(parent) = canvas_path
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
        else {
            self.status_error(
                "Could not resolve the Canvas folder.",
                "Kunne ikke finne Canvas-mappen.",
                cx,
            );
            return;
        };
        let Ok(relative_directory) = parent.strip_prefix(&root) else {
            self.status_error(
                "Canvas files must remain inside the open vault.",
                "Canvas-filer må forbli i det åpne hvelvet.",
                cx,
            );
            return;
        };
        let mut index = 0;
        let (path, relative) = loop {
            let name = if index == 0 {
                "Untitled.md".to_string()
            } else {
                format!("Untitled {index}.md")
            };
            let path = parent.join(&name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    if std::io::Write::write_all(&mut file, text.as_bytes()).is_err()
                        || file.sync_all().is_err()
                    {
                        let _ = std::fs::remove_file(&path);
                        self.status_error(
                            "Could not create the Markdown note.",
                            "Kunne ikke opprette Markdown-notatet.",
                            cx,
                        );
                        return;
                    }
                    let relative = relative_directory
                        .join(name)
                        .to_string_lossy()
                        .replace('\\', "/");
                    break (path, relative);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    index += 1;
                }
                Err(_) => {
                    self.status_error(
                        "Could not create the Markdown note.",
                        "Kunne ikke opprette Markdown-notatet.",
                        cx,
                    );
                    return;
                }
            }
        };
        if next.convert_text_to_file(&id, &relative).is_err() {
            let _ = std::fs::remove_file(path);
            self.status_error(
                "The selected text card could not be converted.",
                "Den valgte tekstbrikken kunne ikke konverteres.",
                cx,
            );
            return;
        }
        let after = match next.serialize() {
            Ok(after) => after,
            Err(_) => {
                let _ = std::fs::remove_file(path);
                self.status_error(
                    "Could not update the Canvas card.",
                    "Kunne ikke oppdatere Canvas-brikken.",
                    cx,
                );
                return;
            }
        };
        self.apply_model(next, window, cx);
        if self.source != after {
            if std::fs::read(&path).is_ok_and(|contents| contents.as_slice() == text.as_bytes()) {
                let _ = std::fs::remove_file(path);
                self.preview_context
                    .vault
                    .update(cx, |vault, cx| vault.refresh(cx));
            }
            return;
        }
        self.preview_context
            .vault
            .update(cx, |vault, cx| vault.refresh(cx));
    }

    fn apply_document_history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(document) = self.document.upgrade() {
            document.update(cx, |document, cx| {
                document.undo_canvas(redo, window, cx);
            });
            self.source_revision = None;
            cx.notify();
        }
    }

    fn duplicate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model.clone() else {
            return;
        };
        let selected = self.selected.iter().cloned().collect::<Vec<_>>();
        let mut next = model;
        match next.duplicate_selection(&selected, 32) {
            Ok(ids) => {
                self.selected = ids.into_iter().collect();
                self.apply_model(next, window, cx);
            }
            Err(error) => self.show_error(error, cx),
        }
    }

    fn copy_selected(&self, cx: &mut Context<Self>) {
        let (Some(model), Some(document)) = (self.model.as_ref(), self.document.upgrade()) else {
            return;
        };
        if self.selected.is_empty() {
            return;
        }
        let selected = self.selected.iter().cloned().collect::<Vec<_>>();
        let fragment = model.copy_selection(&selected);
        let vault_root = document.read(cx).vault_root.clone();
        let text = serde_json::to_string(&serde_json::json!({
            "nodes": fragment.nodes,
            "edges": fragment.edges,
        }))
        .unwrap_or_default();
        let metadata = serde_json::json!({
            "kind": "rista-canvas-fragment-v1",
            "vault_root": vault_root.map(|root| root.to_string_lossy().to_string()),
        });
        cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(text, metadata));
    }

    fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let Some(text) = item.text() else { return };
        let clipboard_metadata = item.entries.iter().find_map(|entry| match entry {
            ClipboardEntry::String(string) => string
                .metadata
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok()),
            _ => None,
        });
        let marked_fragment = clipboard_metadata
            .as_ref()
            .and_then(|metadata| metadata["kind"].as_str())
            == Some("rista-canvas-fragment-v1");
        let Some(model) = self.model.clone() else {
            return;
        };
        let mut next = model;
        if marked_fragment {
            let parsed = serde_json::from_str::<serde_json::Value>(&text);
            let Ok(parsed) = parsed else {
                self.show_error(String::new(), cx);
                return;
            };
            let Some(nodes) = parsed["nodes"].as_array() else {
                self.show_error(String::new(), cx);
                return;
            };
            let Some(edges) = parsed["edges"].as_array() else {
                self.show_error(String::new(), cx);
                return;
            };
            let contains_file_refs = nodes.iter().any(|node| {
                node["type"] == "file"
                    || node.get("background").is_some()
                    || node.get("backgroundImage").is_some()
            });
            if contains_file_refs {
                let Some(document) = self.document.upgrade() else {
                    return;
                };
                let destination = document.read(cx).vault_root.clone();
                let source = clipboard_metadata
                    .as_ref()
                    .and_then(|metadata| metadata["vault_root"].as_str())
                    .map(PathBuf::from);
                if destination.is_none() || source != destination {
                    self.show_error(String::new(), cx);
                    return;
                }
            }
            let fragment = CanvasFragment {
                nodes: nodes.clone(),
                edges: edges.clone(),
            };
            match next.paste_fragment(fragment, 24, 24) {
                Ok(ids) => self.selected = ids.into_iter().collect(),
                Err(error) => {
                    self.show_error(error, cx);
                    return;
                }
            }
        } else {
            let position = self.center_world(window);
            match next.add_text(position.0, position.1, &text) {
                Ok(id) => {
                    self.selected.clear();
                    self.selected.insert(id);
                }
                Err(error) => {
                    self.show_error(error, cx);
                    return;
                }
            }
        }
        self.apply_model(next, window, cx);
    }

    fn center_world(&self, window: &Window) -> (i64, i64) {
        let bounds = self.bounds.get();
        let size = if bounds.size.width > px(0.) && bounds.size.height > px(0.) {
            bounds.size
        } else {
            window.viewport_size()
        };
        (
            ((f64::from(size.width) * 0.5 - self.viewport.x) / self.zoom).round() as i64 - 140,
            ((f64::from(size.height) * 0.5 - self.viewport.y) / self.zoom).round() as i64 - 90,
        )
    }

    fn show_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.status = Some(
            self.language
                .text(
                    "Canvas operation could not be completed.",
                    "Canvas-handlingen kunne ikke fullføres.",
                )
                .to_string(),
        );
        let _ = error;
        cx.notify();
    }

    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model.clone() else {
            return;
        };
        let mut next = model;
        if next.remove_selection(&self.selected.iter().cloned().collect::<Vec<_>>()) {
            self.selected.clear();
            self.apply_model(next, window, cx);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if key == "escape" {
            self.drag = None;
            self.space_down = false;
            self.editing = None;
            self.source_revision = None;
            self.sync_source(cx);
            if self.search_query.is_some() {
                self.search_query = None;
                self.search_results.clear();
                self.status = None;
            }
            self.focus_handle.focus(window, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.editing.is_some() && modifiers.platform && matches!(key, "enter" | "return") {
            self.finish_inline_edit(window, cx);
            cx.stop_propagation();
            return;
        }
        if self.editing.is_some() {
            return;
        }
        if self.search_query.is_some()
            && self
                .search_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        {
            return;
        }
        if !modifiers.platform && !self.selected.is_empty() {
            let distance = if modifiers.shift { 10 } else { 1 };
            let (dx, dy) = match key {
                "left" => (-distance, 0),
                "right" => (distance, 0),
                "up" => (0, -distance),
                "down" => (0, distance),
                _ => (0, 0),
            };
            if dx != 0 || dy != 0 {
                if let Some(mut next) = self.model.clone() {
                    let selected = self.selected.iter().cloned().collect::<Vec<_>>();
                    if next.move_selection(&selected, dx, dy).unwrap_or(false) {
                        self.apply_model(next, window, cx);
                    }
                }
                cx.stop_propagation();
                return;
            }
        }
        if key == "space" {
            self.space_down = true;
            return;
        }
        if modifiers.platform && (key == "+" || key == "=") {
            self.zoom = (self.zoom * 1.2).clamp(0.05, 4.);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if modifiers.platform && key == "-" {
            self.zoom = (self.zoom / 1.2).clamp(0.05, 4.);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if modifiers.platform && key == "0" {
            self.fit_selection(false, window, cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.platform && key.eq_ignore_ascii_case("d") && !self.selected.is_empty() {
            self.duplicate_selected(window, cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.platform && key.eq_ignore_ascii_case("c") {
            self.copy_selected(cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.platform && key.eq_ignore_ascii_case("x") {
            self.copy_selected(cx);
            self.delete_selected(window, cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.platform && key.eq_ignore_ascii_case("v") {
            self.paste(window, cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.platform && key.eq_ignore_ascii_case("a") {
            if let Some(model) = &self.model {
                self.selected = model
                    .nodes()
                    .iter()
                    .filter_map(|node| node["id"].as_str().map(str::to_string))
                    .collect();
                self.selected.extend(
                    model
                        .edges()
                        .iter()
                        .filter_map(|edge| edge["id"].as_str().map(str::to_string)),
                );
                cx.stop_propagation();
                cx.notify();
            }
        } else if !modifiers.platform && !modifiers.control && matches!(key, "delete" | "backspace")
        {
            self.delete_selected(window, cx);
            cx.stop_propagation();
        } else if !modifiers.platform && !modifiers.control && !self.selected.is_empty() {
            let amount = if modifiers.shift { 10 } else { 1 };
            let delta = match key {
                "left" | "arrowleft" => Some((-amount, 0)),
                "right" | "arrowright" => Some((amount, 0)),
                "up" | "arrowup" => Some((0, -amount)),
                "down" | "arrowdown" => Some((0, amount)),
                _ => None,
            };
            if let Some((dx, dy)) = delta {
                if let Some(model) = self.model.clone() {
                    let mut next = model;
                    if next
                        .move_selection(&self.selected.iter().cloned().collect::<Vec<_>>(), dx, dy)
                        .is_ok()
                    {
                        self.apply_model(next, window, cx);
                    }
                }
                cx.stop_propagation();
            }
        } else if modifiers.platform && key.eq_ignore_ascii_case("z") {
            if let Some(document) = self.document.upgrade() {
                let redo = modifiers.shift;
                document.update(cx, |document, cx| {
                    document.undo_canvas(redo, window, cx);
                });
                self.source_revision = None;
                cx.stop_propagation();
                cx.notify();
            }
        } else if key == "escape" {
            self.drag = None;
            self.space_down = false;
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn world_to_screen(&self, value: f64) -> f32 {
        let screen = value * self.zoom;
        if !screen.is_finite() {
            return 0.0;
        }
        (screen + self.viewport.x).clamp(-1_000_000., 1_000_000.) as f32
    }

    fn world_to_screen_y(&self, value: f64) -> f32 {
        let screen = value * self.zoom;
        if !screen.is_finite() {
            return 0.0;
        }
        (screen + self.viewport.y).clamp(-1_000_000., 1_000_000.) as f32
    }

    fn window_to_board(&self, position: Point<Pixels>) -> Point<Pixels> {
        let origin = self.bounds.get().origin;
        point(position.x - origin.x, position.y - origin.y)
    }

    fn hit_node(&self, screen: Point<Pixels>) -> Option<String> {
        let model = self.model.as_ref()?;
        model.nodes().iter().rev().find_map(|node| {
            let id = node["id"].as_str()?;
            let geometry = model.node_geometry(node)?;
            let left = self.world_to_screen(geometry.x as f64);
            let top = self.world_to_screen_y(geometry.y as f64);
            let right = left + geometry.width as f32 * self.zoom as f32;
            let bottom = top + geometry.height as f32 * self.zoom as f32;
            (f32::from(screen.x) >= left
                && f32::from(screen.x) <= right
                && f32::from(screen.y) >= top
                && f32::from(screen.y) <= bottom)
                .then(|| id.to_string())
        })
    }

    fn hit_edge(&self, screen: Point<Pixels>) -> Option<String> {
        let model = self.model.as_ref()?;
        let mut edges = model.edges().iter().rev();
        edges.find_map(|edge| {
            let id = edge["id"].as_str()?;
            let from = model.node(edge["fromNode"].as_str()?)?;
            let to = model.node(edge["toNode"].as_str()?)?;
            let from_geometry = model.node_geometry(from)?;
            let to_geometry = model.node_geometry(to)?;
            let a = self.edge_anchor(from_geometry, edge["fromSide"].as_str(), to_geometry);
            let b = self.edge_anchor(to_geometry, edge["toSide"].as_str(), from_geometry);
            let from = point(
                px(self.world_to_screen(a.x)),
                px(self.world_to_screen_y(a.y)),
            );
            let to = point(
                px(self.world_to_screen(b.x)),
                px(self.world_to_screen_y(b.y)),
            );
            let selected = if edge["curve"].as_str() == Some("straight") {
                segment_distance(screen, from, to) <= 7.
            } else {
                let (control1, control2) = edge_curve_controls(from, to);
                let mut previous = from;
                let mut hit = false;
                for step in 1..=24 {
                    let t = step as f32 / 24.;
                    let inverse = 1. - t;
                    let next = point(
                        px(inverse.powi(3) * f32::from(from.x)
                            + 3. * inverse.powi(2) * t * f32::from(control1.x)
                            + 3. * inverse * t.powi(2) * f32::from(control2.x)
                            + t.powi(3) * f32::from(to.x)),
                        px(inverse.powi(3) * f32::from(from.y)
                            + 3. * inverse.powi(2) * t * f32::from(control1.y)
                            + 3. * inverse * t.powi(2) * f32::from(control2.y)
                            + t.powi(3) * f32::from(to.y)),
                    );
                    if segment_distance(screen, previous, next) <= 7. {
                        hit = true;
                        break;
                    }
                    previous = next;
                }
                hit
            };
            selected.then(|| id.to_string())
        })
    }

    fn edge_anchor(&self, geometry: Geometry, side: Option<&str>, toward: Geometry) -> Point<f64> {
        let center = point(
            geometry.x as f64 + geometry.width as f64 * 0.5,
            geometry.y as f64 + geometry.height as f64 * 0.5,
        );
        let target = point(
            toward.x as f64 + toward.width as f64 * 0.5,
            toward.y as f64 + toward.height as f64 * 0.5,
        );
        let side = side.unwrap_or_else(|| {
            if (target.x - center.x).abs() >= (target.y - center.y).abs() {
                if target.x < center.x {
                    "left"
                } else {
                    "right"
                }
            } else if target.y < center.y {
                "top"
            } else {
                "bottom"
            }
        });
        match side {
            "left" => point(geometry.x as f64, center.y),
            "right" => point(geometry.x as f64 + geometry.width as f64, center.y),
            "top" => point(center.x, geometry.y as f64),
            "bottom" => point(center.x, geometry.y as f64 + geometry.height as f64),
            _ => center,
        }
    }

    fn open_file_card(
        &mut self,
        node: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.document.upgrade() else {
            return;
        };
        let (root, relative) = {
            let document = document.read(cx);
            (
                document.vault_root.clone(),
                canvas_file_reference(node).unwrap_or_default(),
            )
        };
        let Some(root) = root else {
            self.status_error(
                "File cards require an open vault.",
                "Filkort krever en åpen boks.",
                cx,
            );
            return;
        };
        let (relative, subpath) = relative
            .split_once('#')
            .map_or((relative.as_str(), None), |(path, subpath)| {
                (path, (!subpath.is_empty()).then(|| subpath.to_string()))
            });
        let Ok(path) = crate::canvas::resolve_vault_path(&root, relative) else {
            self.status_error(
                "This file reference is outside the vault or unavailable.",
                "Denne filreferansen er utenfor boksen eller utilgjengelig.",
                cx,
            );
            return;
        };
        if !path.is_file() {
            self.status_error("File not found.", "Filen ble ikke funnet.", cx);
            return;
        }
        let workspace = self.workspace.clone();
        cx.on_next_frame(window, move |_, window, cx| {
            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    workspace.open_canvas_reference(path, subpath, window, cx);
                });
            }
        });
    }

    fn open_link_card(&mut self, node: &serde_json::Value, cx: &mut Context<Self>) {
        let url = node["url"].as_str().unwrap_or_default();
        if crate::canvas::valid_external_url(url) {
            cx.open_url(url);
        } else {
            self.status_error(
                "Only HTTP and HTTPS links can be opened.",
                "Bare HTTP- og HTTPS-lenker kan åpnes.",
                cx,
            );
        }
    }

    fn status_error(
        &mut self,
        english: &'static str,
        norwegian: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.status = Some(self.language.text(english, norwegian).to_string());
        cx.notify();
    }

    fn fit_selection(&mut self, selected_only: bool, window: &Window, cx: &mut Context<Self>) {
        let Some(model) = self.model.as_ref() else {
            return;
        };
        let ids = if selected_only {
            model.included_node_ids(&self.selected.iter().cloned().collect::<Vec<_>>())
        } else {
            model
                .nodes()
                .iter()
                .filter_map(|node| node["id"].as_str().map(str::to_string))
                .collect()
        };
        let bounds = ids
            .iter()
            .filter_map(|id| model.node(id).and_then(|node| model.node_geometry(node)))
            .fold(None, |bounds: Option<(i64, i64, i64, i64)>, geometry| {
                let right = geometry.right()?;
                let bottom = geometry.bottom()?;
                Some(match bounds {
                    Some((left, top, right0, bottom0)) => (
                        left.min(geometry.x),
                        top.min(geometry.y),
                        right0.max(right),
                        bottom0.max(bottom),
                    ),
                    None => (geometry.x, geometry.y, right, bottom),
                })
            });
        let Some((left, top, right, bottom)) = bounds else {
            return;
        };
        let bounds = self.bounds.get();
        let size = if bounds.size.width > px(0.) && bounds.size.height > px(0.) {
            bounds.size
        } else {
            window.viewport_size()
        };
        let width = (right - left).max(1) as f64;
        let height = (bottom - top).max(1) as f64;
        self.zoom = ((f64::from(size.width) - 120.) / width)
            .min((f64::from(size.height) - 180.) / height)
            .clamp(0.05, 2.);
        self.viewport = point(
            f64::from(size.width) * 0.5 - (left as f64 + right as f64) * 0.5 * self.zoom,
            f64::from(size.height) * 0.5 - (top as f64 + bottom as f64) * 0.5 * self.zoom,
        );
        cx.notify();
    }

    fn frame(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .size_full()
            .track_focus(&self.focus_handle)
            .key_context("CanvasBoard")
            .on_action(
                cx.listener(|this, _: &crate::actions::ToggleCheckbox, window, cx| {
                    if this.editing.is_some() {
                        this.finish_inline_edit(window, cx);
                    }
                }),
            )
            .capture_action::<input::Enter>(cx.listener(
                |this, enter: &input::Enter, window, cx| {
                    if enter.secondary && this.editing.is_some() {
                        this.finish_inline_edit(window, cx);
                        cx.stop_propagation();
                    }
                },
            ))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape"
                    || (this.editing.is_some()
                        && event.keystroke.modifiers.platform
                        && matches!(event.keystroke.key.as_str(), "enter" | "return"))
                {
                    this.on_key_down(event, window, cx);
                }
            }))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(|this, _: &crate::actions::ZoomIn, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    cx.propagate();
                    return;
                }
                this.zoom = (this.zoom * 1.2).clamp(0.05, 4.);
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &crate::actions::ZoomOut, window, cx| {
                    if !this.focus_handle.is_focused(window) {
                        cx.propagate();
                        return;
                    }
                    this.zoom = (this.zoom / 1.2).clamp(0.05, 4.);
                    cx.notify();
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ZoomReset, window, cx| {
                    if !this.focus_handle.is_focused(window) {
                        cx.propagate();
                        return;
                    }
                    this.fit_selection(false, window, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &input::Undo, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    cx.propagate();
                    return;
                }
                this.apply_document_history(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &input::Redo, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    cx.propagate();
                    return;
                }
                this.apply_document_history(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &input::Copy, window, cx| {
                if this.focus_handle.is_focused(window) {
                    this.copy_selected(cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &input::Cut, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    cx.propagate();
                    return;
                }
                if !this.selected.is_empty() {
                    this.copy_selected(cx);
                    this.delete_selected(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &input::Paste, window, cx| {
                if this.focus_handle.is_focused(window) {
                    this.paste(window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &input::SelectAll, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    cx.propagate();
                    return;
                }
                if let Some(model) = &this.model {
                    this.selected = model
                        .nodes()
                        .iter()
                        .chain(model.edges())
                        .filter_map(|value| value["id"].as_str().map(str::to_string))
                        .collect();
                    cx.notify();
                }
            }))
            .on_action(
                cx.listener(|this, _: &crate::actions::CanvasDelete, window, cx| {
                    if !this.focus_handle.is_focused(window) {
                        cx.propagate();
                        return;
                    }
                    this.delete_selected(window, cx)
                }),
            )
    }
    fn move_delta(&self, ids: &[String], dx: f64, dy: f64) -> (i64, i64) {
        if !self.snap_drag || (dx == 0. && dy == 0.) {
            return (dx.round() as i64, dy.round() as i64);
        }
        let anchor = ids
            .first()
            .and_then(|id| self.model.as_ref()?.node(id))
            .and_then(|node| self.model.as_ref()?.node_geometry(node));
        match anchor {
            Some(anchor) => (
                (((anchor.x as f64 + dx) / 10.).round() * 10.) as i64 - anchor.x,
                (((anchor.y as f64 + dy) / 10.).round() * 10.) as i64 - anchor.y,
            ),
            None => (dx.round() as i64, dy.round() as i64),
        }
    }

    fn resized_geometry(&self, mut geometry: Geometry, dx: f64, dy: f64) -> Geometry {
        if dx == 0. && dy == 0. {
            return geometry;
        }
        let grid = if self.snap_drag { 10. } else { 1. };
        geometry.width = (((geometry.width as f64 + dx / self.zoom) / grid).round() * grid)
            .clamp(40., 1_000_000_000.) as i64;
        geometry.height = (((geometry.height as f64 + dy / self.zoom) / grid).round() * grid)
            .clamp(40., 1_000_000_000.) as i64;
        geometry
    }

    fn staged_geometry(&self, id: &str, mut geometry: Geometry) -> Geometry {
        match &self.drag {
            Some(Drag::Move { ids, dx, dy, .. }) if ids.iter().any(|selected| selected == id) => {
                let (dx, dy) = self.move_delta(ids, *dx, *dy);
                geometry.x += dx;
                geometry.y += dy;
                geometry
            }
            Some(Drag::Resize {
                id: selected,
                dx,
                dy,
                ..
            }) if selected == id => self.resized_geometry(geometry, *dx, *dy),
            _ => geometry,
        }
    }

    fn render_card(
        &self,
        id: &str,
        node: &serde_json::Value,
        geometry: Geometry,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let mut palette = crate::preview::highlight_palette(&theme);
        palette[4] = theme.cyan;
        let color = node["color"]
            .as_str()
            .and_then(|value| {
                value
                    .parse::<usize>()
                    .ok()
                    .filter(|index| (1..=6).contains(index))
                    .map(|index| palette[index - 1])
                    .or_else(|| gpui_kit::component::theme::try_parse_color(value).ok())
            })
            .unwrap_or(theme.border);
        let width = (geometry.width as f32 * self.zoom as f32).max(1.);
        let height = (geometry.height as f32 * self.zoom as f32).max(1.);
        let selected = self.selected.contains(id);
        let mut card = div()
            .id(id.to_string())
            .absolute()
            .left(px(self.world_to_screen(geometry.x as f64)))
            .top(px(self.world_to_screen_y(geometry.y as f64)))
            .w(px(width))
            .h(px(height))
            .rounded(theme.radius)
            .border_1()
            .border_color(if selected { theme.primary } else { color })
            .bg(theme.group_box)
            .p_2()
            .overflow_hidden();
        if self.zoom < 0.25 {
            return card;
        }
        if node["type"].as_str() == Some("text") {
            if let Some(editing) = self
                .editing
                .as_ref()
                .filter(|editing| editing.node_id == id)
            {
                card = card.child(Editor::new(&editing.state).h_full().appearance(false));
            } else if let Some(state) = self.text_views.get(id) {
                card = card.child(
                    TextView::new(state)
                        .markdown_extensions(crate::preview::extensions(
                            &crate::preview::CalloutFolds::default(),
                            Some(&self.preview_context),
                            false,
                        ))
                        .selectable(true)
                        .scrollable(true),
                );
            } else {
                card = card.child(node["text"].as_str().unwrap_or_default().to_string());
            }
        } else if node["type"].as_str() == Some("file") {
            let file = node["file"].as_str().unwrap_or_default();
            let available = self.file_status.get(id).copied().unwrap_or(false);
            card = card
                .child(div().text_sm().font_semibold().child(file.to_string()))
                .child(
                    div()
                        .text_xs()
                        .text_color(if available {
                            theme.muted_foreground
                        } else {
                            theme.warning
                        })
                        .child(if available {
                            self.language.text("Open file", "Åpne filen")
                        } else {
                            self.language
                                .text("File unavailable", "Filen er utilgjengelig")
                        }),
                );
            if let Some(state) = self.file_views.get(id) {
                let mut preview_context = self.preview_context.clone();
                preview_context.doc_path = self.file_paths.get(id).cloned();
                card = card.child(
                    TextView::new(state)
                        .markdown_extensions(crate::preview::extensions(
                            &crate::preview::CalloutFolds::default(),
                            Some(&preview_context),
                            false,
                        ))
                        .selectable(true)
                        .scrollable(true),
                );
            }
        } else if node["type"].as_str() == Some("link") {
            card = card
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .child(self.language.text("Link", "Lenke")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(node["url"].as_str().unwrap_or_default().to_string()),
                );
        } else if node["type"].as_str() == Some("group") {
            card = card.bg(theme.group_box).border_color(color);
            if let Some(path) = self.group_backgrounds.get(id) {
                let source = ImageSource::from(path.clone());
                let fit = if node["backgroundStyle"].as_str() == Some("ratio") {
                    ObjectFit::Contain
                } else {
                    ObjectFit::Cover
                };
                let image = if node["backgroundStyle"].as_str() == Some("repeat") {
                    let zoom = self.zoom as f32;
                    let board_bounds = self.bounds.clone();
                    gpui::canvas(
                        |_, _, _| {},
                        move |bounds, _, window, cx| {
                            let ImageSource::Resource(resource) = &source else {
                                return;
                            };
                            let Some(Ok(image)) =
                                window.use_asset::<ImgResourceLoader>(resource, cx)
                            else {
                                return;
                            };
                            let native = image.size(0);
                            let scale = window.scale_factor();
                            let tile = size(
                                px((native.width.0 as f32 / scale * zoom).max(1.)),
                                px((native.height.0 as f32 / scale * zoom).max(1.)),
                            );
                            let clip = bounds.intersect(&board_bounds.get());
                            if clip.size.width <= px(0.) || clip.size.height <= px(0.) {
                                return;
                            }
                            let first_x = ((clip.origin.x - bounds.origin.x) / tile.width).floor();
                            let first_y = ((clip.origin.y - bounds.origin.y) / tile.height).floor();
                            let columns = (clip.size.width / tile.width).ceil() as usize + 1;
                            let rows = (clip.size.height / tile.height).ceil() as usize + 1;
                            if columns.saturating_mul(rows) > 4096 {
                                let image_bounds =
                                    ObjectFit::Cover.get_bounds(bounds, image.size(0));
                                let _ = window.paint_image(
                                    clip,
                                    image_bounds,
                                    Corners::default(),
                                    image,
                                    0,
                                    false,
                                );
                                return;
                            }
                            for row in 0..rows {
                                for column in 0..columns {
                                    let origin = point(
                                        bounds.origin.x + tile.width * (first_x + column as f32),
                                        bounds.origin.y + tile.height * (first_y + row as f32),
                                    );
                                    let _ = window.paint_image(
                                        clip,
                                        Bounds::new(origin, tile),
                                        Corners::default(),
                                        image.clone(),
                                        0,
                                        false,
                                    );
                                }
                            }
                        },
                    )
                    .size_full()
                    .into_any_element()
                } else {
                    img(source).size_full().object_fit(fit).into_any_element()
                };
                card = card.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .w_full()
                        .h_full()
                        .child(image),
                );
            }
            card = card.child(
                div()
                    .absolute()
                    .left(px(8.))
                    .top(px(8.))
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        node["label"]
                            .as_str()
                            .unwrap_or(self.language.text("Group", "Gruppe"))
                            .to_string(),
                    ),
            );
        } else {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "{}: {}",
                        self.language
                            .text("Unsupported card", "Korttypen støttes ikke"),
                        node["type"]
                            .as_str()
                            .unwrap_or(self.language.text("unknown", "ukjent"))
                    )),
            );
        }
        if selected {
            let resize_id = id.to_string();
            let edge_source_id = id.to_string();
            card = card.child(
                div()
                    .absolute()
                    .right(px(1.))
                    .top(px((height * 0.5 - 4.).max(0.)))
                    .size(px(8.))
                    .rounded(px(4.))
                    .bg(theme.primary)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            let position = this.window_to_board(event.position);
                            this.drag = Some(Drag::Edge {
                                from_id: edge_source_id.clone(),
                                last: position,
                            });
                            this.focus_handle.focus(window, cx);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ),
            );
            card = card.child(
                div()
                    .absolute()
                    .right(px(1.))
                    .bottom(px(1.))
                    .size(px(10.))
                    .rounded(px(2.))
                    .bg(theme.primary)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.drag = Some(Drag::Resize {
                                id: resize_id.clone(),
                                last: event.position,
                                dx: 0.,
                                dy: 0.,
                            });
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        card
    }
}

impl Focusable for CanvasView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CanvasView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let language = self.language;
        let document = self.document.upgrade();
        let read_only = document
            .as_ref()
            .is_none_or(|document| document.read(cx).is_read_only());
        let (can_undo, can_redo) = document
            .as_ref()
            .map(|document| document.read(cx).canvas_history_state())
            .unwrap_or_default();
        let view = cx.entity();
        let add_view = view.clone();
        let overflow_view = view;
        let has_selection = !self.selected.is_empty();
        let selection_count = self.selected.len();
        let can_convert = self.selected_text_node().is_some();
        let add_menu = Button::new("canvas-add-menu")
            .ghost()
            .xsmall()
            .label(language.text("Add", "Legg til"))
            .disabled(read_only || self.model.is_none())
            .dropdown_menu(move |menu, _, _| {
                let view = add_view.clone();
                menu.item(
                    PopupMenuItem::new(language.text("Text card", "Tekstkort")).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.add_text(window, cx));
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(language.text("File card…", "Filkort…")).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.pick_canvas_file(window, cx));
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(language.text("Link card", "Lenkekort")).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.add_link(window, cx));
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(if has_selection {
                        language.text("Group selection", "Grupper utvalg")
                    } else {
                        language.text("Group", "Gruppe")
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.add_group(window, cx));
                        }
                    }),
                )
            });
        let overflow = Button::new("canvas-commands")
            .ghost()
            .xsmall()
            .label(language.text("More", "Mer"))
            .disabled(!has_selection)
            .dropdown_menu(move |menu, _, _| {
                let view = overflow_view.clone();
                menu.item(
                    PopupMenuItem::new(language.text("Edit selection…", "Rediger utvalg…"))
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| this.open_inspector(window, cx));
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new(language.text("Duplicate", "Dupliser")).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.duplicate_selected(window, cx));
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(
                        language.text("Convert text to note", "Gjør tekst om til notat"),
                    )
                    .disabled(!can_convert)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.convert_selected_text_to_file(window, cx)
                            });
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(language.text("Align left", "Juster til venstre"))
                        .disabled(selection_count < 2)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| this.align_selected("left", window, cx));
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new(language.text("Align center", "Juster midtstilt"))
                        .disabled(selection_count < 2)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.align_selected("center", window, cx)
                                });
                            }
                        }),
                )
                .item(
                    PopupMenuItem::new(language.text("Distribute horizontally", "Fordel vannrett"))
                        .disabled(selection_count < 3)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.align_selected("distribute-horizontal", window, cx)
                                });
                            }
                        }),
                )
                .map(|mut menu| {
                    for (action, en, nb) in [
                        ("right", "Align right", "Juster til høyre"),
                        ("top", "Align top", "Juster øverst"),
                        ("middle", "Align middle", "Juster midten"),
                        ("bottom", "Align bottom", "Juster nederst"),
                        (
                            "distribute-vertical",
                            "Distribute vertically",
                            "Fordel loddrett",
                        ),
                    ] {
                        let view = view.clone();
                        menu = menu.item(
                            PopupMenuItem::new(language.text(en, nb))
                                .disabled(selection_count < 2)
                                .on_click(move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.align_selected(action, window, cx)
                                    });
                                }),
                        );
                    }
                    let view = view.clone();
                    menu.item(
                        PopupMenuItem::new(language.text("Fit selection", "Tilpass utvalg"))
                            .on_click(move |_, window, cx| {
                                view.update(cx, |this, cx| this.fit_selection(true, window, cx));
                            }),
                    )
                })
                .item(
                    PopupMenuItem::new(language.text("Bring to front", "Flytt fremst")).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.reorder_selected(true, window, cx));
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(language.text("Send to back", "Flytt bakerst")).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.reorder_selected(false, window, cx));
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(language.text("Delete", "Slett")).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.delete_selected(window, cx));
                        }
                    }),
                )
            });
        let toolbar = h_flex()
            .w_full()
            .h(px(34.))
            .px_2()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(add_menu)
            .child(
                Button::new("canvas-fit")
                    .ghost()
                    .xsmall()
                    .label(language.text("Fit", "Tilpass"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.fit_selection(false, window, cx);
                    })),
            )
            .child(overflow)
            .when_some(self.search_query.as_ref(), |bar, _| {
                bar.child(
                    div()
                        .w(px(160.))
                        .min_w(px(80.))
                        .child(Input::new(&self.search_input).appearance(true)),
                )
            })
            .child(
                Button::new("canvas-find")
                    .ghost()
                    .xsmall()
                    .icon(assets::IconName::Search)
                    .tooltip(language.text("Find", "Finn"))
                    .on_click(cx.listener(|this, _, window, cx| this.begin_search(window, cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new("canvas-undo")
                    .ghost()
                    .xsmall()
                    .icon(assets::IconName::Undo)
                    .tooltip(language.text("Undo", "Angre"))
                    .disabled(read_only || !can_undo)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apply_document_history(false, window, cx)
                    })),
            )
            .child(
                Button::new("canvas-redo")
                    .ghost()
                    .xsmall()
                    .icon(assets::IconName::Redo)
                    .tooltip(language.text("Redo", "Gjør om"))
                    .disabled(read_only || !can_redo)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apply_document_history(true, window, cx)
                    })),
            )
            .when(
                self.model.as_ref().is_some_and(|model| {
                    model
                        .nodes()
                        .iter()
                        .any(|node| model.geometry_is_unsupported(node))
                }),
                |bar| {
                    bar.child(
                        div()
                            .text_xs()
                            .text_color(theme.warning)
                            .child(language.text(
                                "Some cards have unsupported geometry",
                                "Noen kort har geometri som ikke støttes",
                            )),
                    )
                },
            )
            .when_some(self.status.clone(), |bar, status| {
                bar.child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(theme.warning)
                        .child(status),
                )
            });

        if self.error.is_some() {
            let body = self.frame(cx)
                .child(toolbar)
                .child(
                    div()
                        .flex_1()
                        .p_4()
                        .text_sm()
                        .text_color(theme.danger)
                        .child(language.text(
                            "This Canvas could not be parsed as JSON Canvas. Its original contents are preserved.",
                            "Dette Canvas-dokumentet kunne ikke leses som JSON Canvas. Det opprinnelige innholdet er bevart.",
                        )),
                );
            return body.into_any_element();
        }

        let Some(model) = self.model.as_ref() else {
            return self.frame(cx).child(toolbar).into_any_element();
        };
        let board_bounds = self.bounds.get();
        let viewport = if board_bounds.size.width > px(0.) && board_bounds.size.height > px(0.) {
            board_bounds.size
        } else {
            window.viewport_size()
        };
        let visible_left = -self.viewport.x / self.zoom - 100.;
        let visible_top = -self.viewport.y / self.zoom - 100.;
        let visible_right = visible_left + f64::from(viewport.width) / self.zoom + 200.;
        let visible_bottom = visible_top + f64::from(viewport.height) / self.zoom + 200.;
        let edge_specs = model
            .edges()
            .iter()
            .filter_map(|edge| {
                let from_node = model.node(edge["fromNode"].as_str()?)?;
                let to_node = model.node(edge["toNode"].as_str()?)?;
                let from_geometry = self
                    .staged_geometry(from_node["id"].as_str()?, model.node_geometry(from_node)?);
                let to_geometry =
                    self.staged_geometry(to_node["id"].as_str()?, model.node_geometry(to_node)?);
                let from = self.edge_anchor(from_geometry, edge["fromSide"].as_str(), to_geometry);
                let to = self.edge_anchor(to_geometry, edge["toSide"].as_str(), from_geometry);
                if from.x.max(to.x) < visible_left
                    || from.x.min(to.x) > visible_right
                    || from.y.max(to.y) < visible_top
                    || from.y.min(to.y) > visible_bottom
                {
                    return None;
                }
                Some(EdgePaint {
                    id: edge["id"].as_str()?.to_string(),
                    from: point(
                        f64::from(self.world_to_screen(from.x)),
                        f64::from(self.world_to_screen_y(from.y)),
                    ),
                    to: point(
                        f64::from(self.world_to_screen(to.x)),
                        f64::from(self.world_to_screen_y(to.y)),
                    ),
                    curved: edge["curve"].as_str() != Some("straight"),
                    from_arrow: edge["fromEnd"].as_str() == Some("arrow"),
                    to_arrow: edge["toEnd"].as_str().unwrap_or("arrow") == "arrow",
                    color: edge["color"].as_str().unwrap_or_default().to_string(),
                    label: edge["label"]
                        .as_str()
                        .filter(|label| !label.is_empty())
                        .map(str::to_string),
                })
            })
            .collect::<Vec<_>>();
        let edge_specs_for_paint = edge_specs.clone();
        let selected_edges = self.selected.clone();
        let edge_base = theme.muted_foreground;
        let pending_edge = match &self.drag {
            Some(Drag::Edge { from_id, last }) => model.node(from_id).and_then(|node| {
                let geometry = model.node_geometry(node)?;
                let from = self.edge_anchor(geometry, Some("right"), geometry);
                Some((
                    point(
                        f64::from(self.world_to_screen(from.x)),
                        f64::from(self.world_to_screen_y(from.y)),
                    ),
                    point(f64::from(last.x), f64::from(last.y)),
                ))
            }),
            _ => None,
        };
        let mut edge_palette = crate::preview::highlight_palette(&theme);
        edge_palette[4] = theme.cyan;
        let edge_default = theme.border;
        let bounds_slot = self.bounds.clone();
        let edge_canvas = gpui::canvas(
            move |bounds, _window, _cx| bounds_slot.set(bounds),
            move |bounds, _, window, _cx| {
                for edge in &edge_specs_for_paint {
                    let from = point(
                        bounds.origin.x + px(edge.from.x as f32),
                        bounds.origin.y + px(edge.from.y as f32),
                    );
                    let to = point(
                        bounds.origin.x + px(edge.to.x as f32),
                        bounds.origin.y + px(edge.to.y as f32),
                    );
                    let color = if selected_edges.contains(&edge.id) {
                        edge_base
                    } else {
                        match edge.color.as_str() {
                            "1" => edge_palette[0],
                            "2" => edge_palette[1],
                            "3" => edge_palette[2],
                            "4" => edge_palette[3],
                            "5" => edge_palette[4],
                            "6" => edge_palette[5],
                            value => gpui_kit::component::theme::try_parse_color(value)
                                .unwrap_or(edge_default),
                        }
                    };
                    let mut path =
                        gpui::PathBuilder::stroke(px(if selected_edges.contains(&edge.id) {
                            2.
                        } else {
                            1.
                        }));
                    path.move_to(from);
                    if edge.curved {
                        let (control1, control2) = edge_curve_controls(from, to);
                        path.cubic_bezier_to(to, control1, control2);
                    } else {
                        path.line_to(to);
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                    let (control1, control2) = edge_curve_controls(from, to);
                    if edge.from_arrow {
                        paint_edge_arrow(
                            window,
                            from,
                            if edge.curved { control1 } else { to },
                            color,
                        );
                    }
                    if edge.to_arrow {
                        paint_edge_arrow(
                            window,
                            to,
                            if edge.curved { control2 } else { from },
                            color,
                        );
                    }
                }
                if let Some((from, to)) = pending_edge {
                    let from = point(
                        bounds.origin.x + px(from.x as f32),
                        bounds.origin.y + px(from.y as f32),
                    );
                    let to = point(
                        bounds.origin.x + px(to.x as f32),
                        bounds.origin.y + px(to.y as f32),
                    );
                    let mut path = gpui::PathBuilder::stroke(px(1.));
                    path.move_to(from);
                    path.line_to(to);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, edge_base);
                    }
                }
            },
        );
        let edge_labels = edge_specs
            .iter()
            .filter_map(|edge| {
                let label = edge.label.as_ref()?;
                Some(
                    div()
                        .id(format!("canvas-edge-label-{}", edge.id))
                        .absolute()
                        .left(px(((edge.from.x + edge.to.x) * 0.5) as f32))
                        .top(px(((edge.from.y + edge.to.y) * 0.5) as f32))
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(label.clone()),
                )
            })
            .collect::<Vec<_>>();
        let cards = model
            .nodes()
            .iter()
            .filter_map(|node| {
                let id = node["id"].as_str()?.to_string();
                let geometry = self.staged_geometry(&id, model.node_geometry(node)?);
                let x = geometry.x as f64;
                let y = geometry.y as f64;
                let right = x + geometry.width as f64;
                let bottom = y + geometry.height as f64;
                if right < visible_left
                    || x > visible_right
                    || bottom < visible_top
                    || y > visible_bottom
                {
                    return None;
                }
                let node_type = node["type"].as_str().unwrap_or_default().to_string();
                let node = node.clone();
                let file_node = node.clone();
                let link_node = node.clone();
                let context_node = node.clone();
                let context_id = id.clone();
                let context_type = node_type.clone();
                let view = cx.entity();
                let language = self.language;
                let card = self.render_card(&id, &node, geometry, cx).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        if this
                            .editing
                            .as_ref()
                            .is_some_and(|editing| editing.node_id == id)
                        {
                            return;
                        }
                        if event.click_count >= 2 {
                            this.selected.clear();
                            this.selected.insert(id.clone());
                            match node_type.as_str() {
                                "text" => this.begin_inline_edit(&id, window, cx),
                                "file" => this.open_file_card(&file_node, window, cx),
                                "link" => this.open_link_card(&link_node, cx),
                                _ => {}
                            }
                            return;
                        }
                        if event.modifiers.shift {
                            if !this.selected.insert(id.clone()) {
                                this.selected.remove(&id);
                            }
                        } else if !this.selected.contains(&id) {
                            this.selected.clear();
                            this.selected.insert(id.clone());
                        }
                        if let Some(model) = &this.model {
                            let ids = model.included_node_ids(
                                &this.selected.iter().cloned().collect::<Vec<_>>(),
                            );
                            this.drag = if this.space_down {
                                Some(Drag::Pan {
                                    last: event.position,
                                })
                            } else {
                                Some(Drag::Move {
                                    ids,
                                    last: event.position,
                                    dx: 0.,
                                    dy: 0.,
                                })
                            };
                        }
                        this.focus_handle.focus(window, cx);
                        cx.notify();
                    }),
                );
                Some(card.context_menu(move |menu, _, _| {
                    menu.item(
                        PopupMenuItem::new(language.text("Edit", "Rediger")).on_click({
                            let view = view.clone();
                            let id = context_id.clone();
                            let node_type = context_type.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.selected.clear();
                                    this.selected.insert(id.clone());
                                    match node_type.as_str() {
                                        "text" => this.begin_inline_edit(&id, window, cx),
                                        "link" | "group" | "file" => {
                                            this.open_inspector(window, cx)
                                        }
                                        _ => this.fit_selection(true, window, cx),
                                    }
                                });
                            }
                        }),
                    )
                    .item(
                        PopupMenuItem::new(language.text("Open", "Åpne"))
                            .disabled(!matches!(context_type.as_str(), "file" | "link"))
                            .on_click({
                                let view = view.clone();
                                let node = context_node.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| match node["type"].as_str() {
                                        Some("file") => this.open_file_card(&node, window, cx),
                                        Some("link") => this.open_link_card(&node, cx),
                                        _ => this.fit_selection(true, window, cx),
                                    });
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new(language.text("Duplicate", "Dupliser")).on_click({
                            let view = view.clone();
                            let id = context_id.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.selected.clear();
                                    this.selected.insert(id.clone());
                                    this.duplicate_selected(window, cx);
                                });
                            }
                        }),
                    )
                    .item(
                        PopupMenuItem::new(language.text("Delete", "Slett")).on_click({
                            let view = view.clone();
                            let id = context_id.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.selected.clear();
                                    this.selected.insert(id.clone());
                                    this.delete_selected(window, cx);
                                });
                            }
                        }),
                    )
                }))
            })
            .collect::<Vec<_>>();
        let board = div()
            .id("canvas-board")
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(theme.background)
            .can_drop(|value, _, _| value.is::<PathBuf>())
            .on_drop::<PathBuf>(cx.listener(|this, path: &PathBuf, window, cx| {
                this.add_file_path(path, window, cx);
            }))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" {
                    this.space_down = false;
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.drag = Some(Drag::Pan {
                        last: event.position,
                    });
                    this.focus_handle.focus(window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let position = this.window_to_board(event.position);
                    if let Some(edge_id) = this.hit_edge(position) {
                        if !event.modifiers.shift {
                            this.selected.clear();
                        }
                        if !this.selected.insert(edge_id.clone()) {
                            this.selected.remove(&edge_id);
                        }
                        this.drag = None;
                        this.focus_handle.focus(window, cx);
                        cx.notify();
                        return;
                    }
                    if this.hit_node(position).is_none() {
                        if !event.modifiers.shift {
                            this.selected.clear();
                        }
                        this.drag = if this.space_down {
                            Some(Drag::Pan {
                                last: event.position,
                            })
                        } else {
                            Some(Drag::Marquee {
                                start: position,
                                end: position,
                                additive: event.modifiers.shift,
                            })
                        };
                        this.focus_handle.focus(window, cx);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.snap_drag = !event.modifiers.alt;
                let Some(drag) = this.drag.clone() else {
                    return;
                };
                match drag {
                    Drag::Pan { last } => {
                        this.viewport.x += f64::from(event.position.x - last.x);
                        this.viewport.y += f64::from(event.position.y - last.y);
                        this.drag = Some(Drag::Pan {
                            last: event.position,
                        });
                    }
                    Drag::Move { ids, last, dx, dy } => {
                        let step_x = f64::from(event.position.x - last.x) / this.zoom;
                        let step_y = f64::from(event.position.y - last.y) / this.zoom;
                        this.drag = Some(Drag::Move {
                            ids,
                            last: event.position,
                            dx: dx + step_x,
                            dy: dy + step_y,
                        });
                    }
                    Drag::Resize { id, last, dx, dy } => {
                        let step_x = f64::from(event.position.x - last.x);
                        let step_y = f64::from(event.position.y - last.y);
                        this.drag = Some(Drag::Resize {
                            id,
                            last: event.position,
                            dx: dx + step_x,
                            dy: dy + step_y,
                        });
                    }
                    Drag::Edge { from_id, .. } => {
                        let position = this.window_to_board(event.position);
                        this.drag = Some(Drag::Edge {
                            from_id,
                            last: position,
                        });
                    }
                    Drag::Marquee {
                        start, additive, ..
                    } => {
                        let position = this.window_to_board(event.position);
                        this.drag = Some(Drag::Marquee {
                            start,
                            end: position,
                            additive,
                        });
                    }
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    if matches!(this.drag, Some(Drag::Pan { .. })) {
                        this.drag = None;
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.snap_drag = !event.modifiers.alt;
                    match take_finished_drag(&mut this.drag) {
                        Some(FinishedDrag::Edge { from_id }) => {
                            if let Some(to_id) = this.hit_node(this.window_to_board(event.position))
                            {
                                if to_id != from_id {
                                    if let Some(mut next) = this.model.clone() {
                                        let edge = serde_json::json!({
                                            "fromNode": from_id,
                                            "toNode": to_id,
                                            "fromEnd": "none",
                                            "toEnd": "arrow"
                                        });
                                        if let Ok(id) = next.add_edge(edge) {
                                            this.selected.clear();
                                            this.selected.insert(id);
                                            this.apply_model(next, window, cx);
                                        }
                                    }
                                }
                            }
                        }
                        Some(FinishedDrag::Move { ids, dx, dy }) => {
                            let (dx, dy) = this.move_delta(&ids, dx, dy);
                            if let Some(mut next) = this.model.clone() {
                                if next.move_selection(&ids, dx, dy).is_ok() {
                                    this.apply_model(next, window, cx);
                                }
                            }
                        }
                        Some(FinishedDrag::Resize { id, dx, dy }) => {
                            if let Some(mut next) = this.model.clone() {
                                if let Some(geometry) =
                                    next.node(&id).and_then(|node| next.node_geometry(node))
                                {
                                    let geometry = this.resized_geometry(geometry, dx, dy);
                                    if next
                                        .resize_node(&id, geometry.width, geometry.height)
                                        .is_ok()
                                    {
                                        this.apply_model(next, window, cx);
                                    }
                                }
                            }
                        }
                        Some(FinishedDrag::Marquee {
                            start,
                            end,
                            additive,
                        }) => {
                            if !additive {
                                this.selected.clear();
                            }
                            if let Some(model) = &this.model {
                                let left = f64::from(start.x.min(end.x));
                                let right = f64::from(start.x.max(end.x));
                                let top = f64::from(start.y.min(end.y));
                                let bottom = f64::from(start.y.max(end.y));
                                for node in model.nodes() {
                                    let (Some(id), Some(geometry)) =
                                        (node["id"].as_str(), model.node_geometry(node))
                                    else {
                                        continue;
                                    };
                                    let x = this.world_to_screen(geometry.x as f64);
                                    let y = this.world_to_screen_y(geometry.y as f64);
                                    let card_right = this
                                        .world_to_screen(geometry.x as f64 + geometry.width as f64);
                                    let card_bottom = this.world_to_screen_y(
                                        geometry.y as f64 + geometry.height as f64,
                                    );
                                    if f64::from(card_right) >= left
                                        && f64::from(x) <= right
                                        && f64::from(card_bottom) >= top
                                        && f64::from(y) <= bottom
                                    {
                                        this.selected.insert(id.to_string());
                                    }
                                }
                            }
                            cx.notify();
                        }
                        Some(FinishedDrag::Pan) | None => {}
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(20.));
                this.viewport.x += f64::from(delta.x);
                this.viewport.y += f64::from(delta.y);
                cx.notify();
            }))
            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| {
                let old = this.zoom;
                let new_zoom = (old * (1. + f64::from(event.delta))).clamp(0.05, 4.);
                let position = this.window_to_board(event.position);
                let world_x = (f64::from(position.x) - this.viewport.x) / old;
                let world_y = (f64::from(position.y) - this.viewport.y) / old;
                this.viewport.x = f64::from(position.x) - world_x * new_zoom;
                this.viewport.y = f64::from(position.y) - world_y * new_zoom;
                this.zoom = new_zoom;
                if this.zoom != old {
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    let position = this.window_to_board(event.position);
                    if this.hit_node(position).is_none() && this.hit_edge(position).is_none() {
                        this.context_position = Some((
                            ((f64::from(position.x) - this.viewport.x) / this.zoom).round() as i64
                                - 140,
                            ((f64::from(position.y) - this.viewport.y) / this.zoom).round() as i64
                                - 90,
                        ));
                    }
                }),
            )
            .child(edge_canvas.absolute().size_full())
            .children(edge_labels)
            .children(cards)
            .when_some(
                match &self.drag {
                    Some(Drag::Marquee { start, end, .. }) => Some((
                        f32::from(start.x.min(end.x)),
                        f32::from(start.y.min(end.y)),
                        f32::from((start.x - end.x).abs()),
                        f32::from((start.y - end.y).abs()),
                    )),
                    _ => None,
                },
                |board, (left, top, width, height)| {
                    board.child(
                        div()
                            .absolute()
                            .left(px(left))
                            .top(px(top))
                            .w(px(width.max(1.)))
                            .h(px(height.max(1.)))
                            .border_1()
                            .border_color(theme.primary),
                    )
                },
            )
            .context_menu({
                let view = cx.entity();
                let language = self.language;
                move |menu, _, _| {
                    menu.item(
                        PopupMenuItem::new(language.text("Add text card", "Legg til tekstkort"))
                            .on_click({
                                let view = view.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        let position = this
                                            .context_position
                                            .take()
                                            .unwrap_or_else(|| this.center_world(window));
                                        this.add_text_at(position, window, cx);
                                    });
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new(language.text("Add link card", "Legg til lenkekort"))
                            .on_click({
                                let view = view.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        let position = this
                                            .context_position
                                            .take()
                                            .unwrap_or_else(|| this.center_world(window));
                                        this.add_link_at(position, window, cx);
                                    });
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new(language.text("Add file card", "Legg til filkort"))
                            .on_click({
                                let view = view.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| this.pick_canvas_file(window, cx));
                                }
                            }),
                    )
                    .map(|menu| {
                        menu.item(
                            PopupMenuItem::new(language.text("Add group", "Legg til gruppe"))
                                .on_click({
                                    let view = view.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| this.add_group(window, cx));
                                    }
                                }),
                        )
                    })
                }
            });
        self.frame(cx)
            .child(toolbar)
            .child(board)
            .into_any_element()
    }
}

impl Drop for CanvasView {
    fn drop(&mut self) {
        self.drag = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        canvas_file_reference, canvas_markdown_excerpt, canvas_node_matches_search,
        take_finished_drag, Drag, FinishedDrag,
    };
    use gpui_kit::{point, px};

    #[test]
    fn canvas_completed_move_resize_and_marquee_drags_are_consumed_once() {
        let mut drag = Some(Drag::Move {
            ids: vec!["node".into()],
            last: point(px(0.), px(0.)),
            dx: 1.,
            dy: 2.,
        });
        let first = take_finished_drag(&mut drag).expect("move completion");
        assert!(match first {
            FinishedDrag::Move { ids, dx, dy } => ids == ["node"] && dx == 1. && dy == 2.,
            _ => false,
        });
        assert!(drag.is_none());

        drag = Some(Drag::Resize {
            id: "node".into(),
            last: point(px(0.), px(0.)),
            dx: 3.,
            dy: 4.,
        });
        let second = take_finished_drag(&mut drag).expect("resize completion");
        assert!(match second {
            FinishedDrag::Resize { id, dx, dy } => id == "node" && dx == 3. && dy == 4.,
            _ => false,
        });
        assert!(drag.is_none());

        drag = Some(Drag::Marquee {
            start: point(px(0.), px(0.)),
            end: point(px(12.), px(16.)),
            additive: false,
        });
        let third = take_finished_drag(&mut drag).expect("marquee completion");
        assert!(matches!(third, FinishedDrag::Marquee { .. }));
        assert!(drag.is_none());
    }

    #[test]
    fn canvas_file_card_uses_separate_subpath_and_excerpt() {
        let node = serde_json::json!({
            "type": "file",
            "file": "notes/guide.md",
            "subpath": "#Usage"
        });
        assert_eq!(
            canvas_file_reference(&node).as_deref(),
            Some("notes/guide.md#Usage")
        );
        assert_eq!(
            canvas_markdown_excerpt(
                "# Intro\n\nText.\n\n# Usage\n\nDetails.\n\n# End\nDone.",
                Some("Usage")
            ),
            "# Usage\n\nDetails."
        );
        assert_eq!(
            canvas_markdown_excerpt(
                "First block ^block-a\n\nSecond paragraph.",
                Some("^block-a")
            ),
            "First block"
        );
    }

    #[test]
    fn canvas_board_find_uses_only_typed_node_search_fields() {
        let text = serde_json::json!({
            "type": "text",
            "text": "Visible prose",
            "label": "hidden metadata"
        });
        assert!(canvas_node_matches_search(&text, "visible"));
        assert!(!canvas_node_matches_search(&text, "hidden"));

        let file = serde_json::json!({
            "type": "file",
            "file": "notes/guide.md",
            "text": "hidden metadata"
        });
        assert!(canvas_node_matches_search(&file, "guide.md"));
        assert!(!canvas_node_matches_search(&file, "hidden"));

        let link = serde_json::json!({
            "type": "link",
            "url": "https://example.test/path",
            "label": "hidden metadata"
        });
        assert!(canvas_node_matches_search(&link, "example.test"));
        assert!(!canvas_node_matches_search(&link, "hidden"));

        let group = serde_json::json!({
            "type": "group",
            "label": "Project group",
            "url": "hidden metadata"
        });
        assert!(canvas_node_matches_search(&group, "project"));
        assert!(!canvas_node_matches_search(&group, "hidden"));

        let unknown = serde_json::json!({
            "type": "future",
            "text": "hidden metadata",
            "label": "hidden metadata",
            "file": "hidden metadata",
            "url": "hidden metadata"
        });
        assert!(!canvas_node_matches_search(&unknown, "hidden"));
    }
}
