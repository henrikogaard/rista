//! Rísta — a local-first Markdown editor carved in Rust + GPUI.
//!
//! Nordic restraint: quiet surfaces, honest files, no chrome you didn't ask for.

mod actions;
mod app;
mod bases;
mod decorations;
mod document;
mod emoji;
mod extensions;
mod file_preview;
mod folder;
mod graph;
mod history;
mod http;
mod note_icons;
mod preview;
mod properties;
mod search;
mod settings;
mod settings_panel;
mod slash;
mod terminal;
mod terminal_session;
mod theme;
mod updater;
mod vault;

use crate::actions::*;
use gpui_kit::component::input;
use gpui_kit::component::{Theme, TitleBar};
use gpui_kit::*;

#[derive(Clone)]
struct WorkspaceWindow {
    window: AnyWindowHandle,
    workspace: WeakEntity<app::Workspace>,
}

impl Global for WorkspaceWindow {}

fn open_paths(paths: Vec<std::path::PathBuf>, cx: &mut App) {
    if let Some(current) = cx.try_global::<WorkspaceWindow>().cloned() {
        if matches!(
            current.window.update(cx, |_, window, cx| {
                current.workspace.update(cx, |workspace, cx| {
                    for path in &paths {
                        workspace.open_path(path.clone(), window, cx);
                    }
                    window.activate_window();
                })
            }),
            Ok(Ok(()))
        ) {
            return;
        }
    }
    let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
    let (window, workspace) = gpui_kit::open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(720.), px(480.))),
            kind: WindowKind::Normal,
            ..TitleBar::window_options()
        },
        cx,
        move |window, cx| cx.new(|cx| app::Workspace::new(window, cx, paths)),
    )
    .expect("Failed to open window");
    cx.set_global(WorkspaceWindow {
        window,
        workspace: workspace.downgrade(),
    });
    cx.activate(true);
}

