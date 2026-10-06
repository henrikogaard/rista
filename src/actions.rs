//! Application actions — dispatched by menus, key bindings, and the palette.

use gpui_kit::*;

actions!(
    rista,
    [
        // File
        NewFile,
        NewFolder,
        OpenFolder,
        OpenDailyNote,
        CloseFolder,
        SaveFile,
        SaveFileAs,
        CloseTab,
        ReopenTab,
        // Edit
        MoveLineUp,
        MoveLineDown,
        ToggleCheckbox,
        ToggleItalic,
        DuplicateBlock,
        DeleteLine,
        ToggleComment,
        // Navigation
        NextTab,
        PrevTab,
        NavigateBack,
        NavigateForward,
        FollowLink,
        ToggleSidebar,
        ToggleZen,
        // View
        ViewSource,
        ViewSplit,
        ViewPreview,
        ToggleEditPreview,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        OpenGraph,
        CloseGraph,
        // App
        OpenCommandPalette,
        OpenProjectSearch,
        OpenSettings,
        ToggleTheme,
        Quit,
        About,
    ]
);

/// Auto-pair actions — `pair` is the two-character pair string
/// ("()", "\"\"", "~~", …) whose first half opens and second half closes.
/// `PairInsert` is bound on openers and symmetric chars; `PairClose` on the
/// closers `)]}`. Bound with the "RistaEditor" key context so they only fire
/// in the document editor (dialog/search inputs type the plain chars).
#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = rista, no_json)]
pub struct PairInsert {
    pub pair: &'static str,
}

#[derive(Clone, Debug, PartialEq, gpui_kit::Action)]
#[action(namespace = rista, no_json)]
pub struct PairClose {
    pub pair: &'static str,
}
