use crate::app::Workspace;
use crate::settings::{Settings, WindowSession};
use gpui_kit::component::TitleBar;
use gpui_kit::*;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

const MAX_RESTORED_WINDOWS: usize = 64;

const LOCAL_FIELDS: &[&str] = &[
    "last_vault",
    "active_folder",
    "open_tabs",
    "active_tab",
    "editor_session",
    "view_mode",
    "pinned_tabs",
    "sidebar_collapsed",
    "sidebar_width",
    "dock_width",
    "dock_heights",
    "inspector_open",
    "graph_dock_open",
    "agent_open",
    "panes",
    "expanded_folders",
    "window_sessions",
    "active_window_id",
];

#[derive(Clone)]
pub(crate) struct WindowRecord {
    pub(crate) id: u64,
    pub(crate) window: AnyWindowHandle,
    pub(crate) workspace: WeakEntity<Workspace>,
}

pub(crate) struct WindowManager {
    pub(crate) settings: Settings,
    pub(crate) records: Vec<WindowRecord>,
    pub(crate) next_id: u64,
    pub(crate) restoring: bool,
    pub(crate) quitting: bool,
}

impl Global for WindowManager {}

#[derive(Clone)]
pub(crate) struct SharedPreferences {
    pub(crate) settings: Settings,
}

impl Global for SharedPreferences {}

impl WindowManager {
    pub(crate) fn new(mut settings: Settings) -> Self {
        normalize_sessions(&mut settings);
        let start_id = settings
            .window_sessions
            .iter()
            .map(|session| session.id)
            .max()
            .unwrap_or(0)
            .wrapping_add(1)
            .max(1);
        let next_id = next_available_id(start_id, &settings.window_sessions, &[]);
        Self {
            settings,
            records: Vec::new(),
            next_id,
            restoring: false,
            quitting: false,
        }
    }

    pub(crate) fn reserve_id(&mut self) -> u64 {
        let id = next_available_id(self.next_id, &self.settings.window_sessions, &self.records);
        self.next_id = id.wrapping_add(1).max(1);
        id
    }

    pub(crate) fn session(&self, id: u64) -> Option<WindowSession> {
        self.settings
            .window_sessions
            .iter()
            .find(|session| session.id == id)
            .cloned()
    }

    pub(crate) fn register(
        &mut self,
        id: u64,
        window: AnyWindowHandle,
        workspace: WeakEntity<Workspace>,
    ) {
        self.records.push(WindowRecord {
            id,
            window,
            workspace,
        });
    }

    pub(crate) fn active(&self) -> Option<WindowRecord> {
        let id = self.settings.active_window_id?;
        self.records.iter().find(|record| record.id == id).cloned()
    }
}

pub(crate) fn install(settings: Settings, cx: &mut App) {
    let mut settings = settings;
    settings.bookmarks.migrate_stars(&settings.starred);
    settings.starred = settings.bookmarks.starred_paths();
    let manager = WindowManager::new(settings);
    cx.set_global(SharedPreferences {
        settings: manager.settings.clone(),
    });
    cx.set_global(manager);
}

pub(crate) fn open_active_document_in_new_window(cx: &mut App) {
    let record = cx
        .global::<WindowManager>()
        .active()
        .or_else(|| cx.global::<WindowManager>().records.last().cloned());
    let path = record.and_then(|record| {
        record
            .workspace
            .update(cx, |workspace, cx| workspace.active_document_path(cx))
            .ok()
            .flatten()
    });
    if let Some(path) = path {
        open_new_window(vec![path], cx);
    }
}

pub(crate) fn open_paths(paths: Vec<PathBuf>, cx: &mut App) {
    let record = {
        let manager = cx.global_mut::<WindowManager>();
        let record = manager.active().or_else(|| manager.records.last().cloned());
        if let Some(record) = &record {
            manager.settings.active_window_id = Some(record.id);
            sync_compatibility_facade(&mut manager.settings);
            manager.settings.save();
        }
        record
    };
    if let Some(record) = record {
        let routed = record.window.update(cx, |_, window, cx| {
            record.workspace.update(cx, |workspace, cx| {
                for path in &paths {
                    workspace.open_path(path.clone(), window, cx);
                }
                window.activate_window();
            })
        });
        if matches!(routed, Ok(Ok(()))) {
            return;
        }
    }

    if paths.is_empty() {
        restore_startup(cx);
    } else {
        let manager = cx.global_mut::<WindowManager>();
        prepare_explicit_startup(&mut manager.settings);
        open_workspace(paths, false, cx);
    }
}

fn prepare_explicit_startup(settings: &mut Settings) {
    let view_mode = settings.view_mode;
    settings.window_sessions.clear();
    apply_session(settings, &WindowSession::default());
    settings.view_mode = view_mode;
    settings.active_window_id = None;
}

pub(crate) fn open_workspace(initial_paths: Vec<PathBuf>, restore_session: bool, cx: &mut App) {
    let session = if restore_session {
        let manager = cx.global::<WindowManager>();
        manager
            .settings
            .active_window_id
            .and_then(|id| manager.session(id))
    } else {
        None
    };
    let (id, reserved) = if let Some(session) = &session {
        (session.id, false)
    } else {
        (cx.global_mut::<WindowManager>().reserve_id(), true)
    };
    create_workspace(id, session, initial_paths, restore_session, reserved, cx);
}

