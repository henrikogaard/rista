//! Project-wide text search — a small dialog: one input, results beneath.
//! Picking a hit opens the note at its first matching line.

use crate::search_service::{ReplacePlan, SearchOptions, SearchService, SearchSort};
use crate::vault::VaultEvent;
use crate::{
    actions::{SearchNextResult, SearchPreviousResult},
    app::Workspace,
};
use gpui_kit::base::Selectable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Sizable, WindowExt};
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
    pub snippet: Option<String>,
}

/// Read a compact snippet around `line` (1-based) plus context lines for the
/// right-hand search preview pane.
fn load_hit_context(
    path: &std::path::Path,
    focus_line: Option<usize>,
) -> (Option<String>, Vec<(usize, String)>) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (None, Vec::new());
    };
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return (None, Vec::new());
    }
    let target_ix = focus_line
        .map(|l| l.saturating_sub(1).min(lines.len().saturating_sub(1)))
        .unwrap_or(0);
    let snippet = lines
        .get(target_ix)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.chars().count() > 96 {
                let mut out: String = s.chars().take(95).collect();
                out.push('…');
                out
            } else {
                s.to_string()
            }
        });
    let start = target_ix.saturating_sub(4);
    let end = (target_ix + 10).min(lines.len());
    let context = lines[start..end]
        .iter()
        .enumerate()
        .map(|(offset, line)| (start + offset + 1, (*line).to_string()))
        .collect();
    (snippet, context)
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
    replace_input: Entity<InputState>,
    replace_open: bool,
    pending_replace: Option<ReplacePlan>,
    replace_message: Option<(String, bool)>,
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

        let replace_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(if norwegian {
                "Erstatt med…"
            } else {
                "Replace with…"
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
                if let VaultEvent::Files { changed } = event {
                    match changed {
                        Some(paths) => this.service.invalidate(paths.iter().cloned()),
                        None => this.service.invalidate_all(),
                    }
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
            replace_input,
            replace_open: false,
            pending_replace: None,
            replace_message: None,
            selected_index: 0,
            results_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn schedule_search(&mut self, cx: &mut Context<Self>) {
        // A plan taken for one query must not survive into another: the
        // query field, case toggle and sort all funnel through here.
        self.pending_replace = None;
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
                            .map(|hit| {
                                let (snippet, _) = load_hit_context(&hit.path, hit.first_line);
                                SearchHit {
                                    sample: hit.relative_path.clone(),
                                    path: hit.path,
                                    count: hit.count,
                                    first_line: hit.first_line,
                                    snippet,
                                }
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
        let query = self.query_input.read(cx).value().trim().to_string();
        let bookmark_root = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).vault_entity().read(cx).root.clone());
        let selected_index = self.selected_index;
        let explanation = self.explanation.clone();
        let explanation_open = self.explanation_open;
        let replace_open = self.replace_open;
        let pending_replace = self.pending_replace.clone();
        let replace_message = self.replace_message.clone();
        let replace_input = self.replace_input.clone();
        let replace_workspace = self.workspace.clone();
        let search_entity = this.clone();
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
                // One-click search operator filter chips — styled as tactile mineral
                // pills distinct from the action bar below.
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_1p5()
                    .pb_1()
                    .children(
                        [
                            ("tag:", if norwegian { "emneknagg" } else { "tag" }),
                            ("path:", if norwegian { "mappe" } else { "folder" }),
                            ("file:", if norwegian { "filnavn" } else { "file" }),
                            ("task:", if norwegian { "oppgave" } else { "task" }),
                            ("-", if norwegian { "utelat" } else { "exclude" }),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(ix, (op, hint))| {
                            let active = query.contains(op);
                            let theme = cx.theme();
                            div()
                                .id(("search-chip", ix))
                                .px_1p5()
                                .py_0p5()
                                .rounded(px(5.))
                                .border_1()
                                .border_color(if active {
                                    theme.accent.opacity(0.6)
                                } else {
                                    theme.border.opacity(0.6)
                                })
                                .bg(if active {
                                    theme.accent.opacity(0.16)
                                } else {
                                    theme.muted.opacity(0.5)
                                })
                                .hover(|s| s.bg(theme.muted.opacity(0.85)))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_family(theme.mono_font_family.clone())
                                        .text_color(if active {
                                            theme.accent
                                        } else {
                                            theme.foreground
                                        })
                                        .child(op),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(hint),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let current = this.query_input.read(cx).value().to_string();
                                    let trimmed = current.trim_end();
                                    let next = if trimmed.is_empty() {
                                        op.to_string()
                                    } else {
                                        format!("{trimmed} {op}")
                                    };
                                    this.query_input.update(cx, |input, cx| {
                                        input.set_value(next, window, cx);
                                        input.focus(window, cx);
                                    });
                                    this.schedule_search(cx);
                                }))
                        }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("bookmark-search")
                            .ghost()
                            .xsmall()
                            .label(if norwegian {
                                "Bokmerk søket"
                            } else {
                                "Bookmark search"
                            })
                            .disabled(query.is_empty() || bookmark_root.is_none())
                            .on_click({
                                let workspace = self.workspace.clone();
                                let query = query.clone();
                                move |_, _, cx| {
                                    if let Some(workspace) = workspace.upgrade() {
                                        workspace.update(cx, |workspace, cx| {
                                            workspace.bookmark_search(
                                                query.clone(),
                                                match_case,
                                                sort,
                                                cx,
                                            );
                                        });
                                    }
                                }
                            }),
                    )
                    .child(
                        Button::new("search-match-case")
                            .ghost()
                            .xsmall()
                            .label(if norwegian {
                                "Skil store/små"
                            } else {
                                "Match case"
                            })
                            .selected(match_case)
                            .toggled(match_case)
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
                    )
                    .child(
                        Button::new("search-replace-toggle")
                            .ghost()
                            .xsmall()
                            .label(if replace_open {
                                if norwegian {
                                    "Skjul erstatning"
                                } else {
                                    "Hide replace"
                                }
                            } else if norwegian {
                                "Erstatt"
                            } else {
                                "Replace"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.replace_open = !this.replace_open;
                                this.pending_replace = None;
                                this.replace_message = None;
                                cx.notify();
                            })),
                    ),
            )
            .when(replace_open, |this| {
                this.child(match pending_replace.as_ref() {
                    Some(plan) => {
                        let files = plan.files.len();
                        let total = plan.total;
                        let confirm_plan = plan.clone();
                        let cancel_view = search_entity.clone();
                        let confirm_view = search_entity.clone();
                        let confirm_workspace = replace_workspace.clone();
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if norwegian {
                                        format!("Erstatt {total} forekomster i {files} notater?")
                                    } else {
                                        format!("Replace {total} occurrences across {files} notes?")
                                    }),
                            )
                            .child(
                                Button::new("replace-cancel")
                                    .ghost()
                                    .xsmall()
                                    .label(if norwegian { "Avbryt" } else { "Cancel" })
                                    .on_click(move |_, _, cx| {
                                        cancel_view.update(cx, |search, cx| {
                                            search.pending_replace = None;
                                            search.replace_message = None;
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(
                                Button::new("replace-confirm")
                                    .primary()
                                    .xsmall()
                                    .label(if norwegian { "Erstatt" } else { "Replace" })
                                    .on_click(move |_, window, cx| {
                                        let plan = confirm_plan.clone();
                                        let Some(workspace) = confirm_workspace.upgrade() else {
                                            return;
                                        };
                                        let result = workspace.update(cx, |workspace, cx| {
                                            workspace.apply_replace_in_project(&plan, window, cx)
                                        });
                                        confirm_view.update(cx, |search, cx| {
                                            match result {
                                                Ok(message) => {
                                                    search.pending_replace = None;
                                                    search.replace_message = Some((message, false));
                                                    search.schedule_search(cx);
                                                }
                                                Err(message) => {
                                                    search.replace_message = Some((message, true));
                                                }
                                            }
                                            cx.notify();
                                        });
                                    }),
                            )
                    }
                    None => h_flex()
                        .gap_1()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&replace_input).appearance(true)),
                        )
                        .child(
                            Button::new("replace-all")
                                .xsmall()
                                .label(if norwegian {
                                    "Erstatt alle"
                                } else {
                                    "Replace all"
                                })
                                .disabled(query.is_empty())
                                .on_click({
                                    let workspace = replace_workspace.clone();
                                    let input = replace_input.clone();
                                    let view = search_entity.clone();
                                    let query = query.clone();
                                    move |_, _, cx| {
                                        let replacement = input.read(cx).value().to_string();
                                        let Some(workspace) = workspace.upgrade() else {
                                            return;
                                        };
                                        let result = workspace.read(cx).plan_replace_in_project(
                                            &query,
                                            &replacement,
                                            match_case,
                                            cx,
                                        );
                                        view.update(cx, |search, cx| {
                                            match result {
                                                Ok(plan) => {
                                                    search.pending_replace = Some(plan);
                                                    search.replace_message = None;
                                                }
                                                Err(message) => {
                                                    search.pending_replace = None;
                                                    search.replace_message = Some((message, true));
                                                }
                                            }
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                })
            })
            .when_some(replace_message, |this, (message, is_error)| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(if is_error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(message),
                )
            })
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
                            h_flex().child(
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
                            ),
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
            .child({
                let selected_hit = results.get(selected_index).cloned();
                let preview_pane = selected_hit.as_ref().map(|hit| {
                    let (_, ctx_lines) = load_hit_context(&hit.path, hit.first_line);
                    let focus_line = hit.first_line;
                    let note_title = hit
                        .path
                        .file_stem()
                        .or_else(|| hit.path.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let open_hit_btn = hit.clone();
                    let open_this = this.clone();
                    let theme = cx.theme();
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h(px(360.))
                        .rounded(theme.radius)
                        .bg(theme.muted.opacity(0.28))
                        .border_1()
                        .border_color(theme.border.opacity(0.6))
                        .overflow_hidden()
                        .child(
                            h_flex()
                                .w_full()
                                .px_3()
                                .py_2()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .border_b_1()
                                .border_color(theme.border.opacity(0.5))
                                .child(
                                    v_flex()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_sm()
                                                .truncate()
                                                .text_color(theme.foreground)
                                                .child(note_title),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .truncate()
                                                .text_color(theme.muted_foreground)
                                                .child(hit.sample.clone()),
                                        ),
                                )
                                .child(
                                    Button::new("search-preview-open")
                                        .outline()
                                        .xsmall()
                                        .label(if let Some(line) = focus_line {
                                            if norwegian {
                                                format!("Åpne L{line}")
                                            } else {
                                                format!("Open L{line}")
                                            }
                                        } else if norwegian {
                                            "Åpne".to_string()
                                        } else {
                                            "Open".to_string()
                                        })
                                        .on_click(move |_, window, cx| {
                                            open_this.update(cx, |search, cx| {
                                                search.open_hit(&open_hit_btn, window, cx);
                                            });
                                        }),
                                ),
                        )
                        .child(
                            v_flex()
                                .id("search-preview-lines")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .p_2()
                                .gap_0p5()
                                .children(ctx_lines.into_iter().map(|(line_no, line_text)| {
                                    let is_match = focus_line == Some(line_no);
                                    h_flex()
                                        .w_full()
                                        .px_2()
                                        .py_0p5()
                                        .gap_2()
                                        .rounded(px(4.))
                                        .when(is_match, |row| row.bg(theme.accent.opacity(0.24)))
                                        .child(
                                            div()
                                                .w(px(28.))
                                                .flex_shrink_0()
                                                .text_xs()
                                                .text_color(theme.muted_foreground.opacity(0.65))
                                                .child(format!("{line_no}")),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_xs()
                                                .text_color(if is_match {
                                                    theme.foreground
                                                } else {
                                                    theme.muted_foreground
                                                })
                                                .child(if line_text.is_empty() {
                                                    " ".to_string()
                                                } else {
                                                    line_text
                                                }),
                                        )
                                })),
                        )
                });
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_3()
                    .child(
                        v_flex()
                            .id("project-search-results")
                            .when(preview_pane.is_some(), |col| {
                                col.w(px(320.)).flex_shrink_0()
                            })
                            .when(preview_pane.is_none(), |col| col.w_full())
                            .track_scroll(&self.results_scroll)
                            .gap_0p5()
                            .children(results.iter().enumerate().map(|(ix, hit)| {
                                let name = hit
                                    .path
                                    .file_stem()
                                    .or_else(|| hit.path.file_name())
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                let theme = cx.theme();
                                let subtitle = match (&hit.snippet, hit.first_line) {
                                    (Some(snip), Some(line)) => format!("L{line} · {snip}"),
                                    (Some(snip), None) => snip.clone(),
                                    (None, _) => hit.sample.clone(),
                                };
                                ListItem::new(ix)
                                    .w_full()
                                    .px_2p5()
                                    .py_1p5()
                                    .rounded(theme.radius)
                                    .when(ix == selected_index, |this| this.bg(theme.secondary))
                                    .child(
                                        v_flex()
                                            .w_full()
                                            .gap_0p5()
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .truncate()
                                                            .text_color(theme.foreground)
                                                            .child(name),
                                                    )
                                                    .child(
                                                        div()
                                                            .px_1p5()
                                                            .py(px(1.))
                                                            .rounded(px(999.))
                                                            .bg(theme.muted.opacity(0.65))
                                                            .text_xs()
                                                            .text_color(theme.muted_foreground)
                                                            .child(format!("{}", hit.count)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.muted_foreground)
                                                    .truncate()
                                                    .child(subtitle),
                                            ),
                                    )
                                    .on_click({
                                        let this = this.clone();
                                        let hit = hit.clone();
                                        move |ev, window, cx| {
                                            if ev.click_count() >= 2 {
                                                this.update(cx, |search, cx| {
                                                    search.open_hit(&hit, window, cx);
                                                });
                                            } else {
                                                this.update(cx, |search, cx| {
                                                    if search.selected_index == ix {
                                                        search.open_hit(&hit, window, cx);
                                                    } else {
                                                        search.selected_index = ix;
                                                        cx.notify();
                                                    }
                                                });
                                            }
                                        }
                                    })
                                    .into_any_element()
                            }))
                            .max_h(px(360.))
                            .overflow_y_scroll(),
                    )
                    .when_some(preview_pane, |row, pane| row.child(pane))
            })
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
        show_project_search(workspace, query, None, window, cx);
    });
}

pub fn open_project_search_saved(
    workspace: Entity<Workspace>,
    query: String,
    match_case: bool,
    sort: SearchSort,
    window: &mut Window,
    cx: &mut App,
) {
    window.defer(cx, move |window, cx| {
        show_project_search(workspace, Some(query), Some((match_case, sort)), window, cx);
    });
}

fn show_project_search(
    workspace: Entity<Workspace>,
    query: Option<String>,
    options: Option<(bool, SearchSort)>,
    window: &mut Window,
    cx: &mut App,
) {
    let search = cx.new(|cx| ProjectSearch::new(workspace.downgrade(), window, cx));
    if let Some((match_case, sort)) = options {
        search.update(cx, |search, _cx| {
            search.match_case = match_case;
            search.sort = sort;
        });
    }
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
            .w(px(780.))
            .overlay_closable(true)
            // PressEnter opens the hit; the dialog must not also restore its old focus.
            .on_ok(|_, _, _| false)
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
