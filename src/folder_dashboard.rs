use super::*;
use crate::folder::{self, Kind};
use gpui_kit::component::text::TextViewState;

pub(super) struct FolderPage {
    pub path: PathBuf,
    pub scroll: ScrollHandle,
    contents: std::io::Result<folder::Contents>,
    introduction: Option<Entity<TextViewState>>,
    introduction_error: bool,
    banner: Option<preview::BannerSpec>,
    folds: preview::CalloutFolds,
    embeds: preview::EmbedViews,
}

impl Workspace {
    pub(super) fn tr(&self, en: &'static str, nb: &'static str) -> &'static str {
        self.settings.language.text(en, nb)
    }

    pub(super) fn load_folder_page(
        &self,
        path: PathBuf,
        scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> FolderPage {
        let contents = folder::read(&path);
        let mut introduction = None;
        let mut introduction_error = false;
        let mut banner = None;
        if let Some(note) = contents.as_ref().ok().and_then(|c| c.introduction.as_ref()) {
            // Prefer the open buffer; navigating away must never show stale saved text.
            let raw = self
                .docs
                .iter()
                .find(|d| d.entity.read(cx).path == *note)
                .map(|d| Ok(d.entity.read(cx).editor.read(cx).value().to_string()))
                .unwrap_or_else(|| std::fs::read_to_string(note));
            match raw {
                Ok(raw) => {
                    let vault = self.vault.read(cx);
                    let resolver = vault.image_resolver();
                    banner = preview::banner_spec(&raw, &path, &*resolver);
                    let body = crate::properties::frontmatter_span(&raw)
                        .map(|span| &raw[span.end..])
                        .unwrap_or(&raw);
                    let text = preview::preprocess(body, note, vault.root.as_deref(), &*resolver);
                    introduction = Some(cx.new(|cx| TextViewState::markdown(&text, cx)));
                }
                Err(_) => introduction_error = true,
            }
        }
        FolderPage {
            path,
            scroll,
            contents,
            introduction,
            introduction_error,
            banner,
            folds: preview::CalloutFolds::default(),
            embeds: preview::EmbedViews::default(),
        }
    }

    pub(super) fn open_folder_page(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        // No file reveal here: opening a page must not change disclosure state.
        let path = path.canonicalize().unwrap_or(path);
        if !path.starts_with(&root) {
            return;
        }
        self.folder = Some(self.load_folder_page(path.clone(), ScrollHandle::new(), cx));
        self.active = None;
        self.graph = None;
        self.peek = None;
        self.record_nav(&path);
        self.persist_tabs(cx);
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    fn edit_folder_introduction(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let note = self
            .folder
            .as_ref()
            .and_then(|p| p.contents.as_ref().ok())
            .and_then(|c| c.introduction.clone())
            .unwrap_or_else(|| folder::introduction_path(&path));
        if !note.exists() {
            use std::io::Write;
            let result = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&note)
                .and_then(|mut f| {
                    writeln!(
                        f,
                        "# {}\n",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )
                });
            if result.is_err() {
                self.note_status(
                    self.tr(
                        "Could not create the introduction",
                        "Kunne ikke opprette introduksjonen",
                    ),
                    cx,
                );
                return;
            }
            self.vault.update(cx, |v, cx| v.refresh(cx));
        }
        self.set_view_mode(ViewMode::Source, window, cx);
        self.open_document(note, window, cx);
    }

    pub(super) fn folder_breadcrumb(&self, path: &Path, cx: &mut Context<Self>) -> Div {
        let root = self.vault.read(cx).root.clone().unwrap_or_default();
        let mut crumbs = vec![root.clone()];
        if let Ok(relative) = path.strip_prefix(&root) {
            let mut target = root;
            for part in relative.components() {
                target.push(part);
                crumbs.push(target.clone());
            }
        }
        h_flex()
            .w_full()
            .flex_wrap()
            .gap_1()
            .items_center()
            .children(crumbs.into_iter().enumerate().map(|(ix, target)| {
                h_flex()
                    .gap_1()
                    .items_center()
                    .when(ix > 0, |row| {
                        row.child(div().text_color(cx.theme().muted_foreground).child("›"))
                    })
                    .child(
                        Button::new(("folder-crumb", ix))
                            .ghost()
                            .xsmall()
                            .label(
                                target
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_folder_page(target.clone(), window, cx)
                            })),
                    )
            }))
    }

    pub(super) fn render_folder_page(
        &self,
        page: &FolderPage,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let path = page.path.clone();
        let title = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let mut content = v_flex()
            .w_full()
            .max_w(px(960.))
            .gap_6()
            .px_6()
            .py_6()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        Icon::new(assets::IconName::FolderOpen)
                            .size_6()
                            .text_color(theme.muted_foreground),
                    )
                    .child(div().text_2xl().font_semibold().child(title)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("folder-new-note")
                            .ghost()
                            .small()
                            .icon(assets::IconName::FilePlus)
                            .label(self.tr("New note", "Nytt notat"))
                            .on_click(cx.listener({
                                let path = path.clone();
                                move |this, _, window, cx| {
                                    this.set_view_mode(ViewMode::Source, window, cx);
                                    this.new_file_in(path.clone(), window, cx);
                                }
                            })),
                    )
                    .child(
                        Button::new("folder-introduction")
                            .ghost()
                            .small()
                            .icon(assets::IconName::SquarePen)
                            .label(self.tr("Edit introduction", "Rediger introduksjon"))
                            .on_click(cx.listener({
                                let path = path.clone();
                                move |this, _, window, cx| {
                                    this.edit_folder_introduction(path.clone(), window, cx);
                                }
                            })),
                    ),
            );
        if let Some(intro) = &page.introduction {
            let view = cx.entity();
            let context = preview::PreviewCtx {
                vault: self.vault.clone(),
                workspace: cx.weak_entity(),
                views: page.embeds.clone(),
                doc_path: page
                    .contents
                    .as_ref()
                    .ok()
                    .and_then(|c| c.introduction.clone()),
                depth: 0,
            };
            content = content.child(
                TextView::new(intro)
                    .markdown_extensions(preview::extensions(&page.folds, Some(&context), false))
                    .selectable(true)
                    .scrollable(false)
                    .w_full()
                    .on_link_click(move |link, _, window, cx| {
                        if let Some(target) = link.strip_prefix("wiki:") {
                            view.update(cx, |this, cx| this.open_wikilink(target, window, cx));
                        } else {
                            cx.open_url(link);
                        }
                    }),
            );
        }
        if page.introduction_error {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.tr(
                        "Could not read the introduction",
                        "Kunne ikke lese introduksjonen",
                    )),
            );
        }
        match &page.contents {
            Err(_) => {
                content = content.child(div().text_sm().child(self.tr(
                    "This folder is unavailable. Use the breadcrumbs to go back.",
                    "Denne mappen er utilgjengelig. Bruk mappestien for å gå tilbake.",
                )))
            }
            Ok(contents) => {
                if contents.entries.is_empty() {
                    content = content.child(
                        v_flex()
                            .gap_2()
                            .py_6()
                            .child(div().font_medium().child(
                                self.tr("A place for your next idea", "Et sted for din neste idé"),
                            ))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.tr(
                                        "Create a note or add files to this folder.",
                                        "Opprett et notat eller legg filer i denne mappen.",
                                    )),
                            ),
                    );
                }
                for (kind, en, nb) in [
                    (Kind::Folder, "Folders", "Mapper"),
                    (Kind::Note, "Notes", "Notater"),
                    (Kind::Database, "Databases", "Databaser"),
                    (Kind::Image, "Images", "Bilder"),
                ] {
                    let entries: Vec<_> =
                        contents.entries.iter().filter(|e| e.kind == kind).collect();
                    if entries.is_empty() {
                        continue;
                    }
                    let count = entries.len();
                    let mut group = v_flex().w_full().gap_2().child(
                        h_flex()
                            .justify_between()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.tr(en, nb))
                            .child(count.to_string()),
                    );
                    let mut rows = h_flex().w_full().flex_wrap().gap_2();
                    for entry in entries {
                        let target = entry.path.clone();
                        let icon = match kind {
                            Kind::Folder => assets::IconName::FolderClosed,
                            Kind::Note => assets::IconName::FileText,
                            Kind::Database => assets::IconName::Database,
                            Kind::Image => assets::IconName::FileImage,
                        };
                        rows = rows.child(
                            h_flex()
                                .id(SharedString::from(format!(
                                    "folder-object-{}",
                                    target.display()
                                )))
                                .items_center()
                                .gap_3()
                                .px_3()
                                .py_3()
                                .rounded(cx.theme().radius)
                                .when(kind == Kind::Folder, |row| {
                                    row.w(px(212.)).border_1().border_color(cx.theme().border)
                                })
                                .when(kind != Kind::Folder, |row| row.w_full())
                                .cursor_pointer()
                                .hover(|s| s.bg(cx.theme().accent))
                                .child(
                                    Icon::new(icon)
                                        .size_4()
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .truncate()
                                        .child(entry.name.clone()),
                                )
                                .child(
                                    Icon::new(assets::IconName::ChevronRight)
                                        .size_3()
                                        .text_color(cx.theme().muted_foreground),
                                )
                                .on_click(cx.listener(
                                    move |this, ev: &gpui::ClickEvent, window, cx| {
                                        if kind == Kind::Database {
                                            this.set_view_mode(ViewMode::Preview, window, cx);
                                        }
                                        if kind == Kind::Folder {
                                            this.open_folder_page(target.clone(), window, cx);
                                        } else if ev.modifiers().platform {
                                            this.open_document_new_tab(target.clone(), window, cx);
                                        } else {
                                            this.open_document(target.clone(), window, cx);
                                        }
                                    },
                                )),
                        );
                    }
                    group = group.child(rows);
                    content = content.child(group);
                }
            }
        }
        v_flex()
            .size_full()
            .child(div().px_4().py_1().child(self.folder_breadcrumb(&path, cx)))
            .child(
                div()
                    .id("folder-page-scroll")
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_y_scroll()
                    .track_scroll(&page.scroll)
                    .child(
                        v_flex()
                            .w_full()
                            .items_center()
                            .when_some(page.banner.clone(), |d, banner| {
                                d.child(render_banner(&banner))
                            })
                            .child(content),
                    )
                    .vertical_scrollbar(&page.scroll),
            )
            .into_any_element()
    }

    pub(super) fn render_inspector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let properties = self
            .active_doc()
            .map(|d| crate::properties::properties(d.read(cx).editor.read(cx).value().as_ref()))
            .unwrap_or_default();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h_11()
                    .px_3()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(self.tr("Inspector", "Inspektør")),
                    )
                    .child(
                        Button::new("close-inspector")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::X)
                            .tooltip(self.tr("Hide inspector", "Skjul inspektør"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.inspector_open = false;
                                this.settings.save();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("inspector-details")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.sidebar_details_scroll)
                    .child(
                        v_flex()
                            .w_full()
                            .px_2()
                            .py_2()
                            .when(self.active_doc().is_some(), |column| {
                                column.child(
                                    v_flex()
                                        .gap_2()
                                        .px_2()
                                        .pb_3()
                                        .child(
                                            h_flex()
                                                .justify_between()
                                                .items_center()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(self.tr("Properties", "Egenskaper")),
                                                )
                                                .child(
                                                    Button::new("inspector-properties")
                                                        .ghost()
                                                        .xsmall()
                                                        .icon(assets::IconName::SquarePen)
                                                        .tooltip(self.tr(
                                                            "Edit properties",
                                                            "Rediger egenskaper",
                                                        ))
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.show_properties(window, cx)
                                                            },
                                                        )),
                                                ),
                                        )
                                        .children(properties.into_iter().map(|(key, value)| {
                                            h_flex()
                                                .gap_2()
                                                .text_xs()
                                                .child(
                                                    div()
                                                        .w(px(88.))
                                                        .truncate()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(key),
                                                )
                                                .child(
                                                    div().flex_1().min_w_0().truncate().child(
                                                        match value {
                                                            serde_yaml::Value::String(s) => s,
                                                            value => serde_yaml::to_string(&value)
                                                                .unwrap_or_default()
                                                                .trim()
                                                                .to_string(),
                                                        },
                                                    ),
                                                )
                                        })),
                                )
                            })
                            .child(self.render_outline(cx))
                            .child(self.render_backlinks_pane(cx))
                            .child(self.render_outgoing_pane(cx))
                            .child(self.render_tasks(cx))
                            .child(self.render_calendar_pane(cx))
                            .child(self.render_tags(cx)),
                    )
                    .vertical_scrollbar(&self.sidebar_details_scroll),
            )
    }
}
