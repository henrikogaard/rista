//! Workspace: the root view — title bar, sidebar, tabs, editor, preview,
//! status bar, palette, dialogs. Everything routes through here.

use crate::actions::*;
use crate::document::{Document, DocumentEvent, ImageResolver};
use crate::preview;
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
use gpui_kit::component::input::{Editor, Input, InputState};
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
    needs_fs_check: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

struct OpenDoc {
    entity: Entity<Document>,
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
        let settings = self.settings.clone();
        let doc = cx.new(|cx| Document::open(path, &settings, resolver, window, cx));
        let sub = cx.subscribe_in(&doc, window, |this, _doc, event, _window, cx| {
            if matches!(event, DocumentEvent::Saved | DocumentEvent::Changed) {
                this.status_note = None;
                cx.notify();
            }
        });
        self.docs.push(OpenDoc {
            entity: doc.clone(),
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
        let mut name = "Untitled.md".to_string();
        let mut n = 1;
        while dir.join(&name).exists() {
            n += 1;
            name = format!("Untitled {}.md", n);
        }
        let path = dir.join(&name);
        if std::fs::write(&path, b"").is_ok() {
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
            self.status_note = Some("Open a folder first".into());
            cx.notify();
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

    fn set_view_mode(&mut self, mode: ViewMode, cx: &mut Context<Self>) {
        self.settings.view_mode = mode;
        self.settings.save();
        cx.notify();
    }

    fn on_view_source(&mut self, _: &ViewSource, _w: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::Source, cx);
    }
    fn on_view_split(&mut self, _: &ViewSplit, _w: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::Split, cx);
    }
    fn on_view_preview(&mut self, _: &ViewPreview, _w: &mut Window, cx: &mut Context<Self>) {
        self.set_view_mode(ViewMode::Preview, cx);
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
        self.status_note = Some("Rísta — a quiet place for words.".into());
        cx.notify();
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
            PaletteCmd::SourceMode => self.set_view_mode(ViewMode::Source, cx),
            PaletteCmd::SplitMode => self.set_view_mode(ViewMode::Split, cx),
            PaletteCmd::PreviewMode => self.set_view_mode(ViewMode::Preview, cx),
            PaletteCmd::ToggleSidebar => self.on_toggle_sidebar(&ToggleSidebar, window, cx),
            PaletteCmd::ToggleZen => self.on_toggle_zen(&ToggleZen, window, cx),
            PaletteCmd::ProjectSearch => {
                self.on_open_project_search(&OpenProjectSearch, window, cx)
            }
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
                .title(format!("Delete “{}”?", title))
                .description("This removes the file from disk. This cannot be undone.")
                .show_cancel(true)
                .ok_text("Delete")
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

    fn delete_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
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

    fn open_wikilink(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.vault.read(cx).resolve_wikilink(target) {
            Some(path) => self.open_document(path, window, cx),
            None => {
                self.status_note = Some(format!("No note named “{}”", target).into());
                cx.notify();
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
        let mode = self.settings.view_mode;
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
                        TabBar::new("view-mode")
                            .segmented()
                            .selected_index(match mode {
                                ViewMode::Source => 0,
                                ViewMode::Split => 1,
                                ViewMode::Preview => 2,
                            })
                            .children([
                                Tab::new().icon(assets::IconName::SquarePen).label("Source"),
                                Tab::new()
                                    .icon(assets::IconName::SquareSplitHorizontal)
                                    .label("Split"),
                                Tab::new().icon(assets::IconName::Eye).label("Preview"),
                            ])
                            .on_click(cx.listener(|this, &ix, _window, cx| {
                                match ix {
                                    0 => this.set_view_mode(ViewMode::Source, cx),
                                    1 => this.set_view_mode(ViewMode::Split, cx),
                                    _ => this.set_view_mode(ViewMode::Preview, cx),
                                }
                                cx.notify();
                            })),
                    )
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
                                ListItem::new(ix)
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
                    (doc.title(), doc.dirty)
                };
                Tab::new()
                    .label(if dirty {
                        format!("{} •", title)
                    } else {
                        title
                    })
                    .icon(assets::IconName::FileText)
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

        TabBar::new("doc-tabs")
            .underline()
            .selected_index(self.active.unwrap_or(0))
            .children(tabs)
            .on_click(cx.listener(|this, &ix, _window, cx| {
                this.active = Some(ix);
                cx.notify();
            }))
    }

    fn render_preview(&self, doc: &Entity<Document>, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let state = doc.read(cx).preview.clone();
        TextView::new(&state)
            .markdown_extensions(preview::extensions())
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
            .h_full()
            .px_6()
            .py_4()
    }

    fn render_editor_area(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(doc) = self.active_doc().cloned() else {
            return self.render_empty_editor(cx).into_any_element();
        };

        match self.settings.view_mode {
            ViewMode::Source => div()
                .size_full()
                .child(Editor::new(&doc.read(cx).editor).h_full())
                .into_any_element(),
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
                                .child(Editor::new(&doc.read(cx).editor).h_full()),
                        )
                        .child(
                            resizable_panel()
                                .size_range(px(280.)..px(4000.))
                                .child(self.render_preview(&doc, cx)),
                        ),
                )
                .into_any_element(),
        }
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
                doc.entity
                    .update(cx, |doc, cx| doc.check_external(window, cx));
            }
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
