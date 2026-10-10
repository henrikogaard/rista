//! Project-wide text search — a small dialog: one input, results beneath.
//! Picking a hit opens the note at its first matching line.

use crate::search_service::{SearchOptions, SearchService, SearchSort};
use crate::vault::VaultEvent;
use crate::{
    actions::{SearchNextResult, SearchPreviousResult},
    app::Workspace,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable, WindowExt};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Clone)]
pub struct SearchHit {
    pub path: PathBuf,
    pub count: usize,
    pub sample: String,
    pub first_line: Option<usize>,
}

pub struct ProjectSearch {
    query_input: Entity<InputState>,
    workspace: WeakEntity<Workspace>,
    results: Vec<SearchHit>,
    service: SearchService,
    generation: u64,
    cancellation: Arc<AtomicU64>,
    busy: bool,
    error: Option<String>,
    match_case: bool,
    sort: SearchSort,
    total: usize,
    more: bool,
    explanation: Option<String>,
    explanation_open: bool,
    selected_index: usize,
    results_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl ProjectSearch {
    fn new(workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let norwegian = workspace.upgrade().is_some_and(|workspace| {
            workspace.read(cx).language() == crate::settings::Language::Norwegian
        });
        let query_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(if norwegian {
                "Søk… (tag:, path:, file:, task:, -term utelater)"
            } else {
                "Search… (tag:, path:, file:, task:, -term negates)"
            })
        });

