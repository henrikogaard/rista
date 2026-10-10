use crate::{
    app::Workspace,
    search_service::{SearchOptions, SearchResult, SearchService},
    settings::Language,
    vault::{Vault, VaultEvent},
};
use gpui_kit::{
    component::{h_flex, v_flex, ActiveTheme},
    prelude::{FluentBuilder, InteractiveElement},
    *,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

pub struct QueryView {
    query: String,
    root: PathBuf,
    paths: Arc<Vec<PathBuf>>,
    workspace: WeakEntity<Workspace>,
    service: SearchService,
    results: Vec<SearchResult>,
    total: usize,
    more: bool,
    busy: bool,
    error: Option<String>,
    generation: u64,
    cancellation: Arc<AtomicU64>,
    active: bool,
    _subscription: Subscription,
}

impl QueryView {
    pub fn new(
        query: String,
        vault: Entity<Vault>,
        workspace: WeakEntity<Workspace>,
        cx: &mut Context<Self>,
    ) -> Self {
        let root = vault.read(cx).root.clone().unwrap_or_default();
        let paths = vault.read(cx).explorer_path_snapshot();
        let service = workspace
            .upgrade()
            .map(|workspace| workspace.read(cx).search_service())
            .unwrap_or_default();
        let subscription = cx.subscribe(&vault, |this, vault, event: &VaultEvent, cx| {
            if matches!(event, VaultEvent::Files) && this.active {
                this.root = vault.read(cx).root.clone().unwrap_or_default();
                this.paths = vault.read(cx).explorer_path_snapshot();
                this.start(cx);
            }
        });
        Self {
            query: query.clone(),
            root,
            paths,
            workspace,
            service,
            results: Vec::new(),
            total: 0,
            more: false,
            busy: true,
            error: None,
            generation: 0,
            cancellation: Arc::new(AtomicU64::new(0)),
            active: true,
            _subscription: subscription,
        }
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let cancellation_generation = self.cancellation.fetch_add(1, Ordering::Relaxed) + 1;
        let query = self.query.clone();
        self.busy = true;
        self.error = None;
        self.results.clear();
        self.total = 0;
        self.more = false;
        cx.notify();
        let root = self.root.clone();
        let paths = self.paths.clone();
        let workspace = self.workspace.clone();
        let service = self.service.clone();
        let cancellation = self.cancellation.clone();
        let language = workspace
            .upgrade()
            .map(|workspace| workspace.read(cx).language())
            .unwrap_or(Language::English);
        let task = cx.background_executor().spawn(async move {
            smol::unblock(move || {
                service.search_cancellable(
                    &root,
                    paths.as_slice(),
                    &query,
                    SearchOptions {
                        limit: 6,
                        ..SearchOptions::default()
                    },
                    Some((cancellation, cancellation_generation)),
                )
            })
            .await
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = task.await;
            let _ = this.update(&mut *cx, |view, cx| {
                if !view.active || view.generation != generation {
                    return;
                }
                view.busy = false;
                match result {
                    Ok(response) => {
                        view.results = response.results;
                        view.total = response.total;
                        view.more = response.more;
                    }
                    Err(error) => {
                        view.error =
                            Some(error.message(language == Language::Norwegian).to_string())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.busy = false;
        self.generation = self.generation.wrapping_add(1);
        self.cancellation.fetch_add(1, Ordering::Relaxed);
    }
}

impl Drop for QueryView {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Render for QueryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let results = self.results.clone();
        let workspace = self.workspace.clone();
        let norwegian = self
            .workspace
            .upgrade()
            .is_some_and(|workspace| workspace.read(cx).language() == Language::Norwegian);
        v_flex()
            .w_full()
            .gap_1()
            .px_2()
            .py_1()
            .bg(cx.theme().secondary)
            .rounded(cx.theme().radius)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{}: {}",
                        if norwegian { "spørring" } else { "query" },
                        self.query
                    )),
            )
            .when(self.busy, |this| {
                this.child(div().text_xs().child(if norwegian {
                    "Søker…"
                } else {
                    "Searching…"
                }))
            })
            .when(
                !self.busy && self.error.is_none() && self.total == 0,
                |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if norwegian {
                                "Ingen treff"
                            } else {
                                "No results"
                            }),
                    )
                },
            )
            .when(self.more, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if norwegian {
                            format!("Viser {} av {} treff", results.len(), self.total)
                        } else {
                            format!("Showing {} of {} results", results.len(), self.total)
                        }),
                )
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_xs().text_color(cx.theme().danger).child(error))
            })
            .children(results.iter().map(|result| {
                let path: PathBuf = result.path.clone();
                let line = result.first_line;
                let workspace = workspace.clone();
                div()
                    .id(result.path.to_string_lossy().to_string())
                    .w_full()
                    .px_1()
                    .py_0p5()
                    .cursor_pointer()
                    .hover(|this| this.bg(cx.theme().muted))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .child(div().text_sm().child(result.relative_path.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if norwegian {
                                        format!("{} treff", result.count)
                                    } else {
                                        format!("{} matches", result.count)
                                    }),
                            ),
                    )
                    .on_click(move |_, window, cx| {
                        let _ = workspace.update(cx, |workspace, cx| {
                            workspace.navigate_search_result(path.clone(), line, window, cx);
                        });
                    })
            }))
    }
}