pub(crate) fn open_new_window(paths: Vec<PathBuf>, cx: &mut App) {
    if let Some(path) = paths.first() {
        let path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let records = cx.global::<WindowManager>().records.clone();
        for record in &records {
            let result = record.window.update(cx, |_, window, cx| {
                record.workspace.update(cx, |workspace, cx| {
                    workspace.flush_path_for_new_window(&path, window, cx)
                })
            });
            if !matches!(result, Ok(Ok(Ok(())))) {
                let _ = record.window.update(cx, |_, window, cx| {
                    window.activate_window();
                    let _ = record.workspace.update(cx, |workspace, cx| {
                        workspace.note_status(
                            workspace.tr(
                                "Could not save this note before opening another window.",
                                "Kunne ikke lagre dette notatet før et nytt vindu åpnes.",
                            ),
                            cx,
                        );
                    });
                });
                return;
            }
        }
    }
    let has_live_windows = !cx.global::<WindowManager>().records.is_empty();
    clear_fallback_session_without_live_windows(
        &mut cx.global_mut::<WindowManager>().settings,
        has_live_windows,
    );
    let active_record = cx
        .global::<WindowManager>()
        .active()
        .or_else(|| cx.global::<WindowManager>().records.last().cloned());
    let active_session = active_record.and_then(|record| {
        record
            .workspace
            .update(cx, |workspace, _cx| {
                WindowSession::from_settings(record.id, workspace.settings())
            })
            .ok()
    });
    let id = cx.global_mut::<WindowManager>().reserve_id();
    let session = active_session.map(|session| prepare_new_window_session(session, &paths));
    create_workspace(id, session, paths, false, true, cx);
}

fn prepare_new_window_session(mut session: WindowSession, paths: &[PathBuf]) -> WindowSession {
    if let Some(path) = paths.first() {
        let path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let same_vault = session
            .last_vault
            .as_ref()
            .is_some_and(|root| path.starts_with(root));
        if !same_vault {
            session.last_vault = None;
            session.active_folder = None;
            session.expanded_folders.clear();
        }
    }
    session.open_tabs.clear();
    session.active_tab = None;
    session.editor_session = None;
    session.pinned_tabs.clear();
    session
}

fn restore_startup(cx: &mut App) {
    let active_window = {
        let manager = cx.global_mut::<WindowManager>();
        if manager.records.is_empty() {
            None
        } else {
            let record = manager.active().or_else(|| manager.records.last().cloned());
            if let Some(record) = &record {
                manager.settings.active_window_id = Some(record.id);
            }
            record.map(|record| record.window)
        }
    };
    if let Some(window) = active_window {
        let _ = window.update(cx, |_, window, _cx| window.activate_window());
        return;
    }
    let (sessions, saved_active_id) = {
        let manager = cx.global_mut::<WindowManager>();
        manager.restoring = true;
        (
            manager.settings.window_sessions.clone(),
            manager.settings.active_window_id,
        )
    };
    let sessions = if sessions.is_empty() {
        vec![WindowSession::from_settings(
            cx.global::<WindowManager>().next_id,
            &cx.global::<WindowManager>().settings,
        )]
    } else {
        sessions.into_iter().take(MAX_RESTORED_WINDOWS).collect()
    };
    for session in &sessions {
        create_workspace(
            session.id,
            Some(session.clone()),
            Vec::new(),
            true,
            false,
            cx,
        );
    }
    let records = cx.global::<WindowManager>().records.clone();
    let mut restored = records
        .iter()
        .filter_map(|record| {
            record
                .workspace
                .update(cx, |workspace, _cx| {
                    WindowSession::from_settings(record.id, workspace.settings())
                })
                .ok()
        })
        .collect::<Vec<_>>();
    let settings = {
        let manager = cx.global_mut::<WindowManager>();
        manager.restoring = false;
        for session in &mut restored {
            session.geometry = manager.session(session.id).and_then(|saved| saved.geometry);
        }
        replace_session_snapshot(&mut manager.settings, restored, !records.is_empty());
        manager.settings.active_window_id = saved_active_id
            .filter(|id| manager.records.iter().any(|record| record.id == *id))
            .or_else(|| manager.records.first().map(|record| record.id));
        sync_compatibility_facade(&mut manager.settings);
        manager.settings.clone()
    };
    settings.save();
    cx.update_global::<SharedPreferences, _>(|shared, _cx| shared.settings = settings.clone());
    if let Some(record) = cx.global::<WindowManager>().active() {
        let _ = record
            .window
            .update(cx, |_, window, _cx| window.activate_window());
    }
}

