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
    query_embeds: preview::QueryEmbedViews,
    metadata: folder::Metadata,
    config: folder::Dashboard,
    cards: Vec<FolderCard>,
    pub pages: [usize; 7],
}

struct FolderCard {
    path: PathBuf,
    kind: Kind,
    metadata: folder::Metadata,
    banner: Option<preview::BannerSpec>,
    modified: Option<SystemTime>,
    count: usize,
}

fn folder_icon(metadata: &folder::Metadata, fallback: assets::IconName, cx: &App) -> AnyElement {
    if let Some(icon) = metadata.icon.as_deref() {
        if let Some(path) = crate::note_icons::icon_path(icon) {
            return svg()
                .path(path)
                .size_5()
                .text_color(cx.theme().muted_foreground)
                .into_any_element();
        }
        return div().text_lg().child(icon.to_string()).into_any_element();
    }
    Icon::new(fallback)
        .size_5()
        .text_color(cx.theme().muted_foreground)
        .into_any_element()
}

impl Workspace {
    pub(crate) fn tr(&self, en: &'static str, nb: &'static str) -> &'static str {
        self.settings.language.text(en, nb)
    }

    pub(super) fn load_folder_page(
        &self,
        path: PathBuf,
        scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> FolderPage {
        let contents = folder::read(&path).map(|mut contents| {
            contents.entries.retain(|entry| {
                entry.kind == Kind::Folder
                    || crate::file_preview::visible(&entry.path, self.settings.show_other_files)
            });
            contents
        });
        let mut introduction = None;
        let mut introduction_error = false;
        let mut banner = None;
        let mut metadata =
            folder::Metadata::parse("", &path.file_name().unwrap_or_default().to_string_lossy());
        let mut config = folder::Dashboard::default();
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
                    metadata = folder::Metadata::parse(&raw, &metadata.title);
                    if !crate::properties::properties(&raw)
                        .iter()
                        .any(|(key, _)| key == "description")
                    {
                        metadata.description.clear();
                    }
                    config = folder::dashboard(&raw);
                    let vault = self.vault.read(cx);
                    let resolver = vault.image_resolver();
                    banner = preview::banner_spec(&raw, &path, &*resolver);
                    let body = folder::introduction_body(&raw, &metadata.title);
                    let marks = preview::MarkColors::from_theme(cx.theme());
                    let text =
                        preview::preprocess(body, note, vault.root.as_deref(), &*resolver, &marks);
                    introduction = Some(cx.new(|cx| TextViewState::markdown(&text, cx)));
                }
                Err(_) => introduction_error = true,
            }
        }
        let vault = self.vault.read(cx);
        let resolver = vault.image_resolver();
        let cards = contents
            .as_ref()
            .map(|contents| {
                contents
                    .entries
                    .iter()
                    .filter(|entry| Some(&entry.path) != contents.introduction.as_ref())
                    .map(|entry| {
                        let note = if entry.kind == Kind::Folder {
                            let conventional = folder::introduction_path(&entry.path);
                            if conventional.is_file() {
                                conventional
                            } else {
                                entry.path.join("README.md")
                            }
                        } else {
                            entry.path.clone()
                        };
                        let raw = if matches!(entry.kind, Kind::Folder | Kind::Note) {
                            self.docs
                                .iter()
                                .find(|d| d.entity.read(cx).path == note)
                                .map(|d| d.entity.read(cx).editor.read(cx).value().to_string())
                                .unwrap_or_else(|| folder::summary(&note))
                        } else {
                            String::new()
                        };
                        let fallback =
                            if matches!(entry.kind, Kind::Folder | Kind::Other | Kind::Image) {
                                entry.name.clone()
                            } else {
                                entry
                                    .path
                                    .file_stem()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned()
                            };
                        FolderCard {
                            path: entry.path.clone(),
                            kind: entry.kind,
                            metadata: folder::Metadata::parse(&raw, &fallback),
                            banner: if entry.kind == Kind::Image {
                                Some(preview::BannerSpec {
                                    source: preview::BannerSource::File(entry.path.clone()),
                                    y: None,
                                    icon: None,
                                })
                            } else {
                                preview::banner_spec(
                                    &raw,
                                    note.parent().unwrap_or(&path),
                                    &*resolver,
                                )
                            },
                            modified: std::fs::metadata(&entry.path)
                                .and_then(|m| m.modified())
                                .ok(),
                            count: if entry.kind == Kind::Folder {
                                vault
                                    .notes
                                    .iter()
                                    .filter(|p| p.starts_with(&entry.path))
                                    .count()
                            } else {
                                0
                            },
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        FolderPage {
            path,
            scroll,
            contents,
            introduction,
            introduction_error,
            banner,
            folds: preview::CalloutFolds::default(),
            embeds: preview::EmbedViews::default(),
            query_embeds: preview::QueryEmbedViews::default(),
            metadata,
            config,
            cards,
            pages: [0; 7],
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
        self.folder_search
            .update(cx, |input, cx| input.set_value("", window, cx));
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
                let siblings = target.clone();
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
                    .child(self.sibling_button(ix, siblings, cx))
            }))
    }

    pub(super) fn render_folder_page(
        &self,
        page: &FolderPage,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.settings.folder_file_list {
            return self.render_folder_files(page, cx);
        }
        let muted_foreground = cx.theme().muted_foreground;
        let path = page.path.clone();
        let title = page.metadata.title.clone();
        let mut content = v_flex()
            .w_full()
            .max_w(px(1060.))
            .gap_6()
            .px_6()
            .py_6()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(folder_icon(
                        &page.metadata,
                        assets::IconName::FolderOpen,
                        cx,
                    ))
                    .child(div().text_2xl().font_semibold().child(title)),
            )
            .when(!page.metadata.description.is_empty(), |d| {
                d.child(
                    div()
                        .text_sm()
                        .text_color(muted_foreground)
                        .child(page.metadata.description.clone()),
                )
            })
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
                        Button::new("folder-new-folder")
                            .ghost()
                            .small()
                            .icon(assets::IconName::FolderPlus)
                            .label(self.tr("New folder", "Ny mappe"))
                            .on_click(cx.listener({
                                let path = path.clone();
                                move |this, _, window, cx| {
                                    let label = this.tr("New folder", "Ny mappe");
                                    let mut target = path.join(label);
                                    let mut n = 2;
                                    while target.exists() {
                                        target = path.join(format!("{label} {n}"));
                                        n += 1;
                                    }
                                    match std::fs::create_dir(&target) {
                                        Ok(()) => {
                                            this.vault.update(cx, |v, cx| v.refresh(cx));
                                            this.open_folder_page(target, window, cx);
                                        }
                                        Err(_) => this.note_status(
                                            this.tr(
                                                "Could not create the folder",
                                                "Kunne ikke opprette mappen",
                                            ),
                                            cx,
                                        ),
                                    }
                                }
                            })),
                    )
                    .child(
                        Button::new("folder-introduction")
                            .ghost()
                            .small()
                            .icon(assets::IconName::SquarePen)
                            .label(self.tr("Customize page", "Tilpass side"))
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
                properties_visibility: self.settings.properties_visibility,
                language: self.settings.language,
                vault: self.vault.clone(),
                workspace: cx.weak_entity(),
                views: page.embeds.clone(),
                query_views: page.query_embeds.clone(),
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
                content = content.child(self.folder_controls(page, cx));
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
                let query = self.folder_search.read(cx).value().to_string();
                let pinned = |card: &&FolderCard| {
                    page.config
                        .pinned
                        .iter()
                        .position(|p| page.path.join(p) == card.path)
                };
                for (section, (kind, is_pinned, en, nb)) in [
                    (Kind::Note, true, "Pinned", "Festet"),
                    (Kind::Folder, false, "Folders", "Mapper"),
                    (Kind::Note, false, "Notes", "Notater"),
                    (Kind::Database, false, "Databases", "Databaser"),
                    (Kind::Image, false, "Images", "Bilder"),
                    (Kind::Other, false, "Other files", "Andre filer"),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut entries: Vec<_> = page
                        .cards
                        .iter()
                        .filter(|e| e.kind == kind)
                        .filter(|e| kind != Kind::Note || pinned(e).is_some() == is_pinned)
                        .filter(|e| e.metadata.matches(&query, &page.config))
                        .collect();
                    entries.sort_by(|a, b| {
                        if is_pinned {
                            pinned(a).cmp(&pinned(b))
                        } else if page.config.sort == folder::Sort::Modified {
                            b.modified.cmp(&a.modified).then(a.path.cmp(&b.path))
                        } else if page.config.sort == folder::Sort::Type {
                            crate::file_preview::extension(&a.path)
                                .cmp(&crate::file_preview::extension(&b.path))
                                .then(
                                    a.metadata
                                        .title
                                        .to_lowercase()
                                        .cmp(&b.metadata.title.to_lowercase()),
                                )
                                .then(a.path.cmp(&b.path))
                        } else {
                            a.metadata
                                .title
                                .to_lowercase()
                                .cmp(&b.metadata.title.to_lowercase())
                                .then(a.path.cmp(&b.path))
                        }
                    });
                    if entries.is_empty() {
                        continue;
                    }
                    let count = entries.len();
                    let range = folder::page_range(count, page.pages[section]);
                    let mut group = v_flex().w_full().gap_2().child(
                        h_flex()
                            .justify_between()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.tr(en, nb))
                            .child(self.folder_pagination(page, section, count, cx)),
                    );
                    let mut rows = h_flex().w_full().items_stretch().flex_wrap().gap_2();
                    for entry in &entries[range] {
                        rows = rows.child(self.folder_card(entry, page, is_pinned, cx));
                    }
                    group = group.child(rows);
                    content = content.child(group);
                }
                if !page.cards.is_empty()
                    && !page
                        .cards
                        .iter()
                        .any(|e| e.metadata.matches(&query, &page.config))
                {
                    content =
                        content.child(div().py_6().text_sm().text_color(muted_foreground).child(
                            self.tr(
                                "No matching pages. Clear search or filters.",
                                "Ingen treff. Tøm søk eller filtre.",
                            ),
                        ));
                }
                let tasks: Vec<_> = self
                    .vault
                    .read(cx)
                    .tasks
                    .iter()
                    .filter(|t| t.path.starts_with(&path))
                    .take(5)
                    .cloned()
                    .collect();
                if !tasks.is_empty()
                    && query.is_empty()
                    && page.config.tag.is_empty()
                    && page.config.status.is_empty()
                {
                    content = content.child(
                        v_flex()
                            .items_start()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted_foreground)
                                    .child(self.tr("Open tasks", "Åpne oppgaver")),
                            )
                            .children(tasks.into_iter().enumerate().map(|(ix, task)| {
                                let target = task.path.clone();
                                Button::new(("folder-task", ix))
                                    .ghost()
                                    .small()
                                    .icon(assets::IconName::Square)
                                    .label(task.text)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_document(target.clone(), window, cx)
                                    }))
                            })),
                    );
                }
            }
        }
        v_flex()
            .size_full()
            .child(self.folder_view_picker(cx))
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

    fn folder_view_picker(&self, cx: &mut Context<Self>) -> Div {
        h_flex().px_4().py_1().gap_2().children(
            [(false, "Dashboard", "Oversikt"), (true, "Files", "Filer")]
                .into_iter()
                .enumerate()
                .map(|(ix, (files, en, nb))| {
                    Button::new(("folder-view", ix))
                        .ghost()
                        .small()
                        .label(self.tr(en, nb))
                        .selected(self.settings.folder_file_list == files)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.settings.folder_file_list = files;
                            this.settings.save();
                            cx.notify();
                        }))
                }),
        )
    }

    fn render_folder_files(&self, page: &FolderPage, cx: &mut Context<Self>) -> AnyElement {
        let query = self.folder_search.read(cx).value().to_lowercase();
        let filter = self.vault.read(cx).explorer_filter;
        let mut entries: Vec<_> = page
            .contents
            .as_ref()
            .map(|contents| {
                contents
                    .entries
                    .iter()
                    .filter(|entry| entry.name.to_lowercase().contains(&query))
                    .filter(|entry| entry.kind == Kind::Folder || filter.accepts(&entry.path))
                    .map(|entry| (entry, std::fs::metadata(&entry.path).ok()))
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_by(|(a, am), (b, bm)| {
            (b.kind == Kind::Folder)
                .cmp(&(a.kind == Kind::Folder))
                .then_with(|| {
                    if a.kind == Kind::Folder {
                        return a.name.to_lowercase().cmp(&b.name.to_lowercase());
                    }
                    match self.settings.tree_sort {
                        TreeSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                        TreeSort::Type => crate::file_preview::extension(&a.path)
                            .cmp(&crate::file_preview::extension(&b.path)),
                        TreeSort::Modified => bm
                            .as_ref()
                            .and_then(|m| m.modified().ok())
                            .cmp(&am.as_ref().and_then(|m| m.modified().ok())),
                        TreeSort::Size => bm
                            .as_ref()
                            .map(|m| m.len())
                            .cmp(&am.as_ref().map(|m| m.len())),
                    }
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                })
        });
        let mut header = h_flex().w_full().px_2().gap_3();
        for (ix, (sort, en, nb, width)) in [
            (TreeSort::Name, "Name", "Navn", 0.),
            (TreeSort::Type, "Type", "Type", 80.),
            (TreeSort::Modified, "Modified", "Endret", 140.),
            (TreeSort::Size, "Size", "Størrelse", 80.),
        ]
        .into_iter()
        .enumerate()
        {
            header = header.child(
                div()
                    .when(width == 0., |d| d.flex_1().min_w_0())
                    .when(width > 0., |d| d.w(px(width)))
                    .child(
                        Button::new(("file-column", ix))
                            .ghost()
                            .xsmall()
                            .label(format!(
                                "{}{}",
                                self.tr(en, nb),
                                if self.settings.tree_sort == sort {
                                    if matches!(sort, TreeSort::Modified | TreeSort::Size) {
                                        " ↓"
                                    } else {
                                        " ↑"
                                    }
                                } else {
                                    ""
                                }
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.settings.tree_sort = sort;
                                this.settings.save();
                                this.vault.update(cx, |vault, cx| {
                                    vault.tree_sort = sort;
                                    vault.refresh(cx);
                                });
                                cx.notify();
                            })),
                    ),
            );
        }
        let mut rows = v_flex().w_full();
        if entries.is_empty() {
            rows = rows.child(
                div()
                    .px_3()
                    .py_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.tr(
                        "No matching files. Check the search and file filters.",
                        "Ingen samsvarende filer. Kontroller søket og filfiltrene.",
                    )),
            );
        }
        let count = entries.len();
        let range = folder::page_range(count, page.pages[6]);
        for (ix, (entry, metadata)) in entries
            .into_iter()
            .enumerate()
            .skip(range.start)
            .take(range.len())
        {
            let path = entry.path.clone();
            let context_path = path.clone();
            let drag_path = path.clone();
            let folder = entry.kind == Kind::Folder;
            let modified = metadata
                .as_ref()
                .and_then(|m| m.modified().ok())
                .map(|time| {
                    chrono::DateTime::<chrono::Local>::from(time)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_else(|| "—".into());
            rows = rows.child(
                h_flex()
                    .id(("file-list-row", ix))
                    .w_full()
                    .px_2()
                    .h_8()
                    .gap_3()
                    .items_center()
                    .text_sm()
                    .hover(|row| row.bg(cx.theme().muted))
                    .cursor_pointer()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(
                                Icon::new(if folder {
                                    assets::IconName::Folder
                                } else if entry.kind == Kind::Image {
                                    assets::IconName::FileImage
                                } else {
                                    assets::IconName::FileText
                                })
                                .size_4(),
                            )
                            .child(div().truncate().child(entry.name.clone())),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(if folder {
                                self.tr("Folder", "Mappe").to_string()
                            } else {
                                crate::file_preview::extension(&path).to_uppercase()
                            }),
                    )
                    .child(
                        div()
                            .w(px(140.))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(modified),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if folder {
                                "—".into()
                            } else {
                                metadata
                                    .map(|m| crate::explorer::size_label(m.len()))
                                    .unwrap_or_else(|| "—".into())
                            }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.open_explorer_path(
                                path.clone(),
                                event.click_count == 2 || event.modifiers.platform,
                                window,
                                cx,
                            );
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.show_file_menu(
                                context_path.clone(),
                                folder,
                                event.position,
                                window,
                                cx,
                            )
                        }),
                    )
                    .on_drag(drag_path.clone(), move |path, _, _, cx| {
                        cx.new(|_| TreeDragPreview {
                            label: path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string()
                                .into(),
                        })
                    })
                    .when(folder, |row| {
                        row.drag_over::<PathBuf>(|style, _, _, cx| style.bg(cx.theme().muted))
                            .on_drop::<PathBuf>(cx.listener(
                                move |this, source: &PathBuf, _, cx| {
                                    this.move_tree_entry(source.clone(), drag_path.clone(), cx)
                                },
                            ))
                    }),
            );
        }
        v_flex()
            .size_full()
            .child(self.folder_view_picker(cx))
            .child(
                div()
                    .px_4()
                    .py_1()
                    .child(self.folder_breadcrumb(&page.path, cx)),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .child(Input::new(&self.folder_search).small()),
            )
            .child(header)
            .when(count > folder::page_range(count, 0).len(), |view| {
                view.child(
                    div()
                        .px_4()
                        .py_1()
                        .child(self.folder_pagination(page, 6, count, cx)),
                )
            })
            .child(
                div()
                    .id("folder-file-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&page.scroll)
                    .child(rows),
            )
            .into_any_element()
    }

    fn folder_pagination(
        &self,
        page: &FolderPage,
        section: usize,
        total: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let first = folder::page_range(total, 0);
        if total <= first.len() {
            return div().text_xs().child(total.to_string()).into_any_element();
        }
        let range = folder::page_range(total, page.pages[section]);
        let current = range.start / first.len();
        let last = (total - 1) / first.len();
        h_flex()
            .gap_1()
            .items_center()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(format!(
                "{}–{} {} {total}",
                range.start + 1,
                range.end,
                self.tr("of", "av")
            ))
            .children(
                [
                    ("first", "First page", "Første side", "«", 0),
                    (
                        "previous",
                        "Previous page",
                        "Forrige side",
                        "‹",
                        current.saturating_sub(1),
                    ),
                    (
                        "next",
                        "Next page",
                        "Neste side",
                        "›",
                        (current + 1).min(last),
                    ),
                    ("last", "Last page", "Siste side", "»", last),
                ]
                .into_iter()
                .map(|(id, en, nb, label, target)| {
                    let path = page.path.clone();
                    Button::new(SharedString::from(format!("folder-page-{section}-{id}")))
                        .ghost()
                        .xsmall()
                        .label(label)
                        .tooltip(self.tr(en, nb))
                        .disabled(target == current)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(page) =
                                this.folder.as_mut().filter(|page| page.path == path)
                            {
                                page.pages[section] = target;
                                page.scroll.set_offset(point(px(0.), px(0.)));
                                cx.notify();
                            }
                        }))
                }),
            )
            .into_any_element()
    }

    fn folder_controls(&self, page: &FolderPage, cx: &mut Context<Self>) -> AnyElement {
        let mut controls = h_flex().w_full().gap_2().flex_wrap().child(
            div()
                .w(px(220.))
                .child(Input::new(&self.folder_search).small()),
        );
        for (layout, icon, en, nb) in [
            (
                folder::Layout::List,
                assets::IconName::List,
                "List",
                "Liste",
            ),
            (
                folder::Layout::Cards,
                assets::IconName::LayoutGrid,
                "Cards",
                "Kort",
            ),
            (
                folder::Layout::Gallery,
                assets::IconName::Images,
                "Gallery",
                "Galleri",
            ),
        ] {
            controls = controls.child(
                Button::new(en)
                    .ghost()
                    .small()
                    .icon(icon)
                    .label(self.tr(en, nb))
                    .selected(page.config.layout == layout)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(page) = &this.folder {
                            let mut config = page.config.clone();
                            config.layout = layout;
                            this.save_folder_view(config, window, cx);
                        }
                    })),
            );
        }
        controls = controls.child(
            Button::new("folder-sort")
                .ghost()
                .small()
                .icon(assets::IconName::ArrowDownWideNarrow)
                .label(match page.config.sort {
                    folder::Sort::Modified => self.tr("Recently edited", "Sist redigert"),
                    folder::Sort::Name => self.tr("Name", "Navn"),
                    folder::Sort::Type => self.tr("File type", "Filtype"),
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(page) = &this.folder {
                        let mut config = page.config.clone();
                        config.sort = match config.sort {
                            folder::Sort::Name => folder::Sort::Modified,
                            folder::Sort::Modified => folder::Sort::Type,
                            folder::Sort::Type => folder::Sort::Name,
                        };
                        this.save_folder_view(config, window, cx);
                    }
                })),
        );
        let tags: std::collections::BTreeSet<_> = page
            .cards
            .iter()
            .flat_map(|c| c.metadata.tags.iter().cloned())
            .collect();
        let statuses: std::collections::BTreeSet<_> = page
            .cards
            .iter()
            .map(|c| c.metadata.status.clone())
            .filter(|s| !s.is_empty())
            .collect();
        let mut filters = h_flex().gap_1().flex_wrap();
        for (ix, (value, is_tag)) in tags
            .into_iter()
            .map(|v| (v, true))
            .chain(statuses.into_iter().map(|v| (v, false)))
            .enumerate()
        {
            let selected = if is_tag {
                page.config.tag == value
            } else {
                page.config.status == value
            };
            filters = filters.child(
                Button::new(("folder-filter", ix))
                    .ghost()
                    .xsmall()
                    .label(if is_tag {
                        format!("#{value}")
                    } else {
                        value.clone()
                    })
                    .selected(selected)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(page) = &this.folder {
                            let mut config = page.config.clone();
                            let field = if is_tag {
                                &mut config.tag
                            } else {
                                &mut config.status
                            };
                            *field = if *field == value {
                                String::new()
                            } else {
                                value.clone()
                            };
                            this.save_folder_view(config, window, cx);
                        }
                    })),
            );
        }
        if !page.config.tag.is_empty() || !page.config.status.is_empty() {
            filters = filters.child(
                Button::new("folder-clear-filters")
                    .ghost()
                    .xsmall()
                    .label(self.tr("Clear filters", "Tøm filtre"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(page) = &this.folder {
                            let mut config = page.config.clone();
                            config.tag.clear();
                            config.status.clear();
                            this.save_folder_view(config, window, cx);
                        }
                    })),
            );
        }
        v_flex()
            .gap_2()
            .child(controls)
            .child(filters)
            .into_any_element()
    }

    fn save_folder_view(
        &mut self,
        config: folder::Dashboard,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(page) = &self.folder else {
            return;
        };
        let path = page.path.clone();
        let scroll = page.scroll.clone();
        let note = page
            .contents
            .as_ref()
            .ok()
            .and_then(|c| c.introduction.clone())
            .unwrap_or_else(|| folder::introduction_path(&path));
        let result = (|| -> anyhow::Result<()> {
            let doc = self
                .docs
                .iter()
                .find(|d| d.entity.read(cx).path == note)
                .map(|d| d.entity.clone());
            let raw = if let Some(doc) = &doc {
                anyhow::ensure!(!doc.read(cx).conflict, "conflict");
                doc.read(cx).editor.read(cx).value().to_string()
            } else if note.exists() {
                std::fs::read_to_string(&note)?
            } else {
                String::new()
            };
            if raw.starts_with("---") {
                let span = crate::properties::frontmatter_span(&raw)
                    .ok_or_else(|| anyhow::anyhow!("frontmatter"))?;
                let yaml = raw[..span.end]
                    .lines()
                    .skip(1)
                    .take_while(|l| !matches!(l.trim(), "---" | "..."))
                    .collect::<Vec<_>>()
                    .join("\n");
                let _: serde_yaml::Mapping = serde_yaml::from_str(&yaml)?;
            }
            let body =
                crate::properties::body_with(&raw, "dashboard", serde_yaml::to_value(&config)?);
            if let Some(doc) = doc {
                doc.update(cx, |doc, cx| doc.set_properties(body, window, cx));
            } else {
                let updated = crate::properties::splice_frontmatter(&raw, body);
                if !note.exists() {
                    use std::io::Write;
                    std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&note)?
                        .write_all(updated.as_bytes())?;
                } else {
                    anyhow::ensure!(std::fs::read_to_string(&note)? == raw, "changed");
                    std::fs::write(&note, updated)?;
                }
            }
            Ok(())
        })();
        if result.is_err() {
            self.note_status(self.tr("Could not save this view. Check the introduction's YAML and any external-edit conflict.", "Kunne ikke lagre visningen. Kontroller YAML i introduksjonen og eventuelle konflikter med eksterne endringer."), cx);
            return;
        }
        self.folder = Some(self.load_folder_page(path, scroll, cx));
        cx.notify();
    }

    fn folder_card(
        &self,
        card: &FolderCard,
        page: &FolderPage,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let kind = card.kind;
        let target = card.path.clone();
        let icon = match kind {
            Kind::Folder => assets::IconName::FolderClosed,
            Kind::Note => assets::IconName::FileText,
            Kind::Database => assets::IconName::Database,
            Kind::Image => assets::IconName::FileImage,
            Kind::Other => assets::IconName::File,
        };
        let list = page.config.layout == folder::Layout::List && kind != Kind::Folder;
        let mut header = h_flex()
            .gap_2()
            .items_center()
            .child(folder_icon(&card.metadata, icon, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_medium()
                    .child(card.metadata.title.clone()),
            );
        if kind == Kind::Note {
            let relative = card
                .path
                .strip_prefix(&page.path)
                .unwrap_or(&card.path)
                .to_string_lossy()
                .into_owned();
            header = header.child(
                Button::new(SharedString::from(format!("pin-{}", card.path.display())))
                    .ghost()
                    .xsmall()
                    .icon(assets::IconName::Pin)
                    .selected(pinned)
                    .tooltip(if pinned {
                        self.tr("Unpin page", "Løsne side")
                    } else {
                        self.tr("Pin page", "Fest side")
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        if let Some(page) = &this.folder {
                            let mut config = page.config.clone();
                            if pinned {
                                config.pinned.retain(|p| p != &relative);
                            } else {
                                config.pinned.push(relative.clone());
                            }
                            this.save_folder_view(config, window, cx);
                        }
                    })),
            );
        }
        let mut body = v_flex().gap_2().p_3().text_sm().child(header);
        if !list && !card.metadata.description.is_empty() {
            body = body.child(
                div()
                    .h(px(36.))
                    .line_height(px(18.))
                    .overflow_hidden()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(card.metadata.description.clone()),
            );
        }
        let details = if kind == Kind::Folder {
            format!(
                "{} {}",
                card.count,
                if card.count == 1 {
                    self.tr("page", "side")
                } else {
                    self.tr("pages", "sider")
                }
            )
        } else if !card.metadata.status.is_empty() {
            card.metadata.status.clone()
        } else {
            card.metadata
                .tags
                .iter()
                .take(3)
                .map(|t| format!("#{t}"))
                .collect::<Vec<_>>()
                .join("  ")
        };
        if !details.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(details),
            );
        }
        v_flex()
            .id(SharedString::from(format!(
                "folder-object-{}",
                target.display()
            )))
            .when(list, |d| d.w_full())
            .when(!list, |d| d.w(px(228.)).flex_grow(1.).max_w(px(320.)))
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .overflow_hidden()
            .bg(cx.theme().background)
            .cursor_pointer()
            .hover(|s| s.bg(cx.theme().accent))
            .when(
                page.config.layout == folder::Layout::Gallery && card.banner.is_some(),
                |d| {
                    let source: ImageSource = match &card.banner.as_ref().unwrap().source {
                        preview::BannerSource::File(p) => p.clone().into(),
                        preview::BannerSource::Remote(u) => u.clone().into(),
                    };
                    d.child(
                        img(source)
                            .w_full()
                            .h(px(110.))
                            .object_fit(ObjectFit::Cover),
                    )
                },
            )
            .when(
                page.config.layout == folder::Layout::Gallery && card.banner.is_none(),
                |d| {
                    d.child(
                        h_flex()
                            .w_full()
                            .h(px(110.))
                            .justify_center()
                            .bg(cx.theme().muted)
                            .child(folder_icon(&card.metadata, icon, cx)),
                    )
                },
            )
            .child(body)
            .on_mouse_down(MouseButton::Right, {
                let path = target.clone();
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.show_file_menu(
                        path.clone(),
                        kind == Kind::Folder,
                        event.position,
                        window,
                        cx,
                    );
                })
            })
            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                if kind == Kind::Database {
                    this.set_view_mode(ViewMode::Preview, window, cx);
                }
                if kind == Kind::Folder {
                    this.open_folder_page(target.clone(), window, cx);
                } else if ev.modifiers().platform {
                    this.open_document_new_tab(target.clone(), window, cx);
                } else if ev.click_count() == 2 {
                    this.open_document(target.clone(), window, cx);
                } else {
                    this.preview_document(target.clone(), window, cx);
                }
            }))
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
