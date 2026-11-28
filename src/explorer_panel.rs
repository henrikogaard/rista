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
        let theme = cx.theme();
        let selected = self.vault.read(cx).explorer_filter;
        let has_query = !self.vault.read(cx).explorer_query.is_empty();
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
            .px_2p5()
            .gap_1p5()
            .pb_2()
            .child(
                h_flex()
                    .h(px(28.))
                    .items_center()
                    .gap_1p5()
                    .px_2()
                    .bg(theme.group_box)
                    .border_1()
                    .border_color(theme.border.opacity(0.65))
                    .rounded(px(6.))
                    .child(
                        Icon::new(assets::IconName::Search)
                            .size_3p5()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        Input::new(&self.explorer_search)
                            .small()
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false)
                            .flex_1()
                            .min_w_0(),
                    )
                    .when(has_query, |row| {
                        row.child(
                            Button::new("explorer-clear-search")
                                .ghost()
                                .xsmall()
                                .icon(assets::IconName::Close)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.explorer_search
                                        .update(cx, |input, cx| input.set_value("", window, cx));
                                    this.vault.update(cx, |vault, cx| {
                                        vault.explorer_query.clear();
                                        vault.filter_tree(cx);
                                    });
                                    cx.notify();
                                })),
                        )
                    }),
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
        let language = self.settings.language;
        let can_create_note = self.vault.read(cx).root.is_some();
        let items: Vec<_> = paths
            .iter()
            .map(|path| {
                let is_dir = path.is_dir();
                let icon = if is_dir {
                    assets::IconName::Folder
                } else {
                    assets::IconName::File
                };
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                let title_text = path
                    .file_stem()
                    .or_else(|| path.file_name())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| rel.clone());
                let folder_hint = path
                    .strip_prefix(&root)
                    .ok()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_string_lossy().to_string())
                    .filter(|s| !s.is_empty());
                let snippet = (!is_dir).then(|| note_preview_snippet(path)).flatten();
                CommandItem::new()
                    .icon(icon)
                    .label(rel.clone())
                    .keywords([path.to_string_lossy().to_string(), title_text.clone()])
                    .child(move |_, cx| {
                        let theme = cx.theme();
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Icon::new(icon)
                                            .size_3p5()
                                            .flex_shrink_0()
                                            .text_color(theme.muted_foreground),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .truncate()
                                            .text_color(theme.foreground)
                                            .child(title_text.clone()),
                                    )
                                    .when_some(folder_hint.clone(), |row, folder| {
                                        row.child(
                                            div()
                                                .ml_auto()
                                                .text_xs()
                                                .truncate()
                                                .text_color(theme.muted_foreground.opacity(0.75))
                                                .child(folder),
                                        )
                                    }),
                            )
                            .when_some(snippet.clone(), |col, snip| {
                                col.child(
                                    div()
                                        .pl(px(22.))
                                        .text_xs()
                                        .truncate()
                                        .text_color(theme.muted_foreground.opacity(0.8))
                                        .child(snip),
                                )
                            })
                    })
            })
            .collect();
        let view = cx.entity();
        let focus = state.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let paths = paths.clone();
            let view = view.clone();
            let on_pick = on_pick.clone();
            let empty_view = view.clone();
            let footer_view = view.clone();
            dialog.w(px(600.)).close_button(false).child(
                Command::new(&state)
                    .placeholder(title)
                    .searchable(true)
                    .filterable(true)
                    .group(CommandGroup::new().items(items.clone()))
                    .max_h(px(420.))
                    .empty(move |cmd_state, _window, cx| {
                        let q = cmd_state.query(cx).trim().to_string();
                        if can_create_note && !q.is_empty() && !q.starts_with('#') {
                            let create_target = q.clone();
                            let ws = empty_view.clone();
                            return h_flex()
                                .w_full()
                                .px_3()
                                .py_3()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{} “{}”",
                                            language.text("Create note", "Opprett notat"),
                                            q
                                        )),
                                )
                                .child(
                                    Button::new("quick-open-create-empty")
                                        .primary()
                                        .xsmall()
                                        .icon(assets::IconName::FilePlus)
                                        .label(language.text("Create", "Opprett"))
                                        .on_click(move |_, window, cx| {
                                            window.close_dialog(cx);
                                            let target = create_target.clone();
                                            let ws = ws.clone();
                                            window.defer(cx, move |window, cx| {
                                                ws.update(cx, |this, cx| {
                                                    this.open_wikilink(&target, window, cx);
                                                });
                                            });
                                        }),
                                )
                                .into_any_element();
                        }
                        div()
                            .w_full()
                            .px_3()
                            .py_4()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(language.text("No matching files", "Ingen samsvarende filer"))
                            .into_any_element()
                    })
                    .footer(move |cmd_state, _window, cx| {
                        let q = cmd_state.query(cx).trim().to_string();
                        let theme = cx.theme();
                        if can_create_note && !q.is_empty() && !q.starts_with('#') {
                            let target = q.clone();
                            let ws = footer_view.clone();
                            return h_flex()
                                .w_full()
                                .px_3()
                                .py_1p5()
                                .border_t_1()
                                .border_color(theme.border.opacity(0.6))
                                .items_center()
                                .justify_between()
                                .child(div().text_xs().text_color(theme.muted_foreground).child(
                                    language.text(
                                        "Enter to open · Click to create",
                                        "Enter for å åpne · Klikk for å opprette",
                                    ),
                                ))
                                .child(
                                    Button::new("quick-open-create-footer")
                                        .ghost()
                                        .xsmall()
                                        .icon(assets::IconName::FilePlus)
                                        .label(format!(
                                            "{} “{}”",
                                            language.text("New note", "Nytt notat"),
                                            q
                                        ))
                                        .on_click(move |_, window, cx| {
                                            window.close_dialog(cx);
                                            let target = target.clone();
                                            let ws = ws.clone();
                                            window.defer(cx, move |window, cx| {
                                                ws.update(cx, |this, cx| {
                                                    this.open_wikilink(&target, window, cx);
                                                });
                                            });
                                        }),
                                )
                                .into_any_element();
                        }
                        div().into_any_element()
                    })
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
