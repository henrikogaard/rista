//! Rísta — a local-first Markdown editor carved in Rust + GPUI.
//!
//! Nordic restraint: quiet surfaces, honest files, no chrome you didn't ask for.

#[cfg(target_os = "macos")]
mod about;

mod actions;
mod agent;
mod app;
mod bases;
mod bookmarks;
mod canvas;
mod canvas_history;
mod canvas_view;
mod decorations;
mod document;
mod editor_layout;
mod emoji;
mod explorer;
mod extensions;
mod file_preview;
mod folder;
mod graph;
mod graph_layout;
mod history;
mod hotkeys;
mod http;
mod math;
mod mermaid;
mod note_icons;
mod pdf;
mod pdf_backend;
mod pdf_export;
mod preview;
mod properties;
mod query_embed;
mod search;
pub mod search_query;
pub mod search_service;
mod settings;
mod settings_panel;
mod slash;
mod spellcheck;
mod templater;
mod terminal;
mod terminal_session;
mod theme;
mod updater;
mod vault;
mod windows;

use crate::actions::*;
use gpui_kit::component::input;
use gpui_kit::component::Theme;
use gpui_kit::*;

struct MenuLanguageSubscription {
    _subscription: Subscription,
}

impl Global for MenuLanguageSubscription {}

fn open_paths(paths: Vec<std::path::PathBuf>, cx: &mut App) {
    windows::open_paths(paths, cx);
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
        pdf::init(cx);
        theme::install_themes(cx);

        let settings = settings::Settings::load();
        windows::install(settings.clone(), cx);
        windows::observe_window_closed(cx);
        let menu_language_subscription = cx.observe_global::<windows::SharedPreferences>(|cx| {
            let language = cx.global::<windows::SharedPreferences>().settings.language;
            cx.set_menus(menus(language));
        });
        cx.set_global(MenuLanguageSubscription {
            _subscription: menu_language_subscription,
        });
        theme::apply(&settings, cx);
        apply_ui_settings(&settings, cx);

        hotkeys::initialize(cx, internal_keymap());
        cx.set_menus(menus(settings.language));
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &About, _cx| {
            about::show();
        });
        // Global actions still hold the dispatching window. Release that borrow
        // before visiting every workspace through its window handle.
        cx.on_action(|_: &Quit, cx| cx.defer(windows::quit));
        cx.on_action(|_: &NewWindow, cx| {
            cx.defer(|cx| windows::open_new_window(Vec::new(), cx));
        });
        cx.on_action(|_: &OpenInNewWindow, cx| {
            cx.defer(windows::open_active_document_in_new_window);
        });
        cx.on_action(|action: &OpenPathInNewWindow, cx| {
            let path = std::path::PathBuf::from(&action.path);
            cx.defer(move |cx| windows::open_new_window(vec![path], cx));
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

pub(crate) fn internal_keymap() -> Vec<KeyBinding> {
    vec![
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

fn menus(language: settings::Language) -> Vec<Menu> {
    let mut app_items = vec![
        MenuItem::separator(),
        MenuItem::Action {
            name: language.text("About Rísta", "Om Rísta").into(),
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
                name: language.text("New Window", "Nytt vindu").into(),
                action: Box::new(NewWindow),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: language
                    .text("Open in New Window", "Åpne i nytt vindu")
                    .into(),
                action: Box::new(OpenInNewWindow),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::separator(),
            MenuItem::Action {
                name: "New File".into(),
                action: Box::new(NewFile),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: language.text("New Canvas", "Nytt lerret").into(),
                action: Box::new(NewCanvas),
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
                name: language.text("Delete", "Slett").into(),
                action: Box::new(CanvasDelete),
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
            MenuItem::Action {
                name: language
                    .text("Toggle local graph panel", "Vis/skjul lokalt grafpanel")
                    .into(),
                action: Box::new(ToggleLocalGraphPanel),
                os_action: None,
                checked: false,
                disabled: false,
            },
            MenuItem::Action {
                name: language
                    .text("Toggle agent panel", "Vis/skjul agentpanel")
                    .into(),
                action: Box::new(ToggleAgentPanel),
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
