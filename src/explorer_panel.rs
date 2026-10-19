use super::*;
use crate::explorer::FileFilter;

/// What the path switcher does with the picked path.
pub(super) type PathPick =
    Rc<dyn Fn(&mut Workspace, PathBuf, &mut Window, &mut Context<Workspace>)>;

pub(super) struct CanvasMoveRewrite {
    destination: PathBuf,
    before: String,
    after: String,
    disk_before: Option<Vec<u8>>,
    document: Option<Entity<crate::document::Document>>,
}

impl Workspace {
    pub(super) fn prepare_canvas_move_rewrites(
        &self,
        source: &Path,
        destination: &Path,
        cx: &App,
    ) -> Result<Vec<CanvasMoveRewrite>, String> {
        let root = self
            .vault
            .read(cx)
            .root
            .clone()
            .ok_or_else(|| "Open a vault before moving Canvas references.".to_string())?;
        let source_relative = source
            .strip_prefix(&root)
            .map_err(|_| "The moved path is outside the open vault.".to_string())?;
        let destination_relative = destination
            .strip_prefix(&root)
            .map_err(|_| "The destination is outside the open vault.".to_string())?;
        let source_relative = source_relative.to_path_buf();
        let destination_relative = destination_relative.to_path_buf();
        let canvas_paths = self
            .vault
            .read(cx)
            .notes
            .iter()
            .filter(|path| crate::canvas::is_canvas(path))
            .cloned()
            .collect::<Vec<_>>();
        let mut rewrites = Vec::new();
        for path in canvas_paths {
            let mut next_path = path.clone();
            crate::explorer::repoint_path(&mut next_path, source, destination);
            let open_document = self
                .docs
                .iter()
                .find(|open| open.entity.read(cx).path == path)
                .map(|open| open.entity.clone());
            let (before, disk_before) = if let Some(document) = &open_document {
                let document = document.read(cx);
                if document.conflict || !document.disk_matches_loaded() {
                    return Err(format!(
                        "Canvas references were not updated because {} has an unresolved disk conflict.",
                        path.display()
                    ));
                }
                (document.editor.read(cx).value().to_string(), None)
            } else {
                let bytes = std::fs::read(&path).map_err(|error| {
                    format!(
                        "Canvas references were not updated because {} could not be read: {error}",
                        path.display()
                    )
                })?;
                let text = String::from_utf8(bytes.clone()).map_err(|_| {
                    format!(
                        "Canvas references were not updated because {} is not UTF-8.",
                        path.display()
                    )
                })?;
                (text, Some(bytes))
            };
            let Ok(mut canvas) = crate::canvas::CanvasDocument::parse(&before) else {
                continue;
            };
            if !canvas.retarget_paths(
                &source_relative,
                &destination_relative,
                path.strip_prefix(&root).unwrap_or(&path),
                next_path.strip_prefix(&root).unwrap_or(&next_path),
                Some(&|target, wiki| {
                    let vault = self.vault.read(cx);
                    let resolved = if wiki {
                        vault.resolve_link_target(target)
                    } else {
                        vault.md_link_path(target, path.parent().unwrap_or(&root))
                    }?;
                    resolved.strip_prefix(&root).ok().map(Path::to_path_buf)
                }),
            ) {
                continue;
            }
            let after = canvas.serialize().map_err(|error| {
                format!(
                    "Canvas references in {} could not be serialized: {error}",
                    path.display()
                )
            })?;
            if after != before {
                rewrites.push(CanvasMoveRewrite {
                    destination: next_path,
                    before,
                    after,
                    disk_before,
                    document: open_document,
                });
            }
        }
        Ok(rewrites)
    }