        let sub = cx.subscribe_in(&query_input, window, |this, _input, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.schedule_search(cx);
            } else if matches!(event, InputEvent::PressEnter { .. }) {
                if let Some(hit) = this.results.get(this.selected_index).cloned() {
                    this.open_hit(&hit, window, cx);
                }
            }
        });
        let mut subscriptions = vec![sub];
        let service = workspace
            .upgrade()
            .map(|workspace| workspace.read(cx).search_service())
            .unwrap_or_default();
        if let Some(workspace_entity) = workspace.upgrade() {
            let vault = workspace_entity.read(cx).vault_entity().clone();
            subscriptions.push(cx.subscribe(&vault, |this, _, event: &VaultEvent, cx| {
                if matches!(event, VaultEvent::Files) {
                    this.service.invalidate_all();
                    this.schedule_search(cx);
                }
            }));
        }

        Self {
            query_input,
            workspace,
            results: Vec::new(),
            service,
            generation: 0,
            cancellation: Arc::new(AtomicU64::new(0)),
            busy: false,
            error: None,
            match_case: false,
            sort: SearchSort::Relevance,
            total: 0,
            more: false,
            explanation: None,
            explanation_open: false,
            selected_index: 0,
            results_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let cancellation_generation = self.cancellation.fetch_add(1, Ordering::Relaxed) + 1;
        let query = self.query_input.read(cx).value().trim().to_string();
        self.busy = !query.is_empty();
        self.error = None;
        self.explanation = None;
        self.results.clear();
        self.total = 0;
        self.more = false;
        self.selected_index = 0;
        if query.is_empty() {
            cx.notify();
            return;
        }
        cx.notify();
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let vault = workspace.read(cx).vault_entity().read(cx);
        let root = vault.root.clone().unwrap_or_default();
        let paths = vault.explorer_path_snapshot();
        let service = self.service.clone();
        let cancellation = self.cancellation.clone();
        let language = workspace.read(cx).language();
        let match_case = self.match_case;
        let sort = self.sort;
        let task = cx.background_executor().spawn(async move {
            smol::Timer::after(std::time::Duration::from_millis(100)).await;
            smol::unblock(move || {
                service.search_cancellable(
                    &root,
                    paths.as_slice(),
                    &query,
                    SearchOptions {
                        match_case,
                        sort,
                        limit: 100,
                    },
                    Some((cancellation, cancellation_generation)),
                )
            })
            .await
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = task.await;
            let _ = this.update(&mut *cx, |search, cx| {
                if search.generation != generation {
                    return;
                }
                search.busy = false;
                match result {
                    Ok(response) => {
                        search.results = response
                            .results
                            .into_iter()
                            .map(|hit| SearchHit {
                                sample: hit.relative_path.clone(),
                                path: hit.path,
                                count: hit.count,
                                first_line: hit.first_line,
                            })
                            .collect();
                        search.total = response.total;
                        search.more = response.more;
                        search.explanation =
                            Some(if language == crate::settings::Language::Norwegian {
                                response.explanation_norwegian
                            } else {
                                response.explanation
                            });
                        search.selected_index = 0;
                    }
                    Err(error) => {
                        search.error = Some(
                            error
                                .message(language == crate::settings::Language::Norwegian)
                                .to_string(),
                        )
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_hit(&self, hit: &SearchHit, window: &mut Window, cx: &mut App) {
        self.cancellation.fetch_add(1, Ordering::Relaxed);
        let path = hit.path.clone();
        let line = hit.first_line;
        let workspace = self.workspace.clone();
        window.close_dialog(cx);
        window.defer(cx, move |window, cx| {
            let _ = workspace.update(cx, |workspace, cx| {
                workspace.navigate_search_result(path, line, window, cx);
            });
        });
    }

    fn on_search_next(
        &mut self,
        _: &SearchNextResult,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.results.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.results.len() - 1);
            self.results_scroll.scroll_to_item(self.selected_index);
            cx.notify();
        }
    }

    fn on_search_previous(
        &mut self,
        _: &SearchPreviousResult,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_index = self.selected_index.saturating_sub(1);
        self.results_scroll.scroll_to_item(self.selected_index);
        cx.notify();
    }
}

impl Render for ProjectSearch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let results = self.results.clone();
        let this = cx.entity();
        let norwegian = self.workspace.upgrade().is_some_and(|workspace| {
            workspace.read(cx).language() == crate::settings::Language::Norwegian
        });
        let match_case = self.match_case;
        let sort = self.sort;
        let selected_index = self.selected_index;
        let explanation = self.explanation.clone();
        let explanation_open = self.explanation_open;
        let status = if self.busy {
            Some(
                if norwegian {
                    "Søker…"
                } else {
                    "Searching…"
                }
                .to_string(),
            )
        } else if let Some(error) = self.error.as_deref() {
            Some(error.to_string())
        } else if self.total == 0 && !self.query_input.read(cx).value().is_empty() {
            Some(
                if norwegian {
                    "Ingen treff"
                } else {
                    "No results"
                }
                .to_string(),
            )
        } else {
            None
        };
        let status_is_error = self.error.is_some();
        v_flex()
            .key_context("ProjectSearch")
            .on_action(cx.listener(Self::on_search_next))
            .on_action(cx.listener(Self::on_search_previous))
            .w_full()
            .gap_2()
            .child(Input::new(&self.query_input).appearance(true))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("search-match-case")
                            .ghost()
                            .xsmall()
                            .label(if match_case {
                                if norwegian {
                                    "Skil store/små"
                                } else {
                                    "Match case"
                                }
                            } else if norwegian {
                                "Ignorer store/små"
                            } else {
                                "Ignore case"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.match_case = !this.match_case;
                                this.schedule_search(cx);
                            })),
                    )
                    .child(
                        Button::new("search-sort")
                            .ghost()
                            .xsmall()
                            .label(match sort {
                                SearchSort::Relevance => {
                                    if norwegian {
                                        "Relevans"
                                    } else {
                                        "Relevance"
                                    }
                                }
                                SearchSort::Filename => {
                                    if norwegian {
                                        "Filnavn"
                                    } else {
                                        "Filename"
                                    }
                                }
                                SearchSort::Modified => {
                                    if norwegian {
                                        "Endret"
                                    } else {
                                        "Modified"
                                    }
                                }
                                SearchSort::Created => {
                                    if norwegian {
                                        "Opprettet"
                                    } else {
                                        "Created"
                                    }
                                }
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort = match this.sort {
                                    SearchSort::Relevance => SearchSort::Filename,
                                    SearchSort::Filename => SearchSort::Modified,
                                    SearchSort::Modified => SearchSort::Created,
                                    SearchSort::Created => SearchSort::Relevance,
                                };
                                this.schedule_search(cx);
                            })),
                    ),
            )
            .when_some(status, |this, status| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(if status_is_error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(status.to_string()),
                )
            })
            .when(self.total > 0, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.more {
                            if norwegian {
                                format!("Viser {} av {} treff", results.len(), self.total)
                            } else {
                                format!("Showing {} of {} results", results.len(), self.total)
                            }
                        } else if norwegian {
                            format!("{} treff", self.total)
                        } else {
                            format!("{} results", self.total)
                        }),
                )
            })
            .when_some(explanation, |this, explanation| {
                this.child(
                    v_flex()
                        .gap_1()
                        .child(
                            Button::new("search-explain")
                                .ghost()
                                .xsmall()
                                .label(if explanation_open {
                                    if norwegian {
                                        "Skjul forklaring"
                                    } else {
                                        "Hide explanation"
                                    }
                                } else if norwegian {
                                    "Forklar søket"
                                } else {
                                    "Explain query"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.explanation_open = !this.explanation_open;
                                    cx.notify();
                                })),
                        )
                        .when(explanation_open, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(explanation),
                            )
                        }),
                )
            })
            .child(
                v_flex()
                    .id("project-search-results")
                    .w_full()
                    .track_scroll(&self.results_scroll)
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
                            .when(ix == selected_index, |this| this.bg(cx.theme().secondary))
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
                    }))
                    .max_h(px(360.))
                    .overflow_y_scroll(),
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
    let query = query.map(str::to_owned);
    window.defer(cx, move |window, cx| {
        show_project_search(workspace, query, window, cx);
    });
}

fn show_project_search(
    workspace: Entity<Workspace>,
    query: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let search = cx.new(|cx| ProjectSearch::new(workspace.downgrade(), window, cx));
    let input = search.read(cx).query_input.clone();
    let seeded = search.clone();
    let norwegian = workspace.read(cx).language() == crate::settings::Language::Norwegian;
    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog
            .title(if norwegian {
                "Søk i prosjektet"
            } else {
                "Find in project"
            })
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
            seeded.update(cx, |search, cx| search.schedule_search(cx));
        }
    });
}

impl Drop for ProjectSearch {
    fn drop(&mut self) {
        self.cancellation.fetch_add(1, Ordering::Relaxed);
    }
}
