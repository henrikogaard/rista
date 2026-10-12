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
mod cli;
mod cli_install;
mod cli_ipc;
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
mod note_composer;
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
mod vault_settings_import;
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
    let startup = match cli::parse_argv(std::env::args_os().skip(1).collect()) {
        Ok(startup) => startup,
        Err(error) => {
            let settings = settings::Settings::load();
            eprintln!("{}", error.message(cli::settings_language(&settings)));
            std::process::exit(2);
        }
    };
    let startup = match startup {
        cli::Startup::Help => {
            let settings = settings::Settings::load();
            println!("{}", cli::help(cli::settings_language(&settings)));
            return;
        }
        cli::Startup::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return;
        }
        cli::Startup::Headless(mut request) => {
            let settings = settings::Settings::load();
            let result = if cli::is_read_only(&request.command) {
                cli::run_read_only(&request.command).map(|lines| {
                    for line in lines {
                        println!("{line}");
                    }
                })
            } else {
                cli::execute_write_coordinated(
                    &mut request,
                    &settings,
                    chrono::Local::now().naive_local(),
                    &mut std::io::stdin().lock(),
                )
                .map(|lines| {
                    for line in lines {
                        println!("{line}");
                    }
                })
            };
            if let Err(error) = result {
                eprintln!("{}", error.message(cli::settings_language(&settings)));
                std::process::exit(1);
            }
            return;
        }
        cli::Startup::Gui(requests) => {
            if let Err(error) = cli::validate_gui_startup(&requests) {
                let settings = settings::Settings::load();
                eprintln!("{}", error.message(cli::settings_language(&settings)));
                std::process::exit(2);
            }
            requests
        }
    };
    #[cfg(unix)]
    let (ipc_paths, ipc_lock, ipc_listener) = {
        let (paths, attempt) = match cli_ipc::try_lock(&settings::config_path()) {
            Ok(result) => result,
            Err(error) => {
                let settings = settings::Settings::load();
                eprintln!("{}", error.message(cli::settings_language(&settings)));
                std::process::exit(1);
            }
        };
        match attempt {
            cli_ipc::LockAttempt::Acquired(lock) => {
                let listener = match cli_ipc::bind_locked(&paths, &lock) {
                    Ok(listener) => listener,
                    Err(error) => {
                        let settings = settings::Settings::load();
                        eprintln!("{}", error.message(cli::settings_language(&settings)));
                        std::process::exit(1);
                    }
                };
                (paths, lock, listener)
            }
            cli_ipc::LockAttempt::Busy => {
                let requests = if startup.is_empty() {
                    vec![cli::CliRequest {
                        command: cli::Command::OpenPaths { paths: Vec::new() },
                        origin: cli::RequestOrigin::Legacy,
                    }]
                } else {
                    startup.clone()
                };
                for request in requests {
                    let request = cli::normalize_request_paths(&request);
                    let response = cli_ipc::forward(&paths, &request)
                        .unwrap_or_else(cli::CliResponse::failure);
                    if let Err(error) = response.result {
                        let settings = settings::Settings::load();
                        eprintln!("{}", error.message(cli::settings_language(&settings)));
                        std::process::exit(1);
                    }
                }
                return;
            }
        }
    };
    let (open_url_tx, open_url_rx) = smol::channel::unbounded::<Vec<String>>();
    #[cfg(unix)]
    let (ipc_tx, ipc_rx) = smol::channel::unbounded::<cli_ipc::PendingRequest>();
    #[cfg(unix)]
    let ipc_running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    #[cfg(unix)]
    let ipc_server_running = ipc_running.clone();
    #[cfg(unix)]
    let ipc_thread = std::thread::spawn(move || {
        cli_ipc::serve(ipc_listener, ipc_tx, ipc_server_running);
    });
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

        dispatch_cli_requests(startup, cx);
        cx.spawn(async move |cx| {
            while let Ok(urls) = open_url_rx.recv().await {
                for url in urls {
                    let request = if url::Url::parse(&url)
                        .ok()
                        .filter(|url| url.scheme() == "file")
                        .is_some()
                    {
                        cli::parse_file_url(&url).map(|path| cli::CliRequest {
                            command: cli::Command::OpenPaths { paths: vec![path] },
                            origin: cli::RequestOrigin::Legacy,
                        })
                    } else {
                        cli::parse_url(&url)
                    };
                    match request {
                        Ok(request) => {
                            cx.update(|cx| {
                                dispatch_cli_requests(vec![request], cx);
                            });
                        }
                        Err(error) => {
                            cx.update(|cx| windows::show_cli_error(&error, cx));
                        }
                    }
                }
            }
        })
        .detach();
        #[cfg(unix)]
        cx.spawn(async move |cx| {
            while let Ok((request, reply)) = ipc_rx.recv().await {
                let response = cx.update(|cx| dispatch_cli_requests(vec![request], cx));
                let _ = reply.send(response);
            }
        })
        .detach();
    });
    #[cfg(unix)]
    {
        ipc_running.store(false, std::sync::atomic::Ordering::Release);
        let _ = ipc_thread.join();
        let _ = cli_ipc::remove_owned_socket(&ipc_paths);
        drop(ipc_lock);
    }
}

