//! Workspace: the root view — title bar, sidebar, tabs, editor, preview,
//! status bar, palette, dialogs. Everything routes through here.

use crate::actions::*;
use crate::bases;
use crate::bookmarks::{BookmarkKind, DropTarget};
use crate::document::{Document, DocumentEvent, ImageResolver};
use crate::editor_layout::{Axis, EditorLayout, GroupId, Node, PaneMode, TabToken};
use crate::preview;
use crate::properties;
use crate::search;
use crate::search_service::SearchService;
use crate::settings::{Appearance, Language, Period, Settings, TreeSort, ViewMode};
use crate::settings_panel::SettingsView;
use crate::theme;
use crate::vault::{Vault, VaultEvent, IMAGE_EXTS};
use chrono::NaiveDate;
use gpui_kit::assets;
use gpui_kit::base::ElementExt;
use gpui_kit::base::Placement;
use gpui_kit::base::Selectable;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::date_picker::{DatePicker, DatePickerEvent, DatePickerState, DateTime};
use gpui_kit::component::dialog::{DialogAction, DialogClose, DialogFooter};
use gpui_kit::component::input::{self, Editor, Input, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, ResizablePanelGroup};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    h_flex, v_flex, ActiveTheme, Disableable, Icon, IndexPath, Sizable, TitleBar, WindowExt,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

#[path = "agent_panel.rs"]
mod agent_panel;
#[path = "bookmarks_panel.rs"]
mod bookmarks_panel;
#[path = "explorer_panel.rs"]
mod explorer_panel;
#[path = "folder_dashboard.rs"]
mod folder_dashboard;
#[path = "pdf_export_ui.rs"]
mod pdf_export_ui;
#[path = "saved_workspaces.rs"]
mod saved_workspaces;
#[path = "tools_panel.rs"]
mod tools_panel;

/// Below this pane tab-strip width the view-mode control shows icons
/// instead of labels, so a narrow split pane keeps room for its tabs.
const PANE_MODE_LABELS_MIN_WIDTH: Pixels = px(420.);

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

fn same_canonical_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn bookmark_kinds(items: &[crate::bookmarks::Bookmark], kinds: &mut HashMap<u64, BookmarkKind>) {
    for item in items {
        kinds.insert(item.id, item.kind.clone());
        if let BookmarkKind::Group { items, .. } = &item.kind {
            bookmark_kinds(items, kinds);
        }
    }
}

