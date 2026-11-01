//! The Page history dialog: browse a document's saved snapshots, preview what
//! restoring one would change, and restore it.

use super::*;
use crate::document::Document;

pub struct HistoryView {
    doc: Entity<Document>,
    path: PathBuf,
    snapshots: Vec<(u64, PathBuf)>,
    selected: Option<usize>,
    language: Language,
}

impl HistoryView {
    pub fn new(doc: Entity<Document>, root: PathBuf, path: PathBuf, language: Language) -> Self {
        let snapshots = crate::history::snapshots(&root, &path);
        Self {
            doc,
            path,
            selected: (!snapshots.is_empty()).then_some(0),
            snapshots,
            language,
        }
    }
}

impl Render for HistoryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let language = self.language;
        let norwegian = language == Language::Norwegian;
        let selected = self.selected;
        let doc = self.doc.clone();
        let current = doc.read(cx).composer_source(cx);
        let label = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let preview = selected
            .and_then(|index| self.snapshots.get(index))
            .and_then(|(_, file)| std::fs::read_to_string(file).ok())
            .map(|snapshot| {
                // `simple_diff` compares lines, so it cannot tell a
                // newline-only change from no change; compare the text.
                let identical = current == snapshot;
                let diff = crate::agent::simple_diff(&label, &current, &snapshot);
                (snapshot, diff, identical)
            });

        let mut list = v_flex().w_full().py_1();
        if self.snapshots.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(language.text(
                        "No snapshots yet — history is taken on each save.",
                        "Ingen øyeblikksbilder ennå — historikk tas ved hver lagring.",
                    )),
            );
        }
        for (index, (timestamp, _)) in self.snapshots.iter().enumerate() {
            list = list.child(
                div()
                    .id(("history-row", index))
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .rounded(cx.theme().radius)
                    .cursor_pointer()
                    .when(Some(index) == selected, |row| row.bg(cx.theme().secondary))
                    .hover(|style| style.bg(cx.theme().muted))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(crate::history::format_epoch(*timestamp)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(index);
                        cx.notify();
                    })),
            );
        }

        v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .id("history-list")
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .child(list),
            )
            .when_some(preview, |this, (snapshot, diff, identical)| {
                let newline_only = !identical && diff.is_empty();
                this.child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if identical {
                                    language.text(
                                        "This version matches the current text.",
                                        "Denne versjonen er lik gjeldende tekst.",
                                    )
                                } else if newline_only {
                                    language.text(
                                        "Only line endings or trailing whitespace differ.",
                                        "Bare linjeskift eller etterfølgende mellomrom skiller.",
                                    )
                                } else {
                                    language.text(
                                        "Changes if you restore this version:",
                                        "Endringer hvis du gjenoppretter denne versjonen:",
                                    )
                                }),
                        )
                        .when(!diff.is_empty(), |this| {
                            this.child(
                                div()
                                    .id("history-diff")
                                    .max_h(px(240.))
                                    .overflow_y_scroll()
                                    .child(render_patch(&diff, cx)),
                            )
                        })
                        .child(
                            Button::new("history-restore")
                                .primary()
                                .small()
                                .label(if norwegian {
                                    "Gjenopprett denne versjonen"
                                } else {
                                    "Restore this version"
                                })
                                .disabled(identical)
                                .on_click(move |_, window, cx| {
                                    let text = snapshot.clone();
                                    doc.update(cx, |doc, cx| doc.restore_text(text, window, cx));
                                    window.close_dialog(cx);
                                }),
                        ),
                )
            })
    }
}

fn render_patch(diff: &str, cx: &Context<HistoryView>) -> impl IntoElement {
    v_flex()
        .w_full()
        .font_family("monospace")
        .text_xs()
        .children(
            diff.lines()
                .filter(|line| {
                    !(line.starts_with("--- ")
                        || line.starts_with("+++ ")
                        || line.starts_with("@@"))
                })
                .map(|line| {
                    let text = match line.split_at_checked(1) {
                        Some((sign @ ("+" | "-"), rest)) => format!("{sign} {rest}"),
                        _ => line.to_string(),
                    };
                    let row = div().w_full().px_1().rounded(cx.theme().radius).child(text);
                    if line.starts_with('+') {
                        row.text_color(cx.theme().success)
                            .bg(cx.theme().success.opacity(0.1))
                    } else if line.starts_with('-') {
                        row.text_color(cx.theme().danger)
                            .bg(cx.theme().danger.opacity(0.1))
                    } else {
                        row.text_color(cx.theme().muted_foreground)
                    }
                }),
        )
}
