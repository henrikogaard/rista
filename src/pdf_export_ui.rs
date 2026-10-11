use super::*;
use crate::pdf_export::{ExportError, Options, Paper, Style};
use std::sync::Arc;

impl Workspace {
    pub(super) fn export_pdf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !cfg!(target_os = "macos") {
            self.note_status(ExportError::Unsupported.message(self.settings.language), cx);
            return;
        }
        let Some(doc) = self.active_doc().cloned() else {
            self.note_status(
                self.tr("Open a Markdown note first", "Åpne et Markdown-notat først"),
                cx,
            );
            return;
        };
        let doc = doc.read(cx);
        if !matches!(
            crate::file_preview::extension(&doc.path).as_str(),
            "md" | "markdown"
        ) {
            self.note_status(
                self.tr("Open a Markdown note first", "Åpne et Markdown-notat først"),
                cx,
            );
            return;
        }
        let source = doc.path.clone();
        let text = doc.editor.read(cx).value().to_string();
        let root = doc.vault_root.clone();
        let files = self.vault.read(cx).attachment_path_snapshot();
        let language = self.settings.language;
        let export_font = crate::pdf_export::font_family(&self.settings.editor_font_family);
        let font_warning = (export_font != self.settings.editor_font_family).then(|| {
            format!(
                "{}: {} → {}",
                language.text(
                    "Selected font unavailable; export will use a fallback",
                    "Valgt skrifttype er utilgjengelig; eksporten bruker en erstatning",
                ),
                self.settings.editor_font_family,
                export_font
            )
        });
        let view = cx.entity();
        let options = Rc::new(RefCell::new(Options::default()));
        let error = Rc::new(RefCell::new(None::<SharedString>));
        let margin = cx.new(|cx| InputState::new(window, cx).default_value("20"));
        self.refocus(window, cx);
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let state = *options.borrow();
            let mut paper = h_flex().gap_2();
            for (id, label, value) in [
                ("pdf-a4", "A4", Paper::A4),
                ("pdf-letter", "Letter", Paper::Letter),
            ] {
                let options = options.clone();
                paper = paper.child(
                    Button::new(id)
                        .label(label)
                        .small()
                        .selected(state.paper == value)
                        .on_click(move |_, window, _| {
                            options.borrow_mut().paper = value;
                            window.refresh();
                        }),
                );
            }
            let mut theme = h_flex().gap_2();
            for (id, label, value) in [
                (
                    "pdf-light",
                    language.text("Light theme", "Lyst tema"),
                    false,
                ),
                (
                    "pdf-active",
                    language.text("Active theme", "Aktivt tema"),
                    true,
                ),
            ] {
                let options = options.clone();
                theme = theme.child(
                    Button::new(id)
                        .label(label)
                        .small()
                        .selected(state.active_theme == value)
                        .on_click(move |_, window, _| {
                            options.borrow_mut().active_theme = value;
                            window.refresh();
                        }),
                );
            }
            dialog
                .title(language.text("Export PDF", "Eksporter PDF"))
                .w(px(420.))
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .show_cancel(true)
                        .cancel_text(language.text("Cancel", "Avbryt"))
                        .ok_text(language.text("Choose destination…", "Velg målmappe…")),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .child(language.text("Paper size", "Papirstørrelse"))
                        .child(paper)
                        .child(language.text("Margins (mm, 5–50)", "Marger (mm, 5–50)"))
                        .child(Input::new(&margin).w(px(100.)))
                        .child(theme)
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(language.text(
                                    "Uses the current note and editor font. Local images only.",
                                    "Bruker gjeldende notat og skrifttype. Kun lokale bilder.",
                                )),
                        )
                        .when_some(font_warning.clone(), |col, warning| {
                            col.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(warning),
                            )
                        })
                        .when_some(error.borrow().clone(), |col, error| {
                            col.child(div().text_sm().text_color(cx.theme().danger).child(error))
                        }),
                )
                .on_ok({
                    let (view, options, margin, error, source, text, root, files) = (
                        view.clone(),
                        options.clone(),
                        margin.clone(),
                        error.clone(),
                        source.clone(),
                        text.clone(),
                        root.clone(),
                        files.clone(),
                    );
                    move |_, window, cx| {
                        let value = margin
                            .read(cx)
                            .value()
                            .replace(',', ".")
                            .parse::<f64>()
                            .ok();
                        let opts = Options {
                            margin_mm: value.unwrap_or(f64::NAN),
                            ..*options.borrow()
                        };
                        if !opts.valid() {
                            *error.borrow_mut() = Some(
                                language
                                    .text(
                                        "Enter a margin between 5 and 50 mm.",
                                        "Skriv inn en marg mellom 5 og 50 mm.",
                                    )
                                    .into(),
                            );
                            window.refresh();
                            return false;
                        }
                        let style = Style::new(&view.read(cx).settings, opts.active_theme, cx);
                        let (view, source, text, root, files) = (
                            view.clone(),
                            source.clone(),
                            text.clone(),
                            root.clone(),
                            files.clone(),
                        );
                        window.close_dialog(cx);
                        window.defer(cx, move |window, cx| {
                            view.update(cx, |this, cx| {
                                this.save_pdf_snapshot(
                                    source, text, root, files, style, opts, window, cx,
                                );
                            });
                        });
                        false
                    }
                })
                .on_cancel(|_, _, _| true)
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| this.refocus(window, cx));
                    }
                })
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn save_pdf_snapshot(
        &mut self,
        source: PathBuf,
        text: String,
        root: Option<PathBuf>,
        files: Arc<Vec<PathBuf>>,
        style: Style,
        options: Options,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dir = source.parent().unwrap_or(Path::new("."));
        let name = source
            .with_extension("pdf")
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let receiver = cx.prompt_for_new_path(dir, Some(&name));
        let language = self.settings.language;
        cx.spawn_in(window, async move |view, window| {
            let Ok(Ok(Some(destination))) = receiver.await else {
                return;
            };
            if crate::file_preview::extension(&destination) != "pdf" {
                let _ = view.update(window, |this, cx| {
                    this.note_status(
                        language.text(
                            "Choose a filename ending in .pdf.",
                            "Velg et filnavn som slutter med .pdf.",
                        ),
                        cx,
                    )
                });
                return;
            }
            let _ = view.update(window, |this, cx| {
                this.note_status(language.text("Preparing PDF…", "Klargjør PDF…"), cx)
            });
            let html = window
                .background_executor()
                .spawn(async move {
                    crate::pdf_export::html(
                        &text,
                        &source,
                        root.as_deref(),
                        files.as_ref().as_slice(),
                        &style,
                    )
                })
                .await;
            let result = match html {
                Err(error) => Err(error),
                Ok(html) => {
                    #[cfg(target_os = "macos")]
                    {
                        match crate::pdf_export::ExportJob::start(html, options) {
                            Err(error) => Err(error),
                            Ok(mut job) => loop {
                                if let Some(result) = job.poll() {
                                    break result;
                                }
                                smol::Timer::after(std::time::Duration::from_millis(50)).await;
                            },
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = (html, options);
                        Err::<Vec<u8>, _>(ExportError::Unsupported)
                    }
                }
            };
            let result = match result {
                Ok(bytes) => {
                    window
                        .background_executor()
                        .spawn(async move { crate::pdf_export::save(&destination, &bytes) })
                        .await
                }
                Err(error) => Err(error),
            };
            let _ = view.update(window, |this, cx| {
                this.note_status(
                    match result {
                        Ok(()) => language.text("PDF exported", "PDF eksportert"),
                        Err(error) => error.message(language),
                    },
                    cx,
                )
            });
        })
        .detach();
    }
}