fn dispatch_cli_requests(requests: Vec<cli::CliRequest>, cx: &mut App) -> cli::CliResponse {
    let mut output = Vec::new();
    if requests.is_empty() {
        open_paths(Vec::new(), cx);
        return cli::CliResponse::success(Vec::new());
    }
    for request in requests {
        let result = (|| -> Result<Vec<String>, cli::CliError> {
            match &request.command {
                cli::Command::Open { vault, note } => {
                    windows::open_cli_vault(vault, note.as_deref(), cx)?;
                    Ok(Vec::new())
                }
                cli::Command::OpenPaths { paths } => {
                    windows::open_paths_checked(paths.clone(), cx)?;
                    Ok(Vec::new())
                }
                cli::Command::Search { vault, query } => {
                    windows::open_cli_search(vault, query, cx)?;
                    Ok(Vec::new())
                }
                cli::Command::Links { vault, .. } | cli::Command::Backlinks { vault, .. } => {
                    let lines = cli::run_read_only(&request.command)?;
                    let language = cx.global::<windows::WindowManager>().settings.language;
                    let count = lines.len();
                    let title = match (&request.command, language) {
                        (cli::Command::Links { .. }, settings::Language::English) => {
                            format!("Local links ({count})")
                        }
                        (cli::Command::Links { .. }, settings::Language::Norwegian) => {
                            format!("Lokale lenker ({count})")
                        }
                        (_, settings::Language::English) => format!("Backlinks ({count})"),
                        (_, settings::Language::Norwegian) => {
                            format!("Tilbakekoblinger ({count})")
                        }
                    };
                    windows::show_cli_results(vault.as_deref(), title, lines.clone(), cx)?;
                    Ok(lines)
                }
                cli::Command::New { .. }
                | cli::Command::Daily { .. }
                | cli::Command::Append { .. } => {
                    if request.origin == cli::RequestOrigin::Url {
                        windows::confirm_cli_write(request.clone(), cx)?;
                        Ok(Vec::new())
                    } else {
                        windows::execute_cli_write(request.clone(), cx)
                    }
                }
            }
        })();
        match result {
            Ok(lines) => output.extend(lines),
            Err(error) => {
                windows::show_cli_error(&error, cx);
                return cli::CliResponse::failure(error);
            }
        }
    }
    cli::CliResponse::success(output)
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
    app_items.push(MenuItem::separator());
    #[cfg(target_os = "macos")]
    app_items.push(MenuItem::Action {
        name: language
            .text(
                "Install Command-Line Tool",
                "Installer kommandolinjeverktøy",
            )
            .into(),
        action: Box::new(InstallCli),
        os_action: None,
        checked: false,
        disabled: false,
    });
    app_items.extend([
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
