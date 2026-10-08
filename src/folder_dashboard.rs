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
    metadata: folder::Metadata,
    config: folder::Dashboard,
    cards: Vec<FolderCard>,
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
                    let text = preview::preprocess(body, note, vault.root.as_deref(), &*resolver);
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
                        let fallback = if entry.kind == Kind::Folder {
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
            metadata,
            config,
            cards,
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
                for (kind, is_pinned, en, nb) in [
                    (Kind::Note, true, "Pinned", "Festet"),
                    (Kind::Folder, false, "Folders", "Mapper"),
                    (Kind::Note, false, "Notes", "Notater"),
                    (Kind::Database, false, "Databases", "Databaser"),
                    (Kind::Image, false, "Images", "Bilder"),
                ] {
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
                .label(if page.config.sort == folder::Sort::Modified {
                    self.tr("Recently edited", "Sist redigert")
                } else {
                    self.tr("Name", "Navn")
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Some(page) = &this.folder {
                        let mut config = page.config.clone();
                        config.sort = if config.sort == folder::Sort::Name {
                            folder::Sort::Modified
                        } else {
                            folder::Sort::Name
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
                    .h(px(42.))
                    .overflow_hidden()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(card.metadata.description.clone()),
            );
        }
        let details = if kind == Kind::Folder {
            format!("{} {}", card.count, self.tr("pages", "sider"))
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
            .child(body)
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
