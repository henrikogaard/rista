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
        // App
        OpenCommandPalette,
        OpenProjectSearch,
        OpenSettings,
        ToggleTheme,
        Quit,
        About,
    ]
);
