//! Project-wide text search — a small dialog: one input, results beneath.
//! Picking a hit opens the note and arms the editor's built-in find.

use crate::app::Workspace;
use crate::document::Document;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, WindowExt};
use gpui_kit::*;
use std::path::PathBuf;

#[derive(Clone)]
pub struct SearchHit {
    pub path: PathBuf,
    pub count: usize,
    pub sample: String,
}

pub struct ProjectSearch {
    query_input: Entity<InputState>,
    workspace: WeakEntity<Workspace>,
    results: Vec<SearchHit>,
    _subscriptions: Vec<Subscription>,
}

impl ProjectSearch {
    fn new(workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search across every note…"));

        let sub = cx.subscribe_in(&query_input, window, |this, _input, event, _window, cx| {
            if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                this.recompute(cx);
            }
        });

        Self {
            query_input,
            workspace,
            results: Vec::new(),
            _subscriptions: vec![sub],
        }
    }

    fn recompute(&mut self, cx: &mut Context<Self>) {
        let query = self.query_input.read(cx).value().to_string();
        let query = query.trim();
        self.results.clear();
        if query.len() < 2 {
            cx.notify();
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let notes = workspace.read(cx).vault_entity().read(cx).notes.clone();
        let needle = query.to_lowercase();
        let mut results = Vec::new();
        for path in notes {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut count = 0usize;
            let mut sample = String::new();
            for line in content.lines() {
                let lower = line.to_lowercase();
                let mut offset = 0usize;
                while let Some(pos) = lower[offset..].find(&needle) {
                    count += 1;
                    if sample.is_empty() {
                        sample = line.trim().chars().take(120).collect();
                    }
                    offset += pos + needle.len();
                }
                if count > 200 {
                    break;
                }
            }
            if count > 0 {
                results.push(SearchHit {
                    path,
                    count,
                    sample,
                });
            }
            if results.len() >= 50 {
                break;
            }
        }
        self.results = results;
        cx.notify();
    }

    fn open_hit(&self, hit: &SearchHit, window: &mut Window, cx: &mut App) {
        let query = self.query_input.read(cx).value().to_string();
        let path = hit.path.clone();
        let _ = self.workspace.update(cx, |ws, cx| {
            ws.open_document_pub(path, window, cx);
            // Arm the editor's find so matches highlight and ⌘F steps through.
            if let Some(doc) = ws.iter_docs().find(|d| d.read(cx).path == hit.path) {
                doc.update(cx, |doc: &mut Document, cx| {
                    doc.editor.update(cx, |editor, cx| {
                        editor.set_search_query(query.clone(), true, cx);
                    });
                });
            }
        });
        window.close_dialog(cx);
    }
}

impl Render for ProjectSearch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let results = self.results.clone();
        let this = cx.entity();
        v_flex()
            .w_full()
            .gap_2()
            .child(Input::new(&self.query_input).appearance(true))
            .child(
                v_flex()
                    .w_full()
                    .children(results.iter().enumerate().map(|(ix, hit)| {
                        let name = hit
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        ListItem::new(ix)
                            .w_full()
                            .px_2()
                            .py_1()
                            .rounded(cx.theme().radius)
                            .child(
                                v_flex()
                                    .w_full()
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .justify_between()
                                            .child(div().text_sm().child(name))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(format!("{}", hit.count)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .truncate()
                                            .child(hit.sample.clone()),
                                    ),
                            )
                            .on_click({
                                let this = this.clone();
                                let hit = hit.clone();
                                move |_, window, cx| {
                                    this.update(cx, |search, cx| {
                                        search.open_hit(&hit, window, cx);
                                    });
                                }
                            })
                            .into_any_element()
                    })),
            )
    }
}

/// Open the project-search dialog from anywhere with a workspace handle.
pub fn open_project_search(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
    let search = cx.new(|cx| ProjectSearch::new(workspace.downgrade(), window, cx));
    let input = search.read(cx).query_input.clone();
    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title("Find in project")
            .w(px(560.))
            .overlay_closable(true)
            .child(search.clone())
    });
    window.defer(cx, move |window, cx| {
        input.update(cx, |input, cx| input.focus(window, cx));
    });
}
