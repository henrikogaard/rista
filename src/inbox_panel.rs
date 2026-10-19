use super::*;
use crate::folder::Kind;
use crate::inbox::{DoneBlocked, InboxUndo, INBOX_DIR};
use gpui_kit::component::input::{Textarea, TextareaState};

impl Workspace {
    fn inbox_dir(&self, cx: &App) -> Option<PathBuf> {
        self.vault
            .read(cx)
            .root
            .as_ref()
            .map(|root| root.join(INBOX_DIR))
    }

    /// Notes waiting in `Inbox/`, for the sidebar's "Inbox · N".
    fn inbox_count(&self, cx: &App) -> usize {
        let Some(dir) = self.inbox_dir(cx) else {
            return 0;
        };
        self.vault
            .read(cx)
            .notes
            .iter()
            .filter(|note| note.parent() == Some(dir.as_path()))
            .count()
    }

    pub(super) fn is_inbox_page(&self, page: &folder_dashboard::FolderPage, cx: &App) -> bool {
        self.inbox_dir(cx).is_some_and(|dir| page.path == dir)
    }

    pub(super) fn on_quick_capture(
        &mut self,
        _: &QuickCapture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_quick_capture(window, cx);
    }

    /// A small dialog: Enter saves to `Inbox/`, Shift+Enter adds a line,
    /// Esc cancels. The note is not opened.
    pub(super) fn show_quick_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.vault.read(cx).root.is_none() {
            self.note_status(self.tr("Open a folder first", "Åpne en mappe først"), cx);
            return;
        }
        let placeholder = self.tr("Capture a thought…", "Skriv ned en tanke…");
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .submit_on_enter(true)
                .placeholder(placeholder)
        });
        let title = self.tr("Quick capture", "Hurtignotat");
        let hint = self.tr(
            "Enter to save · Shift+Enter for a new line",
            "Enter for å lagre · Shift+Enter for ny linje",
        );
        let view = cx.entity();
        window.open_dialog(cx, {
            let input = input.clone();
            move |dialog, _window, cx| {
                dialog
                    .title(title)
                    .w(px(440.))
                    .child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(Textarea::new(&input).h(px(96.)).appearance(true))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(hint),
                            ),
                    )
                    .on_ok({
                        let view = view.clone();
                        let input = input.clone();
                        move |_, window, cx| {
                            let text = input.read(cx).value().to_string();
                            view.update(cx, |this, cx| {
                                this.capture_to_inbox(&text, cx);
                                this.refocus(window, cx);
                            });
                            true
                        }
                    })
            }
        });
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx));
        });
    }

    /// Writes one capture note through the same path as `rista inbox`.
    fn capture_to_inbox(&mut self, text: &str, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        if text.trim().is_empty() {
            return;
        }
        let command = crate::cli::Command::Inbox {
            vault: root,
            text: Some(text.to_string()),
        };
        let now = chrono::Local::now().naive_local();
        match crate::cli::execute_write(&command, &self.settings, now, &mut std::io::empty()) {
            Ok(_) => {
                self.vault.update(cx, |vault, cx| vault.refresh(cx));
                self.note_status(self.tr("Captured to Inbox", "Lagt i innboksen"), cx);
            }
            Err(error) => self.note_status(error.message(self.settings.language).to_owned(), cx),
        }
    }

    pub(super) fn on_open_inbox(
        &mut self,
        _: &OpenInbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = self.inbox_dir(cx) else {
            self.note_status(self.tr("Open a folder first", "Åpne en mappe først"), cx);
            return;
        };
        if !dir.is_dir() {
            self.note_status(self.tr("The Inbox is empty", "Innboksen er tom"), cx);
            return;
        }
        self.open_folder_page(dir, window, cx);
    }

    /// Quiet sidebar link, shown only while the Inbox holds notes.
    pub(super) fn render_inbox_link(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.inbox_count(cx);
        (count > 0).then(|| {
            div()
                .id("sidebar-inbox")
                .px_3()
                .py_0p5()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .cursor_pointer()
                .hover(|row| row.text_color(cx.theme().foreground))
                .child(format!("{} · {count}", self.tr("Inbox", "Innboks")))
                .on_click(
                    cx.listener(|this, _, window, cx| this.on_open_inbox(&OpenInbox, window, cx)),
                )
                .into_any_element()
        })
    }

    /// The `Inbox/` folder page: oldest capture first, each row with
    /// Open, Move to… and Done.
    pub(super) fn render_inbox_page(
        &self,
        page: &folder_dashboard::FolderPage,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let mut entries: Vec<_> = page
            .contents()
            .map(|contents| {
                contents
                    .entries
                    .iter()
                    .filter(|entry| entry.kind != Kind::Folder)
                    .collect()
            })
            .unwrap_or_default();
        entries
            .sort_by_cached_key(|entry| crate::inbox::capture_order_of(&entry.path, &entry.name));
        let mut rows = v_flex().w_full();
        if entries.is_empty() {
            rows = rows.child(
                div()
                    .px_2()
                    .py_4()
                    .text_sm()
                    .text_color(muted)
                    .child(self.tr("The Inbox is empty", "Innboksen er tom")),
            );
        }
        for (ix, entry) in entries.into_iter().enumerate() {
            let path = entry.path.clone();
            let label = if entry.kind == Kind::Note {
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                entry.name.clone()
            };
            rows = rows.child(
                h_flex()
                    .id(("inbox-row", ix))
                    .w_full()
                    .px_2()
                    .h_8()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .hover(|row| row.bg(cx.theme().muted))
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .child(
                        Button::new(("inbox-open", ix))
                            .ghost()
                            .xsmall()
                            .label(self.tr("Open", "Åpne"))
                            .on_click(cx.listener({
                                let path = path.clone();
                                move |this, _, window, cx| {
                                    this.open_document(path.clone(), window, cx)
                                }
                            })),
                    )
                    .child(
                        Button::new(("inbox-move", ix))
                            .ghost()
                            .xsmall()
                            .label(self.tr("Move to…", "Flytt til…"))
                            .on_click(cx.listener({
                                let path = path.clone();
                                move |this, _, window, cx| {
                                    this.pick_inbox_destination(path.clone(), window, cx)
                                }
                            })),
                    )
                    .child(
                        Button::new(("inbox-done", ix))
                            .ghost()
                            .xsmall()
                            .label(self.tr("Done", "Ferdig"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.inbox_done(path.clone(), window, cx)
                            })),
                    ),
            );
        }
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .px_4()
                    .py_3()
                    .gap_3()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .child(self.tr("Inbox", "Innboks")),
                    )
                    .when(!self.inbox_undo.is_empty(), |header| {
                        header.child(
                            Button::new("inbox-undo")
                                .ghost()
                                .xsmall()
                                .label(self.tr("Undo", "Angre"))
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.undo_inbox(window, cx)),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .id("inbox-list")
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .overflow_y_scroll()
                    .track_scroll(&page.scroll)
                    .child(rows),
            )
            .into_any_element()
    }

    fn pick_inbox_destination(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let inbox = root.join(INBOX_DIR);
        let mut folders = vec![root.clone()];
        folders.extend(
            self.vault
                .read(cx)
                .explorer_paths()
                .into_iter()
                .filter(|folder| folder.is_dir() && *folder != inbox),
        );
        self.open_path_switcher(
            folders,
            root,
            self.tr("Move to folder…", "Flytt til mappe…"),
            Rc::new(move |this, folder, window, cx| {
                this.inbox_move(path.clone(), folder, window, cx)
            }),
            window,
            cx,
        );
    }

    fn inbox_move(
        &mut self,
        path: PathBuf,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = path.file_name() else {
            return;
        };
        let destination = folder.join(name);
        if destination.exists() {
            self.note_status(
                format!(
                    "{} {}",
                    self.tr("Already exists there:", "Finnes allerede der:"),
                    name.to_string_lossy()
                ),
                cx,
            );
            return;
        }
        if self.move_with_links(path.clone(), destination.clone(), window, cx) {
            self.inbox_undo.push(InboxUndo::Moved {
                from: path,
                to: destination,
            });
            self.reload_inbox_page(cx);
        }
    }

    /// Done sends the note to the vault trash; it never hard-deletes. An
    /// open copy is saved first, and a conflicted one blocks Done, so the
    /// tab closes with no unsaved edits.
    fn inbox_done(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let open = self
            .docs
            .iter()
            .find(|doc| doc.entity.read(cx).path == path)
            .map(|doc| doc.entity.clone());
        if let Some(doc) = &open {
            let conflict = doc.read(cx).conflict;
            let guard = crate::inbox::guard_done(conflict, || {
                doc.update(cx, |doc, cx| doc.flush_and_save(cx))
            });
            if let Err(blocked) = guard {
                let message = match blocked {
                    DoneBlocked::Conflict => self.tr(
                        "Not moved to trash: the open note conflicts with the file on disk",
                        "Ikke flyttet til papirkurven: det åpne notatet er i konflikt med filen på disken",
                    ),
                    DoneBlocked::SaveFailed => self.tr(
                        "Not moved to trash: could not save the open note",
                        "Ikke flyttet til papirkurven: kunne ikke lagre det åpne notatet",
                    ),
                };
                self.note_status(message, cx);
                return;
            }
        }
        match crate::history::move_to_trash(&root, &path) {
            Ok(trashed) => {
                if let Some(ix) = open.and_then(|open| {
                    self.docs
                        .iter()
                        .position(|doc| doc.entity.entity_id() == open.entity_id())
                }) {
                    self.close_tab_now(ix, window, cx);
                }
                self.inbox_undo.push(InboxUndo::Trashed { trashed });
                self.vault.update(cx, |vault, cx| vault.refresh(cx));
                self.reload_inbox_page(cx);
                self.note_status(self.tr("Moved to trash", "Flyttet til papirkurven"), cx);
            }
            Err(_) => self.note_status(
                self.tr(
                    "Could not move to trash",
                    "Kunne ikke flytte til papirkurven",
                ),
                cx,
            ),
        }
    }

    fn undo_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let undone = match self.inbox_undo.last() {
            Some(InboxUndo::Moved { from, to }) => {
                self.move_with_links(to.clone(), from.clone(), window, cx)
            }
            Some(InboxUndo::Trashed { trashed }) => crate::history::restore(&root, trashed).is_ok(),
            None => return,
        };
        if undone {
            self.inbox_undo.pop();
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
            self.reload_inbox_page(cx);
            self.note_status(self.tr("Undone", "Angret"), cx);
        } else {
            self.note_status(
                self.tr(
                    "Cannot undo: a path changed or the original location is occupied.",
                    "Kan ikke angre: en sti er endret eller den opprinnelige plasseringen er opptatt.",
                ),
                cx,
            );
        }
        cx.notify();
    }

    fn reload_inbox_page(&mut self, cx: &mut Context<Self>) {
        if let Some(page) = &self.folder {
            let (path, scroll) = (page.path.clone(), page.scroll.clone());
            self.folder = Some(self.load_folder_page(path, scroll, cx));
        }
        cx.notify();
    }
}