fn create_workspace(
    id: u64,
    session: Option<WindowSession>,
    initial_paths: Vec<PathBuf>,
    restore_session: bool,
    reserved: bool,
    cx: &mut App,
) {
    let initial_geometry = session
        .as_ref()
        .and_then(|session| session.geometry.clone());
    let mut settings = cx.global::<WindowManager>().settings.clone();
    if let Some(session) = &session {
        apply_session(&mut settings, session);
    } else if !restore_session {
        prepare_explicit_startup(&mut settings);
    }
    let geometry = session
        .as_ref()
        .and_then(|session| session.geometry.as_ref())
        .filter(|geometry| valid_geometry(geometry) && visible_on_display(geometry, cx));
    let fallback_size = size(px(1280.), px(800.));
    let bounds = geometry.map_or_else(
        || Bounds::centered(None, fallback_size, cx),
        |geometry| {
            Bounds::new(
                point(px(geometry.x), px(geometry.y)),
                size(px(geometry.width), px(geometry.height)),
            )
        },
    );
    let window_bounds = session
        .as_ref()
        .and_then(|session| session.geometry.as_ref())
        .filter(|geometry| valid_geometry(geometry) && visible_on_display(geometry, cx))
        .map(|geometry| {
            let bounds = Bounds::new(
                point(px(geometry.x), px(geometry.y)),
                size(px(geometry.width), px(geometry.height)),
            );
            if geometry.maximized {
                WindowBounds::Maximized(bounds)
            } else {
                WindowBounds::Windowed(bounds)
            }
        })
        .unwrap_or(WindowBounds::Windowed(bounds));
    let options = WindowOptions {
        window_bounds: Some(window_bounds),
        window_min_size: Some(size(px(720.), px(480.))),
        kind: WindowKind::Normal,
        ..TitleBar::window_options()
    };
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            Workspace::new(
                window,
                cx,
                id,
                settings.clone(),
                initial_paths.clone(),
                restore_session,
            )
        })
    });
    let Ok((window, workspace)) = result else {
        cx.update_global::<WindowManager, _>(|manager, _cx| {
            manager.records.retain(|record| record.id != id);
            if reserved {
                manager
                    .settings
                    .window_sessions
                    .retain(|session| session.id != id);
                if manager.next_id == id.saturating_add(1) {
                    manager.next_id = id;
                }
                if manager.settings.active_window_id == Some(id) {
                    manager.settings.active_window_id =
                        manager.records.last().map(|record| record.id);
                }
                manager.settings.save();
            }
        });
        if let Some(record) = cx
            .try_global::<WindowManager>()
            .and_then(|manager| manager.active().or_else(|| manager.records.last().cloned()))
        {
            let _ = record.workspace.update(cx, |workspace, cx| {
                workspace.note_status(
                    workspace.tr(
                        "Could not create a new window.",
                        "Kunne ikke opprette et nytt vindu.",
                    ),
                    cx,
                );
            });
        }
        return;
    };
    let initial_session = workspace.update(cx, |workspace, _cx| {
        WindowSession::from_settings(id, workspace.settings())
    });
    cx.update_global::<WindowManager, _>(|manager, _cx| {
        manager.next_id = next_available_id(
            manager.next_id,
            &manager.settings.window_sessions,
            &manager.records,
        );
        manager.register(id, window, workspace.downgrade());
        let mut session = initial_session;
        session.geometry = manager
            .session(id)
            .and_then(|saved| saved.geometry)
            .or(initial_geometry);
        update_session(&mut manager.settings, session);
        if !manager.restoring || manager.settings.active_window_id.is_none() {
            manager.settings.active_window_id = Some(id);
        }
        if manager.settings.active_window_id == Some(id) {
            sync_compatibility_facade(&mut manager.settings);
        }
        if !manager.restoring {
            manager.settings.save();
        }
    });
    cx.activate(true);
}

pub(crate) fn save_geometry(
    id: u64,
    geometry: crate::settings::WindowGeometry,
    cx: &mut Context<Workspace>,
) {
    cx.update_global::<WindowManager, _>(|manager, _cx| {
        if manager.restoring || manager.quitting {
            return;
        }
        if let Some(session) = manager
            .settings
            .window_sessions
            .iter_mut()
            .find(|session| session.id == id)
        {
            if valid_geometry(&geometry) {
                session.geometry = Some(geometry);
            }
        }
        manager.settings.save();
    });
}

fn valid_geometry(geometry: &crate::settings::WindowGeometry) -> bool {
    geometry.x.is_finite()
        && geometry.y.is_finite()
        && geometry.width.is_finite()
        && geometry.height.is_finite()
        && geometry.width >= 720.
        && geometry.height >= 480.
        && geometry.width <= 10000.
        && geometry.height <= 10000.
}

fn visible_on_display(geometry: &crate::settings::WindowGeometry, cx: &App) -> bool {
    let right = geometry.x + geometry.width;
    let bottom = geometry.y + geometry.height;
    cx.displays().iter().any(|display| {
        let bounds = display.bounds();
        let left = f32::from(bounds.origin.x);
        let top = f32::from(bounds.origin.y);
        let display_right = left + f32::from(bounds.size.width);
        let display_bottom = top + f32::from(bounds.size.height);
        geometry.x < display_right && right > left && geometry.y < display_bottom && bottom > top
    })
}

pub(crate) fn activate_window(id: u64, window: &Window, cx: &mut Context<Workspace>) {
    if !window.is_window_active() {
        return;
    }
    cx.update_global::<WindowManager, _>(|manager, _cx| {
        if manager.restoring || manager.quitting {
            return;
        }
        manager.settings.active_window_id = Some(id);
        sync_compatibility_facade(&mut manager.settings);
        manager.settings.save();
    });
}

pub(crate) fn observe_window_closed(cx: &mut App) {
    cx.on_window_closed(|cx, closed_id| {
        let settings = {
            let manager = cx.global_mut::<WindowManager>();
            let id = manager
                .records
                .iter()
                .find(|record| record.window.window_id() == closed_id)
                .map(|record| record.id);
            manager
                .records
                .retain(|record| record.window.window_id() != closed_id);
            if !manager.restoring && !manager.quitting {
                if let Some(id) = id {
                    let active_is_live = manager.settings.active_window_id.is_some_and(|active| {
                        manager.records.iter().any(|record| record.id == active)
                    });
                    if !active_is_live {
                        manager.settings.active_window_id =
                            manager.records.last().map(|record| record.id).or(Some(id));
                    }
                    let next_active = manager
                        .settings
                        .active_window_id
                        .filter(|active| manager.records.iter().any(|record| record.id == *active));
                    remove_closed_session(&mut manager.settings, id, next_active);
                    sync_compatibility_facade(&mut manager.settings);
                }
            }
            (!manager.restoring && !manager.quitting).then(|| manager.settings.clone())
        };
        if let Some(settings) = settings {
            settings.save();
        }
    })
    .detach();
}

