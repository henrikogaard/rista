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
        // Navigation
        NextTab,
        PrevTab,
        ToggleSidebar,
        ToggleZen,
        // View
        ViewSource,
        ViewSplit,
        ViewPreview,
        // App
        OpenCommandPalette,
        OpenProjectSearch,
        OpenSettings,
        ToggleTheme,
        Quit,
        About,
    ]
);
