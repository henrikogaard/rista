//! Workspace: the root view — title bar, sidebar, tabs, editor, preview,
//! status bar, palette, dialogs. Everything routes through here.

use crate::actions::*;
use crate::bases;
use crate::document::{Document, DocumentEvent, ImageResolver};
use crate::preview;
use crate::properties;
use crate::search;
use crate::settings::{Appearance, Settings, ViewMode};
use crate::settings_panel::SettingsView;
use crate::theme;
use crate::vault::{Vault, VaultEvent};
use gpui_kit::assets;
use gpui_kit::base::Placement;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::input::{self, Editor, Input, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::resizable::{h_resizable, resizable_panel};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::text::TextView;
use gpui_kit::component::tree::tree;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Icon, IndexPath, Sizable, TitleBar, WindowExt,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::path::PathBuf;

pub struct Workspace {
    vault: Entity<Vault>,
    docs: Vec<OpenDoc>,
    active: Option<usize>,
    settings: Settings,
    zen: bool,
    palette_state: Entity<CommandState>,
    palette_sections: Vec<Vec<PaletteEntry>>,
    settings_view: Entity<SettingsView>,
    rename_input: Entity<InputState>,
    status_note: Option<SharedString>,
    /// Bumped on every status update so an old expiry timer can't
    /// clear a newer note.
    status_epoch: u64,
    needs_fs_check: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

struct OpenDoc {
    entity: Entity<Document>,
    /// Present when the file is a `.base` — a live view over the vault.
    base: Option<Entity<bases::BaseView>>,
    _sub: Subscription,
}

impl Focusable for Workspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[derive(Clone)]
enum PaletteEntry {
    File(PathBuf),
    Command(PaletteCmd),
}

#[derive(Clone)]
enum PaletteCmd {
    NewFile,
    NewFolder,
    OpenFolder,
    DailyNote,
    CloseFolder,
    Save,
    SaveAs,
    SourceMode,
    SplitMode,
    PreviewMode,
    ToggleSidebar,
    ToggleZen,
    ProjectSearch,
    MoveLineUp,
    MoveLineDown,
    PageHistory,
    RestoreDeleted,
    InsertTemplate,
    EditProperties,
    BrowseTags,
    Backlinks,
    Outline,
    Settings,
    ToggleTheme,
    Quit,
}

impl PaletteCmd {
    fn spec(&self) -> (assets::IconName, &'static str, &'static [&'static str]) {
        use PaletteCmd::*;
        match self {
            NewFile => (assets::IconName::FilePlus, "New file", &["create", "note"]),
            NewFolder => (
                assets::IconName::FolderPlus,
                "New folder",
                &["create", "directory"],
            ),
            OpenFolder => (
                assets::IconName::FolderOpen,
                "Open folder…",
                &["vault", "workspace"],
            ),
            DailyNote => (
                assets::IconName::BookOpen,
                "Open daily note",
                &["today", "journal"],
            ),
            CloseFolder => (
                assets::IconName::FolderClosed,
                "Close folder",
                &["close", "vault"],
            ),
            Save => (assets::IconName::Check, "Save", &["write", "disk"]),
            SaveAs => (assets::IconName::FileText, "Save as…", &["export", "copy"]),
            SourceMode => (
                assets::IconName::SquarePen,
                "View: source",
                &["editor", "mode"],
            ),
            SplitMode => (
                assets::IconName::SquareSplitHorizontal,
                "View: split",
                &["mode", "side by side"],
            ),
            PreviewMode => (assets::IconName::Eye, "View: preview", &["reading", "mode"]),
            ToggleSidebar => (
                assets::IconName::PanelLeft,
                "Toggle sidebar",
                &["files", "hide"],
            ),
            ToggleZen => (
                assets::IconName::Maximize,
                "Toggle zen mode",
                &["focus", "distraction"],
            ),
            ProjectSearch => (
                assets::IconName::Search,
                "Find in project…",
                &["grep", "search"],
            ),
            MoveLineUp => (
                assets::IconName::ArrowUp,
                "Move line up",
                &["block", "reorder", "option"],
            ),
            MoveLineDown => (
                assets::IconName::ArrowDown,
                "Move line down",
                &["block", "reorder", "option"],
            ),
            PageHistory => (
                assets::IconName::FileClock,
                "Page history",
                &["version", "snapshot", "restore", "undo"],
            ),
            RestoreDeleted => (
                assets::IconName::ArchiveRestore,
                "Restore deleted files…",
                &["trash", "recover", "undelete"],
            ),
            InsertTemplate => (
                assets::IconName::LayoutTemplate,
                "Insert template…",
                &["template", "boilerplate", "snippet"],
            ),
            EditProperties => (
                assets::IconName::TableProperties,
                "Edit properties…",
                &["properties", "frontmatter", "metadata", "fields"],
            ),
            BrowseTags => (
                assets::IconName::Tags,
                "Browse tags…",
                &["tags", "labels", "topics"],
            ),
            Backlinks => (
                assets::IconName::Link,
                "Show backlinks…",
                &["backlinks", "links", "mentions", "references"],
            ),
            Outline => (
                assets::IconName::List,
                "Document outline…",
                &["outline", "headings", "sections", "toc", "jump"],
            ),
            Settings => (
                assets::IconName::Settings,
                "Settings…",
                &["preferences", "theme"],
            ),
            ToggleTheme => (assets::IconName::Moon, "Toggle theme", &["dark", "light"]),
            Quit => (assets::IconName::Close, "Quit Rísta", &["exit"]),
        }
    }
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let vault = cx.new(Vault::new);
        let palette_state = cx.new(|cx| CommandState::new(window, cx));
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let settings = Settings::load();
        let weak = cx.weak_entity();
        let settings_view = cx.new(|cx| SettingsView::new(weak, &settings, window, cx));
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let vault_sub = cx.subscribe(&vault, |this, _v, event: &VaultEvent, cx| {
            this.on_vault_event(event, cx);
        });

        let mut this = Self {
            vault,
            docs: Vec::new(),
            active: None,
            zen: false,
            palette_state,
            palette_sections: Vec::new(),
            settings_view,
            rename_input,
            status_note: None,
            status_epoch: 0,
            needs_fs_check: false,
            focus_handle,
            settings,
            _subscriptions: vec![vault_sub],
        };

        // Reopen the vault the user last had open.
        if let Some(root) = this.settings.last_vault.clone() {
            if root.exists() {
                this.open_vault_at(root, cx);
            }
        }
        this
    }

    // ------------------------------------------------------------------
    // Vault plumbing
    // ------------------------------------------------------------------

    fn open_vault_at(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.close_all_docs(cx);
        self.vault
            .update(cx, |vault, cx| vault.open(root.clone(), cx));
        self.settings.last_vault = Some(root);
        self.settings.save();
        self.status_note = None;
        cx.notify();
    }

    fn close_vault(&mut self, cx: &mut Context<Self>) {
        self.save_all(cx);
        self.close_all_docs(cx);
        self.vault.update(cx, |vault, cx| vault.close(cx));
        self.settings.last_vault = None;
        self.settings.save();
        cx.notify();
    }

    /// Show a status-bar note that fades after ~4s. A bumped epoch keeps
    /// an older timer from clearing a newer message.
    fn note_status(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status_epoch += 1;
        let epoch = self.status_epoch;
        self.status_note = Some(msg.into());
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            smol::Timer::after(std::time::Duration::from_secs(4)).await;
            let _ = this.update(&mut *cx, |this, cx| {
                if this.status_epoch == epoch {
                    this.status_note = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Put keyboard focus back where it belongs after a dialog or sheet
    /// closes: the active editor when a note is open, else the workspace.
    fn refocus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc() {
            let doc = doc.clone();
            doc.update(cx, |doc, cx| {
                doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
            });
        } else {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    fn on_vault_event(&mut self, _event: &VaultEvent, _cx: &mut Context<Self>) {
        // Flag docs whose files changed underneath; they reload themselves on
        // the next frame where a window handle is available.
        self.needs_fs_check = true;
    }

    // ------------------------------------------------------------------
    // Documents
    // ------------------------------------------------------------------

    fn active_doc(&self) -> Option<&Entity<Document>> {
        self.active
            .and_then(|i| self.docs.get(i))
            .map(|d| &d.entity)
    }

    /// Preview checkbox click → flip the `- [ ]` marker on `line`
    /// (1-based) in the active document.
    pub fn toggle_task_pub(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc() {
            doc.update(cx, |doc, cx| doc.toggle_task(line, window, cx));
        }
    }

    fn open_document(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .docs
            .iter()
            .position(|d| d.entity.read(cx).path == path)
        {
            self.active = Some(ix);
            cx.notify();
            return;
        }
        if !path.is_file() {
            return;
        }
        let resolver: ImageResolver = self.vault.read(cx).image_resolver();
        let vault_root = self.vault.read(cx).root.clone();
        let settings = self.settings.clone();
        let is_base_file = is_base(&path);
        let vault = self.vault.clone();
        let doc = cx.new(|cx| {
            Document::open(
                path,
                vault_root,
                &settings,
                resolver,
                Some(vault),
                window,
                cx,
            )
        });
        let sub = cx.subscribe_in(&doc, window, |this, _doc, event, _window, cx| {
            if matches!(event, DocumentEvent::Saved | DocumentEvent::Changed) {
                this.status_note = None;
                cx.notify();
            }
        });
        let base = is_base_file.then(|| {
            let workspace = cx.entity().downgrade();
            let vault = self.vault.clone();
            cx.new(|cx| bases::BaseView::new(doc.clone(), vault, workspace, window, cx))
        });
        self.docs.push(OpenDoc {
            entity: doc.clone(),
            base,
            _sub: sub,
        });
        self.active = Some(self.docs.len() - 1);
        cx.notify();

        // Focus the editor once the frame settles.
        let doc = doc.clone();
        window.defer(cx, move |window, cx| {
            doc.update(cx, |doc, cx| {
                doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
            });
        });
    }

    fn close_all_docs(&mut self, _cx: &mut Context<Self>) {
        self.docs.clear();
        self.active = None;
    }

    fn close_tab_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        let dirty = self.docs[ix].entity.read(cx).dirty;
        let title = self.docs[ix].entity.read(cx).title();
        if dirty {
            let view = cx.entity();
            window.open_alert_dialog(cx, move |dialog, _window, _cx| {
                dialog
                    .title(format!("Close “{}” without saving?", title))
                    .description("The note has unsaved changes.")
                    .show_cancel(true)
                    .ok_text("Close without saving")
                    .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                    .on_ok({
                        let view = view.clone();
                        move |_, _window, cx| {
                            view.update(cx, |this, cx| this.close_tab_now(ix, cx));
                            true
                        }
                    })
            });
            return;
        }
        self.close_tab_now(ix, cx);
    }

    fn close_tab_now(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        self.docs.remove(ix);
        if let Some(active) = self.active {
            if active >= self.docs.len() {
                self.active = self.docs.len().checked_sub(1);
            } else if active > ix {
                self.active = Some(active - 1);
            }
        }
        if self.docs.is_empty() {
            self.active = None;
        }
        cx.notify();
    }

    fn save_all(&mut self, cx: &mut Context<Self>) {
        for doc in &self.docs {
            doc.entity.update(cx, |doc, cx| {
                if doc.dirty {
                    let _ = doc.save(cx);
                }
            });
        }
    }

    // ------------------------------------------------------------------
    // Action handlers
    // ------------------------------------------------------------------

    fn on_new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.on_open_folder(&OpenFolder, window, cx);
            return;
        };
        self.new_file_in(root, window, cx);
    }

    /// Create a uniquely-named empty note in `dir` and open it.
    fn new_file_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.new_note_in(dir, Vec::new(), window, cx);
    }

    /// Create a uniquely-named note in `dir`, prefilled with frontmatter
    /// `fm` pairs (`key: yaml-scalar` lines), and open it. Called by base
    /// views so a new note satisfies the view's `==` filters at once.
    pub fn new_note_in(
        &mut self,
        dir: PathBuf,
        fm: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut name = "Untitled.md".to_string();
        let mut n = 1;
        while dir.join(&name).exists() {
            n += 1;
            name = format!("Untitled {}.md", n);
        }
        let path = dir.join(&name);
        let mut body = String::new();
        if !fm.is_empty() {
            body.push_str("---\n");
            for (key, value) in &fm {
                body.push_str(&format!("{key}: {value}\n"));
            }
            body.push_str("---\n\n");
        }
        if std::fs::write(&path, body.as_bytes()).is_ok() {
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
            self.open_document(path, window, cx);
        }
    }

    fn on_new_folder(&mut self, _: &NewFolder, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let mut name = "New folder".to_string();
        let mut n = 1;
        while root.join(&name).exists() {
            n += 1;
            name = format!("New folder {}", n);
        }
        if std::fs::create_dir(root.join(&name)).is_ok() {
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
        }
    }

    fn on_open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open a folder as a vault".into()),
        });
        cx.spawn_in(window, async move |view, window| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = window.update(|_window, cx| {
                        view.update(cx, |this, cx| this.open_vault_at(path, cx))
                    });
                }
            }
        })
        .detach();
    }

    fn on_open_daily(&mut self, _: &OpenDailyNote, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.vault.read(cx).daily_note() else {
            self.note_status("Open a folder first", cx);
            return;
        };
        if !path.exists() {
            let title = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if std::fs::write(&path, format!("# {}\n\n", title)).is_err() {
                return;
            }
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
        }
        self.open_document(path, window, cx);
    }

    fn on_close_folder(&mut self, _: &CloseFolder, _window: &mut Window, cx: &mut Context<Self>) {
        self.close_vault(cx);
    }

    fn on_save(&mut self, _: &SaveFile, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| {
                let _ = doc.save(cx);
            });
        }
    }

    fn on_save_as(&mut self, _: &SaveFileAs, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        let dir = doc
            .read(cx)
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let name = doc.read(cx).file_name();
        let receiver = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |view, window| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let _ = window.update(|_window, cx| {
                    view.update(cx, |this, cx| {
                        let _ = doc.update(cx, |doc, cx| doc.save_as(path, cx));
                        this.vault.update(cx, |vault, cx| vault.refresh(cx));
                    })
                });
            }
        })
        .detach();
    }

    fn on_move_line_up(&mut self, _: &MoveLineUp, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.move_block(false, window, cx));
        }
    }

    fn on_move_line_down(&mut self, _: &MoveLineDown, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.move_block(true, window, cx));
        }
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.active {
            self.close_tab_at(ix, window, cx);
        }
    }

    fn on_next_tab(&mut self, _: &NextTab, _window: &mut Window, cx: &mut Context<Self>) {
        if self.docs.is_empty() {
            return;
        }
        self.active = Some(match self.active {
            Some(i) => (i + 1) % self.docs.len(),
            None => 0,
        });
        cx.notify();
    }

    fn on_prev_tab(&mut self, _: &PrevTab, _window: &mut Window, cx: &mut Context<Self>) {
        if self.docs.is_empty() {
            return;
        }
        self.active = Some(match self.active {
            Some(0) | None => self.docs.len() - 1,
            Some(i) => i - 1,
        });
        cx.notify();
    }

    fn on_toggle_sidebar(&mut self, _: &ToggleSidebar, _w: &mut Window, cx: &mut Context<Self>) {
        self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed;
        self.settings.save();
        cx.notify();
    }

    fn on_toggle_zen(&mut self, _: &ToggleZen, _w: &mut Window, cx: &mut Context<Self>) {
        self.zen = !self.zen;
        cx.notify();
    }

    fn set_view_mode(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.view_mode = mode;
        self.settings.save();
        if mode == ViewMode::Preview {
            // The editor unmounts and its focus handle goes stale — reclaim
            // focus for the workspace or ⌘ shortcuts stop dispatching.
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    fn on_view_source(&mut self, _: &ViewSource, w: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::Source, w, cx);
    }
    fn on_view_split(&mut self, _: &ViewSplit, w: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::Split, w, cx);
    }
    fn on_view_preview(&mut self, _: &ViewPreview, w: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::Preview, w, cx);
    }

    fn on_toggle_theme(&mut self, _: &ToggleTheme, _w: &mut Window, cx: &mut Context<Self>) {
        self.settings.appearance = match self.settings.appearance {
            Appearance::Dark => Appearance::Light,
            _ => Appearance::Dark,
        };
        theme::set_theme_mode(self.settings.theme_mode(cx), cx);
        self.settings.save();
        cx.notify();
    }

    fn on_quit(&mut self, _: &Quit, _w: &mut Window, cx: &mut Context<Self>) {
        self.save_all(cx);
        cx.quit();
    }

    fn on_about(&mut self, _: &About, _w: &mut Window, cx: &mut Context<Self>) {
        self.note_status("Rísta — a quiet place for words.", cx);
    }

    // ------------------------------------------------------------------
    // Command palette
    // ------------------------------------------------------------------

    fn on_open_palette(
        &mut self,
        _: &OpenCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_palette(window, cx);
    }

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let commands = [
            PaletteCmd::NewFile,
            PaletteCmd::DailyNote,
            PaletteCmd::OpenFolder,
            PaletteCmd::NewFolder,
            PaletteCmd::Save,
            PaletteCmd::SaveAs,
            PaletteCmd::SourceMode,
            PaletteCmd::SplitMode,
            PaletteCmd::PreviewMode,
            PaletteCmd::ToggleSidebar,
            PaletteCmd::ToggleZen,
            PaletteCmd::ProjectSearch,
            PaletteCmd::MoveLineUp,
            PaletteCmd::MoveLineDown,
            PaletteCmd::PageHistory,
            PaletteCmd::RestoreDeleted,
            PaletteCmd::InsertTemplate,
            PaletteCmd::EditProperties,
            PaletteCmd::BrowseTags,
            PaletteCmd::Backlinks,
            PaletteCmd::Outline,
            PaletteCmd::ToggleTheme,
            PaletteCmd::Settings,
            PaletteCmd::CloseFolder,
            PaletteCmd::Quit,
        ];
        let notes: Vec<PaletteEntry> = self
            .vault
            .read(cx)
            .notes
            .iter()
            .cloned()
            .map(PaletteEntry::File)
            .collect();

        self.palette_sections = vec![
            commands
                .iter()
                .cloned()
                .map(PaletteEntry::Command)
                .collect(),
            notes,
        ];

        let command_items: Vec<CommandItem> = commands
            .iter()
            .map(|cmd| {
                let (icon, label, keywords) = cmd.spec();
                CommandItem::new()
                    .icon(icon)
                    .label(label)
                    .keywords(keywords.iter().copied())
            })
            .collect();
        let note_items: Vec<CommandItem> = self.palette_sections[1]
            .iter()
            .map(|entry| match entry {
                PaletteEntry::File(path) => CommandItem::new()
                    .icon(assets::IconName::File)
                    .label(
                        path.file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    )
                    .keywords([path.to_string_lossy().to_string()]),
                PaletteEntry::Command(_) => CommandItem::new().label(""),
            })
            .collect();

        let state = self.palette_state.clone();
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .w(px(560.))
                .overlay_closable(true)
                .close_button(false)
                .child(
                    Command::new(&state)
                        .placeholder("Notes and commands…")
                        .searchable(true)
                        .filterable(true)
                        .group(
                            CommandGroup::new()
                                .label("Commands")
                                .items(command_items.clone()),
                        )
                        .group(CommandGroup::new().label("Notes").items(note_items.clone()))
                        .max_h(px(440.))
                        .on_confirm({
                            let view = view.clone();
                            move |index, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.on_palette_pick(index, window, cx);
                                });
                                window.close_dialog(cx);
                                view.update(cx, |this, cx| {
                                    this.refocus(window, cx);
                                });
                            }
                        })
                        .on_cancel({
                            let view = view.clone();
                            move |window, cx| {
                                view.update(cx, |this, cx| {
                                    this.palette_sections.clear();
                                    this.refocus(window, cx);
                                });
                                window.close_dialog(cx);
                            }
                        }),
                )
        });

        let state = self.palette_state.clone();
        window.defer(cx, move |window, cx| {
            state.update(cx, |state, cx| {
                state.set_query("", window, cx);
                state.focus(window, cx);
            });
        });
    }

    fn on_palette_pick(&mut self, index: IndexPath, window: &mut Window, cx: &mut Context<Self>) {
        let entry = self
            .palette_sections
            .get(index.section)
            .and_then(|s| s.get(index.row))
            .cloned();
        match entry {
            Some(PaletteEntry::File(path)) => self.open_document(path, window, cx),
            Some(PaletteEntry::Command(cmd)) => self.run_command(cmd, window, cx),
            None => {}
        }
    }

    fn run_command(&mut self, cmd: PaletteCmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            PaletteCmd::NewFile => self.on_new_file(&NewFile, window, cx),
            PaletteCmd::NewFolder => self.on_new_folder(&NewFolder, window, cx),
            PaletteCmd::OpenFolder => self.on_open_folder(&OpenFolder, window, cx),
            PaletteCmd::DailyNote => self.on_open_daily(&OpenDailyNote, window, cx),
            PaletteCmd::CloseFolder => self.close_vault(cx),
            PaletteCmd::Save => self.on_save(&SaveFile, window, cx),
            PaletteCmd::SaveAs => self.on_save_as(&SaveFileAs, window, cx),
            PaletteCmd::SourceMode => self.set_view_mode(ViewMode::Source, window, cx),
            PaletteCmd::SplitMode => self.set_view_mode(ViewMode::Split, window, cx),
            PaletteCmd::PreviewMode => self.set_view_mode(ViewMode::Preview, window, cx),
            PaletteCmd::ToggleSidebar => self.on_toggle_sidebar(&ToggleSidebar, window, cx),
            PaletteCmd::ToggleZen => self.on_toggle_zen(&ToggleZen, window, cx),
            PaletteCmd::ProjectSearch => {
                self.on_open_project_search(&OpenProjectSearch, window, cx)
            }
            PaletteCmd::MoveLineUp => self.on_move_line_up(&MoveLineUp, window, cx),
            PaletteCmd::MoveLineDown => self.on_move_line_down(&MoveLineDown, window, cx),
            // These commands open their own dialog — defer past the
            // palette's own close_dialog, which would close them too.
            PaletteCmd::PageHistory => self.defer_dialog(Self::show_history, window, cx),
            PaletteCmd::RestoreDeleted => self.defer_dialog(Self::show_trash, window, cx),
            PaletteCmd::InsertTemplate => self.defer_dialog(Self::show_templates, window, cx),
            PaletteCmd::EditProperties => self.defer_dialog(Self::show_properties, window, cx),
            PaletteCmd::BrowseTags => self.defer_dialog(Self::show_tags, window, cx),
            PaletteCmd::Backlinks => self.defer_dialog(Self::show_backlinks, window, cx),
            PaletteCmd::Outline => self.defer_dialog(Self::show_outline, window, cx),
            PaletteCmd::Settings => self.on_open_settings(&OpenSettings, window, cx),
            PaletteCmd::ToggleTheme => self.on_toggle_theme(&ToggleTheme, window, cx),
            PaletteCmd::Quit => self.on_quit(&Quit, window, cx),
        }
    }

    // ------------------------------------------------------------------
    // Search / settings
    // ------------------------------------------------------------------

    fn on_open_project_search(
        &mut self,
        _: &OpenProjectSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        search::open_project_search(cx.entity(), window, cx);
    }

    fn on_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        let view = self.settings_view.clone();
        window.open_sheet_at(Placement::Right, cx, move |sheet, _window, _cx| {
            sheet.title("Settings").w(px(360.)).child(view.clone())
        });
    }

    /// Called by SettingsView after a change — write through to disk and
    /// apply theme + fonts + per-editor options.
    pub fn apply_settings(
        &mut self,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings = settings.clone();
        theme::set_theme_mode(settings.theme_mode(cx), cx);
        crate::apply_ui_settings(&settings, cx);
        for doc in &self.docs {
            doc.entity
                .update(cx, |doc, cx| doc.apply_settings(&settings, window, cx));
        }
        settings.save();
        cx.notify();
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    // ------------------------------------------------------------------
    // File ops (tree context menu)
    // ------------------------------------------------------------------

    fn show_rename_dialog(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.rename_input.clone();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        input.update(cx, |input, cx| input.set_value(name.clone(), window, cx));
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title("Rename")
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .on_ok({
                    let view = view.clone();
                    let path = path.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.commit_rename(path.clone(), window, cx);
                            this.refocus(window, cx);
                        });
                        true
                    }
                })
        });

        let input = self.rename_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx));
        });
    }

    fn commit_rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.rename_input.read(cx).value().to_string();
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let Some(dir) = path.parent().map(|p| p.to_path_buf()) else {
            return;
        };
        let new_path = dir.join(name);
        if new_path == path || new_path.exists() {
            return;
        }
        if std::fs::rename(&path, &new_path).is_ok() {
            // Keep open tabs honest.
            for doc in &self.docs {
                doc.entity.update(cx, |doc, _cx| {
                    if doc.path == path {
                        doc.path = new_path.clone();
                    }
                });
            }
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
        }
        let _ = window;
    }

    fn show_delete_confirm(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(format!("Move “{}” to trash?", title))
                .description("Moves it to the vault trash (.rista/trash) — restorable from the command palette.")
                .show_cancel(true)
                .ok_text("Trash")
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok({
                    let view = view.clone();
                    let path = path.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.delete_path(path.clone(), cx);
                            this.refocus(window, cx);
                        });
                        true
                    }
                })
        });
    }

    /// Move a file or folder into `dest_dir` (file-tree drag/drop).
    /// No-ops on same-spot, self/descendant drops, and name clashes.
    /// Open docs under the moved path get repointed so their next save
    /// doesn't resurrect a file at the old location.
    fn move_tree_entry(&mut self, src: PathBuf, dest_dir: PathBuf, cx: &mut Context<Self>) {
        if src == dest_dir || src.parent() == Some(dest_dir.as_path()) || dest_dir.starts_with(&src)
        {
            return;
        }
        let Some(name) = src.file_name() else {
            return;
        };
        let dest = dest_dir.join(name);
        if dest.exists() {
            self.note_status(
                format!("“{}” already exists there", name.to_string_lossy()),
                cx,
            );
            return;
        }
        match std::fs::rename(&src, &dest) {
            Ok(()) => {
                self.note_status(format!("Moved {}", name.to_string_lossy()), cx);
                for doc in &self.docs {
                    doc.entity.update(cx, |doc, _cx| {
                        if let Ok(rel) = doc.path.strip_prefix(&src) {
                            doc.path = dest.join(rel);
                        }
                    });
                }
                self.vault.update(cx, |vault, cx| vault.refresh(cx));
            }
            Err(err) => {
                self.note_status(format!("Move failed: {err}"), cx);
            }
        }
        cx.notify();
    }

    /// Run `f` on the workspace one frame later — used when a palette
    /// command needs to open a dialog after the palette has closed.
    fn defer_dialog(
        &mut self,
        f: fn(&mut Self, &mut Window, &mut Context<Self>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| f(this, window, cx));
        });
    }

    /// List a document's history snapshots; clicking one loads it into
    /// the editor as a dirty buffer (autosave makes it current).
    fn show_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        let (path, root) = {
            let doc = doc.read(cx);
            (doc.path.clone(), doc.vault_root.clone())
        };
        let Some(root) = root else {
            return;
        };
        let snaps = crate::history::snapshots(&root, &path);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut list = v_flex().w_full().py_1();
            if snaps.is_empty() {
                list = list.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No snapshots yet — history is taken on each save."),
                );
            }
            for (ix, (ts, file)) in snaps.iter().enumerate() {
                let file = file.clone();
                let doc = doc.clone();
                list = list.child(
                    div()
                        .id(("history-row", ix))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.foreground)
                                .child(crate::history::format_epoch(*ts)),
                        )
                        .on_click(move |_, window, cx| {
                            if let Ok(text) = std::fs::read_to_string(&file) {
                                doc.update(cx, |doc, cx| doc.restore_text(text, window, cx));
                            }
                            window.close_dialog(cx);
                        }),
                );
            }
            dialog
                .title("Page history")
                .w(px(440.))
                .overlay_closable(true)
                .child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        list.max_h(px(360.)),
                    ),
                )
        });
    }

    /// Open the frontmatter properties editor for the active note.
    fn show_properties(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            self.note_status("Open a note first", cx);
            return;
        };
        properties::open_properties(doc, window, cx);
    }

    /// List every `#tag` and `tags:` entry in the vault; clicking one
    /// opens project search scoped to it.
    fn show_tags(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tags = self.vault.read(cx).tags.clone();
        let workspace = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut list = v_flex().w_full().py_1();
            if tags.is_empty() {
                list = list.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No tags — add #tags inline or a tags: list in frontmatter."),
                );
            }
            for (ix, (tag, count)) in tags.iter().enumerate() {
                let query = format!("#{tag}");
                let workspace = workspace.clone();
                list = list.child(
                    div()
                        .id(("tag-row", ix))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.foreground)
                                        .child(format!("#{tag}")),
                                )
                                .child(div().text_xs().text_color(theme.muted_foreground).child(
                                    format!("{} note{}", count, if *count == 1 { "" } else { "s" }),
                                )),
                        )
                        .on_click(move |_, window, cx| {
                            let workspace = workspace.clone();
                            let query = query.clone();
                            window.close_dialog(cx);
                            window.defer(cx, move |window, cx| {
                                search::open_project_search_for(
                                    workspace,
                                    Some(query.as_str()),
                                    window,
                                    cx,
                                );
                            });
                        }),
                );
            }
            dialog
                .title("Tags")
                .w(px(400.))
                .overlay_closable(true)
                .child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        list.max_h(px(360.)),
                    ),
                )
        });
    }

    /// Notes whose `[[wikilinks]]` resolve to the active document —
    /// Obsidian's backlink panel as a dialog.
    fn show_backlinks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc_path) = self.active_doc().map(|d| d.read(cx).path.clone()) else {
            return;
        };
        let (notes, vault, root) = {
            let vault = self.vault.read(cx);
            (
                vault.notes.clone(),
                self.vault.clone(),
                vault.root.clone().unwrap_or_default(),
            )
        };
        let mut links: Vec<PathBuf> = Vec::new();
        for note in &notes {
            if *note == doc_path || note.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(note) else {
                continue;
            };
            let mut cursor = 0;
            let mut linked = false;
            while let Some(at) = text[cursor..].find("[[") {
                let start = cursor + at + 2;
                let Some(end) = text[start..].find("]]") else {
                    break;
                };
                let target = &text[start..start + end];
                if vault.read(cx).resolve_wikilink(target).as_deref() == Some(doc_path.as_path()) {
                    linked = true;
                    break;
                }
                cursor = start + end + 2;
            }
            if linked {
                links.push(note.clone());
            }
        }
        let workspace = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut list = v_flex().w_full().py_1();
            if links.is_empty() {
                list = list.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No backlinks — nothing links to this note yet."),
                );
            }
            for (ix, path) in links.iter().enumerate() {
                let rel = path
                    .strip_prefix(&root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                let open = path.clone();
                let workspace = workspace.clone();
                list = list.child(
                    div()
                        .id(("backlink-row", ix))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .child(div().text_sm().text_color(theme.foreground).child(rel))
                        .on_click(move |_, window, cx| {
                            let workspace = workspace.clone();
                            let open = open.clone();
                            window.close_dialog(cx);
                            window.defer(cx, move |window, cx| {
                                workspace
                                    .update(cx, |ws, cx| ws.open_document_pub(open, window, cx));
                            });
                        }),
                );
            }
            dialog
                .title("Backlinks")
                .w(px(400.))
                .overlay_closable(true)
                .child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        list.max_h(px(360.)),
                    ),
                )
        });
    }

    /// Heading navigator for the active note — picks a heading, jumps
    /// the editor caret to its line (fences skipped so `#` inside code
    /// blocks doesn't list).
    fn show_outline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            self.note_status("Open a note first", cx);
            return;
        };
        let raw = doc.read(cx).editor.read(cx).value().to_string();
        let mut headings: Vec<(usize, usize, String)> = Vec::new();
        let mut in_fence = false;
        for (ix, line) in raw.split('\n').enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&level) && trimmed.chars().nth(level) == Some(' ') {
                headings.push((ix + 1, level, trimmed[level + 1..].trim().to_string()));
            }
        }
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut list = v_flex().w_full().py_1();
            if headings.is_empty() {
                list = list.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No headings"),
                );
            }
            for (ix, (line, level, text)) in headings.iter().enumerate() {
                let (line, level, text) = (*line, *level, text.clone());
                let doc = doc.clone();
                list = list.child(
                    div()
                        .id(("outline-row", ix))
                        .w_full()
                        .px_3()
                        .pl(px(12. + 12. * (level as f32 - 1.)))
                        .py_1p5()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .child(
                            div()
                                .text_sm()
                                .text_color(if level == 1 {
                                    theme.foreground
                                } else {
                                    theme.muted_foreground
                                })
                                .child(text),
                        )
                        .on_click(move |_, window, cx| {
                            doc.update(cx, |doc, cx| doc.jump_to_line(line, window, cx));
                            window.close_dialog(cx);
                        }),
                );
            }
            dialog
                .title("Outline")
                .w(px(360.))
                .overlay_closable(true)
                .child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        list.max_h(px(360.)),
                    ),
                )
        });
    }

    /// List `templates/**/*.md`; clicking one inserts it at the cursor
    /// with `{{date}}`/`{{time}}`/`{{title}}`/`{{cursor}}` expansion.
    fn show_templates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let Some(doc) = self.active_doc().cloned() else {
            self.note_status("Open a note first", cx);
            return;
        };
        let files = template_files(&root);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut list = v_flex().w_full().py_1();
            if files.is_empty() {
                list = list.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No templates — drop .md files into templates/."),
                );
            }
            for (ix, file) in files.iter().enumerate() {
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let file = file.clone();
                let doc = doc.clone();
                list = list.child(
                    div()
                        .id(("template-row", ix))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .child(div().text_sm().text_color(theme.foreground).child(name))
                        .on_click(move |_, window, cx| {
                            if let Ok(text) = std::fs::read_to_string(&file) {
                                doc.update(cx, |doc, cx| doc.insert_template(&text, window, cx));
                            }
                            window.close_dialog(cx);
                        }),
                );
            }
            dialog
                .title("Insert template")
                .w(px(440.))
                .overlay_closable(true)
                .child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        list.max_h(px(360.)),
                    ),
                )
        });
    }

    /// List vault trash entries; clicking one restores it to its
    /// original path.
    fn show_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let entries = crate::history::trash_entries(&root);
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut list = v_flex().w_full().py_1();
            if entries.is_empty() {
                list = list.child(
                    div()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("Trash is empty."),
                );
            }
            for (ix, entry) in entries.iter().enumerate() {
                let deleted_at: u64 = entry
                    .trashed
                    .split("--")
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_default();
                let root = root.clone();
                let trashed = entry.trashed.clone();
                let view = view.clone();
                list =
                    list.child(
                        div()
                            .id(("trash-row", ix))
                            .w_full()
                            .px_3()
                            .py_1p5()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.foreground)
                                    .child(entry.original.clone()),
                            )
                            .child(div().text_xs().text_color(theme.muted_foreground).child(
                                format!("deleted {}", crate::history::format_epoch(deleted_at)),
                            ))
                            .on_click(move |_, window, cx| {
                                let _ = crate::history::restore(&root, &trashed);
                                view.update(cx, |this, cx| {
                                    this.vault.update(cx, |vault, cx| vault.refresh(cx));
                                    this.refocus(window, cx);
                                });
                                window.close_dialog(cx);
                            }),
                    );
            }
            dialog
                .title("Restore deleted files")
                .w(px(440.))
                .overlay_closable(true)
                .child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        list.max_h(px(360.)),
                    ),
                )
        });
    }

    fn delete_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let root = self.vault.read(cx).root.clone();
        let moved = root
            .as_ref()
            .is_some_and(|root| crate::history::move_to_trash(root, &path).is_ok());
        if !moved {
            // Outside the vault or trash failed — fall back to real delete.
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
        // Close any open tab pointing at it.
        if let Some(ix) = self
            .docs
            .iter()
            .position(|d| d.entity.read(cx).path == path)
        {
            self.close_tab_now(ix, cx);
        }
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
    }

    /// Public hook used by preview plugins (transclusion fallback links).
    pub fn open_wikilink(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.vault.read(cx).resolve_wikilink(target) {
            Some(path) => self.open_document(path, window, cx),
            None => {
                self.note_status(format!("No note named “{}”", target), cx);
            }
        }
    }

    /// Public hook used by the project-search view.
    pub fn open_document_pub(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_document(path, window, cx);
    }

    pub fn iter_docs(&self) -> impl Iterator<Item = &Entity<Document>> {
        self.docs.iter().map(|d| &d.entity)
    }

    pub fn vault_entity(&self) -> &Entity<Vault> {
        &self.vault
    }

    // ------------------------------------------------------------------
    // Render
    // ------------------------------------------------------------------

    fn render_title_bar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vault_name = self
            .vault
            .read(cx)
            .root
            .as_ref()
            .and_then(|r| r.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "Rísta".to_string());

        let collapsed = self.settings.sidebar_collapsed;
        let dark = matches!(self.settings.appearance, Appearance::Dark);

        TitleBar::new()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("toggle-sidebar")
                            .ghost()
                            .xsmall()
                            .icon(if collapsed {
                                assets::IconName::PanelLeftOpen
                            } else {
                                assets::IconName::PanelLeftClose
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_sidebar(&ToggleSidebar, window, cx);
                            })),
                    )
                    .when(self.vault.read(cx).is_open(), |this| {
                        this.child(
                            div()
                                .px_2()
                                .text_sm()
                                .font_medium()
                                .text_color(cx.theme().muted_foreground)
                                .child(vault_name),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("palette")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Search)
                            .label("⌘K")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_palette(window, cx);
                            })),
                    )
                    .child(
                        Button::new("theme")
                            .ghost()
                            .xsmall()
                            .icon(if dark {
                                assets::IconName::Sun
                            } else {
                                assets::IconName::Moon
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_theme(&ToggleTheme, window, cx);
                            })),
                    )
                    .child(
                        Button::new("settings")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Settings)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_open_settings(&OpenSettings, window, cx);
                            })),
                    ),
            )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let tree_state = self.vault.read(cx).tree.clone();

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1p5()
                    .border_b_1()
                    .border_color(cx.theme().sidebar_border)
                    // Drop zone for "move to vault root" — the header is
                    // the only always-visible non-folder target.
                    .drag_over::<PathBuf>(|style, _, _, cx| {
                        style.bg(cx.theme().accent.opacity(0.2))
                    })
                    .on_drop::<PathBuf>({
                        let view = view.clone();
                        move |src, _window, cx| {
                            let src = src.clone();
                            view.update(cx, |this, cx| {
                                let Some(root) = this.vault.read(cx).root.clone() else {
                                    return;
                                };
                                this.move_tree_entry(src, root, cx);
                            });
                        }
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Files"),
                    )
                    .child(
                        h_flex()
                            .gap_0p5()
                            .child(
                                Button::new("new-file")
                                    .ghost()
                                    .xsmall()
                                    .icon(assets::IconName::FilePlus)
                                    .tooltip("New file")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.on_new_file(&NewFile, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("new-folder")
                                    .ghost()
                                    .xsmall()
                                    .icon(assets::IconName::FolderPlus)
                                    .tooltip("New folder")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.on_new_folder(&NewFolder, window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    tree(&tree_state, {
                        let render_view = view.clone();
                        move |ix, entry, selected, _window, cx| {
                            render_view.update(cx, |_, cx| {
                                let item = entry.item();
                                let path = PathBuf::from(item.id.as_str());
                                let is_file = !entry.is_folder();
                                let icon: assets::IconName = if is_file {
                                    assets::IconName::FileText
                                } else if entry.is_expanded() {
                                    assets::IconName::FolderOpen
                                } else {
                                    assets::IconName::FolderClosed
                                };
                                let row = ListItem::new(ix)
                                    .w_full()
                                    .rounded(cx.theme().radius)
                                    .py_0p5()
                                    .px_2()
                                    .pl(px(16.) * entry.depth() + px(8.))
                                    .selected(selected)
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(Icon::new(icon).size_4())
                                            .child(div().truncate().child(item.label.clone())),
                                    )
                                    .on_click(cx.listener({
                                        let path = path.clone();
                                        move |this, _, window, cx| {
                                            if is_file {
                                                this.open_document(path.clone(), window, cx);
                                            }
                                        }
                                    }))
                                    .on_drag(path.clone(), {
                                        let label = item.label.clone();
                                        move |_, _, _, cx| {
                                            cx.new(|_| TreeDragPreview {
                                                label: label.clone(),
                                            })
                                        }
                                    });
                                // Folders accept drops; files only drag.
                                let row = if entry.is_folder() {
                                    let dest_dir = path.clone();
                                    row.drag_over::<PathBuf>(|style, _, _, cx| {
                                        style.bg(cx.theme().accent.opacity(0.2))
                                    })
                                    .on_drop::<PathBuf>({
                                        let view = render_view.clone();
                                        move |src, _window, cx| {
                                            let src = src.clone();
                                            let dest_dir = dest_dir.clone();
                                            view.update(cx, |this, cx| {
                                                this.move_tree_entry(src, dest_dir, cx);
                                            });
                                        }
                                    })
                                } else {
                                    row
                                };
                                row
                            })
                        }
                    })
                    .context_menu({
                        let view = view.clone();
                        move |_ix, entry, menu, _window, _cx| {
                            let path = PathBuf::from(entry.item().id.as_str());
                            let view = view.clone();
                            let dir = if entry.is_folder() {
                                path.clone()
                            } else {
                                path.parent()
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or_else(|| path.clone())
                            };
                            menu.item(
                                PopupMenuItem::new("New file here")
                                    .icon(assets::IconName::FilePlus)
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.new_file_in(dir.clone(), window, cx);
                                            });
                                        }
                                    }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("Rename…")
                                    .icon(assets::IconName::SquarePen)
                                    .on_click({
                                        let path = path.clone();
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.show_rename_dialog(path.clone(), window, cx);
                                            });
                                        }
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("Delete…")
                                    .icon(assets::IconName::Delete)
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.show_delete_confirm(path.clone(), window, cx);
                                            });
                                        }
                                    }),
                            )
                        }
                    })
                    .p_1()
                    .text_sm(),
                ),
            )
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs: Vec<Tab> = self
            .docs
            .iter()
            .enumerate()
            .map(|(ix, doc)| {
                let (title, dirty) = {
                    let doc = doc.entity.read(cx);
                    // .base tabs keep their extension so `tasks` and
                    // `tasks.base` don't look like the same document.
                    let title = if doc.path.extension().and_then(|e| e.to_str()) == Some("base") {
                        doc.file_name()
                    } else {
                        doc.title()
                    };
                    (title, doc.dirty)
                };
                Tab::new()
                    .label(if dirty {
                        format!("{} •", title)
                    } else {
                        title
                    })
                    // Icon goes in `prefix`: the vendored Tab renders the
                    // `icon` slot INSTEAD of the label, not beside it.
                    .prefix(Icon::new(assets::IconName::FileText).size_3p5())
                    .suffix(
                        Button::new(("close-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Close)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab_at(ix, window, cx);
                            })),
                    )
            })
            .collect();

        h_flex()
            .w_full()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                TabBar::new("doc-tabs")
                    .underline()
                    .selected_index(self.active.unwrap_or(0))
                    .children(tabs)
                    .on_click(cx.listener(|this, &ix, _window, cx| {
                        this.active = Some(ix);
                        cx.notify();
                    }))
                    .flex_1(),
            )
            .child(div().pr_2().child(self.render_view_mode_tabs(cx)))
    }

    /// The Source/Split/Preview switcher, kept out of the window titlebar so
    /// the top bar stays quiet. Lives at the right end of the tab strip.
    fn render_view_mode_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.settings.view_mode;
        TabBar::new("view-mode")
            .segmented()
            .small()
            .selected_index(match mode {
                ViewMode::Source => 0,
                ViewMode::Split => 1,
                ViewMode::Preview => 2,
            })
            .children([
                Tab::new().label("Source"),
                Tab::new().label("Split"),
                Tab::new().label("Preview"),
            ])
            .on_click(cx.listener(|this, &ix, window, cx| {
                match ix {
                    0 => this.set_view_mode(ViewMode::Source, window, cx),
                    1 => this.set_view_mode(ViewMode::Split, window, cx),
                    _ => this.set_view_mode(ViewMode::Preview, window, cx),
                }
                cx.notify();
            }))
    }

    fn render_preview(&self, doc: &Entity<Document>, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let (state, banner, folds, embeds) = {
            let doc = doc.read(cx);
            (
                doc.preview.clone(),
                doc.banner.clone(),
                doc.callout_folds.clone(),
                doc.base_embeds.clone(),
            )
        };
        let base_ctx = preview::PreviewCtx {
            vault: self.vault.clone(),
            workspace: view.downgrade(),
            views: embeds,
            depth: 0,
        };
        v_flex()
            .size_full()
            .overflow_hidden()
            .when_some(banner, |this, banner| this.child(render_banner(&banner)))
            .child(
                TextView::new(&state)
                    .markdown_extensions(preview::extensions(&folds, Some(&base_ctx), true))
                    .selectable(true)
                    .scrollable(true)
                    .on_link_click({
                        let view = view.clone();
                        move |link, _event, window, cx| {
                            let url = link.to_string();
                            if let Some(target) = url.strip_prefix("wiki:") {
                                let target = target.to_string();
                                view.update(cx, |this, cx| {
                                    this.open_wikilink(&target, window, cx);
                                });
                            } else {
                                cx.open_url(&url);
                            }
                        }
                    })
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .px_6()
                    .py_4(),
            )
    }

    fn render_editor_area(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(doc) = self.active_doc().cloned() else {
            return self.render_empty_editor(cx).into_any_element();
        };

        // `.base` files render their live view in Preview/Split; Source
        // stays the raw YAML so the spec stays editable.
        let base = self
            .active
            .and_then(|i| self.docs.get(i))
            .and_then(|d| d.base.clone());
        match self.settings.view_mode {
            ViewMode::Source => self.editor_container(&doc, cx).into_any_element(),
            ViewMode::Preview if base.is_some() => {
                div().size_full().child(base.unwrap()).into_any_element()
            }
            ViewMode::Preview => div()
                .size_full()
                .child(self.render_preview(&doc, cx))
                .into_any_element(),
            ViewMode::Split => div()
                .size_full()
                .child(
                    h_resizable("split")
                        .child(
                            resizable_panel()
                                .size_range(px(280.)..px(4000.))
                                .child(self.editor_container(&doc, cx)),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(280.)..px(4000.))
                                .child(match base {
                                    Some(base) => div().size_full().child(base).into_any_element(),
                                    None => div()
                                        .size_full()
                                        .child(self.render_preview(&doc, cx))
                                        .into_any_element(),
                                }),
                        ),
                )
                .into_any_element(),
        }
    }

    /// The editor plus the paste/drop handlers that turn images into
    /// attachments. `can_drop` + `on_drop` receive OS file drops; the
    /// capture-phase action listener sees `Paste` before the editor's own
    /// handler (⌘V arrives as the action, not a raw key event — the menu's
    /// key equivalent intercepts it on macOS).
    fn editor_container(&self, doc: &Entity<Document>, cx: &mut Context<Self>) -> Div {
        div()
            .size_full()
            .capture_action::<input::Paste>(cx.listener(|this, _paste, window, cx| {
                this.on_editor_paste_action(window, cx);
            }))
            .can_drop(|value, _window, _cx| value.is::<ExternalPaths>())
            .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
                style.bg(cx.theme().accent.opacity(0.08))
            })
            .on_drop::<ExternalPaths>(cx.listener(|this, paths, window, cx| {
                this.on_editor_drop(paths, window, cx);
            }))
            .child(Editor::new(&doc.read(cx).editor).h_full())
    }

    /// Paste with images/files on the clipboard → import into the attachments
    /// folder and insert `![[name]]`. Plain-text paste falls through to the
    /// editor's own handler (propagation continues).
    fn on_editor_paste_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let mut handled = false;
        for entry in &item.entries {
            match entry {
                ClipboardEntry::Image(image) => {
                    handled |=
                        self.import_image_bytes(&image.bytes, image.format.extension(), window, cx);
                }
                ClipboardEntry::ExternalPaths(paths) => {
                    handled |= self.import_paths(paths.paths(), window, cx);
                }
                ClipboardEntry::String(_) => {}
            }
        }
        if handled {
            cx.stop_propagation();
        }
    }

    fn on_editor_drop(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.import_paths(paths.paths(), window, cx);
    }

    /// Write clipboard image bytes into the attachments dir and insert
    /// `![[name]]` — the Obsidian "Pasted image <stamp>" convention.
    fn import_image_bytes(
        &mut self,
        bytes: &[u8],
        ext: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(dir) = self.attachments_dir(cx) else {
            return false;
        };
        let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
        let mut name = format!("Pasted image {stamp}.{ext}");
        let mut counter = 0u32;
        while dir.join(&name).exists() {
            counter += 1;
            name = format!("Pasted image {stamp}-{counter}.{ext}");
        }
        if std::fs::create_dir_all(&dir).is_err() || std::fs::write(dir.join(&name), bytes).is_err()
        {
            return false;
        }
        self.refresh_image_index(cx);
        self.insert_embed(&name, window, cx);
        true
    }

    /// Copy dropped/copied image files into the attachments dir and insert
    /// `![[name]]` for each. Files already inside the vault just get linked.
    fn import_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(dir) = self.attachments_dir(cx) else {
            return false;
        };
        let mut imported = false;
        for src in paths {
            if !crate::vault::is_image_file(src) {
                continue;
            }
            let Some(name) = src.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                continue;
            };
            let dest = dir.join(&name);
            if dest != *src
                && !dest.exists()
                && (std::fs::create_dir_all(&dir).is_err() || std::fs::copy(src, &dest).is_err())
            {
                continue;
            }
            self.insert_embed(&name, window, cx);
            imported = true;
        }
        if imported {
            self.refresh_image_index(cx);
        }
        imported
    }

    /// Put freshly imported attachments into the vault's live image index
    /// right away — the watcher catches up ~250ms later, but the preview
    /// re-syncs on the inserted text before that.
    fn refresh_image_index(&mut self, cx: &mut Context<Self>) {
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
    }

    fn insert_embed(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        let text = format!("![[{name}]]");
        doc.update(cx, |doc, cx| {
            doc.editor.update(cx, |editor, cx| {
                editor.insert(text, window, cx);
            });
        });
    }

    /// The vault's attachment folder (`settings.attachments_dir`, relative to
    /// the vault root unless absolute).
    fn attachments_dir(&self, cx: &App) -> Option<PathBuf> {
        let root = self.vault.read(cx).root.clone()?;
        Some(root.join(&self.settings.attachments_dir))
    }

    fn render_empty_editor(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let vault_open = self.vault.read(cx).is_open();
        let message = if vault_open {
            "Open a note — ⌘K to search."
        } else {
            ""
        };
        v_flex().size_full().items_center().justify_center().child(
            v_flex()
                .items_center()
                .gap_3()
                .child(
                    Icon::new(assets::IconName::BookOpen)
                        .size_8()
                        .text_color(cx.theme().muted_foreground),
                )
                .when(!vault_open, |this| {
                    this.child(div().text_lg().font_semibold().child("Rísta"))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("A quiet place for words."),
                        )
                        .child(
                            Button::new("open-folder")
                                .primary()
                                .icon(assets::IconName::FolderOpen)
                                .label("Open folder…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.on_open_folder(&OpenFolder, window, cx);
                                })),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Every file stays a file — Markdown on disk, nothing else."),
                        )
                })
                .when(vault_open, |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(message),
                    )
                }),
        )
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let doc = self.active_doc();
        let (rel_path, words, dirty, conflict) = doc
            .map(|doc| {
                let doc = doc.read(cx);
                let rel = self
                    .vault
                    .read(cx)
                    .root
                    .as_ref()
                    .and_then(|r| doc.path.strip_prefix(r).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| doc.path.to_string_lossy().to_string());
                (rel, doc.stats.0, doc.dirty, doc.conflict)
            })
            .unwrap_or_default();

        let mode_label = match self.settings.view_mode {
            ViewMode::Source => "Source",
            ViewMode::Split => "Split",
            ViewMode::Preview => "Preview",
        };

        StatusBar::new()
            .left(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(if conflict {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(if conflict {
                                format!("{} — changed on disk", rel_path)
                            } else if dirty {
                                format!("{} •", rel_path)
                            } else {
                                rel_path
                            }),
                    )
                    .children(self.status_note.as_ref().map(|note| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().warning)
                            .child(note.clone())
                    })),
            )
            .right(
                h_flex()
                    .gap_3()
                    .items_center()
                    .when(doc.is_some(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{} words", words)),
                        )
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(mode_label),
                    ),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Docs pick up external edits here — a window handle is guaranteed.
        if self.needs_fs_check {
            self.needs_fs_check = false;
            for doc in &self.docs {
                doc.entity.update(cx, |doc, cx| {
                    doc.check_external(window, cx);
                    // Embeds/banners resolve against the vault index — a
                    // newly added image should light up open previews.
                    doc.resync_preview(cx);
                });
            }
        }

        // Focus fallback: shortcuts and menu items only dispatch through a
        // focused view. When nothing holds focus (welcome screen, preview
        // mode, just-closed dialog) give it back to the workspace root.
        if window.focused(cx).is_none() {
            let handle = self.focus_handle.clone();
            window.on_next_frame(move |window, cx| handle.focus(window, cx));
        }

        let vault_open = self.vault.read(cx).is_open();
        let sidebar_visible = vault_open && !self.settings.sidebar_collapsed && !self.zen;
        let background = cx.theme().background;

        v_flex()
            .size_full()
            .bg(background)
            .key_context("Rista")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_new_file))
            .on_action(cx.listener(Self::on_new_folder))
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_open_daily))
            .on_action(cx.listener(Self::on_close_folder))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(cx.listener(Self::on_move_line_up))
            .on_action(cx.listener(Self::on_move_line_down))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_zen))
            .on_action(cx.listener(Self::on_view_source))
            .on_action(cx.listener(Self::on_view_split))
            .on_action(cx.listener(Self::on_view_preview))
            .on_action(cx.listener(Self::on_open_palette))
            .on_action(cx.listener(Self::on_open_project_search))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_quit))
            .on_action(cx.listener(Self::on_about))
            .when(!self.zen, |this| {
                this.child(self.render_title_bar(window, cx))
            })
            .child(div().flex_1().min_h_0().child(if !vault_open {
                div()
                    .size_full()
                    .child(self.render_empty_editor(cx))
                    .into_any_element()
            } else {
                h_resizable("workspace")
                    .when(sidebar_visible, |this| {
                        this.child(
                            resizable_panel()
                                .size(px(240.))
                                .size_range(px(170.)..px(420.))
                                .child(self.render_sidebar(cx)),
                        )
                    })
                    .child(
                        resizable_panel().child(
                            v_flex()
                                .size_full()
                                .when(!self.zen && !self.docs.is_empty(), |this| {
                                    this.child(self.render_tab_bar(cx))
                                })
                                .child(div().flex_1().min_h_0().child({
                                    let content = self.render_editor_area(cx);
                                    if self.zen {
                                        h_flex()
                                            .size_full()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .h_full()
                                                    .w_full()
                                                    .max_w(px(920.))
                                                    .child(content),
                                            )
                                            .into_any_element()
                                    } else {
                                        content
                                    }
                                }))
                                .when(!self.zen, |this| this.child(self.render_status_bar(cx))),
                        ),
                    )
                    .into_any_element()
            }))
    }
}