pub(crate) fn quit(cx: &mut App) {
    let records = cx
        .try_global::<WindowManager>()
        .map(|manager| manager.records.clone())
        .unwrap_or_default();
    cx.global_mut::<WindowManager>().restoring = true;
    for record in &records {
        let result = record.window.update(cx, |_, window, cx| {
            record.workspace.update(cx, |workspace, cx| {
                let saved = workspace.save_all(cx).is_ok();
                if !saved {
                    workspace.note_status(
                        workspace.tr(
                            "Could not save before quitting.",
                            "Kunne ikke lagre før avslutning.",
                        ),
                        cx,
                    );
                    workspace.focus_workspace(window, cx);
                    window.activate_window();
                } else {
                    workspace.persist_tabs(cx);
                }
                saved
            })
        });
        if !matches!(&result, Ok(Ok(true))) {
            cx.global_mut::<WindowManager>().restoring = false;
            cx.global_mut::<WindowManager>().quitting = false;
            let _ = record
                .window
                .update(cx, |_, window, _| window.activate_window());
            return;
        }
    }

    let mut aggregate = Settings::default();
    for record in &records {
        let geometry = record.window.update(cx, |_, window, _| {
            let bounds = window.window_bounds().get_bounds();
            crate::settings::WindowGeometry {
                x: f32::from(bounds.origin.x),
                y: f32::from(bounds.origin.y),
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
                maximized: window.is_maximized(),
            }
        });
        let session = record.workspace.update(cx, |workspace, _cx| {
            WindowSession::from_settings(record.id, workspace.settings())
        });
        if let Ok(mut session) = session {
            if let Some(geometry) = geometry.ok().filter(valid_geometry) {
                session.geometry = Some(geometry);
            } else {
                session.geometry = cx
                    .global::<WindowManager>()
                    .session(record.id)
                    .and_then(|saved| saved.geometry);
            }
            update_session(&mut aggregate, session);
        }
    }
    let manager = cx.global_mut::<WindowManager>();
    replace_session_snapshot(
        &mut manager.settings,
        aggregate.window_sessions,
        !records.is_empty(),
    );
    if !records.is_empty()
        && !manager
            .settings
            .active_window_id
            .is_some_and(|id| records.iter().any(|record| record.id == id))
    {
        manager.settings.active_window_id = records.last().map(|record| record.id);
    }
    sync_compatibility_facade(&mut manager.settings);
    manager.quitting = true;
    manager.restoring = false;
    manager.settings.save();
    for record in &records {
        let _ = record.workspace.update(cx, |workspace, cx| {
            workspace.stop_agent_process(cx);
        });
    }
    cx.quit();
}

pub(crate) fn publish(
    id: u64,
    baseline: &Settings,
    current: &Settings,
    cx: &mut Context<Workspace>,
) -> Settings {
    let manager = cx.global::<WindowManager>();
    if manager.restoring || manager.quitting {
        return with_local_settings(manager.settings.clone(), current);
    }
    let (settings, shared_settings, changed) = {
        let manager = cx.global::<WindowManager>();
        let (mut merged, changed) = merge_shared_settings(baseline, current, &manager.settings);
        let mut session = WindowSession::from_settings(id, current);
        session.geometry = manager.session(id).and_then(|session| session.geometry);
        update_session(&mut merged, session);
        sync_compatibility_facade(&mut merged);
        (merged.clone(), merged, changed)
    };
    cx.update_global::<WindowManager, _>(|manager, _cx| {
        manager.settings = settings.clone();
    });
    if changed {
        cx.update_global::<SharedPreferences, _>(|shared, _cx| {
            shared.settings = shared_settings.clone();
        });
    }
    settings.save();
    settings
}

pub(crate) fn publish_checked(
    id: u64,
    baseline: &Settings,
    current: &Settings,
    cx: &mut Context<Workspace>,
) -> std::io::Result<Settings> {
    let manager = cx.global::<WindowManager>();
    if manager.restoring || manager.quitting {
        return Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "workspace lifecycle transition in progress",
        ));
    }
    let (settings, shared_settings, changed) = {
        let manager = cx.global::<WindowManager>();
        let (merged, changed) = merge_shared_settings(baseline, current, &manager.settings);
        let mut merged = merged;
        let mut session = WindowSession::from_settings(id, current);
        session.geometry = manager.session(id).and_then(|session| session.geometry);
        update_session(&mut merged, session);
        sync_compatibility_facade(&mut merged);
        (merged.clone(), merged, changed)
    };
    settings.save_checked()?;
    cx.update_global::<WindowManager, _>(|manager, _cx| {
        manager.settings = settings.clone();
    });
    if changed {
        cx.update_global::<SharedPreferences, _>(|shared, _cx| {
            shared.settings = shared_settings;
        });
    }
    Ok(with_caller_facade(settings, current))
}

fn with_caller_facade(settings: Settings, current: &Settings) -> Settings {
    let sessions = settings.window_sessions.clone();
    let active_window_id = settings.active_window_id;
    let mut local = with_local_settings(settings, current);
    local.window_sessions = sessions;
    local.active_window_id = active_window_id;
    local
}

fn next_available_id(start: u64, sessions: &[WindowSession], records: &[WindowRecord]) -> u64 {
    let used = sessions
        .iter()
        .map(|session| session.id)
        .chain(records.iter().map(|record| record.id))
        .collect::<BTreeSet<_>>();
    let mut candidate = start.max(1);
    while used.contains(&candidate) {
        candidate = candidate.wrapping_add(1).max(1);
    }
    candidate
}

fn sync_compatibility_facade(settings: &mut Settings) {
    let session = settings
        .window_sessions
        .iter()
        .find(|session| Some(session.id) == settings.active_window_id)
        .or_else(|| settings.window_sessions.last())
        .cloned();
    if let Some(session) = session {
        apply_session(settings, &session);
    }
}

fn replace_session_snapshot(
    settings: &mut Settings,
    live_sessions: Vec<WindowSession>,
    has_live_windows: bool,
) {
    if has_live_windows {
        settings.window_sessions = live_sessions;
    }
}

fn clear_fallback_session_without_live_windows(settings: &mut Settings, has_live_windows: bool) {
    if !has_live_windows {
        settings.window_sessions.clear();
        settings.active_window_id = None;
    }
}