    pub(super) fn apply_canvas_move_rewrites(
        &mut self,
        rewrites: Vec<CanvasMoveRewrite>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut complete = true;
        for rewrite in rewrites {
            if let Some(document) = rewrite.document {
                let applied = document.update(cx, |document, cx| {
                    document.apply_canvas_snapshot(
                        &rewrite.before,
                        &rewrite.after,
                        None,
                        window,
                        cx,
                    )
                });
                complete &= applied;
            } else if let Some(baseline) = rewrite.disk_before {
                if let Some(root) = self.vault.read(cx).root.as_deref() {
                    if std::fs::read(&rewrite.destination).ok().as_deref()
                        == Some(baseline.as_slice())
                    {
                        crate::history::snapshot_before_write(
                            root,
                            &rewrite.destination,
                            &rewrite.after,
                        );
                    }
                }
                complete &= crate::document::guarded_write(
                    &rewrite.destination,
                    &baseline,
                    rewrite.after.as_bytes(),
                )
                .is_ok();
            }
        }
        complete
    }

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
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .bg(cx.theme().muted)
                    .rounded(cx.theme().radius)
                    .child(
                        Icon::new(assets::IconName::Search)
                            .size_3p5()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        Input::new(&self.explorer_search)
                            .small()
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .flex_1()
                            .min_w_0(),
                    ),
            )
            // Filters left, sort and reveal right, on one row; it wraps
            // only when the sidebar is too narrow for both.
            .child(
                h_flex().flex_wrap().justify_between().child(filters).child(
                    h_flex()
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
                ),
            )
            .when(!self.move_undo.is_empty(), |d| {
                d.child(
                    Button::new("undo-file-move")
                        .ghost()
                        .xsmall()
                        .label(self.tr("Undo last move", "Angre siste flytting"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.undo_file_move(window, cx)),
                        ),
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
            doc.entity.update(cx, |doc, cx| {
                let mut path = doc.path.clone();
                repoint(&mut path);
                if path != doc.path {
                    doc.set_path(path);
                    doc.resync_preview(cx);
                    cx.notify();
                }
            });
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
        self.settings.bookmarks.repoint(source, destination);
        self.persist_bookmarks(cx);
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        if let Some(page) = &self.folder {
            let mut path = page.path.clone();
            repoint(&mut path);
            self.folder = Some(self.load_folder_page(path, page.scroll.clone(), cx));
        }
        self.persist_tabs(cx);
    }

    fn undo_file_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((source, destination)) = self.move_undo.last().cloned() else {
            return;
        };
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let rewrites = match self.prepare_canvas_move_rewrites(&destination, &source, cx) {
            Ok(rewrites) => rewrites,
            Err(_) => {
                self.note_status(
                    self.tr(
                        "Cannot undo this move because Canvas references have a conflict.",
                        "Kan ikke angre flyttingen fordi Canvas-referanser har en konflikt.",
                    ),
                    cx,
                );
                return;
            }
        };
        if crate::explorer::move_entry(&root, &destination, &source).is_ok() {
            self.move_undo.pop();
            self.repoint_moved_entry(&destination, &source, cx);
            let complete = self.apply_canvas_move_rewrites(rewrites, window, cx);
            self.note_status(
                self.tr(
                    if complete {
                        "Move undone"
                    } else {
                        "Move undone, but some Canvas references were not updated."
                    },
                    if complete {
                        "Flytting angret"
                    } else {
                        "Flytting angret, men noen Canvas-referanser ble ikke oppdatert."
                    },
                ),
                cx,
            );
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
            Rc::new(|this, path, window, cx| this.open_explorer_path(path, false, window, cx)),
            window,
            cx,
        );
    }

    pub(super) fn open_path_switcher(
        &mut self,
        paths: Vec<PathBuf>,
        root: PathBuf,
        title: &'static str,
        on_pick: PathPick,
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
            let on_pick = on_pick.clone();
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
                                let on_pick = on_pick.clone();
                                move |window, cx| {
                                    view.update(cx, |this, cx| on_pick(this, path, window, cx));
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
            Rc::new(|this, path, window, cx| this.open_explorer_path(path, false, window, cx)),
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
