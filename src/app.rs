//! Workspace: the root view — title bar, sidebar, tabs, editor, preview,
//! status bar, palette, dialogs. Everything routes through here.

use crate::actions::*;
use crate::bases;
use crate::document::{Document, DocumentEvent, ImageResolver};
use crate::preview;
use crate::properties;
use crate::search;
use crate::settings::{Appearance, Settings, TreeSort, ViewMode};
use crate::settings_panel::SettingsView;
use crate::theme;
use crate::vault::{Vault, VaultEvent, IMAGE_EXTS};
use chrono::NaiveDate;
use gpui_kit::assets;
use gpui_kit::base::Placement;
use gpui_kit::base::Selectable;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState, DateTime};
use gpui_kit::component::input::{self, Editor, Input, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, ResizablePanelGroup};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Icon, IndexPath, Sizable, TitleBar, WindowExt,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[path = "explorer_panel.rs"]
mod explorer_panel;
#[path = "folder_dashboard.rs"]
mod folder_dashboard;
#[path = "tools_panel.rs"]
mod tools_panel;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileMetadataSnapshot {
    modified: Option<SystemTime>,
    len: u64,
    identity: Option<(u64, u64, i64, i64)>,
}

fn file_metadata_snapshot(path: &Path) -> Option<FileMetadataSnapshot> {
    let metadata = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        Some((
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec(),
        ))
    };
    #[cfg(not(unix))]
    let identity = None;
    Some(FileMetadataSnapshot {
        modified: metadata.modified().ok(),
        len: metadata.len(),
        identity,
    })
}

fn update_observed_snapshot(
    observed: &mut HashMap<PathBuf, Option<FileMetadataSnapshot>>,
    path: &Path,
    snapshot: Option<FileMetadataSnapshot>,
) -> bool {
    let changed = observed
        .get(path)
        .is_none_or(|previous| previous.as_ref() != snapshot.as_ref());
    observed.insert(path.to_path_buf(), snapshot);
    changed
}

fn canvas_panels(id: &'static str, axis: Axis) -> ResizablePanelGroup {
    let panels = match axis {
        Axis::Horizontal => h_resizable(id),
        Axis::Vertical => gpui_kit::component::resizable::v_resizable(id),
    };
    panels.with_handle_appearance(std::rc::Rc::new(move |handle, _, cx| {
        let engaged = handle.state() != gpui_kit::base::ResizeHandleState::Idle;
        Some(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .when(engaged, |this| {
                    this.child(
                        div()
                            .flex_none()
                            .w(px(if axis == Axis::Horizontal { 3. } else { 32. }))
                            .h(px(if axis == Axis::Horizontal { 32. } else { 3. }))
                            .rounded(cx.theme().radius)
                            .bg(cx.theme().muted_foreground),
                    )
                })
                .into_any_element(),
        )
    }))
}