/// The frontmatter banner rendered above the preview: a 160px cover strip
/// cropped to `banner_y`, with an optional `banner_icon` overlay.
fn render_banner(banner: &preview::BannerSpec) -> impl IntoElement {
    let source: ImageSource = match &banner.source {
        preview::BannerSource::File(path) => path.clone().into(),
        preview::BannerSource::Remote(url) => url.clone().into(),
    };
    let y = banner.y.unwrap_or(0.5).clamp(0.0, 1.0);
    div()
        .w_full()
        .h(px(160.))
        .flex_none()
        .overflow_hidden()
        .relative()
        .child(
            img(source)
                .w_full()
                .h(px(320.))
                .object_fit(ObjectFit::Cover)
                .mt(px(-160. * y)),
        )
        .when_some(banner.icon.clone(), |this, icon| {
            this.child(
                div()
                    .absolute()
                    .bottom(px(6.))
                    .left(px(24.))
                    .text_3xl()
                    .child(icon),
            )
        })
}

/// Whether the file is an Obsidian-style `.base` database spec.
fn is_base(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("base"))
        .unwrap_or(false)
}

/// `templates/**/*.md` under the vault root, sorted for a stable dialog list.
fn template_files(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("templates")];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Floating label shown while dragging a file-tree row.
struct TreeDragPreview {
    label: SharedString,
}

impl Render for TreeDragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .text_sm()
            .child(self.label.clone())
    }
}
