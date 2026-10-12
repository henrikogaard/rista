use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::text::{TextView, TextViewState};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable};
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

pub(crate) struct CanvasView {
    focus_handle: FocusHandle,
    document: WeakEntity<Document>,
    workspace: WeakEntity<crate::app::Workspace>,
    language: Language,
    preview_context: crate::preview::PreviewCtx,
    source_revision: Option<u64>,
    source: String,
    model: Option<CanvasDocument>,
    error: Option<String>,
    text_views: HashMap<String, Entity<TextViewState>>,
    editing: Option<InlineEdit>,
    file_status: HashMap<String, bool>,
    selected: HashSet<String>,
    viewport: Point<f64>,
    zoom: f64,
    bounds: std::rc::Rc<Cell<Bounds<Pixels>>>,
    drag: Option<Drag>,
    context_position: Option<(i64, i64)>,
    space_down: bool,
    status: Option<String>,
    search_query: Option<String>,
    search_results: Vec<String>,
    search_index: usize,
}

impl CanvasView {
    pub(crate) fn new(
        document: Entity<Document>,
        workspace: WeakEntity<crate::app::Workspace>,
        vault: Entity<crate::vault::Vault>,
        language: Language,
        properties_visibility: crate::settings::PropertiesVisibility,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc = document.read(cx);
        let preview_context = crate::preview::PreviewCtx {
            properties_visibility,
            language,
            vault,
            workspace: workspace.clone(),
            views: doc.base_embeds.clone(),
            query_views: doc.query_embeds.clone(),
            pdf_views: doc.pdf_embeds.clone(),
            doc_path: Some(doc.path.clone()),
            depth: 0,
        };
        Self {
            focus_handle: cx.focus_handle(),
            document: document.downgrade(),
            workspace,
            language,
            preview_context,
            source_revision: None,
            source: String::new(),
            model: None,
            error: None,
            text_views: HashMap::new(),
            editing: None,
            file_status: HashMap::new(),
            selected: HashSet::new(),
            viewport: point(80., 80.),
            zoom: 1.,
            bounds: std::rc::Rc::new(Cell::new(Bounds::default())),
            drag: None,
            context_position: None,
            space_down: false,
            status: None,
            search_query: None,
            search_results: Vec::new(),
            search_index: 0,
        }
    }