pub struct Workspace {
    vault: Entity<Vault>,
    docs: Vec<OpenDoc>,
    active: Option<usize>,
    folder: Option<folder_dashboard::FolderPage>,
    folder_search: Entity<InputState>,
    explorer_search: Entity<InputState>,
    move_undo: Vec<(PathBuf, PathBuf)>,
    terminals: Vec<tools_panel::TerminalTab>,
    active_terminal: Option<usize>,
    next_terminal_id: usize,
    terminal_visible: bool,
    settings: Settings,
    zen: bool,
    palette_state: Entity<CommandState>,
    palette_sections: Vec<Vec<PaletteEntry>>,
    settings_view: Entity<SettingsView>,
    rename_input: Entity<InputState>,
    /// Input backing the base-cell edit dialog.
    cell_input: Entity<InputState>,
    /// Name input for the tag-rename dialog (Tags pane context menu).
    tag_input: Entity<InputState>,
    /// Property-name input for the strip's "Add property" dialog —
    /// the value field reuses `cell_input`.
    prop_name_input: Entity<InputState>,
    /// Keeps the date-picker dialog's Change subscription alive
    /// while it is open.
    date_sub: Option<Subscription>,
    status_note: Option<SharedString>,
    /// Bumped on every status update so an old expiry timer can't
    /// clear a newer note.
    status_epoch: u64,
    /// Most-recently-opened note paths, front = latest. The palette's
    /// "Recent" group reads this.
    recent: Vec<PathBuf>,
    /// Paths of tabs closed this session, latest last — ⌘⇧T pops it.
    closed_tabs: Vec<PathBuf>,
    /// Tab index the right-click menu should act on — set by each tab's
    /// Right mouse-down handler (the strip-level context menu reads it
    /// when it builds its items on the next frame).
    tab_menu_ix: Option<usize>,
    /// Focus mode: dim every editor block except the one under the
    /// caret. Toggled via the palette; applies to all open docs.
    focus_mode: bool,
    /// Back/forward navigation: paths in visit order, `nav_pos` = the
    /// current entry. Recorded on every `open_document` activation and
    /// truncated past `nav_pos` like a browser history.
    nav_stack: Vec<PathBuf>,
    nav_pos: usize,
    /// Set while back/forward itself activates a doc — that traversal
    /// must not append a new history entry.
    nav_suppress: bool,
    sidebar_details_scroll: ScrollHandle,
    /// Whether the sidebar's Starred group is expanded.
    starred_open: bool,
    /// Whether the sidebar's Tags group is expanded.
    tags_open: bool,
    /// Nested-tag paths expanded in the Tags pane (collapsed by
    /// default,).
    tags_expanded: std::collections::BTreeSet<String>,
    tasks_open: bool,
    outline_open: bool,
    backlinks_open: bool,
    outgoing_open: bool,
    cal_open: bool,
    /// Month the sidebar calendar is showing (year, month 1-12).
    cal_month: (i32, u32),
    /// Wikilink hover preview — target + anchor point, rendered as a
    /// floating card over the workspace (the page preview).
    peek: Option<(PeekKind, gpui::Point<gpui::Pixels>)>,
    file_menu: Option<(Entity<PopupMenu>, gpui::Point<gpui::Pixels>)>,
    file_menu_sub: Option<Subscription>,
    /// Vault link graph — open as a full editor-area view (⌘G).
    graph: Option<Entity<crate::graph::GraphView>>,
    needs_fs_check: bool,
    needs_standalone_fs_check: bool,
    standalone_fs_task: Option<Task<()>>,
    focus_fallback_pending: bool,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

struct OpenDoc {
    entity: Entity<Document>,
    preview: bool,
    /// Present when the file is a `.base` — a live view over the vault.
    base: Option<Entity<bases::BaseView>>,
    _sub: Subscription,
}

fn preview_tab_is_replaceable(preview: bool, dirty: bool, conflict: bool, pinned: bool) -> bool {
    preview && !dirty && !conflict && !pinned
}

/// Nested tag node for the sidebar Tags pane — `#a/b` hangs under `#a`.
#[derive(Default)]
struct TagNode {
    /// Notes carrying exactly this tag path.
    own: usize,
    kids: std::collections::BTreeMap<String, TagNode>,
}

impl TagNode {
    fn total(&self) -> usize {
        self.own + self.kids.values().map(TagNode::total).sum::<usize>()
    }
}

/// Insert `count` at the leaf of `segs`, creating intermediate nodes.
fn tag_insert(
    nodes: &mut std::collections::BTreeMap<String, TagNode>,
    segs: &[&str],
    count: usize,
) {
    let Some((seg, rest)) = segs.split_first() else {
        return;
    };
    let node = nodes.entry(seg.to_string()).or_default();
    if rest.is_empty() {
        node.own += count;
    } else {
        tag_insert(&mut node.kids, rest, count);
    }
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
    Tool(crate::extensions::Launcher),
}

#[derive(Clone)]
enum PaletteCmd {
    NewFile,
    NewFolder,
    OpenFile,
    OpenFolder,
    DailyNote,
    AppendDaily,
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
    ToggleCheckbox,
    ToggleBold,
    ToggleItalic,
    ToggleHighlight,
    ToggleStrike,
    ExtractSelection,
    NewBase,
    ToggleCode,
    ToggleQuote,
    CodeBlock,
    ListBullet,
    ListNumbered,
    ListTask,
    Heading1,
    Heading2,
    Heading3,
    Heading4,
    Heading5,
    Heading6,
    InsertLink,
    InsertHr,
    InsertCallout,
    InsertDate,
    InsertTime,
    InsertImage,
    InsertTable,
    FormatTable,
    InsertFootnote,
    DeleteLine,
    PageHistory,
    RestoreDeleted,
    InsertTemplate,
    DuplicateNote,
    EditProperties,
    BrowseTags,
    Backlinks,
    Outline,
    ToggleFocus,
    GoBack,
    GoForward,
    FollowLink,
    ToggleStar,
    ReopenTab,
    CopyLink,
    CopyLinkHeading,
    RevealFile,
    CloseOtherTabs,
    CloseTabsRight,
    CloseAllTabs,
    TogglePin,
    ToggleReadable,
    ExportHtml,
    Settings,
    ToggleTheme,
    Quit,
    Graph,
    LocalGraph,
}

impl PaletteCmd {
    fn spec(&self) -> (assets::IconName, &'static str, &'static [&'static str]) {
        use PaletteCmd::*;
        match self {
            NewFile => (assets::IconName::FilePlus, "New file", &["create", "note"]),
            NewBase => (
                assets::IconName::Database,
                "New base",
                &["create", "database", "table"],
            ),
            NewFolder => (
                assets::IconName::FolderPlus,
                "New folder",
                &["create", "directory"],
            ),
            OpenFile => (
                assets::IconName::FileText,
                "Open file…",
                &["markdown", "document"],
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
            AppendDaily => (
                assets::IconName::SquarePen,
                "Append to daily note…",
                &["today", "journal", "log", "quick"],
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
            ToggleCheckbox => (
                assets::IconName::ListTodo,
                "Toggle checkbox",
                &["task", "todo", "check", "done", "line"],
            ),
            ToggleBold => (
                assets::IconName::Bold,
                "Bold",
                &["strong", "format", "wrap"],
            ),
            ToggleItalic => (
                assets::IconName::Italic,
                "Italic",
                &["emphasis", "format", "wrap"],
            ),
            ToggleHighlight => (
                assets::IconName::Highlighter,
                "Highlight",
                &["mark", "format", "wrap"],
            ),
            ToggleStrike => (
                assets::IconName::Strikethrough,
                "Strikethrough",
                &["strike", "format", "wrap"],
            ),
            ExtractSelection => (
                assets::IconName::Scissors,
                "Extract selection to new note…",
                &["cut", "refactor", "composer"],
            ),
            ToggleCode => (
                assets::IconName::Code,
                "Code",
                &["inline", "backtick", "format", "wrap"],
            ),
            ToggleQuote => (
                assets::IconName::Quote,
                "Blockquote",
                &["quote", "cite", "format"],
            ),
            CodeBlock => (
                assets::IconName::SquareCode,
                "Code block",
                &["fence", "pre", "format", "wrap"],
            ),
            ListBullet => (
                assets::IconName::List,
                "Toggle bulleted list",
                &["unordered", "dash", "format"],
            ),
            ListNumbered => (
                assets::IconName::ListOrdered,
                "Toggle numbered list",
                &["ordered", "1 2 3", "format"],
            ),
            ListTask => (
                assets::IconName::ListTodo,
                "Toggle task list",
                &["checklist", "todo", "format"],
            ),
            Heading1 => (
                assets::IconName::Heading1,
                "Heading 1",
                &["title", "format", "h1"],
            ),
            Heading2 => (
                assets::IconName::Heading2,
                "Heading 2",
                &["title", "format", "h2"],
            ),
            Heading3 => (
                assets::IconName::Heading3,
                "Heading 3",
                &["title", "format", "h3"],
            ),
            Heading4 => (
                assets::IconName::Heading4,
                "Heading 4",
                &["title", "format", "h4"],
            ),
            Heading5 => (
                assets::IconName::Heading5,
                "Heading 5",
                &["title", "format", "h5"],
            ),
            Heading6 => (
                assets::IconName::Heading6,
                "Heading 6",
                &["title", "format", "h6"],
            ),
            InsertLink => (
                assets::IconName::Link,
                "Insert markdown link",
                &["url", "anchor", "format"],
            ),
            InsertHr => (
                assets::IconName::Minus,
                "Insert horizontal rule",
                &["divider", "separator", "hr"],
            ),
            InsertCallout => (
                assets::IconName::MessageSquareQuote,
                "Toggle callout",
                &["admonition", "note", "warning", "tip", "aside"],
            ),
            InsertDate => (
                assets::IconName::CalendarDays,
                "Insert current date",
                &["today", "now", "stamp"],
            ),
            InsertTime => (
                assets::IconName::Clock,
                "Insert current time",
                &["now", "hour", "stamp"],
            ),
            InsertImage => (
                assets::IconName::FileImage,
                "Insert image…",
                &["picture", "photo", "attach", "embed"],
            ),
            InsertTable => (
                assets::IconName::Table,
                "Insert table",
                &["grid", "cells", "columns"],
            ),
            FormatTable => (
                assets::IconName::Table,
                "Format table",
                &["align", "pipes", "columns", "pretty"],
            ),
            InsertFootnote => (
                assets::IconName::Superscript,
                "Insert footnote",
                &["reference", "annotation", "note"],
            ),
            DeleteLine => (
                assets::IconName::Delete,
                "Delete line",
                &["remove", "paragraph", "row"],
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
            DuplicateNote => (
                assets::IconName::CopyPlus,
                "Duplicate current note",
                &["copy", "clone", "file"],
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
            ToggleFocus => (
                assets::IconName::Frame,
                "Toggle focus mode",
                &["focus", "dim", "paragraph", "distraction", "writing"],
            ),
            GoBack => (
                assets::IconName::ChevronLeft,
                "Navigate back",
                &["back", "history", "previous", "navigate"],
            ),
            GoForward => (
                assets::IconName::ChevronRight,
                "Navigate forward",
                &["forward", "history", "next", "navigate"],
            ),
            FollowLink => (
                assets::IconName::ExternalLink,
                "Open link under cursor",
                &["link", "wikilink", "follow", "url", "open"],
            ),
            ReopenTab => (
                assets::IconName::Undo,
                "Reopen closed tab",
                &["restore", "undo", "tab", "recent"],
            ),
            ToggleStar => (
                assets::IconName::Star,
                "Star/unstar current note",
                &["star", "favorite", "bookmark", "pin"],
            ),
            CopyLink => (
                assets::IconName::Link,
                "Copy wikilink to note",
                &["copy", "link", "wikilink", "reference", "clipboard"],
            ),
            CopyLinkHeading => (
                assets::IconName::Link,
                "Copy wikilink to heading",
                &["copy", "link", "anchor", "section", "heading", "clipboard"],
            ),
            RevealFile => (
                assets::IconName::Crosshair,
                "Reveal active file in tree",
                &["locate", "show", "finder", "sidebar"],
            ),
            CloseOtherTabs => (
                assets::IconName::X,
                "Close other tabs",
                &["tab", "keep", "only"],
            ),
            CloseTabsRight => (
                assets::IconName::X,
                "Close tabs to the right",
                &["tab", "after"],
            ),
            CloseAllTabs => (
                assets::IconName::X,
                "Close all tabs",
                &["tab", "everything", "clear"],
            ),
            TogglePin => (
                assets::IconName::Pin,
                "Pin/unpin tab",
                &["keep", "protect", "sticky"],
            ),
            ToggleReadable => (
                assets::IconName::TextWrap,
                "Toggle readable line length",
                &["width", "column", "center", "narrow", "wrap"],
            ),
            ExportHtml => (
                assets::IconName::FileText,
                "Export note as HTML…",
                &["export", "html", "share", "publish", "save"],
            ),
            Settings => (
                assets::IconName::Settings,
                "Settings…",
                &["preferences", "theme"],
            ),
            ToggleTheme => (assets::IconName::Moon, "Toggle theme", &["dark", "light"]),
            Quit => (assets::IconName::Close, "Quit Rísta", &["exit"]),
            Graph => (
                assets::IconName::Waypoints,
                "Open graph view",
                &["graph", "network", "map", "links", "wiki"],
            ),
            LocalGraph => (
                assets::IconName::Waypoints,
                "Open local graph",
                &["graph", "local", "neighborhood", "links", "current"],
            ),
        }
    }
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, initial_paths: Vec<PathBuf>) -> Self {
        let workspace = cx.entity().downgrade();
        let appearance_sub = window.observe_window_appearance(move |_, cx| {
            let _ = workspace.update(cx, |this, cx| {
                if this.settings.appearance == Appearance::System {
                    theme::apply(&this.settings, cx);
                    crate::apply_ui_settings(&this.settings, cx);
                    cx.notify();
                }
            });
        });
        let vault = cx.new(Vault::new);
        let palette_state = cx.new(|cx| CommandState::new(window, cx));
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let cell_input = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let tag_input = cx.new(|cx| InputState::new(window, cx).placeholder("new-name"));
        let prop_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Property name"));
        let settings = Settings::load();
        let folder_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                settings
                    .language
                    .text("Search this folder…", "Søk i denne mappen…"),
            )
        });
        let folder_search_sub = cx.subscribe(&folder_search, |_, _, _: &input::InputEvent, cx| {
            cx.notify()
        });
        let explorer_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                settings
                    .language
                    .text("Find files or paths…", "Finn filer eller stier…"),
            )
        });
        let explorer_sub = cx.subscribe(
            &explorer_search,
            |this, input, _: &input::InputEvent, cx| {
                let query = input.read(cx).value().to_string();
                this.vault.update(cx, |vault, cx| {
                    vault.explorer_query = query;
                    vault.filter_tree(cx);
                });
                cx.notify();
            },
        );
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
            folder: None,
            folder_search,
            explorer_search,
            move_undo: Vec::new(),
            terminals: Vec::new(),
            active_terminal: None,
            next_terminal_id: 0,
            terminal_visible: false,
            zen: false,
            palette_state,
            palette_sections: Vec::new(),
            settings_view,
            rename_input,
            cell_input,
            tag_input,
            prop_name_input,
            date_sub: None,
            status_note: None,
            status_epoch: 0,
            recent: Vec::new(),
            closed_tabs: Vec::new(),
            tab_menu_ix: None,
            focus_mode: settings.focus_mode,
            nav_stack: Vec::new(),
            nav_pos: 0,
            nav_suppress: false,
            sidebar_details_scroll: ScrollHandle::new(),
            starred_open: settings.panes.starred,
            tags_open: settings.panes.tags,
            tags_expanded: Default::default(),
            tasks_open: settings.panes.tasks,
            outline_open: settings.panes.outline,
            backlinks_open: settings.panes.backlinks,
            outgoing_open: settings.panes.outgoing,
            cal_open: settings.panes.calendar,
            cal_month: {
                let now = chrono::Local::now();
                (
                    now.format("%Y").to_string().parse().unwrap_or(2026),
                    now.format("%m").to_string().parse().unwrap_or(1),
                )
            },
            peek: None,
            file_menu: None,
            file_menu_sub: None,
            graph: None,
            needs_fs_check: false,
            needs_standalone_fs_check: false,
            standalone_fs_task: None,
            focus_fallback_pending: false,
            focus_handle,
            settings,
            _subscriptions: vec![vault_sub, appearance_sub, folder_search_sub, explorer_sub],
        };

        // `.base` `file.starred` reads this set — `toggle_star` keeps
        // it in sync with `settings.starred`.
        this.vault.update(cx, |vault, _cx| {
            vault.starred = this.settings.starred.iter().cloned().collect();
        });

        let restored_tabs = this.settings.open_tabs.clone();
        let restored_active = this.settings.active_tab.clone();
        let restored_folder = this.settings.active_folder.clone();
        if initial_paths.is_empty() {
            if let Some(root) = this
                .settings
                .last_vault
                .clone()
                .filter(|root| root.exists())
            {
                this.open_vault_at(root, window, cx);
            }
            for path in restored_tabs {
                this.open_path(PathBuf::from(path), window, cx);
            }
            if let Some(active) = restored_active {
                if let Some(ix) = this
                    .docs
                    .iter()
                    .position(|d| d.entity.read(cx).path.display().to_string() == active)
                {
                    this.active = Some(ix);
                }
            }
            if let Some(folder) = restored_folder.filter(|p| p.is_dir()) {
                this.open_folder_page(folder, window, cx);
            }
        } else {
            for path in initial_paths {
                this.open_path(path, window, cx);
            }
        }

        let workspace = cx.entity();
        window.on_window_should_close(cx, move |_, cx| {
            workspace.update(cx, |this, cx| match this.save_all(cx) {
                Ok(()) => true,
                Err(err) => {
                    this.note_status(format!("Could not save before closing: {err}"), cx);
                    false
                }
            })
        });
        this
    }

    // ------------------------------------------------------------------
    // Vault plumbing
    // ------------------------------------------------------------------

    fn open_vault_at(
        &mut self,
        root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Err(err) = self.save_all(cx) {
            self.note_status(format!("Could not switch folders: {err}"), cx);
            return false;
        }
        let root = match root.canonicalize() {
            Ok(root) if root.is_dir() => root,
            Ok(_) => {
                self.note_status("That path is not a folder", cx);
                return false;
            }
            Err(err) => {
                self.note_status(format!("Could not open folder: {err}"), cx);
                return false;
            }
        };
        self.close_all_docs(window, cx);
        self.clear_file_menu();
        self.graph = None;
        self.nav_stack.clear();
        self.nav_pos = 0;
        self.nav_suppress = false;
        self.move_undo.clear();
        self.explorer_search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.vault.update(cx, |vault, cx| {
            vault.tree_sort = self.settings.tree_sort;
            vault.show_other_files = self.settings.show_other_files;
            vault.templates_dir = self.settings.templates_dir.clone();
            vault.open(root.clone(), cx);
        });
        self.terminals.clear();
        self.active_terminal = None;
        self.terminal_visible = false;
        self.settings.last_vault = Some(root.clone());
        self.settings.save();
        self.status_note = None;
        self.open_folder_page(root, window, cx);
        cx.notify();
        true
    }

    fn close_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Err(err) = self.save_all(cx) {
            self.note_status(format!("Could not close folder: {err}"), cx);
            return false;
        }
        self.close_all_docs(window, cx);
        self.clear_file_menu();
        self.graph = None;
        self.nav_stack.clear();
        self.nav_pos = 0;
        self.nav_suppress = false;
        self.vault.update(cx, |vault, cx| vault.close(cx));
        self.terminals.clear();
        self.active_terminal = None;
        self.terminal_visible = false;
        self.settings.last_vault = None;
        self.settings.save();
        cx.notify();
        true
    }

    fn clear_file_menu(&mut self) {
        self.file_menu = None;
        self.file_menu_sub = None;
    }

    fn show_file_menu(
        &mut self,
        path: PathBuf,
        is_folder: bool,
        position: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.peek = None;
        let action_context = window
            .focused(cx)
            .unwrap_or_else(|| self.focus_handle.clone());
        let starred_list = self.settings.starred.clone();
        let language = self.settings.language;
        let view = cx.entity();
        self.clear_file_menu();

        let menu = PopupMenu::build(window, cx, move |menu, _window, _cx| {
            Self::build_file_menu(menu, path, is_folder, starred_list, language, view)
                .action_context(action_context)
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

    fn build_file_menu(
        menu: PopupMenu,
        path: PathBuf,
        is_folder: bool,
        starred_list: Vec<String>,
        language: crate::settings::Language,
        view: Entity<Self>,
    ) -> PopupMenu {
        let dir = if is_folder {
            path.clone()
        } else {
            path.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| path.clone())
        };
        let menu = menu
            .item(
                PopupMenuItem::new(language.text("Tools here…", "Verktøy her…"))
                    .icon(assets::IconName::Blocks)
                    .on_click({
                        let view = view.clone();
                        let path = path.clone();
                        move |_, window, _| {
                            let view = view.clone();
                            let path = path.clone();
                            window.on_next_frame(move |window, cx| {
                                view.update(cx, |this, cx| {
                                    this.open_tools_for(Some(path), window, cx)
                                })
                            });
                        }
                    }),
            )
            .item(
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
            .separator();
        // Files can be pinned into the Starred group.
        let menu = if is_folder {
            menu.item(
                PopupMenuItem::new(language.text("Open terminal here", "Åpne terminal her"))
                    .icon(assets::IconName::Terminal)
                    .on_click({
                        let view = view.clone();
                        let path = path.clone();
                        move |_, window, _cx| {
                            let view = view.clone();
                            let path = path.clone();
                            window.on_next_frame(move |window, cx| {
                                view.update(cx, |this, cx| {
                                    this.open_terminal_here(path, window, cx);
                                });
                            });
                        }
                    }),
            )
        } else {
            let starred = starred_list.contains(&path.to_string_lossy().to_string());
            menu.item(
                PopupMenuItem::new(if starred { "Unstar" } else { "Star" })
                    .icon(if starred {
                        assets::IconName::StarOff
                    } else {
                        assets::IconName::Star
                    })
                    .on_click({
                        let path = path.clone();
                        let view = view.clone();
                        move |_, _window, cx| {
                            view.update(cx, |this, cx| {
                                this.toggle_star(path.clone(), cx);
                            });
                        }
                    }),
            )
        };
        menu.item(
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
        .when(!is_folder, |menu| {
            menu.item(
                PopupMenuItem::new("Duplicate")
                    .icon(assets::IconName::CopyPlus)
                    .on_click({
                        let path = path.clone();
                        let view = view.clone();
                        move |_, _window, cx| {
                            view.update(cx, |this, cx| {
                                this.duplicate_file(path.clone(), cx);
                            });
                        }
                    }),
            )
        })
        .when(
            !is_folder && path.extension().map(|e| e == "md").unwrap_or(false),
            |menu| {
                menu.item(
                    PopupMenuItem::new("Open local graph")
                        .icon(assets::IconName::Waypoints)
                        .on_click({
                            let path = path.clone();
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.open_local_graph_for(path.clone(), window, cx);
                                });
                            }
                        }),
                )
            },
        )
        .item(
            PopupMenuItem::new("Copy path")
                .icon(assets::IconName::Link)
                .on_click({
                    let path = path.clone();
                    let view = view.clone();
                    move |_, _window, cx| {
                        view.update(cx, |this, cx| {
                            this.copy_rel_path(path.clone(), cx);
                        });
                    }
                }),
        )
        .item(
            PopupMenuItem::new("Copy wikilink")
                .icon(assets::IconName::Copy)
                .on_click({
                    let path = path.clone();
                    move |_, _window, cx| {
                        let stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        cx.write_to_clipboard(ClipboardItem::new_string(format!("[[{stem}]]")));
                    }
                }),
        )
        .item(
            PopupMenuItem::new("Reveal in Finder")
                .icon(assets::IconName::FolderOpen)
                .on_click({
                    let path = path.clone();
                    move |_, _window, _cx| reveal_in_file_manager(&path)
                }),
        )
        .item(
            PopupMenuItem::new("Open in default app")
                .icon(assets::IconName::ExternalLink)
                .on_click({
                    let path = path.clone();
                    move |_, _window, _cx| open_in_default_app(&path)
                }),
        )
        .item(
            PopupMenuItem::new("Delete…")
                .icon(assets::IconName::Delete)
                .on_click({
                    let path = path.clone();
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.show_delete_confirm(path.clone(), window, cx);
                        });
                    }
                }),
        )
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
        // In Preview (and on image docs) no editor is mounted — its
        // focus handle is dead weight and ⌘ bindings go nowhere, so
        // the workspace itself takes focus.
        let editor_alive =
            self.settings.view_mode != ViewMode::Preview && !self.active_doc_is_image(cx);
        if editor_alive {
            if let Some(doc) = self.active_doc() {
                let doc = doc.clone();
                doc.update(cx, |doc, cx| {
                    doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
                });
            } else {
                self.focus_handle.focus(window, cx);
            }
        } else {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    fn on_vault_event(&mut self, event: &VaultEvent, cx: &mut Context<Self>) {
        if matches!(event, VaultEvent::TreeExpansionChanged) {
            let vault = self.vault.read(cx);
            if let Some(root) = &vault.root {
                self.settings
                    .expanded_folders
                    .insert(root.to_string_lossy().to_string(), vault.expanded_folders());
                self.settings.save();
            }
            return;
        }
        // Flag docs whose files changed underneath; they reload themselves on
        // the next frame where a window handle is available.
        self.needs_fs_check = true;
        cx.notify();
        // An open graph keeps its map in sync — positions carry over
        // so it settles instead of jumping.
        if let Some(graph) = self.graph.clone() {
            graph.update(cx, |g, cx| g.rebuild(cx));
        }
    }

    // ------------------------------------------------------------------
    // Documents
    // ------------------------------------------------------------------

    /// ⌘F when a `.base` view is rendered (Preview/Split) filters its
    /// rows — otherwise the action is re-dispatched to the note editor
    /// so its find bar opens even when the workspace held focus. In
    /// Source the YAML editor's own binding handles it first.
    fn on_find(&mut self, _: &input::Search, window: &mut Window, cx: &mut Context<Self>) {
        // Graph open → ⌘F is the node filter, not the note find bar.
        if let Some(graph) = self.graph.clone() {
            graph.update(cx, |g, cx| g.focus_filter(window, cx));
            return;
        }
        if self.settings.view_mode != ViewMode::Source {
            if let Some(base) = self
                .active
                .and_then(|i| self.docs.get(i))
                .and_then(|d| d.base.clone())
            {
                base.update(cx, |base, cx| base.focus_search(window, cx));
                return;
            }
        }
        // In Preview the editor is unmounted — forwarding would bounce
        // the action straight back here, so stop instead.
        if self.settings.view_mode == ViewMode::Preview {
            return;
        }
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| {
                doc.editor.update(cx, |editor, cx| editor.focus(window, cx))
            });
            window.dispatch_action(input::Search.boxed_clone(), cx);
        }
    }

    fn active_doc(&self) -> Option<&Entity<Document>> {
        self.active
            .and_then(|i| self.docs.get(i))
            .map(|d| &d.entity)
    }

    /// `(`/`[`/`{`/quotes/etc. — insert the pair, wrap the selection, or
    /// skip the matching closer (Document::insert_pair).
    fn on_pair_insert(&mut self, action: &PairInsert, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.insert_pair(action.pair, window, cx));
        }
    }

    /// `)`/`]`/`}` — skip over the closer, wrap the selection, or insert the
    /// closer plainly (Document::close_pair).
    fn on_pair_close(&mut self, action: &PairClose, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.close_pair(action.pair, window, cx));
        }
    }

    /// Preview checkbox click → flip the `- [ ]` marker on `line`
    /// (1-based) in the active document.
    pub fn toggle_task_pub(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc() {
            doc.update(cx, |doc, cx| doc.toggle_task(line, window, cx));
        }
    }

    /// Sidebar Tasks-pane checkbox → mark the task done (`- [x]`).
    /// Goes through the editor when the note is open (undoable,
    /// autosaves); flips the marker on disk for closed notes.
    fn complete_task(
        &mut self,
        path: PathBuf,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self
            .docs
            .iter()
            .find(|d| d.entity.read(cx).path == path)
            .map(|d| d.entity.clone())
        {
            doc.update(cx, |doc, cx| doc.toggle_task(line, window, cx));
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let out: String = text
            .split_inclusive('\n')
            .enumerate()
            .map(|(i, l)| {
                if i + 1 == line {
                    l.replacen("[ ]", "[x]", 1)
                } else {
                    l.to_string()
                }
            })
            .collect();
        if std::fs::write(&path, out).is_ok() {
            self.note_status("Task completed", cx);
        }
    }

    /// Preview wikilink hover → anchor the peek card at `pos`. Repeat
    /// moves over the same link update silently — the card stays
    /// anchored where the hover started, like the page preview.
    pub fn peek_at(
        &mut self,
        kind: PeekKind,
        pos: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        match &mut self.peek {
            Some((k, at)) if *k == kind => *at = pos,
            _ => {
                self.peek = Some((kind, pos));
                cx.notify();
            }
        }
    }

    pub fn hide_peek(&mut self, kind: &PeekKind, cx: &mut Context<Self>) {
        if self.peek.as_ref().is_some_and(|(k, _)| k == kind) {
            self.peek = None;
            cx.notify();
        }
    }

    /// Footnote peek dismissal compares on the label only — callers
    /// never recompute the body.
    pub fn hide_footnote_peek(&mut self, label: &str, cx: &mut Context<Self>) {
        let hit = matches!(
            &self.peek,
            Some((PeekKind::Footnote(l, _), _)) if l == label
        );
        if hit {
            self.peek = None;
            cx.notify();
        }
    }

    /// Footnote-ref hover → peek card showing the `[^label]:` body
    /// read from the active document.
    pub fn peek_footnote(
        &mut self,
        label: String,
        pos: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let body = self
            .active_doc()
            .map(|doc| doc.read(cx).editor.read(cx).value().to_string())
            .map(|text| {
                let marker = format!("[^{label}]:");
                text.lines()
                    .find_map(|line| {
                        line.find(&marker)
                            .map(|i| line[i + marker.len()..].trim().to_string())
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        self.peek_at(PeekKind::Footnote(label, body), pos, cx);
    }

    fn open_document(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_document_impl(path, false, false, window, cx);
    }

    fn preview_document(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_document_impl(path, false, true, window, cx);
    }

    pub(crate) fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(err) => {
                self.note_status(format!("Could not open {}: {err}", path.display()), cx);
                return;
            }
        };
        if path.is_dir() {
            self.open_vault_at(path, window, cx);
        } else if path.is_file() {
            self.open_document(path, window, cx);
        } else {
            self.note_status(format!("Could not open {}", path.display()), cx);
        }
    }

    fn start_standalone_fs_check(&mut self, cx: &mut Context<Self>) {
        if self.standalone_fs_task.is_some() {
            return;
        }
        self.standalone_fs_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let mut observed = HashMap::new();
            loop {
                smol::Timer::after(std::time::Duration::from_millis(500)).await;
                let keep_running = this
                    .update(&mut *cx, |this, cx| {
                        let standalone_paths: HashSet<PathBuf> = this
                            .docs
                            .iter()
                            .filter_map(|doc| {
                                let doc = doc.entity.read(cx);
                                doc.vault_root.is_none().then(|| doc.path.clone())
                            })
                            .collect();
                        observed.retain(|path, _| standalone_paths.contains(path));
                        let mut changed = false;
                        for path in &standalone_paths {
                            let snapshot = file_metadata_snapshot(path);
                            changed |= update_observed_snapshot(&mut observed, path, snapshot);
                        }
                        if !standalone_paths.is_empty() {
                            if changed {
                                this.needs_standalone_fs_check = true;
                                cx.notify();
                            }
                            true
                        } else {
                            this.standalone_fs_task = None;
                            false
                        }
                    })
                    .unwrap_or(false);
                if !keep_running {
                    break;
                }
            }
        }));
    }

    /// ⌘+click semantics: always open in a new tab, even
    /// when the note already has one.
    fn open_document_impl(
        &mut self,
        path: PathBuf,
        new_tab: bool,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.peek = None;
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(err) => {
                self.note_status(format!("Could not open {}: {err}", path.display()), cx);
                return;
            }
        };
        if path.is_dir() {
            self.open_folder_page(path, window, cx);
            return;
        }
        if !new_tab {
            if let Some(ix) = self
                .docs
                .iter()
                .position(|d| d.entity.read(cx).path == path)
            {
                if !preview {
                    self.docs[ix].preview = false;
                }
                self.active = Some(ix);
                self.folder = None;
                self.graph = None;
                self.record_nav(&path);
                self.recent.retain(|p| *p != path);
                self.recent.insert(0, path);
                self.recent.truncate(12);
                self.persist_tabs(cx);
                self.reveal_active_file(cx);
                cx.notify();
                return;
            }
        }
        if !path.is_file() {
            return;
        }
        let initial = match Document::load_initial(&path) {
            Ok(initial) => initial,
            Err(err) => {
                self.note_status(format!("Could not open {}: {err}", path.display()), cx);
                return;
            }
        };
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path.clone());
        self.recent.truncate(12);
        self.record_nav(&path);
        let vault_root = self
            .vault
            .read(cx)
            .root
            .clone()
            .filter(|root| path.starts_with(root));
        let resolver: ImageResolver = if vault_root.is_some() {
            self.vault.read(cx).image_resolver()
        } else {
            let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
            std::rc::Rc::new(move |reference| {
                let candidate = parent.join(reference);
                candidate.exists().then_some(candidate)
            })
        };
        let settings = self.settings.clone();
        let is_base_file = vault_root.is_some() && is_base(&path);
        let vault = vault_root.as_ref().map(|_| self.vault.clone());
        let is_standalone = vault_root.is_none();
        let doc = cx
            .new(|cx| Document::open(vault_root, &settings, resolver, vault, initial, window, cx));
        let sub = cx.subscribe_in(
            &doc,
            window,
            |this, changed_doc, event, _window, cx| match event {
                DocumentEvent::Saved | DocumentEvent::Changed => {
                    if matches!(event, DocumentEvent::Changed) && changed_doc.read(cx).dirty {
                        if let Some(tab) = this.docs.iter_mut().find(|d| d.entity == *changed_doc) {
                            tab.preview = false;
                        }
                    }
                    this.status_note = None;
                    cx.notify();
                }
                DocumentEvent::Selection => cx.notify(),
            },
        );
        let base = is_base_file.then(|| {
            let workspace = cx.entity().downgrade();
            let vault = self.vault.clone();
            // The view this file was left on — restored
            // does. Read here: BaseView::new runs inside our update
            // borrow, so it can't read Workspace itself.
            let key = doc.read(cx).path.to_string_lossy().to_string();
            let saved_ix = self.settings.base_views.get(&key).copied().unwrap_or(0);
            // Header sorts saved per view name — BaseView::new can't
            // read Workspace from inside this borrow, so the settings
            // are snapshotted here.
            let saved_sorts: Vec<(String, String, bool)> = self
                .settings
                .base_sorts
                .iter()
                .filter_map(|(k, (col, desc))| {
                    k.rsplit_once("::")
                        .and_then(|(p, v)| (p == key).then(|| (v.to_string(), col.clone(), *desc)))
                })
                .collect();
            cx.new(|cx| {
                bases::BaseView::new(
                    doc.clone(),
                    vault,
                    workspace,
                    saved_ix,
                    saved_sorts,
                    window,
                    cx,
                )
            })
        });
        doc.update(cx, |doc, cx| doc.set_focus_mode(self.focus_mode, cx));
        let replacement = preview
            .then(|| {
                self.docs.iter().position(|tab| {
                    let doc = tab.entity.read(cx);
                    preview_tab_is_replaceable(
                        tab.preview,
                        doc.dirty,
                        doc.conflict,
                        self.is_pinned(&doc.path),
                    )
                })
            })
            .flatten();
        let opened = OpenDoc {
            entity: doc.clone(),
            preview,
            base,
            _sub: sub,
        };
        let ix = if let Some(ix) = replacement {
            self.docs[ix] = opened;
            ix
        } else {
            self.docs.push(opened);
            self.docs.len() - 1
        };
        self.active = Some(ix);
        self.folder = None;
        self.graph = None;
        self.persist_tabs(cx);
        self.reveal_active_file(cx);
        if is_standalone {
            self.start_standalone_fs_check(cx);
        }
        cx.notify();

        // Focus the editor once the frame settles. Preview mode and
        // image docs mount no editor — the workspace takes focus so
        // ⌘ bindings keep working (same dead-handle fix as refocus).
        let focus_editor =
            !doc.read(cx).is_read_only() && self.settings.view_mode != ViewMode::Preview;
        let view = cx.entity();
        let doc = doc.clone();
        window.defer(cx, move |window, cx| {
            if focus_editor {
                doc.update(cx, |doc, cx| {
                    doc.editor.update(cx, |editor, cx| editor.focus(window, cx));
                });
            } else {
                view.update(cx, |ws, cx2| ws.focus_handle.focus(window, cx2));
            }
        });
    }

    /// Browser-style history: every activation truncates anything past
    /// `nav_pos` then pushes — unless back/forward itself is the cause.
    fn record_nav(&mut self, path: &Path) {
        if self.nav_suppress {
            return;
        }
        self.nav_stack.truncate(self.nav_pos + 1);
        if self.nav_stack.last().map(|p| p == path).unwrap_or(false) {
            self.nav_pos = self.nav_stack.len() - 1;
            return;
        }
        self.nav_stack.push(path.to_path_buf());
        self.nav_pos = self.nav_stack.len() - 1;
    }

    fn nav_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav_pos == 0 {
            return;
        }
        self.nav_pos -= 1;
        let path = self.nav_stack[self.nav_pos].clone();
        self.nav_suppress = true;
        self.open_document(path, window, cx);
        self.nav_suppress = false;
    }

    fn nav_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.nav_pos + 1 >= self.nav_stack.len() {
            return;
        }
        self.nav_pos += 1;
        let path = self.nav_stack[self.nav_pos].clone();
        self.nav_suppress = true;
        self.open_document(path, window, cx);
        self.nav_suppress = false;
    }

    fn on_navigate_back(&mut self, _: &NavigateBack, window: &mut Window, cx: &mut Context<Self>) {
        self.nav_back(window, cx);
    }

    /// ⌥⏎ — open the `[[wikilink]]`, `[..](..)`, or bare URL the caret
    /// sits on. Note links route through the vault resolver (records
    /// nav history); URLs go to the system browser.
    fn on_follow_link(&mut self, _: &FollowLink, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        match doc.update(cx, |doc, cx| doc.link_at_cursor(cx)) {
            Some(crate::document::LinkTarget::Url(url)) => cx.open_url(&url),
            Some(crate::document::LinkTarget::Note(name)) => {
                // Resolution, `#anchor` jump, and create-on-click all
                // live in open_wikilink now.
                self.open_wikilink(&name, window, cx);
            }
            None => self.note_status("No link under cursor", cx),
        }
    }

    fn on_navigate_forward(
        &mut self,
        _: &NavigateForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.nav_forward(window, cx);
    }

    /// ⌘G / palette / View menu — the vault link graph takes over the
    /// editor area (the Graph view); ⌘G again closes it.
    fn on_open_graph(&mut self, _: &OpenGraph, window: &mut Window, cx: &mut Context<Self>) {
        if self.graph.is_some() {
            self.close_graph(window, cx);
            return;
        }
        if self.vault.read(cx).root.is_none() {
            self.note_status("Open a vault first", cx);
            return;
        }
        let weak = cx.weak_entity();
        let vault = self.vault.clone();
        let active = self
            .active
            .and_then(|i| self.docs.get(i))
            .map(|d| d.entity.read(cx).path.clone());
        let graph = cx.new(|cx| crate::graph::GraphView::new(weak, vault, window, cx));
        graph.update(cx, |g, _cx| g.active = active);
        graph.read(cx).focus_handle(cx).focus(window, cx);
        self.graph = Some(graph);
        cx.notify();
    }

    /// Palette "Open local graph" — neighbourhood view pinned on the
    /// current note (the local graph).
    fn on_open_local_graph(
        &mut self,
        _: &OpenLocalGraph,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self
            .active
            .and_then(|i| self.docs.get(i))
            .map(|d| d.entity.read(cx).path.clone())
        else {
            self.note_status("No note open", cx);
            return;
        };
        self.open_local_graph_for(path, window, cx);
    }

    /// Local graph centered on `path` — the tree's context-menu entry
    /// and the `OpenLocalGraph` action share it.
    fn open_local_graph_for(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.weak_entity();
        let vault = self.vault.clone();
        let graph = cx.new(|cx| crate::graph::GraphView::new_local(weak, vault, &path, window, cx));
        graph.read(cx).focus_handle(cx).focus(window, cx);
        self.graph = Some(graph);
        cx.notify();
    }

    /// Point an open graph's halo at the currently active doc.
    fn sync_graph_active(&mut self, cx: &mut Context<Self>) {
        if let Some(graph) = self.graph.clone() {
            let path = self
                .active
                .and_then(|i| self.docs.get(i))
                .map(|d| d.entity.read(cx).path.clone());
            graph.update(cx, |g, _cx| g.active = path);
        }
    }

    /// Close the graph pane and hand focus back to the editor surface.
    pub(crate) fn close_graph(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.graph.take().is_some() {
            self.refocus(window, cx);
            cx.notify();
        }
    }

    fn close_all_docs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.docs.clear();
        self.folder = None;
        self.active = None;
        self.persist_tabs(cx);
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                if this.docs.is_empty() && this.graph.is_none() && this.file_menu.is_none() {
                    this.focus_handle.focus(window, cx);
                }
            });
        });
    }

    /// Snapshot open doc paths + the active one into settings — the
    /// restore-on-launch list. Also keeps an open graph's active-note
    /// halo in step — this runs after every tab mutation.
    fn persist_tabs(&mut self, cx: &mut Context<Self>) {
        self.settings.active_folder = self.folder.as_ref().map(|p| p.path.clone());
        self.sync_graph_active(cx);
        self.settings.open_tabs = self
            .docs
            .iter()
            .map(|d| d.entity.read(cx).path.display().to_string())
            .collect();
        self.settings.active_tab = self
            .active
            .and_then(|i| self.docs.get(i))
            .map(|d| d.entity.read(cx).path.display().to_string());
        self.settings.save();
    }

    /// Palette "Pin tab" / "Unpin tab" — pinned tabs keep their tab
    /// across close-others/close-right and can't be closed until
    /// unpinned. Persisted in `pinned_tabs`.
    fn toggle_pin(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            self.note_status("No note open", cx);
            return;
        };
        self.toggle_pin_at(ix, cx);
    }

    /// Pin/unpin the tab at `ix` (tab context menu + palette share this).
    fn toggle_pin_at(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(doc) = self.docs.get_mut(ix) {
            doc.preview = false;
        }
        let Some(doc) = self.docs.get(ix) else {
            return;
        };
        let path = doc.entity.read(cx).path.display().to_string();
        if let Some(ix) = self.settings.pinned_tabs.iter().position(|p| *p == path) {
            self.settings.pinned_tabs.remove(ix);
            self.note_status("Tab unpinned", cx);
        } else {
            self.settings.pinned_tabs.push(path);
            self.note_status("Tab pinned", cx);
        }
        self.settings.save();
        cx.notify();
    }

    /// Close every tab except `keep` — pinned tabs survive (they refuse
    /// inside `close_tab_at`).
    fn close_others_except(&mut self, keep: usize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self
            .docs
            .iter()
            .enumerate()
            .filter(|(i, d)| *i != keep && !self.is_pinned(&d.entity.read(cx).path))
            .count();
        for ix in (0..self.docs.len()).rev() {
            if ix != keep {
                self.close_tab_at(ix, window, cx);
            }
        }
        self.note_status(format!("Closed {n} other tabs"), cx);
    }

    fn close_tabs_right_of(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        for t in (ix + 1..self.docs.len()).rev() {
            self.close_tab_at(t, window, cx);
        }
    }

    /// Close every unpinned tab — palette "Close all tabs".
    fn close_all_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let n = self
            .docs
            .iter()
            .filter(|d| !self.is_pinned(&d.entity.read(cx).path))
            .count();
        if n == 0 {
            if !self.docs.is_empty() {
                self.note_status("All tabs pinned", cx);
            }
            return;
        }
        for ix in (0..self.docs.len()).rev() {
            if !self.is_pinned(&self.docs[ix].entity.read(cx).path) {
                self.close_tab_at(ix, window, cx);
            }
        }
        self.note_status(format!("Closed {n} tabs"), cx);
    }

    /// Scroll the file tree to `path`, expanding ancestors.
    pub(crate) fn reveal_file(&self, path: &std::path::Path, cx: &mut Context<Self>) {
        let tree = self.vault.read(cx).tree.clone();
        tree.update(cx, |tree, cx| {
            let id: SharedString = path.to_string_lossy().to_string().into();
            tree.reveal_item(&id, ScrollStrategy::Nearest, cx);
        });
    }

    fn is_pinned(&self, path: &std::path::Path) -> bool {
        let s = path.display().to_string();
        self.settings.pinned_tabs.contains(&s)
    }

    fn close_tab_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        if self.is_pinned(&self.docs[ix].entity.read(cx).path) {
            self.note_status("Tab is pinned", cx);
            return;
        }
        let dirty = self.docs[ix].entity.read(cx).dirty;
        let title = self.docs[ix].entity.read(cx).title();
        if dirty {
            let target = self.docs[ix].entity.clone();
            let view = cx.entity();
            window.open_alert_dialog(cx, move |dialog, _window, _cx| {
                let target = target.clone();
                let view = view.clone();
                dialog
                    .title(format!("Close “{}” without saving?", title))
                    .description("The note has unsaved changes.")
                    .show_cancel(true)
                    .ok_text("Close without saving")
                    .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                    .on_ok(move |_, window, cx| {
                        let target_id = target.entity_id();
                        view.update(cx, |this, cx| {
                            if let Some(ix) = this
                                .docs
                                .iter()
                                .position(|doc| doc.entity.entity_id() == target_id)
                            {
                                this.close_tab_now(ix, window, cx);
                            }
                        });
                        true
                    })
            });
            return;
        }
        self.close_tab_now(ix, window, cx);
    }

    /// Drag a tab onto another tab to reorder the strip. `self.active`
    /// tracks by path so the active doc keeps its highlight wherever it
    /// lands; nav history is path-based and unaffected.
    fn move_tab(&mut self, from: usize, to: usize, _window: &mut Window, cx: &mut Context<Self>) {
        if from >= self.docs.len() || to >= self.docs.len() || from == to {
            return;
        }
        let active_path = self
            .active
            .and_then(|i| self.docs.get(i))
            .map(|d| d.entity.read(cx).path.clone());
        let entry = self.docs.remove(from);
        self.docs.insert(to, entry);
        if let Some(path) = active_path {
            self.active = self
                .docs
                .iter()
                .position(|d| d.entity.read(cx).path == path);
        }
        self.persist_tabs(cx);
        cx.notify();
    }

    /// ⌘⇧T / palette — reopen the most recently closed tab.
    fn on_reopen_tab(&mut self, _: &ReopenTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.closed_tabs.pop() {
            self.open_document(path, window, cx);
        }
    }

    fn close_tab_now(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.docs.len() {
            return;
        }
        let closed_focus = self.docs[ix]
            .entity
            .read(cx)
            .editor
            .read(cx)
            .focus_handle(cx);
        let path = self.docs[ix].entity.read(cx).path.clone();
        self.docs.remove(ix);
        self.closed_tabs.push(path);
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
        self.persist_tabs(cx);
        self.reveal_active_file(cx);
        cx.notify();
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                let focused = window.focused(cx);
                if focused
                    .as_ref()
                    .is_none_or(|focused| focused == &closed_focus)
                {
                    this.refocus(window, cx);
                }
            });
        });
    }

    /// Expand ancestors and scroll the file tree to the active document —
    /// the "reveal active file in navigation", automatic.
    fn reveal_active_file(&self, cx: &mut Context<Self>) {
        let Some(path) = self.active_doc().map(|d| d.read(cx).path.clone()) else {
            return;
        };
        self.reveal_file(&path, cx);
    }

    /// Insert `text` at the active editor's caret (palette insert cmds).
    fn insert_at_caret(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        let text = text.to_string();
        doc.update(cx, |doc, cx| {
            doc.editor
                .update(cx, |e, cx| e.insert(text.clone(), window, cx));
        });
    }

    fn save_all(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        for doc in &self.docs {
            doc.entity.update(cx, |doc, cx| doc.flush_and_save(cx))?;
        }
        Ok(())
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

    /// Palette "New base" — a starter `.base` at the vault root,
    /// named like Untitled notes, opened in Preview.
    fn new_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let mut name = "Untitled.base".to_string();
        let mut n = 1;
        while root.join(&name).exists() {
            n += 1;
            name = format!("Untitled {n}.base");
        }
        let path = root.join(&name);
        let spec = format!(
            "filters:\n  and:\n    - file.folder != \"{}\"\nviews:\n  - type: table\n    name: Table\n",
            self.settings.templates_dir
        );
        if std::fs::write(&path, spec.as_bytes()).is_ok() {
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
            self.open_document(path, window, cx);
        }
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

    fn toggle_tree_sort(&mut self, cx: &mut Context<Self>) {
        self.settings.tree_sort = match self.settings.tree_sort {
            TreeSort::Name => TreeSort::Modified,
            TreeSort::Modified => TreeSort::Type,
            TreeSort::Type => TreeSort::Size,
            TreeSort::Size => TreeSort::Name,
        };
        self.settings.save();
        self.vault.update(cx, |vault, cx| {
            vault.tree_sort = self.settings.tree_sort;
            vault.refresh(cx);
        });
        cx.notify();
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
                    let _ = window.update(|window, cx| {
                        view.update(cx, |this, cx| this.open_vault_at(path, window, cx))
                    });
                }
            }
        })
        .detach();
    }

    fn on_open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.tr("Open a file", "Åpne en fil").into()),
        });
        cx.spawn_in(window, async move |view, window| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = window.update(|window, cx| {
                        view.update(cx, |this, cx| this.open_path(path, window, cx))
                    });
                }
            }
        })
        .detach();
    }

    /// `<daily_dir>/<daily_format>.md` for `date` — the daily-format
    /// setting is Moment syntax (`YYYY-MM-DD`), translated via
    /// `bases::moment_to_chrono`.
    fn daily_path_for(&self, date: NaiveDate, cx: &App) -> Option<PathBuf> {
        let fmt = crate::bases::moment_to_chrono(&self.settings.daily_format);
        self.vault
            .read(cx)
            .daily_note(date, &self.settings.daily_dir, &fmt)
    }

    fn on_open_daily(&mut self, _: &OpenDailyNote, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .daily_path_for(chrono::Local::now().date_naive(), cx)
            .is_none()
        {
            self.note_status("Open a folder first", cx);
            return;
        }
        self.open_daily_at(chrono::Local::now().date_naive(), window, cx);
    }

    /// Initial content for a missing daily note — the reference editor convention:
    /// `<templates_dir>/daily.md` seeds it with `{{date}}`/`{{time}}`/
    /// `{{title}}`/`{{cursor}}` expanded, else a plain heading.
    fn daily_seed(&self, path: &Path, cx: &App) -> String {
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        self.vault
            .read(cx)
            .root
            .as_ref()
            .and_then(|root| {
                std::fs::read_to_string(root.join(&self.settings.templates_dir).join("daily.md"))
                    .ok()
            })
            .map(|tpl| {
                let now = crate::history::epoch();
                tpl.replace("{{date}}", &crate::history::format_date(now))
                    .replace("{{time}}", &crate::history::format_time(now))
                    .replace("{{title}}", &title)
                    .replace("{{cursor}}", "")
            })
            .unwrap_or_else(|| format!("# {}\n\n", title))
    }

    /// Shared tail of `on_open_daily`: write (templated if missing),
    /// refresh the vault, open — `daily_dir`/`daily_format` resolve
    /// `date` to the path.
    pub(crate) fn open_daily_at(
        &mut self,
        date: NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.daily_path_for(date, cx) else {
            self.note_status("Open a folder first", cx);
            return;
        };
        if !path.exists() {
            if path
                .parent()
                .is_some_and(|dir| std::fs::create_dir_all(dir).is_err())
                || std::fs::write(&path, self.daily_seed(&path, cx)).is_err()
            {
                return;
            }
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
        }
        self.open_document(path, window, cx);
    }

    /// Palette "Append to daily note…" — prompt for a line, append it
    /// as `- {text}` without opening the note.
    fn show_append_daily_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.daily_path_for(chrono::Local::now().date_naive(), cx) else {
            self.note_status("Open a folder first", cx);
            return;
        };
        let input = self.cell_input.clone();
        input.update(cx, |input, cx| input.set_value("", window, cx));
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title("Append to daily note")
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .on_ok({
                    let view = view.clone();
                    let path = path.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            let text = this.cell_input.read(cx).value().trim().to_string();
                            if !text.is_empty() {
                                this.append_daily(&path, &text, window, cx);
                            }
                            this.refocus(window, cx);
                        });
                        true
                    }
                })
        });

        let input = self.cell_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx));
        });
    }

    /// `- {text}` appended to the daily note — through the editor when
    /// it's open (autosave persists), straight to disk otherwise.
    fn append_daily(
        &mut self,
        path: &Path,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !path.exists()
            && (path
                .parent()
                .is_some_and(|dir| std::fs::create_dir_all(dir).is_err())
                || std::fs::write(path, self.daily_seed(path, cx)).is_err())
        {
            self.note_status("Could not create daily note", cx);
            return;
        }
        let line = format!("- {text}");
        if let Some(doc) = self
            .docs
            .iter()
            .find(|d| d.entity.read(cx).path == path)
            .map(|d| d.entity.clone())
        {
            doc.update(cx, |doc, cx| doc.append_line(&line, window, cx));
        } else {
            let mut body = std::fs::read_to_string(path).unwrap_or_default();
            if !body.ends_with('\n') && !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&line);
            body.push('\n');
            if std::fs::write(path, body).is_err() {
                self.note_status("Could not append", cx);
                return;
            }
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
        }
        self.note_status("Appended to daily note", cx);
    }

    fn on_close_folder(&mut self, _: &CloseFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.close_vault(window, cx);
    }

    fn on_save(&mut self, _: &SaveFile, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            if let Err(err) = doc.update(cx, |doc, cx| doc.save(cx)) {
                self.note_status(format!("Could not save: {err}"), cx);
            }
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
                        match doc.update(cx, |doc, cx| doc.save_as(path, cx)) {
                            Err(err) => {
                                this.note_status(format!("Could not save: {err}"), cx);
                            }
                            Ok(()) => {
                                let saved_path = doc.read(cx).path.clone();
                                let root = this
                                    .vault
                                    .read(cx)
                                    .root
                                    .clone()
                                    .filter(|root| saved_path.starts_with(root));
                                let resolver: ImageResolver = if root.is_some() {
                                    this.vault.read(cx).image_resolver()
                                } else {
                                    let parent = saved_path
                                        .parent()
                                        .map(Path::to_path_buf)
                                        .unwrap_or_default();
                                    std::rc::Rc::new(move |reference| {
                                        let candidate = parent.join(reference);
                                        candidate.exists().then_some(candidate)
                                    })
                                };
                                let is_standalone = root.is_none();
                                let vault = root.as_ref().map(|_| this.vault.clone());
                                doc.update(cx, |doc, cx| {
                                    doc.set_location_context(root, vault, resolver, cx)
                                });
                                if is_standalone {
                                    this.start_standalone_fs_check(cx);
                                }
                                this.persist_tabs(cx);
                                if this
                                    .vault
                                    .read(cx)
                                    .root
                                    .as_ref()
                                    .is_some_and(|root| saved_path.starts_with(root))
                                {
                                    this.vault.update(cx, |vault, cx| vault.refresh(cx));
                                }
                                this.note_status(format!("Saved {}", saved_path.display()), cx);
                            }
                        }
                    })
                });
            }
        })
        .detach();
    }

    fn confirm_reload_from_disk(
        &mut self,
        doc: Entity<Document>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = doc.read(cx).title();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            let doc = doc.clone();
            dialog
                .title(format!("Reload “{}” from disk?", title))
                .description("This will discard your local changes.")
                .show_cancel(true)
                .ok_text("Discard and reload")
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let reload_result = doc.update(cx, |doc, cx| doc.reload(window, cx));
                    view.update(cx, |this, cx| match reload_result {
                        Ok(()) => this.note_status("Reloaded from disk", cx),
                        Err(err) => this.note_status(format!("Could not reload: {err}"), cx),
                    });
                    true
                })
        });
    }

    /// Palette → "Export note as HTML…" — a save panel for `<stem>.html`
    /// next to the note; the body is `markdown::to_html` wrapped in a
    /// minimal readable stylesheet.
    fn export_html(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            self.note_status("Open a note first", cx);
            return;
        };
        let (text, title, dir) = {
            let doc = doc.read(cx);
            (
                doc.editor.read(cx).value().to_string(),
                doc.title(),
                doc.path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default(),
            )
        };
        let name = format!("{title}.html");
        let body = markdown::to_html(&text);
        let html = format!(
            "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
             <title>{title}</title>\n\
             <style>body{{max-width:44rem;margin:2rem auto;padding:0 1rem;\
             font-family:-apple-system,sans-serif;line-height:1.6}}\
             img{{max-width:100%}}pre{{padding:1em;overflow:auto;background:#f5f5f5}}\
             blockquote{{border-left:3px solid #ddd;margin:0;padding-left:1em;color:#555}}</style>\n\
             </head>\n<body>\n{body}</body>\n</html>\n"
        );
        let receiver = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |view, window| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let _ = window.update(|_window, cx| {
                    view.update(cx, |this, cx| {
                        if std::fs::write(&path, &html).is_ok() {
                            this.note_status(format!("Exported {}", path.display()), cx);
                        } else {
                            this.note_status("Export failed", cx);
                        }
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

    fn on_toggle_checkbox(
        &mut self,
        _: &ToggleCheckbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self.active_doc().cloned() {
            let toggled = doc.update(cx, |doc, cx| doc.toggle_task_at_cursor(window, cx));
            if !toggled {
                self.note_status("No task on this line", cx);
            }
        }
    }

    /// ⌘I — italic wrap on the selection or the word under the caret.
    fn on_toggle_italic(&mut self, _: &ToggleItalic, window: &mut Window, cx: &mut Context<Self>) {
        self.wrap("*", window, cx);
    }

    /// Inline-formatting wrap — `**`, `*`, `~~`, `==` around the
    /// selection or the word under the caret (palette + ⌘I).
    fn wrap(&mut self, marker: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.toggle_wrap(marker, window, cx));
        }
    }

    /// "Extract selection to new note" — the name dialog.
    fn show_extract_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        let Some((body, range)) = doc.read(cx).selected_text(cx) else {
            self.note_status("Select some text first", cx);
            return;
        };
        let input = self.rename_input.clone();
        input.update(cx, |input, cx| input.set_value("", window, cx));
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title("Extract to new note")
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .on_ok({
                    let view = view.clone();
                    let doc = doc.clone();
                    let range = range.clone();
                    let body = body.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.commit_extract(
                                doc.clone(),
                                range.clone(),
                                body.clone(),
                                window,
                                cx,
                            );
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

    /// Writes `<name>.md` with the selection's body and replaces the
    /// selection with `[[name]]` — the Note Composer extract.
    fn commit_extract(
        &mut self,
        doc: Entity<Document>,
        range: std::ops::Range<usize>,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.rename_input.read(cx).value().to_string();
        let name = name.trim().trim_end_matches(".md").replace('\\', "/");
        let name = name.trim();
        let bad_name = name.is_empty()
            || name.starts_with('/')
            || name.contains("..")
            || name
                .chars()
                .any(|c| matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'));
        if bad_name {
            self.note_status("Invalid note name", cx);
            return;
        }
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let dir = if name.contains('/') {
            root.clone()
        } else {
            doc.read(cx)
                .path
                .parent()
                .map(|p| p.to_path_buf())
                .filter(|p| p.starts_with(&root))
                .unwrap_or_else(|| root.clone())
        };
        let path = dir.join(format!("{name}.md"));
        if path.exists() {
            self.note_status(format!("\u{201c}{name}\u{201d} already exists"), cx);
            return;
        }
        if std::fs::write(&path, body).is_err() {
            self.note_status("Couldn't write the note", cx);
            return;
        }
        doc.update(cx, |doc, cx| {
            doc.replace_range(range, format!("[[{name}]]"), window, cx);
        });
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        self.note_status(format!("Extracted to {name}.md"), cx);
    }

    /// Palette list toggles — bulleted/numbered/checklist.
    fn list_toggle(&mut self, style: &'static str, cx: &mut Context<Self>, window: &mut Window) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.toggle_list(style, window, cx));
        }
    }

    /// Palette heading toggles — `#`*N + ` ` per selected line.
    fn heading_toggle(&mut self, level: usize, cx: &mut Context<Self>, window: &mut Window) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.toggle_heading(level, window, cx));
        }
    }

    /// File context menu "Duplicate" — `<stem> copy.ext` (then
    /// `copy 2`, `copy 3`, …) next to the source; refreshes the tree.
    fn duplicate_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(parent) = path.parent().map(|p| p.to_path_buf()) else {
            return;
        };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("copy");
        let ext = path.extension().and_then(|e| e.to_str());
        let mut n = 0u32;
        let dest = loop {
            let suffix = if n == 0 {
                " copy".to_string()
            } else {
                format!(" copy {}", n + 1)
            };
            let name = match ext {
                Some(e) => format!("{stem}{suffix}.{e}"),
                None => format!("{stem}{suffix}"),
            };
            let cand = parent.join(name);
            if !cand.exists() {
                break cand;
            }
            n += 1;
            if n > 99 {
                return;
            }
        };
        if std::fs::copy(&path, &dest).is_ok() {
            self.vault.update(cx, |v, cx| v.refresh(cx));
            let name = dest
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            self.note_status(format!("Duplicated as {name}"), cx);
        }
    }

    /// Copy the vault-relative path (forward slashes) to the clipboard.
    fn copy_rel_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let rel = self
            .vault
            .read(cx)
            .root
            .as_ref()
            .and_then(|root| path.strip_prefix(root).ok().map(|p| p.to_path_buf()))
            .unwrap_or(path);
        cx.write_to_clipboard(ClipboardItem::new_string(
            rel.to_string_lossy().replace('\\', "/"),
        ));
        self.note_status("Path copied", cx);
    }

    /// ⌘⇧K — delete the line(s) under the selection.
    fn on_delete_line(&mut self, _: &DeleteLine, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.delete_lines(window, cx));
        }
    }

    /// ⌘D — duplicate the line(s) under the selection.
    fn on_duplicate_block(
        &mut self,
        _: &DuplicateBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.duplicate_block(window, cx));
        }
    }

    /// ⌘/ — toggle `%%` around the selection (or the current line).
    fn on_toggle_comment(
        &mut self,
        _: &ToggleComment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.toggle_comment(window, cx));
        }
    }

    /// ⌘+/⌘−/⌘0 — editor font zoom, clamped to a readable range.
    fn adjust_editor_font(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.settings.clone();
        s.editor_font_size = (s.editor_font_size + delta).clamp(10.0, 28.0);
        self.note_status(format!("Editor font {:.0}px", s.editor_font_size), cx);
        self.apply_settings(s, window, cx);
    }

    fn on_zoom_in(&mut self, _: &ZoomIn, w: &mut Window, cx: &mut Context<Self>) {
        self.adjust_editor_font(1.0, w, cx);
    }

    fn on_zoom_out(&mut self, _: &ZoomOut, w: &mut Window, cx: &mut Context<Self>) {
        self.adjust_editor_font(-1.0, w, cx);
    }

    fn on_zoom_reset(&mut self, _: &ZoomReset, w: &mut Window, cx: &mut Context<Self>) {
        let mut s = self.settings.clone();
        s.editor_font_size = 14.0;
        self.apply_settings(s, w, cx);
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.folder.take().is_some() {
            self.active = self.docs.len().checked_sub(1);
            self.persist_tabs(cx);
            self.refocus(window, cx);
            cx.notify();
            return;
        }
        if let Some(ix) = self.active {
            self.close_tab_at(ix, window, cx);
        }
    }

    fn on_next_tab(&mut self, _: &NextTab, _window: &mut Window, cx: &mut Context<Self>) {
        if self.docs.is_empty() {
            return;
        }
        self.folder = None;
        self.graph = None;
        self.active = Some(match self.active {
            Some(i) => (i + 1) % self.docs.len(),
            None => 0,
        });
        self.persist_tabs(cx);
        self.reveal_active_file(cx);
        cx.notify();
    }

    fn on_prev_tab(&mut self, _: &PrevTab, _window: &mut Window, cx: &mut Context<Self>) {
        if self.docs.is_empty() {
            return;
        }
        self.folder = None;
        self.graph = None;
        self.active = Some(match self.active {
            Some(0) | None => self.docs.len() - 1,
            Some(i) => i - 1,
        });
        self.persist_tabs(cx);
        self.reveal_active_file(cx);
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

    fn toggle_focus_mode(&mut self, cx: &mut Context<Self>) {
        self.focus_mode = !self.focus_mode;
        let on = self.focus_mode;
        for doc in &self.docs {
            doc.entity.update(cx, |doc, cx| doc.set_focus_mode(on, cx));
        }
        self.settings.focus_mode = on;
        self.settings.save();
        self.note_status(
            if on {
                "Focus mode on"
            } else {
                "Focus mode off"
            },
            cx,
        );
        cx.notify();
    }

    /// Star/unstar a note — pinned group at the top of the sidebar,
    /// persisted in settings (absolute paths).
    fn toggle_star(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let key = path.to_string_lossy().to_string();
        let starred = if let Some(ix) = self.settings.starred.iter().position(|s| s == &key) {
            self.settings.starred.remove(ix);
            false
        } else {
            self.settings.starred.push(key.clone());
            true
        };
        self.note_status(if starred { "Starred" } else { "Unstarred" }, cx);
        // `file.starred` in .base reads the vault copy; the event
        // re-renders any open base views.
        self.vault.update(cx, |vault, cx| {
            if starred {
                vault.starred.insert(key);
            } else {
                vault.starred.remove(&key);
            }
            cx.emit(VaultEvent::StarredChanged);
        });
        self.settings.save();
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

    /// ⌘E — the edit/read toggle: Preview ⇄ Source (Split
    /// counts as editing → jumps to Preview).
    fn on_toggle_edit_preview(
        &mut self,
        _: &ToggleEditPreview,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = if self.settings.view_mode == ViewMode::Preview {
            ViewMode::Source
        } else {
            ViewMode::Preview
        };
        self.set_view_mode(mode, w, cx);
    }

    fn on_toggle_theme(&mut self, _: &ToggleTheme, _w: &mut Window, cx: &mut Context<Self>) {
        self.settings.appearance = match self.settings.appearance {
            Appearance::Dark => Appearance::Light,
            _ => Appearance::Dark,
        };
        theme::apply(&self.settings, cx);
        self.settings.save();
        cx.notify();
    }

    pub(crate) fn quit(&mut self, cx: &mut Context<Self>) {
        match self.save_all(cx) {
            Ok(()) => cx.quit(),
            Err(err) => self.note_status(format!("Could not save before quitting: {err}"), cx),
        }
    }

    fn on_about(&mut self, _: &About, _w: &mut Window, cx: &mut Context<Self>) {
        self.note_status("Rísta — a quiet place for words.", cx);
    }

    fn on_check_for_updates(
        &mut self,
        _: &CheckForUpdates,
        _w: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        crate::updater::check_for_updates();
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
            PaletteCmd::NewBase,
            PaletteCmd::DailyNote,
            PaletteCmd::AppendDaily,
            PaletteCmd::OpenFile,
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
            PaletteCmd::Graph,
            PaletteCmd::LocalGraph,
            PaletteCmd::MoveLineUp,
            PaletteCmd::MoveLineDown,
            PaletteCmd::ToggleCheckbox,
            PaletteCmd::ToggleBold,
            PaletteCmd::ToggleItalic,
            PaletteCmd::ToggleHighlight,
            PaletteCmd::ToggleStrike,
            PaletteCmd::ExtractSelection,
            PaletteCmd::ToggleCode,
            PaletteCmd::ToggleQuote,
            PaletteCmd::CodeBlock,
            PaletteCmd::ListBullet,
            PaletteCmd::ListNumbered,
            PaletteCmd::ListTask,
            PaletteCmd::Heading1,
            PaletteCmd::Heading2,
            PaletteCmd::Heading3,
            PaletteCmd::Heading4,
            PaletteCmd::Heading5,
            PaletteCmd::Heading6,
            PaletteCmd::InsertLink,
            PaletteCmd::InsertHr,
            PaletteCmd::InsertCallout,
            PaletteCmd::InsertDate,
            PaletteCmd::InsertTime,
            PaletteCmd::InsertImage,
            PaletteCmd::InsertTable,
            PaletteCmd::FormatTable,
            PaletteCmd::InsertFootnote,
            PaletteCmd::DeleteLine,
            PaletteCmd::PageHistory,
            PaletteCmd::RestoreDeleted,
            PaletteCmd::InsertTemplate,
            PaletteCmd::DuplicateNote,
            PaletteCmd::EditProperties,
            PaletteCmd::BrowseTags,
            PaletteCmd::Backlinks,
            PaletteCmd::Outline,
            PaletteCmd::ToggleFocus,
            PaletteCmd::GoBack,
            PaletteCmd::GoForward,
            PaletteCmd::FollowLink,
            PaletteCmd::ToggleStar,
            PaletteCmd::ReopenTab,
            PaletteCmd::CopyLink,
            PaletteCmd::CopyLinkHeading,
            PaletteCmd::RevealFile,
            PaletteCmd::CloseOtherTabs,
            PaletteCmd::CloseTabsRight,
            PaletteCmd::CloseAllTabs,
            PaletteCmd::TogglePin,
            PaletteCmd::ToggleReadable,
            PaletteCmd::ExportHtml,
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
        // Recently opened notes come first — the reference editor quick-switcher
        // style — but only ones still on disk.
        let recent: Vec<PaletteEntry> = self
            .recent
            .iter()
            .filter(|p| p.is_file())
            .cloned()
            .map(PaletteEntry::File)
            .collect();

        self.palette_sections = vec![commands
            .iter()
            .cloned()
            .map(PaletteEntry::Command)
            .collect()];
        if !recent.is_empty() {
            self.palette_sections.push(recent.clone());
        }
        self.palette_sections.push(notes);

        let file_item = |path: &std::path::Path| {
            CommandItem::new()
                .icon(assets::IconName::File)
                .label(
                    path.file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default(),
                )
                .keywords([path.to_string_lossy().to_string()])
        };
        let recent_items: Vec<CommandItem> = recent
            .iter()
            .filter_map(|entry| match entry {
                PaletteEntry::File(path) => Some(file_item(path)),
                _ => None,
            })
            .collect();

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
        let notes_section = self.palette_sections.len() - 1;
        let note_items: Vec<CommandItem> = self.palette_sections[notes_section]
            .iter()
            .map(|entry| match entry {
                PaletteEntry::File(path) => file_item(path),
                _ => CommandItem::new().label(""),
            })
            .collect();
        let (tools, _) = self.tool_launchers(cx);
        let tool_items: Vec<_> = tools
            .iter()
            .map(|tool| {
                CommandItem::new()
                    .icon(assets::IconName::Terminal)
                    .label(tool.name.text(self.settings.language).to_string())
                    .keywords([tool.program.clone()])
            })
            .collect();
        self.palette_sections
            .push(tools.into_iter().map(PaletteEntry::Tool).collect());
        let tools_label = self.tr("Tools", "Verktøy");

        let state = self.palette_state.clone();
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let mut palette = Command::new(&state)
                .placeholder("Notes and commands…")
                .searchable(true)
                .filterable(true)
                .group(
                    CommandGroup::new()
                        .label("Commands")
                        .items(command_items.clone()),
                );
            if !recent_items.is_empty() {
                palette = palette.group(
                    CommandGroup::new()
                        .label("Recent")
                        .items(recent_items.clone()),
                );
            }
            palette = palette
                .group(CommandGroup::new().label("Notes").items(note_items.clone()))
                .group(
                    CommandGroup::new()
                        .label(tools_label)
                        .items(tool_items.clone()),
                )
                .max_h(px(440.));
            dialog
                .w(px(560.))
                .overlay_closable(true)
                .close_button(false)
                .child(
                    palette
                        .on_confirm({
                            let view = view.clone();
                            move |index, window, cx| {
                                window.close_dialog(cx);
                                view.update(cx, |this, cx| {
                                    this.refocus(window, cx);
                                    this.on_palette_pick(index, window, cx);
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
            Some(PaletteEntry::Tool(tool)) => self.confirm_tool(tool, window, cx),
            None => {}
        }
    }

    fn run_command(&mut self, cmd: PaletteCmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            PaletteCmd::NewFile => self.on_new_file(&NewFile, window, cx),
            PaletteCmd::NewBase => self.new_base(window, cx),
            PaletteCmd::NewFolder => self.on_new_folder(&NewFolder, window, cx),
            PaletteCmd::OpenFile => self.on_open_file(&OpenFile, window, cx),
            PaletteCmd::OpenFolder => self.on_open_folder(&OpenFolder, window, cx),
            PaletteCmd::DailyNote => self.on_open_daily(&OpenDailyNote, window, cx),
            PaletteCmd::AppendDaily => {
                self.defer_dialog(Self::show_append_daily_dialog, window, cx)
            }
            PaletteCmd::CloseFolder => {
                self.close_vault(window, cx);
            }
            PaletteCmd::Save => self.on_save(&SaveFile, window, cx),
            PaletteCmd::SaveAs => self.on_save_as(&SaveFileAs, window, cx),
            PaletteCmd::SourceMode => self.set_view_mode(ViewMode::Source, window, cx),
            PaletteCmd::SplitMode => self.set_view_mode(ViewMode::Split, window, cx),
            PaletteCmd::PreviewMode => self.set_view_mode(ViewMode::Preview, window, cx),
            PaletteCmd::ToggleSidebar => self.on_toggle_sidebar(&ToggleSidebar, window, cx),
            PaletteCmd::ToggleZen => self.on_toggle_zen(&ToggleZen, window, cx),
            PaletteCmd::ProjectSearch => self.defer_dialog(
                |ws, window, cx| ws.on_open_project_search(&OpenProjectSearch, window, cx),
                window,
                cx,
            ),
            PaletteCmd::Graph => self.on_open_graph(&OpenGraph, window, cx),
            PaletteCmd::LocalGraph => self.on_open_local_graph(&OpenLocalGraph, window, cx),
            PaletteCmd::MoveLineUp => self.on_move_line_up(&MoveLineUp, window, cx),
            PaletteCmd::MoveLineDown => self.on_move_line_down(&MoveLineDown, window, cx),
            PaletteCmd::ToggleCheckbox => self.on_toggle_checkbox(&ToggleCheckbox, window, cx),
            PaletteCmd::ToggleBold => self.wrap("**", window, cx),
            PaletteCmd::ToggleItalic => self.wrap("*", window, cx),
            PaletteCmd::ToggleHighlight => self.wrap("==", window, cx),
            PaletteCmd::ToggleStrike => self.wrap("~~", window, cx),
            PaletteCmd::ExtractSelection => {
                self.defer_dialog(Self::show_extract_dialog, window, cx)
            }
            PaletteCmd::ToggleCode => self.wrap("`", window, cx),
            PaletteCmd::ToggleQuote => {
                if let Some(doc) = self.active_doc().cloned() {
                    doc.update(cx, |doc, cx| doc.toggle_line_prefix("> ", window, cx));
                }
            }
            PaletteCmd::CodeBlock => {
                if let Some(doc) = self.active_doc().cloned() {
                    doc.update(cx, |doc, cx| doc.toggle_fence(window, cx));
                }
            }
            PaletteCmd::ListBullet => self.list_toggle("- ", cx, window),
            PaletteCmd::ListNumbered => self.list_toggle("1. ", cx, window),
            PaletteCmd::ListTask => self.list_toggle("- [ ] ", cx, window),
            PaletteCmd::Heading1 => self.heading_toggle(1, cx, window),
            PaletteCmd::Heading2 => self.heading_toggle(2, cx, window),
            PaletteCmd::Heading3 => self.heading_toggle(3, cx, window),
            PaletteCmd::Heading4 => self.heading_toggle(4, cx, window),
            PaletteCmd::Heading5 => self.heading_toggle(5, cx, window),
            PaletteCmd::Heading6 => self.heading_toggle(6, cx, window),
            PaletteCmd::InsertLink => {
                let url = cx.read_from_clipboard().and_then(|item| {
                    item.entries.iter().find_map(|e| match e {
                        ClipboardEntry::String(t) => {
                            let u = t.text.trim();
                            ((u.starts_with("https://") || u.starts_with("http://"))
                                && !u.chars().any(char::is_whitespace)
                                && u.len() < 2048)
                                .then(|| u.to_string())
                        }
                        _ => None,
                    })
                });
                if let Some(doc) = self.active_doc().cloned() {
                    doc.update(cx, |doc, cx| doc.insert_link(url.as_deref(), window, cx));
                }
            }
            PaletteCmd::InsertHr => {
                if let Some(doc) = self.active_doc().cloned() {
                    doc.update(cx, |doc, cx| doc.insert_hr(window, cx));
                }
            }
            PaletteCmd::InsertCallout => {
                if let Some(doc) = self.active_doc().cloned() {
                    doc.update(cx, |doc, cx| doc.toggle_callout("note", window, cx));
                }
            }
            PaletteCmd::InsertDate => self.insert_at_caret(
                &chrono::Local::now().format("%Y-%m-%d").to_string(),
                window,
                cx,
            ),
            PaletteCmd::InsertTime => self.insert_at_caret(
                &chrono::Local::now().format("%H:%M").to_string(),
                window,
                cx,
            ),
            PaletteCmd::InsertImage => {
                // Native multi-file picker → each image copies into
                // attachments/ and inserts ![[name]] at the caret
                // (import_paths, shared with paste/drop).
                let receiver = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: true,
                    prompt: Some("Insert image".into()),
                });
                cx.spawn_in(window, async move |view, window| {
                    if let Ok(Ok(Some(paths))) = receiver.await {
                        let _ = window.update(|window, cx| {
                            view.update(cx, |this, cx| {
                                this.import_paths(&paths, window, cx);
                            })
                        });
                    }
                })
                .detach();
            }
            PaletteCmd::InsertTable => {
                if let Some(doc) = self.active_doc().cloned() {
                    doc.update(cx, |doc, cx| doc.insert_table(window, cx));
                }
            }
            PaletteCmd::FormatTable => {
                if let Some(doc) = self.active_doc().cloned() {
                    let done = doc.update(cx, |doc, cx| doc.format_table(window, cx));
                    if !done {
                        self.note_status("No table at the caret", cx);
                    }
                }
            }
            PaletteCmd::InsertFootnote => {
                if let Some(doc) = self.active_doc().cloned() {
                    doc.update(cx, |doc, cx| doc.insert_footnote(window, cx));
                }
            }
            PaletteCmd::DeleteLine => self.on_delete_line(&DeleteLine, window, cx),
            // These commands open their own dialog — defer past the
            // palette's own close_dialog, which would close them too.
            PaletteCmd::PageHistory => self.defer_dialog(Self::show_history, window, cx),
            PaletteCmd::RestoreDeleted => self.defer_dialog(Self::show_trash, window, cx),
            PaletteCmd::InsertTemplate => self.defer_dialog(Self::show_templates, window, cx),
            PaletteCmd::DuplicateNote => {
                if let Some(doc) = self.active_doc().cloned() {
                    let path = doc.read(cx).path.clone();
                    self.duplicate_file(path, cx);
                } else {
                    self.note_status("No note open", cx);
                }
            }
            PaletteCmd::EditProperties => self.defer_dialog(Self::show_properties, window, cx),
            PaletteCmd::BrowseTags => self.defer_dialog(Self::show_tags, window, cx),
            PaletteCmd::Backlinks => self.defer_dialog(Self::show_backlinks, window, cx),
            PaletteCmd::Outline => self.defer_dialog(Self::show_outline, window, cx),
            PaletteCmd::ToggleFocus => self.toggle_focus_mode(cx),
            PaletteCmd::GoBack => self.nav_back(window, cx),
            PaletteCmd::GoForward => self.nav_forward(window, cx),
            PaletteCmd::FollowLink => self.on_follow_link(&FollowLink, window, cx),
            PaletteCmd::ReopenTab => self.on_reopen_tab(&ReopenTab, window, cx),
            PaletteCmd::ToggleStar => {
                let path = self.active_doc().map(|d| d.read(cx).path.clone());
                match path {
                    Some(path) => self.toggle_star(path, cx),
                    None => self.note_status("No note open", cx),
                }
            }
            PaletteCmd::CopyLink => {
                let link = self.active_doc().and_then(|d| {
                    d.read(cx)
                        .path
                        .file_stem()
                        .map(|s| format!("[[{}]]", s.to_string_lossy()))
                });
                match link {
                    Some(link) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                        self.note_status(format!("Copied {link}"), cx);
                    }
                    None => self.note_status("No note open", cx),
                }
            }
            PaletteCmd::CopyLinkHeading => {
                // `[[stem#heading]]` for the nearest heading at or above
                // the caret — the "copy link to heading".
                let link = self.active_doc().and_then(|d| {
                    let doc = d.read(cx);
                    let stem = doc.path.file_stem()?.to_string_lossy().to_string();
                    let text = doc.editor.read(cx).value().to_string();
                    let cursor = doc.editor.read(cx).cursor().min(text.len());
                    let line_no = text[..cursor].matches('\n').count() + 1;
                    let mut heading: Option<String> = None;
                    let mut in_fence = false;
                    for (ix, line) in text.split('\n').enumerate() {
                        if ix + 1 > line_no {
                            break;
                        }
                        let t = line.trim_start();
                        if t.starts_with("```") {
                            in_fence = !in_fence;
                            continue;
                        }
                        if in_fence {
                            continue;
                        }
                        let level = t.chars().take_while(|&c| c == '#').count();
                        if (1..=6).contains(&level) && t.chars().nth(level) == Some(' ') {
                            heading = Some(t[level + 1..].trim().to_string());
                        }
                    }
                    Some(match heading {
                        Some(h) => format!("[[{stem}#{h}]]"),
                        None => format!("[[{stem}]]"),
                    })
                });
                match link {
                    Some(link) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                        self.note_status(format!("Copied {link}"), cx);
                    }
                    None => self.note_status("No note open", cx),
                }
            }
            PaletteCmd::CloseOtherTabs => {
                if let Some(active) = self.active {
                    self.close_others_except(active, window, cx);
                } else {
                    self.note_status("No active tab", cx);
                }
            }
            PaletteCmd::CloseTabsRight => {
                if let Some(active) = self.active {
                    self.close_tabs_right_of(active, window, cx);
                }
            }
            PaletteCmd::CloseAllTabs => self.close_all_tabs(window, cx),
            PaletteCmd::RevealFile => self.reveal_active_file(cx),
            PaletteCmd::TogglePin => self.toggle_pin(cx),
            PaletteCmd::ToggleReadable => {
                self.settings.readable_width = !self.settings.readable_width;
                let on = self.settings.readable_width;
                self.settings.save();
                self.note_status(
                    if on {
                        "Readable line length on"
                    } else {
                        "Readable line length off"
                    },
                    cx,
                );
                cx.notify();
            }
            PaletteCmd::ExportHtml => self.export_html(window, cx),
            PaletteCmd::Settings => self.defer_dialog(
                |ws, window, cx| ws.on_open_settings(&OpenSettings, window, cx),
                window,
                cx,
            ),
            PaletteCmd::ToggleTheme => self.on_toggle_theme(&ToggleTheme, window, cx),
            PaletteCmd::Quit => self.quit(cx),
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
        if self.settings.properties_visibility != settings.properties_visibility {
            for doc in &self.docs {
                doc.entity.update(cx, |doc, _| {
                    doc.callout_folds = preview::CalloutFolds::default();
                });
            }
        }
        let files_changed = self.settings.show_other_files != settings.show_other_files;
        self.settings = settings.clone();
        if files_changed {
            self.vault.update(cx, |vault, cx| {
                vault.show_other_files = settings.show_other_files;
                vault.refresh(cx);
            });
            if let Some(page) = &self.folder {
                self.folder =
                    Some(self.load_folder_page(page.path.clone(), page.scroll.clone(), cx));
            }
        }
        theme::apply(&settings, cx);
        for tab in &self.terminals {
            tab.terminal
                .update(cx, |terminal, cx| terminal.apply_settings(&settings, cx));
        }
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
                .title("Rename / move")
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
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        });
    }

    fn commit_rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.rename_input.read(cx).value().to_string();
        let name = name.trim();
        let bad_name = name.is_empty()
            || name.starts_with('/')
            || name.ends_with('/')
            || name.contains("..")
            || name
                .chars()
                .any(|c| matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'));
        if bad_name {
            self.note_status("Invalid name", cx);
            return;
        }
        // A `/` in the name moves the file: it resolves against the
        // vault root (the rename-as-move), creating folders
        // on the way. A bare name stays in the same directory.
        let new_path = if name.contains('/') {
            let Some(root) = self.vault.read(cx).root.clone() else {
                return;
            };
            root.join(name)
        } else {
            let Some(dir) = path.parent().map(|p| p.to_path_buf()) else {
                return;
            };
            dir.join(name)
        };
        if new_path == path || new_path.exists() {
            return;
        }
        if name.contains('/') {
            if let Some(parent) = new_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        // Collect inbound `[[link]]`/`![[embed]]` targets resolving to the
        // file *before* it moves — resolution uses the pre-rename index.
        // Notes retarget by stem, other files (images) by full filename.
        let link_name = if path.extension().and_then(|e| e.to_str()) == Some("md") {
            new_path
                .file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            new_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        };
        let mut rewrites: Vec<(PathBuf, crate::vault::TextEdits)> = Vec::new();
        // When the file changed directories, dir-prefixed and
        // relative links need the new root-relative path, not a
        // basename swap.
        let moved = new_path.parent() != path.parent();
        if path.is_file() {
            let vault = self.vault.read(cx);
            let file_name = new_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let link_rel = new_path
                .strip_prefix(vault.root.as_deref().unwrap_or(std::path::Path::new("")))
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let wiki_rel = link_rel.trim_end_matches(".md").to_string();
            for note in &vault.notes {
                let text = match self.docs.iter().find(|d| d.entity.read(cx).path == *note) {
                    Some(doc) => doc.entity.read(cx).editor.read(cx).value().to_string(),
                    None => match std::fs::read_to_string(note) {
                        Ok(text) => text,
                        Err(_) => continue,
                    },
                };
                let mut edits: Vec<_> = vault
                    .link_spans_to(&text, &path)
                    .into_iter()
                    .map(|range| {
                        (
                            range.clone(),
                            if moved {
                                crate::vault::retarget_link_full(&text[range], &wiki_rel)
                            } else {
                                crate::vault::retarget_link(&text[range], &link_name)
                            },
                        )
                    })
                    .collect();
                // Markdown `[label](path)` links too — resolved against
                // the note's own dir, then the vault root.
                if let Some(from_dir) = note.parent() {
                    edits.extend(
                        vault
                            .md_link_spans_to(&text, from_dir, &path)
                            .into_iter()
                            .map(|range| {
                                (
                                    range.clone(),
                                    if moved {
                                        crate::vault::retarget_md_link_full(&text[range], &link_rel)
                                    } else {
                                        crate::vault::retarget_md_link(&text[range], &file_name)
                                    },
                                )
                            }),
                    );
                    edits.sort_by_key(|(range, _)| range.start);
                }
                if !edits.is_empty() {
                    rewrites.push((note.clone(), edits));
                }
            }
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
            let mut updated = 0;
            for (note, edits) in rewrites {
                updated += edits.len();
                // The renamed file itself lives at `new_path` now.
                let target = if note == path { &new_path } else { &note };
                if let Some(doc) = self.docs.iter().find(|d| d.entity.read(cx).path == *target) {
                    doc.entity
                        .update(cx, |doc, cx| doc.apply_text_edits(edits, window, cx));
                } else if let Ok(mut text) = std::fs::read_to_string(target) {
                    for (range, rep) in edits.into_iter().rev() {
                        text.replace_range(range, &rep);
                    }
                    let _ = std::fs::write(target, text);
                }
            }
            // Starred + recent entries point at the old path too.
            let old_s = path.to_string_lossy().to_string();
            let new_s = new_path.to_string_lossy().to_string();
            let mut touched = false;
            for s in self
                .settings
                .starred
                .iter_mut()
                .chain(self.settings.pinned_tabs.iter_mut())
            {
                if *s == old_s {
                    *s = new_s.clone();
                    touched = true;
                }
            }
            if touched {
                self.settings.save();
            }
            for p in &mut self.recent {
                if *p == path {
                    *p = new_path.clone();
                }
            }
            // A move can leave the source folder(s) empty — remove them,
            // climbing toward the vault root (never past it).
            if moved {
                let root = self.vault.read(cx).root.clone();
                let mut dir = path.parent();
                while let Some(d) = dir {
                    if Some(d) == root.as_deref() || std::fs::remove_dir(d).is_err() {
                        break;
                    }
                    dir = d.parent();
                }
            }
            self.note_status(
                if updated > 0 {
                    format!(
                        "Renamed — {updated} link{} updated",
                        if updated == 1 { "" } else { "s" }
                    )
                } else {
                    "Renamed".to_string()
                },
                cx,
            );
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
        }
        let _ = window;
    }

    /// Edit `prop` on the active document — the preview's Properties
    /// strip calls through here so the write lands in the open doc.
    pub fn edit_active_property(
        &mut self,
        prop: String,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.active_doc().map(|d| d.read(cx).path.clone()) else {
            return;
        };
        self.edit_note_property(path, prop, current, window, cx);
    }

    /// `true`/`false` properties toggle in place like the Properties checkbox; everything else opens the text dialog.
    pub fn edit_note_property(
        &mut self,
        path: PathBuf,
        prop: String,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match current.trim() {
            "true" => {
                self.set_note_property(path, &prop, serde_yaml::Value::Bool(false), window, cx)
            }
            "false" => {
                self.set_note_property(path, &prop, serde_yaml::Value::Bool(true), window, cx)
            }
            _ if is_iso_date(&current).is_some() => {
                self.show_date_dialog(path, prop, current, window, cx)
            }
            _ => self.show_cell_dialog(path, prop, current, window, cx),
        }
    }

    /// Date-valued properties open a calendar popover instead of the
    /// text dialog — the Properties panel does the same. A pick
    /// writes `YYYY-MM-DD` back and closes.
    pub fn show_date_dialog(
        &mut self,
        path: PathBuf,
        prop: String,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(date) = is_iso_date(&current) else {
            return;
        };
        let date_state = cx.new(|cx| DatePickerState::new(window, cx));
        date_state.update(cx, |s, cx| s.set_date(date, window, cx));
        self.date_sub = Some(cx.subscribe_in(&date_state, window, {
            let prop = prop.clone();
            move |this, _state, ev: &DatePickerEvent, window, cx| {
                if let DatePickerEvent::Change(DateTime::Single(Some(dt))) = ev {
                    this.set_note_property(
                        path.clone(),
                        &prop,
                        serde_yaml::Value::String(dt.date().format("%Y-%m-%d").to_string()),
                        window,
                        cx,
                    );
                    window.close_dialog(cx);
                }
            }
        }));
        let title = format!("Edit {prop}");
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(title.clone())
                .w(px(360.))
                .child(div().w_full().child(DatePicker::new(&date_state)))
        });
    }

    /// Edit one frontmatter property of `path` — opened from a base
    /// table cell. The current display text prefills the input.
    pub fn show_cell_dialog(
        &mut self,
        path: PathBuf,
        prop: String,
        current: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.cell_input.clone();
        input.update(cx, |input, cx| input.set_value(current, window, cx));
        let view = cx.entity();
        let title = format!("Edit {prop}");

        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(title.clone())
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .on_ok({
                    let view = view.clone();
                    let path = path.clone();
                    let prop = prop.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.commit_cell_edit(path.clone(), prop.clone(), window, cx);
                            this.refocus(window, cx);
                        });
                        true
                    }
                })
        });

        let input = self.cell_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        });
    }

    fn commit_cell_edit(
        &mut self,
        path: PathBuf,
        prop: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.cell_input.read(cx).value().to_string();
        let text = text.trim();
        // Parse as YAML so `2`, `true`, `[a, b]` land with their real
        // types; anything else (incl. `a, b`) stays a plain string.
        // Empty input clears the property.
        let value = serde_yaml::from_str::<serde_yaml::Value>(text)
            .unwrap_or_else(|_| serde_yaml::Value::String(text.to_string()));
        self.set_note_property(path, &prop, value, window, cx);
    }

    /// Tags pane context menu → "Rename tag…" dialog.
    pub fn show_rename_tag(&mut self, tag: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.tag_input.clone();
        input.update(cx, |input, cx| input.set_value(tag.clone(), window, cx));
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(format!("Rename tag #{tag}"))
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .on_ok({
                    let view = view.clone();
                    let tag = tag.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.commit_tag_rename(&tag, window, cx);
                            this.refocus(window, cx);
                        });
                        true
                    }
                })
        });
        let input = self.tag_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        });
    }

    /// Rename `old` → the dialog's tag value across the vault: inline
    /// `#tag`/`#tag/sub` and `tags:` frontmatter entries alike, nested
    /// `old/x` → `new/x`. Open docs splice through the editor; closed
    /// notes write direct — same path as note renames.
    fn commit_tag_rename(&mut self, old: &str, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.tag_input.read(cx).value().to_string();
        let new = raw.trim().trim_start_matches('#').to_string();
        let valid = !new.is_empty()
            && !new.ends_with('/')
            && new
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
            && new
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/');
        if !valid || new == old {
            self.note_status("Invalid tag name", cx);
            return;
        }
        let notes = self.vault.read(cx).notes.clone();
        let mut touched = 0usize;
        for note in notes {
            if note.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let open = self
                .docs
                .iter()
                .find(|d| d.entity.read(cx).path == note)
                .map(|d| d.entity.clone());
            let text = match &open {
                Some(doc) => doc.read(cx).editor.read(cx).value().to_string(),
                None => match std::fs::read_to_string(&note) {
                    Ok(text) => text,
                    Err(_) => continue,
                },
            };
            let edits = crate::properties::tag_rename_edits(&text, old, &new);
            if edits.is_empty() {
                continue;
            }
            touched += 1;
            if let Some(doc) = open {
                doc.update(cx, |doc, cx| doc.apply_text_edits(edits, window, cx));
            } else {
                let mut text = text;
                for (range, rep) in edits.into_iter().rev() {
                    text.replace_range(range, &rep);
                }
                let _ = std::fs::write(&note, text);
            }
        }
        if touched == 0 {
            self.note_status(format!("No #{old} found"), cx);
        } else {
            self.note_status(format!("Renamed #{old} → #{new} in {touched} notes"), cx);
        }
    }

    /// Edit cell `col` of the markdown table row on `line` (1-based) —
    /// the preview's table cells call through here. The write lands in
    /// the open doc's editor, so undo and autosave cover it.
    pub fn show_table_cell_dialog(
        &mut self,
        line: usize,
        col: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        let Some(current) = doc.update(cx, |doc, cx| doc.table_cell_text(line, col, cx)) else {
            return;
        };
        let input = self.cell_input.clone();
        input.update(cx, |input, cx| input.set_value(current, window, cx));
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title("Edit cell")
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .on_ok({
                    let view = view.clone();
                    let doc = doc.clone();
                    move |_, window, cx| {
                        let text = view.read(cx).cell_input.read(cx).value().to_string();
                        doc.update(cx, |doc, cx| {
                            doc.set_table_cell(line, col, &text, window, cx)
                        });
                        view.update(cx, |this, cx| this.refocus(window, cx));
                        true
                    }
                })
        });

        let input = self.cell_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        });
    }

    /// Rendered table's "+ New row" — splices `|  |  |…` after `line`.
    pub fn add_table_row(
        &mut self,
        line: usize,
        cols: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |d, cx| d.add_table_row(line, cols, window, cx));
        }
    }

    /// Rendered table's header "+" — appends an empty column over
    /// `start..=end` (`start + 1` is the GFM separator).
    pub fn add_table_col(
        &mut self,
        start: usize,
        end: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |d, cx| d.add_table_col(start, end, window, cx));
        }
    }

    /// The Properties strip's "+ Add property" footer — name + value
    /// on the active document. Same YAML-typed commit as cell edits.
    pub fn show_add_property_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_doc().is_none() {
            return;
        }
        let name_input = self.prop_name_input.clone();
        let value_input = self.cell_input.clone();
        name_input.update(cx, |i, cx| i.set_value("", window, cx));
        value_input.update(cx, |i, cx| i.set_value("", window, cx));
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title("Add property")
                .w(px(400.))
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .w_full()
                                .child(Input::new(&name_input).appearance(true)),
                        )
                        .child(
                            div()
                                .w_full()
                                .child(Input::new(&value_input).appearance(true)),
                        ),
                )
                .on_ok({
                    let view = view.clone();
                    move |_, window, cx| {
                        view.update(cx, |this, cx| {
                            this.commit_add_property(window, cx);
                            this.refocus(window, cx);
                        });
                        true
                    }
                })
        });

        let name_input = self.prop_name_input.clone();
        window.defer(cx, move |window, cx| {
            name_input.update(cx, |input, cx| input.focus(window, cx));
        });
    }

    fn commit_add_property(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.prop_name_input.read(cx).value().trim().to_string();
        if name.is_empty()
            || name
                .chars()
                .any(|c| c == ':' || c == '\n' || c == '[' || c == ']')
        {
            self.note_status("Property needs a name", cx);
            return;
        }
        let text = self.cell_input.read(cx).value().to_string();
        let text = text.trim();
        // Empty writes a blank `key:` value — `body_with` treats
        // YAML-null as "remove", which would make "add" a no-op.
        let value = if text.is_empty() {
            serde_yaml::Value::String(String::new())
        } else {
            serde_yaml::from_str::<serde_yaml::Value>(text)
                .unwrap_or_else(|_| serde_yaml::Value::String(text.to_string()))
        };
        let Some(path) = self.active_doc().map(|d| d.read(cx).path.clone()) else {
            return;
        };
        self.set_note_property(path, &name, value, window, cx);
    }

    /// Click a Properties row's type glyph — the property type
    /// picker: coerce the value to the chosen type and write it back.
    pub fn show_property_type_picker(
        &mut self,
        key: String,
        edit: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_doc().is_none() {
            return;
        }
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let theme = _cx.theme();
            let mut list = v_flex().w_full().py_1();
            for (tix, (label, kind, icon)) in [
                ("Text", "text", assets::IconName::Type),
                ("List", "list", assets::IconName::List),
                ("Number", "number", assets::IconName::Hash),
                ("Checkbox", "checkbox", assets::IconName::SquareCheck),
                ("Date", "date", assets::IconName::Calendar),
                ("Time", "time", assets::IconName::Clock),
            ]
            .into_iter()
            .enumerate()
            {
                let view = view.clone();
                let key = key.clone();
                let edit = edit.clone();
                list = list.child(
                    div()
                        .id(("prop-type", tix))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Icon::new(icon)
                                        .size(px(13.))
                                        .text_color(theme.muted_foreground),
                                )
                                .child(div().text_sm().text_color(theme.foreground).child(label)),
                        )
                        .on_click(move |_, window, cx| {
                            let value = coerce_property_value(&edit, kind);
                            view.update(cx, |this, cx| {
                                if let Some(path) =
                                    this.active_doc().map(|d| d.read(cx).path.clone())
                                {
                                    this.set_note_property(path, &key, value, window, cx);
                                }
                                this.refocus(window, cx);
                            });
                            window.close_dialog(cx);
                        }),
                );
            }
            dialog
                .title(format!("{key} — type"))
                .w(px(260.))
                .overlay_closable(true)
                .child(list)
        });
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
                            this.delete_path(path.clone(), window, cx);
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
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        match crate::explorer::move_entry(&root, &src, &dest) {
            Ok(()) => {
                self.repoint_moved_entry(&src, &dest, cx);
                self.move_undo.push((src, dest));
                self.note_status(
                    self.tr(
                        "Moved · Undo available in explorer",
                        "Flyttet · Angre er tilgjengelig i filutforskeren",
                    ),
                    cx,
                );
            }
            Err(_) => {
                self.note_status(
                    self.tr(
                        "Could not move. Check the destination and permissions.",
                        "Kunne ikke flytte. Kontroller målmappen og tilganger.",
                    ),
                    cx,
                );
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
    /// the backlink panel as a dialog.
    fn show_backlinks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc_path) = self.active_doc().map(|d| d.read(cx).path.clone()) else {
            return;
        };
        let (links, root) = {
            let vault = self.vault.read(cx);
            (
                vault.backlinks_to(&doc_path),
                vault.root.clone().unwrap_or_default(),
            )
        };
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
                        .on_click(move |ev, window, cx| {
                            let workspace = workspace.clone();
                            let open = open.clone();
                            let new_tab = ev.modifiers().platform;
                            window.close_dialog(cx);
                            window.defer(cx, move |window, cx| {
                                workspace.update(cx, |ws, cx| {
                                    if new_tab {
                                        ws.open_document_new_tab(open, window, cx)
                                    } else {
                                        ws.open_document_pub(open, window, cx)
                                    }
                                });
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

    /// Backlink paths for the active document — shared by the
    /// backlinks dialog and the sidebar pane.
    fn backlinks(&self, cx: &App) -> Vec<PathBuf> {
        self.active_doc()
            .map(|d| self.vault.read(cx).backlinks_to(&d.read(cx).path))
            .unwrap_or_default()
    }

    /// Unlinked mentions of the active document (plain-text title
    /// occurrences) — listed under the linked mentions.
    fn unlinked(&self, cx: &App) -> Vec<PathBuf> {
        self.active_doc()
            .map(|d| self.vault.read(cx).unlinked_mentions(&d.read(cx).path))
            .unwrap_or_default()
    }

    /// Convert the first plain-text mention of the active note's
    /// title inside `note` into a `[[wikilink]]` — the "Link"
    /// action on unlinked mentions. Splices through the editor when
    /// the note is open, writes the file otherwise.
    fn link_up_mention(&mut self, note: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stem) = self
            .active_doc()
            .and_then(|d| d.read(cx).path.file_stem().and_then(|s| s.to_str()))
            .map(|s| s.to_string())
        else {
            return;
        };
        let needle = stem.to_lowercase();
        if let Some(doc) = self
            .docs
            .iter()
            .find(|d| d.entity.read(cx).path == note)
            .map(|d| d.entity.clone())
        {
            doc.update(cx, |doc, cx| {
                let text = doc.editor.read(cx).value().to_string();
                if let Some(start) = crate::vault::plain_mention_offset(&text, &needle) {
                    doc.editor.update(cx, |editor, cx| {
                        editor.set_selected_range(start..start + stem.len(), cx);
                        editor.replace(format!("[[{stem}]]"), window, cx);
                    });
                }
            });
        } else if let Ok(text) = std::fs::read_to_string(&note) {
            if let Some(start) = crate::vault::plain_mention_offset(&text, &needle) {
                let mut text = text;
                text.replace_range(start..start + stem.len(), &format!("[[{stem}]]"));
                if std::fs::write(&note, text).is_err() {
                    self.note_status("Couldn't update the mention", cx);
                    return;
                }
            }
        }
        cx.notify();
    }

    /// Heading navigator for the active note — picks a heading, jumps
    /// the editor caret to its line (fences skipped so `#` inside code
    /// blocks doesn't list).
    /// (line 1-based, level, text) for the active document, skipping
    /// fenced code — shared by the outline dialog and sidebar pane.
    fn doc_headings(&self, cx: &App) -> Vec<(usize, usize, String)> {
        let Some(doc) = self.active_doc() else {
            return Vec::new();
        };
        let raw = doc.read(cx).editor.read(cx).value().to_string();
        crate::preview::headings(&raw)
    }

    fn show_outline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            self.note_status("Open a note first", cx);
            return;
        };
        let headings = self.doc_headings(cx);
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
        let files = template_files(&root, &self.settings.templates_dir);
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

    fn delete_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
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
            self.close_tab_now(ix, window, cx);
        }
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
    }

    /// Public hook used by preview plugins (wikilinks, transclusion
    /// fallback links) and ⌥⏎. the create-on-click: an
    /// unresolved `[[link]]` becomes a new note; `[[note#heading]]`
    /// jumps the caret to the heading (Source view); `[[#heading]]`
    /// jumps within the open note.
    pub fn open_wikilink(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_wikilink_impl(target, false, window, cx);
    }

    /// ⌘+click: open the wikilink target in a new tab.
    pub fn open_wikilink_new_tab(
        &mut self,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_wikilink_impl(target, true, window, cx);
    }

    fn open_wikilink_impl(
        &mut self,
        target: &str,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (note, anchor) = match target.split_once('#') {
            Some((n, a)) => {
                let a = a.trim();
                (n.trim(), (!a.is_empty()).then(|| a.to_string()))
            }
            None => (target.trim(), None),
        };
        // `|` aliases arrive via ⌥⏎ on the raw `[[inner]]` — the alias
        // is display text, never part of the target.
        let note = note.split('|').next().unwrap_or(note).trim();
        let resolved = if note.is_empty() {
            // `[[#heading]]` — an anchor inside the open note.
            self.active_doc().map(|d| d.read(cx).path.clone())
        } else {
            self.vault.read(cx).resolve_wikilink(note)
        };
        match resolved {
            Some(path) => {
                self.open_document_impl(path, new_tab, false, window, cx);
                if let Some(anchor) = anchor {
                    self.jump_to_anchor(&anchor, window, cx);
                }
            }
            None => {
                if note.is_empty() {
                    return;
                }
                self.create_note_for_wikilink(note, window, cx);
            }
        }
    }

    /// Move the caret to the heading line matching an the reference editor anchor
    /// (`[[note#heading]]`) — case-insensitive match on the text after
    /// the `#`s, fences skipped.
    fn jump_to_anchor(&mut self, anchor: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        let raw = doc.read(cx).editor.read(cx).value().to_string();
        // `[[note#^block-id]]` — the line whose trailing `^id` matches.
        if let Some(block) = anchor.strip_prefix('^') {
            for (ix, line) in raw.split('\n').enumerate() {
                if line.trim_end().ends_with(&format!("^{block}")) {
                    doc.update(cx, |doc, cx| doc.jump_to_line(ix + 1, window, cx));
                    return;
                }
            }
            self.note_status(format!("No block “{}”", anchor), cx);
            return;
        }
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
            if !(1..=6).contains(&level) || trimmed.chars().nth(level) != Some(' ') {
                continue;
            }
            if trimmed[level + 1..].trim().eq_ignore_ascii_case(anchor) {
                doc.update(cx, |doc, cx| doc.jump_to_line(ix + 1, window, cx));
                return;
            }
        }
        self.note_status(format!("No heading “{}”", anchor), cx);
    }

    /// Create an empty note for an unresolved `[[link]]` — alongside the
    /// open document, or vault-root-relative when the target carries a
    /// `dir/name` path — then open it.
    fn create_note_for_wikilink(
        &mut self,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stem = target.trim().trim_end_matches(".md").replace('\\', "/");
        let stem = stem.trim();
        let bad_name = stem.is_empty()
            || stem.starts_with('/')
            || stem.contains("..")
            || stem
                .chars()
                .any(|c| matches!(c, ':' | '<' | '>' | '"' | '|' | '?' | '*'));
        if bad_name {
            self.note_status(format!("No note named “{}”", target), cx);
            return;
        }
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let dir = if stem.contains('/') {
            root.clone()
        } else {
            self.active_doc()
                .and_then(|d| d.read(cx).path.parent().map(|p| p.to_path_buf()))
                .filter(|p| p.starts_with(&root))
                .unwrap_or_else(|| root.clone())
        };
        let path = dir.join(format!("{stem}.md"));
        if path.exists() {
            // On disk but not indexed yet — just open it.
            self.open_document(path, window, cx);
            return;
        }
        let ok = path
            .parent()
            .map(|p| std::fs::create_dir_all(p).is_ok())
            .unwrap_or(false)
            && std::fs::write(&path, b"").is_ok();
        if !ok {
            self.note_status(format!("Couldn't create “{}”", target), cx);
            return;
        }
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        self.note_status(
            format!(
                "Created {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            cx,
        );
        self.open_document(path, window, cx);
    }

    /// Public hook used by the project-search view.
    /// Set one frontmatter property on a note — through the editor when
    /// the note is open (undo-able, autosaved), straight to disk
    /// otherwise so base views can write back.
    pub fn set_note_property(
        &mut self,
        path: PathBuf,
        key: &str,
        value: serde_yaml::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self
            .docs
            .iter()
            .find(|d| d.entity.read(cx).path == path)
            .map(|d| d.entity.clone())
        {
            doc.update(cx, |doc, cx| {
                let text = doc.editor.read(cx).value().to_string();
                let body = crate::properties::body_with(&text, key, value);
                doc.set_properties(body, window, cx);
            });
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            let body = crate::properties::body_with(&text, key, value);
            if std::fs::write(&path, crate::properties::splice_frontmatter(&text, body)).is_ok() {
                self.note_status(format!("Set {key}"), cx);
            }
        }
    }

    pub fn open_document_pub(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_document(path, window, cx);
    }

    /// ⌘+click on a row link: always open in a new tab.
    pub fn open_document_new_tab(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_document_impl(path, true, false, window, cx);
    }

    /// Called by BaseView when its view tab changes — remembered per
    /// file so a `.base` reopens on the view it was left on.
    pub fn remember_base_view(&mut self, path: String, view_ix: usize) {
        self.settings.base_views.insert(path, view_ix);
        self.settings.save();
    }

    /// Interactive header sort per `.base` view — saved under
    /// `{path}::{view_name}` so a column name survives view reorders.
    pub fn remember_base_sort(&mut self, path: String, view: String, col: Option<(String, bool)>) {
        let key = format!("{path}::{view}");
        if let Some(col) = col {
            self.settings.base_sorts.insert(key, col);
        } else {
            self.settings.base_sorts.remove(&key);
        }
        self.settings.save();
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
            .border_0()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("toggle-sidebar")
                            .ghost()
                            .small()
                            .tooltip("Toggle sidebar")
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
                    // Inset the right edge — the platform titlebar only
                    // pads the left (traffic lights), so icons here sat
                    // flush against the window corner.
                    .pr_3()
                    .child(
                        Button::new("toggle-inspector")
                            .ghost()
                            .small()
                            .tooltip(self.tr("Toggle inspector", "Vis/skjul inspektør"))
                            .icon(assets::IconName::TableProperties)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.inspector_open = !this.settings.inspector_open;
                                this.settings.save();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("palette")
                            .ghost()
                            .small()
                            .tooltip("Search commands and notes")
                            .icon(assets::IconName::Search)
                            .label("⌘K")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_palette(window, cx);
                            })),
                    )
                    .child(
                        Button::new("theme")
                            .ghost()
                            .small()
                            .tooltip("Toggle appearance")
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
                            .small()
                            .tooltip("Settings")
                            .icon(assets::IconName::Settings)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_open_settings(&OpenSettings, window, cx);
                            })),
                    ),
            )
    }

    /// The Starred group pinned at the top of the sidebar — collapsible,
    /// rows open their note, unstar via the tree's context menu below.
    fn render_starred(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let root = self.vault.read(cx).root.clone().unwrap_or_default();
        let rows: Vec<PathBuf> = self
            .settings
            .starred
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();

        v_flex()
            .w_full()
            .child(
                div()
                    .id("starred-toggle")
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Icon::new(if self.starred_open {
                                    assets::IconName::ChevronDown
                                } else {
                                    assets::IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("Starred · {}", rows.len())),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.starred_open = !this.starred_open;
                        this.settings.panes.starred = this.starred_open;
                        this.settings.save();
                        cx.notify();
                    })),
            )
            .when(self.starred_open, |this| {
                this.children(rows.iter().enumerate().map(|(ix, path)| {
                    let name = path
                        .file_stem()
                        .and_then(|f| f.to_str())
                        .unwrap_or_default()
                        .to_string();
                    let dir = path
                        .parent()
                        .and_then(|p| p.strip_prefix(&root).ok())
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let open_path = path.clone();
                    div()
                        .id(("starred-row", ix))
                        .w_full()
                        .px_2()
                        .py_0p5()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(
                                    Icon::new(assets::IconName::StarFill)
                                        .size_4()
                                        .text_color(theme.info),
                                )
                                .child(div().text_sm().truncate().child(name))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .truncate()
                                        .child(dir),
                                ),
                        )
                        .hover(|s| s.bg(theme.muted.opacity(0.5)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_document(open_path.clone(), window, cx);
                        }))
                }))
            })
            .when(self.starred_open && rows.is_empty(), |this| {
                this.child(
                    div().w_full().px_2().pb_1().child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("No starred notes — star one from its context menu"),
                    ),
                )
            })
    }

    /// Sidebar outline of the active note's headings — click jumps the
    /// caret to that line, like the palette outline but always visible.
    fn render_outline(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let headings = self.doc_headings(cx);

        v_flex()
            .w_full()
            .child(
                div()
                    .id("outline-toggle")
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Icon::new(if self.outline_open {
                                    assets::IconName::ChevronDown
                                } else {
                                    assets::IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("Outline · {}", headings.len())),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.outline_open = !this.outline_open;
                        this.settings.panes.outline = this.outline_open;
                        this.settings.save();
                        cx.notify();
                    })),
            )
            .when(self.outline_open, |this| {
                let rows = v_flex().w_full().children(headings.iter().enumerate().map(
                    |(ix, (line, level, text))| {
                        let (line, level, text) = (*line, *level, text.clone());
                        div()
                            .id(("outline-side", ix))
                            .w_full()
                            .px_2()
                            .pl(px(8. + 10. * (level as f32 - 1.)))
                            .py_0p5()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted.opacity(0.5)))
                            .child(
                                div()
                                    .text_sm()
                                    .truncate()
                                    .text_color(if level == 1 {
                                        theme.foreground
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .child(text),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(doc) = this.active_doc().cloned() {
                                    doc.update(cx, |doc, cx| doc.jump_to_line(line, window, cx));
                                }
                            }))
                    },
                ));
                this.child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        rows.max_h(px(160.)),
                    ),
                )
            })
            .when(self.outline_open && headings.is_empty(), |this| {
                this.child(
                    div().w_full().px_2().pb_1().child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("No headings in this note"),
                    ),
                )
            })
    }

    /// Notes that link to the active document — the linked-
    /// mentions pane, pinned in the sidebar. Click opens the note.
    /// Hover rows get the same peek card as the tree and .base rows.
    fn render_backlinks_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws_entity = cx.entity().downgrade();
        let theme = cx.theme();
        let links = self.backlinks(cx);
        let root = self.vault.read(cx).root.clone().unwrap_or_default();

        v_flex()
            .w_full()
            .child(
                div()
                    .id("backlinks-toggle")
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Icon::new(if self.backlinks_open {
                                    assets::IconName::ChevronDown
                                } else {
                                    assets::IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("Linked mentions · {}", links.len())),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.backlinks_open = !this.backlinks_open;
                        this.settings.panes.backlinks = this.backlinks_open;
                        this.settings.save();
                        cx.notify();
                    })),
            )
            .when(self.backlinks_open, |this| {
                let unlinked = self.unlinked(cx);
                let active_path = self.active_doc().map(|d| d.read(cx).path.clone());
                let mut rows = v_flex().w_full();
                if links.is_empty() && unlinked.is_empty() {
                    return this.child(
                        div().w_full().px_2().pb_1().child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child("No mentions of this note yet"),
                        ),
                    );
                }
                for (ix, path) in links.iter().enumerate() {
                    let rel = path
                        .strip_prefix(&root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| path.to_string_lossy().to_string());
                    // First line containing the link — the reference editor shows
                    // the mention's context under each backlink.
                    let snippet = active_path
                        .as_ref()
                        .and_then(|target| self.vault.read(cx).backlink_context(path, target));
                    let open = path.clone();
                    rows = rows.child(
                        div()
                            .id(("backlink-side", ix))
                            .w_full()
                            .px_2()
                            .py_0p5()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted.opacity(0.5)))
                            .child(
                                v_flex()
                                    .w_full()
                                    .child(div().text_sm().truncate().child(rel))
                                    .when_some(snippet, |row, line| {
                                        row.child(
                                            div()
                                                .pl_2()
                                                .text_xs()
                                                .truncate()
                                                .text_color(theme.muted_foreground)
                                                .child(line),
                                        )
                                    }),
                            )
                            .on_click(cx.listener(
                                move |this, ev: &gpui::ClickEvent, window, cx| {
                                    if ev.modifiers().platform {
                                        this.open_document_new_tab(open.clone(), window, cx);
                                    } else {
                                        this.open_document_pub(open.clone(), window, cx);
                                    }
                                },
                            ))
                            .on_mouse_move({
                                let ws = ws_entity.clone();
                                let path = path.clone();
                                move |ev: &gpui::MouseMoveEvent, _window, cx| {
                                    let _ = ws.update(cx, |ws, cx| {
                                        ws.peek_at(
                                            crate::app::PeekKind::Note(path.clone()),
                                            ev.position,
                                            cx,
                                        )
                                    });
                                }
                            })
                            .on_hover({
                                let ws = ws_entity.clone();
                                let path = path.clone();
                                move |hovered: &bool, _window, cx| {
                                    if !*hovered {
                                        let _ = ws.update(cx, |ws, cx| {
                                            ws.hide_peek(
                                                &crate::app::PeekKind::Note(path.clone()),
                                                cx,
                                            )
                                        });
                                    }
                                }
                            }),
                    );
                }
                if !unlinked.is_empty() {
                    rows = rows.child(
                        div().w_full().px_2().pt_1().child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("Unlinked · {}", unlinked.len())),
                        ),
                    );
                    // The plain-text needle is the active note's stem —
                    // `unlinked_context` finds the line it's on.
                    let needle = active_path
                        .as_ref()
                        .and_then(|p| p.file_stem().and_then(|s| s.to_str()))
                        .map(|s| s.to_lowercase());
                    for (ix, path) in unlinked.iter().enumerate() {
                        let rel = path
                            .strip_prefix(&root)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| path.to_string_lossy().to_string());
                        let snippet = needle
                            .as_ref()
                            .and_then(|n| self.vault.read(cx).unlinked_context(path, n));
                        let open = path.clone();
                        let link_note = path.clone();
                        rows = rows.child(
                            div()
                                .id(("unlinked-side", ix))
                                .w_full()
                                .px_2()
                                .py_0p5()
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.muted.opacity(0.5)))
                                .child(
                                    v_flex()
                                        .w_full()
                                        .child(div().text_sm().truncate().child(rel))
                                        .when_some(snippet, |row, line| {
                                            row.child(
                                                div()
                                                    .pl_2()
                                                    .text_xs()
                                                    .truncate()
                                                    .text_color(theme.muted_foreground)
                                                    .child(line),
                                            )
                                        }),
                                )
                                .on_click(cx.listener(
                                    move |this, ev: &gpui::ClickEvent, window, cx| {
                                        if ev.modifiers().platform {
                                            this.open_document_new_tab(open.clone(), window, cx);
                                        } else {
                                            this.open_document_pub(open.clone(), window, cx);
                                        }
                                    },
                                ))
                                .on_mouse_move({
                                    let ws = ws_entity.clone();
                                    let path = path.clone();
                                    move |ev: &gpui::MouseMoveEvent, _window, cx| {
                                        let _ = ws.update(cx, |ws, cx| {
                                            ws.peek_at(
                                                crate::app::PeekKind::Note(path.clone()),
                                                ev.position,
                                                cx,
                                            )
                                        });
                                    }
                                })
                                .on_hover({
                                    let ws = ws_entity.clone();
                                    let path = path.clone();
                                    move |hovered: &bool, _window, cx| {
                                        if !*hovered {
                                            let _ = ws.update(cx, |ws, cx| {
                                                ws.hide_peek(
                                                    &crate::app::PeekKind::Note(path.clone()),
                                                    cx,
                                                )
                                            });
                                        }
                                    }
                                }),
                        );
                        rows = rows.child(
                            div()
                                .id(("unlinked-link", ix))
                                .w_full()
                                .pl(px(24.))
                                .pr_2()
                                .pb_0p5()
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.muted.opacity(0.5)))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.accent)
                                        .child("→ Link this mention"),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.link_up_mention(link_note.clone(), window, cx);
                                })),
                        );
                    }
                }
                this.child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        // Two-line rows (name + context snippet) need
                        // more than the old one-line 160px budget.
                        rows.max_h(px(240.)),
                    ),
                )
            })
    }

    /// Links the active note points at — the Outgoing Links
    /// pane. Resolved rows open the note; unresolvable `[[targets]]`
    /// list dimmed below. Refreshes on the preview debounce.
    fn render_outgoing_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws_entity = cx.entity().downgrade();
        let theme = cx.theme();
        let (links, unresolved) = self
            .active_doc()
            .map(|d| {
                let d = d.read(cx);
                (d.outgoing_links.clone(), d.outgoing_unresolved.clone())
            })
            .unwrap_or_default();
        let root = self.vault.read(cx).root.clone().unwrap_or_default();

        v_flex()
            .w_full()
            .child(
                div()
                    .id("outgoing-toggle")
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Icon::new(if self.outgoing_open {
                                    assets::IconName::ChevronDown
                                } else {
                                    assets::IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(theme.muted_foreground),
                            )
                            .child(div().text_xs().text_color(theme.muted_foreground).child(
                                format!(
                                    "{} · {}",
                                    self.tr("Outgoing links", "Utgående lenker"),
                                    links.len() + unresolved.len()
                                ),
                            )),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.outgoing_open = !this.outgoing_open;
                        this.settings.panes.outgoing = this.outgoing_open;
                        this.settings.save();
                        cx.notify();
                    })),
            )
            .when(self.outgoing_open, |this| {
                let mut rows = v_flex().w_full();
                if links.is_empty() && unresolved.is_empty() {
                    return this.child(
                        div().w_full().px_2().pb_1().child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(self.tr(
                                    "This note doesn't link anywhere",
                                    "Dette notatet har ingen lenker",
                                )),
                        ),
                    );
                }
                for (ix, path) in links.iter().enumerate() {
                    let rel = path
                        .strip_prefix(&root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| path.to_string_lossy().to_string());
                    let open = path.clone();
                    rows = rows.child(
                        div()
                            .id(("outgoing-side", ix))
                            .w_full()
                            .px_2()
                            .py_0p5()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted.opacity(0.5)))
                            .child(div().text_sm().truncate().child(rel))
                            .on_click(cx.listener(
                                move |this, ev: &gpui::ClickEvent, window, cx| {
                                    if ev.modifiers().platform {
                                        this.open_document_new_tab(open.clone(), window, cx);
                                    } else {
                                        this.open_document_pub(open.clone(), window, cx);
                                    }
                                },
                            ))
                            .on_mouse_move({
                                let ws = ws_entity.clone();
                                let path = path.clone();
                                move |ev: &gpui::MouseMoveEvent, _window, cx| {
                                    let _ = ws.update(cx, |ws, cx| {
                                        ws.peek_at(
                                            crate::app::PeekKind::Note(path.clone()),
                                            ev.position,
                                            cx,
                                        )
                                    });
                                }
                            })
                            .on_hover({
                                let ws = ws_entity.clone();
                                let path = path.clone();
                                move |hovered: &bool, _window, cx| {
                                    if !*hovered {
                                        let _ = ws.update(cx, |ws, cx| {
                                            ws.hide_peek(
                                                &crate::app::PeekKind::Note(path.clone()),
                                                cx,
                                            )
                                        });
                                    }
                                }
                            }),
                    );
                }
                for (ix, target) in unresolved.iter().enumerate() {
                    rows =
                        rows.child(
                            div()
                                .id(("outgoing-miss", ix))
                                .w_full()
                                .px_2()
                                .py_0p5()
                                .child(
                                    div().text_sm().truncate().text_color(theme.warning).child(
                                        format!("{}: {target}", self.tr("Missing", "Mangler")),
                                    ),
                                ),
                        );
                }
                this.child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        rows.max_h(px(160.)),
                    ),
                )
            })
    }

    /// Month-grid mini-calendar — the Calendar plugin. Days
    /// with a `YYYY-MM-DD.md` daily note render accent+bold; today is
    /// ringed; click opens (or templates) that day's note.
    fn render_calendar_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use chrono::Datelike;
        let theme = cx.theme();
        let (year, month) = self.cal_month;
        let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else {
            return v_flex();
        };
        let today = chrono::Local::now().date_naive();
        let days_in_month = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1)
        }
        .map(|d| d.pred_opt().map(|p| p.day()).unwrap_or(30))
        .unwrap_or(30);
        // Monday-first leading blanks.
        let lead = (first.weekday().num_days_from_monday()) as usize;
        // Which days have a daily note in the daily-note folder.
        let root = self.vault.read(cx).root.clone().unwrap_or_default();
        let daily_root = root.join(&self.settings.daily_dir);
        let have: std::collections::HashSet<String> = self
            .vault
            .read(cx)
            .notes
            .iter()
            .filter(|p| p.parent() == Some(daily_root.as_path()))
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .collect();
        let month_name = first.format("%B %Y").to_string();

        let day_cell = |day: u32| {
            let date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
            let stamp = date.format("%Y-%m-%d").to_string();
            let has_note = have.contains(&stamp);
            let is_today = date == today;
            div()
                .id(("cal-day", day as usize))
                .w(px(26.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .cursor_pointer()
                .text_xs()
                .text_color(if has_note {
                    theme.info
                } else {
                    theme.foreground
                })
                .when(has_note, |d| d.font_semibold())
                .when(is_today, |d| d.bg(theme.muted))
                .hover(|s| s.bg(theme.muted.opacity(0.5)))
                .child(day.to_string())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_daily_at(date, window, cx);
                }))
        };

        let mut weeks: Vec<Div> = Vec::new();
        let mut week = h_flex().gap_0p5();
        for _ in 0..lead {
            week = week.child(div().w(px(26.)).h(px(20.)));
        }
        let mut slot = lead;
        for day in 1..=days_in_month {
            week = week.child(day_cell(day));
            slot += 1;
            if slot.is_multiple_of(7) {
                weeks.push(week);
                week = h_flex().gap_0p5();
            }
        }
        if !slot.is_multiple_of(7) {
            weeks.push(week);
        }

        let nav = |icon: assets::IconName, delta: i32| {
            div()
                .id(("cal-nav", (delta + 1) as usize))
                .px_1()
                .cursor_pointer()
                .rounded(px(3.))
                .hover(|s| s.bg(theme.muted.opacity(0.5)))
                .child(Icon::new(icon).size_3().text_color(theme.muted_foreground))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let (mut y, mut m) = this.cal_month;
                    m = (m as i32 + delta) as u32;
                    if m == 0 {
                        m = 12;
                        y -= 1;
                    } else if m == 13 {
                        m = 1;
                        y += 1;
                    }
                    this.cal_month = (y, m);
                    cx.notify();
                }))
        };

        v_flex()
            .w_full()
            .child(
                div()
                    .id("cal-toggle")
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Icon::new(if self.cal_open {
                                    assets::IconName::ChevronDown
                                } else {
                                    assets::IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("Calendar · {month_name}")),
                            )
                            .child(nav(assets::IconName::ChevronLeft, -1))
                            .child(nav(assets::IconName::ChevronRight, 1)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cal_open = !this.cal_open;
                        this.settings.panes.calendar = this.cal_open;
                        this.settings.save();
                        cx.notify();
                    })),
            )
            .when(self.cal_open, |this| {
                let head =
                    h_flex()
                        .gap_0p5()
                        .children(["M", "T", "W", "T", "F", "S", "S"].iter().map(|d| {
                            div()
                                .w(px(26.))
                                .flex()
                                .justify_center()
                                .text_color(theme.muted_foreground)
                                .child(d.to_string())
                                .into_any_element()
                        }));
                this.child(
                    v_flex()
                        .px_2()
                        .pb_1p5()
                        .text_xs()
                        .child(head)
                        .children(weeks),
                )
            })
    }

    /// Open `- [ ]` checkboxes vault-wide — click opens the note at
    /// the task's line.
    fn render_tasks(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tasks = self.vault.read(cx).tasks.clone();

        v_flex()
            .w_full()
            .child(
                div()
                    .id("tasks-toggle")
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Icon::new(if self.tasks_open {
                                    assets::IconName::ChevronDown
                                } else {
                                    assets::IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("Tasks · {}", tasks.len())),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tasks_open = !this.tasks_open;
                        this.settings.panes.tasks = this.tasks_open;
                        this.settings.save();
                        cx.notify();
                    })),
            )
            .when(self.tasks_open, |this| {
                let rows =
                    v_flex()
                        .w_full()
                        .children(tasks.iter().enumerate().map(|(ix, task)| {
                            let path = task.path.clone();
                            let note = task
                                .path
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let line = task.line;
                            div()
                                .id(("task-row", ix))
                                .w_full()
                                .px_2()
                                .py_0p5()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .gap_1p5()
                                        .items_center()
                                        .child(
                                            div()
                                                .id(("task-check", ix))
                                                .cursor_pointer()
                                                .child(
                                                    Icon::new(assets::IconName::Square)
                                                        .size_3()
                                                        .text_color(theme.muted_foreground),
                                                )
                                                .on_click(cx.listener({
                                                    let path = path.clone();
                                                    move |this, _, window, cx| {
                                                        cx.stop_propagation();
                                                        this.complete_task(
                                                            path.clone(),
                                                            line,
                                                            window,
                                                            cx,
                                                        );
                                                    }
                                                })),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .text_sm()
                                                .truncate()
                                                .text_color(if task.text.is_empty() {
                                                    theme.muted_foreground
                                                } else {
                                                    theme.foreground
                                                })
                                                .child(if task.text.is_empty() {
                                                    "(empty task)".to_string()
                                                } else {
                                                    task.text.clone()
                                                }),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(theme.muted_foreground)
                                                .child(note),
                                        ),
                                )
                                .hover(|s| s.bg(theme.muted.opacity(0.5)))
                                .on_click(cx.listener(
                                    move |this, ev: &gpui::ClickEvent, window, cx| {
                                        if ev.modifiers().platform {
                                            this.open_document_new_tab(path.clone(), window, cx);
                                        } else {
                                            this.open_document(path.clone(), window, cx);
                                        }
                                        if let Some(doc) = this.active_doc() {
                                            let doc = doc.clone();
                                            doc.update(cx, |doc, cx| {
                                                doc.jump_to_line(line, window, cx)
                                            });
                                        }
                                    },
                                ))
                        }));
                this.child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        rows.max_h(px(200.)),
                    ),
                )
            })
            .when(self.tasks_open && tasks.is_empty(), |this| {
                this.child(
                    div().w_full().px_2().pb_1().child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("No open tasks"),
                    ),
                )
            })
    }

    /// Vault-wide tag index pinned at the bottom of the sidebar —
    /// the tag pane. Clicking a tag opens project search
    /// pre-filled with `#tag`.
    fn render_tags(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tags = self.vault.read(cx).tags.clone();

        v_flex()
            .w_full()
            .child(
                div()
                    .id("tags-toggle")
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(
                                Icon::new(if self.tags_open {
                                    assets::IconName::ChevronDown
                                } else {
                                    assets::IconName::ChevronRight
                                })
                                .size_4()
                                .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(format!("Tags · {}", tags.len())),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tags_open = !this.tags_open;
                        this.settings.panes.tags = this.tags_open;
                        this.settings.save();
                        cx.notify();
                    })),
            )
            .when(self.tags_open, |this| {
                // Nest `a/b` under `a` — parents aggregate descendant
                // counts and expand to show children (the tag
                // pane). A tag clicked at any level searches that path,
                // which already matches its nested tags.
                let mut tree: std::collections::BTreeMap<String, TagNode> = Default::default();
                for (tag, count) in &tags {
                    let segs: Vec<&str> = tag.split('/').collect();
                    tag_insert(&mut tree, &segs, *count);
                }
                // (depth, full path, segment, total count, has children,
                // expanded) — children of collapsed parents are skipped.
                let mut flat: Vec<(usize, String, String, usize, bool, bool)> = Vec::new();
                fn flatten(
                    nodes: &std::collections::BTreeMap<String, TagNode>,
                    depth: usize,
                    prefix: String,
                    expanded: &std::collections::BTreeSet<String>,
                    out: &mut Vec<(usize, String, String, usize, bool, bool)>,
                ) {
                    for (seg, node) in nodes {
                        let full = if prefix.is_empty() {
                            seg.clone()
                        } else {
                            format!("{prefix}/{seg}")
                        };
                        let has_kids = !node.kids.is_empty();
                        let open = expanded.contains(&full);
                        out.push((
                            depth,
                            full.clone(),
                            seg.clone(),
                            node.total(),
                            has_kids,
                            open,
                        ));
                        if open {
                            flatten(&node.kids, depth + 1, full, expanded, out);
                        }
                    }
                }
                flatten(&tree, 0, String::new(), &self.tags_expanded, &mut flat);

                let rows = v_flex().w_full().children(flat.into_iter().enumerate().map(
                    |(ix, (depth, full, seg, total, has_kids, open))| {
                        let query = format!("#{full}");
                        let full_for_toggle = full.clone();
                        div()
                            .id(("tag-row", ix))
                            .w_full()
                            .pr_2()
                            .pl(px(8. + depth as f32 * 14.))
                            .py_0p5()
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .items_center()
                                            .child(if has_kids {
                                                div()
                                                    .id(("tag-toggle", ix))
                                                    .cursor_pointer()
                                                    .child(
                                                        Icon::new(if open {
                                                            assets::IconName::ChevronDown
                                                        } else {
                                                            assets::IconName::ChevronRight
                                                        })
                                                        .size_3()
                                                        .text_color(theme.muted_foreground),
                                                    )
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        if open {
                                                            this.tags_expanded
                                                                .remove(&full_for_toggle);
                                                        } else {
                                                            this.tags_expanded
                                                                .insert(full_for_toggle.clone());
                                                        }
                                                        cx.notify();
                                                    }))
                                                    .into_any_element()
                                            } else {
                                                // Keep leaf labels aligned
                                                // with siblings' chevrons.
                                                div().w(px(12.)).into_any_element()
                                            })
                                            .child(
                                                div().text_sm().truncate().child(format!("#{seg}")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(total.to_string()),
                                    ),
                            )
                            .hover(|s| s.bg(theme.muted.opacity(0.5)))
                            .on_click(cx.listener(move |_, _, window, cx| {
                                crate::search::open_project_search_for(
                                    cx.entity(),
                                    Some(query.as_str()),
                                    window,
                                    cx,
                                );
                            }))
                            .context_menu({
                                let tag = full.clone();
                                let view = cx.entity();
                                move |menu, _window, _cx| {
                                    menu.item(
                                        PopupMenuItem::new("Rename tag…")
                                            .icon(assets::IconName::SquarePen)
                                            .on_click({
                                                let tag = tag.clone();
                                                let view = view.clone();
                                                move |_, window, cx| {
                                                    view.update(cx, |this, cx| {
                                                        this.show_rename_tag(
                                                            tag.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    });
                                                }
                                            }),
                                    )
                                }
                            })
                    },
                ));
                this.child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        rows.max_h(px(200.)),
                    ),
                )
            })
            .when(self.tags_open && tags.is_empty(), |this| {
                this.child(
                    div().w_full().px_2().pb_1().child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("No tags in this vault"),
                    ),
                )
            })
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let tree_state = self.vault.read(cx).tree.clone();
        let tree_scroll = tree_state.read(cx).scroll_handle().clone();

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .h_11()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .px_3()
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
                        Button::new("vault-home").ghost().xsmall()
                            .icon(assets::IconName::House)
                            .label(self.tr("Home", "Hjem"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(root) = this.vault.read(cx).root.clone() {
                                    this.open_folder_page(root, window, cx);
                                }
                            })),
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
                            )
                            .child(
                                Button::new("sort-files")
                                    .ghost()
                                    .xsmall()
                                    .icon(match self.settings.tree_sort {
                                        TreeSort::Name => assets::IconName::ListOrdered,
                                        TreeSort::Modified => assets::IconName::FileClock,
                                        TreeSort::Type => assets::IconName::File,
                                        TreeSort::Size => assets::IconName::ListOrdered,
                                    })
                                    .tooltip(match self.settings.tree_sort {
                                        TreeSort::Name => self.tr("Sorted by name", "Sortert etter navn"),
                                        TreeSort::Modified => self.tr("Sorted by modified", "Sortert etter endring"),
                                        TreeSort::Type => self.tr("Sorted by type", "Sortert etter filtype"),
                                        TreeSort::Size => self.tr("Sorted by size", "Sortert etter størrelse"),
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_tree_sort(cx);
                                    })),
                            ),
                    ),
            )
            .child(self.render_explorer_controls(cx))
            .child(self.render_starred(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(120.))
                    .child(
                        gpui_kit::base::Tree::new(&tree_state)
                            .item({
                                let render_view = view.clone();
                                let starred_rows = self.settings.starred.clone();
                                move |ix, entry, entry_state, _window, cx| {
                                    render_view.update(cx, |this, cx| {
                                        let item = entry.item();
                                        let path = PathBuf::from(item.id.as_str());
                                        let is_folder = path.is_dir();
                                        let is_file = !is_folder;
                                        let is_starred = is_file
                                            && starred_rows.iter().any(|s| s == item.id.as_str());
                                        // The open note stays lit even when the
                                        // tree's own selection moved (nav via
                                        // wikilinks, palette, tabs…).
                                        let is_active = this.folder.as_ref().is_some_and(|page| page.path == path)
                                            || (is_file && this
                                                .active_doc()
                                                .map(|d| d.read(cx).path == path)
                                                .unwrap_or(false));
                                        let icon: assets::IconName = if !is_file {
                                            if entry.is_expanded() {
                                                assets::IconName::FolderOpen
                                            } else {
                                                assets::IconName::FolderClosed
                                            }
                                        } else {
                                            match path
                                                .extension()
                                                .and_then(|e| e.to_str())
                                                .map(|e| e.to_lowercase())
                                                .as_deref()
                                            {
                                                Some("base") => assets::IconName::Database,
                                                Some(e) if IMAGE_EXTS.contains(&e) => {
                                                    assets::IconName::FileImage
                                                }
                                                _ => assets::IconName::FileText,
                                            }
                                        };
                                        let row = ListItem::new(ix)
                                            .w_full()
                                            .rounded(cx.theme().radius)
                                            .py_0p5()
                                            .px_2()
                                            .pl(px(16.) * entry.depth() + px(8.))
                                            .selected(entry_state.is_selected() || is_active)
                                            .secondary_selected(entry_state.is_right_clicked())
                                            .disabled(entry.is_disabled())
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div().w_3().flex_shrink_0().when(entry.is_folder(), |d| {
                                                            d.child(Icon::new(if entry.is_expanded() {
                                                                assets::IconName::ChevronDown
                                                            } else { assets::IconName::ChevronRight }).size_3())
                                                        })
                                                    )
                                                    .child(Icon::new(icon).size_4())
                                                    .child(
                                                        div()
                                                            .id(("folder-label", ix))
                                                            .flex_1()
                                                            .truncate()
                                                            .when(is_folder, |label| {
                                                                let path = path.clone();
                                                                label.cursor_pointer()
                                                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                                                    .on_click(cx.listener(move |this, _, window, cx| {
                                                                        cx.stop_propagation();
                                                                        this.open_folder_page(path.clone(), window, cx);
                                                                    }))
                                                            })
                                                            .child(item.label.clone()),
                                                    )
                                                    .when(is_starred, |h| {
                                                        h.child(
                                                            Icon::new(assets::IconName::StarFill)
                                                                .size_3()
                                                                .text_color(cx.theme().info),
                                                        )
                                                    }),
                                            )
                                            .on_click(cx.listener({
                                                let path = path.clone();
                                                move |this, ev: &gpui::ClickEvent, window, cx| {
                                                    if is_file {
                                                        if ev.modifiers().platform {
                                                            this.open_document_new_tab(
                                                                path.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        } else if ev.click_count() == 2 {
                                                            this.open_document(
                                                                path.clone(),
                                                                window,
                                                                cx,
                                                            );
                                                        } else {
                                                            this.preview_document(path.clone(), window, cx);
                                                        }
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
                                            })
                                            // Peek card on hover — same machinery
                                            // as .base rows and property chips.
                                            .on_mouse_move({
                                                let path = path.clone();
                                                let view = render_view.clone();
                                                move |ev: &gpui::MouseMoveEvent, _window, cx| {
                                                    if is_file {
                                                        view.update(cx, |ws, cx| {
                                                            ws.peek_at(
                                                                crate::app::PeekKind::Note(
                                                                    path.clone(),
                                                                ),
                                                                ev.position,
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                }
                                            })
                                            .on_hover({
                                                let path = path.clone();
                                                let view = render_view.clone();
                                                move |hovered: &bool, _window, cx| {
                                                    if is_file && !*hovered {
                                                        view.update(cx, |ws, cx| {
                                                            ws.hide_peek(
                                                                &crate::app::PeekKind::Note(
                                                                    path.clone(),
                                                                ),
                                                                cx,
                                                            )
                                                        });
                                                    }
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
                                        div()
                                            .id(SharedString::from(format!("file-row-{}", item.id)))
                                            .child(row)
                                            .on_mouse_down(gpui::MouseButton::Right, {
                                                let path = path.clone();
                                                let view = render_view.clone();
                                                move |event, window, cx| {
                                                    window.prevent_default();
                                                    view.update(cx, |this, cx| {
                                                        this.show_file_menu(
                                                            path.clone(),
                                                            is_folder,
                                                            event.position,
                                                            window,
                                                            cx,
                                                        );
                                                    });
                                                }
                                            })
                                            .into_any_element()
                                    })
                                }
                            })
                            .list_style(StyleRefinement::default().flex_grow_1().size_full())
                            .relative()
                            .size_full(),
                    )
                    .p_1()
                    .text_sm()
                    .vertical_scrollbar(&tree_scroll),
            )
    }

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Count same-name tabs first: when a title is duplicated, append
        // the parent folder so two `notes.md` tabs stay distinguishable.
        let mut title_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for doc in &self.docs {
            let doc = doc.entity.read(cx);
            let title = if doc.is_read_only()
                || doc.path.extension().and_then(|e| e.to_str()) == Some("base")
            {
                doc.file_name()
            } else {
                doc.title()
            };
            *title_counts.entry(title).or_default() += 1;
        }
        let tabs: Vec<Tab> = self
            .docs
            .iter()
            .enumerate()
            .map(|(ix, doc)| {
                let (title, dirty, icon) = {
                    let doc = doc.entity.read(cx);
                    // .base tabs keep their extension so `tasks` and
                    // `tasks.base` don't look like the same document.
                    let mut title = if doc.is_read_only()
                        || doc.path.extension().and_then(|e| e.to_str()) == Some("base")
                    {
                        doc.file_name()
                    } else {
                        doc.title()
                    };
                    if title_counts.get(&title).copied().unwrap_or(0) > 1 {
                        if let Some(dir) = doc
                            .path
                            .parent()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().to_string())
                        {
                            title = format!("{title} · {dir}");
                        }
                    }
                    let ext = doc
                        .path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase());
                    let icon = match ext.as_deref() {
                        Some("base") => assets::IconName::Database,
                        Some(e) if IMAGE_EXTS.contains(&e) => assets::IconName::FileImage,
                        _ => assets::IconName::FileText,
                    };
                    (title, doc.dirty, icon)
                };
                let view = cx.entity();
                let tab = Tab::new()
                    .when(doc.preview, |tab| tab.italic())
                    .label(if dirty {
                        format!("{} •", title.clone())
                    } else {
                        title.clone()
                    })
                    // Icon goes in `prefix`: the vendored Tab renders the
                    // `icon` slot INSTEAD of the label, not beside it.
                    .prefix(Icon::new(icon).size_3p5());
                let pinned = {
                    let doc = doc.entity.read(cx);
                    let s = doc.path.display().to_string();
                    self.settings.pinned_tabs.contains(&s)
                };
                let tab = if pinned {
                    // Pinned tabs swap the × for a pin glyph — no way to
                    // close them without unpinning first.
                    tab.suffix(
                        Icon::new(assets::IconName::Pin)
                            .size_3p5()
                            .text_color(cx.theme().muted_foreground),
                    )
                } else {
                    tab.suffix(
                        Button::new(("close-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Close)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab_at(ix, window, cx);
                            })),
                    )
                };
                tab.on_mouse_down(gpui::MouseButton::Left, {
                    let view = view.clone();
                    move |event, _, cx| {
                        if event.click_count == 2 {
                            view.update(cx, |this, cx| {
                                if let Some(tab) = this.docs.get_mut(ix) {
                                    tab.preview = false;
                                }
                                this.persist_tabs(cx);
                                cx.notify();
                            });
                        }
                    }
                })
                .on_drag(DraggedTab(ix), {
                    let label = title.clone();
                    move |_, _, _, cx| {
                        cx.new(|_| TreeDragPreview {
                            label: label.clone().into(),
                        })
                    }
                })
                .drag_over::<DraggedTab>(|style, _, _, cx| {
                    style.bg(cx.theme().accent.opacity(0.15))
                })
                .on_drop::<DraggedTab>({
                    let view = view.clone();
                    move |src, window, cx| {
                        view.update(cx, |this, cx| {
                            this.move_tab(src.0, ix, window, cx);
                        });
                    }
                })
                // Middle-click closes the tab (browser/the reference editor parity).
                .on_mouse_down(gpui::MouseButton::Middle, {
                    let view = view.clone();
                    move |_ev, window, cx| {
                        view.update(cx, |this, cx| {
                            this.close_tab_at(ix, window, cx);
                        });
                    }
                })
                // Right-click records which tab the strip-level context
                // menu should act on (the menu builder runs next frame).
                .on_mouse_down(gpui::MouseButton::Right, {
                    let view = view.clone();
                    move |_ev, _window, cx| {
                        view.update(cx, |this, _cx| {
                            this.tab_menu_ix = Some(ix);
                        });
                    }
                })
            })
            .collect();

        let can_back = self.nav_pos > 0;
        let can_fwd = self.nav_pos + 1 < self.nav_stack.len();
        let view = cx.entity();
        h_flex()
            .id("tab-strip")
            .w_full()
            .h_11()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            // Drop a file from the tree onto the strip to open it as a tab.
            .drag_over::<PathBuf>(|style, _, _, cx| style.bg(cx.theme().accent.opacity(0.15)))
            .on_drop::<PathBuf>({
                let view = view.clone();
                move |src, window, cx| {
                    let src = src.clone();
                    view.update(cx, |this, cx| {
                        this.open_document_new_tab(src, window, cx);
                    });
                }
            })
            .child(
                h_flex()
                    .pl_3()
                    .child(
                        Button::new("nav-back")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::ChevronLeft)
                            .disabled(!can_back)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.nav_back(window, cx);
                            })),
                    )
                    .child(
                        Button::new("nav-fwd")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::ChevronRight)
                            .disabled(!can_fwd)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.nav_forward(window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        TabBar::new("doc-tabs")
                            .segmented()
                            .small()
                            .bg(cx.theme().transparent)
                            .selected_index(self.active.unwrap_or(usize::MAX))
                            .children(tabs)
                            .on_click(cx.listener(|this, &ix, _window, cx| {
                                // The × suffix button closes a tab but its
                                // click still lands here — skip reselecting
                                // an index that no longer exists, which
                                // would blank the editor until another tab
                                // is clicked.
                                if ix < this.docs.len() {
                                    this.folder = None;
                                    this.graph = None;
                                    this.active = Some(ix);
                                    let path = this.docs[ix].entity.read(cx).path.clone();
                                    this.record_nav(&path);
                                    this.persist_tabs(cx);
                                    this.reveal_active_file(cx);
                                    cx.notify();
                                }
                            })),
                    )
                    // Right-click anywhere on the strip opens the tab menu
                    //. Each tab's Right mouse-down sets
                    // `tab_menu_ix` first — the menu builder runs deferred.
                    .context_menu({
                        let view = view.clone();
                        move |menu, _window, cx| {
                            let ws = view.read(cx);
                            let Some((ix, pinned, path)) =
                                ws.tab_menu_ix.or(ws.active).and_then(|ix| {
                                    ws.docs.get(ix).map(|d| {
                                        let p = d.entity.read(cx).path.clone();
                                        (ix, ws.is_pinned(&p), p)
                                    })
                                })
                            else {
                                return menu;
                            };
                            menu.item(
                                PopupMenuItem::new(if pinned { "Unpin tab" } else { "Pin tab" })
                                    .icon(assets::IconName::Pin)
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _w, cx| {
                                            view.update(cx, |this, cx| this.toggle_pin_at(ix, cx));
                                        }
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("Reveal in tree")
                                    .icon(assets::IconName::Crosshair)
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _w, cx| {
                                            view.update(cx, |this, cx| this.reveal_file(&path, cx));
                                        }
                                    }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("Close tab")
                                    .icon(assets::IconName::X)
                                    .on_click({
                                        let view = view.clone();
                                        move |_, w, cx| {
                                            view.update(cx, |this, cx| {
                                                this.close_tab_at(ix, w, cx)
                                            });
                                        }
                                    }),
                            )
                            .item(PopupMenuItem::new("Close other tabs").on_click({
                                let view = view.clone();
                                move |_, w, cx| {
                                    view.update(cx, |this, cx| this.close_others_except(ix, w, cx));
                                }
                            }))
                            .item(
                                PopupMenuItem::new("Close tabs to the right").on_click({
                                    let view = view.clone();
                                    move |_, w, cx| {
                                        view.update(cx, |this, cx| {
                                            this.close_tabs_right_of(ix, w, cx)
                                        });
                                    }
                                }),
                            )
                        }
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    // Same right inset as the titlebar cluster above.
                    .pr_3()
                    // Image tabs have no source/preview modes; an open
                    // graph replaces the content area entirely.
                    .when(
                        self.active.is_some()
                            && !self.active_doc_is_image(cx)
                            && self.graph.is_none(),
                        |this| this.child(self.render_view_mode_tabs(cx)),
                    ),
            )
    }

    /// Is the active doc an image file (rendered as a picture, not a
    /// text editor)? Hides the Source/Split/Preview switcher.
    fn active_doc_is_image(&self, cx: &App) -> bool {
        self.active_doc()
            .map(|d| d.read(cx).is_read_only())
            .unwrap_or(false)
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
        let (state, banner, folds, embeds, mentions, mentions_open, has_frontmatter, doc_path) = {
            let doc = doc.read(cx);
            (
                doc.preview.clone(),
                doc.banner.clone(),
                doc.callout_folds.clone(),
                doc.base_embeds.clone(),
                doc.linked_mentions.clone(),
                doc.mentions_open,
                crate::properties::frontmatter_span(doc.editor.read(cx).value().as_ref()).is_some(),
                doc.path.clone(),
            )
        };
        let base_ctx = preview::PreviewCtx {
            properties_visibility: self.settings.properties_visibility,
            language: self.settings.language,
            vault: self.vault.clone(),
            workspace: view.downgrade(),
            views: embeds,
            doc_path: Some(doc_path),
            depth: 0,
        };
        let theme = cx.theme();
        let footer_colors = (theme.foreground, theme.muted_foreground);
        v_flex()
            .size_full()
            .overflow_hidden()
            .when_some(banner, |this, banner| this.child(render_banner(&banner)))
            .when(
                !has_frontmatter
                    && self.settings.properties_visibility
                        != crate::settings::PropertiesVisibility::Hidden,
                |this| {
                    this.child(
                        div()
                            .id("properties-empty")
                            .w_full()
                            .px_6()
                            .py_1()
                            .my_2()
                            .cursor_pointer()
                            .rounded(theme.radius)
                            .hover(|row| row.bg(theme.accent.opacity(0.4)))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(self.tr("+ Add property", "+ Legg til egenskap")),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_add_property_dialog(window, cx);
                            })),
                    )
                },
            )
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
                            } else if let Some(target) = url.strip_prefix("missing:") {
                                // Unresolved `![[file]]` embed — not a note.
                                let target = target.to_string();
                                view.update(cx, |this, cx| {
                                    this.note_status(format!("No file named “{}”", target), cx);
                                });
                            } else if let Some(tag) = url.strip_prefix("tag:") {
                                // `#tag` in the note — project search for it,
                                // same as the sidebar tag pane.
                                let query = format!("#{tag}");
                                let view = view.clone();
                                crate::search::open_project_search_for(
                                    view,
                                    Some(query.as_str()),
                                    window,
                                    cx,
                                );
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
            .when(!mentions.is_empty(), |this| {
                this.child(render_linked_mentions(
                    doc.clone(),
                    mentions,
                    mentions_open,
                    footer_colors,
                    self.vault.read(cx).root.clone().unwrap_or_default(),
                    view.clone(),
                ))
            })
    }

    fn render_editor_area(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(page) = &self.folder {
            return self.render_folder_page(page, cx);
        }
        let Some(doc) = self.active_doc().cloned() else {
            return self.render_empty_editor(cx).into_any_element();
        };

        // Image files render the picture itself, not a
        // text editor over binary bytes.
        if doc.read(cx).is_image {
            return self.render_image_view(&doc, cx).into_any_element();
        }
        if doc.read(cx).file_preview.is_some() {
            return self.render_file_preview(&doc, cx);
        }

        // `.base` files render their live view in Preview/Split; Source
        // stays the raw YAML so the spec stays editable.
        let base = self
            .active
            .and_then(|i| self.docs.get(i))
            .and_then(|d| d.base.clone());
        let content = match self.settings.view_mode {
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
                    canvas_panels("split", Axis::Horizontal)
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
        };
        let content = match self.render_breadcrumb(&doc, cx) {
            Some(crumb) => v_flex()
                .size_full()
                .child(crumb)
                .child(div().flex_1().min_h_0().child(content))
                .into_any_element(),
            None => content,
        };
        if !doc.read(cx).conflict {
            return content;
        }

        let reload_doc = doc.clone();
        let banner = h_flex()
            .w_full()
            .min_h(px(36.))
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .bg(cx.theme().warning.opacity(0.12))
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(cx.theme().warning)
                    .child("Changed on disk — saving paused"),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("conflict-save-copy")
                            .ghost()
                            .xsmall()
                            .label("Save copy…")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_save_as(&SaveFileAs, window, cx);
                            })),
                    )
                    .child(
                        Button::new("conflict-reload")
                            .ghost()
                            .xsmall()
                            .label("Reload from disk")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_reload_from_disk(reload_doc.clone(), window, cx);
                            })),
                    ),
            );
        v_flex()
            .size_full()
            .child(banner)
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }

    /// Image document: the picture centered in the editor area with a
    /// `name.ext · N KB` caption — the breadcrumb stays on top.
    fn render_file_preview(&self, doc: &Entity<Document>, cx: &mut Context<Self>) -> AnyElement {
        use crate::file_preview::Preview;
        let document = doc.read(cx);
        let path = document.path.clone();
        let text = match &document.file_preview {
            Some(Preview::Text(text)) => Some(text.clone()),
            _ => None,
        };
        let message = match &document.file_preview {
            Some(Preview::TooLarge) => self.tr(
                "Too large for inline preview (256 KB limit). Open in another app.",
                "For stor for forhåndsvisning (grense på 256 KB). Åpne i en annen app.",
            ),
            _ => self.tr(
                "No inline preview for this format. Open in another app.",
                "Ingen forhåndsvisning for dette formatet. Åpne i en annen app.",
            ),
        };
        let mut toolbar = h_flex()
            .px_4()
            .py_2()
            .gap_2()
            .items_center()
            .child(div().flex_1().min_w_0().truncate().text_xs().child(format!(
                "{} · {}",
                document.file_name(),
                self.tr("Read-only preview", "Skrivebeskyttet forhåndsvisning")
            )))
            .child(
                Button::new("file-open-external")
                    .ghost()
                    .small()
                    .label(self.tr("Open in default app", "Åpne i standardappen"))
                    .on_click({
                        let path = path.clone();
                        move |_, _, _| open_in_default_app(&path)
                    }),
            );
        if let Some(text) = &text {
            let text = text.clone();
            toolbar = toolbar.child(
                Button::new("file-preview-copy")
                    .ghost()
                    .small()
                    .icon(assets::IconName::Copy)
                    .tooltip(self.tr("Copy text", "Kopier tekst"))
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                    }),
            );
        }
        #[cfg(target_os = "macos")]
        {
            toolbar = toolbar.child(
                Button::new("file-quick-look")
                    .ghost()
                    .small()
                    .label(self.tr("Quick Look", "Hurtigvisning"))
                    .on_click(move |_, _, _| {
                        let _ = std::process::Command::new("/usr/bin/qlmanage")
                            .arg("-p")
                            .arg(&path)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn();
                    }),
            );
        }
        v_flex()
            .size_full()
            .child(toolbar)
            .child(
                div()
                    .id("file-preview-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .text_sm()
                    .font_family("monospace")
                    .child(text.unwrap_or_else(|| message.into())),
            )
            .into_any_element()
    }

    fn render_image_view(
        &self,
        doc: &Entity<Document>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let path = doc.read(cx).path.clone();
        let caption = {
            let name = doc.read(cx).file_name();
            let kb = std::fs::metadata(&path)
                .map(|m| m.len() as f64 / 1024.)
                .unwrap_or(0.);
            let kb = if kb >= 1024. {
                format!("{:.1} MB", kb / 1024.)
            } else {
                format!("{} KB", kb.round() as u64)
            };
            format!("{name} · {kb}")
        };
        let picture = div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                img(gpui::ImageSource::from(path.clone()))
                    .size_full()
                    .object_fit(gpui::ObjectFit::Contain),
            );
        v_flex()
            .size_full()
            .overflow_hidden()
            .when_some(self.render_breadcrumb(doc, cx), |this, crumb| {
                this.child(crumb)
            })
            .child(div().flex_1().min_h_0().child(picture))
            .child(
                div()
                    .w_full()
                    .p_2()
                    .text_xs()
                    .text_center()
                    .text_color(muted)
                    .child(caption),
            )
    }

    /// Breadcrumb row above the editor: `folder / sub / name` — each
    /// folder segment opens its dashboard; the filename
    /// opens the rename dialog, like the inline title.
    fn render_breadcrumb(&self, doc: &Entity<Document>, cx: &mut Context<Self>) -> Option<Div> {
        let path = doc.read(cx).path.clone();
        let root = self.vault.read(cx).root.clone()?;
        let rel = path
            .strip_prefix(&root)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        let mut segs: Vec<&str> = rel.split('/').collect();
        let name = segs.pop()?;
        let stem = std::path::Path::new(name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.to_string());
        let view = cx.entity();
        let mut row = h_flex()
            .h_8()
            .flex_shrink_0()
            .w_full()
            // Same left edge as the preview text column (px_6).
            .px_6()
            .gap_1()
            .items_center()
            .text_xs()
            .text_color(cx.theme().muted_foreground);
        let root_target = root.clone();
        let root_siblings = root.clone();
        row = row
            .child(
                div()
                    .id("crumb-vault")
                    .cursor_pointer()
                    .hover(|s| s.text_color(cx.theme().foreground))
                    .child(
                        root.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_folder_page(root_target.clone(), window, cx);
                    })),
            )
            .child(self.sibling_button(0, root_siblings, cx))
            .child(div().child("›"));
        let mut acc = String::new();
        for (ix, seg) in segs.iter().enumerate() {
            if ix > 0 {
                acc.push('/');
            }
            acc.push_str(seg);
            let target = root.join(&acc);
            let siblings = target.clone();
            row = row
                .child(
                    div()
                        .id(SharedString::from(format!("crumb-{ix}")))
                        .cursor_pointer()
                        .hover(|s| s.text_color(cx.theme().foreground))
                        .child(seg.to_string())
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.open_folder_page(target.clone(), window, cx)
                                });
                            }
                        }),
                )
                .child(self.sibling_button(ix + 1, siblings, cx))
                .child(div().child("›"));
        }
        Some(
            row.child(
                div()
                    .id("crumb-name")
                    .cursor_pointer()
                    .text_color(cx.theme().foreground)
                    .hover(|s| s.text_color(cx.theme().info))
                    .child(stem)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.show_rename_dialog(path.clone(), window, cx);
                            });
                        }
                    }),
            ),
        )
    }

    /// The editor plus the paste/drop handlers that turn images into
    /// attachments. `can_drop` + `on_drop` receive OS file drops; the
    /// capture-phase action listener sees `Paste` before the editor's own
    /// handler (⌘V arrives as the action, not a raw key event — the menu's
    /// key equivalent intercepts it on macOS).
    fn editor_container(&self, doc: &Entity<Document>, cx: &mut Context<Self>) -> Div {
        div()
            .size_full()
            .on_scroll_wheel(cx.listener({
                let doc = doc.clone();
                move |this, _, window, _cx| {
                    if this.settings.preview_follows_source
                        && this.settings.view_mode == ViewMode::Split
                    {
                        let doc = doc.clone();
                        window.on_next_frame(move |_, cx| {
                            doc.update(cx, |doc, cx| doc.follow_source_scroll(cx))
                        });
                    }
                }
            }))
            .px_3()
            .py_4()
            // Scoped key context — the auto-pair bindings only fire when the
            // document editor is focused, so dialog/search inputs keep the
            // plain characters.
            .key_context("RistaEditor")
            .on_action(cx.listener(Self::on_pair_insert))
            .on_action(cx.listener(Self::on_pair_close))
            .capture_action::<input::Paste>(cx.listener(|this, _paste, window, cx| {
                this.on_editor_paste_action(window, cx);
            }))
            .capture_action::<input::Enter>(cx.listener(|this, _enter, window, cx| {
                let handled = this
                    .active_doc()
                    .cloned()
                    .map(|doc| doc.update(cx, |doc, cx| doc.continue_list(window, cx)))
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            }))
            // Pair-delete — backspace inside `( | )`/`" | "` removes both
            // chars; elsewhere the editor's own Backspace runs.
            .capture_action::<input::Backspace>(cx.listener(|this, _a, window, cx| {
                let handled = this
                    .active_doc()
                    .cloned()
                    .map(|doc| doc.update(cx, |doc, cx| doc.delete_pair(window, cx)))
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            }))
            .capture_action::<input::IndentInline>(cx.listener(|this, _a, window, cx| {
                let handled = this
                    .active_doc()
                    .cloned()
                    .map(|doc| {
                        doc.update(cx, |doc, cx| {
                            doc.table_cell_nav(false, cx) || doc.indent_selection(false, window, cx)
                        })
                    })
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            }))
            .capture_action::<input::OutdentInline>(cx.listener(|this, _a, window, cx| {
                let handled = this
                    .active_doc()
                    .cloned()
                    .map(|doc| {
                        doc.update(cx, |doc, cx| {
                            doc.table_cell_nav(true, cx) || doc.indent_selection(true, window, cx)
                        })
                    })
                    .unwrap_or(false);
                if handled {
                    cx.stop_propagation();
                }
            }))
            .can_drop(|value, _window, _cx| value.is::<ExternalPaths>() || value.is::<PathBuf>())
            .drag_over::<ExternalPaths>(|style, _paths, _window, cx| {
                style.bg(cx.theme().accent.opacity(0.08))
            })
            .drag_over::<PathBuf>(|style, _path, _window, cx| {
                style.bg(cx.theme().accent.opacity(0.08))
            })
            .on_drop::<ExternalPaths>(cx.listener(|this, paths, window, cx| {
                this.on_editor_drop(paths, window, cx);
            }))
            .on_drop::<PathBuf>(cx.listener(|this, path: &PathBuf, window, cx| {
                this.insert_tree_link(path, window, cx);
            }))
            .child({
                // Right-click menu — clipboard row plus the markdown actions
                // the palette already exposes (the edit context menu).
                // `appearance(false)` — the editor's bordered box would
                // draw a second frame inside the pane card.
                let editor = Editor::new(&doc.read(cx).editor)
                    .h_full()
                    .appearance(false)
                    .context_menu(|menu, _window, _cx| {
                        menu.menu("Cut", Box::new(input::Cut))
                            .menu("Copy", Box::new(input::Copy))
                            .menu("Paste", Box::new(input::Paste))
                            .menu("Select All", Box::new(input::SelectAll))
                            .separator()
                            .menu("Italic", Box::new(ToggleItalic))
                            .menu("Task checkbox", Box::new(ToggleCheckbox))
                            .menu("Toggle comment", Box::new(ToggleComment))
                            .separator()
                            .menu("Move line up", Box::new(MoveLineUp))
                            .menu("Move line down", Box::new(MoveLineDown))
                            .menu("Duplicate line", Box::new(DuplicateBlock))
                            .menu("Delete line", Box::new(DeleteLine))
                            .separator()
                            .menu("Open link under cursor", Box::new(FollowLink))
                    });
                // `cssclasses:` per-note override — `wide` lifts the
                // readable-width cap for this note, `narrow`/`readable`
                // forces it on (the per-note styling hook).
                let classes = &doc.read(cx).css_classes;
                let readable = if classes.iter().any(|c| c == "wide") {
                    false
                } else if classes.iter().any(|c| c == "narrow" || c == "readable") {
                    true
                } else {
                    self.settings.readable_width
                };
                if readable {
                    // Readable line length: cap the text column and
                    // center it (the editor setting).
                    div()
                        .size_full()
                        .flex()
                        .justify_center()
                        .child(div().h_full().w(px(760.)).max_w_full().child(editor))
                        .into_any_element()
                } else {
                    editor.into_any_element()
                }
            })
    }

    /// Dropping a file from the tree into the editor inserts a vault link:
    /// `![[name.png]]` for images, `[[stem]]` for notes and `.base` files.
    fn insert_tree_link(
        &mut self,
        src: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();
        let markup = if IMAGE_EXTS.contains(&ext.as_str()) {
            src.file_name()
                .and_then(|n| n.to_str())
                .map(|n| format!("![[{n}]]"))
        } else if matches!(ext.as_str(), "md" | "base") {
            src.file_stem()
                .and_then(|n| n.to_str())
                .map(|n| format!("[[{n}]]"))
        } else {
            None
        };
        if let Some(markup) = markup {
            doc.update(cx, |doc, cx| {
                doc.editor.update(cx, |e, cx| e.insert(markup, window, cx));
            });
        }
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
                ClipboardEntry::String(text) => {
                    // Pasting a URL over a selection wraps it in a
                    // markdown link — smart-editor behavior.
                    let url = text.text.trim();
                    let is_url = (url.starts_with("https://") || url.starts_with("http://"))
                        && !url.chars().any(char::is_whitespace)
                        && url.len() < 2048;
                    if is_url {
                        handled |= self
                            .active_doc()
                            .cloned()
                            .map(|doc| {
                                doc.update(cx, |doc, cx| {
                                    doc.wrap_selection_in_link(url, window, cx)
                                })
                            })
                            .unwrap_or(false);
                    }
                }
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
    /// `![[name]]` — the the reference editor "Pasted image <stamp>" convention.
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
                            Button::new("open-file")
                                .primary()
                                .icon(assets::IconName::FileText)
                                .label("Open file…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.on_open_file(&OpenFile, window, cx);
                                })),
                        )
                        .child(
                            Button::new("open-folder")
                                .ghost()
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
        let (rel_path, words, dirty, conflict, cursor, selected) = doc
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
                let editor = doc.editor.read(cx);
                let pos = editor.cursor_position();
                let sel = editor.selected_range();
                (
                    rel,
                    doc.stats.0,
                    doc.dirty,
                    doc.conflict,
                    Some((pos.line + 1, pos.character + 1)),
                    (!sel.is_empty()).then(|| sel.end - sel.start),
                )
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
                    .when_some(cursor, |this, (line, col)| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Ln {line}, Col {col}")),
                        )
                    })
                    .when_some(selected, |this, n| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("({n} selected)")),
                        )
                    })
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
                    )
                    .when(
                        self.settings.view_mode == ViewMode::Split && !self.active_doc_is_image(cx),
                        |bar| {
                            bar.child(
                                Button::new("preview-follow-source")
                                    .ghost()
                                    .xsmall()
                                    .icon(assets::IconName::Link)
                                    .selected(self.settings.preview_follows_source)
                                    .tooltip(self.tr(
                                        "Preview follows source scrolling",
                                        "Forhåndsvisning følger rulling i kilden",
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.preview_follows_source =
                                            !this.settings.preview_follows_source;
                                        this.settings.save();
                                        cx.notify();
                                    })),
                            )
                        },
                    )
                    .when(
                        doc.is_some_and(|doc| !doc.read(cx).outgoing_unresolved.is_empty()),
                        |bar| {
                            bar.child(
                                Button::new("link-diagnostics")
                                    .ghost()
                                    .xsmall()
                                    .icon(assets::IconName::TriangleAlert)
                                    .label(format!(
                                        "{} {}",
                                        doc.map(|doc| doc.read(cx).outgoing_unresolved.len())
                                            .unwrap_or(0),
                                        self.tr("missing links", "manglende lenker")
                                    ))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.settings.inspector_open = true;
                                        this.outgoing_open = true;
                                        this.settings.panes.outgoing = true;
                                        this.settings.save();
                                        cx.notify();
                                    })),
                            )
                        },
                    )
                    .child(
                        Button::new("workspace-tools")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Blocks)
                            .label(self.tr("Tools", "Verktøy"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_open_tools(&OpenTools, window, cx)
                            })),
                    )
                    .child(
                        Button::new("workspace-terminal")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Terminal)
                            .label(self.tr("Terminal", "Terminal"))
                            .selected(self.terminal_visible)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_terminal(&ToggleTerminal, window, cx)
                            })),
                    ),
            )
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for (index, tab) in self.terminals.iter().enumerate() {
            tab.terminal.update(cx, |terminal, _| {
                terminal.visible = self.terminal_visible && self.active_terminal == Some(index)
            });
        }
        // Docs pick up external edits here — a window handle is guaranteed.
        if self.needs_fs_check {
            self.needs_fs_check = false;
            for doc in &self.docs {
                doc.entity.update(cx, |doc, cx| {
                    if doc.vault_root.is_some() {
                        doc.check_external(window, cx);
                        // Embeds/banners resolve against the vault index — a
                        // newly added image should light up open previews.
                        doc.resync_preview(cx);
                    }
                });
            }
            if let Some(page) = self.folder.take() {
                self.folder = Some(self.load_folder_page(page.path, page.scroll, cx));
            }
        }
        if self.needs_standalone_fs_check {
            self.needs_standalone_fs_check = false;
            for doc in &self.docs {
                doc.entity.update(cx, |doc, cx| {
                    if doc.vault_root.is_none() {
                        doc.check_external(window, cx);
                    }
                });
            }
        }

        // Focus fallback: shortcuts and menu items only dispatch through a
        // focused view. When nothing holds focus (welcome screen, preview
        // mode, just-closed dialog) give it back to the workspace root.
        if window.focused(cx).is_none() && !self.focus_fallback_pending {
            self.focus_fallback_pending = true;
            let handle = self.focus_handle.clone();
            let view = cx.entity();
            window.on_next_frame(move |window, cx| {
                view.update(cx, |this, cx| {
                    this.focus_fallback_pending = false;
                    if window.focused(cx).is_none() {
                        handle.focus(window, cx);
                    }
                });
            });
        }

        let vault_open = self.vault.read(cx).is_open();
        let has_workspace = vault_open || !self.docs.is_empty();
        let sidebar_visible = vault_open && !self.settings.sidebar_collapsed && !self.zen;
        let background = cx.theme().background;

        v_flex()
            .size_full()
            .relative()
            .bg(background)
            .key_context("Rista")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_new_file))
            .on_action(cx.listener(Self::on_new_folder))
            .on_action(cx.listener(Self::on_open_file))
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_open_daily))
            .on_action(cx.listener(Self::on_close_folder))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(cx.listener(Self::on_move_line_up))
            .on_action(cx.listener(Self::on_move_line_down))
            .on_action(cx.listener(Self::on_toggle_checkbox))
            .on_action(cx.listener(Self::on_toggle_italic))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_reopen_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_navigate_back))
            .on_action(cx.listener(Self::on_navigate_forward))
            .on_action(cx.listener(Self::on_follow_link))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_zen))
            .on_action(cx.listener(Self::on_view_source))
            .on_action(cx.listener(Self::on_view_split))
            .on_action(cx.listener(Self::on_view_preview))
            .on_action(cx.listener(Self::on_toggle_edit_preview))
            .on_action(cx.listener(Self::on_open_graph))
            .on_action(cx.listener(Self::on_open_local_graph))
            .on_action(cx.listener(Self::on_duplicate_block))
            .on_action(cx.listener(Self::on_delete_line))
            .on_action(cx.listener(Self::on_toggle_comment))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::on_open_palette))
            .on_action(cx.listener(Self::on_quick_open))
            .on_action(cx.listener(Self::on_find))
            .on_action(cx.listener(Self::on_open_project_search))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_toggle_terminal))
            .on_action(cx.listener(Self::on_open_tools))
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_about))
            .on_action(cx.listener(Self::on_check_for_updates))
            .when(!self.zen, |this| {
                this.child(self.render_title_bar(window, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_1()
                    .py_1()
                    .child(if !has_workspace {
                        div()
                            .size_full()
                            .child(self.render_empty_editor(cx))
                            .into_any_element()
                    } else {
                        canvas_panels("workspace", Axis::Horizontal)
                            .when(sidebar_visible, |this| {
                                this.child(
                                    resizable_panel()
                                        .size(px(260.))
                                        .size_range(px(200.)..px(420.))
                                        .child(
                                            div().size_full().p_1().child(
                                                div()
                                                    .size_full()
                                                    .bg(cx.theme().sidebar)
                                                    .rounded(cx.theme().radius_lg)
                                                    .overflow_hidden()
                                                    .child(self.render_sidebar(cx)),
                                            ),
                                        ),
                                )
                            })
                            .child(
                                resizable_panel().child(
                                    canvas_panels("editor-terminal", Axis::Vertical)
                                        .child(
                                            resizable_panel().child(
                                                div().size_full().p_1().child(
                                                    v_flex()
                                                        .size_full()
                                                        .bg(cx.theme().group_box)
                                                        .rounded(cx.theme().radius_lg)
                                                        .overflow_hidden()
                                                        .when(!self.zen, |this| {
                                                            this.child(self.render_tab_bar(cx))
                                                        })
                                                        .child(div().flex_1().min_h_0().child({
                                                            if let Some(graph) = self.graph.clone()
                                                            {
                                                                graph.into_any_element()
                                                            } else {
                                                                let content =
                                                                    self.render_editor_area(cx);
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
                                                            }
                                                        })),
                                                ),
                                            ),
                                        )
                                        .when(self.terminal_visible && !self.zen, |panels| {
                                            panels.child(
                                                resizable_panel()
                                                    .size(px(270.))
                                                    .size_range(px(140.)..px(600.))
                                                    .child(
                                                        div()
                                                            .size_full()
                                                            .p_1()
                                                            .child(self.render_terminal_panel(cx)),
                                                    ),
                                            )
                                        }),
                                ),
                            )
                            .when(!self.zen && self.settings.inspector_open, |columns| {
                                columns.child(
                                    resizable_panel()
                                        .size(px(268.))
                                        .size_range(px(220.)..px(380.))
                                        .child(
                                            div().size_full().p_1().child(
                                                div()
                                                    .size_full()
                                                    .bg(cx.theme().sidebar)
                                                    .rounded(cx.theme().radius_lg)
                                                    .overflow_hidden()
                                                    .child(self.render_inspector(cx)),
                                            ),
                                        ),
                                )
                            })
                            .into_any_element()
                    }),
            )
            .when(!self.zen && has_workspace, |this| {
                this.child(self.render_status_bar(cx))
            })
            .when_some(self.peek.clone(), |this, (kind, pos)| {
                this.child(self.render_peek_card(&kind, pos, window, cx))
            })
            .when_some(self.file_menu.clone(), |this, (menu, pos)| {
                this.child(
                    deferred(
                        anchored()
                            .position(pos)
                            .snap_to_window_with_margin(px(8.))
                            .child(menu),
                    )
                    .with_priority(gpui_kit::base::POPUP_PRIORITY),
                )
            })
    }
}

impl Workspace {
    /// The floating hover-preview card: note title + the first few
    /// non-empty body lines, anchored next to the cursor and clamped
    /// inside the window.
    fn render_peek_card(
        &self,
        kind: &PeekKind,
        pos: gpui::Point<gpui::Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (title, excerpt): (String, Vec<String>) = match kind {
            PeekKind::Footnote(label, body) => (
                format!("[{label}]"),
                vec![body.clone()]
                    .into_iter()
                    .filter(|l| !l.is_empty())
                    .collect(),
            ),
            PeekKind::Note(path) => {
                let title = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let excerpt: Vec<String> = std::fs::read_to_string(path)
                    .ok()
                    .map(|text| {
                        let body = match crate::properties::frontmatter_span(&text) {
                            Some(span) => &text[span.end..],
                            None => text.as_str(),
                        };
                        body.lines()
                            .map(str::trim)
                            .filter(|l| !l.is_empty())
                            .take(4)
                            .map(|l| {
                                l.trim_start_matches(|c: char| {
                                    c == '#' || c == '>' || c == '-' || c == '*' || c == ' '
                                })
                                .to_string()
                            })
                            .filter(|l| !l.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                (title, excerpt)
            }
        };
        let viewport = window.bounds().size;
        let w = px(340.);
        let left = (pos.x + px(12.))
            .min(viewport.width - w - px(12.))
            .max(px(4.));
        // Below the cursor normally, above it near the window's foot.
        let top = if pos.y > viewport.height - px(240.) {
            pos.y - px(180.)
        } else {
            pos.y + px(18.)
        };
        v_flex()
            .id("peek-card")
            .absolute()
            .left(left)
            .top(top)
            .w(w)
            .gap_1()
            .p_3()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .shadow_lg()
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .truncate()
                    .child(title),
            )
            .children(excerpt.into_iter().map(|line| {
                div()
                    .w_full()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .truncate()
                    .child(line)
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

/// The preview's linked-mentions footer — the "Linked mentions"
/// section pinned under the note: a count header that expands into rows,
/// each opening the note that links here.
fn render_linked_mentions(
    doc: Entity<Document>,
    mentions: Vec<PathBuf>,
    open: bool,
    colors: (gpui::Hsla, gpui::Hsla),
    root: PathBuf,
    view: Entity<Workspace>,
) -> impl IntoElement {
    let (fg, muted) = colors;
    let count = mentions.len();
    let chevron = if open {
        assets::IconName::ChevronDown
    } else {
        assets::IconName::ChevronRight
    };
    let toggle_view = view.clone();
    v_flex()
        .w_full()
        .flex_none()
        .child(
            div()
                .id("mentions-toggle")
                .w_full()
                .px_6()
                .py_1p5()
                .cursor_pointer()
                .hover(|s| s.bg(muted.opacity(0.5)))
                .child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .text_color(muted)
                        .child(Icon::new(chevron).size_3p5())
                        .child(format!(
                            "{count} linked mention{}",
                            if count == 1 { "" } else { "s" }
                        )),
                )
                .on_click(move |_, _window, cx| {
                    doc.update(cx, |doc, cx| {
                        doc.mentions_open = !doc.mentions_open;
                        cx.notify();
                    });
                    // The toggle lives on Document but the strip is rendered
                    // by the workspace — notify it or nothing repaints.
                    toggle_view.update(cx, |_, cx| cx.notify());
                }),
        )
        .when(open, |this| {
            this.child(
                v_flex()
                    .pb_1()
                    .children(mentions.iter().enumerate().map(|(ix, path)| {
                        let title = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default()
                            .to_string();
                        let rel = path
                            .parent()
                            .and_then(|p| p.strip_prefix(&root).ok())
                            .map(|p| p.to_string_lossy().to_string())
                            .filter(|s| !s.is_empty());
                        let open_path = path.clone();
                        let view = view.clone();
                        div()
                            .id(("mention-row", ix))
                            .w_full()
                            .px_6()
                            .py_1()
                            .cursor_pointer()
                            .hover(|s| s.bg(muted.opacity(0.5)))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(div().text_sm().text_color(fg).child(title))
                                    .when_some(rel, |this, rel| {
                                        this.child(div().text_xs().text_color(muted).child(rel))
                                    }),
                            )
                            .on_click(move |_, window, cx| {
                                let open_path = open_path.clone();
                                view.update(cx, |ws, cx| {
                                    ws.open_document_pub(open_path, window, cx);
                                });
                            })
                    })),
            )
        })
}

/// Open `path`'s containing folder in the OS file manager, with the
/// entry selected where the platform supports it.
fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.to_string_lossy()))
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(dir) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
    let _ = path;
}

/// Open `path` in the OS default application — the "Open in
/// default app" file context item.
fn open_in_default_app(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
    let _ = path;
}

/// Whether the file is an `.base` database spec.
pub(crate) fn is_base(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("base"))
        .unwrap_or(false)
}

/// `<templates_dir>/**/*.md` under the vault root, sorted for a stable dialog list.
fn template_files(root: &std::path::Path, templates_dir: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.join(templates_dir)];
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

/// `YYYY-MM-DD` / `YYYY/MM/DD` property values get the date picker;
/// datetimes stay text so a pick can't silently drop a time.
fn is_iso_date(s: &str) -> Option<chrono::NaiveDate> {
    let s = s.trim();
    for fmt in ["%Y-%m-%d", "%Y/%m/%d"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d);
        }
    }
    None
}