pub(crate) fn normalize_sessions(settings: &mut Settings) {
    if settings.window_sessions.is_empty() {
        settings.window_sessions = vec![WindowSession::from_settings(1, settings)];
        settings.active_window_id = Some(1);
        return;
    }

    let mut used = BTreeSet::new();
    let mut next = 1_u64;
    settings.window_sessions.truncate(MAX_RESTORED_WINDOWS);
    for session in &mut settings.window_sessions {
        if session.id == 0 || !used.insert(session.id) {
            while used.contains(&next) {
                next = next.saturating_add(1);
            }
            session.id = next;
            used.insert(next);
            next = next.saturating_add(1);
        }
    }
    if !settings
        .active_window_id
        .is_some_and(|id| used.contains(&id))
    {
        settings.active_window_id = settings.window_sessions.last().map(|session| session.id);
    }
}

pub(crate) fn merge_shared_settings(
    baseline: &Settings,
    updated: &Settings,
    authoritative: &Settings,
) -> (Settings, bool) {
    let Ok(baseline) = serde_json::to_value(baseline) else {
        return (authoritative.clone(), false);
    };
    let Ok(updated) = serde_json::to_value(updated) else {
        return (authoritative.clone(), false);
    };
    let Ok(mut authoritative_value) = serde_json::to_value(authoritative) else {
        return (authoritative.clone(), false);
    };
    let changed = merge_object(&baseline, &updated, &mut authoritative_value, true);
    if !changed {
        return (authoritative.clone(), false);
    }
    (
        serde_json::from_value(authoritative_value).unwrap_or_else(|_| authoritative.clone()),
        true,
    )
}

pub(crate) fn with_local_settings(mut settings: Settings, local: &Settings) -> Settings {
    let (Ok(mut settings_value), Ok(local_value)) =
        (serde_json::to_value(&settings), serde_json::to_value(local))
    else {
        return settings;
    };
    let (Some(settings_object), Some(local_object)) =
        (settings_value.as_object_mut(), local_value.as_object())
    else {
        return settings;
    };
    for field in LOCAL_FIELDS {
        if let Some(value) = local_object.get(*field) {
            settings_object.insert((*field).to_string(), value.clone());
        }
    }
    serde_json::from_value(settings_value).unwrap_or_else(|_| {
        settings.window_sessions = local.window_sessions.clone();
        settings.active_window_id = local.active_window_id;
        settings.last_vault = local.last_vault.clone();
        settings.active_folder = local.active_folder.clone();
        settings.open_tabs = local.open_tabs.clone();
        settings.active_tab = local.active_tab.clone();
        settings.editor_session = local.editor_session.clone();
        settings.view_mode = local.view_mode;
        settings.pinned_tabs = local.pinned_tabs.clone();
        settings.sidebar_collapsed = local.sidebar_collapsed;
        settings.inspector_open = local.inspector_open;
        settings.graph_dock_open = local.graph_dock_open;
        settings.agent_open = local.agent_open;
        settings.panes = local.panes.clone();
        settings.expanded_folders = local.expanded_folders.clone();
        settings.sidebar_width = local.sidebar_width;
        settings.dock_width = local.dock_width;
        settings.dock_heights = local.dock_heights.clone();
        settings
    })
}

fn remove_closed_session(settings: &mut Settings, closed_id: u64, next_active: Option<u64>) {
    if let Some(next_active) = next_active {
        settings
            .window_sessions
            .retain(|session| session.id != closed_id);
        if settings.active_window_id == Some(closed_id) {
            settings.active_window_id = Some(next_active);
        }
    }
}

fn merge_object(baseline: &Value, updated: &Value, target: &mut Value, top_level: bool) -> bool {
    if !baseline.is_object() || !updated.is_object() || !target.is_object() {
        if baseline != updated {
            *target = updated.clone();
            return true;
        }
        return false;
    }
    let (Some(baseline), Some(updated), Some(target)) = (
        baseline.as_object(),
        updated.as_object(),
        target.as_object_mut(),
    ) else {
        return false;
    };

    let mut changed = false;
    for (key, updated_value) in updated {
        if top_level && LOCAL_FIELDS.contains(&key.as_str()) {
            continue;
        }
        let Some(baseline_value) = baseline.get(key) else {
            if target.get(key) != Some(updated_value) {
                target.insert(key.clone(), updated_value.clone());
                changed = true;
            }
            continue;
        };
        if baseline_value == updated_value {
            continue;
        }
        if let Some(target_value) = target.get_mut(key) {
            changed |= merge_object(baseline_value, updated_value, target_value, false);
        } else {
            target.insert(key.clone(), updated_value.clone());
            changed = true;
        }
    }
    for key in baseline.keys() {
        if updated.contains_key(key) || (top_level && LOCAL_FIELDS.contains(&key.as_str())) {
            continue;
        }
        if target.get(key) == baseline.get(key) && target.remove(key).is_some() {
            changed = true;
        }
    }
    changed
}

pub(crate) fn apply_session(settings: &mut Settings, session: &WindowSession) {
    settings.dock_width = session
        .dock_width
        .filter(|width| crate::settings::valid_dock_width(*width));
    settings.dock_heights = session.dock_heights.clone();
    settings.last_vault = session.last_vault.clone();
    settings.active_folder = session.active_folder.clone();
    settings.open_tabs = session.open_tabs.clone();
    settings.active_tab = session.active_tab.clone();
    settings.editor_session = session.editor_session.clone();
    settings.view_mode = session.view_mode;
    settings.pinned_tabs = session.pinned_tabs.clone();
    settings.sidebar_collapsed = session.sidebar_collapsed;
    settings.inspector_open = session.inspector_open;
    settings.graph_dock_open = session.graph_dock_open;
    settings.agent_open = session.agent_open;
    settings.panes = session.panes.clone();
    settings.expanded_folders = session.expanded_folders.clone();
    settings.sidebar_width = session
        .sidebar_width
        .filter(|width| crate::settings::valid_sidebar_width(*width));
}

