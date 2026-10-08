use super::*;
use crate::explorer::FileFilter;

impl Workspace {
    pub(super) fn render_explorer_controls(&self, cx: &mut Context<Self>) -> Div {
        let selected = self.vault.read(cx).explorer_filter;
        let mut filters = h_flex().gap_0p5();
        for (ix, (filter, en, nb)) in [
            (FileFilter::All, "All", "Alle"),
            (FileFilter::Notes, "Notes", "Notater"),
            (FileFilter::Images, "Images", "Bilder"),
            (FileFilter::Other, "Other", "Andre"),
        ]
        .into_iter()
        .enumerate()
        {
            filters = filters.child(
                Button::new(("explorer-filter", ix))
                    .ghost()
                    .xsmall()
                    .label(self.tr(en, nb))
                    .selected(filter == selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.vault.update(cx, |vault, cx| {
                            vault.explorer_filter = filter;
                            vault.filter_tree(cx);
                        });
                        cx.notify();
                    })),
            );
        }
        v_flex()
            .px_2()
            .gap_1()
            .pb_2()
            .child(Input::new(&self.explorer_search).small())
            .child(filters)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        Button::new("explorer-sort-label")
                            .ghost()
                            .xsmall()
                            .label(match self.settings.tree_sort {
                                TreeSort::Name => self.tr("Name ↑", "Navn ↑"),
                                TreeSort::Modified => self.tr("Modified ↓", "Endret ↓"),
                                TreeSort::Type => self.tr("Type ↑", "Type ↑"),
                                TreeSort::Size => self.tr("Size ↓", "Størrelse ↓"),
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_tree_sort(cx))),
                    )
                    .child(
                        Button::new("explorer-reveal")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Locate)
                            .tooltip(self.tr("Reveal active file", "Vis aktiv fil"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.explorer_search
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                this.vault.update(cx, |vault, cx| {
                                    vault.explorer_filter = FileFilter::All;
                                    vault.explorer_query.clear();
                                    vault.filter_tree(cx);
                                });
                                this.reveal_active_file(cx);
                            })),
                    ),
            )
            .when(!self.move_undo.is_empty(), |d| {
                d.child(
                    Button::new("undo-file-move")
                        .ghost()
                        .xsmall()
                        .label(self.tr("Undo last move", "Angre siste flytting"))
                        .on_click(cx.listener(|this, _, _, cx| this.undo_file_move(cx))),
                )
            })
    }

    pub(super) fn repoint_moved_entry(
        &mut self,
        source: &Path,
        destination: &Path,
        cx: &mut Context<Self>,
    ) {
        let repoint = |path: &mut PathBuf| {
            crate::explorer::repoint_path(path, source, destination);
        };
        for doc in &self.docs {
            doc.entity.update(cx, |doc, _| repoint(&mut doc.path));
        }
        for path in self
            .recent
            .iter_mut()
            .chain(self.closed_tabs.iter_mut())
            .chain(self.nav_stack.iter_mut())
        {
            repoint(path);
        }
        for value in self
            .settings
            .starred
            .iter_mut()
            .chain(self.settings.pinned_tabs.iter_mut())
        {
            let mut path = PathBuf::from(&*value);
            repoint(&mut path);
            *value = path.to_string_lossy().to_string();
        }
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        if let Some(page) = &self.folder {
            let mut path = page.path.clone();
            repoint(&mut path);
            self.folder = Some(self.load_folder_page(path, page.scroll.clone(), cx));
        }
        self.persist_tabs(cx);
    }

    fn undo_file_move(&mut self, cx: &mut Context<Self>) {
        let Some((source, destination)) = self.move_undo.last().cloned() else {
            return;
        };
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        if crate::explorer::move_entry(&root, &destination, &source).is_ok() {
            self.move_undo.pop();
            self.repoint_moved_entry(&destination, &source, cx);
            self.note_status(self.tr("Move undone", "Flytting angret"), cx);
        } else {
            self.note_status(self.tr("Cannot undo: a path changed or the original location is occupied.", "Kan ikke angre: en sti er endret eller den opprinnelige plasseringen er opptatt."), cx);
        }
        cx.notify();
    }

    pub(super) fn open_explorer_path(
        &mut self,
        path: PathBuf,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path.is_dir() {
            self.open_folder_page(path, window, cx);
        } else if keep {
            self.open_document_new_tab(path, window, cx);
        } else {
            self.open_document(path, window, cx);
        }
    }

    pub(super) fn on_quick_open(
        &mut self,
        _: &QuickOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let vault = self.vault.read(cx);
        let root = vault.root.clone().unwrap_or_default();
        let mut paths: Vec<_> = self.recent.iter().filter(|p| p.exists()).cloned().collect();
        for path in vault.explorer_paths() {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        self.open_path_switcher(
            paths,
            root,
            self.tr("Open file or folder…", "Åpne fil eller mappe…"),
            window,
            cx,
        );
    }

    fn open_path_switcher(
        &mut self,
        paths: Vec<PathBuf>,
        root: PathBuf,
        title: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = cx.new(|cx| CommandState::new(window, cx));
        let items: Vec<_> = paths
            .iter()
            .map(|path| {
                CommandItem::new()
                    .icon(if path.is_dir() {
                        assets::IconName::Folder
                    } else {
                        assets::IconName::File
                    })
                    .label(
                        path.strip_prefix(&root)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .to_string(),
                    )
                    .keywords([path.to_string_lossy().to_string()])
            })
            .collect();
        let view = cx.entity();
        let focus = state.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let paths = paths.clone();
            let view = view.clone();
            dialog.w(px(580.)).close_button(false).child(
                Command::new(&state)
                    .placeholder(title)
                    .searchable(true)
                    .filterable(true)
                    .group(CommandGroup::new().items(items.clone()))
                    .max_h(px(420.))
                    .on_confirm(move |index, window, cx| {
                        window.close_dialog(cx);
                        if let Some(path) = paths.get(index.row) {
                            let path = path.clone();
                            window.defer(cx, {
                                let view = view.clone();
                                move |window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.open_explorer_path(path, false, window, cx)
                                    });
                                }
                            });
                        }
                    })
                    .on_cancel(|window, cx| {
                        window.close_dialog(cx);
                    }),
            )
        });
        window.defer(cx, move |window, cx| {
            focus.update(cx, |state, cx| state.focus(window, cx))
        });
    }

    pub(super) fn open_siblings(
        &mut self,
        folder: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = crate::folder::read(&folder)
            .map(|c| {
                c.entries
                    .into_iter()
                    .filter(|e| {
                        e.kind == crate::folder::Kind::Folder
                            || crate::file_preview::visible(&e.path, self.settings.show_other_files)
                    })
                    .map(|e| e.path)
                    .collect()
            })
            .unwrap_or_default();
        self.open_path_switcher(
            paths,
            folder,
            self.tr("Browse this folder…", "Bla i denne mappen…"),
            window,
            cx,
        );
    }

    pub(super) fn sibling_button(
        &self,
        id: usize,
        folder: PathBuf,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(("browse-siblings", id))
            .ghost()
            .xsmall()
            .icon(assets::IconName::ChevronDown)
            .tooltip(self.tr("Browse this folder", "Bla i denne mappen"))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_siblings(folder.clone(), window, cx)
            }))
    }
}