/// Floating label shown while dragging a file-tree row.
struct TreeDragPreview {
    label: SharedString,
}

/// Drag payload for reordering document tabs — carries the source index.
struct DraggedTab(usize);

/// What a hover-peek card shows: a note excerpt, or arbitrary text
/// (footnote definitions carry their resolved body inline).
#[derive(Clone, PartialEq)]
pub enum PeekKind {
    Note(PathBuf),
    Footnote(String, String),
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

/// The Properties strip type picker's coercion — the reference editor writes a
/// typed YAML value when a property's type changes.
fn coerce_property_value(edit: &str, kind: &str) -> serde_yaml::Value {
    use serde_yaml::Value as V;
    let v = serde_yaml::from_str::<V>(edit).unwrap_or_else(|_| V::String(edit.to_string()));
    fn text_of(v: &V) -> String {
        match v {
            V::String(s) => s.clone(),
            V::Number(n) => n.to_string(),
            V::Bool(b) => b.to_string(),
            V::Sequence(items) => items.iter().map(text_of).collect::<Vec<_>>().join(", "),
            V::Null => String::new(),
            other => serde_yaml::to_string(other)
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
        }
    }
    // `YYYY-MM-DD` at a position — the date shape the strip's calendar
    // icon and `.base` `file.day` already speak.
    let date_at = |s: &str, at: usize| -> bool {
        let b = s.as_bytes();
        s.len() >= at + 10
            && b[at + 4] == b'-'
            && b[at + 7] == b'-'
            && b[at..at + 4].iter().all(|c| c.is_ascii_digit())
            && b[at + 5..at + 7].iter().all(|c| c.is_ascii_digit())
            && b[at + 8..at + 10].iter().all(|c| c.is_ascii_digit())
    };
    let time_at = |s: &str, at: usize| -> bool {
        let b = s.as_bytes();
        s.len() >= at + 5
            && b[at + 2] == b':'
            && b[at..at + 2].iter().all(|c| c.is_ascii_digit())
            && b[at + 3..at + 5].iter().all(|c| c.is_ascii_digit())
    };
    match kind {
        "list" => match v {
            V::Sequence(_) => v,
            V::Null => V::Sequence(Vec::new()),
            other => V::Sequence(vec![other]),
        },
        "number" => match &v {
            V::Number(_) => v,
            V::String(s) => s
                .trim()
                .parse::<f64>()
                .map(serde_yaml::Value::from)
                .unwrap_or_else(|_| V::from(0)),
            _ => V::from(0),
        },
        "checkbox" => match &v {
            V::Bool(_) => v,
            V::String(s) => V::Bool(matches!(s.trim(), "true" | "yes" | "1" | "on")),
            _ => V::Bool(false),
        },
        "date" => match &v {
            V::String(s) if date_at(s, 0) => V::String(s[..10].to_string()),
            _ => V::String(chrono::Local::now().format("%Y-%m-%d").to_string()),
        },
        "time" => match &v {
            V::String(s) if time_at(s, 0) => V::String(s[..5].to_string()),
            V::String(s)
                if s.len() >= 16 && (s.as_bytes()[10] == b'T' || s.as_bytes()[10] == b' ') =>
            {
                if time_at(s, 11) {
                    V::String(s[11..16].to_string())
                } else {
                    V::String(chrono::Local::now().format("%H:%M").to_string())
                }
            }
            _ => V::String(chrono::Local::now().format("%H:%M").to_string()),
        },
        _ => V::String(text_of(&v)),
    }
}

#[cfg(test)]
mod standalone_fs_check_tests {
    use super::{update_observed_snapshot, FileMetadataSnapshot};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::time::UNIX_EPOCH;

