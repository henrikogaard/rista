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
        let query_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search… (tag:, path:, file:, task: filters)")
        });

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
        let vault = workspace.read(cx).vault_entity().read(cx);
        let notes = vault.notes.clone();
        let root = vault.root.clone();
        // Obsidian search operators: `tag:`/`path:`/`file:`/`task:`/
        // `content:` tokens filter per-note; every term must match (AND).
        // Non-operator words rejoin into one content needle so multiword
        // queries still substring-match verbatim.
        let mut ops: Vec<(String, String)> = Vec::new();
        let mut rest: Vec<&str> = Vec::new();
        for tok in query.split_whitespace() {
            let known = tok
                .split_once(':')
                .filter(|(k, v)| {
                    !v.is_empty()
                        && matches!(
                            k.to_ascii_lowercase().as_str(),
                            "tag" | "path" | "file" | "task" | "content" | "line"
                        )
                })
                .map(|(k, v)| (k.to_ascii_lowercase(), v.to_lowercase()));
            if let Some(op) = known {
                ops.push(op);
            } else {
                rest.push(tok);
            }
        }
        if !rest.is_empty() {
            ops.push(("content".into(), rest.join(" ").to_lowercase()));
        }
        if ops.is_empty() {
            cx.notify();
            return;
        }
        let mut results = Vec::new();
        for path in notes {
            let rel = root
                .as_ref()
                .and_then(|r| path.strip_prefix(r).ok())
                .unwrap_or(&path)
                .to_string_lossy()
                .to_lowercase();
            let file = path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let need_content = ops
                .iter()
                .any(|(k, _)| matches!(k.as_str(), "tag" | "task" | "content" | "line"));
            let content = if need_content {
                std::fs::read_to_string(&path).unwrap_or_default()
            } else {
                String::new()
            };
            let mut count = 0usize;
            let mut sample = String::new();
            let mut all = true;
            for (kind, needle) in &ops {
                match kind.as_str() {
                    "path" => {
                        if rel.contains(needle) {
                            count += 1;
                            if sample.is_empty() {
                                sample = rel.clone();
                            }
                        } else {
                            all = false;
                            break;
                        }
                    }
                    "file" => {
                        if file.contains(needle) {
                            count += 1;
                            if sample.is_empty() {
                                sample = rel.clone();
                            }
                        } else {
                            all = false;
                            break;
                        }
                    }
                    "tag" => {
                        // `tag:#a` also matches nested `#a/b` (Obsidian).
                        let arg = needle.trim_start_matches('#');
                        let hit = crate::properties::note_tags(&content)
                            .iter()
                            .any(|t| t == arg || t.starts_with(&format!("{arg}/")));
                        if hit {
                            count += 1;
                            if sample.is_empty() {
                                sample = format!("#{arg}");
                            }
                        } else {
                            all = false;
                            break;
                        }
                    }
                    "task" => {
                        let mut hits = 0;
                        for line in content.lines() {
                            let t = line.trim_start();
                            let is_task = t.len() > 4
                                && matches!(t.as_bytes()[0], b'-' | b'*' | b'+')
                                && t[1..].starts_with(" [");
                            if is_task && t.to_lowercase().contains(needle) {
                                hits += 1;
                                if sample.is_empty() {
                                    sample = t.chars().take(120).collect();
                                }
                            }
                        }
                        if hits == 0 {
                            all = false;
                            break;
                        }
                        count += hits;
                    }
                    _ => {
                        // `content:` (and `line:` — we search line-wise
                        // either way, so they behave the same here).
                        let mut hits = 0;
                        for line in content.lines() {
                            let lower = line.to_lowercase();
                            let mut offset = 0usize;
                            while let Some(pos) = lower[offset..].find(needle.as_str()) {
                                hits += 1;
                                if sample.is_empty() {
                                    sample = line.trim().chars().take(120).collect();
                                }
                                offset += pos + needle.len();
                            }
                            if hits > 200 {
                                break;
                            }
                        }
                        if hits == 0 {
                            all = false;
                            break;
                        }
                        count += hits;
                    }
                }
            }
            if all && count > 0 {
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
    open_project_search_for(workspace, None, window, cx);
}

/// As [`open_project_search`], with the query field pre-filled.
pub fn open_project_search_for(
    workspace: Entity<Workspace>,
    query: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let search = cx.new(|cx| ProjectSearch::new(workspace.downgrade(), window, cx));
    let input = search.read(cx).query_input.clone();
    let seeded = search.clone();
    let query = query.map(|q| q.to_string());
    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title("Find in project")
            .w(px(560.))
            .overlay_closable(true)
            .child(search.clone())
    });
    window.defer(cx, move |window, cx| {
        input.update(cx, |input, cx| {
            if let Some(query) = &query {
                input.set_value(query.clone(), window, cx);
            }
            input.focus(window, cx);
        });
        if query.is_some() {
            // set_value suppresses Change — run the search explicitly.
            seeded.update(cx, |search, cx| search.recompute(cx));
        }
    });
}
