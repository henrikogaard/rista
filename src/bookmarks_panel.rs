use super::*;
use crate::bookmarks::{Bookmark, BookmarkKind, DropTarget};
use gpui_kit::base::ElementExt;
use std::cell::Cell;
use std::ffi::OsStr;
use std::rc::Rc;

impl Workspace {
    fn visible_markdown_document(&self, cx: &App) -> Option<Entity<Document>> {
        if self.graph.is_some() || self.folder.is_some() {
            return None;
        }
        let doc = self.active_doc()?.clone();
        doc.read(cx)
            .path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            .then_some(doc)
    }

    fn bookmark_title(item: &Bookmark, language: crate::settings::Language) -> String {
        if let Some(title) = item
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            return title.to_owned();
        }
        match &item.kind {
            BookmarkKind::File { path, anchor, .. } => {
                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                anchor.as_deref().map_or_else(
                    || name.to_string(),
                    |anchor| format!("{name} · {}", anchor.trim_start_matches('^')),
                )
            }
            BookmarkKind::Folder { path, .. } => path
                .file_name()
                .unwrap_or_else(|| OsStr::new(""))
                .to_string_lossy()
                .into(),
            BookmarkKind::Search { query, .. } => query.clone(),
            BookmarkKind::Graph { .. } => language.text("Graph", "Graf").to_owned(),
            BookmarkKind::Base { path, view } => format!(
                "{} · {view}",
                path.file_stem().unwrap_or_default().to_string_lossy()
            ),
            BookmarkKind::Group { .. } => language.text("Group", "Gruppe").to_owned(),
        }
    }

    fn bookmark_missing(item: &Bookmark) -> bool {
        match &item.kind {
            BookmarkKind::File { path, .. } | BookmarkKind::Base { path, .. } => !path.is_file(),
            BookmarkKind::Folder { path, root } => {
                !path.is_dir() || root.as_ref().is_some_and(|root| !root.is_dir())
            }
            BookmarkKind::Search { root, .. } => !root.is_dir(),
            BookmarkKind::Graph { root, center } => {
                !root.is_dir() || center.as_ref().is_some_and(|center| !center.is_file())
            }
            BookmarkKind::Group { .. } => false,
        }
    }

    fn bookmark_icon(item: &Bookmark) -> assets::IconName {
        match &item.kind {
            BookmarkKind::Folder { .. } | BookmarkKind::Group { .. } => assets::IconName::Folder,
            BookmarkKind::Search { .. } => assets::IconName::Search,
            BookmarkKind::Graph { .. } => assets::IconName::Waypoints,
            BookmarkKind::Base { .. } => assets::IconName::LayoutTemplate,
            BookmarkKind::File { .. } => assets::IconName::File,
        }
    }

    fn bookmark_count(items: &[Bookmark]) -> usize {
        items
            .iter()
            .map(|item| match &item.kind {
                BookmarkKind::Group { items, .. } => 1 + Self::bookmark_count(items),
                _ => 1,
            })
            .sum()
    }

    fn render_bookmark_item(
        &self,
        item: &Bookmark,
        depth: usize,
        view: Entity<Self>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let id = item.id;
        let title = Self::bookmark_title(item, self.settings.language);
        let missing = Self::bookmark_missing(item);
        let is_group = matches!(item.kind, BookmarkKind::Group { .. });
        let collapsed = matches!(
            item.kind,
            BookmarkKind::Group {
                collapsed: true,
                ..
            }
        );
        let drop_before = self.bookmark_drop_target == Some(DropTarget::Before(id));
        let drop_after = self.bookmark_drop_target == Some(DropTarget::After(id));
        let drop_into = is_group && self.bookmark_drop_target == Some(DropTarget::Into(id));
        let bounds = Rc::new(Cell::new(Bounds::default()));
        let drag_title = title.clone();
        let row_target = move |position: Point<Pixels>, bounds: Bounds<Pixels>| {
            let height = f32::from(bounds.size.height);
            let relative = if height > 0. {
                f32::from(position.y - bounds.origin.y) / height
            } else {
                0.5
            };
            if is_group && (0.25..=0.75).contains(&relative) {
                DropTarget::Into(id)
            } else if relative < 0.5 {
                DropTarget::Before(id)
            } else {
                DropTarget::After(id)
            }
        };
        let row = div()
            .id(("bookmark-row", id))
            .w_full()
            .min_h(px(26.))
            .px_2()
            .pl(px(8. + depth as f32 * 16.))
            .py_0p5()
            .when(drop_into, |this| this.bg(theme.muted.opacity(0.35)))
            .on_prepaint({
                let bounds = bounds.clone();
                move |resolved, _, _| bounds.set(resolved)
            })
            .on_drag_move::<DraggedBookmark>({
                let view = view.clone();
                move |event, _, cx| {
                    if event.bounds.contains(&event.event.position) {
                        let dragged = event.drag(cx);
                        let target = (dragged.0 != id)
                            .then(|| row_target(event.event.position, event.bounds));
                        view.update(cx, |this, cx| {
                            if this.bookmark_drop_target != target {
                                this.bookmark_drop_target = target;
                                cx.notify();
                            }
                        });
                    }
                }
            })
            .capture_any_mouse_up({
                let view = view.clone();
                move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        this.dragged_bookmark = None;
                        this.bookmark_drop_target = None;
                        cx.notify();
                    });
                }
            })
            .on_drag(DraggedBookmark(id), {
                let view = view.clone();
                move |_, _, _, cx| {
                    view.update(cx, |this, cx| {
                        this.dragged_bookmark = Some(id);
                        this.bookmark_drop_target = None;
                        cx.notify();
                    });
                    cx.new(|_| TreeDragPreview {
                        label: drag_title.clone().into(),
                    })
                }
            })
            .on_drop::<DraggedBookmark>({
                let view = view.clone();
                move |dragged, window, cx| {
                    let target = row_target(window.mouse_position(), bounds.get());
                    view.update(cx, |this, cx| {
                        this.bookmark_drop_target = Some(target);
                        if dragged.0 != id && this.settings.bookmarks.move_to(dragged.0, target) {
                            this.persist_bookmarks(cx);
                        }
                        this.dragged_bookmark = None;
                        this.bookmark_drop_target = None;
                        cx.notify();
                    });
                }
            })
            .on_mouse_down(MouseButton::Right, {
                let view = view.clone();
                move |event, window, cx| {
                    window.prevent_default();
                    view.update(cx, |this, cx| {
                        this.show_bookmark_menu(id, event.position, window, cx);
                    });
                }
            })
            .hover(|style| style.bg(theme.muted.opacity(0.35)))
            .on_click({
                let view = view.clone();
                move |_, window, cx| {
                    if is_group {
                        view.update(cx, |this, cx| {
                            if let Some(Bookmark {
                                kind: BookmarkKind::Group { collapsed, .. },
                                ..
                            }) = this.settings.bookmarks.find_mut(id)
                            {
                                *collapsed = !*collapsed;
                                this.settings.save();
                                cx.notify();
                            }
                        });
                    } else {
                        view.update(cx, |this, cx| this.activate_bookmark(id, window, cx));
                    }
                }
            })
            .child(
                v_flex()
                    .w_full()
                    .when(drop_before, |this| {
                        this.child(div().w_full().h(px(1.)).bg(theme.accent))
                    })
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Icon::new(if is_group {
                                    if collapsed {
                                        assets::IconName::ChevronRight
                                    } else {
                                        assets::IconName::ChevronDown
                                    }
                                } else {
                                    Self::bookmark_icon(item)
                                })
                                .size_4()
                                .text_color(if missing {
                                    theme.danger
                                } else {
                                    theme.muted_foreground
                                }),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .truncate()
                                    .text_color(if missing {
                                        theme.danger
                                    } else {
                                        theme.foreground
                                    })
                                    .child(title),
                            )
                            .when(missing, |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.danger)
                                        .child(self.tr("Missing", "Mangler")),
                                )
                            })
                            .when(is_group, |this| {
                                this.child(
                                    div().text_xs().text_color(theme.muted_foreground).child(
                                        format!(
                                            "{}",
                                            match &item.kind {
                                                BookmarkKind::Group { items, .. } =>
                                                    Self::bookmark_count(items),
                                                _ => 0,
                                            }
                                        ),
                                    ),
                                )
                            }),
                    )
                    .when(drop_after, |this| {
                        this.child(div().w_full().h(px(1.)).bg(theme.accent))
                    }),
            );
        let element = row.into_any_element();
        if collapsed {
            return element;
        }
        if let BookmarkKind::Group { items, .. } = &item.kind {
            return v_flex()
                .w_full()
                .child(element)
                .children(
                    items
                        .iter()
                        .map(|item| self.render_bookmark_item(item, depth + 1, view.clone(), cx)),
                )
                .into_any_element();
        }
        element
    }

    pub(super) fn render_bookmarks(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted_foreground = cx.theme().muted_foreground;
        let view = cx.entity();
        let count = Self::bookmark_count(&self.settings.bookmarks.items);
        let menu_button = Button::new("bookmark-actions")
            .ghost()
            .xsmall()
            .icon(assets::IconName::Plus)
            .tooltip(self.tr("Bookmark actions", "Bokmerkehandlinger"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.show_bookmark_actions(window, cx);
            }));
        v_flex()
            .w_full()
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .id("bookmark-toggle")
                            .flex_grow(1.)
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        Icon::new(if self.starred_open {
                                            assets::IconName::ChevronDown
                                        } else {
                                            assets::IconName::ChevronRight
                                        })
                                        .size_4()
                                        .text_color(muted_foreground),
                                    )
                                    .child(div().text_xs().text_color(muted_foreground).child(
                                        format!("{} · {count}", self.tr("Bookmarks", "Bokmerker")),
                                    )),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.starred_open = !this.starred_open;
                                this.settings.panes.starred = this.starred_open;
                                this.settings.save();
                                cx.notify();
                            })),
                    )
                    .child(menu_button)
                    .on_drop::<DraggedBookmark>({
                        let view = view.clone();
                        move |dragged, _, cx| {
                            view.update(cx, |this, cx| {
                                if this.settings.bookmarks.move_to(dragged.0, DropTarget::Root) {
                                    this.persist_bookmarks(cx);
                                }
                                this.dragged_bookmark = None;
                                this.bookmark_drop_target = None;
                                cx.notify();
                            });
                        }
                    })
                    .on_drag_move::<DraggedBookmark>({
                        let view = view.clone();
                        move |event, _, cx| {
                            if event.bounds.contains(&event.event.position) {
                                view.update(cx, |this, cx| {
                                    if this.bookmark_drop_target != Some(DropTarget::Root) {
                                        this.bookmark_drop_target = Some(DropTarget::Root);
                                        cx.notify();
                                    }
                                });
                            }
                        }
                    }),
            )
            .when(
                self.bookmark_drop_target == Some(DropTarget::Root),
                |this| this.child(div().h(px(1.)).w_full().bg(cx.theme().accent.opacity(0.7))),
            )
            .when(self.starred_open, |this| {
                this.child(
                    v_flex()
                        .w_full()
                        .children(
                            self.settings
                                .bookmarks
                                .items
                                .iter()
                                .map(|item| self.render_bookmark_item(item, 0, view.clone(), cx)),
                        )
                        .capture_any_mouse_up({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.dragged_bookmark = None;
                                    this.bookmark_drop_target = None;
                                    cx.notify();
                                });
                            }
                        }),
                )
            })
            .when(
                self.starred_open && self.settings.bookmarks.items.is_empty(),
                |this| {
                    this.child(
                        div()
                            .px_2()
                            .pb_1()
                            .text_xs()
                            .text_color(muted_foreground)
                            .child(self.tr("No bookmarks yet", "Ingen bokmerker ennå")),
                    )
                },
            )
    }

    fn show_bookmark_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_file_menu();
        let view = cx.entity();
        let language = self.settings.language;
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            menu.item(
                PopupMenuItem::new(
                    language.text("Bookmark current view", "Bokmerk gjeldende visning"),
                )
                .on_click({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| this.bookmark_current_view(window, cx))
                    }
                }),
            )
            .item(
                PopupMenuItem::new(
                    language.text("Bookmark current heading", "Bokmerk gjeldende overskrift"),
                )
                .on_click({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| this.bookmark_current_heading(window, cx))
                    }
                }),
            )
            .item(
                PopupMenuItem::new(
                    language.text("Bookmark current block", "Bokmerk gjeldende blokk"),
                )
                .on_click({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| this.bookmark_current_block(window, cx))
                    }
                }),
            )
            .separator()
            .item(
                PopupMenuItem::new(language.text("New group", "Ny gruppe")).on_click({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.defer_bookmark_dialog(window, cx, |this, window, cx| {
                                this.show_new_group_dialog(None, window, cx)
                            })
                        })
                    }
                }),
            )
            .item(
                PopupMenuItem::new(
                    language.text("Import vault bookmarks…", "Importer hvelvbokmerker…"),
                )
                .on_click({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| this.import_vault_bookmarks(window, cx))
                    }
                }),
            )
        });
        menu.focus_handle(cx).focus(window, cx);
        self.file_menu_sub = Some(cx.subscribe(
            &menu,
            |this, _menu, _: &gpui_kit::DismissEvent, cx| {
                this.file_menu = None;
                this.file_menu_sub = None;
                cx.notify();
            },
        ));
        self.file_menu = Some((menu, window.mouse_position()));
        cx.notify();
    }

    fn show_bookmark_menu(
        &mut self,
        id: u64,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_file_menu();
        let view = cx.entity();
        let language = self.settings.language;
        let is_group = matches!(
            self.settings.bookmarks.find(id).map(|item| &item.kind),
            Some(BookmarkKind::Group { .. })
        );
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            menu.item(
                PopupMenuItem::new(language.text("Rename…", "Gi nytt navn…")).on_click({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.defer_bookmark_dialog(window, cx, move |this, window, cx| {
                                this.show_bookmark_rename_dialog(id, window, cx)
                            })
                        })
                    }
                }),
            )
            .item(
                PopupMenuItem::new(language.text("Remove bookmark", "Fjern bokmerke")).on_click({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| this.remove_bookmark(id, window, cx))
                    }
                }),
            )
            .when(is_group, |menu| {
                menu.item(
                    PopupMenuItem::new(language.text("New subgroup", "Ny undergruppe")).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.defer_bookmark_dialog(window, cx, move |this, window, cx| {
                                    this.show_new_group_dialog(Some(id), window, cx)
                                })
                            })
                        }
                    }),
                )
            })
        });
        menu.focus_handle(cx).focus(window, cx);
        self.file_menu_sub = Some(cx.subscribe(
            &menu,
            |this, _menu, _: &gpui_kit::DismissEvent, cx| {
                this.file_menu = None;
                this.file_menu_sub = None;
                cx.notify();
            },
        ));
        self.file_menu = Some((menu, position));
        cx.notify();
    }

    fn bookmark_current_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kind = if let Some(graph) = &self.graph {
            let root = self.vault.read(cx).root.clone();
            BookmarkKind::Graph {
                root: root.unwrap_or_default(),
                center: graph.read(cx).local_center_path(),
            }
        } else if let Some(folder) = &self.folder {
            let Some(root) = self.vault.read(cx).root.clone() else {
                self.note_status(self.tr("Open a vault first", "Åpne et hvelv først"), cx);
                return;
            };
            BookmarkKind::Folder {
                path: folder.path.clone(),
                root: Some(root),
            }
        } else if let Some(doc) = self.active_doc().cloned() {
            let path = doc.read(cx).path.clone();
            if let Some(base) = self
                .active
                .and_then(|ix| self.docs.get(ix))
                .and_then(|doc| doc.base.clone())
            {
                let Some(view) = base.read(cx).spec_view_name(cx) else {
                    self.note_status(
                        self.tr("This view has no name", "Denne visningen har ikke noe navn"),
                        cx,
                    );
                    return;
                };
                BookmarkKind::Base { path, view }
            } else {
                BookmarkKind::file(path)
            }
        } else {
            self.note_status(self.tr("Nothing to bookmark", "Ingenting å bokmerke"), cx);
            return;
        };
        if self.settings.bookmarks.find_kind(&kind).is_none() {
            self.settings.bookmarks.add(kind, None, None);
            self.persist_bookmarks(cx);
        } else {
            self.note_status(self.tr("Already bookmarked", "Allerede bokmerket"), cx);
        }
        let _ = window;
    }

    fn bookmark_current_heading(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.visible_markdown_document(cx) else {
            self.note_status(
                self.tr("Open a Markdown note first", "Åpne et Markdown-notat først"),
                cx,
            );
            return;
        };
        let (path, text, cursor) = {
            let doc = doc.read(cx);
            (
                doc.path.clone(),
                doc.editor.read(cx).value().to_string(),
                doc.editor.read(cx).cursor(),
            )
        };
        let Some((normalized, occurrence)) = crate::bookmarks::capture_heading(&text, cursor)
        else {
            self.note_status(
                self.tr("No heading at the caret", "Ingen overskrift ved markøren"),
                cx,
            );
            return;
        };
        let kind = BookmarkKind::File {
            path,
            anchor: Some(normalized),
            occurrence,
        };
        if self.settings.bookmarks.find_kind(&kind).is_none() {
            self.settings.bookmarks.add(kind, None, None);
            self.persist_bookmarks(cx);
        }
    }

    fn bookmark_current_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.visible_markdown_document(cx) else {
            self.note_status(
                self.tr("Open a Markdown note first", "Åpne et Markdown-notat først"),
                cx,
            );
            return;
        };
        if doc.read(cx).is_read_only() {
            self.note_status(
                self.tr(
                    "This document is read-only",
                    "Dette dokumentet er skrivebeskyttet",
                ),
                cx,
            );
            return;
        }
        let Some(anchor) = doc.update(cx, |doc, cx| doc.ensure_block_id(window, cx)) else {
            self.note_status(
                self.tr("No block at the caret", "Ingen blokk ved markøren"),
                cx,
            );
            return;
        };
        let kind = BookmarkKind::File {
            path: doc.read(cx).path.clone(),
            anchor: Some(format!("^{anchor}")),
            occurrence: 0,
        };
        if self.settings.bookmarks.find_kind(&kind).is_none() {
            self.settings.bookmarks.add(kind, None, None);
            self.persist_bookmarks(cx);
        }
    }

    fn activate_bookmark(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.settings.bookmarks.find(id).cloned() else {
            return;
        };
        match item.kind {
            BookmarkKind::File {
                path,
                anchor,
                occurrence,
            } => {
                if !path.is_file() {
                    self.note_status(
                        self.tr("Bookmark target is missing", "Bokmerkemålet mangler"),
                        cx,
                    );
                    return;
                }
                if let Some(anchor) = anchor {
                    self.open_document(path.clone(), window, cx);
                    let Some(doc) = self.active_doc().cloned() else {
                        return;
                    };
                    if doc.read(cx).path != path {
                        self.note_status(
                            self.tr(
                                "Bookmark document could not be opened",
                                "Bokmerkedokumentet kunne ikke åpnes",
                            ),
                            cx,
                        );
                        return;
                    }
                    let text = doc.read(cx).editor.read(cx).value().to_string();
                    if let Some(block) = anchor.strip_prefix('^') {
                        let lines: Vec<&str> = text.split('\n').collect();
                        if let Some((start, _)) = preview::block_lines(&lines, block) {
                            self.navigate_search_result(path, Some(start + 1), window, cx);
                        } else {
                            self.note_status(
                                self.tr("Block target is missing", "Blokkmålet mangler"),
                                cx,
                            );
                        }
                    } else {
                        if let Some(line) =
                            crate::bookmarks::resolve_heading(&text, &anchor, occurrence)
                        {
                            self.navigate_search_result(path, Some(line), window, cx);
                        } else {
                            self.note_status(
                                self.tr("Heading target is missing", "Overskriftsmålet mangler"),
                                cx,
                            );
                        }
                    }
                } else {
                    self.open_document(path, window, cx);
                }
            }
            BookmarkKind::Folder { path, root } => {
                if !path.is_dir() {
                    self.note_status(
                        self.tr("Folder target is missing", "Mappemålet mangler"),
                        cx,
                    );
                    return;
                }
                let path_root = path.canonicalize().unwrap_or_else(|_| path.clone());
                let current_root = self.vault.read(cx).root.clone();
                let current_canonical = current_root
                    .as_ref()
                    .map(|root| root.canonicalize().unwrap_or_else(|_| root.clone()));
                let root = root
                    .or_else(|| {
                        current_root.clone().filter(|_| {
                            current_canonical
                                .as_ref()
                                .is_some_and(|root| path_root.starts_with(root))
                        })
                    })
                    .unwrap_or_else(|| path.clone());
                let Ok(canonical_root) = root.canonicalize() else {
                    self.note_status(
                        self.tr("Folder vault is missing", "Mappens hvelv mangler"),
                        cx,
                    );
                    return;
                };
                if !canonical_root.is_dir() {
                    self.note_status(
                        self.tr("Folder vault is missing", "Mappens hvelv mangler"),
                        cx,
                    );
                    return;
                }
                if !path_root.starts_with(&canonical_root) {
                    self.note_status(
                        self.tr(
                            "Folder is outside its saved vault",
                            "Mappen er utenfor det lagrede hvelvet",
                        ),
                        cx,
                    );
                    return;
                }
                if current_root.as_ref() != Some(&root) && !self.open_vault_at(root, window, cx) {
                    return;
                }
                self.open_folder_page(path, window, cx);
            }
            BookmarkKind::Search {
                root,
                query,
                match_case,
                sort,
            } => {
                if !root.is_dir() {
                    self.note_status(
                        self.tr("Search vault is missing", "Søkehvelvet mangler"),
                        cx,
                    );
                    return;
                }
                if self.vault.read(cx).root.as_ref() != Some(&root)
                    && !self.open_vault_at(root, window, cx)
                {
                    return;
                }
                search::open_project_search_saved(cx.entity(), query, match_case, sort, window, cx);
            }
            BookmarkKind::Graph { root, center } => {
                if !root.is_dir() {
                    self.note_status(self.tr("Graph vault is missing", "Grafhvelvet mangler"), cx);
                    return;
                }
                if center.as_ref().is_some_and(|center| !center.is_file()) {
                    self.note_status(
                        self.tr("Graph center is missing", "Grafens midtpunkt mangler"),
                        cx,
                    );
                    return;
                }
                if self.vault.read(cx).root.as_ref() != Some(&root)
                    && !self.open_vault_at(root, window, cx)
                {
                    return;
                }
                if let Some(center) = center {
                    self.open_local_graph_for(center, window, cx);
                } else {
                    if self.graph.is_some() {
                        self.graph = None;
                    }
                    self.on_open_graph(&OpenGraph, window, cx);
                }
            }
            BookmarkKind::Base { path, view } => {
                if !path.is_file() {
                    self.note_status(
                        self.tr("Base view target is missing", "Base-visningsmålet mangler"),
                        cx,
                    );
                    return;
                }
                self.open_document(path.clone(), window, cx);
                let Some(doc) = self.active_doc().cloned() else {
                    return;
                };
                if doc.read(cx).path != path {
                    self.note_status(
                        self.tr(
                            "Base view could not be opened",
                            "Base-visningen kunne ikke åpnes",
                        ),
                        cx,
                    );
                    return;
                }
                let base = self
                    .active
                    .and_then(|ix| self.docs.get(ix))
                    .and_then(|doc| doc.base.clone());
                let Some(base) = base else {
                    self.note_status(
                        self.tr(
                            "Base view is unavailable",
                            "Base-visningen er utilgjengelig",
                        ),
                        cx,
                    );
                    return;
                };
                if base.read(cx).has_view_named(&view, cx) {
                    base.update(cx, |base, cx| base.select_view_by_name(&view, cx));
                } else {
                    self.note_status(
                        self.tr("Saved view is missing", "Den lagrede visningen mangler"),
                        cx,
                    );
                }
            }
            BookmarkKind::Group { .. } => {}
        }
    }

    fn show_new_group_dialog(
        &mut self,
        parent: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window, cx);
        let input = cx.new(|cx| InputState::new(window, cx));
        let dialog_input = input.clone();
        let ok_input = input.clone();
        let view = cx.entity();
        let title = self.tr("New bookmark group", "Ny bokmerkegruppe");
        let cancel_label = self.tr("Cancel", "Avbryt");
        let ok_label = self.tr("OK", "OK");
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title)
                .w(px(380.))
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .show_cancel(true)
                        .cancel_text(cancel_label)
                        .ok_text(ok_label),
                )
                .child(Input::new(&dialog_input).appearance(true))
                .on_ok({
                    let input = ok_input.clone();
                    let view = view.clone();
                    move |_, _window, cx| {
                        let name = input.read(cx).value().trim().to_string();
                        if name.is_empty() {
                            return false;
                        }
                        view.update(cx, |this, cx| {
                            this.settings.bookmarks.add(
                                BookmarkKind::Group {
                                    items: Vec::new(),
                                    collapsed: false,
                                },
                                Some(name),
                                parent,
                            );
                            this.persist_bookmarks(cx);
                        });
                        true
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
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx))
        });
    }

    fn show_bookmark_rename_dialog(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window, cx);
        let Some(item) = self.settings.bookmarks.find(id) else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        let dialog_input = input.clone();
        let ok_input = input.clone();
        let initial = item
            .title
            .clone()
            .unwrap_or_else(|| Self::bookmark_title(item, self.settings.language));
        input.update(cx, |input, cx| input.set_value(initial, window, cx));
        let view = cx.entity();
        let title = self.tr("Rename bookmark", "Gi nytt navn på bokmerke");
        let cancel_label = self.tr("Cancel", "Avbryt");
        let ok_label = self.tr("OK", "OK");
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title)
                .w(px(380.))
                .button_props(
                    gpui_kit::component::dialog::DialogButtonProps::default()
                        .show_cancel(true)
                        .cancel_text(cancel_label)
                        .ok_text(ok_label),
                )
                .child(Input::new(&dialog_input).appearance(true))
                .on_ok({
                    let input = ok_input.clone();
                    let view = view.clone();
                    move |_, _window, cx| {
                        let name = input.read(cx).value().trim().to_string();
                        let is_group = view
                            .read(cx)
                            .settings
                            .bookmarks
                            .find(id)
                            .is_some_and(|item| matches!(item.kind, BookmarkKind::Group { .. }));
                        if is_group && name.is_empty() {
                            return false;
                        }
                        view.update(cx, |this, cx| {
                            if let Some(item) = this.settings.bookmarks.find_mut(id) {
                                item.title = (!name.is_empty()).then_some(name);
                            }
                            this.persist_bookmarks(cx);
                        });
                        true
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
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx))
        });
    }

    fn remove_bookmark(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let nonempty_group = self.settings.bookmarks.find(id).is_some_and(
            |item| matches!(&item.kind, BookmarkKind::Group { items, .. } if !items.is_empty()),
        );
        if !nonempty_group {
            self.settings.bookmarks.remove(id);
            self.persist_bookmarks(cx);
            return;
        }
        self.defer_bookmark_dialog(window, cx, move |this, window, cx| {
            this.confirm_remove_bookmark(id, window, cx)
        });
    }

    fn confirm_remove_bookmark(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let nonempty_group = self.settings.bookmarks.find(id).is_some_and(
            |item| matches!(&item.kind, BookmarkKind::Group { items, .. } if !items.is_empty()),
        );
        if !nonempty_group {
            return;
        }
        self.focus_handle.focus(window, cx);
        let view = cx.entity();
        let title = self.tr("Remove non-empty group?", "Fjerne gruppe som ikke er tom?");
        let description = self.tr(
            "Only bookmark references will be removed.",
            "Bare bokmerkereferanser fjernes.",
        );
        let remove_label = self.tr("Remove", "Fjern");
        let cancel_label = self.tr("Cancel", "Avbryt");
        let dialog_view = view.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title)
                .description(description)
                .show_cancel(true)
                .cancel_text(cancel_label)
                .ok_text(remove_label)
                .on_ok({
                    let dialog_view = dialog_view.clone();
                    move |_, _window, cx| {
                        dialog_view.update(cx, |this, cx| {
                            this.settings.bookmarks.remove(id);
                            this.persist_bookmarks(cx);
                        });
                        true
                    }
                })
                .on_cancel(|_, _, _| true)
                .on_close({
                    let dialog_view = dialog_view.clone();
                    move |_, window, cx| {
                        dialog_view.update(cx, |this, cx| this.refocus(window, cx));
                    }
                })
        });
    }

    fn defer_bookmark_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        open: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                this.focus_handle.focus(window, cx);
                open(this, window, cx);
            });
        });
    }

    fn import_vault_bookmarks(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.note_status(self.tr("Open a vault first", "Åpne et hvelv først"), cx);
            return;
        };
        let path = root.join(".obsidian/bookmarks.json");
        match std::fs::read_to_string(&path) {
            Ok(raw) => match self.settings.bookmarks.import_vault(&root, &raw) {
                Ok(result) => {
                    let imported = result.imported;
                    let skipped = result.skipped;
                    self.settings.bookmarks.items.extend(result.items);
                    self.persist_bookmarks(cx);
                    self.note_status(
                        if self.settings.language == crate::settings::Language::Norwegian {
                            format!("{imported} bokmerker importert, {skipped} hoppet over")
                        } else {
                            format!("{imported} bookmarks imported, {skipped} skipped")
                        },
                        cx,
                    );
                }
                Err(error) => {
                    eprintln!(
                        "Failed to parse vault bookmarks from {}: {error}",
                        path.display()
                    );
                    self.note_status(
                        self.tr(
                            "The vault bookmark file contains invalid JSON",
                            "Hvelvets bokmerkefil inneholder ugyldig JSON",
                        ),
                        cx,
                    )
                }
            },
            Err(error) => {
                eprintln!(
                    "Failed to read vault bookmarks from {}: {error}",
                    path.display()
                );
                self.note_status(
                    if error.kind() == std::io::ErrorKind::NotFound {
                        self.tr(
                            "No vault bookmark file was found",
                            "Fant ingen bokmerkefil i hvelvet",
                        )
                    } else {
                        self.tr(
                            "Could not read the vault bookmark file",
                            "Kunne ikke lese hvelvets bokmerkefil",
                        )
                    },
                    cx,
                )
            }
        }
    }
}
