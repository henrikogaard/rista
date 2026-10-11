use crate::actions::*;
use crate::settings::{self, Language};
use gpui_kit::component::input;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

type ActionFactory = fn() -> Box<dyn Action>;

#[derive(Clone, Copy)]
enum CommandTarget {
    Action(ActionFactory),
    Palette,
}

#[derive(Clone, Copy)]
pub(crate) struct CommandSpec {
    pub(crate) id: &'static str,
    pub(crate) english: &'static str,
    pub(crate) norwegian: &'static str,
    pub(crate) default: Option<&'static str>,
    context: Option<&'static str>,
    target: CommandTarget,
}

impl CommandSpec {
    pub(crate) fn label(self, language: Language) -> &'static str {
        language.text(self.english, self.norwegian)
    }
}

macro_rules! action_command {
    ($id:literal, $english:literal, $norwegian:literal, $default:expr, $context:expr, $action:path) => {
        CommandSpec {
            id: $id,
            english: $english,
            norwegian: $norwegian,
            default: $default,
            context: $context,
            target: CommandTarget::Action(|| Box::new($action)),
        }
    };
}

const ACTION_COMMANDS: &[CommandSpec] = &[
    action_command!(
        "new_file",
        "New file",
        "Ny fil",
        Some("cmd-n"),
        None,
        NewFile
    ),
    action_command!(
        "new_folder",
        "New folder",
        "Ny mappe",
        Some("cmd-shift-n"),
        None,
        NewFolder
    ),
    action_command!(
        "open_file",
        "Open file",
        "Åpne fil",
        Some("cmd-o"),
        None,
        OpenFile
    ),
    action_command!(
        "open_folder",
        "Open folder",
        "Åpne mappe",
        Some("cmd-shift-o"),
        None,
        OpenFolder
    ),
    action_command!(
        "open_daily_note",
        "Open daily note",
        "Åpne dagsnotat",
        Some("cmd-shift-d"),
        None,
        OpenDailyNote
    ),
    action_command!(
        "close_folder",
        "Close folder",
        "Lukk mappe",
        None,
        None,
        CloseFolder
    ),
    action_command!("save", "Save", "Lagre", Some("cmd-s"), None, SaveFile),
    action_command!(
        "save_as",
        "Save as",
        "Lagre som",
        Some("cmd-shift-s"),
        None,
        SaveFileAs
    ),
    action_command!(
        "close_tab",
        "Close tab",
        "Lukk fane",
        Some("cmd-w"),
        None,
        CloseTab
    ),
    action_command!(
        "reopen_tab",
        "Reopen closed tab",
        "Gjenåpne lukket fane",
        Some("cmd-shift-t"),
        None,
        ReopenTab
    ),
    action_command!(
        "move_line_up",
        "Move line up",
        "Flytt linje opp",
        Some("alt-up"),
        None,
        MoveLineUp
    ),
    action_command!(
        "move_line_down",
        "Move line down",
        "Flytt linje ned",
        Some("alt-down"),
        None,
        MoveLineDown
    ),
    action_command!(
        "toggle_checkbox",
        "Toggle checkbox",
        "Veksle avkryssing",
        Some("cmd-enter"),
        None,
        ToggleCheckbox
    ),
    action_command!(
        "toggle_italic",
        "Toggle italic",
        "Veksle kursiv",
        Some("cmd-i"),
        None,
        ToggleItalic
    ),
    action_command!(
        "duplicate_block",
        "Duplicate block",
        "Dupliser blokk",
        Some("cmd-d"),
        None,
        DuplicateBlock
    ),
    action_command!(
        "delete_line",
        "Delete line",
        "Slett linje",
        Some("cmd-shift-k"),
        None,
        DeleteLine
    ),
    action_command!(
        "toggle_comment",
        "Toggle comment",
        "Veksle kommentar",
        Some("cmd-/"),
        None,
        ToggleComment
    ),
    action_command!(
        "copy_block_link",
        "Copy link to block",
        "Kopier blokklenke",
        None,
        None,
        CopyBlockLink
    ),
    action_command!(
        "copy_block_embed",
        "Copy block embed",
        "Kopier blokkinnbygging",
        None,
        None,
        CopyBlockEmbed
    ),
    action_command!(
        "next_tab",
        "Next tab",
        "Neste fane",
        Some("ctrl-tab"),
        None,
        NextTab
    ),
    action_command!(
        "previous_tab",
        "Previous tab",
        "Forrige fane",
        Some("ctrl-shift-tab"),
        None,
        PrevTab
    ),
    action_command!(
        "search_next_result",
        "Next search result",
        "Neste søkeresultat",
        Some("down"),
        Some("ProjectSearch"),
        SearchNextResult
    ),
    action_command!(
        "search_previous_result",
        "Previous search result",
        "Forrige søkeresultat",
        Some("up"),
        Some("ProjectSearch"),
        SearchPreviousResult
    ),
    action_command!(
        "navigate_back",
        "Navigate back",
        "Gå tilbake",
        Some("cmd-["),
        None,
        NavigateBack
    ),
    action_command!(
        "navigate_forward",
        "Navigate forward",
        "Gå frem",
        Some("cmd-]"),
        None,
        NavigateForward
    ),
    action_command!(
        "follow_link",
        "Open link under cursor",
        "Åpne lenke under markøren",
        Some("alt-enter"),
        None,
        FollowLink
    ),
    action_command!(
        "toggle_sidebar",
        "Toggle sidebar",
        "Veksle sidepanel",
        Some("cmd-b"),
        None,
        ToggleSidebar
    ),
    action_command!(
        "toggle_zen",
        "Toggle focus mode",
        "Veksle fokusmodus",
        Some("cmd-shift-enter"),
        None,
        ToggleZen
    ),
    action_command!(
        "view_source",
        "Source mode",
        "Kildemodus",
        Some("cmd-1"),
        None,
        ViewSource
    ),
    action_command!(
        "view_split",
        "Split mode",
        "Delt visning",
        Some("cmd-2"),
        None,
        ViewSplit
    ),
    action_command!(
        "view_preview",
        "Preview mode",
        "Forhåndsvisning",
        Some("cmd-3"),
        None,
        ViewPreview
    ),
    action_command!(
        "toggle_edit_preview",
        "Toggle edit preview",
        "Veksle redigeringsforhåndsvisning",
        Some("cmd-e"),
        None,
        ToggleEditPreview
    ),
    action_command!(
        "zoom_in",
        "Zoom in",
        "Zoom inn",
        Some("cmd-="),
        None,
        ZoomIn
    ),
    action_command!(
        "zoom_out",
        "Zoom out",
        "Zoom ut",
        Some("cmd-minus"),
        None,
        ZoomOut
    ),
    action_command!(
        "zoom_reset",
        "Reset zoom",
        "Tilbakestill zoom",
        Some("cmd-0"),
        None,
        ZoomReset
    ),
    action_command!(
        "open_graph",
        "Open graph",
        "Åpne graf",
        Some("cmd-g"),
        None,
        OpenGraph
    ),
    action_command!(
        "open_local_graph",
        "Open local graph",
        "Åpne lokal graf",
        None,
        None,
        OpenLocalGraph
    ),
    action_command!(
        "close_graph",
        "Close graph",
        "Lukk graf",
        Some("escape"),
        Some("RistaGraph"),
        CloseGraph
    ),
    action_command!(
        "toggle_local_graph_panel",
        "Toggle local graph panel",
        "Veksle lokalt grafpanel",
        None,
        None,
        ToggleLocalGraphPanel
    ),
    action_command!(
        "toggle_agent_panel",
        "Toggle agent panel",
        "Veksle agentpanel",
        None,
        None,
        ToggleAgentPanel
    ),
    action_command!(
        "open_command_palette",
        "Open command palette",
        "Åpne kommandopalett",
        Some("cmd-k"),
        None,
        OpenCommandPalette
    ),
    action_command!(
        "quick_open",
        "Quick open",
        "Åpne raskt",
        Some("cmd-p"),
        None,
        QuickOpen
    ),
    action_command!(
        "open_project_search",
        "Search project",
        "Søk i prosjektet",
        Some("cmd-shift-f"),
        None,
        OpenProjectSearch
    ),
    action_command!(
        "open_settings",
        "Open settings",
        "Åpne innstillinger",
        Some("cmd-,"),
        None,
        OpenSettings
    ),
    action_command!(
        "toggle_terminal",
        "Toggle terminal",
        "Veksle terminal",
        Some("cmd-j"),
        None,
        ToggleTerminal
    ),
    action_command!(
        "open_tools",
        "Open tools",
        "Åpne verktøy",
        None,
        None,
        OpenTools
    ),
    action_command!(
        "toggle_theme",
        "Toggle theme",
        "Veksle tema",
        None,
        None,
        ToggleTheme
    ),
    action_command!(
        "check_for_updates",
        "Check for updates",
        "Se etter oppdateringer",
        None,
        None,
        CheckForUpdates
    ),
    action_command!("quit", "Quit", "Avslutt", Some("cmd-q"), None, Quit),
    action_command!("about", "About", "Om", None, None, About),
    action_command!(
        "find_in_note",
        "Find in note",
        "Finn i notat",
        Some("cmd-f"),
        None,
        input::Search
    ),
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeymapConfig {
    bindings: BTreeMap<String, Option<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapFile {
    version: u32,
    bindings: BTreeMap<String, Option<String>>,
}

#[derive(Serialize)]
struct KeymapFileRef<'a> {
    version: u32,
    bindings: &'a BTreeMap<String, Option<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HotkeyNotice {
    InvalidFile,
    UnsupportedVersion(u32),
    UnknownCommand(String),
    InvalidShortcut(String),
    Conflict(String),
    SaveFailed,
}

impl HotkeyNotice {
    pub(crate) fn message(&self, language: Language) -> String {
        match self {
            Self::InvalidFile => language
                .text(
                    "Could not read keymap.json; default shortcuts are active.",
                    "Kunne ikke lese keymap.json; standardsnarveiene er aktive.",
                )
                .to_string(),
            Self::UnsupportedVersion(version) => format!(
                "{} {version}",
                language.text(
                    "Unsupported keymap.json version:",
                    "Ustøttet versjon av keymap.json:",
                )
            ),
            Self::UnknownCommand(id) => format!(
                "{} {id}",
                language.text(
                    "Unknown command in keymap.json:",
                    "Ukjent handling i keymap.json:",
                )
            ),
            Self::InvalidShortcut(id) => format!(
                "{} {id}",
                language.text(
                    "Invalid shortcut for command:",
                    "Ugyldig snarvei for handling:",
                )
            ),
            Self::Conflict(id) => format!(
                "{} {id}",
                language.text(
                    "Shortcut conflicts with another command or editor key:",
                    "Snarveien kolliderer med en annen handling eller editorsnarvei:",
                )
            ),
            Self::SaveFailed => language
                .text(
                    "Could not save keymap.json; active shortcuts were not changed.",
                    "Kunne ikke lagre keymap.json; aktive snarveier ble ikke endret.",
                )
                .to_string(),
        }
    }

    pub(crate) fn startup_message(&self, language: Language) -> String {
        if matches!(self, Self::InvalidFile | Self::SaveFailed) {
            return self.message(language);
        }
        format!(
            "{} {}",
            self.message(language),
            language.text(
                "Default shortcuts are active.",
                "Standardsnarveiene er aktive.",
            )
        )
    }
}

#[derive(Clone)]
struct HotkeyRuntime {
    framework: Vec<KeyBinding>,
    defaults: Vec<KeyBinding>,
    internal: Vec<KeyBinding>,
    config: KeymapConfig,
    warning: Option<HotkeyNotice>,
}

impl Global for HotkeyRuntime {}

fn palette_spec(id: &str) -> Option<CommandSpec> {
    crate::app::PaletteCmd::ALL
        .iter()
        .find(|command| command.id() == id)
        .map(|command| CommandSpec {
            id: command.id(),
            english: command.label(),
            norwegian: command.label_norwegian(),
            default: None,
            context: None,
            target: CommandTarget::Palette,
        })
}

pub(crate) fn command(id: &str) -> Option<CommandSpec> {
    ACTION_COMMANDS
        .iter()
        .copied()
        .find(|command| command.id == id)
        .or_else(|| palette_spec(id))
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    let mut commands = ACTION_COMMANDS.to_vec();
    for palette_command in crate::app::PaletteCmd::ALL {
        if !ACTION_COMMANDS
            .iter()
            .any(|command| command.id == palette_command.id())
        {
            if let Some(spec) = palette_spec(palette_command.id()) {
                commands.push(spec);
            }
        }
    }
    commands
}

fn action_for(spec: CommandSpec) -> Box<dyn Action> {
    match spec.target {
        CommandTarget::Action(factory) => factory(),
        CommandTarget::Palette => Box::new(RunPaletteCommand {
            id: spec.id.to_string(),
        }),
    }
}

fn binding_for(
    spec: CommandSpec,
    shortcut: &str,
    mapper: &dyn PlatformKeyboardMapper,
) -> Result<KeyBinding, HotkeyNotice> {
    if shortcut.trim().is_empty() {
        return Err(HotkeyNotice::InvalidShortcut(spec.id.to_string()));
    }
    let context = spec
        .context
        .map(|context| KeyBindingContextPredicate::parse(context).map(Rc::new))
        .transpose()
        .map_err(|_| HotkeyNotice::InvalidShortcut(spec.id.to_string()))?;
    KeyBinding::load(shortcut, action_for(spec), context, false, None, mapper)
        .map_err(|_| HotkeyNotice::InvalidShortcut(spec.id.to_string()))
}

fn parse_keystrokes(shortcut: &str) -> Result<Vec<Keystroke>, HotkeyNotice> {
    let strokes = shortcut
        .split_whitespace()
        .map(|stroke| {
            Keystroke::parse(stroke).map_err(|_| HotkeyNotice::InvalidShortcut(String::new()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if strokes.is_empty() {
        return Err(HotkeyNotice::InvalidShortcut(String::new()));
    }
    Ok(strokes)
}

fn sequences_conflict(first: &[Keystroke], second: &[Keystroke]) -> bool {
    let shared_len = first.len().min(second.len());
    shared_len > 0 && first[..shared_len] == second[..shared_len]
}

fn binding_sequence(binding: &KeyBinding) -> Vec<Keystroke> {
    binding
        .keystrokes()
        .iter()
        .map(|stroke| stroke.inner().clone())
        .collect()
}

fn binding_is_overridden(binding: &KeyBinding, config: &KeymapConfig) -> bool {
    config
        .bindings
        .keys()
        .any(|id| command(id).is_some_and(|spec| action_for(spec).partial_eq(binding.action())))
}

fn is_custom_assignment(spec: CommandSpec, config: &KeymapConfig) -> bool {
    matches!(
        config.bindings.get(spec.id),
        Some(Some(shortcut)) if Some(shortcut.as_str()) != spec.default
    )
}

fn validate_conflicts(
    framework: &[KeyBinding],
    defaults: &[KeyBinding],
    internal: &[KeyBinding],
    config: &KeymapConfig,
    mapper: &dyn PlatformKeyboardMapper,
) -> Result<(), HotkeyNotice> {
    let all_commands = commands();
    for command in all_commands.iter().copied() {
        let Some(custom) = config.bindings.get(command.id) else {
            continue;
        };
        let Some(custom) = custom.as_deref() else {
            continue;
        };
        if !is_custom_assignment(command, config) {
            continue;
        }
        let candidate = parse_keystrokes(custom)
            .map_err(|_| HotkeyNotice::InvalidShortcut(command.id.to_string()))?;
        for other in all_commands
            .iter()
            .copied()
            .filter(|other| other.id != command.id)
        {
            let other_shortcut = config
                .bindings
                .get(other.id)
                .cloned()
                .unwrap_or_else(|| other.default.map(str::to_string));
            let Some(other_shortcut) = other_shortcut else {
                continue;
            };
            if sequences_conflict(&candidate, &parse_keystrokes(&other_shortcut)?) {
                return Err(HotkeyNotice::Conflict(command.id.to_string()));
            }
        }

        let mut retained = framework
            .iter()
            .chain(defaults.iter())
            .chain(internal.iter())
            .filter(|binding| !binding_is_overridden(binding, config));
        if retained.any(|binding| sequences_conflict(&candidate, &binding_sequence(binding))) {
            return Err(HotkeyNotice::Conflict(command.id.to_string()));
        }

        let _ = binding_for(command, custom, mapper)?;
    }
    Ok(())
}

fn resolve_bindings(
    framework: &[KeyBinding],
    defaults: &[KeyBinding],
    internal: &[KeyBinding],
    config: &KeymapConfig,
    mapper: &dyn PlatformKeyboardMapper,
) -> Result<Vec<KeyBinding>, HotkeyNotice> {
    validate_conflicts(framework, defaults, internal, config, mapper)?;
    let mut resolved = framework
        .iter()
        .filter(|binding| !binding_is_overridden(binding, config))
        .cloned()
        .collect::<Vec<_>>();
    resolved.extend(
        defaults
            .iter()
            .filter(|binding| !binding_is_overridden(binding, config))
            .cloned(),
    );
    resolved.extend(internal.iter().cloned());
    for (id, shortcut) in &config.bindings {
        if let Some(shortcut) = shortcut {
            let spec = command(id).ok_or_else(|| HotkeyNotice::UnknownCommand(id.clone()))?;
            resolved.push(binding_for(spec, shortcut, mapper)?);
        }
    }
    Ok(resolved)
}

fn default_bindings(mapper: &dyn PlatformKeyboardMapper) -> Result<Vec<KeyBinding>, HotkeyNotice> {
    ACTION_COMMANDS
        .iter()
        .filter_map(|spec| spec.default.map(|shortcut| (*spec, shortcut)))
        .map(|(spec, shortcut)| binding_for(spec, shortcut, mapper))
        .collect()
}

fn keymap_path() -> PathBuf {
    settings::config_dir().join("keymap.json")
}

fn parse_config(source: &str) -> Result<KeymapConfig, HotkeyNotice> {
    let file: KeymapFile = serde_json::from_str(source).map_err(|_| HotkeyNotice::InvalidFile)?;
    if file.version != 1 {
        return Err(HotkeyNotice::UnsupportedVersion(file.version));
    }
    for (id, shortcut) in &file.bindings {
        let spec = command(id).ok_or_else(|| HotkeyNotice::UnknownCommand(id.clone()))?;
        if let Some(shortcut) = shortcut {
            parse_keystrokes(shortcut)
                .map_err(|_| HotkeyNotice::InvalidShortcut(spec.id.to_string()))?;
        }
    }
    Ok(KeymapConfig {
        bindings: file.bindings,
    })
}

fn read_config(path: &Path) -> Result<KeymapConfig, HotkeyNotice> {
    match fs::read_to_string(path) {
        Ok(source) => parse_config(&source),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(KeymapConfig::default()),
        Err(_) => Err(HotkeyNotice::InvalidFile),
    }
}

fn atomic_write(path: &Path, config: &KeymapConfig) -> std::io::Result<()> {
    static TEMP_ID: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let serialized = serde_json::to_vec_pretty(&KeymapFileRef {
        version: 1,
        bindings: &config.bindings,
    })?;
    let result = (|| {
        file.write_all(&serialized)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn rebuild_runtime(
    runtime: &HotkeyRuntime,
    config: &KeymapConfig,
    cx: &App,
) -> Result<Vec<KeyBinding>, HotkeyNotice> {
    rebuild_runtime_with_mapper(runtime, config, cx.keyboard_mapper().as_ref())
}

fn rebuild_runtime_with_mapper(
    runtime: &HotkeyRuntime,
    config: &KeymapConfig,
    mapper: &dyn PlatformKeyboardMapper,
) -> Result<Vec<KeyBinding>, HotkeyNotice> {
    resolve_bindings(
        &runtime.framework,
        &runtime.defaults,
        &runtime.internal,
        config,
        mapper,
    )
}

fn replace_bindings(cx: &mut App, bindings: Vec<KeyBinding>) {
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    // Native menu key equivalents are snapshots, not live keymap references.
    if let Some(workspace) = cx
        .try_global::<crate::WorkspaceWindow>()
        .and_then(|current| current.workspace.upgrade())
    {
        let language = workspace.read(cx).settings().language;
        cx.set_menus(crate::menus(language));
    }
}

fn apply_runtime(cx: &mut App, config: KeymapConfig, warning: Option<HotkeyNotice>) {
    let runtime = cx.global::<HotkeyRuntime>().clone();
    let resolved = resolve_bindings(
        &runtime.framework,
        &runtime.defaults,
        &runtime.internal,
        &config,
        cx.keyboard_mapper().as_ref(),
    )
    .unwrap_or_else(|_| {
        runtime
            .framework
            .iter()
            .chain(runtime.defaults.iter())
            .chain(runtime.internal.iter())
            .cloned()
            .collect()
    });
    replace_bindings(cx, resolved);
    let state = cx.global_mut::<HotkeyRuntime>();
    state.config = config;
    state.warning = warning;
}

pub(crate) fn initialize(cx: &mut App, internal: Vec<KeyBinding>) {
    let framework = cx.key_bindings().borrow().bindings().cloned().collect();
    let defaults = default_bindings(cx.keyboard_mapper().as_ref())
        .expect("built-in shortcut definitions must be valid");
    let mut runtime = HotkeyRuntime {
        framework,
        defaults,
        internal,
        config: KeymapConfig::default(),
        warning: None,
    };
    match read_config(&keymap_path()) {
        Ok(config) => match rebuild_runtime(&runtime, &config, cx) {
            Ok(_) => runtime.config = config,
            Err(notice) => runtime.warning = Some(notice),
        },
        Err(notice) => runtime.warning = Some(notice),
    }
    let resolved = resolve_bindings(
        &runtime.framework,
        &runtime.defaults,
        &runtime.internal,
        &runtime.config,
        cx.keyboard_mapper().as_ref(),
    )
    .expect("default key bindings must be valid");
    replace_bindings(cx, resolved);
    cx.set_global(runtime);
}

pub(crate) fn effective_shortcut(config: &KeymapConfig, id: &str) -> Option<String> {
    match config.bindings.get(id) {
        Some(Some(shortcut)) => Some(shortcut.clone()),
        Some(None) => None,
        None => command(id).and_then(|spec| spec.default.map(str::to_string)),
    }
}

pub(crate) fn shortcut(cx: &App, id: &str) -> Option<String> {
    match cx.try_global::<HotkeyRuntime>() {
        Some(runtime) => effective_shortcut(&runtime.config, id),
        None => command(id).and_then(|spec| spec.default.map(str::to_string)),
    }
}

pub(crate) fn warning(cx: &App) -> Option<HotkeyNotice> {
    cx.try_global::<HotkeyRuntime>()
        .and_then(|runtime| runtime.warning.clone())
}

fn commit_config(cx: &mut App, config: KeymapConfig) -> Result<(), HotkeyNotice> {
    let runtime = cx.global::<HotkeyRuntime>().clone();
    let bindings = prepare_config_commit(
        &runtime,
        &config,
        &keymap_path(),
        cx.keyboard_mapper().as_ref(),
    )?;
    replace_bindings(cx, bindings);
    let state = cx.global_mut::<HotkeyRuntime>();
    state.config = config;
    state.warning = None;
    Ok(())
}

fn prepare_config_commit(
    runtime: &HotkeyRuntime,
    config: &KeymapConfig,
    path: &Path,
    mapper: &dyn PlatformKeyboardMapper,
) -> Result<Vec<KeyBinding>, HotkeyNotice> {
    let bindings = resolve_bindings(
        &runtime.framework,
        &runtime.defaults,
        &runtime.internal,
        config,
        mapper,
    )?;
    atomic_write(path, config).map_err(|_| HotkeyNotice::SaveFailed)?;
    Ok(bindings)
}

pub(crate) fn set_binding(
    cx: &mut App,
    id: &str,
    shortcut: Option<String>,
) -> Result<(), HotkeyNotice> {
    if command(id).is_none() {
        return Err(HotkeyNotice::UnknownCommand(id.to_string()));
    }
    if let Some(shortcut) = &shortcut {
        parse_keystrokes(shortcut).map_err(|_| HotkeyNotice::InvalidShortcut(id.to_string()))?;
    }
    let mut config = cx.global::<HotkeyRuntime>().config.clone();
    config.bindings.insert(id.to_string(), shortcut);
    commit_config(cx, config)
}

pub(crate) fn reset_command(cx: &mut App, id: &str) -> Result<(), HotkeyNotice> {
    if command(id).is_none() {
        return Err(HotkeyNotice::UnknownCommand(id.to_string()));
    }
    let mut config = cx.global::<HotkeyRuntime>().config.clone();
    config.bindings.remove(id);
    commit_config(cx, config)
}

pub(crate) fn reset_all(cx: &mut App) -> Result<(), HotkeyNotice> {
    commit_config(cx, KeymapConfig::default())
}

pub(crate) fn reload(cx: &mut App) {
    let runtime = cx.global::<HotkeyRuntime>().clone();
    match read_config(&keymap_path()) {
        Ok(config) => match resolve_bindings(
            &runtime.framework,
            &runtime.defaults,
            &runtime.internal,
            &config,
            cx.keyboard_mapper().as_ref(),
        ) {
            Ok(bindings) => {
                replace_bindings(cx, bindings);
                let state = cx.global_mut::<HotkeyRuntime>();
                state.config = config;
                state.warning = None;
            }
            Err(notice) => apply_runtime(cx, KeymapConfig::default(), Some(notice)),
        },
        Err(notice) => apply_runtime(cx, KeymapConfig::default(), Some(notice)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        binding_sequence, command, commands, default_bindings, effective_shortcut, input,
        parse_config, parse_keystrokes, prepare_config_commit, read_config,
        rebuild_runtime_with_mapper, resolve_bindings, DummyKeyboardMapper, HotkeyNotice,
        HotkeyRuntime, KeyBinding, KeymapConfig, ACTION_COMMANDS,
    };
    use std::fs;
    use std::path::PathBuf;

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rista-hotkeys-{name}-{}", std::process::id()))
    }

    fn mapper() -> DummyKeyboardMapper {
        DummyKeyboardMapper
    }

    fn test_bindings() -> Vec<KeyBinding> {
        default_bindings(&mapper()).unwrap()
    }

    #[test]
    fn hotkey_registry_has_unique_snake_case_ids() {
        let commands = commands();
        let mut ids = commands
            .iter()
            .map(|command| command.id)
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), commands.len());
        assert!(commands.len() > ACTION_COMMANDS.len());
        assert!(crate::app::PaletteCmd::ALL
            .iter()
            .all(|palette_command| command(palette_command.id()).is_some()));
        assert!(commands.iter().all(|command| command
            .id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')));
    }

    #[test]
    fn hotkey_keymap_rejects_invalid_files_without_losing_default_semantics() {
        assert!(matches!(
            parse_config(r#"{"version":2,"bindings":{}}"#),
            Err(HotkeyNotice::UnsupportedVersion(2))
        ));
        assert!(matches!(
            parse_config(r#"{"version":1,"bindings":{"not_a_command":"cmd-x"}}"#),
            Err(HotkeyNotice::UnknownCommand(id)) if id == "not_a_command"
        ));
        assert!(matches!(
            parse_config(r#"{"version":1,"bindings":{"new_file":"not-a-real-key"}}"#),
            Err(HotkeyNotice::InvalidShortcut(id)) if id == "new_file"
        ));
        let config = parse_config(r#"{"version":1,"bindings":{"save":null}}"#).unwrap();
        assert_eq!(effective_shortcut(&config, "save"), None);
        assert_eq!(
            effective_shortcut(&KeymapConfig::default(), "save").as_deref(),
            Some("cmd-s")
        );
    }

    #[test]
    fn hotkey_override_unbind_and_reset_rebuild_from_the_framework_snapshot() {
        let mapper = mapper();
        let framework = vec![KeyBinding::new("cmd-f", input::Search, None)];
        let defaults = test_bindings();
        let config = parse_config(r#"{"version":1,"bindings":{"find_in_note":"ctrl-f"}}"#).unwrap();
        let overridden = resolve_bindings(&framework, &defaults, &[], &config, &mapper).unwrap();
        assert!(overridden.iter().any(|binding| {
            binding.action().partial_eq(&input::Search)
                && binding_sequence(binding) == parse_keystrokes("ctrl-f").unwrap()
        }));
        assert!(!overridden.iter().any(|binding| {
            binding.action().partial_eq(&input::Search)
                && binding_sequence(binding) == parse_keystrokes("cmd-f").unwrap()
        }));

        let unbound = parse_config(r#"{"version":1,"bindings":{"find_in_note":null}}"#).unwrap();
        let result = resolve_bindings(&framework, &defaults, &[], &unbound, &mapper).unwrap();
        assert!(!result
            .iter()
            .any(|binding| binding.action().partial_eq(&input::Search)));

        let reset = resolve_bindings(
            &framework,
            &defaults,
            &[],
            &KeymapConfig::default(),
            &mapper,
        )
        .unwrap();
        assert!(reset
            .iter()
            .any(|binding| binding.action().partial_eq(&input::Search)));
        let repeated = resolve_bindings(
            &framework,
            &defaults,
            &[],
            &KeymapConfig::default(),
            &mapper,
        )
        .unwrap();
        assert_eq!(reset.len(), repeated.len());
    }

    #[test]
    fn hotkey_new_file_menu_binding_tracks_override_unbind_and_reset() {
        let defaults = test_bindings();
        for (source, expected) in [
            (
                r#"{"version":1,"bindings":{"new_file":"cmd-alt-n"}}"#,
                Some("cmd-alt-n"),
            ),
            (r#"{"version":1,"bindings":{"new_file":null}}"#, None),
            (r#"{"version":1,"bindings":{}}"#, Some("cmd-n")),
        ] {
            let config = parse_config(source).unwrap();
            let bindings = resolve_bindings(&[], &defaults, &[], &config, &mapper()).unwrap();
            let keymap = gpui_kit::Keymap::new(bindings);
            let bindings = keymap.bindings_for_action(&crate::actions::NewFile);
            let sequences = bindings.map(binding_sequence).collect::<Vec<_>>();
            let expected = expected
                .map(|shortcut| parse_keystrokes(shortcut).unwrap())
                .into_iter()
                .collect::<Vec<_>>();
            assert_eq!(sequences, expected);
        }
    }

    #[test]
    fn hotkey_malformed_file_preserves_bytes_and_falls_back_to_defaults() {
        let path = test_path("malformed");
        let bytes = br#"{"version":1,"bindings":{"new_file":"cmd-s""#;
        fs::write(&path, bytes).unwrap();
        assert!(matches!(read_config(&path), Err(HotkeyNotice::InvalidFile)));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            effective_shortcut(&KeymapConfig::default(), "new_file").as_deref(),
            Some("cmd-n")
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn hotkey_atomic_write_failure_preserves_existing_saved_bytes() {
        let parent = test_path("write-failure");
        let bytes = b"saved keymap bytes";
        fs::write(&parent, bytes).unwrap();
        let target = parent.join("keymap.json");
        let runtime = HotkeyRuntime {
            framework: Vec::new(),
            defaults: test_bindings(),
            internal: Vec::new(),
            config: KeymapConfig::default(),
            warning: None,
        };
        let before = runtime.config.clone();
        assert!(matches!(
            prepare_config_commit(&runtime, &KeymapConfig::default(), &target, &mapper()),
            Err(HotkeyNotice::SaveFailed)
        ));
        assert_eq!(runtime.config, before);
        assert_eq!(fs::read(&parent).unwrap(), bytes);
        let _ = fs::remove_file(parent);
    }

    #[test]
    fn hotkey_normalized_modifier_aliases_conflict() {
        let mapper = mapper();
        let defaults = test_bindings();
        let config = parse_config(
            r#"{"version":1,"bindings":{"new_file":"ctrl-alt-z","open_file":"alt-ctrl-z"}}"#,
        )
        .unwrap();
        assert!(matches!(
            resolve_bindings(&[], &defaults, &[], &config, &mapper),
            Err(HotkeyNotice::Conflict(_))
        ));
    }

    #[test]
    fn hotkey_rebuild_retains_contextual_framework_and_internal_bindings() {
        let mapper = mapper();
        let framework = vec![KeyBinding::new("cmd-f", input::Search, Some("Editor"))];
        let internal = crate::internal_keymap();
        let defaults = test_bindings();
        let runtime = HotkeyRuntime {
            framework: framework.clone(),
            defaults: defaults.clone(),
            internal: internal.clone(),
            config: KeymapConfig::default(),
            warning: None,
        };
        let config = parse_config(r#"{"version":1,"bindings":{"new_file":"ctrl-alt-z"}}"#).unwrap();
        let rebuilt = rebuild_runtime_with_mapper(&runtime, &config, &mapper).unwrap();
        for baseline in framework.iter().chain(internal.iter()) {
            let predicate = baseline.predicate().unwrap();
            assert!(rebuilt.iter().any(|binding| {
                binding_sequence(binding) == binding_sequence(baseline)
                    && binding
                        .predicate()
                        .is_some_and(|actual| std::rc::Rc::ptr_eq(&actual, &predicate))
                    && binding.action().partial_eq(baseline.action())
            }));
        }
    }

    #[test]
    fn hotkey_conflicts_reject_equal_and_prefix_collisions() {
        let mapper = mapper();
        let defaults = test_bindings();
        let unchanged_default =
            parse_config(r#"{"version":1,"bindings":{"new_file":"cmd-n","open_file":"cmd-n"}}"#)
                .unwrap();
        assert!(matches!(
            resolve_bindings(&[], &defaults, &[], &unchanged_default, &mapper),
            Err(HotkeyNotice::Conflict(_))
        ));
        let equal = parse_config(r#"{"version":1,"bindings":{"new_file":"cmd-shift-n"}}"#).unwrap();
        assert!(matches!(
            resolve_bindings(&[], &defaults, &[], &equal, &mapper),
            Err(HotkeyNotice::Conflict(_))
        ));
        let prefix =
            parse_config(r#"{"version":1,"bindings":{"new_file":"cmd-k cmd-s"}}"#).unwrap();
        assert!(matches!(
            resolve_bindings(&[], &defaults, &[], &prefix, &mapper),
            Err(HotkeyNotice::Conflict(_))
        ));
    }

    #[test]
    fn hotkey_custom_assignment_cannot_take_a_retained_framework_shortcut() {
        let mapper = mapper();
        let framework = vec![KeyBinding::new("cmd-f", input::Search, None)];
        let defaults = test_bindings();
        let config = parse_config(r#"{"version":1,"bindings":{"new_file":"cmd-f"}}"#).unwrap();
        assert!(matches!(
            resolve_bindings(&framework, &defaults, &[], &config, &mapper),
            Err(HotkeyNotice::Conflict(_))
        ));
    }
}