    #[test]
    fn observed_metadata_invalidates_only_for_new_or_changed_snapshots() {
        let path = Path::new("/standalone.md");
        let mut observed: HashMap<PathBuf, Option<FileMetadataSnapshot>> = HashMap::new();
        let original = FileMetadataSnapshot {
            modified: Some(UNIX_EPOCH),
            len: 6,
            identity: Some((1, 2, 3, 4)),
        };

        assert!(update_observed_snapshot(
            &mut observed,
            path,
            Some(original.clone())
        ));
        assert!(!update_observed_snapshot(
            &mut observed,
            path,
            Some(original.clone())
        ));

        let changed = FileMetadataSnapshot {
            len: 7,
            ..original.clone()
        };
        assert!(update_observed_snapshot(&mut observed, path, Some(changed)));
        assert!(update_observed_snapshot(&mut observed, path, None));
        assert!(!update_observed_snapshot(&mut observed, path, None));

        let recreated = FileMetadataSnapshot {
            identity: Some((1, 5, 6, 7)),
            ..original.clone()
        };
        assert!(update_observed_snapshot(
            &mut observed,
            path,
            Some(recreated.clone())
        ));
        assert!(!update_observed_snapshot(
            &mut observed,
            path,
            Some(recreated)
        ));
    }
}

#[cfg(test)]
mod preview_tab_tests {
    use super::preview_tab_is_replaceable;

    #[test]
    fn only_clean_unconflicted_unpinned_preview_tabs_are_replaceable() {
        assert!(preview_tab_is_replaceable(true, false, false, false));
        assert!(!preview_tab_is_replaceable(false, false, false, false));
        assert!(!preview_tab_is_replaceable(true, true, false, false));
        assert!(!preview_tab_is_replaceable(true, false, true, false));
        assert!(!preview_tab_is_replaceable(true, false, false, true));
    }
}