fn main() {
    let initial_paths = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .map(|path| std::fs::canonicalize(&path).unwrap_or(path))
        .collect::<Vec<_>>();
    let (open_url_tx, open_url_rx) = smol::channel::unbounded::<Vec<String>>();
    let app = gpui_kit::application()
        // AllAssets embeds the full lucide set — the default `Assets` bundle
        // only covers ~100 icons, which silently blanked several IconName
        // variants we use (Table, SquareKanban, ListTodo, FileClock…).
        .with_assets(gpui_kit::assets::AllAssets)
        .with_http_client(http::client());
    app.on_open_urls(move |urls| {
        let _ = open_url_tx.try_send(urls);
    });
    app.on_reopen(|cx| open_paths(Vec::new(), cx));
    app.run(move |cx| {
        gpui_kit::init(cx);
        theme::install_themes(cx);

        let settings = settings::Settings::load();
        theme::apply(&settings, cx);
        apply_ui_settings(&settings, cx);

        cx.bind_keys(keymap());
        cx.set_menus(menus());
        cx.on_action(|_: &Quit, cx| {
            if cx.windows().is_empty() {
                cx.quit();
            } else if let Some(current) = cx.try_global::<WorkspaceWindow>().cloned() {
                let _ = current
                    .workspace
                    .update(cx, |workspace, cx| workspace.quit(cx));
            }
        });
        cx.activate(true);
        updater::start();

        open_paths(initial_paths, cx);
        cx.spawn(async move |cx| {
            while let Ok(urls) = open_url_rx.recv().await {
                let paths = urls
                    .into_iter()
                    .filter_map(|url| {
                        url::Url::parse(&url)
                            .ok()
                            .filter(|url| url.scheme() == "file")
                            .and_then(|url| url.to_file_path().ok())
                    })
                    .collect::<Vec<_>>();
                if !paths.is_empty() {
                    cx.update(|cx| open_paths(paths, cx));
                }
            }
        })
        .detach();
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
        KeyBinding::new("cmd-o", OpenFile, None),
        KeyBinding::new("cmd-shift-o", OpenFolder, None),
        KeyBinding::new("cmd-shift-d", OpenDailyNote, None),
        KeyBinding::new("cmd-s", SaveFile, None),
        KeyBinding::new("cmd-shift-s", SaveFileAs, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-t", ReopenTab, None),
        // Editing
        KeyBinding::new("alt-up", MoveLineUp, None),
        KeyBinding::new("alt-down", MoveLineDown, None),
        KeyBinding::new("cmd-enter", ToggleCheckbox, None),
        KeyBinding::new("cmd-i", ToggleItalic, None),
        KeyBinding::new("cmd-d", DuplicateBlock, None),
        KeyBinding::new("cmd-shift-k", DeleteLine, None),
        KeyBinding::new("cmd-/", ToggleComment, None),
        // Navigation
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("cmd-[", NavigateBack, None),
        KeyBinding::new("cmd-]", NavigateForward, None),
        KeyBinding::new("alt-enter", FollowLink, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-shift-enter", ToggleZen, None),
        // View modes — 1/2/3 with cmd.
        KeyBinding::new("cmd-1", ViewSource, None),
        KeyBinding::new("cmd-2", ViewSplit, None),
        KeyBinding::new("cmd-3", ViewPreview, None),
        KeyBinding::new("cmd-e", ToggleEditPreview, None),
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("cmd-minus", ZoomOut, None),
        KeyBinding::new("cmd-0", ZoomReset, None),
        KeyBinding::new("cmd-g", OpenGraph, None),
        // Esc only closes the graph when its pane is focused — the
        // RistaGraph key context keeps dialogs/inputs unaffected.
        KeyBinding::new("escape", CloseGraph, Some("RistaGraph")),
        // App
        KeyBinding::new("cmd-k", OpenCommandPalette, None),
        KeyBinding::new("cmd-j", ToggleTerminal, None),
        // the reference editor muscle memory — same palette as ⌘K.
        KeyBinding::new("cmd-p", OpenCommandPalette, None),
        // ⌘F reaches the editor when focused; the workspace handler
        // filters .base rows or refocuses the editor otherwise.
        KeyBinding::new("cmd-f", input::Search, None),
        KeyBinding::new("cmd-shift-f", OpenProjectSearch, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-q", Quit, None),
        // Auto-pairs — "RistaEditor" scopes them to the document
        // editor so search/dialog inputs still type the plain characters.
        KeyBinding::new("(", PairInsert { pair: "()" }, Some("RistaEditor")),
        KeyBinding::new("[", PairInsert { pair: "[]" }, Some("RistaEditor")),
        KeyBinding::new("{", PairInsert { pair: "{}" }, Some("RistaEditor")),
        KeyBinding::new("\"", PairInsert { pair: "\"\"" }, Some("RistaEditor")),
        KeyBinding::new("'", PairInsert { pair: "''" }, Some("RistaEditor")),
        KeyBinding::new("`", PairInsert { pair: "``" }, Some("RistaEditor")),
        KeyBinding::new("~", PairInsert { pair: "~~" }, Some("RistaEditor")),
        KeyBinding::new("=", PairInsert { pair: "==" }, Some("RistaEditor")),
        KeyBinding::new("%", PairInsert { pair: "%%" }, Some("RistaEditor")),
        KeyBinding::new("$", PairInsert { pair: "$$" }, Some("RistaEditor")),
        KeyBinding::new(")", PairClose { pair: "()" }, Some("RistaEditor")),
        KeyBinding::new("]", PairClose { pair: "[]" }, Some("RistaEditor")),
        KeyBinding::new("}", PairClose { pair: "{}" }, Some("RistaEditor")),
    ]
}

fn menus() -> Vec<Menu> {
    let mut app_items = vec![
        MenuItem::separator(),
        MenuItem::Action {
            name: "About Rísta".into(),
            action: Box::new(About),
            os_action: None,
            checked: false,
            disabled: false,
        },
    ];
    // Only builds that link Sparkle can actually check.
    if crate::updater::AVAILABLE {
        app_items.push(MenuItem::Action {
            name: "Check for Updates…".into(),
            action: Box::new(CheckForUpdates),
            os_action: None,
            checked: false,
            disabled: false,
        });
    }
    app_items.extend([
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
    ]);
    vec![
        Menu::new("Rísta").items(app_items),
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
                name: "Open File…".into(),
                action: Box::new(OpenFile),
                os_action: None,
                checked: false,
                disabled: false,
            },
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
            MenuItem::separator(),
            MenuItem::Action {
                name: "Duplicate Line".into(),
                action: Box::new(DuplicateBlock),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Toggle Comment".into(),
                action: Box::new(ToggleComment),
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
            MenuItem::Action {
                name: "Toggle Edit/Preview".into(),
                action: Box::new(ToggleEditPreview),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Graph View".into(),
                action: Box::new(OpenGraph),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Local Graph".into(),
                action: Box::new(OpenLocalGraph),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "Zoom In".into(),
                action: Box::new(ZoomIn),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Zoom Out".into(),
                action: Box::new(ZoomOut),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: "Reset Zoom".into(),
                action: Box::new(ZoomReset),
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