    pub(crate) fn begin_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_query = Some(String::new());
        self.search_results.clear();
        self.search_index = 0;
        self.status = Some(
            self.language
                .text(
                    "Find card: type to search · Enter next · Esc close",
                    "Finn kort: skriv for å søke · Enter neste · Esc lukk",
                )
                .to_string(),
        );
        self.focus_handle.focus(window, cx);
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
                    let searchable = ["text", "label", "file", "url"]
                        .iter()
                        .filter_map(|field| node[*field].as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    searchable
                        .to_lowercase()
                        .contains(&query_folded)
                        .then(|| id.to_string())
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
        if let Some(workspace) = self.workspace.upgrade() {
            self.language = workspace.read(cx).canvas_language();
        }
        let Some(document) = self.document.upgrade() else {
            return;
        };
        let (revision, source, root, path) = {
            let document = document.read(cx);
            (
                document.revision,
                document.editor.read(cx).value().to_string(),
                document.vault_root.clone(),
                document.path.clone(),
            )
        };
        self.preview_context.doc_path = Some(path);
        self.preview_context.language = self.language;
        if self.source_revision == Some(revision) {
            return;
        }
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
        self.source = source.clone();
        match CanvasDocument::parse(&source) {
            Ok(model) => {
                self.error = None;
                let mut text_ids = HashSet::new();
                let mut file_status = HashMap::new();
                for node in model.nodes() {
                    let Some(id) = node["id"].as_str() else {
                        continue;
                    };
                    if node["type"].as_str() == Some("text") {
                        if let Some(text) = node["text"].as_str() {
                            text_ids.insert(id.to_string());
                            if let Some(state) = self.text_views.get(id) {
                                state.update(cx, |state, cx| state.set_text(text, cx));
                            } else {
                                let state = cx.new(|cx| TextViewState::markdown(text, cx));
                                self.text_views.insert(id.to_string(), state);
                            }
                        }
                    }
                    if let Some(path) = node["file"].as_str() {
                        let available = root
                            .as_deref()
                            .and_then(|root| crate::canvas::resolve_vault_path(root, path).ok())
                            .is_some_and(|path| path.is_file());
                        file_status.insert(id.to_string(), available);
                    }
                }
                self.text_views.retain(|id, _| text_ids.contains(id));
                self.file_status = file_status;
                self.selected
                    .retain(|id| model.node(id).is_some() || model.edge(id).is_some());
                self.model = Some(model);
            }
            Err(error) => {
                self.model = None;
                self.error = Some(error);
                self.text_views.clear();
                self.file_status.clear();
                self.selected.clear();
            }
        }
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
            (applied, document.revision)
        });
        if applied {
            self.source = after;
            self.model = Some(next);
            self.source_revision = None;
            if let Some(editing) = &mut self.editing {
                editing.last_revision = revision;
            }
            self.status = None;
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

    fn add_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_text_at(self.center_world(window), window, cx);
    }

    fn add_text_at(&mut self, position: (i64, i64), window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model.clone() else {
            return;
        };
        let mut next = model;
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
        let Some(model) = self.model.clone() else {
            return;
        };
        let mut next = model;
        if let Ok(id) = next.add_link(position.0, position.1, "https://example.invalid/") {
            self.selected.clear();
            self.selected.insert(id);
            self.apply_model(next, window, cx);
        }
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

    fn add_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model.clone() else {
            return;
        };
        let selected = self.selected.iter().cloned().collect::<Vec<_>>();
        let mut next = model;
        match next.add_group(&selected, "Group", 24) {
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
        if self.editing.is_some() && (key == "escape" || (modifiers.platform && key == "enter")) {
            self.editing = None;
            self.focus_handle.focus(window, cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.editing.is_some() {
            return;
        }
        if let Some(mut query) = self.search_query.clone() {
            match key {
                "escape" => {
                    self.search_query = None;
                    self.search_results.clear();
                    self.status = None;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                "enter" => {
                    self.refresh_search(if modifiers.shift { -1 } else { 1 }, window, cx);
                    cx.stop_propagation();
                    return;
                }
                "backspace" => {
                    query.pop();
                    self.search_query = Some(query);
                    self.search_index = 0;
                    self.refresh_search(0, window, cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
            if !modifiers.platform && !modifiers.control {
                if let Some(character) = event.keystroke.key_char.as_deref() {
                    if !character.chars().any(char::is_control) {
                        query.push_str(character);
                        self.search_query = Some(query);
                        self.search_index = 0;
                        self.refresh_search(0, window, cx);
                        cx.stop_propagation();
                        return;
                    }
                }
            }
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
            let (ax, ay) = (self.world_to_screen(a.x), self.world_to_screen_y(a.y));
            let (bx, by) = (self.world_to_screen(b.x), self.world_to_screen_y(b.y));
            let (px, py) = (f64::from(screen.x), f64::from(screen.y));
            let (dx, dy) = (f64::from(bx - ax), f64::from(by - ay));
            let length_squared = dx * dx + dy * dy;
            let t = if length_squared <= f64::EPSILON {
                0.
            } else {
                (((px - f64::from(ax)) * dx + (py - f64::from(ay)) * dy) / length_squared)
                    .clamp(0., 1.)
            };
            let distance = ((px - f64::from(ax) - t * dx).powi(2)
                + (py - f64::from(ay) - t * dy).powi(2))
            .sqrt();
            (distance <= 7.).then(|| id.to_string())
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
                node["file"].as_str().unwrap_or_default().to_string(),
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
        let relative = relative.split('#').next().unwrap_or_default();
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
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.open_document_pub(path, window, cx);
            });
        }
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

    fn render_card(
        &self,
        id: &str,
        node: &serde_json::Value,
        geometry: Geometry,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let color = match node["color"].as_str().unwrap_or_default() {
            "1" => theme.info,
            "2" => theme.success,
            "3" => theme.warning,
            "4" => theme.danger,
            "5" => theme.primary,
            "6" => theme.muted_foreground,
            _ => theme.border,
        };
        let width = (geometry.width as f32 * self.zoom as f32).max(24.);
        let height = (geometry.height as f32 * self.zoom as f32).max(24.);
        let selected = self.selected.contains(id);
        let (dx, dy, resize_dx, resize_dy) = match &self.drag {
            Some(Drag::Move { ids, dx, dy, .. }) if ids.iter().any(|node_id| node_id == id) => {
                (*dx, *dy, 0., 0.)
            }
            Some(Drag::Resize {
                id: resize_id,
                dx,
                dy,
                ..
            }) if resize_id == id => (0., 0., *dx, *dy),
            _ => (0., 0., 0., 0.),
        };
        let mut card = div()
            .id(id.to_string())
            .absolute()
            .left(px(self.world_to_screen(geometry.x as f64 + dx)))
            .top(px(self.world_to_screen_y(geometry.y as f64 + dy)))
            .w(px((width + resize_dx as f32).max(64.)))
            .h(px((height + resize_dy as f32).max(48.)))
            .rounded(theme.radius)
            .border_1()
            .border_color(if selected {
                theme.primary
            } else {
                theme.border
            })
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
        } else if node["type"].as_str() == Some("link") {
            card = card
                .child(div().text_sm().font_semibold().child("Link"))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(node["url"].as_str().unwrap_or_default().to_string()),
                );
        } else if node["type"].as_str() == Some("group") {
            card = card.bg(theme.group_box).border_color(color).child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(node["label"].as_str().unwrap_or("Group").to_string()),
            );
        } else {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "Unsupported Canvas node: {}",
                        node["type"].as_str().unwrap_or("unknown")
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
        self.sync_source(cx);
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
        let toolbar = h_flex()
            .w_full()
            .h(px(36.))
            .px_2()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new("canvas-undo")
                    .ghost()
                    .xsmall()
                    .label(language.text("Undo", "Angre"))
                    .disabled(read_only || !can_undo)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apply_document_history(false, window, cx)
                    })),
            )
            .child(
                Button::new("canvas-redo")
                    .ghost()
                    .xsmall()
                    .label(language.text("Redo", "Gjør om"))
                    .disabled(read_only || !can_redo)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apply_document_history(true, window, cx)
                    })),
            )
            .child(
                Button::new("canvas-add-text")
                    .ghost()
                    .xsmall()
                    .label(language.text("Text", "Tekst"))
                    .disabled(read_only || self.model.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.add_text(window, cx))),
            )
            .child(
                Button::new("canvas-convert-to-file")
                    .ghost()
                    .xsmall()
                    .label(language.text("To note", "Til notat"))
                    .disabled(read_only || self.selected_text_node().is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.convert_selected_text_to_file(window, cx)
                    })),
            )
            .child(
                Button::new("canvas-add-link")
                    .ghost()
                    .xsmall()
                    .label(language.text("Link", "Lenke"))
                    .disabled(read_only || self.model.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.add_link(window, cx))),
            )
            .child(
                Button::new("canvas-group")
                    .ghost()
                    .xsmall()
                    .label(language.text("Group", "Gruppe"))
                    .disabled(read_only || self.selected.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.add_group(window, cx))),
            )
            .child(
                Button::new("canvas-duplicate")
                    .ghost()
                    .xsmall()
                    .label(language.text("Duplicate", "Dupliser"))
                    .disabled(read_only || self.selected.is_empty())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.duplicate_selected(window, cx)),
                    ),
            )
            .child(
                Button::new("canvas-fit")
                    .ghost()
                    .xsmall()
                    .label(language.text("Fit all", "Tilpass alle"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.fit_selection(false, window, cx);
                    })),
            )
            .child(
                Button::new("canvas-fit-selection")
                    .ghost()
                    .xsmall()
                    .label(language.text("Fit selection", "Tilpass utvalg"))
                    .disabled(self.selected.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.fit_selection(true, window, cx);
                    })),
            )
            .child(
                Button::new("canvas-delete")
                    .ghost()
                    .xsmall()
                    .label(language.text("Delete", "Slett"))
                    .disabled(read_only || self.selected.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.delete_selected(window, cx))),
            )
            .child(div().flex_1())
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
                bar.child(div().text_xs().text_color(theme.warning).child(status))
            });

        if self.error.is_some() {
            let body = v_flex()
                .size_full()
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

        let Some(model) = self.model.clone() else {
            return v_flex().size_full().child(toolbar).into_any_element();
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
                let from_geometry = model.node_geometry(from_node)?;
                let to_geometry = model.node_geometry(to_node)?;
                let from = self.edge_anchor(from_geometry, edge["fromSide"].as_str(), to_geometry);
                let to = self.edge_anchor(to_geometry, edge["toSide"].as_str(), from_geometry);
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
                    to_arrow: edge["toEnd"].as_str() == Some("arrow"),
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
                    *last,
                ))
            }),
            _ => None,
        };
        let edge_palette = [
            theme.info,
            theme.success,
            theme.warning,
            theme.danger,
            theme.primary,
            theme.muted_foreground,
        ];
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
                        let dx = f32::from(to.x - from.x);
                        let sign = if dx < 0. { -1. } else { 1. };
                        let control = (dx.abs() * 0.5).max(24.);
                        path.cubic_bezier_to(
                            to,
                            point(from.x + px(control * sign), from.y),
                            point(to.x - px(control * sign), to.y),
                        );
                    } else {
                        path.line_to(to);
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                    if edge.from_arrow {
                        paint_edge_arrow(window, from, to, color);
                    }
                    if edge.to_arrow {
                        paint_edge_arrow(window, to, from, color);
                    }
                }
                if let Some((from, to)) = pending_edge {
                    let from = point(
                        bounds.origin.x + px(from.x as f32),
                        bounds.origin.y + px(from.y as f32),
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
                let geometry = model.node_geometry(node)?;
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
                Some(self.render_card(&id, &node, geometry, cx).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
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
                ))
            })
            .collect::<Vec<_>>();
        let board = div()
            .id("canvas-board")
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(theme.background)
            .track_focus(&self.focus_handle)
            .key_context("CanvasBoard")
            .on_key_down(cx.listener(Self::on_key_down))
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
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    if let Some(Drag::Edge { from_id, .. }) = this.drag.take() {
                        if let Some(to_id) = this.hit_node(this.window_to_board(event.position)) {
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
                    } else if let Some(Drag::Move { ids, dx, dy, .. }) = this.drag.take() {
                        if let Some(model) = this.model.clone() {
                            let mut next = model;
                            if next
                                .move_selection(&ids, dx.round() as i64, dy.round() as i64)
                                .is_ok()
                            {
                                this.apply_model(next, window, cx);
                            }
                        }
                    } else if let Some(Drag::Resize { id, dx, dy, .. }) = this.drag.take() {
                        if let Some(model) = this.model.clone() {
                            let mut next = model;
                            if let Some(geometry) =
                                next.node(&id).and_then(|node| next.node_geometry(node))
                            {
                                if next
                                    .resize_node(
                                        &id,
                                        geometry.width + (dx / this.zoom).round() as i64,
                                        geometry.height + (dy / this.zoom).round() as i64,
                                    )
                                    .is_ok()
                                {
                                    this.apply_model(next, window, cx);
                                }
                            }
                        }
                    } else if let Some(Drag::Marquee {
                        start,
                        end,
                        additive,
                    }) = this.drag.take()
                    {
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
                                let card_right =
                                    this.world_to_screen(geometry.x as f64 + geometry.width as f64);
                                let card_bottom = this
                                    .world_to_screen_y(geometry.y as f64 + geometry.height as f64);
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
                    } else {
                        this.drag = None;
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
            .child(edge_canvas)
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
                let can_group = !self.selected.is_empty();
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
                    .when(can_group, |menu| {
                        menu.item(
                            PopupMenuItem::new(language.text("Group selection", "Grupper utvalg"))
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
        v_flex()
            .size_full()
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