pub(crate) fn update_session(settings: &mut Settings, session: WindowSession) {
    if let Some(existing) = settings
        .window_sessions
        .iter_mut()
        .find(|existing| existing.id == session.id)
    {
        *existing = session;
    } else {
        settings.window_sessions.push(session);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_fallback_session_without_live_windows, merge_shared_settings, next_available_id,
        normalize_sessions, prepare_explicit_startup, prepare_new_window_session,
        remove_closed_session, replace_session_snapshot, sync_compatibility_facade, update_session,
        with_local_settings,
    };
    use crate::settings::{Appearance, Settings, TreeSort, ViewMode, WindowSession};
    use std::{
        collections::{BTreeSet, HashMap},
        path::PathBuf,
    };

    #[test]
    fn old_flat_settings_migrate_to_one_window() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"open_tabs":["/tmp/a.md"],"view_mode":"preview"}"#).unwrap();
        normalize_sessions(&mut settings);
        assert_eq!(settings.window_sessions.len(), 1);
        assert_eq!(settings.window_sessions[0].open_tabs, vec!["/tmp/a.md"]);
        assert_eq!(settings.window_sessions[0].view_mode, ViewMode::Preview);
        assert_eq!(settings.active_window_id, Some(1));
    }

    #[test]
    fn two_window_sessions_roundtrip_without_cross_contamination() {
        let mut settings = Settings::default();
        settings.window_sessions = vec![
            WindowSession::from_settings(1, &settings),
            WindowSession::from_settings(2, &settings),
        ];
        settings.window_sessions[0].last_vault = Some(PathBuf::from("/vault/a"));
        settings.window_sessions[1].last_vault = Some(PathBuf::from("/vault/b"));
        settings.window_sessions[0].sidebar_width = Some(320.);
        settings.window_sessions[1].sidebar_width = Some(410.);
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            restored.window_sessions[0].last_vault,
            Some(PathBuf::from("/vault/a"))
        );
        assert_eq!(
            restored.window_sessions[1].last_vault,
            Some(PathBuf::from("/vault/b"))
        );
        assert_eq!(restored.window_sessions[0].sidebar_width, Some(320.));
        assert_eq!(restored.window_sessions[1].sidebar_width, Some(410.));
    }

    #[test]
    fn stale_publishers_merge_distinct_fields_and_map_keys() {
        let baseline = Settings::default();
        let mut authoritative = baseline.clone();
        authoritative.appearance = Appearance::Light;
        authoritative
            .base_views
            .insert("/vault/other.base".into(), 2);
        let mut first = baseline.clone();
        first.language = crate::settings::Language::Norwegian;
        first.base_views.insert("/vault/first.base".into(), 1);
        let (authoritative, changed) = merge_shared_settings(&baseline, &first, &authoritative);
        assert!(changed);
        assert_eq!(authoritative.appearance, Appearance::Light);
        assert_eq!(authoritative.language, crate::settings::Language::Norwegian);
        assert_eq!(authoritative.base_views.get("/vault/other.base"), Some(&2));
        assert_eq!(authoritative.base_views.get("/vault/first.base"), Some(&1));
        let mut second = baseline.clone();
        second.tree_sort = TreeSort::Modified;
        second.base_views.insert("/vault/second.base".into(), 3);
        let (authoritative, changed) = merge_shared_settings(&baseline, &second, &authoritative);
        assert!(changed);
        assert_eq!(authoritative.language, crate::settings::Language::Norwegian);
        assert_eq!(authoritative.tree_sort, TreeSort::Modified);
        assert_eq!(authoritative.base_views.get("/vault/first.base"), Some(&1));
        assert_eq!(authoritative.base_views.get("/vault/second.base"), Some(&3));
    }

    #[test]
    fn publisher_does_not_acknowledge_shared_values_before_consuming_them() {
        let baseline = Settings::default();
        let mut first = baseline.clone();
        first.language = crate::settings::Language::Norwegian;
        let (authoritative, changed) = merge_shared_settings(&baseline, &first, &baseline);
        assert!(changed);

        let mut second = baseline.clone();
        second.tree_sort = TreeSort::Modified;
        let (authoritative, changed) = merge_shared_settings(&baseline, &second, &authoritative);
        assert!(changed);

        let second_baseline = second.clone();
        let (second_after_update, changed) =
            merge_shared_settings(&second_baseline, &authoritative, &second);
        assert!(changed);
        assert_eq!(
            second_after_update.language,
            crate::settings::Language::Norwegian
        );
        assert_eq!(second_after_update.tree_sort, TreeSort::Modified);
    }

    #[test]
    fn stale_map_updates_preserve_concurrent_deletions() {
        let mut baseline = Settings::default();
        baseline.base_views.insert("/vault/removed.base".into(), 1);
        let mut authoritative = baseline.clone();
        authoritative.base_views.remove("/vault/removed.base");

        let mut stale = baseline.clone();
        stale.base_views.insert("/vault/added.base".into(), 2);
        let (merged, changed) = merge_shared_settings(&baseline, &stale, &authoritative);

        assert!(changed);
        assert!(!merged.base_views.contains_key("/vault/removed.base"));
        assert_eq!(merged.base_views.get("/vault/added.base"), Some(&2));
    }

    #[test]
    fn local_session_fields_never_merge_into_shared_settings() {
        let baseline = Settings::default();
        let authoritative = baseline.clone();
        let mut updated = baseline.clone();
        updated.open_tabs = vec!["/tmp/note.md".into()];
        updated.active_folder = Some(PathBuf::from("/vault/subfolder"));
        updated.sidebar_width = Some(345.);
        updated.expanded_folders = HashMap::from([("/vault".into(), vec!["/vault/a".into()])]);
        let (merged, changed) = merge_shared_settings(&baseline, &updated, &authoritative);
        assert!(!changed);
        assert!(merged.open_tabs.is_empty());
        assert_eq!(merged.active_folder, None);
        assert_eq!(merged.sidebar_width, None);
        assert!(merged.expanded_folders.is_empty());
        let local = with_local_settings(merged, &updated);
        assert_eq!(local.open_tabs, vec!["/tmp/note.md"]);
        assert_eq!(local.active_folder, Some(PathBuf::from("/vault/subfolder")));
        assert_eq!(local.sidebar_width, Some(345.));
        assert_eq!(
            local.expanded_folders,
            HashMap::from([("/vault".into(), vec!["/vault/a".into()])])
        );
    }

    #[test]
    fn explicit_startup_replaces_saved_windows_and_preserves_view_mode() {
        let mut settings = Settings::default();
        settings.window_sessions = vec![WindowSession::from_settings(7, &settings)];
        settings.window_sessions[0].open_tabs = vec!["/saved.md".into()];
        settings.active_window_id = Some(7);
        settings.open_tabs = vec!["/saved.md".into()];
        settings.last_vault = Some(PathBuf::from("/saved-vault"));
        settings.view_mode = ViewMode::Preview;
        settings.pinned_tabs = vec!["/saved.md".into()];

        prepare_explicit_startup(&mut settings);

        assert!(settings.window_sessions.is_empty());
        assert!(settings.open_tabs.is_empty());
        assert!(settings.last_vault.is_none());
        assert_eq!(settings.view_mode, ViewMode::Preview);
        assert!(settings.pinned_tabs.is_empty());
        assert!(settings.active_window_id.is_none());

        normalize_sessions(&mut settings);
        assert_eq!(settings.window_sessions.len(), 1);
        assert_ne!(settings.window_sessions[0].id, 7);
        assert!(settings.window_sessions[0].open_tabs.is_empty());
        assert_eq!(settings.window_sessions[0].view_mode, ViewMode::Preview);
    }

    #[test]
    fn closing_one_session_removes_it_but_final_close_keeps_fallback() {
        let mut settings = Settings::default();
        settings.window_sessions = vec![
            WindowSession::from_settings(1, &settings),
            WindowSession::from_settings(2, &settings),
        ];
        settings.window_sessions[0].open_tabs = vec!["/one.md".into()];
        settings.window_sessions[1].open_tabs = vec!["/two.md".into()];
        settings.active_window_id = Some(2);

        remove_closed_session(&mut settings, 2, Some(1));
        assert_eq!(
            settings
                .window_sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(settings.active_window_id, Some(1));

        remove_closed_session(&mut settings, 1, None);
        assert_eq!(settings.window_sessions.len(), 1);
        assert_eq!(settings.window_sessions[0].open_tabs, vec!["/one.md"]);
        assert_eq!(settings.active_window_id, Some(1));

        clear_fallback_session_without_live_windows(&mut settings, false);
        assert!(settings.window_sessions.is_empty());
        assert_eq!(settings.active_window_id, None);

        update_session(
            &mut settings,
            WindowSession::from_settings(3, &Settings::default()),
        );
        settings.active_window_id = Some(3);
        sync_compatibility_facade(&mut settings);
        assert_eq!(
            settings
                .window_sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert!(settings.open_tabs.is_empty());
    }

    #[test]
    fn quit_snapshot_replaces_saved_windows_but_keeps_no_live_fallback() {
        let mut settings = Settings::default();
        settings.window_sessions = vec![WindowSession::from_settings(1, &settings)];
        settings.window_sessions[0].open_tabs = vec!["/stale.md".into()];
        let live = vec![
            WindowSession::from_settings(2, &Settings::default()),
            WindowSession::from_settings(3, &Settings::default()),
        ];

        replace_session_snapshot(&mut settings, live, true);
        assert_eq!(
            settings
                .window_sessions
                .iter()
                .map(|session| session.id)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );

        let mut fallback = Settings::default();
        fallback.window_sessions = vec![WindowSession::from_settings(4, &fallback)];
        replace_session_snapshot(&mut fallback, Vec::new(), false);
        assert_eq!(fallback.window_sessions.len(), 1);
        assert_eq!(fallback.window_sessions[0].id, 4);
    }

    #[test]
    fn compatibility_facade_tracks_only_the_selected_window() {
        let mut first = WindowSession::from_settings(1, &Settings::default());
        first.last_vault = Some(PathBuf::from("/vault/first"));
        first.open_tabs = vec!["/vault/first.md".into()];
        first.view_mode = ViewMode::Source;
        let mut second = WindowSession::from_settings(2, &Settings::default());
        second.last_vault = Some(PathBuf::from("/vault/second"));
        second.open_tabs = vec!["/vault/second.md".into()];
        second.view_mode = ViewMode::Preview;
        let mut settings = Settings::default();
        settings.window_sessions = vec![first, second];
        settings.active_window_id = Some(1);

        sync_compatibility_facade(&mut settings);
        assert_eq!(settings.last_vault, Some(PathBuf::from("/vault/first")));
        assert_eq!(settings.open_tabs, vec!["/vault/first.md"]);

        settings.window_sessions[1].last_vault = Some(PathBuf::from("/vault/background"));
        sync_compatibility_facade(&mut settings);
        assert_eq!(settings.last_vault, Some(PathBuf::from("/vault/first")));
        assert_eq!(settings.active_window_id, Some(1));

        settings.active_window_id = Some(2);
        sync_compatibility_facade(&mut settings);
        assert_eq!(
            settings.last_vault,
            Some(PathBuf::from("/vault/background"))
        );
        assert_eq!(settings.open_tabs, vec!["/vault/second.md"]);
        assert_eq!(settings.view_mode, ViewMode::Preview);
    }

    #[test]
    fn normalized_and_allocated_window_ids_are_unique_at_u64_max() {
        let mut settings = Settings::default();
        let mut maximum = WindowSession::from_settings(1, &settings);
        maximum.id = u64::MAX;
        let mut duplicate = WindowSession::from_settings(2, &settings);
        duplicate.id = u64::MAX;
        let mut zero = WindowSession::from_settings(3, &settings);
        zero.id = 0;
        settings.window_sessions = vec![maximum, duplicate, zero];

        normalize_sessions(&mut settings);

        let ids = settings
            .window_sessions
            .iter()
            .map(|session| session.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 3);
        assert!(!ids.contains(&0));
        assert!(ids.contains(&u64::MAX));
        let allocated = next_available_id(u64::MAX, &settings.window_sessions, &[]);
        assert_ne!(allocated, 0);
        assert!(!ids.contains(&allocated));
    }

    #[test]
    fn new_window_inherits_vault_but_never_inherits_tabs_or_layout() {
        let mut settings = Settings::default();
        settings.last_vault = Some(PathBuf::from("/vault"));
        settings.active_folder = Some(PathBuf::from("/vault/folder"));
        settings.open_tabs = vec!["/vault/old.md".into()];
        settings.active_tab = Some("/vault/old.md".into());
        settings.pinned_tabs = vec!["/vault/old.md".into()];
        settings.expanded_folders =
            HashMap::from([("/vault".into(), vec!["/vault/folder".into()])]);
        let source = WindowSession::from_settings(1, &settings);

        let empty = prepare_new_window_session(source.clone(), &[]);
        assert_eq!(empty.last_vault, Some(PathBuf::from("/vault")));
        assert!(empty.open_tabs.is_empty());
        assert!(empty.active_tab.is_none());
        assert!(empty.editor_session.is_none());
        assert!(empty.pinned_tabs.is_empty());

        let same_vault =
            prepare_new_window_session(source.clone(), &[PathBuf::from("/vault/new.md")]);
        assert_eq!(same_vault.last_vault, Some(PathBuf::from("/vault")));

        let outside = prepare_new_window_session(source, &[PathBuf::from("/other/new.md")]);
        assert!(outside.last_vault.is_none());
        assert!(outside.active_folder.is_none());
        assert!(outside.expanded_folders.is_empty());
    }

    #[test]
    fn publication_suppression_preserves_each_workspace_local_snapshot() {
        let baseline = Settings::default();
        let mut authoritative = baseline.clone();
        authoritative.language = crate::settings::Language::Norwegian;
        let mut current = baseline.clone();
        current.open_tabs = vec!["/workspace.md".into()];
        current.view_mode = ViewMode::Preview;
        let restored = with_local_settings(authoritative.clone(), &current);

        assert_eq!(restored.language, crate::settings::Language::Norwegian);
        assert_eq!(restored.open_tabs, vec!["/workspace.md"]);
        assert_eq!(restored.view_mode, ViewMode::Preview);
        assert!(authoritative.open_tabs.is_empty());
    }

    #[test]
    fn checked_publication_result_keeps_caller_local_state_and_shared_sessions() {
        let mut window_a = Settings::default();
        window_a.last_vault = Some(PathBuf::from("/vault/a"));
        window_a.open_tabs = vec!["/vault/a/note.md".into()];
        window_a.active_tab = Some("/vault/a/note.md".into());
        let mut window_b = Settings::default();
        window_b.last_vault = Some(PathBuf::from("/vault/b"));
        window_b.open_tabs = vec!["/vault/b/note.md".into()];
        window_b.active_tab = Some("/vault/b/note.md".into());
        window_b.view_mode = ViewMode::Preview;

        let mut persisted = Settings::default();
        persisted.window_sessions = vec![
            WindowSession::from_settings(1, &window_a),
            WindowSession::from_settings(2, &window_b),
        ];
        persisted.active_window_id = Some(1);
        sync_compatibility_facade(&mut persisted);
        let persisted_sessions = persisted.window_sessions.clone();
        let mut current_window_b = window_b.clone();
        current_window_b.window_sessions = vec![persisted_sessions[1].clone()];
        current_window_b.active_window_id = Some(2);

        let caller_settings = super::with_caller_facade(persisted.clone(), &current_window_b);
        assert_eq!(caller_settings.last_vault, Some(PathBuf::from("/vault/b")));
        assert_eq!(caller_settings.open_tabs, vec!["/vault/b/note.md"]);
        assert_eq!(
            caller_settings.active_tab.as_deref(),
            Some("/vault/b/note.md")
        );
        assert_eq!(caller_settings.view_mode, ViewMode::Preview);
        assert_eq!(caller_settings.window_sessions.len(), 2);
        assert_eq!(
            caller_settings.window_sessions[0].last_vault,
            Some(PathBuf::from("/vault/a"))
        );
        assert_eq!(
            caller_settings.window_sessions[1].last_vault,
            Some(PathBuf::from("/vault/b"))
        );
        assert_eq!(persisted.last_vault, Some(PathBuf::from("/vault/a")));
        assert_eq!(persisted.open_tabs, vec!["/vault/a/note.md"]);
        assert_eq!(persisted.window_sessions.len(), 2);
        assert_eq!(
            persisted.window_sessions[0].last_vault,
            Some(PathBuf::from("/vault/a"))
        );
        assert_eq!(
            persisted.window_sessions[1].last_vault,
            Some(PathBuf::from("/vault/b"))
        );
    }
}