fn apply_composer_path_list<T: Clone + PartialEq>(
    values: &mut [T],
    changes: &[crate::document::ComposerPathValueChange<T>],
    direction: crate::document::ComposerHistoryDirection,
) {
    for change in changes {
        let (expected, desired) = match direction {
            crate::document::ComposerHistoryDirection::Undo => (&change.after, &change.before),
            crate::document::ComposerHistoryDirection::Redo => (&change.before, &change.after),
        };
        if values.get(change.index) == Some(expected) {
            values[change.index] = desired.clone();
        }
    }
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

fn canvas_panels(id: impl Into<ElementId>, axis: Axis) -> ResizablePanelGroup {
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
    window_id: u64,
    vault: Entity<Vault>,
    search_service: SearchService,
    docs: Vec<OpenDoc>,
    doc_tokens: Vec<TabToken>,
    layout: EditorLayout,
    layout_epoch: u64,
    restoring_session: bool,
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
    settings_baseline: Settings,
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
    /// Tab token the right-click menu should act on — set by each tab's
    /// Right mouse-down handler (the strip-level context menu reads it
    /// when it builds its items on the next frame).
    tab_menu_token: Option<TabToken>,
    /// Panes whose tab strip is too narrow for the labelled view-mode
    /// control — they show icons instead. Measured on prepaint.
    compact_mode_panes: HashSet<GroupId>,
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
    bookmark_drop_target: Option<DropTarget>,
    dragged_bookmark: Option<u64>,
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
    graph_dock: Option<Entity<crate::graph::GraphView>>,
    agent_panel: Entity<agent_panel::AgentPanel>,
    needs_fs_check: bool,
    needs_standalone_fs_check: bool,
    standalone_fs_task: Option<Task<()>>,
    focus_fallback_pending: bool,
    pending_editor_focus: Option<(Entity<Document>, Option<usize>)>,
    pending_preview_history: Vec<(Entity<Document>, Box<dyn Action>)>,
    preview_history_scheduled: bool,
    hotkey_warning: Option<crate::hotkeys::HotkeyNotice>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

struct OpenDoc {
    entity: Entity<Document>,
    preview: bool,
    /// Present when the file is a `.base` — a live view over the vault.
    base: Option<Entity<bases::BaseView>>,
    _sub: Subscription,
    _focus_sub: Subscription,
}

fn tab_matches_window_move(
    doc: &Document,
    snapshot: &crate::windows::TabWindowMove,
    cx: &App,
) -> bool {
    doc.path.canonicalize().is_ok_and(|path| {
        snapshot.matches(&path, doc.editor.read(cx).value().as_ref(), doc.conflict)
    })
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

/// What a template-picker click does.
#[derive(Clone)]
enum TemplatePick {
    Insert(Entity<Document>),
    NewNote,
    /// Set the template new notes in this folder start from.
    Folder(PathBuf),
}

/// A template to expand for the note at `path`. `frontmatter` is the
/// text `tp.frontmatter` reads (the template itself when `None`).
struct TemplateJob {
    template: String,
    path: PathBuf,
    now: chrono::NaiveDateTime,
    frontmatter: Option<String>,
    /// The selection an inserted template replaces.
    selection: String,
}

/// The continuation `Workspace::ask_prompts` runs with the answers.
type PromptsDone = Rc<
    RefCell<
        Option<Box<dyn FnOnce(&mut Workspace, Vec<String>, &mut Window, &mut Context<Workspace>)>>,
    >,
>;

#[derive(Clone)]
enum PaletteEntry {
    File(PathBuf),
    Command(PaletteCmd),
    Tool(crate::extensions::Launcher),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PaletteCmd {
    SaveWorkspace,
    LoadWorkspace,
    RenameWorkspace,
    DeleteWorkspace,
    NewWindow,
    OpenInNewWindow,
    MoveToNewWindow,
    NewFile,
    NewCanvas,
    NewFolder,
    OpenFile,
    OpenFolder,
    DailyNote,
    WeeklyNote,
    MonthlyNote,
    QuarterlyNote,
    YearlyNote,
    PrevPeriodic,
    NextPeriodic,
    NewFromTemplate,
    AppendDaily,
    CloseFolder,
    Save,
    SaveAs,
    SplitRight,
    SplitDown,
    NextPane,
    PrevPane,
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
    ExtractHeading,
    MergeNote,
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
    CopyLinkBlock,
    CopyEmbedBlock,
    RandomNote,
    NewUniqueNote,
    RevealFile,
    CloseOtherTabs,
    CloseTabsRight,
    CloseAllTabs,
    TogglePin,
    ToggleReadable,
    ExportHtml,
    ExportPdf,
    Settings,
    ImportVaultSettings,
    ToggleTheme,
    Quit,
    Graph,
    LocalGraph,
    ToggleLocalGraphPanel,
    ToggleAgentPanel,
}

impl PaletteCmd {
    pub(crate) const ALL: &'static [Self] = &[
        Self::SaveWorkspace,
        Self::LoadWorkspace,
        Self::RenameWorkspace,
        Self::DeleteWorkspace,
        Self::NewWindow,
        Self::OpenInNewWindow,
        Self::MoveToNewWindow,
        Self::NewFile,
        Self::NewCanvas,
        Self::NewUniqueNote,
        Self::NewFromTemplate,
        Self::NewBase,
        Self::DailyNote,
        Self::WeeklyNote,
        Self::MonthlyNote,
        Self::QuarterlyNote,
        Self::YearlyNote,
        Self::PrevPeriodic,
        Self::NextPeriodic,
        Self::AppendDaily,
        Self::OpenFile,
        Self::OpenFolder,
        Self::NewFolder,
        Self::Save,
        Self::SaveAs,
        Self::SplitRight,
        Self::SplitDown,
        Self::NextPane,
        Self::PrevPane,
        Self::SourceMode,
        Self::SplitMode,
        Self::PreviewMode,
        Self::ToggleSidebar,
        Self::ToggleZen,
        Self::ProjectSearch,
        Self::Graph,
        Self::LocalGraph,
        Self::ToggleLocalGraphPanel,
        Self::ToggleAgentPanel,
        Self::MoveLineUp,
        Self::MoveLineDown,
        Self::ToggleCheckbox,
        Self::ToggleBold,
        Self::ToggleItalic,
        Self::ToggleHighlight,
        Self::ToggleStrike,
        Self::ExtractSelection,
        Self::ExtractHeading,
        Self::MergeNote,
        Self::ToggleCode,
        Self::ToggleQuote,
        Self::CodeBlock,
        Self::ListBullet,
        Self::ListNumbered,
        Self::ListTask,
        Self::Heading1,
        Self::Heading2,
        Self::Heading3,
        Self::Heading4,
        Self::Heading5,
        Self::Heading6,
        Self::InsertLink,
        Self::InsertHr,
        Self::InsertCallout,
        Self::InsertDate,
        Self::InsertTime,
        Self::InsertImage,
        Self::InsertTable,
        Self::FormatTable,
        Self::InsertFootnote,
        Self::DeleteLine,
        Self::PageHistory,
        Self::RestoreDeleted,
        Self::InsertTemplate,
        Self::DuplicateNote,
        Self::EditProperties,
        Self::BrowseTags,
        Self::Backlinks,
        Self::Outline,
        Self::ToggleFocus,
        Self::GoBack,
        Self::GoForward,
        Self::FollowLink,
        Self::ToggleStar,
        Self::ReopenTab,
        Self::CopyLink,
        Self::CopyLinkHeading,
        Self::CopyLinkBlock,
        Self::CopyEmbedBlock,
        Self::RandomNote,
        Self::RevealFile,
        Self::CloseOtherTabs,
        Self::CloseTabsRight,
        Self::CloseAllTabs,
        Self::TogglePin,
        Self::ToggleReadable,
        Self::ExportHtml,
        Self::ExportPdf,
        Self::ToggleTheme,
        Self::Settings,
        Self::ImportVaultSettings,
        Self::CloseFolder,
        Self::Quit,
    ];

    pub(crate) fn id(self) -> &'static str {
        use PaletteCmd::*;
        match self {
            SaveWorkspace => "save_workspace",
            LoadWorkspace => "load_workspace",
            RenameWorkspace => "rename_workspace",
            DeleteWorkspace => "delete_workspace",
            NewWindow => "new_window",
            OpenInNewWindow => "open_in_new_window",
            MoveToNewWindow => "move_to_new_window",
            NewFile => "new_file",
            NewCanvas => "new_canvas",
            NewUniqueNote => "new_unique_note",
            NewFromTemplate => "new_from_template",
            NewBase => "new_base",
            DailyNote => "open_daily_note",
            WeeklyNote => "open_weekly_note",
            MonthlyNote => "open_monthly_note",
            QuarterlyNote => "open_quarterly_note",
            YearlyNote => "open_yearly_note",
            PrevPeriodic => "previous_periodic_note",
            NextPeriodic => "next_periodic_note",
            AppendDaily => "append_daily_note",
            OpenFile => "open_file",
            OpenFolder => "open_folder",
            NewFolder => "new_folder",
            Save => "save",
            SaveAs => "save_as",
            SplitRight => "split_pane_right",
            SplitDown => "split_pane_down",
            NextPane => "next_pane",
            PrevPane => "previous_pane",
            SourceMode => "view_source",
            SplitMode => "view_split",
            PreviewMode => "view_preview",
            ToggleSidebar => "toggle_sidebar",
            ToggleZen => "toggle_zen",
            ProjectSearch => "open_project_search",
            Graph => "open_graph",
            LocalGraph => "open_local_graph",
            ToggleLocalGraphPanel => "toggle_local_graph_panel",
            ToggleAgentPanel => "toggle_agent_panel",
            MoveLineUp => "move_line_up",
            MoveLineDown => "move_line_down",
            ToggleCheckbox => "toggle_checkbox",
            ToggleBold => "toggle_bold",
            ToggleItalic => "toggle_italic",
            ToggleHighlight => "toggle_highlight",
            ToggleStrike => "toggle_strikethrough",
            ExtractSelection => "extract_selection",
            ExtractHeading => "extract_heading",
            MergeNote => "merge_note",
            ToggleCode => "toggle_code",
            ToggleQuote => "toggle_quote",
            CodeBlock => "insert_code_block",
            ListBullet => "toggle_bulleted_list",
            ListNumbered => "toggle_numbered_list",
            ListTask => "toggle_task_list",
            Heading1 => "heading_1",
            Heading2 => "heading_2",
            Heading3 => "heading_3",
            Heading4 => "heading_4",
            Heading5 => "heading_5",
            Heading6 => "heading_6",
            InsertLink => "insert_link",
            InsertHr => "insert_horizontal_rule",
            InsertCallout => "insert_callout",
            InsertDate => "insert_date",
            InsertTime => "insert_time",
            InsertImage => "insert_image",
            InsertTable => "insert_table",
            FormatTable => "format_table",
            InsertFootnote => "insert_footnote",
            DeleteLine => "delete_line",
            PageHistory => "open_page_history",
            RestoreDeleted => "restore_deleted",
            InsertTemplate => "insert_template",
            DuplicateNote => "duplicate_note",
            EditProperties => "edit_properties",
            BrowseTags => "browse_tags",
            Backlinks => "open_backlinks",
            Outline => "open_outline",
            ToggleFocus => "toggle_focus_mode",
            GoBack => "navigate_back",
            GoForward => "navigate_forward",
            FollowLink => "follow_link",
            ToggleStar => "toggle_star",
            ReopenTab => "reopen_tab",
            CopyLink => "copy_link",
            CopyLinkHeading => "copy_link_to_heading",
            CopyLinkBlock => "copy_block_link",
            CopyEmbedBlock => "copy_block_embed",
            RandomNote => "open_random_note",
            RevealFile => "reveal_file",
            CloseOtherTabs => "close_other_tabs",
            CloseTabsRight => "close_tabs_right",
            CloseAllTabs => "close_all_tabs",
            TogglePin => "toggle_pin",
            ToggleReadable => "toggle_readable_width",
            ExportHtml => "export_html",
            ExportPdf => "export_pdf",
            ToggleTheme => "toggle_theme",
            Settings => "open_settings",
            ImportVaultSettings => "import_vault_settings",
            CloseFolder => "close_folder",
            Quit => "quit",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        self.spec().1
    }

    pub(crate) fn label_norwegian(self) -> &'static str {
        use PaletteCmd::*;
        match self {
            SaveWorkspace => "Lagre arbeidsoppsett…",
            LoadWorkspace => "Åpne arbeidsoppsett…",
            RenameWorkspace => "Gi arbeidsoppsett nytt navn…",
            DeleteWorkspace => "Slett arbeidsoppsett…",
            NewWindow => "Nytt vindu",
            OpenInNewWindow => "Åpne i nytt vindu",
            MoveToNewWindow => "Flytt til nytt vindu",
            NewFile => "Ny fil",
            NewCanvas => "Nytt lerret",
            NewUniqueNote => "Nytt unikt notat",
            NewFromTemplate => "Nytt notat fra mal",
            NewBase => "Ny base",
            DailyNote => "Åpne dagsnotat",
            WeeklyNote => "Åpne ukenotat",
            MonthlyNote => "Åpne månedsnotat",
            QuarterlyNote => "Åpne kvartalsnotat",
            YearlyNote => "Åpne årsnotat",
            PrevPeriodic => "Forrige periodiske notat",
            NextPeriodic => "Neste periodiske notat",
            AppendDaily => "Legg til i dagsnotat",
            OpenFile => "Åpne fil…",
            OpenFolder => "Åpne mappe…",
            NewFolder => "Ny mappe",
            Save => "Lagre",
            SaveAs => "Lagre som",
            SplitRight => "Del til høyre",
            SplitDown => "Del ned",
            NextPane => "Neste redigeringsfelt",
            PrevPane => "Forrige redigeringsfelt",
            SourceMode => "Kildemodus",
            SplitMode => "Delt visning",
            PreviewMode => "Forhåndsvisning",
            ToggleSidebar => "Veksle sidepanel",
            ToggleZen => "Veksle fokusmodus",
            ProjectSearch => "Søk i prosjektet",
            Graph => "Åpne graf",
            LocalGraph => "Åpne lokal graf",
            ToggleLocalGraphPanel => "Vis/skjul lokalt grafpanel",
            ToggleAgentPanel => "Vis/skjul agentpanel",
            MoveLineUp => "Flytt linje opp",
            MoveLineDown => "Flytt linje ned",
            ToggleCheckbox => "Veksle avkryssing",
            ToggleBold => "Veksle fet skrift",
            ToggleItalic => "Veksle kursiv",
            ToggleHighlight => "Veksle utheving",
            ToggleStrike => "Veksle gjennomstreking",
            ExtractSelection => "Trekk ut markering",
            ExtractHeading => "Trekk ut denne overskriften…",
            MergeNote => "Slå sammen gjeldende notat med…",
            ToggleCode => "Veksle kode",
            ToggleQuote => "Veksle sitat",
            CodeBlock => "Sett inn kodeblokk",
            ListBullet => "Veksle punktliste",
            ListNumbered => "Veksle nummerert liste",
            ListTask => "Veksle oppgaveliste",
            Heading1 => "Overskrift 1",
            Heading2 => "Overskrift 2",
            Heading3 => "Overskrift 3",
            Heading4 => "Overskrift 4",
            Heading5 => "Overskrift 5",
            Heading6 => "Overskrift 6",
            InsertLink => "Sett inn lenke",
            InsertHr => "Sett inn vannrett linje",
            InsertCallout => "Sett inn uthevet blokk",
            InsertDate => "Sett inn dato",
            InsertTime => "Sett inn klokkeslett",
            InsertImage => "Sett inn bilde",
            InsertTable => "Sett inn tabell",
            FormatTable => "Formater tabell",
            InsertFootnote => "Sett inn fotnote",
            DeleteLine => "Slett linje",
            PageHistory => "Åpne sidehistorikk",
            RestoreDeleted => "Gjenopprett slettet notat",
            InsertTemplate => "Sett inn mal",
            DuplicateNote => "Dupliser notat",
            EditProperties => "Rediger egenskaper",
            BrowseTags => "Bla gjennom emneknagger",
            Backlinks => "Åpne tilbakekoblinger",
            Outline => "Åpne disposisjon",
            ToggleFocus => "Veksle fokusmodus",
            GoBack => "Gå tilbake",
            GoForward => "Gå frem",
            FollowLink => "Åpne lenke under markøren",
            ToggleStar => "Veksle bokmerke",
            ReopenTab => "Gjenåpne lukket fane",
            CopyLink => "Kopier lenke",
            CopyLinkHeading => "Kopier lenke til overskrift",
            CopyLinkBlock => "Kopier lenke til blokk",
            CopyEmbedBlock => "Kopier blokkinnbygging",
            RandomNote => "Åpne tilfeldig notat",
            RevealFile => "Vis fil i filbehandler",
            CloseOtherTabs => "Lukk andre faner",
            CloseTabsRight => "Lukk faner til høyre",
            CloseAllTabs => "Lukk alle faner",
            TogglePin => "Veksle festet fane",
            ToggleReadable => "Veksle lesbar bredde",
            ExportHtml => "Eksporter HTML",
            ExportPdf => "Eksporter notat som PDF…",
            ToggleTheme => "Veksle tema",
            Settings => "Innstillinger",
            ImportVaultSettings => "Importer hvelvinnstillinger…",
            CloseFolder => "Lukk mappe",
            Quit => "Avslutt",
        }
    }

    pub(crate) fn spec(&self) -> (assets::IconName, &'static str, &'static [&'static str]) {
        use PaletteCmd::*;
        match self {
            SaveWorkspace => (
                assets::IconName::AppWindow,
                "Save workspace layout…",
                &["workspace", "layout", "save"],
            ),
            LoadWorkspace => (
                assets::IconName::AppWindow,
                "Load workspace layout…",
                &["workspace", "layout", "restore"],
            ),
            RenameWorkspace => (
                assets::IconName::SquarePen,
                "Rename workspace layout…",
                &["workspace", "layout", "rename"],
            ),
            DeleteWorkspace => (
                assets::IconName::Trash,
                "Delete workspace layout…",
                &["workspace", "layout", "delete"],
            ),
            NewWindow => (
                assets::IconName::AppWindow,
                "New window",
                &["window", "workspace"],
            ),
            OpenInNewWindow => (
                assets::IconName::AppWindow,
                "Open in New Window",
                &["open", "document", "window"],
            ),
            MoveToNewWindow => (
                assets::IconName::AppWindow,
                "Move to New Window",
                &["move", "tab", "window"],
            ),
            NewFile => (assets::IconName::FilePlus, "New file", &["create", "note"]),
            NewCanvas => (
                assets::IconName::FilePlus,
                "New Canvas",
                &["create", "board", "canvas"],
            ),
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
            WeeklyNote => (
                assets::IconName::CalendarRange,
                "Open weekly note",
                &["week", "periodic", "journal", "review"],
            ),
            MonthlyNote => (
                assets::IconName::CalendarDays,
                "Open monthly note",
                &["month", "periodic", "journal", "review"],
            ),
            QuarterlyNote => (
                assets::IconName::Calendar,
                "Open quarterly note",
                &["quarter", "periodic", "review"],
            ),
            YearlyNote => (
                assets::IconName::Calendar,
                "Open yearly note",
                &["year", "annual", "periodic", "review"],
            ),
            PrevPeriodic => (
                assets::IconName::ChevronLeft,
                "Previous daily/periodic note",
                &["previous", "yesterday", "last", "week", "month", "periodic"],
            ),
            NextPeriodic => (
                assets::IconName::ChevronRight,
                "Next daily/periodic note",
                &["next", "tomorrow", "week", "month", "periodic"],
            ),
            NewFromTemplate => (
                assets::IconName::LayoutTemplate,
                "New note from template…",
                &["create", "template", "boilerplate"],
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
            SplitRight => (
                assets::IconName::SquareSplitHorizontal,
                "Split editor right",
                &["pane", "split", "right"],
            ),
            SplitDown => (
                assets::IconName::SquareSplitVertical,
                "Split editor down",
                &["pane", "split", "down"],
            ),
            NextPane => (
                assets::IconName::ChevronRight,
                "Focus next editor pane",
                &["pane", "focus", "next"],
            ),
            PrevPane => (
                assets::IconName::ChevronLeft,
                "Focus previous editor pane",
                &["pane", "focus", "previous"],
            ),
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
            ExtractHeading => (
                assets::IconName::Scissors,
                "Extract this heading…",
                &["heading", "refactor", "composer"],
            ),
            MergeNote => (
                assets::IconName::Scissors,
                "Merge current note into…",
                &["merge", "composer"],
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
                "Bookmark/remove bookmark",
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
            CopyLinkBlock => (
                assets::IconName::Link,
                "Copy link to block",
                &["copy", "link", "block", "reference", "id", "clipboard"],
            ),
            CopyEmbedBlock => (
                assets::IconName::Link2,
                "Copy embed to block",
                &[
                    "copy",
                    "embed",
                    "transclude",
                    "block",
                    "reference",
                    "clipboard",
                ],
            ),
            RandomNote => (
                assets::IconName::Dices,
                "Open random note",
                &["random", "shuffle", "surprise", "serendipity"],
            ),
            NewUniqueNote => (
                assets::IconName::FileClock,
                "New unique note",
                &["create", "zettelkasten", "timestamp", "id", "prefix"],
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
            ExportPdf => (
                assets::IconName::FileText,
                "Export note as PDF…",
                &["export", "pdf", "print", "share", "save"],
            ),
            Settings => (
                assets::IconName::Settings,
                "Settings…",
                &["preferences", "theme"],
            ),
            ImportVaultSettings => (
                assets::IconName::FolderOpen,
                "Import vault settings…",
                &["vault", "settings", "import", "preferences"],
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
            ToggleLocalGraphPanel => (
                assets::IconName::Waypoints,
                "Toggle local graph panel",
                &["dock", "graph", "local", "panel"],
            ),
            ToggleAgentPanel => (
                assets::IconName::MessageSquareQuote,
                "Toggle agent panel",
                &["agent", "assistant", "panel", "dock"],
            ),
        }
    }
}

impl Workspace {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        window_id: u64,
        settings: Settings,
        initial_paths: Vec<PathBuf>,
        restore_session: bool,
    ) -> Self {
        let workspace = cx.entity().downgrade();
        let appearance_sub = window.observe_window_appearance(move |_, cx| {
            let _ = workspace.update(cx, |this, cx| {
                if this.settings.appearance == Appearance::System {
                    theme::apply(&this.settings, cx);
                    crate::apply_ui_settings(&this.settings, cx);
                    this.retheme_previews(cx);
                    cx.notify();
                }
            });
        });
        let shared_settings_sub = cx.observe_global_in::<crate::windows::SharedPreferences>(
            window,
            |this, window, cx| {
                let shared = cx
                    .global::<crate::windows::SharedPreferences>()
                    .settings
                    .clone();
                let (settings, changed) = crate::windows::merge_shared_settings(
                    &this.settings_baseline,
                    &shared,
                    &this.settings,
                );
                if changed {
                    this.settings_baseline = shared;
                    let settings = crate::windows::with_local_settings(settings, &this.settings);
                    this.apply_settings_inner(settings, window, cx, false);
                }
            },
        );
        let style_catalog_sub =
            cx.observe_global::<crate::style_snippets::StyleSnippetCatalog>(|this, cx| {
                theme::apply(&this.settings, cx);
                crate::apply_ui_settings(&this.settings, cx);
                this.retheme_previews(cx);
                this.settings_view.update(cx, |_, cx| cx.notify());
                cx.notify();
            });
        let window_id_for_activation = window_id;
        let activation_sub = cx.observe_window_activation(window, move |_, window, cx| {
            crate::windows::activate_window(window_id_for_activation, window, cx);
        });
        let vault = cx.new(Vault::new);
        let palette_state = cx.new(|cx| CommandState::new(window, cx));
        let rename_input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        let cell_input = cx.new(|cx| InputState::new(window, cx).placeholder("Value"));
        let tag_input = cx.new(|cx| InputState::new(window, cx).placeholder("new-name"));
        let prop_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Property name"));
        let folder_search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                settings
                    .language
                    .text("Search this folder…", "Søk i denne mappen…"),
            )
        });
        let folder_search_sub = cx.subscribe(
            &folder_search,
            |this: &mut Self, _, event: &input::InputEvent, cx| {
                if matches!(event, input::InputEvent::Change) {
                    if let Some(page) = &mut this.folder {
                        page.pages.fill(0);
                        page.scroll.set_offset(point(px(0.), px(0.)));
                    }
                }
                cx.notify();
            },
        );
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
        let panel_workspace = cx.weak_entity();
        let agent_panel = cx
            .new(|cx| agent_panel::AgentPanel::new(panel_workspace, settings.language, window, cx));
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);

        let vault_sub = cx.subscribe(&vault, |this, _v, event: &VaultEvent, cx| {
            this.on_vault_event(event, cx);
        });

        let mut this = Self {
            window_id,
            vault,
            search_service: SearchService::new(),
            docs: Vec::new(),
            doc_tokens: Vec::new(),
            layout: EditorLayout::empty(),
            layout_epoch: 0,
            restoring_session: false,
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
            tab_menu_token: None,
            compact_mode_panes: HashSet::new(),
            focus_mode: settings.focus_mode,
            nav_stack: Vec::new(),
            nav_pos: 0,
            nav_suppress: false,
            sidebar_details_scroll: ScrollHandle::new(),
            starred_open: settings.panes.starred,
            bookmark_drop_target: None,
            dragged_bookmark: None,
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
            graph_dock: None,
            agent_panel,
            needs_fs_check: false,
            needs_standalone_fs_check: false,
            standalone_fs_task: None,
            focus_fallback_pending: false,
            pending_editor_focus: None,
            pending_preview_history: Vec::new(),
            preview_history_scheduled: false,
            hotkey_warning: crate::hotkeys::warning(cx),
            focus_handle,
            settings_baseline: settings.clone(),
            settings,
            _subscriptions: vec![
                vault_sub,
                appearance_sub,
                folder_search_sub,
                explorer_sub,
                shared_settings_sub,
                activation_sub,
                style_catalog_sub,
            ],
        };

        this.settings
            .bookmarks
            .migrate_stars(&this.settings.starred);
        this.settings.starred = this.settings.bookmarks.starred_paths();
        this.vault.update(cx, |vault, _cx| {
            vault.starred = this.settings.starred.iter().cloned().collect();
        });

        let restore_saved_session = restore_session;
        let restored_tabs = this.settings.open_tabs.clone();
        let legacy_active = this
            .settings
            .active_tab
            .clone()
            .map(PathBuf::from)
            .map(|path| path.canonicalize().unwrap_or(path));
        let legacy_mode = match this.settings.view_mode {
            ViewMode::Source => PaneMode::Source,
            ViewMode::Split => PaneMode::Split,
            ViewMode::Preview => PaneMode::Preview,
        };
        let mut restored_session = EditorLayout::snapshot_for_startup(
            this.settings.editor_session.clone(),
            !restore_saved_session,
        );
        let restored_folder = restore_saved_session
            .then(|| this.settings.active_folder.clone())
            .flatten();
        this.restoring_session = true;
        let mut missing_saved_path = restore_saved_session
            && (restored_tabs.iter().any(|path| !Path::new(path).exists())
                || restored_session
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.paths.values().any(|path| !path.exists())));
        if restore_saved_session {
            if let Some(root) = this
                .settings
                .last_vault
                .clone()
                .filter(|root| root.exists())
            {
                this.open_vault_at(root, window, cx);
            }
            if let Some(snapshot) = restored_session.as_mut() {
                for path in snapshot.paths.values_mut() {
                    match path.canonicalize() {
                        Ok(canonical) if canonical.is_file() => *path = canonical,
                        _ => {
                            missing_saved_path = true;
                            path.clear();
                        }
                    }
                }
                snapshot
                    .paths
                    .retain(|_, path| !path.as_os_str().is_empty());
                let paths = snapshot.paths.values().cloned().collect::<Vec<_>>();
                for path in paths {
                    this.open_document_new_tab(path, window, cx);
                }
            } else {
                for path in restored_tabs {
                    let path = PathBuf::from(path);
                    if !path.exists() {
                        missing_saved_path = true;
                        continue;
                    }
                    this.open_path(path, window, cx);
                }
                if let Some(active) = legacy_active.as_ref() {
                    if let Some(ix) = this.docs.iter().position(|d| {
                        d.entity.read(cx).path.canonicalize().ok().as_ref() == Some(active)
                    }) {
                        this.active = Some(ix);
                    }
                }
            }
            if let Some(folder) = restored_folder.filter(|p| p.is_dir()) {
                this.open_folder_page(folder, window, cx);
            }
        } else {
            if let Some(root) = this
                .settings
                .last_vault
                .clone()
                .filter(|root| root.is_dir())
            {
                this.open_vault_at(root, window, cx);
            }
            for path in initial_paths {
                this.open_path(path, window, cx);
            }
        }
        this.restore_editor_session(
            restored_session,
            if restore_saved_session {
                legacy_active.as_deref()
            } else {
                None
            },
            legacy_mode,
            cx,
        );
        this.restoring_session = false;
        this.persist_tabs(cx);
        this.sync_graph_active(cx);
        this.focus_active_pane_after_mount(window, cx);
        this.queue_vault_settings_offer(window, cx);
        if missing_saved_path {
            this.note_status(
                this.tr(
                    "A saved editor tab could not be restored because its file is missing.",
                    "En lagret redigeringsfane kunne ikke gjenopprettes fordi filen mangler.",
                ),
                cx,
            );
        }

        let workspace = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            if cx.global::<crate::windows::WindowManager>().quitting {
                return true;
            }
            workspace.update(cx, |this, cx| match this.save_all(cx) {
                Ok(()) => {
                    this.persist_tabs(cx);
                    let bounds = window.window_bounds().get_bounds();
                    crate::windows::save_geometry(
                        this.window_id,
                        crate::settings::WindowGeometry {
                            x: bounds.origin.x.as_f32(),
                            y: bounds.origin.y.as_f32(),
                            width: bounds.size.width.as_f32(),
                            height: bounds.size.height.as_f32(),
                            maximized: window.is_maximized(),
                        },
                        cx,
                    );
                    this.stop_agent_process(cx);
                    true
                }
                Err(_) => {
                    let message = this.tr(
                        "Could not save before closing. Your changes are still open.",
                        "Kunne ikke lagre før lukking. Endringene dine er fortsatt åpne.",
                    );
                    this.note_status(message, cx);
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
        self.agent_panel
            .update(cx, |panel, cx| panel.vault_changed(cx));
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
        if !self.docs.iter().any(|open| {
            open.entity
                .read(cx)
                .path
                .canonicalize()
                .unwrap_or_else(|_| open.entity.read(cx).path.clone())
                .starts_with(&root)
        }) {
            if let Err(error) = crate::note_composer::recover_pending_transactions(&root) {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return false;
            }
        }
        self.close_all_docs(window, cx);
        self.clear_file_menu();
        self.graph = None;
        self.graph_dock = None;
        self.nav_stack.clear();
        self.nav_pos = 0;
        self.nav_suppress = false;
        self.move_undo.clear();
        self.explorer_search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.vault.update(cx, |vault, cx| {
            vault.tree_sort = self.settings.tree_sort;
            vault.show_other_files = self.settings.show_other_files;
            vault.set_link_format(self.settings.link_format);
            vault.templates_dir = self.settings.templates_dir.clone();
            vault.set_ignore_filters(&self.settings.user_ignore_filters);
            vault.open(
                root.clone(),
                self.settings
                    .expanded_folders
                    .get(&root.to_string_lossy().to_string())
                    .cloned()
                    .unwrap_or_default(),
                cx,
            );
        });
        self.terminals.clear();
        self.active_terminal = None;
        self.terminal_visible = false;
        self.settings.last_vault = Some(root.clone());
        self.persist_settings(cx);
        self.status_note = None;
        self.open_folder_page(root, window, cx);
        if !self.restoring_session {
            self.queue_vault_settings_offer(window, cx);
        }
        cx.notify();
        true
    }

    fn close_vault(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Err(err) = self.save_all(cx) {
            self.note_status(format!("Could not close folder: {err}"), cx);
            return false;
        }
        self.agent_panel
            .update(cx, |panel, cx| panel.vault_changed(cx));
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
        self.persist_settings(cx);
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
        let bookmark_root = if is_folder {
            self.vault.read(cx).root.clone()
        } else {
            None
        };
        let bookmarked = if is_folder {
            self.settings
                .bookmarks
                .find_kind(&BookmarkKind::Folder {
                    path: path.clone(),
                    root: bookmark_root,
                })
                .is_some()
        } else {
            self.settings
                .bookmarks
                .find_kind(&BookmarkKind::file(path.clone()))
                .is_some()
        };
        let language = self.settings.language;
        let view = cx.entity();
        self.clear_file_menu();

        let menu = PopupMenu::build(window, cx, move |menu, _window, _cx| {
            Self::build_file_menu(menu, path, is_folder, bookmarked, language, view)
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
        bookmarked: bool,
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
                PopupMenuItem::new(language.text("Ask agent about this", "Spør agenten om dette"))
                    .icon(assets::IconName::MessageSquareQuote)
                    .on_click({
                        let view = view.clone();
                        let path = path.clone();
                        move |_, window, _| {
                            let view = view.clone();
                            let path = path.clone();
                            window.on_next_frame(move |window, cx| {
                                view.update(cx, |this, cx| {
                                    this.settings.agent_open = true;
                                    this.persist_settings(cx);
                                    this.agent_panel.update(cx, |panel, cx| {
                                        panel.add_context(path, cx);
                                        panel.focus_composer(window, cx);
                                    });
                                    cx.notify();
                                });
                            });
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("New file here")
                    .icon(assets::IconName::FilePlus)
                    .on_click({
                        let view = view.clone();
                        let dir = dir.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.new_file_in(dir.clone(), window, cx);
                            });
                        }
                    }),
            )
            .item(
                PopupMenuItem::new(language.text("New Canvas here", "Nytt Canvas her"))
                    .icon(assets::IconName::FilePlus)
                    .on_click({
                        let view = view.clone();
                        let dir = dir.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.new_canvas_in(dir.clone(), window, cx);
                            });
                        }
                    }),
            )
            .separator();
        // Files can be pinned into the Starred group.
        let menu = if is_folder {
            menu.item(
                PopupMenuItem::new(
                    language.text("Template for new notes…", "Mal for nye notater…"),
                )
                .icon(assets::IconName::LayoutTemplate)
                .on_click({
                    let view = view.clone();
                    let path = path.clone();
                    move |_, window, _cx| {
                        let view = view.clone();
                        let path = path.clone();
                        window.on_next_frame(move |window, cx| {
                            view.update(cx, |this, cx| {
                                this.show_template_picker(TemplatePick::Folder(path), window, cx);
                            });
                        });
                    }
                }),
            )
            .item(
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
            .item(
                PopupMenuItem::new(if bookmarked {
                    language.text("Remove bookmark", "Fjern bokmerke")
                } else {
                    language.text("Bookmark folder", "Bokmerk mappe")
                })
                .icon(assets::IconName::Folder)
                .on_click({
                    let path = path.clone();
                    let view = view.clone();
                    move |_, _window, cx| {
                        view.update(cx, |this, cx| {
                            this.toggle_folder_bookmark(path.clone(), cx);
                        });
                    }
                }),
            )
        } else {
            menu.item(
                PopupMenuItem::new(if bookmarked {
                    language.text("Remove bookmark", "Fjern bokmerke")
                } else {
                    language.text("Bookmark", "Bokmerk")
                })
                .icon(if bookmarked {
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
            PopupMenuItem::new(language.text("Open in New Window", "Åpne i nytt vindu"))
                .icon(assets::IconName::AppWindow)
                .on_click({
                    let path = path.clone();
                    move |_, window, cx| {
                        let path = path.to_string_lossy().into_owned();
                        window.defer(cx, move |window, cx| {
                            window.dispatch_action(OpenPathInNewWindow { path }.boxed_clone(), cx);
                        });
                    }
                }),
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
                    let view = view.clone();
                    move |_, _window, cx| {
                        view.update(cx, |this, cx| {
                            let target = this.vault_wikilink_target(&path, cx);
                            cx.write_to_clipboard(ClipboardItem::new_string(format!(
                                "[[{target}]]"
                            )));
                        });
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
    pub(crate) fn note_status(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
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

    pub(crate) fn show_cli_results(
        &mut self,
        title: String,
        lines: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut visible = lines
            .iter()
            .take(100)
            .map(|line| line.chars().take(500).collect::<String>())
            .collect::<Vec<_>>();
        if lines.len() > 100 {
            let remaining = lines.len() - 100;
            visible.push(
                if self.settings.language == crate::settings::Language::Norwegian {
                    format!("… og {remaining} til")
                } else {
                    format!("… and {remaining} more")
                },
            );
        }
        let close = self.tr("Close", "Lukk");
        let scroll_handle = ScrollHandle::new();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(title.clone())
                .w(px(560.))
                .child(
                    v_flex()
                        .id("cli-results")
                        .max_h(px(420.))
                        .gap_1()
                        .track_scroll(&scroll_handle)
                        .overflow_y_scroll()
                        .vertical_scrollbar(&scroll_handle)
                        .children(visible.iter().cloned().map(|line| {
                            div().flex_none().text_sm().child(line).into_any_element()
                        })),
                )
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().trigger(|button| button.label(close))),
                )
        });
    }

    pub(crate) fn cli_is_showing_vault(&self, root: &Path, cx: &Context<Self>) -> bool {
        self.vault
            .read(cx)
            .root
            .as_deref()
            .is_some_and(|current| same_canonical_path(current, root))
    }

    pub(crate) fn cli_guard_destination(
        &self,
        destination: &Path,
        cx: &Context<Self>,
    ) -> Result<bool, crate::cli::CliError> {
        let mut open = false;
        for tab in &self.docs {
            let result = tab.entity.read(cx);
            if !same_canonical_path(&result.path, destination) {
                continue;
            }
            open = true;
            if result.is_read_only() {
                return Err(crate::cli::CliError::new(
                    "The note is open read-only; close it before writing",
                    "Notatet er åpnet skrivebeskyttet; lukk det før du skriver",
                ));
            }
            let disk = crate::cli::read_bounded_bytes(&result.path)?;
            crate::cli::preflight_open_buffer(
                result.dirty,
                result.conflict,
                result.editor.read(cx).value().as_bytes(),
                &disk,
            )?;
        }
        Ok(open)
    }

    pub(crate) fn cli_refresh_vault(&mut self, root: &Path, cx: &mut Context<Self>) {
        if self.cli_is_showing_vault(root, cx) {
            self.vault.update(cx, |vault, cx| vault.refresh(cx));
        }
    }

    pub(crate) fn confirm_cli_write(
        &mut self,
        request: crate::cli::CliRequest,
        settings: crate::settings::Settings,
        now: chrono::NaiveDateTime,
        destination: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.settings.language;
        let (title, action) = match &request.command {
            crate::cli::Command::New { .. } => (
                language.text("Create this note?", "Opprette dette notatet?"),
                language.text("Create", "Opprett"),
            ),
            crate::cli::Command::Daily { .. } => (
                language.text(
                    "Create or use today's note?",
                    "Opprette eller bruke dagens notat?",
                ),
                language.text("Continue", "Fortsett"),
            ),
            crate::cli::Command::Append { .. } => (
                language.text("Append to this note?", "Legge til tekst i dette notatet?"),
                language.text("Append", "Legg til"),
            ),
            _ => return,
        };
        let destination_display = destination.display().to_string();
        let description = if language == crate::settings::Language::Norwegian {
            format!("Den forespurte handlingen påvirker:\n{destination_display}")
        } else {
            format!("The requested action will affect:\n{destination_display}")
        };
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let request = request.clone();
            let settings = settings.clone();
            let now = now;
            let view = view.clone();
            let destination = destination.clone();
            dialog
                .title(title)
                .description(description.clone())
                .show_cancel(false)
                .on_ok(|_, _, _| true)
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().trigger(|button| {
                            button
                                .label(language.text("Cancel", "Avbryt"))
                                .with_variant(ButtonVariant::Primary)
                        }))
                        .child(
                            Button::new("confirm-cli-write")
                                .label(action)
                                .with_variant(ButtonVariant::Secondary)
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let request = request.clone();
                                    let settings = settings.clone();
                                    let now = now;
                                    let destination = destination.clone();
                                    let view = view.clone();
                                    cx.defer(move |cx| {
                                        let result = crate::windows::execute_cli_write_confirmed(
                                            request,
                                            destination,
                                            settings,
                                            now,
                                            cx,
                                        );
                                        view.update(cx, |workspace, cx| match result {
                                            Ok(paths) => {
                                                let paths = paths.join(", ");
                                                let message = if language
                                                    == crate::settings::Language::Norwegian
                                                {
                                                    format!("Lagret {paths}")
                                                } else {
                                                    format!("Saved {paths}")
                                                };
                                                workspace.note_status(message, cx)
                                            }
                                            Err(error) => workspace.note_status(
                                                error.message(language).to_owned(),
                                                cx,
                                            ),
                                        });
                                    });
                                }),
                        ),
                )
        });
    }

    pub(crate) fn cli_append_open_document(
        &mut self,
        vault_root: &Path,
        destination: &Path,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<bool, crate::cli::CliError> {
        let matches = self
            .docs
            .iter()
            .filter(|tab| same_canonical_path(&tab.entity.read(cx).path, destination))
            .map(|tab| tab.entity.clone())
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return Ok(false);
        }
        let document = matches.into_iter().next().expect("checked non-empty");
        document.update(cx, |document, cx| {
            let current = document.editor.read(cx).value().to_string();
            let size = current.len().checked_add(text.len());
            if size.is_none_or(|size| size > crate::cli::MAX_NOTE_BYTES) {
                return Err(crate::cli::CliError::new(
                    "The note or appended text exceeds the 2 MiB limit",
                    "Notatet eller den tilføyde teksten overskrider grensen på 2 MiB",
                ));
            }
            let mut replacement = current;
            replacement.push_str(text);
            document.cli_snapshot_before_write(vault_root, replacement.as_bytes())?;
            document
                .editor
                .update(cx, |editor, cx| editor.replace_all(replacement, window, cx));
            document.save_without_history(cx).map_err(|error| {
                crate::cli::CliError::new(
                    format!("Could not save the appended note: {error}"),
                    format!("Kunne ikke lagre notatet med tilføyelsen: {error}"),
                )
            })?;
            Ok(())
        })?;
        Ok(true)
    }

    pub(crate) fn cli_refresh_open_document(
        &mut self,
        destination: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for tab in &self.docs {
            if same_canonical_path(&tab.entity.read(cx).path, destination) {
                tab.entity
                    .update(cx, |document, cx| document.check_external(window, cx));
            }
        }
    }

    pub(crate) fn focus_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            self.refocus(window, cx);
        }
    }

    pub(crate) fn cli_focus_after_mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_active_pane_after_mount(window, cx);
    }

    /// Put keyboard focus back where it belongs after a dialog or sheet
    /// closes: the active editor when a note is open, else the workspace.
    fn refocus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(graph) = &self.graph {
            graph.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        if self.folder.is_some() {
            self.focus_handle.focus(window, cx);
            return;
        }
        if let Some(doc) = self.active_doc() {
            if let Some(canvas) = doc.read(cx).canvas_view.clone() {
                canvas.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
                return;
            }
        }
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

    fn focus_active_pane_after_mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        if let Some(doc) = self.active_doc() {
            if let Some(canvas) = doc.read(cx).canvas_view.clone() {
                canvas.read(cx).focus_handle(cx).focus(window, cx);
                self.pending_editor_focus = None;
                cx.notify();
                return;
            }
        }
        self.pending_editor_focus = self.active_doc().and_then(|doc| {
            (self.graph.is_none()
                && self.folder.is_none()
                && self.settings.view_mode != ViewMode::Preview
                && !doc.read(cx).is_read_only())
            .then(|| (doc.clone(), None))
        });
        cx.notify();
    }

    fn on_vault_event(&mut self, event: &VaultEvent, cx: &mut Context<Self>) {
        if matches!(event, VaultEvent::TreeExpansion) {
            let vault = self.vault.read(cx);
            if let Some(root) = &vault.root {
                self.settings
                    .expanded_folders
                    .insert(root.to_string_lossy().to_string(), vault.expanded_folders());
                self.persist_settings(cx);
            }
            return;
        }
        // Flag docs whose files changed underneath; they reload themselves on
        // the next frame where a window handle is available.
        self.needs_fs_check = true;
        if matches!(event, VaultEvent::Files) {
            self.search_service.invalidate_all();
        }
        cx.notify();
        // An open graph keeps its map in sync — positions carry over
        // so it settles instead of jumping.
        if let Some(graph) = self.graph.clone() {
            graph.update(cx, |g, cx| g.rebuild(cx));
        }
        if let Some(graph) = self.graph_dock.clone() {
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
        if let Some(doc) = self
            .active_doc()
            .filter(|doc| doc.read(cx).is_canvas)
            .cloned()
        {
            if let Some(canvas) = doc.read(cx).canvas_view.clone() {
                canvas.update(cx, |canvas, cx| canvas.begin_search(window, cx));
                return;
            }
        }
        if let Some(pdf) = self.active_doc().and_then(|doc| doc.read(cx).pdf.clone()) {
            pdf.update(cx, |pdf, cx| pdf.focus_search(window, cx));
            return;
        }
        if self.settings.graph_dock_open {
            if let Some(graph) = self.graph_dock.clone() {
                graph.update(cx, |g, cx| g.focus_filter(window, cx));
                return;
            }
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

    pub(crate) fn active_document_path(&self, cx: &App) -> Option<PathBuf> {
        self.active_doc().map(|doc| doc.read(cx).path.clone())
    }

    pub(crate) fn prepare_tab_window_move(
        &mut self,
        token: Option<TabToken>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<crate::windows::TabWindowMove> {
        let token = token.or_else(|| {
            self.active
                .and_then(|index| self.doc_tokens.get(index).copied())
        })?;
        let index = self.doc_index_for_token(token)?;
        let doc = self.docs[index].entity.clone();
        if doc.update(cx, |doc, cx| doc.flush_and_save(cx)).is_err() {
            self.note_status(
                self.tr("Save failed; tab kept.", "Lagring feilet; fane beholdt."),
                cx,
            );
            return None;
        }
        let Some(path) = doc
            .read(cx)
            .path
            .canonicalize()
            .ok()
            .filter(|path| path.is_file())
        else {
            self.note_status(
                self.tr(
                    "File unavailable; tab kept.",
                    "Fil utilgjengelig; fane beholdt.",
                ),
                cx,
            );
            return None;
        };
        let doc = doc.read(cx);
        let editor = doc.editor.read(cx);
        let group = self.layout.group_for(token)?;
        let mode = self
            .layout
            .group_specs()
            .into_iter()
            .find(|(id, _, _, _)| *id == group)?
            .3;
        Some(crate::windows::TabWindowMove {
            token,
            path,
            text: editor.value().to_string(),
            cursor: editor.cursor(),
            pinned: self.is_pinned(&doc.path),
            mode,
        })
    }

    pub(crate) fn accept_tab_window_move(
        &mut self,
        snapshot: &crate::windows::TabWindowMove,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(doc) = self.active_doc().cloned() else {
            return false;
        };
        if !tab_matches_window_move(doc.read(cx), snapshot, cx) {
            return false;
        }
        if snapshot.pinned && !self.is_pinned(&snapshot.path) {
            self.settings
                .pinned_tabs
                .push(snapshot.path.to_string_lossy().into_owned());
        }
        doc.read(cx).editor.clone().update(cx, |editor, cx| {
            editor.set_selected_range(snapshot.cursor..snapshot.cursor, cx);
        });
        self.set_pane_view_mode(self.layout.focused(), snapshot.mode, window, cx);
        self.persist_tabs(cx);
        self.focus_active_pane_after_mount(window, cx);
        true
    }

    pub(crate) fn finish_tab_window_move(
        &mut self,
        snapshot: &crate::windows::TabWindowMove,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self.doc_index_for_token(snapshot.token) else {
            return false;
        };
        let doc = self.docs[index].entity.read(cx);
        if !tab_matches_window_move(doc, snapshot, cx) {
            return false;
        }
        let path = doc.path.to_string_lossy().into_owned();
        self.settings.pinned_tabs.retain(|pinned| pinned != &path);
        self.close_tab_now(index, window, cx);
        true
    }

    /// `(`/`[`/`{`/quotes/etc. — insert the pair, wrap the selection, or
    /// skip the matching closer (Document::insert_pair).
    fn on_pair_insert(&mut self, action: &PairInsert, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.insert_pair(action.pair, window, cx));
        }
    }

    fn on_set_highlight_color(
        &mut self,
        action: &SetHighlightColor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| {
                doc.set_highlight_color(action.emoji, window, cx)
            });
        }
    }

    /// `)`/`]`/`}` — skip over the closer, wrap the selection, or insert the
    /// closer plainly (Document::close_pair).
    fn on_pair_close(&mut self, action: &PairClose, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| doc.close_pair(action.pair, window, cx));
        }
    }

    fn on_spell_correction(
        &mut self,
        action: &SpellCorrection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self
            .active_doc()
            .filter(|doc| doc.entity_id().as_non_zero_u64().get() == action.document_id)
            .cloned()
        else {
            return;
        };
        doc.update(cx, |doc, cx| {
            doc.apply_spell_correction(action, window, cx);
        });
    }

    fn on_spell_ignore(
        &mut self,
        action: &SpellIgnore,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self
            .active_doc()
            .filter(|doc| doc.entity_id().as_non_zero_u64().get() == action.document_id)
            .cloned()
        else {
            return;
        };
        doc.update(cx, |doc, cx| {
            doc.ignore_spell_word(action, cx);
        });
    }

    fn on_spell_learn(
        &mut self,
        action: &SpellLearn,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(doc) = self
            .active_doc()
            .filter(|doc| doc.entity_id().as_non_zero_u64().get() == action.document_id)
            .cloned()
        else {
            return;
        };
        let learned = doc.update(cx, |doc, cx| doc.learn_spell_word(action, cx));
        if learned {
            for open_doc in &self.docs {
                if open_doc.entity.entity_id().as_non_zero_u64().get() != action.document_id {
                    open_doc
                        .entity
                        .update(cx, |doc, cx| doc.dictionary_changed(cx));
                }
            }
        }
    }

    fn on_spell_no_suggestions(
        &mut self,
        _action: &SpellNoSuggestions,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
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
            self.open_document(path.clone(), window, cx);
        } else {
            self.note_status(format!("Could not open {}", path.display()), cx);
        }
    }

    pub(crate) fn cli_open_path(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::cli::CliError> {
        let path = path.canonicalize().map_err(|error| {
            crate::cli::CliError::new(
                format!("Could not open '{}': {error}", path.display()),
                format!("Kunne ikke åpne '{}': {error}", path.display()),
            )
        })?;
        if path.is_dir() {
            if !self.open_vault_at(path.clone(), window, cx) {
                return Err(crate::cli::CliError::new(
                    format!("Could not open folder '{}'", path.display()),
                    format!("Kunne ikke åpne mappen '{}'", path.display()),
                ));
            }
            let opened_root = self.vault.read(cx).root.as_ref() == Some(&path);
            if opened_root {
                Ok(())
            } else {
                Err(crate::cli::CliError::new(
                    format!("Folder '{}' did not open", path.display()),
                    format!("Mappen '{}' ble ikke åpnet", path.display()),
                ))
            }
        } else if path.is_file() {
            self.open_document(path.clone(), window, cx);
            if self.docs.iter().any(|doc| doc.entity.read(cx).path == path) {
                Ok(())
            } else {
                Err(crate::cli::CliError::new(
                    format!("Could not open file '{}'", path.display()),
                    format!("Kunne ikke åpne filen '{}'", path.display()),
                ))
            }
        } else {
            Err(crate::cli::CliError::new(
                format!(
                    "Could not open '{}': it is not a file or folder",
                    path.display()
                ),
                format!(
                    "Kunne ikke åpne '{}': den er ikke en fil eller mappe",
                    path.display()
                ),
            ))
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
            Err(_) => {
                self.note_status(
                    format!(
                        "{}: {}",
                        self.tr(
                            "Could not open this file or folder",
                            "Kunne ikke åpne denne filen eller mappen",
                        ),
                        path.display()
                    ),
                    cx,
                );
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
                if let Some(token) = self.doc_tokens.get(ix).copied() {
                    self.layout.activate(token);
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
                self.focus_active_pane_after_mount(window, cx);
                cx.notify();
                return;
            }
        }
        if !path.is_file() {
            return;
        }
        let initial = match Document::load_initial(&path) {
            Ok(initial) => initial,
            Err(_) => {
                self.note_status(
                    format!(
                        "{}: {}",
                        self.tr(
                            "Could not open this file or folder",
                            "Kunne ikke åpne denne filen eller mappen",
                        ),
                        path.display()
                    ),
                    cx,
                );
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
        if doc.read(cx).is_canvas {
            let workspace = cx.entity().downgrade();
            let document = doc.clone();
            let language = settings.language;
            let vault = self.vault.clone();
            let properties_visibility = settings.properties_visibility;
            let canvas_view = cx.new(|cx| {
                crate::canvas_view::CanvasView::new(
                    document,
                    workspace,
                    vault,
                    language,
                    properties_visibility,
                    window,
                    cx,
                )
            });
            doc.update(cx, |doc, _| doc.canvas_view = Some(canvas_view));
        }
        let sub = cx.subscribe_in(
            &doc,
            window,
            |this, changed_doc, event, window, cx| match event {
                DocumentEvent::Saved | DocumentEvent::Changed => {
                    if matches!(event, DocumentEvent::Saved) {
                        this.add_linked_block_ids(changed_doc, window, cx);
                    }
                    if matches!(event, DocumentEvent::Changed) && changed_doc.read(cx).dirty {
                        if let Some(tab) = this.docs.iter_mut().find(|d| d.entity == *changed_doc) {
                            tab.preview = false;
                        }
                    }
                    this.status_note = None;
                    cx.notify();
                }
                DocumentEvent::Selection => cx.notify(),
                DocumentEvent::ComposerHistoryConflict => {
                    this.note_status(
                        this.tr(
                            "Composer Undo/Redo could not complete safely; the editor was restored.",
                            "Composer-angring eller gjentaking kunne ikke fullføres trygt; redigeringsvinduet ble gjenopprettet.",
                        ),
                        cx,
                    );
                }
                DocumentEvent::ComposerHistoryApplied {
                    direction,
                    changes,
                    path_references,
                } => this.apply_composer_history_changes(
                    *direction,
                    changes,
                    path_references,
                    window,
                    cx,
                ),
                DocumentEvent::SourceScrolled => {
                    let follows_in_group = this
                        .docs
                        .iter()
                        .position(|doc| doc.entity == *changed_doc)
                        .and_then(|ix| this.doc_tokens.get(ix))
                        .and_then(|token| this.layout.group_for(*token))
                        .and_then(|group| {
                            this.layout
                                .group_specs()
                                .into_iter()
                                .find(|(id, _, _, _)| *id == group)
                        })
                        .is_some_and(|(_, _, _, mode)| mode == PaneMode::Split);
                    if this.settings.preview_follows_source && follows_in_group {
                        let doc = changed_doc.clone();
                        // Read visible rows after the editor has laid out its new viewport.
                        window.on_next_frame(move |window, _| {
                            window.on_next_frame(move |_, cx| {
                                doc.update(cx, |doc, cx| doc.follow_source_scroll(cx))
                            });
                        });
                    }
                }
            },
        );
        let editor_focus = doc
            .read(cx)
            .canvas_view
            .clone()
            .map(|view| view.read(cx).focus_handle(cx))
            .unwrap_or_else(|| doc.read(cx).editor.read(cx).focus_handle(cx));
        let focused_document_id = doc.entity_id();
        let focus_sub = cx.on_focus_in(
            &editor_focus,
            window,
            move |this: &mut Workspace, _window, cx| {
                if this.restoring_session {
                    return;
                }
                let Some(index) = this
                    .docs
                    .iter()
                    .position(|open| open.entity.entity_id() == focused_document_id)
                else {
                    return;
                };
                let Some(token) = this.doc_tokens.get(index).copied() else {
                    return;
                };
                let Some(group) = this.layout.group_for(token) else {
                    return;
                };
                if this.layout.select(group, token) {
                    this.active = Some(index);
                    this.sync_active_facade();
                    this.persist_tabs(cx);
                    cx.notify();
                }
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
        let preview_group = self.layout.focused();
        let replacement = preview
            .then(|| {
                self.docs.iter().enumerate().find_map(|(ix, tab)| {
                    (self
                        .doc_tokens
                        .get(ix)
                        .and_then(|token| self.layout.group_for(*token))
                        == Some(preview_group))
                    .then(|| {
                        let doc = tab.entity.read(cx);
                        preview_tab_is_replaceable(
                            tab.preview,
                            doc.dirty,
                            doc.conflict,
                            self.is_pinned(&doc.path),
                        )
                        .then_some(ix)
                    })
                    .flatten()
                })
            })
            .flatten();
        let opened = OpenDoc {
            entity: doc.clone(),
            preview,
            base,
            _sub: sub,
            _focus_sub: focus_sub,
        };
        let (ix, token) = if let Some(ix) = replacement {
            let token = self.doc_tokens[ix];
            self.layout.replace_path(token, path.clone());
            self.docs[ix] = opened;
            (ix, token)
        } else {
            self.docs.push(opened);
            let token = self.layout.new_token(path.clone());
            self.layout.add_tab(self.layout.focused(), token);
            self.doc_tokens.push(token);
            (self.docs.len() - 1, token)
        };
        self.layout.activate(token);
        self.active = Some(ix);
        self.folder = None;
        self.graph = None;
        self.persist_tabs(cx);
        self.reveal_active_file(cx);
        if is_standalone {
            self.start_standalone_fs_check(cx);
        }
        // Keep actions dispatchable until the new editor has mounted.
        self.focus_handle.focus(window, cx);
        cx.notify();

        if let Some(canvas) = doc.read(cx).canvas_view.clone() {
            canvas.read(cx).focus_handle(cx).focus(window, cx);
            self.pending_editor_focus = None;
        } else {
            self.pending_editor_focus = Some((doc, None));
        }
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
        let language = self.settings.language;
        let graph = cx.new(|cx| crate::graph::GraphView::new(weak, vault, language, window, cx));
        graph.update(cx, |g, _cx| g.active = active);
        self.graph = Some(graph);
        self.focus_graph_after_mount(window, cx);
        cx.notify();
    }

    fn focus_graph_after_mount(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(graph) = self.graph.clone() else {
            return;
        };
        self.focus_handle.focus(window, cx);
        cx.on_next_frame(window, move |this, window, cx| {
            if !window.has_active_dialog(cx) && this.graph.as_ref() == Some(&graph) {
                graph.read(cx).focus_handle(cx).focus(window, cx);
            }
        });
    }

    fn ensure_graph_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.graph_dock.is_some() {
            return;
        }
        let workspace = cx.weak_entity();
        let vault = self.vault.clone();
        let active = self
            .active
            .and_then(|ix| self.docs.get(ix))
            .map(|doc| doc.entity.read(cx).path.clone());
        let language = self.settings.language;
        self.graph_dock = Some(cx.new(|cx| {
            crate::graph::GraphView::new_docked(
                workspace,
                vault,
                active.as_deref(),
                language,
                window,
                cx,
            )
        }));
    }

    fn on_toggle_local_graph_panel(
        &mut self,
        _: &ToggleLocalGraphPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.graph_dock_open = !self.settings.graph_dock_open;
        if self.settings.graph_dock_open {
            self.ensure_graph_dock(window, cx);
        } else {
            self.refocus(window, cx);
        }
        self.persist_settings(cx);
        cx.notify();
    }

    fn on_toggle_agent_panel(
        &mut self,
        _: &ToggleAgentPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.agent_open = !self.settings.agent_open;
        self.persist_settings(cx);
        if self.settings.agent_open {
            self.agent_panel
                .update(cx, |panel, cx| panel.focus_composer(window, cx));
        }
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
        let language = self.settings.language;
        let graph = cx
            .new(|cx| crate::graph::GraphView::new_local(weak, vault, &path, language, window, cx));
        self.graph = Some(graph);
        self.focus_graph_after_mount(window, cx);
        cx.notify();
    }

    /// Point an open graph's halo at the currently active doc.
    fn sync_graph_active(&mut self, cx: &mut Context<Self>) {
        let path = self
            .active
            .and_then(|i| self.docs.get(i))
            .map(|d| d.entity.read(cx).path.clone());
        if let Some(graph) = self.graph.clone() {
            // Cached view — notify so the active-note halo moves.
            graph.update(cx, |g, cx| {
                g.active = path.clone();
                cx.notify();
            });
        }
        if let Some(graph) = self.graph_dock.clone() {
            graph.update(cx, |g, cx| g.set_local_center(path.as_deref(), cx));
        }
        self.agent_panel
            .update(cx, |panel, cx| panel.sync_active_path(path, cx));
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
        self.doc_tokens.clear();
        self.layout = EditorLayout::empty();
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
    pub(crate) fn persist_tabs(&mut self, cx: &mut Context<Self>) {
        self.sync_active_facade();
        self.settings.active_folder = self.folder.as_ref().map(|p| p.path.clone());
        self.sync_graph_active(cx);
        for (doc, token) in self.docs.iter().zip(self.doc_tokens.iter().copied()) {
            self.layout
                .replace_path(token, doc.entity.read(cx).path.clone());
        }
        self.settings.open_tabs = self
            .layout
            .group_specs()
            .into_iter()
            .flat_map(|(_, tokens, _, _)| tokens)
            .filter_map(|token| {
                self.doc_tokens
                    .iter()
                    .position(|candidate| *candidate == token)
                    .and_then(|ix| self.docs.get(ix))
                    .map(|doc| doc.entity.read(cx).path.display().to_string())
            })
            .collect();
        self.settings.active_tab = self
            .active
            .and_then(|i| self.docs.get(i))
            .map(|d| d.entity.read(cx).path.display().to_string());
        self.settings.editor_session = Some(self.layout.snapshot());
        if self.restoring_session {
            return;
        }
        self.persist_settings(cx);
    }

    fn sync_active_facade(&mut self) {
        let focused = self.layout.focused();
        let selected = self.layout.selected(focused);
        self.active = selected.and_then(|token| {
            self.doc_tokens
                .iter()
                .position(|candidate| *candidate == token)
        });
        if let Some((_, _, _, mode)) = self
            .layout
            .group_specs()
            .into_iter()
            .find(|(group, _, _, _)| *group == focused)
        {
            self.settings.view_mode = match mode {
                PaneMode::Source => ViewMode::Source,
                PaneMode::Split => ViewMode::Split,
                PaneMode::Preview => ViewMode::Preview,
            };
        }
    }

    fn restore_editor_session(
        &mut self,
        snapshot: Option<crate::editor_layout::Snapshot>,
        legacy_active: Option<&Path>,
        legacy_mode: PaneMode,
        cx: &mut Context<Self>,
    ) {
        let document_paths = self
            .docs
            .iter()
            .map(|doc| doc.entity.read(cx).path.clone())
            .collect::<Vec<_>>();
        if let Some(snapshot) = snapshot {
            let (layout, bindings) = EditorLayout::restore_for_documents(snapshot, &document_paths);
            let docs = std::mem::take(&mut self.docs);
            self.docs.clear();
            self.doc_tokens.clear();
            for (doc, token) in docs.into_iter().zip(bindings) {
                if let Some(token) = token {
                    self.docs.push(doc);
                    self.doc_tokens.push(token);
                }
            }
            self.layout = layout;
            self.sync_active_facade();
            return;
        }
        let (layout, tokens, active) =
            EditorLayout::migrate_legacy(&document_paths, legacy_active, legacy_mode);
        self.layout = layout;
        self.doc_tokens = tokens;
        self.active = active;
        self.sync_active_facade();
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
        self.persist_settings(cx);
        cx.notify();
    }

    fn doc_index_for_token(&self, token: TabToken) -> Option<usize> {
        self.doc_tokens
            .iter()
            .position(|candidate| *candidate == token)
    }

    fn close_tab_token(&mut self, token: TabToken, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.doc_index_for_token(token) {
            self.close_tab_at(index, window, cx);
        }
    }

    fn toggle_pin_token(&mut self, token: TabToken, cx: &mut Context<Self>) {
        if let Some(index) = self.doc_index_for_token(token) {
            self.toggle_pin_at(index, cx);
        }
    }

    /// Close every tab except `keep` — pinned tabs survive (they refuse
    /// inside `close_tab_at`).
    fn close_others_except(&mut self, keep: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(token) = self.doc_tokens.get(keep).copied() else {
            return;
        };
        self.close_others_except_token(token, window, cx);
    }

    fn close_others_except_token(
        &mut self,
        keep: TabToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(group) = self.layout.group_for(keep) else {
            return;
        };
        let candidates = self
            .layout
            .group_tokens(group)
            .into_iter()
            .filter(|token| *token != keep)
            .filter(|token| {
                self.doc_index_for_token(*token)
                    .and_then(|index| self.docs.get(index))
                    .is_some_and(|doc| !self.is_pinned(&doc.entity.read(cx).path))
            })
            .collect::<Vec<_>>();
        let n = candidates.len();
        for token in candidates.into_iter().rev() {
            self.close_tab_token(token, window, cx);
        }
        self.note_status(format!("Closed {n} other tabs"), cx);
    }

    fn close_tabs_right_of(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(token) = self.doc_tokens.get(ix).copied() else {
            return;
        };
        self.close_tabs_right_of_token(token, window, cx);
    }

    fn close_tabs_right_of_token(
        &mut self,
        token: TabToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout.group_for(token).is_none() {
            return;
        }
        for target in self.layout.tokens_to_right(token).into_iter().rev() {
            self.close_tab_token(target, window, cx);
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
        let focused_before = self.layout.focused();
        if let Some(token) = self.doc_tokens.get(ix).copied() {
            if let Some(group) = self.layout.group_for(token) {
                self.layout.focus(group);
                if self.layout.selected(group) == Some(token) {
                    self.active = Some(ix);
                } else if let Some(selected) = self.layout.selected(group) {
                    self.active = self.doc_tokens.iter().position(|item| *item == selected);
                }
            }
        }
        if self.layout.focused() != focused_before {
            self.persist_tabs(cx);
        }
        if self.is_pinned(&self.docs[ix].entity.read(cx).path) {
            self.note_status("Tab is pinned", cx);
            return;
        }
        let save_result = self.docs[ix]
            .entity
            .update(cx, |doc, cx| doc.flush_and_save(cx));
        let title = self.docs[ix].entity.read(cx).title();
        if save_result.is_err() {
            let target = self.docs[ix].entity.clone();
            let view = cx.entity();
            let heading = format!(
                "{} “{}”?",
                self.tr("Discard unsaved changes to", "Forkast ulagrede endringer i"),
                title
            );
            let description = self.tr("Saving failed or the file changed on disk. Cancel to keep your edits.", "Lagring mislyktes eller filen ble endret på disken. Avbryt for å beholde endringene dine.");
            let discard = self.tr("Close without saving", "Lukk uten å lagre");
            window.open_alert_dialog(cx, move |dialog, _window, _cx| {
                let target = target.clone();
                let view = view.clone();
                dialog
                    .title(heading.clone())
                    .description(description)
                    .show_cancel(true)
                    .ok_text(discard)
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

    fn move_tab_to_group(
        &mut self,
        token: TabToken,
        destination: GroupId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source_group = self.layout.group_for(token);
        let source_position = self
            .layout
            .group_tokens(destination)
            .iter()
            .position(|candidate| *candidate == token);
        let insertion = if source_group == Some(destination)
            && source_position.is_some_and(|position| position < index)
        {
            index.saturating_sub(1)
        } else {
            index
        };
        if !self.layout.move_tab(token, destination, insertion) {
            return;
        }
        self.active = self.doc_index_for_token(token);
        self.folder = None;
        self.graph = None;
        self.persist_tabs(cx);
        self.focus_active_pane_after_mount(window, cx);
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
        let closing_active = self.active == Some(ix);
        let closed_focus = self.docs[ix]
            .entity
            .read(cx)
            .editor
            .read(cx)
            .focus_handle(cx);
        let closed_pdf_focus = self.docs[ix]
            .entity
            .read(cx)
            .pdf
            .as_ref()
            .map(|pdf| pdf.read(cx).focus_handle(cx));
        let path = self.docs[ix].entity.read(cx).path.clone();
        let token = self.doc_tokens.get(ix).copied();
        self.docs.remove(ix);
        self.doc_tokens.remove(ix);
        if let Some(token) = token {
            self.layout.close(token);
        }
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
        } else if let Some(selected) = self.layout.selected(self.layout.focused()) {
            self.active = self.doc_tokens.iter().position(|token| *token == selected);
        }
        self.persist_tabs(cx);
        self.reveal_active_file(cx);
        cx.notify();
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                let focused = window.focused(cx);
                if !window.has_active_dialog(cx)
                    && (focused.is_none()
                        || closed_focus.contains_focused(window, cx)
                        || closed_pdf_focus
                            .as_ref()
                            .is_some_and(|focus| focus.contains_focused(window, cx))
                        || (closing_active && focused.as_ref() == Some(&this.focus_handle)))
                {
                    this.focus_active_pane_after_mount(window, cx);
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
        if doc.read(cx).is_canvas {
            return;
        }
        let text = text.to_string();
        doc.update(cx, |doc, cx| {
            doc.editor
                .update(cx, |e, cx| e.insert(text.clone(), window, cx));
        });
    }

    pub(crate) fn save_all(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        for doc in &self.docs {
            doc.entity.update(cx, |doc, cx| doc.flush_and_save(cx))?;
        }
        Ok(())
    }

    fn composer_change_conflicts(&self, changes: &[(PathBuf, Vec<u8>, Vec<u8>)], cx: &App) -> bool {
        self.docs.iter().any(|open| {
            let doc = open.entity.read(cx);
            let path = doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone());
            changes.iter().any(|(changed_path, before, _)| {
                path == *changed_path
                    && (doc.dirty
                        || doc.conflict
                        || doc.composer_source(cx).as_bytes() != before
                        || doc.composer_disk_baseline().as_deref() != Some(before.as_slice())
                        || std::fs::read(&path).ok().as_deref() != Some(before.as_slice()))
            })
        })
    }

    fn composer_history_buffer_conflict(
        &self,
        owner: Option<&Entity<Document>>,
        changes: &[crate::document::ComposerFileChange],
        direction: crate::document::ComposerHistoryDirection,
        cx: &mut Context<Self>,
    ) -> bool {
        let conflicts = |workspace: &Workspace, owner: Option<&Entity<Document>>, cx: &App| {
            changes.iter().any(|(path, before, after)| {
                if !crate::note_composer::is_composer_reference_document(path) {
                    return false;
                }
                let expected = match direction {
                    crate::document::ComposerHistoryDirection::Undo => after,
                    crate::document::ComposerHistoryDirection::Redo => before,
                };
                workspace.docs.iter().any(|open| {
                    if owner.is_some_and(|owner| open.entity.entity_id() == owner.entity_id()) {
                        return false;
                    }
                    let document = open.entity.read(cx);
                    if document
                        .path
                        .canonicalize()
                        .unwrap_or_else(|_| document.path.clone())
                        != *path
                    {
                        return false;
                    }
                    let current = document.composer_source(cx);
                    crate::document::composer_history_buffer_conflicts(
                        expected.as_deref(),
                        current.as_bytes(),
                        document.composer_disk_baseline().as_deref(),
                        document.dirty,
                        document.conflict,
                    ) || expected.as_deref().is_some_and(|expected| {
                        std::fs::read(path).ok().as_deref() != Some(expected)
                    })
                })
            })
        };
        if conflicts(self, owner, cx) {
            return true;
        }
        let peers = cx.global::<crate::windows::WindowManager>().records.clone();
        peers
            .iter()
            .filter(|peer| peer.id != self.window_id)
            .any(|peer| {
                let conflict = peer.window.update(cx, |_, _window, cx| {
                    peer.workspace
                        .update(cx, |workspace, cx| conflicts(workspace, None, cx))
                });
                !matches!(conflict, Ok(Ok(false)))
            })
    }

    fn apply_composer_changes(
        &mut self,
        changes: &[(PathBuf, Vec<u8>, Vec<u8>)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (path, before, after) in changes {
            let text = match String::from_utf8(after.clone()) {
                Ok(text) => text,
                Err(_) => continue,
            };
            let affected = self
                .docs
                .iter()
                .filter(|open| {
                    let doc = open.entity.read(cx);
                    doc.path.canonicalize().unwrap_or_else(|_| doc.path.clone()) == *path
                })
                .map(|open| open.entity.clone())
                .collect::<Vec<_>>();
            for entity in affected {
                let current = entity.read(cx).composer_source(cx);
                if current.as_bytes() != before {
                    continue;
                }
                entity.update(cx, |doc, cx| {
                    if doc.is_canvas {
                        doc.apply_composer_canvas_snapshot(before, after.clone(), window, cx);
                    } else {
                        doc.replace_range(0..current.len(), text.clone(), window, cx);
                        doc.mark_composer_written(after.clone(), cx);
                    }
                });
            }
        }
    }

    fn apply_composer_history_changes(
        &mut self,
        direction: crate::document::ComposerHistoryDirection,
        changes: &[crate::document::ComposerFileChange],
        path_references: &crate::document::ComposerPathReferenceDelta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut text_changes = Vec::new();
        let mut deleted_paths = Vec::new();
        for (path, before, after) in changes {
            if !crate::note_composer::is_composer_reference_document(path) {
                continue;
            }
            let (expected, desired) = match direction {
                crate::document::ComposerHistoryDirection::Undo => (after, before),
                crate::document::ComposerHistoryDirection::Redo => (before, after),
            };
            match (expected, desired) {
                (Some(expected), Some(desired)) => {
                    text_changes.push((path.clone(), expected.clone(), desired.clone()));
                }
                (Some(_), None) => deleted_paths.push(path.clone()),
                _ => {}
            }
        }
        self.apply_composer_path_references(direction, path_references, cx);
        self.close_composer_deleted_paths(&deleted_paths, window, cx);
        self.apply_composer_changes(&text_changes, window, cx);
        self.vault.update(cx, |vault, cx| vault.refresh(cx));

        let peers = cx.global::<crate::windows::WindowManager>().records.clone();
        for peer in peers.iter().filter(|peer| peer.id != self.window_id) {
            let _ = peer.window.update(cx, |_, window, cx| {
                peer.workspace.update(cx, |workspace, cx| {
                    workspace.apply_composer_path_references(direction, path_references, cx);
                    workspace.close_composer_deleted_paths(&deleted_paths, window, cx);
                    workspace.apply_composer_changes(&text_changes, window, cx);
                    workspace.vault.update(cx, |vault, cx| vault.refresh(cx));
                })
            });
        }
    }

    fn apply_composer_path_references(
        &mut self,
        direction: crate::document::ComposerHistoryDirection,
        delta: &crate::document::ComposerPathReferenceDelta,
        cx: &mut Context<Self>,
    ) {
        let Some(delta) = delta
            .workspaces
            .iter()
            .find(|delta| delta.window_id == self.window_id)
        else {
            return;
        };
        for (id, before, after) in &delta.bookmarks {
            let (expected_kind, desired_kind) = match direction {
                crate::document::ComposerHistoryDirection::Undo => (after, before),
                crate::document::ComposerHistoryDirection::Redo => (before, after),
            };
            if let Some(bookmark) = self.settings.bookmarks.find_mut(*id) {
                if &bookmark.kind == expected_kind {
                    bookmark.kind = desired_kind.clone();
                }
            }
        }
        apply_composer_path_list(
            &mut self.settings.pinned_tabs,
            &delta.pinned_tabs,
            direction,
        );
        apply_composer_path_list(&mut self.recent, &delta.recent, direction);
        apply_composer_path_list(&mut self.closed_tabs, &delta.closed_tabs, direction);
        apply_composer_path_list(&mut self.nav_stack, &delta.navigation, direction);
        self.settings.starred = self.settings.bookmarks.starred_paths();
        let starred = self.settings.starred.iter().cloned().collect();
        self.vault.update(cx, |vault, cx| {
            vault.starred = starred;
            cx.emit(VaultEvent::Starred);
        });
        self.persist_settings(cx);
        self.persist_tabs(cx);
    }

    fn close_composer_deleted_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let deleted = paths
            .iter()
            .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
            .collect::<HashSet<_>>();
        let indices = self
            .docs
            .iter()
            .enumerate()
            .filter_map(|(index, open)| {
                let path = open.entity.read(cx).path.clone();
                deleted
                    .contains(&path.canonicalize().unwrap_or(path))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        for index in indices.into_iter().rev() {
            if let Some(open) = self.docs.get(index) {
                open.entity.update(cx, |doc, _| doc.cancel_composer_save());
            }
            self.close_tab_now(index, window, cx);
        }
        self.closed_tabs.retain(|path| {
            !deleted.contains(&path.canonicalize().unwrap_or_else(|_| path.clone()))
        });
    }

    pub(crate) fn flush_path_for_new_window(
        &mut self,
        path: &Path,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> std::io::Result<()> {
        for doc in &self.docs {
            let document_path = doc.entity.read(cx).path.clone();
            if document_path.canonicalize().unwrap_or(document_path) == path {
                doc.entity.update(cx, |doc, cx| doc.flush_and_save(cx))?;
            }
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
        let view = cx.entity().downgrade();
        let language = self.settings.language;
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(language.text("New file", "Ny fil"))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("new-file-note")
                                .label(language.text("Note", "Notat"))
                                .on_click({
                                    let view = view.clone();
                                    let root = root.clone();
                                    move |_, window, cx| {
                                        let view = view.clone();
                                        let root = root.clone();
                                        window.close_dialog(cx);
                                        window.defer(cx, move |window, cx| {
                                            let _ = view.update(cx, |this, cx| {
                                                this.new_file_in(root, window, cx)
                                            });
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("new-file-canvas")
                                .label(language.text("Canvas", "Canvas"))
                                .on_click({
                                    let view = view.clone();
                                    let root = root.clone();
                                    move |_, window, cx| {
                                        let view = view.clone();
                                        let root = root.clone();
                                        window.close_dialog(cx);
                                        window.defer(cx, move |window, cx| {
                                            let _ = view.update(cx, |this, cx| {
                                                this.new_canvas_in(root, window, cx)
                                            });
                                        });
                                    }
                                }),
                        ),
                )
                .on_ok({
                    let view = view.clone();
                    let root = root.clone();
                    move |_, window, cx| {
                        let view = view.clone();
                        let root = root.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = view.update(cx, |this, cx| this.new_file_in(root, window, cx));
                        });
                        true
                    }
                })
        });
    }

    fn on_new_canvas(&mut self, _: &NewCanvas, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.on_open_folder(&OpenFolder, window, cx);
            return;
        };
        self.new_canvas_in(root, window, cx);
    }

    pub(crate) fn new_canvas_in(
        &mut self,
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut index = 0;
        loop {
            let name = if index == 0 {
                "Untitled.canvas".to_string()
            } else {
                format!("Untitled {index}.canvas")
            };
            let path = dir.join(name);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    if std::io::Write::write_all(&mut file, br#"{"nodes":[],"edges":[]}"#).is_err()
                    {
                        let _ = std::fs::remove_file(&path);
                        self.note_status(
                            self.settings
                                .language
                                .text("Could not create Canvas.", "Kunne ikke opprette Canvas."),
                            cx,
                        );
                        return;
                    }
                    self.vault.update(cx, |vault, cx| vault.refresh(cx));
                    self.open_document(path, window, cx);
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    index += 1;
                }
                Err(_) => {
                    self.note_status(
                        self.settings
                            .language
                            .text("Could not create Canvas.", "Kunne ikke opprette Canvas."),
                        cx,
                    );
                    return;
                }
            }
        }
    }

    /// Create a uniquely-named empty note in `dir` and open it.
    fn new_file_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.new_note_in(dir, Vec::new(), window, cx);
    }

    /// Palette "Open random note" — any Markdown note except the
    /// active one and templates.
    fn open_random_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.note_status("Open a folder first", cx);
            return;
        };
        let templates = root.join(&self.settings.templates_dir);
        let active = self.active_doc().map(|d| d.read(cx).path.clone());
        let pool: Vec<PathBuf> = self
            .vault
            .read(cx)
            .notes
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .filter(|p| !p.starts_with(&templates) && Some(*p) != active.as_ref())
            .cloned()
            .collect();
        if pool.is_empty() {
            self.note_status("No other notes to pick from", cx);
            return;
        }
        use std::hash::BuildHasher;
        let n =
            std::collections::hash_map::RandomState::new().hash_one(std::time::SystemTime::now());
        let path = pool[(n % pool.len() as u64) as usize].clone();
        self.open_document(path, window, cx);
    }

    /// Palette "New unique note" — `<unique_note_dir>/<stamp>.md` with
    /// a Moment-format timestamp name (the Zettelkasten prefix), seeded
    /// from `<templates_dir>/unique.md` when that exists.
    fn new_unique_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.note_status("Open a folder first", cx);
            return;
        };
        let now = chrono::Local::now().naive_local();
        let stamp = crate::bases::format_moment(now, &self.settings.unique_note_format);
        let path = free_path(
            &root.join(&self.settings.unique_note_dir),
            &format!("{stamp}.md"),
        );
        let template = root.join(&self.settings.templates_dir).join("unique.md");
        self.create_note(path, Some(template), now, window, cx);
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
        if fm.is_empty() {
            if let Some(template) = self.folder_template(&dir, cx) {
                let now = chrono::Local::now().naive_local();
                self.create_note(path, Some(template), now, window, cx);
                return;
            }
        }
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
        self.persist_settings(cx);
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

    fn daily_path_for(&self, date: NaiveDate, cx: &App) -> Option<PathBuf> {
        self.period_path(Period::Day, date, cx)
    }

    /// `<dir>/<format>.md` for the `period` note holding `date` — the
    /// format setting is Moment syntax (`YYYY-MM-DD`, `gggg-[W]ww`).
    fn period_path(&self, period: Period, date: NaiveDate, cx: &App) -> Option<PathBuf> {
        let root = self.vault.read(cx).root.clone()?;
        let (dir, format) = self.settings.period(period);
        let name = crate::bases::format_moment(date.and_hms_opt(0, 0, 0)?, format);
        let relative = if dir.is_empty() {
            format!("{name}.md")
        } else {
            format!("{dir}/{name}.md")
        };
        crate::vault_settings_import::checked_vault_path(&root, &relative)
    }

    /// The period and a date of the note at `path`, when it is a
    /// daily or periodic note — searched within 20 years of today.
    fn period_of(&self, path: &Path, cx: &App) -> Option<(Period, NaiveDate)> {
        let today = chrono::Local::now().date_naive();
        Period::ALL.into_iter().find_map(|period| {
            let span = match period {
                Period::Day => 7305,
                Period::Week => 1044,
                Period::Month => 240,
                Period::Quarter => 80,
                Period::Year => 20,
            };
            (0..=span)
                .flat_map(|k| [k, -k])
                .map(|k| period.shift(today, k))
                .find(|&date| self.period_path(period, date, cx).as_deref() == Some(path))
                .map(|date| (period, date))
        })
    }

    fn open_period_today(&mut self, period: Period, window: &mut Window, cx: &mut Context<Self>) {
        self.open_period_at(period, chrono::Local::now().date_naive(), window, cx);
    }

    /// Palette "Previous/Next periodic note" — the neighbouring note of
    /// the same period as the active daily/weekly/… note.
    fn open_adjacent_period(&mut self, step: i32, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.active_doc().map(|d| d.read(cx).path.clone()) else {
            self.note_status("No note open", cx);
            return;
        };
        match self.period_of(&path, cx) {
            Some((period, date)) => {
                self.open_period_at(period, period.shift(date, step), window, cx)
            }
            None => self.note_status("This isn't a daily or periodic note", cx),
        }
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

    /// Initial content for a daily note created without asking —
    /// `<templates_dir>/<template>` with prompts left at their defaults,
    /// else a plain heading.
    fn template_seed(
        &self,
        path: &Path,
        template: &str,
        date: chrono::NaiveDateTime,
        cx: &App,
    ) -> Result<String, ()> {
        let heading = || {
            Ok(format!(
                "# {}\n\n",
                path.file_stem().unwrap_or_default().to_string_lossy()
            ))
        };
        let Some(template_path) = self.vault.read(cx).root.as_ref().and_then(|root| {
            let selected = self.settings.period_templates.get("daily");
            let template_path = match selected {
                Some(selected) if selected.is_empty() => None,
                Some(selected) => crate::vault_settings_import::checked_vault_path(root, selected),
                None => {
                    let folder = self.settings.templates_dir.trim_matches('/');
                    let relative = if folder.is_empty() {
                        template.to_string()
                    } else {
                        format!("{folder}/{template}")
                    };
                    crate::vault_settings_import::checked_vault_path(root, &relative)
                }
            };
            template_path
        }) else {
            return heading();
        };
        let Ok(mut file) = std::fs::File::open(template_path) else {
            return heading();
        };
        let mut bytes = Vec::new();
        if std::io::Read::take(&mut file, crate::templater::MAX_EXPANSION_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() > crate::templater::MAX_EXPANSION_BYTES
        {
            return Err(());
        }
        let Ok(template) = String::from_utf8(bytes) else {
            return heading();
        };
        let job = TemplateJob {
            template,
            path: path.to_path_buf(),
            now: date,
            frontmatter: None,
            selection: String::new(),
        };
        let expanded = self.expand_template_for(&job, &[], cx);
        if expanded.overflowed {
            Err(())
        } else {
            Ok(expanded.text)
        }
    }

    pub(crate) fn open_daily_at(
        &mut self,
        date: NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_period_at(Period::Day, date, window, cx);
    }

    /// Open the `period` note holding `date`, creating it first from
    /// `<templates_dir>/<period>.md` (`daily.md`, `weekly.md`, …) with
    /// `{{date}}` set to that date.
    pub(crate) fn open_period_at(
        &mut self,
        period: Period,
        date: NaiveDate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.period_path(period, date, cx) else {
            self.note_status("Open a folder first", cx);
            return;
        };
        if path.exists() {
            self.open_document(path, window, cx);
            return;
        }
        let when = date.and_time(chrono::Local::now().time());
        let template = self.vault.read(cx).root.clone().and_then(|root| {
            match self.settings.period_templates.get(period.name()) {
                Some(template) if template.is_empty() => None,
                Some(template) => crate::vault_settings_import::checked_vault_path(&root, template),
                None => {
                    let folder = self.settings.templates_dir.trim_matches('/');
                    let filename = format!("{}.md", period.name());
                    let relative = if folder.is_empty() {
                        filename
                    } else {
                        format!("{folder}/{filename}")
                    };
                    crate::vault_settings_import::checked_vault_path(&root, &relative)
                }
            }
        });
        self.create_note(path, template, when, window, cx);
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
        if !path.exists() {
            let seed = match self.template_seed(
                path,
                "daily.md",
                chrono::Local::now().naive_local(),
                cx,
            ) {
                Ok(seed) => seed,
                Err(()) => {
                    self.note_status(
                        self.tr(
                            "Template output exceeded the size limit; the note was not created.",
                            "Malutdata overskred størrelsesgrensen; notatet ble ikke opprettet.",
                        ),
                        cx,
                    );
                    return;
                }
            };
            if path
                .parent()
                .is_some_and(|dir| std::fs::create_dir_all(dir).is_err())
                || std::fs::write(path, seed).is_err()
            {
                self.note_status("Could not create daily note", cx);
                return;
            }
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
            if doc.read(cx).is_read_only() {
                self.note_status(
                    self.tr(
                        "This document is read-only and cannot be saved as text",
                        "Dette dokumentet er skrivebeskyttet og kan ikke lagres som tekst",
                    ),
                    cx,
                );
                return;
            }
            if let Err(err) = doc.update(cx, |doc, cx| doc.save(cx)) {
                let message = if doc.read(cx).conflict {
                    self.tr(
                        "Saving paused. Save a copy or reload from disk.",
                        "Lagring er satt på pause. Lagre en kopi eller last inn fra disk.",
                    )
                    .to_string()
                } else {
                    format!("{}: {err}", self.tr("Could not save", "Kunne ikke lagre"))
                };
                self.note_status(message, cx);
            }
        }
    }

    fn on_save_as(&mut self, _: &SaveFileAs, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        if doc.read(cx).is_read_only() {
            self.note_status(
                self.tr(
                    "This document is read-only and cannot be saved as text",
                    "Dette dokumentet er skrivebeskyttet og kan ikke lagres som tekst",
                ),
                cx,
            );
            return;
        }
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
                        let source_path = doc.read(cx).path.clone();
                        if crate::canvas::rejects_canvas_kind_change(
                            &source_path,
                            &path,
                            false,
                        ) {
                            this.note_status(
                                this.tr(
                                    "Save As cannot convert between Canvas and text documents.",
                                    "Lagre som kan ikke konvertere mellom Canvas- og tekstdokumenter.",
                                ),
                                cx,
                            );
                            return;
                        }
                        match doc.update(cx, |doc, cx| doc.save_as(path, cx)) {
                            Err(err) => {
                                this.note_status(format!("{}: {err}", this.tr("Could not save", "Kunne ikke lagre")), cx);
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
                                this.note_status(format!("{} {}", this.tr("Saved", "Lagret"), saved_path.display()), cx);
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
        let language = self.settings.language;
        let view = cx.entity();
        self.refocus(window, cx);
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            let close_view = view.clone();
            let doc = doc.clone();
            dialog
                .title(match language {
                    crate::settings::Language::Norwegian => format!("Last inn «{title}» fra disk?"),
                    _ => format!("Reload “{title}” from disk?"),
                })
                .description(language.text(
                    "This will discard your local changes.",
                    "Dette forkaster de lokale endringene dine.",
                ))
                .show_cancel(true)
                .cancel_text(language.text("Cancel", "Avbryt"))
                .ok_text(language.text("Discard and reload", "Forkast og last inn"))
                .ok_variant(gpui_kit::component::button::ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    let reload_result = doc.update(cx, |doc, cx| doc.reload(window, cx));
                    view.update(cx, |this, cx| match reload_result {
                        Ok(()) => this
                            .note_status(this.tr("Reloaded from disk", "Lastet inn fra disk"), cx),
                        Err(err) => this.note_status(
                            format!(
                                "{}: {err}",
                                this.tr("Could not reload", "Kunne ikke laste inn på nytt")
                            ),
                            cx,
                        ),
                    });
                    true
                })
                .on_close(move |_, window, cx| {
                    close_view.update(cx, |this, cx| {
                        this.focus_active_pane_after_mount(window, cx);
                    });
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
        if doc.read(cx).is_canvas {
            return;
        }
        let Some((body, range)) = doc.read(cx).selected_text(cx) else {
            self.note_status(
                self.tr("Select some text first.", "Marker først litt tekst."),
                cx,
            );
            return;
        };
        let source_before = doc.read(cx).composer_source(cx);
        let language = self.settings.language;
        let input = self.rename_input.clone();
        input.update(cx, |input, cx| {
            input.set_placeholder(language.text("Name", "Navn"), window, cx);
            input.set_value("", window, cx);
        });
        let view = cx.entity();

        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(language.text("Extract to new note", "Trekk ut til nytt notat"))
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .trigger(|button| button.label(language.text("Cancel", "Avbryt"))),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("composer-extract-confirm")
                                    .label(language.text("Extract", "Trekk ut"))
                                    .primary(),
                            ),
                        ),
                )
                .on_ok({
                    let view = view.clone();
                    let doc = doc.clone();
                    let range = range.clone();
                    let body = body.clone();
                    let source_before = source_before.clone();
                    move |_, window, cx| {
                        let committed = view.update(cx, |this, cx| {
                            this.commit_extract(
                                doc.clone(),
                                source_before.clone(),
                                range.clone(),
                                body.clone(),
                                window,
                                cx,
                            )
                        });
                        if committed {
                            window.close_dialog(cx);
                            let deferred_view = view.clone();
                            window.defer(cx, move |window, cx| {
                                deferred_view.update(cx, |this, cx| {
                                    this.focus_active_pane_after_mount(window, cx)
                                });
                            });
                        }
                        false
                    }
                })
        });

        let input = self.rename_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx));
        });
    }

    fn show_extract_heading_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            return;
        };
        if doc.read(cx).is_canvas {
            return;
        }
        let Ok(Some((range, _))) = doc.read(cx).composer_heading(cx) else {
            self.note_status(
                self.tr(
                    "Place the caret inside a heading.",
                    "Plasser markøren i en overskrift.",
                ),
                cx,
            );
            return;
        };
        let source_before = doc.read(cx).composer_source(cx);
        let body = source_before[range.clone()].to_string();
        self.show_extract_dialog_for(doc, source_before, range, body, window, cx);
    }

    fn show_extract_dialog_for(
        &mut self,
        doc: Entity<Document>,
        source_before: String,
        range: Range<usize>,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.rename_input.clone();
        let language = self.settings.language;
        input.update(cx, |input, cx| {
            input.set_placeholder(language.text("Name", "Navn"), window, cx);
            input.set_value("", window, cx);
        });
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(language.text("Extract to new note", "Trekk ut til nytt notat"))
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .trigger(|button| button.label(language.text("Cancel", "Avbryt"))),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("composer-extract-confirm")
                                    .label(language.text("Extract", "Trekk ut"))
                                    .primary(),
                            ),
                        ),
                )
                .on_ok({
                    let view = view.clone();
                    let doc = doc.clone();
                    let range = range.clone();
                    let body = body.clone();
                    let source_before = source_before.clone();
                    move |_, window, cx| {
                        let committed = view.update(cx, |this, cx| {
                            this.commit_extract(
                                doc.clone(),
                                source_before.clone(),
                                range.clone(),
                                body.clone(),
                                window,
                                cx,
                            )
                        });
                        if committed {
                            window.close_dialog(cx);
                            let deferred_view = view.clone();
                            window.defer(cx, move |window, cx| {
                                deferred_view.update(cx, |this, cx| {
                                    this.focus_active_pane_after_mount(window, cx)
                                });
                            });
                        }
                        false
                    }
                })
        });
        let input = self.rename_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx))
        });
    }

    fn show_merge_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.active_doc().cloned() else {
            return;
        };
        if source.read(cx).is_canvas {
            return;
        }
        let language = self.settings.language;
        let order = Rc::new(RefCell::new(crate::note_composer::MergeOrder::Append));
        let tab_order = order.clone();
        let input = self.rename_input.clone();
        let target_input = input.clone();
        let source_path = source.read(cx).path.clone();
        let root = self.vault.read(cx).root.clone();
        let targets = self
            .vault
            .read(cx)
            .notes
            .iter()
            .filter(|path| {
                path.as_path() != source_path.as_path()
                    && path.extension().is_some_and(|extension| extension == "md")
            })
            .filter_map(|path| {
                root.as_ref()
                    .and_then(|root| path.strip_prefix(root).ok())
                    .map(|relative| relative.to_string_lossy().replace('\\', "/"))
            })
            .take(100)
            .collect::<Vec<_>>();
        input.update(cx, |input, cx| {
            input.set_placeholder(language.text("Name", "Navn"), window, cx);
            input.set_value("", window, cx);
        });
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let target_rows = targets
                .iter()
                .enumerate()
                .map(|(index, target)| {
                    let value = target.clone();
                    let input = target_input.clone();
                    Button::new(format!("composer-merge-target-{index}"))
                        .label(target.clone())
                        .on_click(move |_, window, cx| {
                            input.update(cx, |input, cx| input.set_value(value.clone(), window, cx))
                        })
                })
                .collect::<Vec<_>>();
            dialog
                .title(language.text(
                    "Merge current note into…",
                    "Slå sammen gjeldende notat med…",
                ))
                .w(px(440.))
                .child(
                    v_flex()
                        .gap_2()
                        .child(Input::new(&input).appearance(true))
                        .child(
                            div()
                                .text_xs()
                                .text_color(_cx.theme().muted_foreground)
                                .child(language.text(
                                    "Enter an existing Markdown path relative to the vault.",
                                    "Skriv inn en eksisterende Markdown-sti relativt til hvelvet.",
                                )),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(_cx.theme().muted_foreground)
                                .child(language.text("Choose a target", "Velg et mål")),
                        )
                        .child(
                            v_flex()
                                .max_h(px(160.))
                                .overflow_y_scrollbar()
                                .gap_1()
                                .children(target_rows),
                        )
                        .child(
                            ButtonGroup::new("note-merge-order")
                                .child(
                                    Button::new("note-merge-append")
                                        .label(language.text("Append", "Legg til sist"))
                                        .selected(
                                            *tab_order.borrow()
                                                == crate::note_composer::MergeOrder::Append,
                                        ),
                                )
                                .child(
                                    Button::new("note-merge-prepend")
                                        .label(language.text("Prepend", "Legg til først"))
                                        .selected(
                                            *tab_order.borrow()
                                                == crate::note_composer::MergeOrder::Prepend,
                                        ),
                                )
                                .on_click({
                                    let tab_order = tab_order.clone();
                                    move |indices, window, _| {
                                        *tab_order.borrow_mut() = if indices.contains(&1) {
                                            crate::note_composer::MergeOrder::Prepend
                                        } else {
                                            crate::note_composer::MergeOrder::Append
                                        };
                                        window.refresh();
                                    }
                                }),
                        ),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new()
                                .trigger(|button| button.label(language.text("Cancel", "Avbryt"))),
                        )
                        .child(
                            DialogAction::new().child(
                                Button::new("composer-merge-confirm")
                                    .label(language.text("Merge", "Slå sammen"))
                                    .primary(),
                            ),
                        ),
                )
                .on_ok({
                    let view = view.clone();
                    let dialog_input = input.clone();
                    let dialog_source = source.clone();
                    let order = order.clone();
                    move |_, window, cx| {
                        let target = dialog_input.read(cx).value().to_string();
                        if target.trim().is_empty() {
                            return false;
                        }
                        let order = *order.borrow();
                        let title = language.text(
                            "Merge this note into the selected target?",
                            "Slå sammen dette notatet med det valgte målet?",
                        );
                        let description = language.text(
                            "The source note will be moved to the vault trash.",
                            "Kildenotatet flyttes til hvelvets papirkurv.",
                        );
                        let deferred_view = view.clone();
                        let deferred_source = dialog_source.clone();
                        let deferred_target = target.clone();
                        window.close_dialog(cx);
                        window.defer(cx, move |window, cx| {
                            deferred_view.update(cx, |this, cx| this.refocus(window, cx));
                            window.open_alert_dialog(cx, move |dialog, _window, _cx| {
                                let view = deferred_view.clone();
                                let dialog_source = deferred_source.clone();
                                let target = deferred_target.clone();
                                dialog
                                    .title(title)
                                    .description(description)
                                    .show_cancel(true)
                                    .cancel_text(language.text("Cancel", "Avbryt"))
                                    .ok_text(language.text("Merge", "Slå sammen"))
                                    .on_ok(move |_, window, cx| {
                                        // Animated dismissal would later restore focus to
                                        // the source editor that this merge closes.
                                        window.close_dialog(cx);
                                        view.update(cx, |this, cx| {
                                            this.commit_merge(
                                                dialog_source.clone(),
                                                target.clone(),
                                                order,
                                                window,
                                                cx,
                                            )
                                        });
                                        let deferred_view = view.clone();
                                        window.defer(cx, move |window, cx| {
                                            deferred_view.update(cx, |this, cx| {
                                                this.focus_active_pane_after_mount(window, cx)
                                            });
                                        });
                                        false
                                    })
                            });
                        });
                        false
                    }
                })
        });
        let input = self.rename_input.clone();
        window.defer(cx, move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx))
        });
    }

    fn commit_merge(
        &mut self,
        source: Entity<Document>,
        target_name: String,
        order: crate::note_composer::MergeOrder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let Ok(root) = root.canonicalize() else {
            self.note_status(
                self.tr(
                    "The vault path is unavailable.",
                    "Hvelvstien er utilgjengelig.",
                ),
                cx,
            );
            return;
        };
        let source_path = source
            .read(cx)
            .path
            .canonicalize()
            .unwrap_or_else(|_| source.read(cx).path.clone());
        let target_name = target_name.trim();
        let mut target_relative = PathBuf::from(target_name);
        if target_relative.extension().is_none() {
            target_relative.set_extension("md");
        }
        if target_relative.is_absolute()
            || target_relative.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
            || target_relative
                .extension()
                .is_none_or(|extension| extension != "md")
        {
            self.note_status(
                self.tr(
                    "Choose an existing Markdown note inside this vault.",
                    "Velg et eksisterende Markdown-notat i dette hvelvet.",
                ),
                cx,
            );
            return;
        }
        let Ok(target_path) = root.join(target_relative).canonicalize() else {
            self.note_status(
                self.tr(
                    "Choose an existing Markdown note inside this vault.",
                    "Velg et eksisterende Markdown-notat i dette hvelvet.",
                ),
                cx,
            );
            return;
        };
        if !source_path.starts_with(&root)
            || !target_path.starts_with(&root)
            || source_path
                .extension()
                .is_none_or(|extension| extension != "md")
            || target_path
                .extension()
                .is_none_or(|extension| extension != "md")
        {
            self.note_status(
                self.tr(
                    "Choose Markdown notes inside this vault.",
                    "Velg Markdown-notater i dette hvelvet.",
                ),
                cx,
            );
            return;
        }
        if source_path == target_path {
            self.note_status(
                self.tr(
                    "A note cannot be merged into itself.",
                    "Et notat kan ikke slås sammen med seg selv.",
                ),
                cx,
            );
            return;
        }
        if source.update(cx, |doc, cx| doc.flush_and_save(cx)).is_err() {
            self.note_status(
                self.tr(
                    "The source note could not be flushed safely; the merge was cancelled.",
                    "Kildenotatet kunne ikke lagres trygt; sammenslåingen ble avbrutt.",
                ),
                cx,
            );
            return;
        }
        let source_text = source.read(cx).composer_source(cx);
        let source_bytes = source_text.as_bytes().to_vec();
        if source.read(cx).dirty
            || source.read(cx).conflict
            || source.read(cx).composer_disk_baseline().as_deref() != Some(source_bytes.as_slice())
            || std::fs::read(&source_path).ok().as_deref() != Some(source_bytes.as_slice())
        {
            self.note_status(
                self.tr(
                    "The source note changed on disk; the merge was cancelled.",
                    "Kildenotatet ble endret på disken; sammenslåingen ble avbrutt.",
                ),
                cx,
            );
            return;
        }
        let source_views = self
            .docs
            .iter()
            .filter(|open| {
                open.entity
                    .read(cx)
                    .path
                    .canonicalize()
                    .unwrap_or_else(|_| open.entity.read(cx).path.clone())
                    == source_path
            })
            .map(|open| open.entity.clone())
            .collect::<Vec<_>>();
        if source_views.iter().any(|view| {
            let doc = view.read(cx);
            doc.dirty
                || doc.conflict
                || doc.composer_source(cx).as_bytes() != source_bytes
                || doc.composer_disk_baseline().as_deref() != Some(source_bytes.as_slice())
        }) {
            self.note_status(
                self.tr(
                    "A duplicate source tab has conflicting or unsaved content.",
                    "En duplisert kildefane har motstridende eller ulagret innhold.",
                ),
                cx,
            );
            return;
        }
        let peers = cx.global::<crate::windows::WindowManager>().records.clone();
        let local_targets = self
            .docs
            .iter()
            .filter(|open| {
                open.entity
                    .read(cx)
                    .path
                    .canonicalize()
                    .unwrap_or_else(|_| open.entity.read(cx).path.clone())
                    == target_path
            })
            .count();
        if local_targets > 1 {
            self.note_status(
                self.tr(
                    "The target is open in duplicate tabs; close duplicates before merging.",
                    "Målet er åpent i dupliserte faner; lukk duplikatene før sammenslåing.",
                ),
                cx,
            );
            return;
        }
        for peer in peers.iter().filter(|peer| peer.id != self.window_id) {
            let target_open = peer.window.update(cx, |_, _window, cx| {
                peer.workspace.update(cx, |workspace, cx| {
                    workspace.docs.iter().any(|open| {
                        let path = &open.entity.read(cx).path;
                        path.canonicalize().unwrap_or_else(|_| path.clone()) == target_path
                    })
                })
            });
            if !matches!(target_open, Ok(Ok(false))) {
                self.note_status(
                    self.tr(
                        "The target is open in another window; close it there before merging.",
                        "Målet er åpent i et annet vindu; lukk det der før sammenslåing.",
                    ),
                    cx,
                );
                return;
            }
        }
        self.open_document(target_path.clone(), window, cx);
        let Some(target_doc) = self
            .docs
            .iter()
            .find(|open| {
                open.entity
                    .read(cx)
                    .path
                    .canonicalize()
                    .unwrap_or_else(|_| open.entity.read(cx).path.clone())
                    == target_path
            })
            .map(|open| open.entity.clone())
        else {
            self.note_status(
                self.tr(
                    "The target note could not be opened safely.",
                    "Målnotatet kunne ikke åpnes trygt.",
                ),
                cx,
            );
            return;
        };
        if target_doc
            .update(cx, |doc, cx| doc.flush_and_save(cx))
            .is_err()
        {
            self.note_status(
                self.tr(
                    "The target note could not be flushed safely; the merge was cancelled.",
                    "Målnotatet kunne ikke lagres trygt; sammenslåingen ble avbrutt.",
                ),
                cx,
            );
            return;
        }
        let target_text = target_doc.read(cx).composer_source(cx);
        let target_bytes = target_text.as_bytes().to_vec();
        if target_doc.read(cx).dirty
            || target_doc.read(cx).conflict
            || target_doc.read(cx).composer_disk_baseline().as_deref()
                != Some(target_bytes.as_slice())
            || std::fs::read(&target_path).ok().as_deref() != Some(target_bytes.as_slice())
        {
            self.note_status(
                self.tr(
                    "The target note changed on disk; the merge was cancelled.",
                    "Målnotatet ble endret på disken; sammenslåingen ble avbrutt.",
                ),
                cx,
            );
            return;
        }
        let note_paths = self.vault.read(cx).notes.clone();
        let snapshot = match crate::note_composer::LinkSnapshot::new(&root, &note_paths) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return;
            }
        };
        if snapshot.text(&source_path) != Some(source_text.as_str())
            || snapshot.text(&target_path) != Some(target_text.as_str())
        {
            self.note_status(
                self.tr(
                    "A source or target note changed while preparing the merge.",
                    "Et kilde- eller målnotat ble endret under klargjøring av sammenslåingen.",
                ),
                cx,
            );
            return;
        }
        let Some(target_text) = snapshot.text(&target_path).map(str::to_string) else {
            self.note_status(
                self.tr(
                    "The target note could not be read safely.",
                    "Målnotatet kunne ikke leses trygt.",
                ),
                cx,
            );
            return;
        };
        let target_rewritten = match snapshot.rewrite_inbound(
            &target_text,
            &target_path,
            &source_path,
            &target_path,
        ) {
            Ok(text) => text,
            Err(error) => {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return;
            }
        };
        let source_text = match snapshot
            .rewrite_inbound(&source_text, &source_path, &source_path, &target_path)
            .and_then(|text| {
                crate::note_composer::rebase_markdown_links(
                    &text,
                    &source_path,
                    &target_path,
                    &root,
                )
            }) {
            Ok(text) => text,
            Err(error) => {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return;
            }
        };
        let merged = match crate::note_composer::merge_text(&target_rewritten, &source_text, order)
        {
            Ok(merged) => merged,
            Err(error) => {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return;
            }
        };
        if merged == target_text {
            self.note_status(
                self.tr(
                    "The source note adds no content to the target.",
                    "Kildenotatet tilfører ikke innhold til målet.",
                ),
                cx,
            );
            return;
        }
        let mut changes = vec![(
            target_path.clone(),
            target_text.into_bytes(),
            merged.into_bytes(),
        )];
        for note in snapshot.notes() {
            if note == &source_path || note == &target_path {
                continue;
            }
            let Some(before) = snapshot.text(note) else {
                continue;
            };
            let after = match snapshot.rewrite_inbound(before, note, &source_path, &target_path) {
                Ok(after) => after,
                Err(error) => {
                    self.note_status(
                        error.message(
                            self.settings.language == crate::settings::Language::Norwegian,
                        ),
                        cx,
                    );
                    return;
                }
            };
            if after != before {
                changes.push((note.clone(), before.as_bytes().to_vec(), after.into_bytes()));
            }
        }
        let mut buffer_changes = changes.clone();
        buffer_changes.push((source_path.clone(), source_bytes.clone(), Vec::new()));
        for peer in peers.iter().filter(|peer| peer.id != self.window_id) {
            let conflict = peer.window.update(cx, |_, _window, cx| {
                peer.workspace.update(cx, |workspace, cx| {
                    workspace.composer_change_conflicts(&buffer_changes, cx)
                })
            });
            if !matches!(conflict, Ok(Ok(false))) {
                self.note_status(
                    self.tr(
                        "A related note is open in another window and cannot be synchronized safely.",
                        "Et beslektet notat er åpent i et annet vindu og kan ikke synkroniseres trygt.",
                    ),
                    cx,
                );
                return;
            }
        }
        if self.composer_change_conflicts(&buffer_changes, cx) {
            self.note_status(
                self.tr(
                    "A related note changed while preparing the merge.",
                    "Et beslektet notat ble endret under klargjøring av sammenslåingen.",
                ),
                cx,
            );
            return;
        }
        if source.read(cx).composer_source(cx).as_bytes() != source_bytes
            || source.read(cx).composer_disk_baseline().as_deref() != Some(source_bytes.as_slice())
            || std::fs::read(&source_path).ok().as_deref() != Some(source_bytes.as_slice())
        {
            self.note_status(
                self.tr(
                    "The source note changed before the merge could commit.",
                    "Kildenotatet ble endret før sammenslåingen kunne fullføres.",
                ),
                cx,
            );
            return;
        }
        let (_trash_entry, trash_changes) =
            match crate::history::prepare_trash_transaction(&root, &source_path, &source_bytes) {
                Ok(plan) => plan,
                Err(_) => {
                    self.note_status(
                        self.tr(
                            "The source note could not be prepared for safe trashing.",
                            "Kildenotatet kunne ikke klargjøres for trygg sletting.",
                        ),
                        cx,
                    );
                    return;
                }
            };
        for (path, _, after) in &changes {
            if crate::history::snapshot_composer_before_write_checked(&root, path, after).is_err() {
                self.note_status(
                    self.tr(
                        "Could not create a history snapshot; the merge was cancelled.",
                        "Kunne ikke opprette et historikkøyeblikksbilde; sammenslåingen ble avbrutt.",
                    ),
                    cx,
                );
                return;
            }
        }
        if crate::history::snapshot_composer_before_write_checked(&root, &source_path, b"").is_err()
        {
            self.note_status(
                self.tr(
                    "Could not create a history snapshot; the merge was cancelled.",
                    "Kunne ikke opprette et historikkøyeblikksbilde; sammenslåingen ble avbrutt.",
                ),
                cx,
            );
            return;
        }
        let mut transaction_changes = changes
            .iter()
            .map(|(path, before, after)| (path.clone(), Some(before.clone()), Some(after.clone())))
            .collect::<Vec<_>>();
        transaction_changes.extend(trash_changes);
        if let Err(error) =
            crate::note_composer::merge_transaction_changes(&root, &transaction_changes)
        {
            self.note_status(
                error.message(self.settings.language == crate::settings::Language::Norwegian),
                cx,
            );
            return;
        }
        self.apply_composer_changes(&changes, window, cx);
        let mut workspace_path_references = Vec::new();
        for peer in peers.iter().filter(|peer| peer.id != self.window_id) {
            if let Ok(Ok(delta)) = peer.window.update(cx, |_, window, cx| {
                peer.workspace.update(cx, |workspace, cx| {
                    workspace.apply_composer_changes(&changes, window, cx);
                    let delta =
                        workspace.repoint_trashed_composer_source(&source_path, &target_path, cx);
                    workspace.close_composer_deleted_paths(
                        std::slice::from_ref(&source_path),
                        window,
                        cx,
                    );
                    delta
                })
            }) {
                workspace_path_references.push(delta);
            }
        }
        workspace_path_references.push(self.repoint_trashed_composer_source(
            &source_path,
            &target_path,
            cx,
        ));
        for view in &source_views {
            view.update(cx, |doc, _| doc.cancel_composer_save());
        }
        let mut source_indices = self
            .docs
            .iter()
            .enumerate()
            .filter_map(|(index, open)| {
                (open
                    .entity
                    .read(cx)
                    .path
                    .canonicalize()
                    .unwrap_or_else(|_| open.entity.read(cx).path.clone())
                    == source_path)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        source_indices.sort_unstable_by(|left, right| right.cmp(left));
        for source_index in source_indices {
            self.close_tab_now(source_index, window, cx);
        }
        self.closed_tabs.retain(|path| path != &source_path);
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        target_doc.update(cx, |doc, _| {
            doc.record_composer_edit(
                changes[0].1.clone(),
                changes[0].2.clone(),
                transaction_changes.clone(),
                crate::document::ComposerPathReferenceDelta {
                    workspaces: workspace_path_references,
                },
            );
        });
        if let Some(index) = self
            .docs
            .iter()
            .position(|open| open.entity.entity_id() == target_doc.entity_id())
        {
            self.active = Some(index);
            if let Some(token) = self.doc_tokens.get(index).copied() {
                self.layout.activate(token);
            }
        }
        self.note_status(
            self.tr(
                "Merged note; the source was moved to trash.",
                "Notatet er slått sammen; kilden ble flyttet til papirkurven.",
            ),
            cx,
        );
    }

    /// Writes `<name>.md` with the selection's body and replaces the
    /// selection with `[[name]]` — the Note Composer extract.
    fn commit_extract(
        &mut self,
        doc: Entity<Document>,
        source_before: String,
        range: std::ops::Range<usize>,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
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
            self.note_status(self.tr("Invalid note name.", "Ugyldig notatnavn."), cx);
            return false;
        }
        let Some(root) = self.vault.read(cx).root.clone() else {
            return false;
        };
        if doc.update(cx, |doc, cx| doc.flush_and_save(cx)).is_err() {
            self.note_status(
                self.tr(
                    "The source note could not be flushed safely; extraction was cancelled.",
                    "Kildenotatet kunne ikke lagres trygt; uttrekket ble avbrutt.",
                ),
                cx,
            );
            return false;
        }
        let source_path = doc
            .read(cx)
            .path
            .canonicalize()
            .unwrap_or_else(|_| doc.read(cx).path.clone());
        let source = doc.read(cx).composer_source(cx);
        let source_bytes = source.as_bytes().to_vec();
        if doc.read(cx).dirty
            || doc.read(cx).conflict
            || doc.read(cx).composer_disk_baseline().as_deref() != Some(source_bytes.as_slice())
            || std::fs::read(&source_path).ok().as_deref() != Some(source_bytes.as_slice())
        {
            self.note_status(
                self.tr(
                    "The source note changed on disk; extraction was cancelled.",
                    "Kildenotatet ble endret på disken; uttrekket ble avbrutt.",
                ),
                cx,
            );
            return false;
        }
        if source != source_before || source.get(range.clone()) != Some(body.as_str()) {
            self.note_status(
                self.tr(
                    "The source note changed; extraction was cancelled.",
                    "Kildenotatet ble endret; uttrekket ble avbrutt.",
                ),
                cx,
            );
            return false;
        }
        let source_views = self
            .docs
            .iter()
            .filter(|open| {
                open.entity
                    .read(cx)
                    .path
                    .canonicalize()
                    .unwrap_or_else(|_| open.entity.read(cx).path.clone())
                    == source_path
            })
            .map(|open| open.entity.clone())
            .collect::<Vec<_>>();
        if source_views.iter().any(|view| {
            let open = view.read(cx);
            open.dirty
                || open.conflict
                || open.composer_source(cx).as_bytes() != source_bytes
                || open.composer_disk_baseline().as_deref() != Some(source_bytes.as_slice())
        }) {
            self.note_status(
                self.tr(
                    "A duplicate source tab has conflicting or unsaved content.",
                    "En duplisert kildefane har motstridende eller ulagret innhold.",
                ),
                cx,
            );
            return false;
        }
        if source_views.len() > 1 {
            self.note_status(
                self.tr(
                    "The source is open in duplicate tabs; close duplicates before extracting.",
                    "Kilden er åpen i dupliserte faner; lukk duplikatene før uttrekk.",
                ),
                cx,
            );
            return false;
        }
        let peers = cx.global::<crate::windows::WindowManager>().records.clone();
        for peer in peers.iter().filter(|peer| peer.id != self.window_id) {
            let open = peer.window.update(cx, |_, _window, cx| {
                peer.workspace.update(cx, |workspace, cx| {
                    workspace.docs.iter().any(|open| {
                        open.entity
                            .read(cx)
                            .path
                            .canonicalize()
                            .unwrap_or_else(|_| open.entity.read(cx).path.clone())
                            == source_path
                    })
                })
            });
            if !matches!(open, Ok(Ok(false))) {
                self.note_status(
                    self.tr(
                        "The source note is open in another window; close it before extracting.",
                        "Kildenotatet er åpent i et annet vindu; lukk det før uttrekk.",
                    ),
                    cx,
                );
                return false;
            }
        }
        let dir = if name.contains('/') {
            root.clone()
        } else {
            source_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.clone())
        };
        let path = dir.join(format!("{name}.md"));
        let Ok(path) = path.canonicalize().or_else(|_| {
            let parent = path.parent().and_then(|parent| parent.canonicalize().ok());
            let file_name = path.file_name().map(PathBuf::from);
            match (parent, file_name) {
                (Some(parent), Some(file_name)) => Ok(parent.join(file_name)),
                _ => Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            }
        }) else {
            self.note_status(
                self.tr("Invalid extraction path.", "Ugyldig uttrekkssti."),
                cx,
            );
            return false;
        };
        let note_paths = self.vault.read(cx).notes.clone();
        let snapshot = match crate::note_composer::LinkSnapshot::new(&root, &note_paths) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return false;
            }
        };
        if snapshot.text(&source_path) != Some(source.as_str()) {
            self.note_status(
                self.tr(
                    "A source or linked note changed while preparing the extraction.",
                    "Kilde- eller lenkenotatet ble endret under klargjøring av uttrekket.",
                ),
                cx,
            );
            return false;
        }
        let anchors = match crate::note_composer::ExtractionAnchors::new(&source, range.clone()) {
            Ok(anchors) => anchors,
            Err(error) => {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return false;
            }
        };
        let (body, source_definitions) = match crate::note_composer::extract_reference_definitions(
            &source,
            range.clone(),
            &body,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return false;
            }
        };
        let body =
            match crate::note_composer::rebase_markdown_links(&body, &source_path, &path, &root)
                .and_then(|body| {
                    snapshot.rewrite_extraction_links(
                        &body,
                        &path,
                        &source_path,
                        &path,
                        &anchors,
                        true,
                    )
                }) {
                Ok(body) => body,
                Err(error) => {
                    self.note_status(
                        error.message(
                            self.settings.language == crate::settings::Language::Norwegian,
                        ),
                        cx,
                    );
                    return false;
                }
            };
        let relative = path
            .strip_prefix(&root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| name.to_string());
        let link_target = self.wikilink_target(&path, &source_path, cx);
        let replacement = self.settings.extract_behavior.replacement(&link_target);
        let mut source_after = source.clone();
        source_after.replace_range(range.clone(), &replacement);
        if !source_definitions.is_empty() {
            if !source_after.ends_with("\n\n") {
                if source_after.ends_with('\n') {
                    source_after.push('\n');
                } else {
                    source_after.push_str("\n\n");
                }
            }
            source_after.push_str(&source_definitions);
        }
        source_after = match snapshot.rewrite_extraction_links(
            &source_after,
            &source_path,
            &source_path,
            &path,
            &anchors,
            false,
        ) {
            Ok(source_after) => source_after,
            Err(error) => {
                self.note_status(
                    error.message(self.settings.language == crate::settings::Language::Norwegian),
                    cx,
                );
                return false;
            }
        };
        let mut additional_changes = Vec::new();
        for note in snapshot.notes() {
            if note == &source_path {
                continue;
            }
            let Some(before) = snapshot.text(note) else {
                self.note_status(
                    self.tr(
                        "A linked note could not be read safely.",
                        "Et lenket notat kunne ikke leses trygt.",
                    ),
                    cx,
                );
                return false;
            };
            let after = match snapshot.rewrite_extraction_links(
                before,
                note,
                &source_path,
                &path,
                &anchors,
                false,
            ) {
                Ok(after) => after,
                Err(error) => {
                    self.note_status(
                        error.message(
                            self.settings.language == crate::settings::Language::Norwegian,
                        ),
                        cx,
                    );
                    return false;
                }
            };
            if after != before {
                additional_changes.push((
                    note.clone(),
                    Some(before.as_bytes().to_vec()),
                    Some(after.into_bytes()),
                ));
            }
        }
        let buffer_changes = additional_changes
            .iter()
            .filter_map(|(path, before, after)| {
                Some((path.clone(), before.clone()?, after.clone()?))
            })
            .collect::<Vec<_>>();
        if self.composer_change_conflicts(&buffer_changes, cx) {
            self.note_status(
                self.tr(
                    "A related note changed while preparing the extraction.",
                    "Et beslektet notat ble endret under klargjøring av uttrekket.",
                ),
                cx,
            );
            return false;
        }
        for peer in peers.iter().filter(|peer| peer.id != self.window_id) {
            let conflict = peer.window.update(cx, |_, _window, cx| {
                peer.workspace.update(cx, |workspace, cx| {
                    workspace.composer_change_conflicts(&buffer_changes, cx)
                })
            });
            if !matches!(conflict, Ok(Ok(false))) {
                self.note_status(
                    self.tr(
                        "A related note is open in another window and cannot be synchronized safely.",
                        "Et beslektet notat er åpent i et annet vindu og kan ikke synkroniseres trygt.",
                    ),
                    cx,
                );
                return false;
            }
        }
        if source.as_bytes() != source_bytes
            || doc.read(cx).composer_disk_baseline().as_deref() != Some(source_bytes.as_slice())
            || std::fs::read(&source_path).ok().as_deref() != Some(source_bytes.as_slice())
        {
            self.note_status(
                self.tr(
                    "The source note changed before extraction could commit.",
                    "Kildenotatet ble endret før uttrekket kunne fullføres.",
                ),
                cx,
            );
            return false;
        }
        if let Err(error) = crate::note_composer::extract_transaction(
            &root,
            &source_path,
            source.as_bytes(),
            source_after.as_bytes(),
            &path,
            body.as_bytes(),
            &additional_changes,
        ) {
            self.note_status(
                error.message(self.settings.language == crate::settings::Language::Norwegian),
                cx,
            );
            return false;
        }
        let mut history_changes = vec![
            (
                source_path.clone(),
                Some(source.as_bytes().to_vec()),
                Some(source_after.as_bytes().to_vec()),
            ),
            (path.clone(), None, Some(body.as_bytes().to_vec())),
        ];
        history_changes.extend(additional_changes);
        for source_view in source_views {
            let owner = source_view.entity_id() == doc.entity_id();
            source_view.update(cx, |source_doc, cx| {
                source_doc.replace_range(0..source.len(), source_after.clone(), window, cx);
                source_doc.mark_composer_written(source_after.as_bytes().to_vec(), cx);
                if owner {
                    source_doc.record_composer_edit(
                        source.as_bytes().to_vec(),
                        source_after.as_bytes().to_vec(),
                        history_changes.clone(),
                        crate::document::ComposerPathReferenceDelta::default(),
                    );
                }
            });
        }
        self.apply_composer_changes(&buffer_changes, window, cx);
        for peer in peers.iter().filter(|peer| peer.id != self.window_id) {
            let _ = peer.window.update(cx, |_, window, cx| {
                peer.workspace.update(cx, |workspace, cx| {
                    workspace.apply_composer_changes(&buffer_changes, window, cx);
                    workspace.vault.update(cx, |vault, cx| vault.refresh(cx));
                })
            });
        }
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        self.note_status(
            if self.settings.language == crate::settings::Language::Norwegian {
                format!("Trukket ut til {relative}")
            } else {
                format!("Extracted to {relative}")
            },
            cx,
        );
        true
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

    fn wikilink_target(&self, target: &Path, source: &Path, cx: &App) -> String {
        self.vault
            .read(cx)
            .link_target_for(target, source)
            .or_else(|| {
                target
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
            })
            .unwrap_or_default()
    }

    fn vault_wikilink_target(&self, target: &Path, cx: &App) -> String {
        self.vault
            .read(cx)
            .root
            .as_deref()
            .and_then(|root| target.strip_prefix(root).ok())
            .map(|relative| {
                relative
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .filter(|relative| !relative.is_empty())
            .or_else(|| {
                target
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
            })
            .unwrap_or_default()
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

    fn on_copy_block_link(
        &mut self,
        _: &CopyBlockLink,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_block_ref(false, window, cx);
    }

    fn on_copy_block_embed(
        &mut self,
        _: &CopyBlockEmbed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_block_ref(true, window, cx);
    }

    /// Copy `[[stem#^id]]` (or the `![[…]]` embed) for the block at the
    /// caret, adding the `^id` marker first when the block has none.
    fn copy_block_ref(&mut self, embed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc().cloned() else {
            self.note_status(self.tr("No note open", "Ingen notater er åpne"), cx);
            return;
        };
        let path = doc.read(cx).path.clone();
        let target = self.vault_wikilink_target(&path, cx);
        if target.is_empty() {
            return;
        }
        match doc.update(cx, |doc, cx| doc.ensure_block_id(window, cx)) {
            Some(id) => {
                let link = format!("{}[[{target}#^{id}]]", if embed { "!" } else { "" });
                cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                self.note_status(format!("{} {link}", self.tr("Copied", "Kopiert")), cx);
            }
            None => self.note_status(
                self.tr(
                    "Put the caret in a paragraph, list item, quote, table, or code block",
                    "Plasser markøren i et avsnitt, listeelement, sitat, tabell eller kodeblokk",
                ),
                cx,
            ),
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

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        let group = self.layout.focused();
        let tabs = self.layout.group_tokens(group);
        if tabs.is_empty() {
            return;
        }
        self.folder = None;
        self.graph = None;
        let current = self.layout.selected(group);
        let index = current
            .and_then(|token| tabs.iter().position(|candidate| *candidate == token))
            .map(|index| (index + 1) % tabs.len())
            .unwrap_or(0);
        self.layout.select(group, tabs[index]);
        self.active = self
            .doc_tokens
            .iter()
            .position(|token| *token == tabs[index]);
        self.persist_tabs(cx);
        self.reveal_active_file(cx);
        self.refocus(window, cx);
        cx.notify();
    }

    fn on_prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        let group = self.layout.focused();
        let tabs = self.layout.group_tokens(group);
        if tabs.is_empty() {
            return;
        }
        self.folder = None;
        self.graph = None;
        let current = self.layout.selected(group);
        let index = current
            .and_then(|token| tabs.iter().position(|candidate| *candidate == token))
            .map(|index| index.checked_sub(1).unwrap_or(tabs.len() - 1))
            .unwrap_or(tabs.len() - 1);
        self.layout.select(group, tabs[index]);
        self.active = self
            .doc_tokens
            .iter()
            .position(|token| *token == tabs[index]);
        self.persist_tabs(cx);
        self.reveal_active_file(cx);
        self.refocus(window, cx);
        cx.notify();
    }

    fn split_pane(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let Some(token) = self.active.and_then(|ix| self.doc_tokens.get(ix).copied()) else {
            return;
        };
        let group = self.layout.focused();
        if self.layout.split(group, token, axis).is_some() {
            self.folder = None;
            self.graph = None;
            self.persist_tabs(cx);
            self.focus_active_pane_after_mount(window, cx);
        }
    }

    fn on_split_pane_right(
        &mut self,
        _: &SplitPaneRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_pane(Axis::Horizontal, window, cx);
    }

    fn on_split_pane_down(
        &mut self,
        _: &SplitPaneDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_pane(Axis::Vertical, window, cx);
    }

    fn cycle_pane(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let groups = self.layout.groups_in_order();
        if groups.len() < 2 {
            return;
        }
        let current = groups
            .iter()
            .position(|group| *group == self.layout.focused())
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(groups.len() as isize) as usize;
        let group = groups[next];
        self.layout.focus(group);
        self.folder = None;
        self.graph = None;
        self.active = self
            .layout
            .selected(group)
            .and_then(|token| self.doc_tokens.iter().position(|item| *item == token));
        self.persist_tabs(cx);
        self.focus_active_pane_after_mount(window, cx);
    }

    fn on_next_pane(&mut self, _: &NextPane, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_pane(1, window, cx);
    }

    fn on_prev_pane(&mut self, _: &PrevPane, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_pane(-1, window, cx);
    }

    fn on_toggle_sidebar(&mut self, _: &ToggleSidebar, _w: &mut Window, cx: &mut Context<Self>) {
        self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed;
        self.persist_settings(cx);
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
        self.persist_settings(cx);
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
        fn whole_file_ids(items: &[crate::bookmarks::Bookmark], path: &Path, ids: &mut Vec<u64>) {
            for item in items {
                match &item.kind {
                    BookmarkKind::File {
                        path: bookmark_path,
                        anchor: None,
                        ..
                    } if bookmark_path == path => ids.push(item.id),
                    BookmarkKind::Group { items, .. } => whole_file_ids(items, path, ids),
                    _ => {}
                }
            }
        }
        let mut ids = Vec::new();
        whole_file_ids(&self.settings.bookmarks.items, &path, &mut ids);
        let bookmarked = if ids.is_empty() {
            self.settings
                .bookmarks
                .add(BookmarkKind::file(path), None, None)
                .is_some()
        } else {
            for id in ids {
                self.settings.bookmarks.remove(id);
            }
            false
        };
        self.note_status(
            self.tr(
                if bookmarked {
                    "Bookmarked"
                } else {
                    "Bookmark removed"
                },
                if bookmarked {
                    "Bokmerket"
                } else {
                    "Bokmerke fjernet"
                },
            ),
            cx,
        );
        self.persist_bookmarks(cx);
    }

    fn toggle_folder_bookmark(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.note_status(self.tr("Open a vault first", "Åpne et hvelv først"), cx);
            return;
        };
        let kind = BookmarkKind::Folder {
            path,
            root: Some(root),
        };
        let bookmarked = if let Some(id) = self.settings.bookmarks.find_kind(&kind) {
            self.settings.bookmarks.remove(id);
            false
        } else {
            self.settings.bookmarks.add(kind, None, None).is_some()
        };
        self.note_status(
            self.tr(
                if bookmarked {
                    "Folder bookmarked"
                } else {
                    "Bookmark removed"
                },
                if bookmarked {
                    "Mappe bokmerket"
                } else {
                    "Bokmerke fjernet"
                },
            ),
            cx,
        );
        self.persist_bookmarks(cx);
    }

    fn persist_bookmarks(&mut self, cx: &mut Context<Self>) {
        self.settings.starred = self.settings.bookmarks.starred_paths();
        self.starred_open = true;
        self.settings.panes.starred = true;
        let starred = self.settings.starred.iter().cloned().collect();
        let vault = self.vault.clone();
        vault.update(cx, |vault, cx| {
            vault.starred = starred;
            cx.emit(VaultEvent::Starred);
        });
        self.persist_settings(cx);
        cx.notify();
    }

    fn repoint_trashed_composer_source(
        &mut self,
        source: &Path,
        target: &Path,
        cx: &mut Context<Self>,
    ) -> crate::document::ComposerWorkspacePathReferenceDelta {
        fn repoint_paths(
            paths: &mut [PathBuf],
            source: &Path,
            target: &Path,
        ) -> Vec<crate::document::ComposerPathValueChange<PathBuf>> {
            paths
                .iter_mut()
                .enumerate()
                .filter_map(|(index, path)| {
                    let before = path.clone();
                    crate::explorer::repoint_path(path, source, target);
                    (before != *path).then(|| crate::document::ComposerPathValueChange {
                        index,
                        before,
                        after: path.clone(),
                    })
                })
                .collect()
        }

        let mut before = HashMap::new();
        bookmark_kinds(&self.settings.bookmarks.items, &mut before);
        self.settings.bookmarks.repoint(source, target);
        let mut pinned_tabs = Vec::new();
        for (index, value) in self.settings.pinned_tabs.iter_mut().enumerate() {
            let before = value.clone();
            let mut path = PathBuf::from(&before);
            crate::explorer::repoint_path(&mut path, source, target);
            let after = path.to_string_lossy().to_string();
            if before != after {
                *value = after.clone();
                pinned_tabs.push(crate::document::ComposerPathValueChange {
                    index,
                    before,
                    after,
                });
            }
        }
        let recent = repoint_paths(&mut self.recent, source, target);
        let closed_tabs = repoint_paths(&mut self.closed_tabs, source, target);
        let navigation = repoint_paths(&mut self.nav_stack, source, target);
        let mut after = HashMap::new();
        bookmark_kinds(&self.settings.bookmarks.items, &mut after);
        let bookmarks = before
            .into_iter()
            .filter_map(|(id, before)| {
                let after = after.get(&id)?;
                (before != *after).then(|| (id, before, after.clone()))
            })
            .collect();
        self.settings.starred = self.settings.bookmarks.starred_paths();
        let starred = self.settings.starred.iter().cloned().collect();
        self.vault.update(cx, |vault, cx| {
            vault.starred = starred;
            cx.emit(VaultEvent::Starred);
        });
        self.persist_settings(cx);
        self.persist_tabs(cx);
        crate::document::ComposerWorkspacePathReferenceDelta {
            window_id: self.window_id,
            bookmarks,
            pinned_tabs,
            recent,
            closed_tabs,
            navigation,
        }
    }

    pub(crate) fn bookmark_search(
        &mut self,
        query: String,
        match_case: bool,
        sort: crate::search_service::SearchSort,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let kind = BookmarkKind::Search {
            root,
            query: query.trim().to_string(),
            match_case,
            sort,
        };
        if self.settings.bookmarks.find_kind(&kind).is_some() {
            self.note_status(
                self.tr("Search already bookmarked", "Søket er allerede bokmerket"),
                cx,
            );
            return;
        }
        self.settings.bookmarks.add(kind, None, None);
        self.note_status(self.tr("Search bookmarked", "Søket er bokmerket"), cx);
        self.persist_bookmarks(cx);
    }

    fn set_view_mode(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        let pane_mode = match mode {
            ViewMode::Source => PaneMode::Source,
            ViewMode::Split => PaneMode::Split,
            ViewMode::Preview => PaneMode::Preview,
        };
        self.set_pane_view_mode(self.layout.focused(), pane_mode, window, cx);
    }

    fn set_pane_view_mode(
        &mut self,
        group: GroupId,
        mode: PaneMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.layout.focus(group);
        if let Some(token) = self.layout.selected(group) {
            self.active = self.doc_tokens.iter().position(|item| *item == token);
        }
        self.layout.set_mode(group, mode);
        self.settings.view_mode = match mode {
            PaneMode::Source => ViewMode::Source,
            PaneMode::Split => ViewMode::Split,
            PaneMode::Preview => ViewMode::Preview,
        };
        self.persist_tabs(cx);
        self.focus_active_pane_after_mount(window, cx);
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
        self.retheme_previews(cx);
        self.persist_settings(cx);
        cx.notify();
    }

    #[cfg(target_os = "macos")]
    fn on_install_cli(&mut self, _: &InstallCli, window: &mut Window, cx: &mut Context<Self>) {
        let language = self.settings.language;
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _window, _cx| {
            let view = view.clone();
            dialog
                .title(language.text(
                    "Install the command-line tool?",
                    "Installere kommandolinjeverktøyet?",
                ))
                .description(language.text(
                    "This adds /usr/local/bin/rista for the current packaged app. Existing commands are never replaced.",
                    "Dette legger til /usr/local/bin/rista for den pakkede appen. Eksisterende kommandoer erstattes aldri.",
                ))
                .show_cancel(true)
                .cancel_text(language.text("Cancel", "Avbryt"))
                .ok_text(language.text("Install", "Installer"))
                .on_ok(move |_, _window, cx| {
                    let result = crate::cli_install::install_current();
                    view.update(cx, |workspace, cx| match result {
                        Ok(()) => workspace.note_status(
                            language.text(
                                "Command-line tool installed at /usr/local/bin/rista",
                                "Kommandolinjeverktøy installert i /usr/local/bin/rista",
                            ),
                            cx,
                        ),
                        Err(error) => {
                            workspace.note_status(error.message(language).to_owned(), cx)
                        }
                    });
                    true
                })
        });
    }

    /// Re-render open previews after a theme change — colored highlights
    /// bake theme colors into the preview markup.
    fn retheme_previews(&self, cx: &mut Context<Self>) {
        for doc in &self.docs {
            doc.entity.update(cx, |doc, cx| {
                doc.refresh_preview_style(cx);
            });
        }
    }

    pub(crate) fn reload_style_snippets(&mut self, cx: &mut Context<Self>) {
        if let Some(queue) = cx.try_global::<crate::style_snippets::StyleSnippetReloadQueue>() {
            queue.request_manual();
        }
    }

    #[cfg(not(target_os = "macos"))]
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
        let commands = PaletteCmd::ALL;
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
                let (icon, filter_label, keywords) = cmd.spec();
                let shortcut_id = cmd.id().to_string();
                let label = crate::hotkeys::command(cmd.id())
                    .map(|spec| spec.label(self.settings.language))
                    .unwrap_or(filter_label)
                    .to_string();
                CommandItem::new()
                    .icon(icon)
                    .label(label.clone())
                    .keywords(std::iter::once(filter_label).chain(keywords.iter().copied()))
                    .child(move |_, cx| {
                        let mut row = h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(icon).size_4().flex_shrink_0())
                            .child(div().flex_1().min_w_0().truncate().child(label.clone()));
                        if let Some(shortcut) = crate::hotkeys::shortcut(cx, &shortcut_id) {
                            let keys = shortcut
                                .split_whitespace()
                                .filter_map(|stroke| Keystroke::parse(stroke).ok())
                                .map(Kbd::new);
                            row = row.child(h_flex().items_center().gap_1().children(keys));
                        }
                        row
                    })
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
                                window.close_dialog(cx);
                                view.update(cx, |this, cx| {
                                    this.palette_sections.clear();
                                    this.refocus(window, cx);
                                });
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

    pub(crate) fn run_palette_hotkey(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = PaletteCmd::ALL
            .iter()
            .copied()
            .find(|command| command.id() == id)
        {
            self.run_command(command, window, cx);
        }
    }

    fn on_run_palette_command(
        &mut self,
        action: &RunPaletteCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_palette_hotkey(&action.id, window, cx);
    }

    fn run_command(&mut self, cmd: PaletteCmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            PaletteCmd::SaveWorkspace => self.show_workspace_name(None, window, cx),
            PaletteCmd::LoadWorkspace => {
                self.show_workspace_picker(PaletteCmd::LoadWorkspace, window, cx)
            }
            PaletteCmd::RenameWorkspace => {
                self.show_workspace_picker(PaletteCmd::RenameWorkspace, window, cx)
            }
            PaletteCmd::DeleteWorkspace => {
                self.show_workspace_picker(PaletteCmd::DeleteWorkspace, window, cx)
            }
            PaletteCmd::NewWindow => window.dispatch_action(NewWindow.boxed_clone(), cx),
            PaletteCmd::OpenInNewWindow => {
                window.dispatch_action(OpenInNewWindow.boxed_clone(), cx)
            }
            PaletteCmd::MoveToNewWindow => {
                window.dispatch_action(MoveToNewWindow.boxed_clone(), cx)
            }
            PaletteCmd::NewFile => self.on_new_file(&NewFile, window, cx),
            PaletteCmd::NewCanvas => self.on_new_canvas(&NewCanvas, window, cx),
            PaletteCmd::NewBase => self.new_base(window, cx),
            PaletteCmd::NewFolder => self.on_new_folder(&NewFolder, window, cx),
            PaletteCmd::OpenFile => self.on_open_file(&OpenFile, window, cx),
            PaletteCmd::OpenFolder => self.on_open_folder(&OpenFolder, window, cx),
            PaletteCmd::DailyNote => self.on_open_daily(&OpenDailyNote, window, cx),
            PaletteCmd::WeeklyNote => self.open_period_today(Period::Week, window, cx),
            PaletteCmd::MonthlyNote => self.open_period_today(Period::Month, window, cx),
            PaletteCmd::QuarterlyNote => self.open_period_today(Period::Quarter, window, cx),
            PaletteCmd::YearlyNote => self.open_period_today(Period::Year, window, cx),
            PaletteCmd::PrevPeriodic => self.open_adjacent_period(-1, window, cx),
            PaletteCmd::NextPeriodic => self.open_adjacent_period(1, window, cx),
            PaletteCmd::NewFromTemplate => {
                self.defer_dialog(Self::show_new_from_template, window, cx)
            }
            PaletteCmd::AppendDaily => {
                self.defer_dialog(Self::show_append_daily_dialog, window, cx)
            }
            PaletteCmd::CloseFolder => {
                self.close_vault(window, cx);
            }
            PaletteCmd::Save => self.on_save(&SaveFile, window, cx),
            PaletteCmd::SaveAs => self.on_save_as(&SaveFileAs, window, cx),
            PaletteCmd::SplitRight => self.on_split_pane_right(&SplitPaneRight, window, cx),
            PaletteCmd::SplitDown => self.on_split_pane_down(&SplitPaneDown, window, cx),
            PaletteCmd::NextPane => self.on_next_pane(&NextPane, window, cx),
            PaletteCmd::PrevPane => self.on_prev_pane(&PrevPane, window, cx),
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
            PaletteCmd::ToggleLocalGraphPanel => {
                self.on_toggle_local_graph_panel(&ToggleLocalGraphPanel, window, cx)
            }
            PaletteCmd::ToggleAgentPanel => {
                self.on_toggle_agent_panel(&ToggleAgentPanel, window, cx)
            }
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
            PaletteCmd::ExtractHeading => {
                self.defer_dialog(Self::show_extract_heading_dialog, window, cx)
            }
            PaletteCmd::MergeNote => self.defer_dialog(Self::show_merge_dialog, window, cx),
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
                let link = self.active_doc().map(|d| {
                    let path = d.read(cx).path.clone();
                    format!("[[{}]]", self.vault_wikilink_target(&path, cx))
                });
                match link {
                    Some(link) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(link.clone()));
                        self.note_status(format!("Copied {link}"), cx);
                    }
                    None => self.note_status("No note open", cx),
                }
            }
            PaletteCmd::CopyLinkBlock => self.on_copy_block_link(&CopyBlockLink, window, cx),
            PaletteCmd::CopyEmbedBlock => self.on_copy_block_embed(&CopyBlockEmbed, window, cx),
            PaletteCmd::RandomNote => self.open_random_note(window, cx),
            PaletteCmd::NewUniqueNote => self.new_unique_note(window, cx),
            PaletteCmd::CopyLinkHeading => {
                // `[[stem#heading]]` for the nearest heading at or above
                // the caret — the "copy link to heading".
                let link = self.active_doc().map(|d| {
                    let doc = d.read(cx);
                    let path = doc.path.clone();
                    let target = self.vault_wikilink_target(&path, cx);
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
                    match heading {
                        Some(h) => format!("[[{target}#{h}]]"),
                        None => format!("[[{target}]]"),
                    }
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
                self.persist_settings(cx);
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
            PaletteCmd::ExportPdf => self.export_pdf(window, cx),
            PaletteCmd::Settings => self.defer_dialog(
                |ws, window, cx| ws.on_open_settings(&OpenSettings, window, cx),
                window,
                cx,
            ),
            PaletteCmd::ImportVaultSettings => self.show_vault_settings_import(false, window, cx),
            PaletteCmd::ToggleTheme => self.on_toggle_theme(&ToggleTheme, window, cx),
            PaletteCmd::Quit => window.dispatch_action(Quit.boxed_clone(), cx),
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

    fn queue_vault_settings_offer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.restoring_session {
            return;
        }
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let Some(root_key) = root.canonicalize().ok() else {
            return;
        };
        let view = cx.entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |this, cx| {
                let current_root = this
                    .vault
                    .read(cx)
                    .root
                    .as_ref()
                    .and_then(|root| root.canonicalize().ok());
                if this.restoring_session
                    || current_root.as_ref() != Some(&root_key)
                    || this
                        .settings
                        .vault_settings_import_dismissed
                        .iter()
                        .any(|dismissed| dismissed == &root_key.to_string_lossy())
                {
                    return;
                }
                this.show_vault_settings_import(true, window, cx);
            });
        });
    }

    fn show_vault_settings_import(
        &mut self,
        first_offer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.note_status(
                self.tr(
                    "Open a vault before importing vault settings.",
                    "Åpne et hvelv før du importerer hvelvinnstillinger.",
                ),
                cx,
            );
            return;
        };
        let mut plan = match crate::vault_settings_import::plan_import(&root, &self.settings) {
            Ok(plan) => plan,
            Err(error) => {
                let (message, detail) =
                    if let Some(detail) = error.strip_prefix("Could not resolve vault: ") {
                        (
                            self.tr("Could not resolve the vault.", "Hvelvet kunne ikke finnes."),
                            Some(detail),
                        )
                    } else {
                        (
                            self.tr(
                                "The selected vault is not a folder.",
                                "Det valgte hvelvet er ikke en mappe.",
                            ),
                            None,
                        )
                    };
                self.show_vault_import_report(
                    self.tr(
                        "Vault settings could not be inspected.",
                        "Hvelvinnstillingene kunne ikke undersøkes.",
                    ),
                    std::iter::once(message.to_string())
                        .chain(detail.map(str::to_string))
                        .collect(),
                    window,
                    cx,
                );
                return;
            }
        };
        plan.hotkey_baseline = crate::hotkeys::imported_bindings_snapshot(cx, &plan.hotkeys);
        if first_offer && !plan.has_sources() {
            return;
        }
        self.show_vault_import_preview(plan, first_offer, window, cx);
    }

    fn show_vault_import_preview(
        &mut self,
        plan: crate::vault_settings_import::ImportPlan,
        first_offer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.settings.language;
        let app_wide = language.text(
            "Imported preferences and shortcuts apply to the whole app, not only this vault.",
            "Importerte innstillinger og snarveier gjelder hele appen, ikke bare dette hvelvet.",
        );
        let mut rows = Vec::new();
        if plan.preview.is_empty() {
            rows.push(
                language
                    .text(
                        "No supported changes were found.",
                        "Fant ingen støttede endringer.",
                    )
                    .to_string(),
            );
        } else {
            for change in &plan.preview {
                let value = match (change.category, change.value.as_str()) {
                    (crate::vault_settings_import::ImportCategory::Bookmarks, count) => {
                        format!("{} {}", count, language.text("items", "elementer"))
                    }
                    (crate::vault_settings_import::ImportCategory::Hotkeys, count) => {
                        format!("{} {}", count, language.text("commands", "kommandoer"))
                    }
                    (crate::vault_settings_import::ImportCategory::Links, "shortest") => {
                        language.text("Shortest", "Korteste").to_string()
                    }
                    (crate::vault_settings_import::ImportCategory::Links, "relative") => {
                        language.text("Relative", "Relativ").to_string()
                    }
                    (crate::vault_settings_import::ImportCategory::Links, "absolute") => {
                        language.text("Absolute", "Absolutt").to_string()
                    }
                    (_, "(none)") => language.text("(none)", "(ingen)").to_string(),
                    (_, value) => value.to_string(),
                };
                rows.push(format!(
                    "{} — {}: {}",
                    Self::import_category_label(change.category, language),
                    Self::import_field_label(&change.field, language),
                    value
                ));
            }
        }
        if plan.bookmarks_skipped > 0 {
            rows.push(format!(
                "{} {}",
                language.text("Bookmarks skipped:", "Bokmerker som hoppes over:",),
                plan.bookmarks_skipped
            ));
        }
        for warning in &plan.warnings {
            rows.push(Self::import_warning_label(warning, language));
        }
        if !plan.unsupported_plugins.is_empty() {
            rows.push(format!(
                "{} {}",
                language.text(
                    "Enabled plugins are not imported:",
                    "Aktiverte programtillegg importeres ikke:",
                ),
                plan.unsupported_plugins.join(", ")
            ));
        }
        let scroll = ScrollHandle::new();
        let view = cx.entity();
        let root = plan.root.clone();
        let plan = std::sync::Arc::new(plan);
        let title = language.text("Import vault settings…", "Importer hvelvinnstillinger…");
        let cancel = language.text("Cancel", "Avbryt");
        let import = language.text("Import", "Importer");
        let dismiss = {
            let view = view.clone();
            let root = root.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let view = view.clone();
                let root = root.clone();
                if first_offer {
                    window.defer(cx, move |window, cx| {
                        view.update(cx, |this, cx| {
                            this.persist_vault_import_dismissal(&root, window, cx);
                            this.refocus(window, cx);
                        });
                    });
                } else {
                    window.defer(cx, move |window, cx| {
                        view.update(cx, |this, cx| this.refocus(window, cx));
                    });
                }
            })
        };
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let preview = div()
                .id("vault-settings-import-preview")
                .h(px(300.))
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .children(rows.iter().cloned().map(|row| div().flex_none().child(row))),
                )
                .vertical_scrollbar(&scroll);
            dialog
                .title(title)
                .w(px(520.))
                .description(app_wide)
                .confirm()
                .child(preview)
                .footer(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(Button::new("cancel").label(cancel).on_click({
                            let dismiss = dismiss.clone();
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                dismiss(window, cx);
                            }
                        }))
                        .child(Button::new("import").label(import).primary().on_click({
                            let view = view.clone();
                            let plan = plan.clone();
                            let root = root.clone();
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                window.defer(cx, {
                                    let view = view.clone();
                                    let plan = plan.clone();
                                    let root = root.clone();
                                    move |window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.apply_vault_import_plan(&plan, &root, window, cx);
                                        });
                                    }
                                });
                            }
                        })),
                )
                .on_ok({
                    let dismiss = dismiss.clone();
                    move |_, window, cx| {
                        dismiss(window, cx);
                        true
                    }
                })
                .on_cancel({
                    let dismiss = dismiss.clone();
                    move |_, window, cx| {
                        dismiss(window, cx);
                        true
                    }
                })
        });
    }

    fn import_category_label(
        category: crate::vault_settings_import::ImportCategory,
        language: Language,
    ) -> &'static str {
        use crate::vault_settings_import::ImportCategory::*;
        match category {
            DailyNotes => language.text("Daily notes", "Daglige notater"),
            PeriodicNotes => language.text("Periodic notes", "Periodiske notater"),
            Templates => language.text("Templates", "Maler"),
            Attachments => language.text("Attachments", "Vedlegg"),
            Links => language.text("Links", "Lenker"),
            IgnoreFilters => language.text("Ignore filters", "Ignoreringsfiltre"),
            Bookmarks => language.text("Bookmarks", "Bokmerker"),
            Hotkeys => language.text("Shortcuts", "Snarveier"),
            CommunityPlugins => language.text("Plugins", "Programtillegg"),
        }
    }

    fn import_field_label(field: &str, language: Language) -> String {
        if let Some(folder) = field.strip_prefix("folder template ") {
            return format!("{} {folder}", language.text("Folder template", "Mappemal"));
        }
        match field {
            "folder" | "templates folder" => language.text("Folder", "Mappe"),
            "daily template" => language.text("Daily template", "Daglig mal"),
            "weekly template" => language.text("Weekly template", "Ukentlig mal"),
            "monthly template" => language.text("Monthly template", "Månedlig mal"),
            "quarterly template" => language.text("Quarterly template", "Kvartalsmal"),
            "yearly template" => language.text("Yearly template", "Årlig mal"),
            "format" => language.text("Format", "Format"),
            "daily directory" => language.text("Daily folder", "Daglig mappe"),
            "weekly directory" => language.text("Weekly folder", "Ukentlig mappe"),
            "monthly directory" => language.text("Monthly folder", "Månedlig mappe"),
            "quarterly directory" => language.text("Quarterly folder", "Kvartalsmappe"),
            "yearly directory" => language.text("Yearly folder", "Årlig mappe"),
            "daily format" => language.text("Daily format", "Daglig format"),
            "weekly format" => language.text("Weekly format", "Ukentlig format"),
            "monthly format" => language.text("Monthly format", "Månedlig format"),
            "quarterly format" => language.text("Quarterly format", "Kvartalsformat"),
            "yearly format" => language.text("Yearly format", "Årlig format"),
            "attachments directory" => language.text("Attachments folder", "Vedleggsmappe"),
            "link format" => language.text("Link format", "Lenkeformat"),
            "bookmarks" => language.text("Bookmarks", "Bokmerker"),
            "ignore filters" => language.text("Ignore filters", "Ignoreringsfiltre"),
            "shortcuts" => language.text("Shortcuts", "Snarveier"),
            "newLinkFormat" => language.text("Link format", "Lenkeformat"),
            "userIgnoreFilters" => language.text("Ignore filters", "Ignoreringsfiltre"),
            "enabled community plugins" => {
                language.text("Enabled plugins", "Aktiverte programtillegg")
            }
            _ => language.text("Setting", "Innstilling"),
        }
        .to_string()
    }

    fn import_warning_label(
        warning: &crate::vault_settings_import::ImportWarning,
        language: Language,
    ) -> String {
        use crate::vault_settings_import::ImportProblem::*;
        let message = match warning.problem {
            ReadFailed => language.text("could not be read", "kunne ikke leses"),
            InvalidJson => language.text("contains invalid JSON", "inneholder ugyldig JSON"),
            TooLarge => language.text(
                "exceeds the import size limit",
                "overskrider størrelsesgrensen",
            ),
            UnsafePath => language.text("uses an unsafe source path", "har en utrygg kildebane"),
            TooManyEntries => language.text(
                "contains too many entries",
                "inneholder for mange oppføringer",
            ),
            InvalidFieldType => {
                language.text("has an invalid value type", "har en ugyldig verditype")
            }
            UnsupportedValue => language.text(
                "contains an unsupported value",
                "inneholder en verdi som ikke støttes",
            ),
            UnsafeConfiguredPath => language.text(
                "contains a path outside the vault",
                "inneholder en bane utenfor hvelvet",
            ),
            TemplateNotFound => language.text(
                "references a template that could not be found",
                "refererer til en mal som ikke ble funnet",
            ),
            InvalidRegex => language.text(
                "contains an invalid filter expression",
                "inneholder et ugyldig filteruttrykk",
            ),
            UnsupportedHotkey => language.text(
                "contains a shortcut that cannot be imported",
                "inneholder en snarvei som ikke kan importeres",
            ),
            UnsupportedCommand => language.text(
                "contains an unsupported command",
                "inneholder en handling som ikke støttes",
            ),
            Precedence => language.text(
                "the active template folder takes precedence, so this folder was ignored",
                "den aktive malmappen har prioritet, så denne mappen ble ignorert",
            ),
        };
        let mut label = format!(
            "{}: {} ({})",
            warning.source,
            message,
            Self::import_category_label(warning.category, language)
        );
        if let Some(detail) = &warning.detail {
            label.push_str(": ");
            label.push_str(detail);
        }
        label
    }

    fn apply_vault_import_plan(
        &mut self,
        plan: &crate::vault_settings_import::ImportPlan,
        expected_root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.settings.language;
        let current_root = self
            .vault
            .read(cx)
            .root
            .as_ref()
            .and_then(|root| root.canonicalize().ok());
        if current_root.as_deref() != Some(expected_root) || !plan.validate_targets() {
            self.show_vault_import_report(
                language.text(
                    "The vault or an imported path changed after preview. Nothing was imported.",
                    "Hvelvet eller en importert bane ble endret etter forhåndsvisningen. Ingenting ble importert.",
                ),
                Vec::new(),
                window,
                cx,
            );
            return;
        }
        if crate::hotkeys::imported_bindings_snapshot(cx, &plan.hotkeys) != plan.hotkey_baseline {
            self.show_vault_import_report(
                language.text(
                    "Shortcuts changed after preview. Nothing was imported; review a new preview.",
                    "Snarveier ble endret etter forhåndsvisningen. Ingenting ble importert; gå gjennom en ny forhåndsvisning.",
                ),
                Vec::new(),
                window,
                cx,
            );
            return;
        }
        let latest = cx
            .global::<crate::windows::WindowManager>()
            .settings
            .clone();
        let mut proposed = crate::windows::with_local_settings(latest.clone(), &self.settings);
        if plan.apply_settings(&mut proposed).is_err() {
            self.show_vault_import_report(
                language.text(
                    "Some settings changed after preview. Nothing was imported; review a new preview.",
                    "Noen innstillinger ble endret etter forhåndsvisningen. Ingenting ble importert; gå gjennom en ny forhåndsvisning.",
                ),
                Vec::new(),
                window,
                cx,
            );
            return;
        }
        let bookmarks_added = proposed.bookmarks.merge_imported(plan.bookmarks.clone());
        let root_key = expected_root.to_string_lossy().to_string();
        let dismissed_offer_added = if proposed.vault_settings_import_dismissed.contains(&root_key)
        {
            false
        } else {
            proposed.vault_settings_import_dismissed.push(root_key);
            true
        };
        let imported_settings_changed = plan.has_settings_changes() || bookmarks_added > 0;
        let needs_settings_save = imported_settings_changed || dismissed_offer_added;
        let mut successes: Vec<String> = Vec::new();
        let mut warnings = plan
            .warnings
            .iter()
            .map(|warning| Self::import_warning_label(warning, language))
            .collect::<Vec<_>>();
        if plan.bookmarks_skipped > 0 {
            warnings.push(format!(
                "{} {}",
                language.text("Bookmarks skipped:", "Bokmerker som hoppes over:"),
                plan.bookmarks_skipped
            ));
        }
        if needs_settings_save {
            match crate::windows::publish_checked(self.window_id, &latest, &proposed, cx) {
                Ok(saved) => {
                    self.apply_settings_inner(saved.clone(), window, cx, false);
                    self.settings_baseline = saved;
                    if imported_settings_changed {
                        successes.push(
                            language
                                .text(
                                    "Vault preferences and bookmarks were imported.",
                                    "Hvelvinnstillinger og bokmerker ble importert.",
                                )
                                .to_string(),
                        );
                    }
                    if bookmarks_added > 0 {
                        successes.push(format!(
                            "{} {}",
                            language.text("Bookmarks added:", "Bokmerker lagt til:"),
                            bookmarks_added
                        ));
                    }
                }
                Err(error) => {
                    warnings.push(format!(
                        "{}: {error}",
                        language.text(
                            "Settings could not be saved; shortcuts were not imported",
                            "Innstillingene kunne ikke lagres; snarveier ble ikke importert",
                        )
                    ));
                    self.show_vault_import_report(
                        language.text(
                            "The import did not complete.",
                            "Importen ble ikke fullført.",
                        ),
                        warnings,
                        window,
                        cx,
                    );
                    return;
                }
            }
        }
        if !plan.hotkeys.is_empty() {
            match crate::hotkeys::apply_imported_bindings(cx, &plan.hotkeys) {
                Ok(()) => successes.push(
                    language
                        .text("Shortcuts were imported.", "Snarveier ble importert.")
                        .to_string(),
                ),
                Err(error) => warnings.push(format!(
                    "{}: {}",
                    language.text(
                        "Shortcuts were not imported",
                        "Snarveier ble ikke importert"
                    ),
                    error.message(language)
                )),
            }
        }
        if !plan.unsupported_plugins.is_empty() {
            warnings.push(format!(
                "{} {}",
                language.text(
                    "Enabled plugins are unsupported and were not imported:",
                    "Aktiverte programtillegg støttes ikke og ble ikke importert:",
                ),
                plan.unsupported_plugins.join(", ")
            ));
        }
        if successes.is_empty() && warnings.is_empty() {
            successes.push(
                language
                    .text(
                        "No settings needed to be changed.",
                        "Ingen innstillinger trengte endring.",
                    )
                    .to_string(),
            );
        }
        self.show_vault_import_report(
            language.text(
                "Vault settings import report",
                "Rapport for import av hvelvinnstillinger",
            ),
            successes.into_iter().chain(warnings).collect(),
            window,
            cx,
        );
    }

    fn persist_vault_import_dismissal(
        &mut self,
        root: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.vault.read(cx).root.as_deref() != Some(root) {
            return;
        }
        let key = root.to_string_lossy().to_string();
        if self.settings.vault_settings_import_dismissed.contains(&key) {
            return;
        }
        let latest = cx
            .global::<crate::windows::WindowManager>()
            .settings
            .clone();
        let mut proposed = crate::windows::with_local_settings(latest.clone(), &self.settings);
        proposed.vault_settings_import_dismissed.push(key);
        match crate::windows::publish_checked(self.window_id, &latest, &proposed, cx) {
            Ok(saved) => {
                self.apply_settings_inner(saved.clone(), window, cx, false);
                self.settings_baseline = saved;
            }
            Err(error) => self.note_status(
                format!(
                    "{}: {error}",
                    self.tr(
                        "Could not remember that import offer",
                        "Kunne ikke huske dette importtilbudet",
                    )
                ),
                cx,
            ),
        }
    }

    fn show_vault_import_report(
        &mut self,
        title: impl Into<SharedString>,
        lines: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self.settings.language;
        let ok = language.text("OK", "OK");
        let title = title.into();
        let scroll = ScrollHandle::new();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let body = div()
                .id("vault-settings-import-report")
                .h(px(300.))
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .child(
                    v_flex().w_full().gap_2().children(
                        lines
                            .iter()
                            .cloned()
                            .map(|line| div().flex_none().child(line)),
                    ),
                )
                .vertical_scrollbar(&scroll);
            dialog
                .title(title.clone())
                .w(px(520.))
                .ok_text(ok)
                .child(body)
                .on_close({
                    let view = view.clone();
                    move |_, window, cx| {
                        let view = view.clone();
                        window.defer(cx, move |window, cx| {
                            view.update(cx, |this, cx| this.refocus(window, cx));
                        });
                    }
                })
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
        self.apply_settings_inner(settings, window, cx, true);
    }

    fn apply_settings_inner(
        &mut self,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
        publish: bool,
    ) {
        if self.settings.properties_visibility != settings.properties_visibility {
            for doc in &self.docs {
                doc.entity.update(cx, |doc, _| {
                    doc.callout_folds = preview::CalloutFolds::default();
                });
            }
        }
        let files_changed = self.settings.show_other_files != settings.show_other_files;
        let tree_sort_changed = self.settings.tree_sort != settings.tree_sort;
        let templates_dir_changed = self.settings.templates_dir != settings.templates_dir;
        let link_format_changed = self.settings.link_format != settings.link_format;
        let ignore_filters_changed =
            self.settings.user_ignore_filters != settings.user_ignore_filters;
        let focus_mode_changed = self.focus_mode != settings.focus_mode
            || self.settings.focus_mode != settings.focus_mode;
        let language_changed = self.settings.language != settings.language;
        let starred_changed = self.settings.starred != settings.starred;
        self.focus_mode = settings.focus_mode;
        self.settings = settings.clone();
        if starred_changed {
            self.vault.update(cx, |vault, cx| {
                vault.starred = settings.starred.iter().cloned().collect();
                cx.emit(VaultEvent::Starred);
            });
        }
        if language_changed {
            self.folder_search.update(cx, |input, cx| {
                input.set_placeholder(
                    settings
                        .language
                        .text("Search this folder…", "Søk i denne mappen…"),
                    window,
                    cx,
                );
            });
            self.explorer_search.update(cx, |input, cx| {
                input.set_placeholder(
                    settings
                        .language
                        .text("Find files or paths…", "Finn filer eller stier…"),
                    window,
                    cx,
                );
            });
        }
        if files_changed || tree_sort_changed || templates_dir_changed || ignore_filters_changed {
            self.vault.update(cx, |vault, cx| {
                vault.tree_sort = settings.tree_sort;
                vault.show_other_files = settings.show_other_files;
                vault.templates_dir = settings.templates_dir.clone();
                vault.set_ignore_filters(&settings.user_ignore_filters);
                vault.refresh(cx);
            });
        }
        if link_format_changed {
            self.vault
                .update(cx, |vault, _cx| vault.set_link_format(settings.link_format));
        }
        if files_changed {
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
            doc.entity.update(cx, |doc, cx| {
                doc.apply_settings(&settings, window, cx);
                if focus_mode_changed {
                    doc.set_focus_mode(settings.focus_mode, cx);
                }
            });
        }
        self.retheme_previews(cx);
        // The agent panel is a cached view that reads settings such as the
        // language, so it must be redrawn alongside the workspace.
        self.agent_panel.update(cx, |_, cx| cx.notify());
        // Graphs are cached too and read the language through the workspace.
        for graph in self.graph.iter().chain(&self.graph_dock) {
            graph.update(cx, |_, cx| cx.notify());
        }
        self.settings = settings;
        if publish {
            self.persist_settings(cx);
        } else {
            let settings = self.settings.clone();
            self.settings_view.update(cx, |view, cx| {
                view.sync_external_settings(&settings, window, cx)
            });
        }
        cx.notify();
    }

    pub(crate) fn persist_settings(&mut self, cx: &mut Context<Self>) {
        if self.restoring_session || cx.global::<crate::windows::WindowManager>().restoring {
            return;
        }
        crate::windows::publish(self.window_id, &self.settings_baseline, &self.settings, cx);
        self.settings_baseline = self.settings.clone();
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
        if crate::canvas::rejects_canvas_kind_change(&path, &new_path, path.is_dir()) {
            self.note_status(
                self.tr(
                    "Renaming cannot convert between Canvas and text documents.",
                    "Et nytt filnavn kan ikke konvertere mellom Canvas- og tekstdokumenter.",
                ),
                cx,
            );
            return;
        }
        let canvas_rewrites = match self.vault.read(cx).root.clone() {
            Some(root) if path.starts_with(&root) && new_path.starts_with(&root) => {
                match self.prepare_canvas_move_rewrites(&path, &new_path, cx) {
                    Ok(rewrites) => rewrites,
                    Err(_) => {
                        self.note_status(
                            self.tr(
                                "Cannot rename or move because Canvas references have a conflict.",
                                "Kan ikke gi nytt navn eller flytte fordi Canvas-referanser har en konflikt.",
                            ),
                            cx,
                        );
                        return;
                    }
                }
            }
            _ => Vec::new(),
        };
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
            for note in vault
                .notes
                .iter()
                .filter(|note| !crate::canvas::is_canvas(note))
            {
                let link_source = if moved && note == &path {
                    new_path.as_path()
                } else {
                    note.as_path()
                };
                let text = match self.docs.iter().find(|d| d.entity.read(cx).path == *note) {
                    Some(doc) => doc.entity.read(cx).editor.read(cx).value().to_string(),
                    None => match std::fs::read_to_string(note) {
                        Ok(text) => text,
                        Err(_) => continue,
                    },
                };
                let mut edits: Vec<_> = vault
                    .link_spans_to(&text, note, &path)
                    .into_iter()
                    .map(|range| {
                        (
                            range.clone(),
                            if moved {
                                if crate::vault::is_relative_link_path(&text[range.clone()]) {
                                    vault
                                        .root
                                        .as_deref()
                                        .and_then(|root| {
                                            crate::vault::relative_link_path_for(
                                                root,
                                                link_source,
                                                &new_path,
                                            )
                                        })
                                        .map(|relative| {
                                            crate::vault::retarget_link_path(
                                                &text[range.clone()],
                                                &relative,
                                            )
                                        })
                                        .unwrap_or_else(|| {
                                            crate::vault::retarget_link_full(
                                                &text[range.clone()],
                                                &wiki_rel,
                                            )
                                        })
                                } else {
                                    crate::vault::retarget_link_full(
                                        &text[range.clone()],
                                        &wiki_rel,
                                    )
                                }
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
                                        if crate::vault::is_relative_link_path(&text[range.clone()])
                                        {
                                            vault
                                                .root
                                                .as_deref()
                                                .and_then(|root| {
                                                    crate::vault::relative_markdown_link_path_for(
                                                        root,
                                                        link_source,
                                                        &new_path,
                                                    )
                                                })
                                                .map(|relative| {
                                                    crate::vault::retarget_md_link_path(
                                                        &text[range.clone()],
                                                        &relative,
                                                    )
                                                })
                                                .unwrap_or_else(|| {
                                                    crate::vault::retarget_md_link_full(
                                                        &text[range.clone()],
                                                        &link_rel,
                                                    )
                                                })
                                        } else {
                                            crate::vault::retarget_md_link_full(
                                                &text[range.clone()],
                                                &link_rel,
                                            )
                                        }
                                    } else {
                                        crate::vault::retarget_md_link(
                                            &text[range.clone()],
                                            &file_name,
                                        )
                                    },
                                )
                            }),
                    );
                }
                if note == &path {
                    if let Some(root) = vault.root.as_deref() {
                        edits.extend(crate::vault::relative_link_edits_for_move(
                            root, &path, &new_path, &text,
                        ));
                    }
                }
                let edits = crate::vault::non_overlapping_text_edits(edits);
                if !edits.is_empty() {
                    rewrites.push((note.clone(), edits));
                }
            }
        }
        if std::fs::rename(&path, &new_path).is_ok() {
            self.repoint_moved_entry(&path, &new_path, cx);
            let canvas_complete = self.apply_canvas_move_rewrites(canvas_rewrites, window, cx);
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
                if !canvas_complete {
                    self.tr(
                        "Renamed, but some Canvas references were not updated.",
                        "Fikk nytt navn, men noen Canvas-referanser ble ikke oppdatert.",
                    )
                    .to_string()
                } else if updated > 0 {
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
            self.persist_tabs(cx);
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
    fn move_tree_entry(
        &mut self,
        src: PathBuf,
        dest_dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let rewrites = match self.prepare_canvas_move_rewrites(&src, &dest, cx) {
            Ok(rewrites) => rewrites,
            Err(_) => {
                self.note_status(
                    self.tr(
                        "Cannot move this path because Canvas references have a conflict.",
                        "Kan ikke flytte stien fordi Canvas-referanser har en konflikt.",
                    ),
                    cx,
                );
                return;
            }
        };
        match crate::explorer::move_entry(&root, &src, &dest) {
            Ok(()) => {
                self.repoint_moved_entry(&src, &dest, cx);
                let complete = self.apply_canvas_move_rewrites(rewrites, window, cx);
                self.move_undo.push((src, dest));
                self.note_status(
                    self.tr(
                        if complete {
                            "Moved · Undo available in explorer"
                        } else {
                            "Moved, but some Canvas references were not updated."
                        },
                        if complete {
                            "Flyttet · Angre er tilgjengelig i filutforskeren"
                        } else {
                            "Flyttet, men noen Canvas-referanser ble ikke oppdatert."
                        },
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
        let Some(target_path) = self.active_doc().map(|doc| doc.read(cx).path.clone()) else {
            return;
        };
        let Some(stem) = target_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            return;
        };
        let target = self.wikilink_target(&target_path, &note, cx);
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
                        editor.replace(format!("[[{target}]]"), window, cx);
                    });
                }
            });
        } else if let Ok(text) = std::fs::read_to_string(&note) {
            if let Some(start) = crate::vault::plain_mention_offset(&text, &needle) {
                let mut text = text;
                text.replace_range(start..start + stem.len(), &format!("[[{target}]]"));
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
        match self.active_doc().cloned() {
            Some(doc) => self.show_template_picker(TemplatePick::Insert(doc), window, cx),
            None => self.note_status("Open a note first", cx),
        }
    }

    /// Palette "New note from template…" — pick a template, then name
    /// the note.
    fn show_new_from_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_template_picker(TemplatePick::NewNote, window, cx);
    }

    /// The template list; `pick` says what a click does.
    fn show_template_picker(
        &mut self,
        pick: TemplatePick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            self.note_status("Open a folder first", cx);
            return;
        };
        let files = template_files(&root, &self.settings.templates_dir);
        let view = cx.entity();
        let title = match &pick {
            TemplatePick::Insert(_) => "Insert template",
            TemplatePick::NewNote => "New note from template",
            TemplatePick::Folder(_) => "Template for new notes in this folder",
        };
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut list = v_flex().w_full().py_1();
            let row = |id: (&'static str, usize), label: String, file: Option<PathBuf>| {
                let (view, pick) = (view.clone(), pick.clone());
                div()
                    .id(id)
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted))
                    .child(div().text_sm().text_color(theme.foreground).child(label))
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let (view, pick, file) = (view.clone(), pick.clone(), file.clone());
                        window.defer(cx, move |window, cx| {
                            view.update(cx, |this, cx| {
                                this.on_template_picked(pick, file, window, cx)
                            });
                        });
                    })
            };
            if matches!(pick, TemplatePick::Folder(_)) {
                list = list.child(row(("template-none", 0), "No template".into(), None));
            }
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
                list = list.child(row(("template-row", ix), name, Some(file.clone())));
            }
            dialog
                .title(title)
                .w(px(440.))
                .overlay_closable(true)
                .child(
                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                        list.max_h(px(360.)),
                    ),
                )
        });
    }

    /// A template-picker click: insert it, name a new note, or set (or,
    /// with `None`, clear) the folder's template for new notes.
    fn on_template_picked(
        &mut self,
        pick: TemplatePick,
        file: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (pick, file) {
            (TemplatePick::Insert(doc), Some(file)) => {
                let Ok(template) = std::fs::read_to_string(&file) else {
                    return;
                };
                let (path, text, selection) = {
                    let doc = doc.read(cx);
                    let selection = doc.selected_text(cx).map(|(s, _)| s).unwrap_or_default();
                    (
                        doc.path.clone(),
                        doc.editor.read(cx).value().to_string(),
                        selection,
                    )
                };
                let now = chrono::Local::now().naive_local();
                let job = TemplateJob {
                    template,
                    path,
                    now,
                    frontmatter: Some(text),
                    selection,
                };
                self.fill_template(
                    job,
                    move |this, expanded, window, cx| {
                        doc.update(cx, |doc, cx| {
                            doc.insert_template(&expanded.text, expanded.cursor, window, cx)
                        });
                        this.warn_unsupported(&expanded, cx);
                    },
                    window,
                    cx,
                );
            }
            (TemplatePick::NewNote, Some(file)) => self.prompt_note_from_template(file, window, cx),
            (TemplatePick::Folder(dir), file) => {
                let Some(root) = self.vault.read(cx).root.clone() else {
                    return;
                };
                let key = vault_rel(&root, &dir);
                let label = if key.is_empty() {
                    "the vault root".to_string()
                } else {
                    key.clone()
                };
                match file {
                    Some(file) => {
                        let name = file
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        self.settings
                            .folder_templates
                            .insert(key, vault_rel(&root, &file));
                        self.note_status(format!("New notes in {label} start from “{name}”"), cx);
                    }
                    None => {
                        self.settings.folder_templates.remove(&key);
                        self.note_status(format!("New notes in {label} start empty"), cx);
                    }
                }
                self.persist_settings(cx);
            }
            _ => {}
        }
    }

    /// The template new notes in `dir` start from, if one is set.
    fn folder_template(&self, dir: &Path, cx: &App) -> Option<PathBuf> {
        let root = self.vault.read(cx).root.clone()?;
        let template = self.settings.folder_templates.get(&vault_rel(&root, dir))?;
        Some(root.join(template)).filter(|p| p.is_file())
    }

    /// Create the note at `path` from `template` and open it — the
    /// template's prompts are asked first and `{{cursor}}` places the
    /// caret. A missing template gives a `# Title` heading.
    fn create_note(
        &mut self,
        path: PathBuf,
        template: Option<PathBuf>,
        now: chrono::NaiveDateTime,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(template) = template else {
            let title = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let heading = crate::templater::Expanded {
                text: format!("# {title}\n\n"),
                cursor: None,
                unsupported: 0,
                overflowed: false,
            };
            self.write_new_note(path, heading, window, cx);
            return;
        };
        let Ok(mut file) = std::fs::File::open(template) else {
            let title = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            self.write_new_note(
                path,
                crate::templater::Expanded {
                    text: format!("# {title}\n\n"),
                    cursor: None,
                    unsupported: 0,
                    overflowed: false,
                },
                window,
                cx,
            );
            return;
        };
        let mut bytes = Vec::new();
        let bounded =
            std::io::Read::take(&mut file, crate::templater::MAX_EXPANSION_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .is_ok()
                && bytes.len() <= crate::templater::MAX_EXPANSION_BYTES;
        let Ok(text) = String::from_utf8(bytes) else {
            self.note_status(
                self.tr("Could not read the template.", "Kunne ikke lese malen."),
                cx,
            );
            return;
        };
        if !bounded {
            self.note_status(
                self.tr(
                    "Template output exceeded the size limit; the note was not created.",
                    "Malutdata overskred størrelsesgrensen; notatet ble ikke opprettet.",
                ),
                cx,
            );
            return;
        }
        let job = TemplateJob {
            template: text,
            path: path.clone(),
            now,
            frontmatter: None,
            selection: String::new(),
        };
        self.fill_template(
            job,
            move |this, expanded, window, cx| this.write_new_note(path, expanded, window, cx),
            window,
            cx,
        );
    }

    fn write_new_note(
        &mut self,
        path: PathBuf,
        expanded: crate::templater::Expanded,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if path
            .parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_err())
            || std::fs::write(&path, &expanded.text).is_err()
        {
            self.note_status("Could not create the note", cx);
            return;
        }
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        self.open_document(path.clone(), window, cx);
        if let (Some(at), Some(doc)) = (expanded.cursor, self.active_doc().cloned()) {
            if doc.read(cx).path == path {
                doc.update(cx, |doc, cx| {
                    doc.editor
                        .update(cx, |editor, cx| editor.set_selected_range(at..at, cx));
                });
            }
        }
        self.warn_unsupported(&expanded, cx);
    }

    fn warn_unsupported(&mut self, expanded: &crate::templater::Expanded, cx: &mut Context<Self>) {
        if expanded.overflowed {
            self.note_status(
                self.tr(
                    "Template output exceeded the 4 MiB limit; the template was not applied.",
                    "Malutdata overskred grensen på 4 MiB; malen ble ikke brukt.",
                ),
                cx,
            );
            return;
        }
        if expanded.unsupported > 0 {
            self.note_status(
                format!(
                    "{} {}",
                    expanded.unsupported,
                    self.tr(
                        "template command(s) were left as written; scripts are not run.",
                        "malhandling(er) ble stående uendret; skript kjøres ikke.",
                    )
                ),
                cx,
            );
        }
    }

    /// Expand a template — after asking its `tp.system.prompt`/
    /// `suggester` questions — and hand the result to `then`.
    /// Cancelling a prompt drops it all.
    fn fill_template(
        &mut self,
        job: TemplateJob,
        then: impl FnOnce(&mut Self, crate::templater::Expanded, &mut Window, &mut Context<Self>)
            + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompts = crate::templater::prompts(&job.template);
        let done: PromptsDone = Rc::new(RefCell::new(Some(Box::new(
            move |this: &mut Self,
                  answers: Vec<String>,
                  window: &mut Window,
                  cx: &mut Context<Self>| {
                let expanded = this.expand_template_for(&job, &answers, cx);
                if expanded.overflowed {
                    this.warn_unsupported(&expanded, cx);
                    return;
                }
                then(this, expanded, window, cx);
            },
        ))));
        self.ask_prompts(prompts.into(), Vec::new(), done, window, cx);
    }

    fn expand_template_for(
        &self,
        job: &TemplateJob,
        answers: &[String],
        cx: &App,
    ) -> crate::templater::Expanded {
        let TemplateJob {
            template,
            path,
            now,
            frontmatter,
            selection,
        } = job;
        let root = self.vault.read(cx).root.clone().unwrap_or_default();
        let title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let rel = vault_rel(&root, path);
        let abs = path.to_string_lossy().to_string();
        let clipboard = cx.read_from_clipboard().and_then(|c| c.text());
        let canonical_root = root.canonicalize().ok();
        let include = |name: &str| {
            let root = canonical_root.as_ref()?;
            let path = find_vault_markdown(root, name)?;
            let file = std::fs::File::open(path).ok()?;
            let mut file = file.take(crate::templater::MAX_EXPANSION_BYTES as u64 + 1);
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).ok()?;
            if bytes.len() > crate::templater::MAX_EXPANSION_BYTES {
                return None;
            }
            String::from_utf8(bytes).ok()
        };
        let ctx = crate::templater::TemplateCtx {
            rel_path: &rel,
            abs_path: &abs,
            frontmatter: frontmatter.as_deref().unwrap_or(template),
            clipboard: clipboard.as_deref(),
            selection,
            include: Some(&include),
            answers,
            ..crate::templater::TemplateCtx::new(&title, *now)
        };
        crate::templater::expand_bounded(template, &ctx, crate::templater::MAX_EXPANSION_BYTES)
            .unwrap_or(crate::templater::Expanded {
                text: String::new(),
                cursor: None,
                unsupported: 0,
                overflowed: true,
            })
    }

    /// Ask template prompts one dialog at a time; `done` runs with the
    /// answers once all are given.
    fn ask_prompts(
        &mut self,
        mut pending: std::collections::VecDeque<crate::templater::Prompt>,
        answers: Vec<String>,
        done: PromptsDone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = pending.pop_front() else {
            let finish = done.borrow_mut().take();
            if let Some(finish) = finish {
                finish(self, answers, window, cx);
            }
            return;
        };
        let view = cx.entity();
        // Answer → next prompt, opened after this dialog closes.
        let next = move |answer: String, window: &mut Window, cx: &mut App| {
            let (view, pending, done) = (view.clone(), pending.clone(), done.clone());
            let mut answers = answers.clone();
            answers.push(answer);
            window.defer(cx, move |window, cx| {
                view.update(cx, |this, cx| {
                    this.ask_prompts(pending, answers, done, window, cx)
                });
            });
        };
        match prompt {
            crate::templater::Prompt::Text { question, default } => {
                let input = self.cell_input.clone();
                input.update(cx, |input, cx| input.set_value(default, window, cx));
                window.open_dialog(cx, move |dialog, _window, _cx| {
                    let (input, next) = (input.clone(), next.clone());
                    dialog
                        .title(question.clone())
                        .w(px(400.))
                        .child(div().w_full().child(Input::new(&input).appearance(true)))
                        .on_ok(move |_, window, cx| {
                            let answer = input.read(cx).value().to_string();
                            window.close_dialog(cx);
                            next(answer, window, cx);
                            false
                        })
                });
                let input = self.cell_input.clone();
                window.on_next_frame(move |window, cx| {
                    input.update(cx, |input, cx| input.focus(window, cx));
                });
            }
            crate::templater::Prompt::Choice {
                labels,
                values,
                placeholder,
            } => {
                window.open_dialog(cx, move |dialog, _window, cx| {
                    let theme = cx.theme();
                    let mut list = v_flex().w_full().py_1();
                    for (ix, label) in labels.iter().enumerate() {
                        let value = values.get(ix).cloned().unwrap_or_else(|| label.clone());
                        let next = next.clone();
                        list = list.child(
                            div()
                                .id(("template-choice", ix))
                                .w_full()
                                .px_3()
                                .py_1p5()
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.muted))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.foreground)
                                        .child(label.clone()),
                                )
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    next(value.clone(), window, cx);
                                }),
                        );
                    }
                    let title = if placeholder.is_empty() {
                        "Choose"
                    } else {
                        placeholder.as_str()
                    };
                    dialog
                        .title(title.to_string())
                        .w(px(400.))
                        .overlay_closable(true)
                        .child(list)
                });
            }
        }
    }

    /// Ask for the new note's name, then create it from `template`.
    fn prompt_note_from_template(
        &mut self,
        template: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = self.cell_input.clone();
        input.update(cx, |input, cx| input.set_value("Untitled", window, cx));
        let view = cx.entity();
        let title = self.tr("Name the new note", "Gi det nye notatet et navn");
        window.open_dialog(cx, move |dialog, _window, _cx| {
            dialog
                .title(title)
                .w(px(400.))
                .child(div().w_full().child(Input::new(&input).appearance(true)))
                .on_ok({
                    let view = view.clone();
                    let template = template.clone();
                    move |_, window, cx| {
                        let name = view.read(cx).cell_input.read(cx).value().to_string();
                        window.close_dialog(cx);
                        let (view, template) = (view.clone(), template.clone());
                        window.defer(cx, move |window, cx| {
                            view.update(cx, |this, cx| {
                                this.create_note_from_template(&template, &name, window, cx);
                            });
                        });
                        false
                    }
                })
        });
        let input = self.cell_input.clone();
        window.on_next_frame(move |window, cx| {
            input.update(cx, |input, cx| input.focus(window, cx));
        });
    }

    /// `<vault>/<name>.md` from `template` (see `create_note`).
    fn create_note_from_template(
        &mut self,
        template: &Path,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.vault.read(cx).root.clone() else {
            return;
        };
        let name = name.trim().trim_end_matches(".md").trim();
        if name.is_empty() || name.contains(['/', '\\']) {
            self.note_status("Give the note a name without slashes", cx);
            return;
        }
        let path = free_path(&root, &format!("{name}.md"));
        let now = chrono::Local::now().naive_local();
        self.create_note(path, Some(template.to_path_buf()), now, window, cx);
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
        self.open_wikilink_impl(target, false, None, window, cx);
    }

    /// ⌘+click: open the wikilink target in a new tab.
    pub fn open_wikilink_new_tab(
        &mut self,
        target: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_wikilink_impl(target, true, None, window, cx);
    }

    pub(crate) fn open_wikilink_from(
        &mut self,
        target: &str,
        source: Option<&Path>,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_wikilink_impl(target, new_tab, source, window, cx);
    }

    fn open_wikilink_impl(
        &mut self,
        target: &str,
        new_tab: bool,
        source_path: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = crate::pdf::PdfTarget::parse(target) {
            let vault = self.vault.read(cx);
            let doc_path = source_path
                .map(Path::to_path_buf)
                .or_else(|| self.active_doc().map(|doc| doc.read(cx).path.clone()));
            let path = target
                .resolve(
                    doc_path.as_deref(),
                    vault.root.as_deref(),
                    &vault.explorer_path_snapshot(),
                )
                .and_then(|path| path.canonicalize().ok());
            if let Some(path) = path {
                self.open_document_impl(path.clone(), new_tab, false, window, cx);
                if let Some(pdf) = self
                    .active_doc()
                    .filter(|doc| doc.read(cx).path == path)
                    .and_then(|doc| doc.read(cx).pdf.clone())
                {
                    pdf.update(cx, |pdf, cx| pdf.jump(target.page, cx));
                }
            } else {
                self.note_status(
                    format!(
                        "{}: {}",
                        self.tr("PDF not found", "Fant ikke PDF-filen"),
                        target.file
                    ),
                    cx,
                );
            }
            return;
        }
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
        let source_path = source_path
            .map(Path::to_path_buf)
            .or_else(|| self.active_doc().map(|doc| doc.read(cx).path.clone()));
        let resolved = if note.is_empty() {
            // `[[#heading]]` — an anchor inside the open note.
            source_path.clone()
        } else {
            source_path
                .as_deref()
                .and_then(|source| self.vault.read(cx).resolve_wikilink_from(note, source))
                .or_else(|| self.vault.read(cx).resolve_wikilink(note))
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
                // `[[file.pdf]]` — an attachment, never a note to create.
                if Path::new(note)
                    .extension()
                    .is_some_and(|e| e != "md" && e != "base")
                {
                    let found = self
                        .vault
                        .read(cx)
                        .root
                        .clone()
                        .and_then(|root| find_vault_file(&root, note));
                    match found {
                        Some(path) => self.open_document_impl(path, new_tab, false, window, cx),
                        None => self.note_status(format!("No file “{note}” in the vault"), cx),
                    }
                    return;
                }
                self.create_note_for_wikilink(note, window, cx);
            }
        }
    }

    /// After a save: every `[[note#^id]]` in `doc` whose target lacks
    /// that id gets it written onto the block it hashes to — the second
    /// half of `[[note#^` completion for blocks without an id. Open
    /// targets are edited in their editor; others on disk.
    fn add_linked_block_ids(
        &mut self,
        doc: &Entity<Document>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (text, own_path) = {
            let doc = doc.read(cx);
            (doc.editor.read(cx).value().to_string(), doc.path.clone())
        };
        if !text.contains("#^") {
            return;
        }
        for (note, id) in crate::document::block_link_targets(&text) {
            let target = if note.is_empty() {
                Some(own_path.clone())
            } else {
                self.vault.read(cx).resolve_wikilink_from(&note, &own_path)
            };
            let Some(target) = target else {
                continue;
            };
            let open = self
                .docs
                .iter()
                .map(|d| d.entity.clone())
                .find(|d| d.read(cx).path == target);
            match open {
                Some(target_doc) => {
                    let target_text = target_doc.read(cx).editor.read(cx).value().to_string();
                    if let Some((at, ins)) =
                        crate::document::linked_block_id_edit(&target_text, &id)
                    {
                        target_doc.update(cx, |d, cx| d.insert_at(at, &ins, window, cx));
                    }
                }
                None => {
                    let Ok(target_text) = std::fs::read_to_string(&target) else {
                        continue;
                    };
                    if let Some((at, ins)) =
                        crate::document::linked_block_id_edit(&target_text, &id)
                    {
                        let mut new_text = target_text;
                        new_text.insert_str(at, &ins);
                        if let Some(root) = self.vault.read(cx).root.clone() {
                            crate::history::snapshot_before_write(&root, &target, &new_text);
                        }
                        let _ = std::fs::write(&target, new_text);
                    }
                }
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
        // `[[note#^block-id]]` — the first line of the block it names.
        if let Some(block) = anchor.strip_prefix('^') {
            let lines: Vec<&str> = raw.split('\n').collect();
            match crate::preview::block_lines(&lines, block) {
                Some((start, _)) => {
                    doc.update(cx, |doc, cx| doc.jump_to_line(start + 1, window, cx))
                }
                None => self.note_status(format!("No block “{}”", anchor), cx),
            }
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
        if let Some(template) = path.parent().and_then(|d| self.folder_template(d, cx)) {
            let now = chrono::Local::now().naive_local();
            self.create_note(path, Some(template), now, window, cx);
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

    pub(crate) fn open_canvas_reference(
        &mut self,
        path: PathBuf,
        subpath: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_document(path.clone(), window, cx);
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
            })
        {
            if let Some(anchor) = subpath {
                self.jump_to_anchor(&anchor, window, cx);
            }
        } else if crate::file_preview::extension(&path) == "pdf" {
            if let Some(target) = crate::pdf::PdfTarget::parse(&format!(
                "{}#{}",
                path.to_string_lossy(),
                subpath.as_deref().unwrap_or("").trim_start_matches('#')
            )) {
                if let Some(pdf) = self
                    .active_doc()
                    .filter(|document| document.read(cx).path == path)
                    .and_then(|document| document.read(cx).pdf.clone())
                {
                    pdf.update(cx, |pdf, cx| pdf.jump(target.page, cx));
                }
            }
        }
    }

    pub fn navigate_search_result(
        &mut self,
        path: PathBuf,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_document(path.clone(), window, cx);
        let Some(line) = line else {
            return;
        };
        if self.settings.view_mode != ViewMode::Preview {
            if let Some(document) = self
                .active_doc()
                .filter(|document| document.read(cx).path == path)
                .cloned()
            {
                self.pending_editor_focus = Some((document, Some(line)));
            }
        }
        cx.on_next_frame(window, move |workspace, window, cx| {
            if window.has_active_dialog(cx)
                || workspace.graph.is_some()
                || workspace.folder.is_some()
            {
                return;
            }
            let Some(document) = workspace
                .active_doc()
                .filter(|document| document.read(cx).path == path)
                .cloned()
            else {
                return;
            };
            document.update(cx, |document, cx| {
                document.reveal_preview_line(line.saturating_sub(1), cx);
            });
        });
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
    pub fn remember_base_view(&mut self, path: String, view_ix: usize, cx: &mut Context<Self>) {
        self.settings.base_views.insert(path, view_ix);
        self.persist_settings(cx);
    }

    /// Interactive header sort per `.base` view — saved under
    /// `{path}::{view_name}` so a column name survives view reorders.
    pub fn remember_base_sort(
        &mut self,
        path: String,
        view: String,
        col: Option<(String, bool)>,
        cx: &mut Context<Self>,
    ) {
        let key = format!("{path}::{view}");
        if let Some(col) = col {
            self.settings.base_sorts.insert(key, col);
        } else {
            self.settings.base_sorts.remove(&key);
        }
        self.persist_settings(cx);
    }

    pub fn vault_entity(&self) -> &Entity<Vault> {
        &self.vault
    }

    pub fn language(&self) -> crate::settings::Language {
        self.settings.language
    }

    pub(crate) fn search_service(&self) -> SearchService {
        self.search_service.clone()
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
                        Button::new("toggle-local-graph-panel")
                            .ghost()
                            .small()
                            .tooltip(
                                self.tr("Toggle local graph panel", "Vis/skjul lokalt grafpanel"),
                            )
                            .icon(assets::IconName::Waypoints)
                            .selected(self.settings.graph_dock_open)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_local_graph_panel(
                                    &ToggleLocalGraphPanel,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new("toggle-agent-panel")
                            .ghost()
                            .small()
                            .tooltip(self.tr("Toggle agent panel", "Vis/skjul agentpanel"))
                            .icon(assets::IconName::MessageSquareQuote)
                            .selected(self.settings.agent_open)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_agent_panel(&ToggleAgentPanel, window, cx);
                            })),
                    )
                    .child(
                        Button::new("toggle-inspector")
                            .ghost()
                            .small()
                            .tooltip(self.tr("Toggle inspector", "Vis/skjul inspektør"))
                            .icon(assets::IconName::TableProperties)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.inspector_open = !this.settings.inspector_open;
                                this.persist_settings(cx);
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
                        Button::new("workspace-layouts")
                            .ghost()
                            .small()
                            .label(self.tr("Layouts…", "Oppsett…"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.show_workspace_picker(PaletteCmd::LoadWorkspace, window, cx);
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

    /// Bookmarks pane mounted at the top of the sidebar.
    fn render_starred(&self, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_bookmarks(cx)
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
                        this.persist_settings(cx);
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
                        this.persist_settings(cx);
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
                        this.persist_settings(cx);
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
        // Monday-first, unless the weekly format counts Moment's
        // Sunday-first locale weeks (`w`/`gggg`) — rows then match the
        // weekly notes the week column opens.
        let sunday_first = crate::bases::moment_locale_weeks(&self.settings.weekly_format);
        let lead = if sunday_first {
            first.weekday().num_days_from_sunday()
        } else {
            first.weekday().num_days_from_monday()
        } as usize;
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

        // Week-number column: click opens that row's weekly note.
        let week_token = if sunday_first { "w" } else { "W" };
        let week_cell = |row: usize| {
            let start =
                first - chrono::Duration::days(lead as i64) + chrono::Duration::weeks(row as i64);
            let has_note = self
                .period_path(Period::Week, start, cx)
                .is_some_and(|p| p.exists());
            div()
                .id(("cal-week", row))
                .w(px(18.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .cursor_pointer()
                .text_color(if has_note {
                    theme.info
                } else {
                    theme.muted_foreground
                })
                .hover(|s| s.bg(theme.muted.opacity(0.5)))
                .child(crate::bases::format_moment(
                    start.and_hms_opt(0, 0, 0).unwrap_or_default(),
                    week_token,
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_period_at(Period::Week, start, window, cx);
                }))
        };

        let mut weeks: Vec<Div> = Vec::new();
        let mut week = h_flex().gap_0p5().child(week_cell(0));
        for _ in 0..lead {
            week = week.child(div().w(px(26.)).h(px(20.)));
        }
        let mut slot = lead;
        for day in 1..=days_in_month {
            week = week.child(day_cell(day));
            slot += 1;
            if slot.is_multiple_of(7) {
                weeks.push(week);
                week = h_flex().gap_0p5().child(week_cell(weeks.len()));
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
                        this.persist_settings(cx);
                        cx.notify();
                    })),
            )
            .when(self.cal_open, |this| {
                let days = if sunday_first {
                    ["S", "M", "T", "W", "T", "F", "S"]
                } else {
                    ["M", "T", "W", "T", "F", "S", "S"]
                };
                let head = h_flex()
                    .gap_0p5()
                    .child(div().w(px(18.)))
                    .children(days.iter().map(|d| {
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
                        this.persist_settings(cx);
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
                        this.persist_settings(cx);
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
                        move |src, window, cx| {
                            let src = src.clone();
                            view.update(cx, |this, cx| {
                                let Some(root) = this.vault.read(cx).root.clone() else {
                                    return;
                                };
                                this.move_tree_entry(src, root, window, cx);
                            });
                        }
                    })
                    .drag_over::<ExternalPaths>(|style, _, _, cx| {
                        style.bg(cx.theme().accent.opacity(0.2))
                    })
                    .on_drop::<ExternalPaths>({
                        let view = view.clone();
                        move |paths, window, cx| {
                            view.update(cx, |this, cx| {
                                let Some(root) = this.vault.read(cx).root.clone() else {
                                    return;
                                };
                                this.copy_external_into(paths.paths(), root, window, cx);
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
                                                                let drag_label = item.label.clone();
                                                                label.cursor_pointer()
                                                                    .on_drag(path.clone(), move |_, _, _, cx| {
                                                                        cx.new(|_| TreeDragPreview {
                                                                            label: drag_label.clone(),
                                                                        })
                                                                    })
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
                                        let row = if is_folder {
                                            let dest_dir = path.clone();
                                            row.drag_over::<PathBuf>(|style, _, _, cx| {
                                                style.bg(cx.theme().accent.opacity(0.2))
                                            })
                                            .on_drop::<PathBuf>({
                                                let view = render_view.clone();
                                                let dest_dir = dest_dir.clone();
                                                move |src, window, cx| {
                                                    let src = src.clone();
                                                    let dest_dir = dest_dir.clone();
                                                    view.update(cx, |this, cx| {
                                                        this.move_tree_entry(src, dest_dir, window, cx);
                                                    });
                                                }
                                            })
                                            .drag_over::<ExternalPaths>(|style, _, _, cx| {
                                                style.bg(cx.theme().accent.opacity(0.2))
                                            })
                                            .on_drop::<ExternalPaths>({
                                                let view = render_view.clone();
                                                move |paths, window, cx| {
                                                    let dest_dir = dest_dir.clone();
                                                    view.update(cx, |this, cx| {
                                                        this.copy_external_into(
                                                            paths.paths(),
                                                            dest_dir,
                                                            window,
                                                            cx,
                                                        );
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

    fn render_tab_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        self.render_group_tab_bar(self.layout.focused(), cx)
    }

    /// Is the active doc an image file (rendered as a picture, not a
    /// text editor)? Hides the Source/Split/Preview switcher.
    fn active_doc_is_image(&self, cx: &App) -> bool {
        self.active_doc()
            .map(|d| d.read(cx).is_read_only())
            .unwrap_or(false)
    }

    fn render_preview(&self, doc: &Entity<Document>, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let undo_doc = doc.clone();
        let redo_doc = doc.clone();
        let images = doc.read(cx).image_preview.clone();
        let (
            state,
            banner,
            folds,
            embeds,
            query_embeds,
            mentions,
            mentions_open,
            has_frontmatter,
            doc_path,
            style,
        ) = {
            let doc = doc.read(cx);
            (
                doc.preview.clone(),
                doc.banner.clone(),
                doc.callout_folds.clone(),
                doc.base_embeds.clone(),
                doc.query_embeds.clone(),
                doc.linked_mentions.clone(),
                doc.mentions_open,
                crate::properties::frontmatter_span(doc.editor.read(cx).value().as_ref()).is_some(),
                doc.path.clone(),
                doc.resolved_style.clone(),
            )
        };
        let base_ctx = preview::PreviewCtx {
            properties_visibility: self.settings.properties_visibility,
            language: self.settings.language,
            vault: self.vault.clone(),
            workspace: view.downgrade(),
            views: embeds,
            query_views: query_embeds,
            pdf_views: doc.read(cx).pdf_embeds.clone(),
            images: Some(images.clone()),
            doc_path: Some(doc_path),
            depth: 0,
            style,
            catalog: cx
                .try_global::<crate::style_snippets::StyleSnippetCatalog>()
                .map(|catalog| catalog.snapshot.clone()),
            enabled_style_snippets: self.settings.enabled_style_snippets.clone(),
        };
        let theme = cx.theme();
        let footer_colors = (theme.foreground, theme.muted_foreground);
        let note_classes = doc.read(cx).css_classes.clone();
        let readable = if note_classes.iter().any(|class| class == "wide") {
            false
        } else if note_classes
            .iter()
            .any(|class| class == "narrow" || class == "readable")
            || base_ctx.style.readable_width.is_some()
        {
            true
        } else {
            self.settings.readable_width
        };
        let readable_width = base_ctx.style.readable_width.unwrap_or(760.);
        v_flex()
            .track_focus(&doc.read(cx).preview_focus)
            .key_context("RistaPreview")
            .on_action(cx.listener(move |this, _: &input::Undo, _, cx| {
                this.pending_preview_history
                    .push((undo_doc.clone(), Box::new(input::Undo)));
                cx.notify();
            }))
            .on_action(cx.listener(move |this, _: &input::Redo, _, cx| {
                this.pending_preview_history
                    .push((redo_doc.clone(), Box::new(input::Redo)));
                cx.notify();
            }))
            .size_full()
            .overflow_hidden()
            .when_some(
                base_ctx.style.colors.get("background").copied(),
                |this, color| this.bg(color),
            )
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
                gpui_kit::base::TextView::new(&state)
                    .style(preview::text_style(&base_ctx, cx))
                    .when_some(base_ctx.style.typography.body_font.clone(), |this, font| {
                        this.font_family(font)
                    })
                    .when_some(base_ctx.style.typography.body_size, |this, size| {
                        this.text_size(px(size))
                    })
                    .when_some(base_ctx.style.typography.line_height, |this, height| {
                        this.line_height(relative(height))
                    })
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
                    .when(readable, |this| this.max_w(px(readable_width)).mx_auto())
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
            .on_prepaint(move |_, _, cx| {
                // Inline plugins are measured at max-content, so percentages
                // cannot constrain an image to the actual text column.
                let width = state.read(cx).bounds().size.width;
                if images.constrain_width(width, cx) {
                    state.update(cx, |state, cx| state.invalidate_inline_layout(cx));
                }
            })
    }

    fn render_editor_panes(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(page) = &self.folder {
            let page = self.render_folder_page(page, cx);
            return v_flex()
                .size_full()
                .when(!self.zen, |this| this.child(self.render_tab_bar(cx)))
                .child(div().flex_1().min_h_0().child(page))
                .into_any_element();
        }
        let root = self.layout.root().clone();
        self.render_layout_node(&root, cx)
    }

    fn render_layout_node(&self, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        match node {
            Node::Group(group) => self.render_editor_group(group.id, cx),
            Node::Split {
                id,
                axis,
                sizes,
                first,
                second,
            } => {
                let split_id = *id;
                let first = self.render_layout_node(first, cx);
                let second = self.render_layout_node(second, cx);
                let view = cx.entity();
                canvas_panels(("editor-split", id.value()), *axis)
                    .child(
                        resizable_panel()
                            .when_some(sizes.map(|sizes| px(sizes[0])), |panel, size| {
                                panel.size(size)
                            })
                            .child(div().size_full().min_w_0().min_h_0().child(first)),
                    )
                    .child(
                        resizable_panel()
                            .when_some(sizes.map(|sizes| px(sizes[1])), |panel, size| {
                                panel.size(size)
                            })
                            .child(div().size_full().min_w_0().min_h_0().child(second)),
                    )
                    .on_resize(move |state, _, cx| {
                        let sizes = state.read(cx).sizes().clone();
                        if sizes.len() == 2 {
                            let measured = [sizes[0] / px(1.), sizes[1] / px(1.)];
                            view.update(cx, |this, cx| {
                                if this.layout.set_split_sizes(split_id, measured) {
                                    this.persist_tabs(cx);
                                }
                            });
                        }
                    })
                    .into_any_element()
            }
        }
    }

    fn render_group_tab_bar(&self, group: GroupId, cx: &mut Context<Self>) -> AnyElement {
        let tokens = self.layout.group_tokens(group);
        let selected = self.layout.selected(group);
        let mode = self
            .layout
            .group_specs()
            .into_iter()
            .find(|(id, _, _, _)| *id == group)
            .map(|(_, _, _, mode)| mode)
            .unwrap_or_default();
        let tabs = tokens
            .iter()
            .filter_map(|token| {
                self.doc_index_for_token(*token)
                    .map(|index| (*token, index))
            })
            .collect::<Vec<_>>();
        let mut title_counts = HashMap::new();
        for (_, ix) in &tabs {
            let doc = self.docs[*ix].entity.read(cx);
            let title = if doc.is_read_only()
                || doc.path.extension().and_then(|ext| ext.to_str()) == Some("base")
            {
                doc.file_name()
            } else {
                doc.title()
            };
            *title_counts.entry(title).or_insert(0usize) += 1;
        }
        let selected_index = tabs
            .iter()
            .position(|(token, _)| Some(*token) == selected)
            .unwrap_or(0);
        let group_tokens = tabs.iter().map(|(token, _)| *token).collect::<Vec<_>>();
        let group_mode_control = if selected
            .and_then(|token| self.doc_tokens.iter().position(|item| *item == token))
            .and_then(|ix| self.docs.get(ix))
            .is_some_and(|doc| doc.entity.read(cx).is_read_only())
        {
            div().into_any_element()
        } else {
            let compact = self.compact_mode_panes.contains(&group);
            self.render_pane_view_mode_tabs(group, mode, compact, cx)
                .into_any_element()
        };
        let tab_bar = TabBar::new(("pane-tabs", group.value()))
            .segmented()
            .small()
            .bg(cx.theme().transparent)
            .selected_index(selected_index)
            .children(tabs.into_iter().enumerate().map(|(index, (token, ix))| {
                let (mut title, dirty, icon) = {
                    let doc = self.docs[ix].entity.read(cx);
                    let title = if doc.is_read_only()
                        || doc.path.extension().and_then(|ext| ext.to_str()) == Some("base")
                    {
                        doc.file_name()
                    } else {
                        doc.title()
                    };
                    let icon = match doc
                        .path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(str::to_ascii_lowercase)
                        .as_deref()
                    {
                        Some("base") => assets::IconName::Database,
                        Some(ext) if IMAGE_EXTS.contains(&ext) => assets::IconName::FileImage,
                        _ => assets::IconName::FileText,
                    };
                    (title, doc.dirty, icon)
                };
                if title_counts.get(&title).copied().unwrap_or(0) > 1 {
                    if let Some(parent) = self.docs[ix]
                        .entity
                        .read(cx)
                        .path
                        .parent()
                        .and_then(Path::file_name)
                        .map(|name| name.to_string_lossy().to_string())
                    {
                        title = format!("{title} · {parent}");
                    }
                }
                let label = if dirty {
                    format!("{title} •")
                } else {
                    title.clone()
                };
                let preview = self.docs[ix].preview;
                let pinned = self.docs[ix].entity.read(cx).path.display().to_string();
                let pinned = self.settings.pinned_tabs.contains(&pinned);
                let view = cx.entity();
                let tab = Self::content_tab(label.clone(), icon)
                    .when(preview, |tab| tab.italic())
                    .selected(Some(token) == selected)
                    .suffix(if pinned {
                        Icon::new(assets::IconName::Pin)
                            .size_3p5()
                            .text_color(cx.theme().muted_foreground)
                            .into_any_element()
                    } else {
                        Button::new(("pane-close-tab", token.value()))
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Close)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_tab_token(token, window, cx);
                            }))
                            .into_any_element()
                    })
                    .on_drag(DraggedTab(token), {
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
                        move |source, window, cx| {
                            view.update(cx, |this, cx| {
                                this.move_tab_to_group(source.0, group, index, window, cx);
                            });
                        }
                    })
                    .on_mouse_up(gpui::MouseButton::Left, {
                        let view = view.clone();
                        move |event, _, cx| {
                            if event.click_count == 2 {
                                view.update(cx, |this, cx| {
                                    if let Some(doc) = this
                                        .doc_index_for_token(token)
                                        .and_then(|index| this.docs.get_mut(index))
                                    {
                                        doc.preview = false;
                                    }
                                    this.persist_tabs(cx);
                                });
                            }
                        }
                    })
                    .on_mouse_down(gpui::MouseButton::Middle, {
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.close_tab_token(token, window, cx));
                        }
                    })
                    .on_mouse_down(gpui::MouseButton::Right, {
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.tab_menu_token = Some(token);
                                this.activate_pane(group, token, window, cx);
                            });
                        }
                    });
                tab
            }))
            .on_click(cx.listener(move |this, &index, window, cx| {
                if let Some(token) = group_tokens.get(index).copied() {
                    this.activate_pane(group, token, window, cx);
                }
            }));
        let can_back = self.nav_pos > 0;
        let can_forward = self.nav_pos + 1 < self.nav_stack.len();
        let view = cx.entity();
        h_flex()
            .id(("pane-tab-strip", group.value()))
            .w_full()
            .h_11()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .child(
                h_flex()
                    .pl_3()
                    .child(
                        Button::new(("pane-nav-back", group.value()))
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::ChevronLeft)
                            .disabled(!can_back)
                            .on_click(cx.listener(|this, _, window, cx| this.nav_back(window, cx))),
                    )
                    .child(
                        Button::new(("pane-nav-forward", group.value()))
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::ChevronRight)
                            .disabled(!can_forward)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.nav_forward(window, cx);
                            })),
                    ),
            )
            .child(div().flex_1().min_w_0().child(tab_bar))
            .child(div().flex_shrink_0().pr_3().child(group_mode_control))
            .on_prepaint({
                let view = view.clone();
                move |bounds, _, cx| {
                    // Labels need room beside the tabs; narrow split panes
                    // fall back to icons so the tabs stay visible.
                    let compact = bounds.size.width < PANE_MODE_LABELS_MIN_WIDTH;
                    view.update(cx, |this, cx| {
                        let changed = if compact {
                            this.compact_mode_panes.insert(group)
                        } else {
                            this.compact_mode_panes.remove(&group)
                        };
                        if changed {
                            cx.notify();
                        }
                    });
                }
            })
            .drag_over::<PathBuf>(|style, _, _, cx| style.bg(cx.theme().accent.opacity(0.15)))
            .on_drop::<PathBuf>({
                let view = view.clone();
                move |path, window, cx| {
                    let path = path.clone();
                    view.update(cx, |this, cx| {
                        this.layout.focus(group);
                        this.open_document_new_tab(path, window, cx);
                    });
                }
            })
            .context_menu({
                let view = view.clone();
                move |menu, _window, cx| {
                    let workspace = view.read(cx);
                    let token = workspace
                        .tab_menu_token
                        .filter(|token| workspace.layout.group_for(*token) == Some(group))
                        .or_else(|| workspace.layout.selected(group));
                    let Some((token, pinned, _path)) = token.and_then(|token| {
                        workspace
                            .doc_index_for_token(token)
                            .and_then(|index| workspace.docs.get(index))
                            .map(|doc| {
                                let path = doc.entity.read(cx).path.clone();
                                (token, workspace.is_pinned(&path), path)
                            })
                    }) else {
                        return menu;
                    };
                    let pin_label = workspace.tr(
                        if pinned { "Unpin tab" } else { "Pin tab" },
                        if pinned { "Løsne fane" } else { "Fest fane" },
                    );
                    let reveal_label = workspace.tr("Reveal in tree", "Vis i filtrær");
                    let open_in_new_window =
                        workspace.tr("Open in New Window", "Åpne i nytt vindu");
                    let move_to_new_window =
                        workspace.tr("Move to New Window", "Flytt til nytt vindu");
                    let source_window_id = workspace.window_id;
                    let split_right = workspace.tr("Split right", "Del til høyre");
                    let split_down = workspace.tr("Split down", "Del ned");
                    let next_pane = workspace.tr("Next pane", "Neste redigeringsfelt");
                    let previous_pane = workspace.tr("Previous pane", "Forrige redigeringsfelt");
                    let close_label = workspace.tr("Close tab", "Lukk fane");
                    let close_others = workspace.tr("Close other tabs", "Lukk andre faner");
                    let close_right =
                        workspace.tr("Close tabs to the right", "Lukk faner til høyre");
                    menu.item(
                        PopupMenuItem::new(pin_label)
                            .icon(assets::IconName::Pin)
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| this.toggle_pin_token(token, cx))
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new(reveal_label)
                            .icon(assets::IconName::Crosshair)
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| {
                                        if let Some(path) = this
                                            .doc_index_for_token(token)
                                            .and_then(|index| this.docs.get(index))
                                            .map(|doc| doc.entity.read(cx).path.clone())
                                        {
                                            this.reveal_file(&path, cx);
                                        }
                                    })
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new(open_in_new_window)
                            .icon(assets::IconName::AppWindow)
                            .on_click({
                                let path = _path.clone();
                                move |_, window, cx| {
                                    let path = path.to_string_lossy().into_owned();
                                    window.defer(cx, move |window, cx| {
                                        window.dispatch_action(
                                            OpenPathInNewWindow { path }.boxed_clone(),
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new(move_to_new_window)
                            .icon(assets::IconName::AppWindow)
                            .on_click(move |_, _, cx| {
                                cx.defer(move |cx| {
                                    crate::windows::move_tab_to_new_window(
                                        source_window_id,
                                        Some(token),
                                        cx,
                                    );
                                });
                            }),
                    )
                    .separator()
                    .item(PopupMenuItem::new(split_right).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.split_tab_into_group(
                                    token,
                                    group,
                                    Axis::Horizontal,
                                    false,
                                    window,
                                    cx,
                                )
                            })
                        }
                    }))
                    .item(PopupMenuItem::new(split_down).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.split_tab_into_group(
                                    token,
                                    group,
                                    Axis::Vertical,
                                    false,
                                    window,
                                    cx,
                                )
                            })
                        }
                    }))
                    .item(PopupMenuItem::new(next_pane).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.cycle_pane(1, window, cx))
                        }
                    }))
                    .item(PopupMenuItem::new(previous_pane).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.cycle_pane(-1, window, cx))
                        }
                    }))
                    .separator()
                    .item(
                        PopupMenuItem::new(close_label)
                            .icon(assets::IconName::X)
                            .on_click({
                                let view = view.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.close_tab_token(token, window, cx)
                                    })
                                }
                            }),
                    )
                    .item(PopupMenuItem::new(close_others).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.close_others_except_token(token, window, cx)
                            })
                        }
                    }))
                    .item(PopupMenuItem::new(close_right).on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.close_tabs_right_of_token(token, window, cx)
                            })
                        }
                    }))
                }
            })
            .into_any_element()
    }

    fn render_editor_group(&self, group: GroupId, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.layout.selected(group);
        let mode = self
            .layout
            .group_specs()
            .into_iter()
            .find(|(id, _, _, _)| *id == group)
            .map(|(_, _, _, mode)| mode)
            .unwrap_or_default();
        let content = selected
            .and_then(|token| self.doc_tokens.iter().position(|item| *item == token))
            .and_then(|ix| self.docs.get(ix))
            .map(|doc| {
                self.render_document_pane(
                    &doc.entity,
                    match mode {
                        PaneMode::Source => ViewMode::Source,
                        PaneMode::Split => ViewMode::Split,
                        PaneMode::Preview => ViewMode::Preview,
                    },
                    cx,
                )
            })
            .unwrap_or_else(|| self.render_empty_editor(cx).into_any_element());
        let view = cx.entity();
        let group_view = cx.entity();
        v_flex()
            .id(("editor-group", group.value()))
            .overflow_hidden()
            .capture_any_mouse_down({
                let group_view = group_view.clone();
                move |_, _, cx| {
                    group_view.update(cx, |this, cx| this.focus_pane(group, cx));
                }
            })
            .on_drop::<DraggedTab>({
                let view = view.clone();
                move |source, window, cx| {
                    view.update(cx, |this, cx| {
                        let end = this.layout.group_tokens(group).len();
                        this.move_tab_to_group(source.0, group, end, window, cx);
                    });
                }
            })
            .on_drop::<PathBuf>({
                let view = view.clone();
                move |path, window, cx| {
                    view.update(cx, |this, cx| {
                        this.layout.focus(group);
                        this.open_document_new_tab(path.clone(), window, cx);
                    });
                }
            })
            .size_full()
            .min_w_0()
            .min_h_0()
            .when(!self.zen, |this| {
                this.child(self.render_group_tab_bar(group, cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(content)
                    .on_drop::<DraggedTab>({
                        let view = view.clone();
                        move |source, window, cx| {
                            view.update(cx, |this, cx| {
                                let end = this.layout.group_tokens(group).len();
                                this.move_tab_to_group(source.0, group, end, window, cx);
                            });
                        }
                    })
                    .relative()
                    .child(self.render_split_drop_target(group, Axis::Horizontal, true, cx))
                    .child(self.render_split_drop_target(group, Axis::Horizontal, false, cx))
                    .child(self.render_split_drop_target(group, Axis::Vertical, true, cx))
                    .child(self.render_split_drop_target(group, Axis::Vertical, false, cx)),
            )
            .into_any_element()
    }

    fn render_split_drop_target(
        &self,
        group: GroupId,
        axis: Axis,
        before: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity();
        let target = div()
            .absolute()
            .drag_over::<DraggedTab>(|style, _, _, cx| style.bg(cx.theme().muted.opacity(0.22)))
            .on_drop::<DraggedTab>(move |source, window, cx| {
                view.update(cx, |this, cx| {
                    this.split_tab_into_group(source.0, group, axis, before, window, cx);
                });
            });
        match (axis, before) {
            (Axis::Horizontal, true) => target.left_0().top_0().bottom_0().w(px(12.)),
            (Axis::Horizontal, false) => target.right_0().top_0().bottom_0().w(px(12.)),
            (Axis::Vertical, true) => target.top_0().left_0().right_0().h(px(12.)),
            (Axis::Vertical, false) => target.bottom_0().left_0().right_0().h(px(12.)),
        }
    }

    fn render_pane_view_mode_tabs(
        &self,
        group: GroupId,
        mode: PaneMode,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        TabBar::new(("pane-mode", group.value()))
            .segmented()
            .small()
            .flex_shrink_0()
            .selected_index(match mode {
                PaneMode::Source => 0,
                PaneMode::Split => 1,
                PaneMode::Preview => 2,
            })
            .children(
                [
                    (assets::IconName::SquarePen, self.tr("Source", "Kilde")),
                    (
                        assets::IconName::SquareSplitHorizontal,
                        self.tr("Split", "Delt"),
                    ),
                    (assets::IconName::Eye, self.tr("Preview", "Forhåndsvisning")),
                ]
                .map(|(icon, label)| {
                    if compact {
                        Tab::new()
                            .icon(icon)
                            .aria_label(label)
                            .tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
                    } else {
                        Tab::new().label(label)
                    }
                }),
            )
            .on_click(cx.listener(move |this, &index, window, cx| {
                let mode = match index {
                    0 => PaneMode::Source,
                    1 => PaneMode::Split,
                    _ => PaneMode::Preview,
                };
                this.set_pane_view_mode(group, mode, window, cx);
            }))
    }

    fn focus_pane(&mut self, group: GroupId, cx: &mut Context<Self>) {
        let next_active = self
            .layout
            .selected(group)
            .and_then(|token| self.doc_tokens.iter().position(|item| *item == token));
        let next_mode = self
            .layout
            .group_specs()
            .into_iter()
            .find(|(id, _, _, _)| *id == group)
            .map(|(_, _, _, mode)| match mode {
                PaneMode::Source => ViewMode::Source,
                PaneMode::Split => ViewMode::Split,
                PaneMode::Preview => ViewMode::Preview,
            });
        if self.layout.focused() == group
            && self.active == next_active
            && next_mode.is_none_or(|mode| self.settings.view_mode == mode)
        {
            return;
        }
        self.layout.focus(group);
        self.active = next_active;
        if let Some(mode) = next_mode {
            self.settings.view_mode = mode;
        }
        self.persist_tabs(cx);
        cx.notify();
    }

    fn split_tab_into_group(
        &mut self,
        token: TabToken,
        group: GroupId,
        axis: Axis,
        before: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout.group_for(token) != Some(group) {
            let index = self.layout.group_tokens(group).len();
            if !self.layout.move_tab(token, group, index) {
                return;
            }
        }
        if self.layout.split_at(group, token, axis, before).is_some() {
            self.active = self.doc_index_for_token(token);
            self.folder = None;
            self.graph = None;
            self.persist_tabs(cx);
            self.focus_active_pane_after_mount(window, cx);
        }
    }

    fn activate_pane(
        &mut self,
        group: GroupId,
        token: TabToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.layout.select(group, token) {
            return;
        }
        self.active = self.doc_tokens.iter().position(|item| *item == token);
        self.folder = None;
        self.graph = None;
        self.persist_tabs(cx);
        self.refocus(window, cx);
        cx.notify();
    }

    fn render_document_pane(
        &self,
        doc: &Entity<Document>,
        mode: ViewMode,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if doc.read(cx).is_canvas {
            let content = doc
                .read(cx)
                .canvas_view
                .clone()
                .map(|view| view.into_any_element())
                .unwrap_or_else(|| {
                    div()
                        .size_full()
                        .child(self.tr(
                            "Canvas view is unavailable.",
                            "Canvas-visningen er utilgjengelig.",
                        ))
                        .into_any_element()
                });
            if !doc.read(cx).conflict {
                return content;
            }
            let reload_doc = doc.clone();
            return v_flex()
                .size_full()
                .child(
                    h_flex()
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
                                .child(self.tr(
                                    "Changed on disk — saving paused",
                                    "Filen er endret på disk — lagring er satt på pause",
                                )),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    Button::new("canvas-conflict-save-copy")
                                        .ghost()
                                        .xsmall()
                                        .label(self.tr("Save copy…", "Lagre kopi…"))
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.on_save_as(&SaveFileAs, window, cx);
                                        })),
                                )
                                .child(
                                    Button::new("canvas-conflict-reload")
                                        .ghost()
                                        .xsmall()
                                        .label(self.tr("Reload from disk", "Last inn fra disk"))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.confirm_reload_from_disk(
                                                reload_doc.clone(),
                                                window,
                                                cx,
                                            );
                                        })),
                                ),
                        ),
                )
                .child(div().flex_1().min_h_0().child(content))
                .into_any_element();
        }
        if let Some(pdf) = doc.read(cx).pdf.clone() {
            return div().size_full().child(pdf).into_any_element();
        }
        // Image files render the picture itself, not a
        // text editor over binary bytes.
        if doc.read(cx).is_image {
            return self.render_image_view(doc, cx).into_any_element();
        }
        if doc.read(cx).file_preview.is_some() {
            return self.render_file_preview(doc, cx);
        }

        // `.base` files render their live view in Preview/Split; Source
        // stays the raw YAML so the spec stays editable.
        let base = self
            .docs
            .iter()
            .find(|open| open.entity == *doc)
            .and_then(|d| d.base.clone());
        let content = match mode {
            ViewMode::Source => self.editor_container(doc, cx).into_any_element(),
            ViewMode::Preview if base.is_some() => {
                div().size_full().child(base.unwrap()).into_any_element()
            }
            ViewMode::Preview => div()
                .size_full()
                .child(self.render_preview(doc, cx))
                .into_any_element(),
            ViewMode::Split => div()
                .size_full()
                .child(
                    canvas_panels(
                        ("document-split", doc.entity_id().as_non_zero_u64().get()),
                        Axis::Horizontal,
                    )
                    .child(
                        resizable_panel().size_range(px(80.)..Pixels::MAX).child(
                            div()
                                .size_full()
                                .min_w_0()
                                .overflow_hidden()
                                .child(self.editor_container(doc, cx)),
                        ),
                    )
                    .child(
                        resizable_panel().size_range(px(80.)..Pixels::MAX).child(
                            div()
                                .size_full()
                                .min_w_0()
                                .overflow_hidden()
                                .child(match base {
                                    Some(base) => base.into_any_element(),
                                    None => self.render_preview(doc, cx).into_any_element(),
                                }),
                        ),
                    ),
                )
                .into_any_element(),
        };
        let content = match self.render_breadcrumb(doc, cx) {
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
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .px_3()
            .py_1()
            .bg(cx.theme().warning.opacity(0.12))
            .child(
                div()
                    .max_w_full()
                    .text_sm()
                    .font_semibold()
                    .text_color(cx.theme().warning)
                    .child(self.tr(
                        "Changed on disk — saving paused",
                        "Endret på disken — lagring satt på pause",
                    )),
            )
            .child(
                h_flex()
                    .max_w_full()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("conflict-save-copy")
                            .ghost()
                            .xsmall()
                            .label(self.tr("Save copy…", "Lagre kopi…"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_save_as(&SaveFileAs, window, cx);
                            })),
                    )
                    .child(
                        Button::new("conflict-reload")
                            .ghost()
                            .xsmall()
                            .label(self.tr("Reload from disk", "Last inn fra disk på nytt"))
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
        let context_doc = doc.clone();
        let note_style = doc.read(cx).resolved_style.clone();
        let undo_doc = doc.clone();
        let redo_doc = doc.clone();
        div()
            .size_full()
            .px_3()
            .py_4()
            .when_some(
                note_style.colors.get("background").copied(),
                |this, color| this.bg(color),
            )
            // Scoped key context — the auto-pair bindings only fire when the
            // document editor is focused, so dialog/search inputs keep the
            // plain characters.
            .key_context("RistaEditor")
            .capture_any_mouse_down(move |event, _window, cx| {
                if event.button == MouseButton::Right {
                    context_doc.update(cx, |doc, cx| doc.prepare_spell_menu(event.position, cx));
                }
            })
            .on_action(cx.listener(Self::on_pair_insert))
            .on_action(cx.listener(Self::on_set_highlight_color))
            .on_action(cx.listener(Self::on_pair_close))
            .capture_action::<input::Paste>(cx.listener(|this, _paste, window, cx| {
                this.on_editor_paste_action(window, cx);
            }))
            .capture_action::<input::Undo>(cx.listener(move |this, _, window, cx| {
                if undo_doc.read(cx).composer_rollback_action(
                    crate::document::ComposerHistoryDirection::Undo,
                ) {
                    return;
                }
                let changes = undo_doc.read(cx).composer_history_changes_for_action(
                    crate::document::ComposerHistoryDirection::Undo,
                    cx,
                );
                if this.composer_history_buffer_conflict(
                    Some(&undo_doc),
                    &changes,
                    crate::document::ComposerHistoryDirection::Undo,
                    cx,
                ) {
                    this.note_status(
                        this.tr(
                            "Composer undo stopped because a related note has unsaved changes.",
                            "Angring av sammensetningen ble stoppet fordi et tilknyttet notat har ulagrede endringer.",
                        ),
                        cx,
                    );
                    cx.stop_propagation();
                    return;
                }
                let prepared = undo_doc.update(cx, |doc, cx| {
                    doc.prepare_composer_history_action(
                        crate::document::ComposerHistoryDirection::Undo,
                        window,
                        cx,
                    )
                });
                if !prepared {
                    this.note_status(
                        this.tr(
                            "Composer undo stopped because a related file changed.",
                            "Angring av sammensetningen ble stoppet fordi en tilknyttet fil ble endret.",
                        ),
                        cx,
                    );
                    cx.stop_propagation();
                }
            }))
            .capture_action::<input::Redo>(cx.listener(move |this, _, window, cx| {
                if redo_doc.read(cx).composer_rollback_action(
                    crate::document::ComposerHistoryDirection::Redo,
                ) {
                    return;
                }
                let changes = redo_doc.read(cx).composer_history_changes_for_action(
                    crate::document::ComposerHistoryDirection::Redo,
                    cx,
                );
                if this.composer_history_buffer_conflict(
                    Some(&redo_doc),
                    &changes,
                    crate::document::ComposerHistoryDirection::Redo,
                    cx,
                ) {
                    this.note_status(
                        this.tr(
                            "Composer redo stopped because a related note has unsaved changes.",
                            "Gjentaking av sammensetningen ble stoppet fordi et tilknyttet notat har ulagrede endringer.",
                        ),
                        cx,
                    );
                    cx.stop_propagation();
                    return;
                }
                let prepared = redo_doc.update(cx, |doc, cx| {
                    doc.prepare_composer_history_action(
                        crate::document::ComposerHistoryDirection::Redo,
                        window,
                        cx,
                    )
                });
                if !prepared {
                    this.note_status(
                        this.tr(
                            "Composer redo stopped because a related file changed.",
                            "Gjentaking av sammensetningen ble stoppet fordi en tilknyttet fil ble endret.",
                        ),
                        cx,
                    );
                    cx.stop_propagation();
                }
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
                let language = self.settings.language;
                let spell_doc = doc.clone();
                let editor = Editor::new(&doc.read(cx).editor)
                    .h_full()
                    .appearance(false)
                    .when_some(
                        note_style.colors.get("foreground").copied(),
                        |this, color| this.text_color(color),
                    )
                    .when_some(note_style.typography.editor_font.clone(), |this, font| {
                        this.font_family(font)
                    })
                    .when_some(note_style.typography.body_size, |this, size| {
                        this.text_size(px(size))
                    })
                    .when_some(note_style.typography.line_height, |this, height| {
                        this.line_height(relative(height))
                    })
                    .context_menu(move |menu, _window, cx| {
                        let spell_context = spell_doc.read(cx).spell_menu_context();
                        let spell_id = spell_doc.entity_id().as_non_zero_u64().get();
                        let mut menu = menu;
                        if let Some(crate::document::SpellMenuContext {
                            revision: spell_revision,
                            range,
                            word,
                            suggestions,
                        }) = spell_context
                        {
                            let mut spelling = gpui_kit::component::native_menu::NativeMenu::new();
                            let no_suggestions = suggestions.is_empty();
                            for suggestion in suggestions {
                                spelling = spelling.menu(
                                    suggestion.clone(),
                                    Box::new(SpellCorrection {
                                        document_id: spell_id,
                                        revision: spell_revision,
                                        start: range.start,
                                        end: range.end,
                                        original: word.clone(),
                                        replacement: suggestion,
                                    }),
                                );
                            }
                            if no_suggestions {
                                spelling = spelling.menu(
                                    language.text("No suggestions", "Ingen forslag"),
                                    Box::new(SpellNoSuggestions),
                                );
                            }
                            spelling = spelling
                                .separator()
                                .menu(
                                    language.text("Add to dictionary", "Legg til i ordliste"),
                                    Box::new(SpellLearn {
                                        document_id: spell_id,
                                        revision: spell_revision,
                                        start: range.start,
                                        end: range.end,
                                        original: word.clone(),
                                    }),
                                )
                                .menu(
                                    language.text("Ignore", "Ignorer"),
                                    Box::new(SpellIgnore {
                                        document_id: spell_id,
                                        revision: spell_revision,
                                        start: range.start,
                                        end: range.end,
                                        original: word,
                                    }),
                                );
                            menu =
                                menu.submenu(language.text("Spelling", "Stavekontroll"), spelling);
                        }
                        menu.menu("Cut", Box::new(input::Cut))
                            .menu("Copy", Box::new(input::Copy))
                            .menu("Paste", Box::new(input::Paste))
                            .menu("Select All", Box::new(input::SelectAll))
                            .separator()
                            .menu("Italic", Box::new(ToggleItalic))
                            .submenu(
                                "Highlight color",
                                preview::HIGHLIGHT_COLORS
                                    .iter()
                                    .fold(
                                        gpui_kit::component::native_menu::NativeMenu::new(),
                                        |menu, (emoji, name)| {
                                            menu.menu(
                                                format!("{emoji} {name}"),
                                                Box::new(SetHighlightColor { emoji }),
                                            )
                                        },
                                    )
                                    .separator()
                                    .menu("No color", Box::new(SetHighlightColor { emoji: "" })),
                            )
                            .menu("Task checkbox", Box::new(ToggleCheckbox))
                            .menu("Toggle comment", Box::new(ToggleComment))
                            .separator()
                            .menu("Move line up", Box::new(MoveLineUp))
                            .menu("Move line down", Box::new(MoveLineDown))
                            .menu("Duplicate line", Box::new(DuplicateBlock))
                            .menu("Delete line", Box::new(DeleteLine))
                            .separator()
                            .menu("Open link under cursor", Box::new(FollowLink))
                            .menu(
                                language.text("Copy link to block", "Kopier lenke til blokk"),
                                Box::new(CopyBlockLink),
                            )
                            .menu(
                                language.text("Copy embed to block", "Kopier innbygging av blokk"),
                                Box::new(CopyBlockEmbed),
                            )
                    });
                // `cssclasses:` per-note override — `wide` lifts the
                // readable-width cap for this note, `narrow`/`readable`
                // forces it on (the per-note styling hook).
                let classes = &doc.read(cx).css_classes;
                let readable = if classes.iter().any(|c| c == "wide") {
                    false
                } else if classes.iter().any(|c| c == "narrow" || c == "readable")
                    || note_style.readable_width.is_some()
                {
                    true
                } else {
                    self.settings.readable_width
                };
                let readable_width = note_style.readable_width.unwrap_or(760.);
                if readable {
                    // Readable line length: cap the text column and
                    // center it (the editor setting).
                    div()
                        .size_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .h_full()
                                .w(px(readable_width))
                                .max_w_full()
                                .child(editor),
                        )
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
        if doc.read(cx).is_canvas {
            return;
        }
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
        let active_path = self.active_doc().map(|doc| doc.read(cx).path.clone());
        let Some(dir) = self.attachments_dir(active_path.as_deref(), cx) else {
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

    /// Copy dropped/copied files in and link them at the caret: images
    /// go to the attachments dir as `![[name]]`, notes next to the
    /// active note as `[[stem]]`, other files to the attachments dir as
    /// `[[name.ext]]`. Files already inside the vault just get linked.
    fn import_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.active_doc().is_some_and(|doc| doc.read(cx).is_canvas) {
            return false;
        }
        let active_path = self.active_doc().map(|doc| doc.read(cx).path.clone());
        let (Some(root), Some(dir)) = (
            self.vault.read(cx).root.clone(),
            self.attachments_dir(active_path.as_deref(), cx),
        ) else {
            return false;
        };
        let note_dir = active_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let mut links = Vec::new();
        for src in paths {
            if !src.is_file() {
                continue;
            }
            let image = crate::vault::is_image_file(src);
            let note = src.extension().is_some_and(|e| e == "md");
            let target_dir = match (note, note_dir.as_ref()) {
                (true, Some(note_dir)) => note_dir,
                _ => &dir,
            };
            let dest = if src.starts_with(&root) {
                src.clone()
            } else {
                match copy_into(target_dir, src) {
                    Some(dest) => dest,
                    None => continue,
                }
            };
            let link = match (image, note) {
                (true, _) => dest
                    .file_name()
                    .map(|n| format!("![[{}]]", n.to_string_lossy())),
                (_, true) => dest
                    .file_stem()
                    .map(|n| format!("[[{}]]", n.to_string_lossy())),
                _ => dest
                    .file_name()
                    .map(|n| format!("[[{}]]", n.to_string_lossy())),
            };
            links.extend(link);
        }
        if links.is_empty() {
            return false;
        }
        self.refresh_image_index(cx);
        if let Some(doc) = self.active_doc().cloned() {
            doc.update(cx, |doc, cx| {
                doc.editor
                    .update(cx, |editor, cx| editor.insert(links.join("\n"), window, cx));
            });
        }
        true
    }

    /// Finder drop onto a sidebar folder (or the header, for the vault
    /// root): copy the files in under free names. A single dropped note
    /// opens.
    fn copy_external_into(
        &mut self,
        paths: &[PathBuf],
        dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let copied: Vec<PathBuf> = paths
            .iter()
            .filter(|p| p.is_file())
            .filter_map(|src| copy_into(&dir, src))
            .collect();
        if copied.is_empty() {
            self.note_status(
                if paths.iter().any(|p| p.is_dir()) {
                    "Folders can't be dropped in yet — drop the files inside"
                } else {
                    "Could not copy the dropped files"
                },
                cx,
            );
            return;
        }
        self.vault.update(cx, |vault, cx| vault.refresh(cx));
        match copied.as_slice() {
            [one] if one.extension().is_some_and(|e| e == "md") => {
                self.open_document(one.clone(), window, cx);
            }
            _ => {
                let added = match copied.as_slice() {
                    [one] => one
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    _ => format!("{} files", copied.len()),
                };
                let hidden = copied
                    .iter()
                    .any(|p| !crate::file_preview::visible(p, self.settings.show_other_files));
                self.note_status(
                    if hidden {
                        format!("Added {added} · turn on “Show non-Markdown files” to see it in the sidebar")
                    } else {
                        format!("Added {added}")
                    },
                    cx,
                );
            }
        }
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
        if doc.read(cx).is_canvas {
            return;
        }
        let text = format!("![[{name}]]");
        doc.update(cx, |doc, cx| {
            doc.editor.update(cx, |editor, cx| {
                editor.insert(text, window, cx);
            });
        });
    }

    fn attachments_dir(&self, note_path: Option<&Path>, cx: &App) -> Option<PathBuf> {
        let root = self.vault.read(cx).root.as_ref()?.canonicalize().ok()?;
        let configured = self.settings.attachments_dir.as_str();
        if configured == "." || configured.is_empty() {
            return Some(root);
        }
        let relative = if configured.starts_with("./") {
            let note_dir = note_path?.parent()?;
            let note_dir = note_dir.canonicalize().ok()?;
            let note_relative = note_dir.strip_prefix(&root).ok()?;
            note_relative.join(configured.trim_start_matches("./"))
        } else {
            PathBuf::from(configured)
        };
        crate::vault_settings_import::checked_vault_path(
            &root,
            &relative.to_string_lossy().replace('\\', "/"),
        )
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

    fn content_tab(label: String, icon: assets::IconName) -> Tab {
        Tab::new().aria_label(label.clone()).child(
            h_flex()
                .gap_1p5()
                .items_center()
                .min_w_0()
                .child(Icon::new(icon).size_3p5().flex_shrink_0())
                .child(
                    div()
                        .min_w_0()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(label),
                ),
        )
    }

    fn render_graph_dock_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .rounded(cx.theme().radius_lg)
            .overflow_hidden()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    // Cached: workspace redraws (agent streaming, typing)
                    // reuse the graph's last frame unless it notifies.
                    .when_some(self.graph_dock.clone(), |this, graph| {
                        this.child(graph.cached(StyleRefinement::default().size_full()))
                    }),
            )
    }

    fn render_status_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let compact =
            window.viewport_size().width < px(960.) && (self.status_note.is_some() || conflict);

        let mode_label = match self.settings.view_mode {
            ViewMode::Source => "Source",
            ViewMode::Split => "Split",
            ViewMode::Preview => "Preview",
        };

        StatusBar::new()
            .px_4()
            .py_2()
            .flex_shrink_0()
            .left(
                h_flex()
                    .gap_3()
                    .items_center()
                    .children(
                        doc.and_then(|doc| doc.read(cx).vim_status(self.settings.language))
                            .map(|status| {
                                div()
                                    .text_xs()
                                    .flex_shrink_0()
                                    .max_w(px(220.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(status)
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(if conflict {
                                cx.theme().danger
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(if conflict {
                                format!(
                                    "{} — {}",
                                    rel_path,
                                    self.tr("changed on disk", "endret på disken")
                                )
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
                                        this.persist_settings(cx);
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
                                        this.persist_settings(cx);
                                        cx.notify();
                                    })),
                            )
                        },
                    )
                    .child(
                        Button::new("workspace-local-graph")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Waypoints)
                            .when(!compact, |button| {
                                button.label(self.tr("Local graph", "Lokal graf"))
                            })
                            .accessibility_label(self.tr("Local graph", "Lokal graf"))
                            .tooltip(self.tr("Local graph", "Lokal graf"))
                            .selected(self.settings.graph_dock_open)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_local_graph_panel(&ToggleLocalGraphPanel, window, cx)
                            })),
                    )
                    .child(
                        Button::new("workspace-agent")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::MessageSquareQuote)
                            .when(!compact, |button| button.label(self.tr("Agent", "Agent")))
                            .accessibility_label(self.tr("Agent", "Agent"))
                            .tooltip(self.tr("Agent", "Agent"))
                            .selected(self.settings.agent_open)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_agent_panel(&ToggleAgentPanel, window, cx)
                            })),
                    )
                    .child(
                        Button::new("workspace-tools")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Blocks)
                            .when(!compact, |button| button.label(self.tr("Tools", "Verktøy")))
                            .accessibility_label(self.tr("Tools", "Verktøy"))
                            .tooltip(self.tr("Tools", "Verktøy"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_open_tools(&OpenTools, window, cx)
                            })),
                    )
                    .child(
                        Button::new("workspace-terminal")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Terminal)
                            .when(!compact, |button| {
                                button.label(self.tr("Terminal", "Terminal"))
                            })
                            .accessibility_label(self.tr("Terminal", "Terminal"))
                            .tooltip(self.tr("Terminal", "Terminal"))
                            .selected(self.terminal_visible)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_toggle_terminal(&ToggleTerminal, window, cx)
                            })),
                    ),
            )
    }
    pub(crate) fn sync_hotkey_warning(&mut self, cx: &mut Context<Self>) {
        self.hotkey_warning = crate::hotkeys::warning(cx);
        cx.notify();
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.settings.graph_dock_open && self.graph_dock.is_none() {
            self.ensure_graph_dock(window, cx);
        }
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

        // Key events can draw before on_next_frame runs. Mount with focus and
        // the destination selection already set so the first input is not lost.
        if let Some((document, line)) = self.pending_editor_focus.take() {
            if !window.has_active_dialog(cx)
                && self.graph.is_none()
                && self.folder.is_none()
                && self.settings.view_mode != ViewMode::Preview
                && self.active_doc() == Some(&document)
                && !document.read(cx).is_read_only()
            {
                document.update(cx, |document, cx| {
                    if let Some(line) = line {
                        document.jump_to_line(line, window, cx);
                    } else {
                        document
                            .editor
                            .update(cx, |editor, cx| editor.focus(window, cx));
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
        let preview_history_host = (self.settings.view_mode == ViewMode::Preview)
            .then(|| {
                self.pending_preview_history
                    .first()
                    .map(|(doc, _)| doc.clone())
            })
            .flatten()
            .filter(|doc| self.active_doc() == Some(doc));
        if !self.pending_preview_history.is_empty() && !self.preview_history_scheduled {
            self.preview_history_scheduled = true;
            let workspace = cx.weak_entity();
            window.on_next_frame(move |window, cx| {
                let pending = workspace
                    .update(cx, |this, cx| {
                        this.preview_history_scheduled = false;
                        cx.notify();
                        std::mem::take(&mut this.pending_preview_history)
                    })
                    .unwrap_or_default();
                for (document, action) in pending {
                    let valid = workspace
                        .read_with(cx, |workspace, _| {
                            workspace.active_doc() == Some(&document)
                                && workspace.folder.is_none()
                                && workspace.graph.is_none()
                        })
                        .unwrap_or(false);
                    if valid && !window.has_active_dialog(cx) {
                        let focus = document.read(cx).editor.read(cx).focus_handle(cx);
                        focus.dispatch_action(action.as_ref(), window, cx);
                    }
                }
            });
        }
        let has_workspace = vault_open || !self.docs.is_empty();
        let sidebar_visible = vault_open && !self.settings.sidebar_collapsed && !self.zen;
        let dock_visible = !self.zen
            && (self.settings.graph_dock_open
                || self.settings.agent_open
                || self.settings.inspector_open);
        let background = cx.theme().background;

        let root = v_flex()
            .size_full()
            .relative()
            // Editor history is action-only. Mount its normal handlers for one
            // frame in Preview, without moving focus or showing a source pane.
            .when_some(preview_history_host, |root, document| {
                root.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h_0()
                        .overflow_hidden()
                        .child(self.editor_container(&document, cx)),
                )
            })
            .bg(background)
            .key_context("Rista")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_new_file))
            .on_action(cx.listener(Self::on_new_canvas))
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
            .on_action(cx.listener(Self::on_split_pane_right))
            .on_action(cx.listener(Self::on_split_pane_down))
            .on_action(cx.listener(Self::on_next_pane))
            .on_action(cx.listener(Self::on_prev_pane))
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
            .on_action(cx.listener(Self::on_toggle_local_graph_panel))
            .on_action(cx.listener(Self::on_toggle_agent_panel))
            .on_action(cx.listener(Self::on_duplicate_block))
            .on_action(cx.listener(Self::on_delete_line))
            .on_action(cx.listener(Self::on_toggle_comment))
            .on_action(cx.listener(Self::on_copy_block_link))
            .on_action(cx.listener(Self::on_copy_block_embed))
            .on_action(cx.listener(Self::on_zoom_in))
            .on_action(cx.listener(Self::on_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::on_open_palette))
            .on_action(cx.listener(Self::on_run_palette_command))
            .on_action(cx.listener(Self::on_spell_correction))
            .on_action(cx.listener(Self::on_spell_ignore))
            .on_action(cx.listener(Self::on_spell_learn))
            .on_action(cx.listener(Self::on_spell_no_suggestions))
            .on_action(cx.listener(Self::on_quick_open))
            .on_action(cx.listener(Self::on_find))
            .on_action(cx.listener(Self::on_open_project_search))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_toggle_terminal))
            .on_action(cx.listener(Self::on_open_tools))
            .on_action(cx.listener(Self::on_toggle_theme));
        #[cfg(target_os = "macos")]
        let root = root.on_action(cx.listener(Self::on_install_cli));
        #[cfg(not(target_os = "macos"))]
        let root = root.on_action(cx.listener(Self::on_about));
        root
            .on_action(cx.listener(Self::on_check_for_updates))
            .when(!self.zen || self.hotkey_warning.is_some(), |this| {
                this.child(self.render_title_bar(window, cx))
            })
            .when_some(self.hotkey_warning.clone(), |this, notice| {
                this.child(
                    div()
                        .flex_none()
                        .w_full()
                        .px_3()
                        .py_1()
                        .bg(cx.theme().warning.opacity(0.12))
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child(notice.startup_message(self.settings.language)),
                )
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
                        let workspace_view = cx.entity();
                        let dock_view = workspace_view.clone();
                        let epoch = self.layout_epoch;
                        let dock_names = [
                            ("graph", self.settings.graph_dock_open),
                            ("agent", self.settings.agent_open),
                            ("inspector", self.settings.inspector_open),
                        ].into_iter().filter_map(|(name, visible)| visible.then_some(name)).collect::<Vec<_>>();
                        let panel_group_id = SharedString::from(format!(
                            "workspace-{sidebar_visible}-{dock_visible}-{epoch}"
                        ));
                        canvas_panels(panel_group_id, Axis::Horizontal)
                            .when(sidebar_visible, |this| {
                                this.child(
                                    resizable_panel()
                                        .flex_none()
                                        .size(px(
                                            self.settings
                                                .sidebar_width
                                                .filter(|width| {
                                                    crate::settings::valid_sidebar_width(*width)
                                                })
                                                .unwrap_or(260.),
                                        ))
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
                                                        .child(div().flex_1().min_h_0().child({
                                                            if let Some(graph) = self.graph.clone()
                                                            {
                                                                let graph = graph
                                                                    .cached(
                                                                        StyleRefinement::default()
                                                                            .size_full(),
                                                                    )
                                                                    .into_any_element()
                                                                ;
                                                                if self.zen {
                                                                    graph
                                                                } else {
                                                                    v_flex()
                                                                        .size_full()
                                                                        .child(self.render_tab_bar(cx))
                                                                        .child(
                                                                            div()
                                                                                .flex_1()
                                                                                .min_h_0()
                                                                                .child(graph),
                                                                        )
                                                                        .into_any_element()
                                                                }
                                                            } else {
                                                                let content = self.render_editor_panes(cx);
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
                            .when(
                                dock_visible,
                                |columns| {
                                columns.child(
                                    resizable_panel()
                                        .flex_none()
                                        .size(px(self.settings.dock_width.filter(|width| crate::settings::valid_dock_width(*width)).unwrap_or(310.)))
                                        .size_range(px(220.)..px(520.))
                                        .child(
                                            div().size_full().child(
                                                canvas_panels(SharedString::from(format!("right-dock-{}", dock_names.join("-"))), Axis::Vertical)
                                                    .when(self.settings.graph_dock_open, |panes| {
                                                        panes.child(
                                                            resizable_panel()
                                                                .when(self.settings.agent_open || self.settings.inspector_open, |panel| panel.size(px(220.)))
                                                                .when_some(self.saved_dock_height("graph"), |panel, height| panel.size(height))
                                                                .size_range(px(150.)..Pixels::MAX)
                                                                .child(
                                                                    div()
                                                                        .size_full()
                                                                        .p_1()
                                                                        .child(self.render_graph_dock_panel(cx)),
                                                                ),
                                                        )
                                                    })
                                                    .when(self.settings.agent_open, |panes| {
                                                        panes.child(
                                                            resizable_panel()
                                                                .when(self.settings.inspector_open, |panel| panel.size(px(300.)))
                                                                .when_some(self.saved_dock_height("agent"), |panel, height| panel.size(height))
                                                                .size_range(px(220.)..Pixels::MAX)
                                                                .child(
                                                                    div()
                                                                        .size_full()
                                                                        .p_1()
                                                                        .child(
                                                                            div()
                                                                                .size_full()
                                                                                .bg(cx.theme().sidebar)
                                                                                .rounded(cx.theme().radius_lg)
                                                                                .overflow_hidden()
                                                                                // Cached so streamed agent output
                                                                                // redraws only this panel.
                                                                                .child(
                                                                                    self.agent_panel
                                                                                        .clone()
                                                                                        .cached(StyleRefinement::default().size_full()),
                                                                                ),
                                                                        ),
                                                                ),
                                                        )
                                                    })
                                                    .when(self.settings.inspector_open, |panes| {
                                                        panes.child(
                                                            resizable_panel()
                                                                .when_some(self.saved_dock_height("inspector"), |panel, height| panel.size(height))
                                                                .size_range(px(170.)..Pixels::MAX)
                                                                .child(
                                                                    div()
                                                                        .size_full()
                                                                        .p_1()
                                                                        .child(
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
                                                    .on_resize(move |state, _, cx| {
                                                        let heights = state.read(cx).sizes().iter().copied().map(f32::from).collect::<Vec<_>>();
                                                        dock_view.update(cx, |this, cx| {
                                                            if this.layout_epoch != epoch || this.restoring_session { return; }
                                                            let mut changed = false;
                                                            for (name, height) in dock_names.iter().zip(heights) {
                                                                if height.is_finite() && height > 0. && this.settings.dock_heights.get(*name) != Some(&height) {
                                                                    this.settings.dock_heights.insert((*name).into(), height);
                                                                    changed = true;
                                                                }
                                                            }
                                                            if changed { this.persist_settings(cx); }
                                                        });
                                                    }),
                                            ),
                                        ),
                                )
                            },
                            )
                            .on_resize(move |state, _, cx| {
                                let dock_width = state.read(cx).sizes().last().copied().map(f32::from);
                                let width = state
                                    .read(cx)
                                    .sizes()
                                    .first()
                                    .copied()
                                    .map(f32::from);
                                if let Some(width) = width {
                                    let manager = cx.global::<crate::windows::WindowManager>();
                                    if manager.restoring || manager.quitting {
                                        return;
                                    }
                                    workspace_view.update(cx, |workspace, cx| {
                                        if workspace.layout_epoch != epoch || workspace.restoring_session { return; }
                                        let dock_visible = !workspace.zen && (workspace.settings.graph_dock_open || workspace.settings.agent_open || workspace.settings.inspector_open);
                                        if let Some(width) = dock_width.filter(|width| dock_visible && crate::settings::valid_dock_width(*width)) {
                                            if workspace.settings.dock_width != Some(width) {
                                                workspace.settings.dock_width = Some(width);
                                                workspace.persist_settings(cx);
                                            }
                                        }
                                        let visible = workspace.vault.read(cx).is_open()
                                            && !workspace.settings.sidebar_collapsed
                                            && !workspace.zen;
                                        if visible
                                            && crate::settings::valid_sidebar_width(width)
                                            && workspace.settings.sidebar_width != Some(width)
                                        {
                                            workspace.settings.sidebar_width = Some(width);
                                            workspace.persist_settings(cx);
                                        }
                                    });
                                }
                            })
                            .into_any_element()
                    }),
            )
            .when(!self.zen && has_workspace, |this| {
                this.child(self.render_status_bar(window, cx))
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
pub(crate) fn open_in_default_app(path: &std::path::Path) {
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

/// `dir/name`, or `dir/<stem> N.<ext>` for the first free `N` —
/// imports never overwrite.
fn free_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

/// `path` relative to the vault root with `/` separators (`""` for the root).
fn vault_rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// The vault file named `name` (`report.pdf`, or a vault-relative path):
/// the exact path first, then the first basename match, skipping
/// hidden folders like `.rista`.
fn find_vault_file(root: &Path, name: &str) -> Option<PathBuf> {
    let exact = root.join(name);
    if exact.is_file() {
        return Some(exact);
    }
    let base = Path::new(name)
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if entry.file_name().to_string_lossy().to_lowercase() == base {
                return Some(path);
            }
        }
    }
    None
}

fn find_vault_markdown(root: &Path, name: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let name_path = Path::new(name);
    let target = match name_path.extension() {
        Some(extension)
            if !extension
                .to_str()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md")) =>
        {
            return None;
        }
        Some(_) => name.to_string(),
        None => format!("{name}.md"),
    };
    let exact = crate::vault_settings_import::checked_vault_path(&root, &target)?;
    match exact.canonicalize() {
        Ok(path) => {
            return (path.starts_with(&root)
                && path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md")))
            .then_some(path);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return None,
    }
    if target.contains('/') {
        return None;
    }
    let basename = Path::new(&target)
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    let mut stack = vec![root.clone()];
    let mut found = None;
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file()
                && entry.file_name().to_string_lossy().to_lowercase() == basename
            {
                let canonical = path.canonicalize().ok()?;
                if canonical.starts_with(&root) && found.replace(canonical).is_some() {
                    return None;
                }
            }
        }
    }
    found
}

/// Copy `src` into `dir` under a free name and return where it went.
/// A same-named file there with identical bytes is reused instead, so
/// pasting the same image twice links one file.
fn copy_into(dir: &Path, src: &Path) -> Option<PathBuf> {
    let same = dir.join(src.file_name()?);
    if same == src {
        return Some(same);
    }
    let identical = |a: &Path, b: &Path| {
        let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else {
            return false;
        };
        ma.is_file()
            && ma.len() == mb.len()
            && matches!((std::fs::read(a), std::fs::read(b)), (Ok(x), Ok(y)) if x == y)
    };
    if identical(&same, src) {
        return Some(same);
    }
    let dest = free_path(dir, &src.file_name()?.to_string_lossy());
    std::fs::create_dir_all(dir).ok()?;
    std::fs::copy(src, &dest).ok()?;
    Some(dest)
}

/// `<templates_dir>/**/*.md` under the vault root, sorted for a stable dialog list.
pub(crate) fn template_files(root: &std::path::Path, templates_dir: &str) -> Vec<PathBuf> {
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
struct DraggedTab(TabToken);

/// Drag payload for reordering nested bookmarks.
#[derive(Clone)]
struct DraggedBookmark(u64);

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

#[cfg(test)]
mod composer_path_reference_tests {
    use super::apply_composer_path_list;
    use crate::document::{ComposerHistoryDirection, ComposerPathValueChange};

    #[test]
    fn note_composer_path_history_only_changes_the_captured_unchanged_slot() {
        let change = ComposerPathValueChange {
            index: 0,
            before: "source.md".to_string(),
            after: "target.md".to_string(),
        };
        let mut paths = vec!["target.md".to_string(), "target.md".to_string()];

        apply_composer_path_list(
            &mut paths,
            std::slice::from_ref(&change),
            ComposerHistoryDirection::Undo,
        );
        assert_eq!(
            paths,
            vec!["source.md".to_string(), "target.md".to_string()]
        );

        apply_composer_path_list(
            &mut paths,
            std::slice::from_ref(&change),
            ComposerHistoryDirection::Redo,
        );
        assert_eq!(
            paths,
            vec!["target.md".to_string(), "target.md".to_string()]
        );

        paths[0] = "later.md".to_string();
        apply_composer_path_list(&mut paths, &[change], ComposerHistoryDirection::Undo);
        assert_eq!(paths, vec!["later.md".to_string(), "target.md".to_string()]);
    }
}

#[cfg(test)]
mod import_tests {
    use super::{copy_into, find_vault_file, find_vault_markdown, free_path};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rista-import-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let dir = std::env::temp_dir().join(format!(
                "rista-import-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return dir,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create test directory: {error}"),
            }
        }
    }

    #[test]
    fn free_path_adds_a_number_instead_of_overwriting() {
        let dir = temp_dir("free");
        assert_eq!(free_path(&dir, "a.png"), dir.join("a.png"));
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        std::fs::write(dir.join("a 1.png"), b"x").unwrap();
        assert_eq!(free_path(&dir, "a.png"), dir.join("a 2.png"));
        std::fs::write(dir.join("README"), b"x").unwrap();
        assert_eq!(free_path(&dir, "README"), dir.join("README 1"));
    }

    #[test]
    fn copy_into_reuses_identical_files_and_renames_different_ones() {
        let src_dir = temp_dir("copy-src");
        let dest = temp_dir("copy-dest");
        let src = src_dir.join("pic.png");
        std::fs::write(&src, b"one").unwrap();

        assert_eq!(copy_into(&dest, &src), Some(dest.join("pic.png")));
        assert_eq!(copy_into(&dest, &src), Some(dest.join("pic.png")));

        std::fs::write(&src, b"two").unwrap();
        assert_eq!(copy_into(&dest, &src), Some(dest.join("pic 1.png")));
        assert_eq!(std::fs::read(dest.join("pic.png")).unwrap(), b"one");
        assert_eq!(std::fs::read(dest.join("pic 1.png")).unwrap(), b"two");
    }

    #[test]
    fn find_vault_file_matches_paths_then_basenames_outside_hidden_dirs() {
        let root = temp_dir("find");
        std::fs::create_dir_all(root.join("attachments/deep")).unwrap();
        std::fs::create_dir_all(root.join(".rista")).unwrap();
        std::fs::write(root.join(".rista/Report.pdf"), b"x").unwrap();
        std::fs::write(root.join("attachments/deep/Report.pdf"), b"x").unwrap();
        assert_eq!(
            find_vault_file(&root, "report.pdf"),
            Some(root.join("attachments/deep/Report.pdf"))
        );
        assert_eq!(
            find_vault_file(&root, "attachments/deep/Report.pdf"),
            Some(root.join("attachments/deep/Report.pdf"))
        );
        assert_eq!(find_vault_file(&root, "missing.pdf"), None);
    }

    #[test]
    fn template_lookup_is_vault_contained_and_refuses_ambiguous_names() {
        let root = unique_temp_dir("markdown");
        let outside = unique_temp_dir("markdown-outside");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::create_dir_all(root.join("two")).unwrap();
        std::fs::write(root.join("nested/Note.md"), b"nested").unwrap();
        std::fs::write(root.join("one/Shared.md"), b"one").unwrap();
        std::fs::write(root.join("two/Shared.md"), b"two").unwrap();
        assert_eq!(
            find_vault_markdown(&root, "nested/Note"),
            Some(root.canonicalize().unwrap().join("nested/Note.md"))
        );
        assert_eq!(find_vault_markdown(&root, "Shared"), None);
        assert_eq!(find_vault_markdown(&root, "nested/Note.txt"), None);

        #[cfg(unix)]
        {
            std::fs::write(outside.join("Secret.md"), b"outside").unwrap();
            std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
            assert_eq!(find_vault_markdown(&root, "linked/Secret"), None);
            assert_eq!(find_vault_markdown(&root, "Secret"), None);
        }

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }
}
