//! Rísta — a local-first Markdown editor carved in Rust + GPUI.
//!
//! Nordic restraint: quiet surfaces, honest files, no chrome you didn't ask for.

mod actions;
mod app;
mod bases;
mod decorations;
mod document;
mod history;
mod http;
mod preview;
mod properties;
mod search;
mod settings;
mod settings_panel;
mod slash;
mod theme;
mod vault;

use crate::actions::*;
use gpui_kit::component::input;
use gpui_kit::component::{Theme, TitleBar};
use gpui_kit::*;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .with_http_client(http::client())
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::install_themes(cx);

            let settings = settings::Settings::load();
            theme::set_theme_mode(settings.theme_mode(cx), cx);
            apply_ui_settings(&settings, cx);

            cx.bind_keys(keymap());
            cx.set_menus(menus());
            cx.activate(true);

            let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(720.), px(480.))),
                    kind: WindowKind::Normal,
                    ..TitleBar::window_options()
                },
                cx,
                move |window, cx| cx.new(|cx| app::Workspace::new(window, cx)),
            )
            .expect("Failed to open window");
        });
}

/// Font-size globals live on the Theme; the editor reads `mono_*` tokens.
fn apply_ui_settings(settings: &settings::Settings, cx: &mut App) {
    let mono_family = SharedString::from(settings.editor_font_family.clone());
    let mono_size = px(settings.editor_font_size);
    let ui_size = px(settings.ui_font_size);
    Theme::update(cx, move |theme| {
        theme.mono_font_family = mono_family;
        theme.mono_font_size = mono_size;
        theme.font_size = ui_size;
    });
}

fn keymap() -> Vec<KeyBinding> {
    vec![
        // Files & folders
        KeyBinding::new("cmd-n", NewFile, None),
        KeyBinding::new("cmd-shift-n", NewFolder, None),
        KeyBinding::new("cmd-o", OpenFolder, None),
        KeyBinding::new("cmd-shift-d", OpenDailyNote, None),
        KeyBinding::new("cmd-s", SaveFile, None),
        KeyBinding::new("cmd-shift-s", SaveFileAs, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        // Editing
        KeyBinding::new("alt-up", MoveLineUp, None),
        KeyBinding::new("alt-down", MoveLineDown, None),
        // Navigation
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("cmd-[", NavigateBack, None),
        KeyBinding::new("cmd-]", NavigateForward, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-shift-enter", ToggleZen, None),
        // View modes — 1/2/3 with cmd.
        KeyBinding::new("cmd-1", ViewSource, None),
        KeyBinding::new("cmd-2", ViewSplit, None),
        KeyBinding::new("cmd-3", ViewPreview, None),
        // App
        KeyBinding::new("cmd-k", OpenCommandPalette, None),
        KeyBinding::new("cmd-shift-f", OpenProjectSearch, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]
}

fn menus() -> Vec<Menu> {
    vec![
        Menu::new("Rísta").items([
            MenuItem::separator(),
            MenuItem::Action {
                name: "About Rísta".into(),
                action: Box::new(About),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::Action {
                name: "Quit Rísta".into(),
                action: Box::new(Quit),
                os_action: None,
                checked: false,
                disabled: false,
            },
        ]),
        Menu::new("File").items([
            MenuItem::Action {
                name: "New File".into(),
                action: Box::new(NewFile),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "New Folder".into(),
                action: Box::new(NewFolder),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Open Folder…".into(),
                action: Box::new(OpenFolder),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Open Daily Note".into(),
                action: Box::new(OpenDailyNote),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Close Folder".into(),
                action: Box::new(CloseFolder),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Save".into(),
                action: Box::new(SaveFile),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Save As…".into(),
                action: Box::new(SaveFileAs),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Close Tab".into(),
                action: Box::new(CloseTab),
                os_action: None,
                checked: false,
                disabled: false,
            },
        ]),
        Menu::new("Edit").items([
            MenuItem::Action {
                name: "Undo".into(),
                action: Box::new(input::Undo),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Redo".into(),
                action: Box::new(input::Redo),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Cut".into(),
                action: Box::new(input::Cut),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Copy".into(),
                action: Box::new(input::Copy),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Paste".into(),
                action: Box::new(input::Paste),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Select All".into(),
                action: Box::new(input::SelectAll),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Find…".into(),
                action: Box::new(input::Search),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Find in Project…".into(),
                action: Box::new(OpenProjectSearch),
                os_action: None,
                checked: false,
                disabled: false,
            },
        ]),
        Menu::new("View").items([
            MenuItem::Action {
                name: "Source".into(),
                action: Box::new(ViewSource),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Split".into(),
                action: Box::new(ViewSplit),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Preview".into(),
                action: Box::new(ViewPreview),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Navigate Back".into(),
                action: Box::new(NavigateBack),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Navigate Forward".into(),
                action: Box::new(NavigateForward),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Toggle Sidebar".into(),
                action: Box::new(ToggleSidebar),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Toggle Theme".into(),
                action: Box::new(ToggleTheme),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Zen Mode".into(),
                action: Box::new(ToggleZen),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Command Palette…".into(),
                action: Box::new(OpenCommandPalette),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Settings…".into(),
                action: Box::new(OpenSettings),
                os_action: None,
                checked: false,
                disabled: false,
            },
        ]),
        Menu::new("Window"),
    ]
}
