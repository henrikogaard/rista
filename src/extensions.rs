use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const API_VERSION: u32 = 1;
const MAX_MANIFESTS_PER_DIRECTORY: usize = 128;
const MAX_MANIFESTS_TOTAL: usize = 256;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_COMMANDS: usize = 64;
const MAX_AGENTS: usize = 64;
const MAX_ID_BYTES: usize = 128;
const MAX_NAME_BYTES: usize = 256;
const MAX_PROGRAM_BYTES: usize = 2 * 1024;
const MAX_ARGUMENT_BYTES: usize = 4 * 1024;
const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENTS_TOTAL_BYTES: usize = 16 * 1024;
const MAX_ERRORS: usize = 512;
pub const BUILTIN_VIBE_AGENT_ID: &str = "vibe";
pub const BUILTIN_CODEX_AGENT_ID: &str = "rista-codex";
pub const BUILTIN_CLAUDE_AGENT_ID: &str = "rista-claude";
pub const BUILTIN_OPENCODE_AGENT_ID: &str = "rista-opencode";
pub const BUILTIN_GROK_AGENT_ID: &str = "rista-grok";
const BUILTIN_AGENT_IDS: &[&str] = &[
    BUILTIN_VIBE_AGENT_ID,
    BUILTIN_CODEX_AGENT_ID,
    BUILTIN_CLAUDE_AGENT_ID,
    BUILTIN_OPENCODE_AGENT_ID,
    BUILTIN_GROK_AGENT_ID,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Localized {
    pub en: String,
    pub nb: String,
}

impl Localized {
    pub fn text(&self, language: crate::settings::Language) -> &str {
        match language {
            crate::settings::Language::English => &self.en,
            crate::settings::Language::Norwegian => &self.nb,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub name: Localized,
    pub commands: Vec<Launcher>,
    #[serde(default)]
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: String,
    pub name: Localized,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

struct BuiltinAgentPreset {
    agent: Agent,
    setup: Localized,
}

fn builtin_presets() -> Vec<BuiltinAgentPreset> {
    [
        (
            BUILTIN_VIBE_AGENT_ID,
            "Vibe",
            "Vibe",
            "vibe-acp",
            &[] as &[&str],
            "Install Vibe with `uv tool install mistral-vibe`, then run `vibe` in Terminal to complete setup and sign in.",
            "Installer Vibe med `uv tool install mistral-vibe`, og kjør deretter `vibe` i Terminal for å fullføre oppsettet og logge inn.",
        ),
        (
            BUILTIN_CODEX_AGENT_ID,
            "Codex",
            "Codex",
            "codex-acp",
            &[] as &[&str],
            "Install the ACP adapter with `npm install -g @agentclientprotocol/codex-acp`, then sign in with `codex login` or follow the adapter's documented authentication setup.",
            "Installer ACP-adapteren med `npm install -g @agentclientprotocol/codex-acp`, og logg deretter inn med `codex login` eller følg adapterens dokumenterte autentiseringsoppsett.",
        ),
        (
            BUILTIN_CLAUDE_AGENT_ID,
            "Claude",
            "Claude",
            "claude-agent-acp",
            &[] as &[&str],
            "Install the ACP adapter with `npm install -g @agentclientprotocol/claude-agent-acp` (Node.js 22+ required), then authenticate using Claude's own CLI or setup.",
            "Installer ACP-adapteren med `npm install -g @agentclientprotocol/claude-agent-acp` (Node.js 22+ kreves), og autentiser deretter med Claudes egen CLI eller oppsett.",
        ),
        (
            BUILTIN_OPENCODE_AGENT_ID,
            "OpenCode",
            "OpenCode",
            "opencode",
            &["acp"] as &[&str],
            "Install OpenCode with `npm install -g opencode-ai`, then run `opencode auth login` and configure a provider.",
            "Installer OpenCode med `npm install -g opencode-ai`, kjør deretter `opencode auth login` og konfigurer en leverandør.",
        ),
        (
            BUILTIN_GROK_AGENT_ID,
            "Grok (via OpenCode)",
            "Grok (via OpenCode)",
            "opencode",
            &["acp"] as &[&str],
            "Install OpenCode with `npm install -g opencode-ai`, configure xAI access with `opencode auth login`, and choose an available model whose ID starts with `xai/`. Rísta does not select or invent a model ID.",
            "Installer OpenCode med `npm install -g opencode-ai`, konfigurer xAI-tilgang med `opencode auth login`, og velg en tilgjengelig modell med ID som starter med `xai/`. Rísta velger eller finner ikke på modell-ID-er.",
        ),
    ]
    .into_iter()
    .map(
        |(id, en, nb, program, args, help_en, help_nb)| BuiltinAgentPreset {
            agent: Agent {
                id: id.into(),
                name: Localized {
                    en: en.into(),
                    nb: nb.into(),
                },
                program: program.into(),
                args: args.iter().map(|arg| (*arg).into()).collect(),
            },
            setup: Localized {
                en: help_en.into(),
                nb: help_nb.into(),
            },
        },
    )
    .collect()
}

pub fn builtin_setup_help(agent_id: &str) -> Option<Localized> {
    builtin_presets()
        .into_iter()
        .find(|preset| preset.agent.id == agent_id)
        .map(|preset| preset.setup)
}

pub fn launch_error_text(
    agent_id: &str,
    program: &str,
    kind: std::io::ErrorKind,
    detail: &str,
    language: crate::settings::Language,
    is_manifest_agent: bool,
) -> String {
    match kind {
        std::io::ErrorKind::NotFound => {
            let mut text = match language {
                crate::settings::Language::English => format!(
                    "Executable `{program}` was not found in the app PATH. It may be installed outside the app PATH."
                ),
                crate::settings::Language::Norwegian => format!(
                    "Fant ikke den kjørbare filen `{program}` i appens PATH. Den kan være installert utenfor appens PATH."
                ),
            };
            if let Some(help) = builtin_setup_help(agent_id) {
                text.push('\n');
                text.push_str(help.text(language));
            } else if is_manifest_agent {
                text.push('\n');
                text.push_str(match language {
                    crate::settings::Language::English => {
                        "For a custom agent, set `program` to its absolute executable path."
                    }
                    crate::settings::Language::Norwegian => {
                        "For en egendefinert agent kan `program` settes til den absolutte filbanen."
                    }
                });
            }
            text
        }
        std::io::ErrorKind::PermissionDenied => match language {
            crate::settings::Language::English => {
                format!("Permission was denied when starting `{program}`: {detail}")
            }
            crate::settings::Language::Norwegian => {
                format!("Tillatelse ble nektet ved oppstart av `{program}`: {detail}")
            }
        },
        _ => match language {
            crate::settings::Language::English => {
                format!("Could not start `{program}`: {detail}")
            }
            crate::settings::Language::Norwegian => {
                format!("Kunne ikke starte `{program}`: {detail}")
            }
        },
    }
}

pub fn is_grok_agent(agent_id: &str) -> bool {
    agent_id == BUILTIN_GROK_AGENT_ID
}

pub fn registered_agents(plugins: Vec<Manifest>) -> Vec<(Option<String>, Agent)> {
    let mut agents = builtin_presets()
        .into_iter()
        .map(|preset| (None, preset.agent))
        .collect::<Vec<_>>();
    agents.extend(plugins.into_iter().flat_map(|manifest| {
        let manifest_id = manifest.id;
        manifest
            .agents
            .into_iter()
            .map(move |agent| (Some(manifest_id.clone()), agent))
    }));
    agents
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launcher {
    pub id: String,
    pub name: Localized,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_directory: WorkingDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkingDirectory {
    #[default]
    Vault,
    Folder,
}

#[derive(Debug, Default)]
pub struct Registry {
    pub plugins: Vec<Manifest>,
    pub errors: Vec<RegistryError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError {
    pub path: PathBuf,
    pub message: Localized,
    pub detail: Option<String>,
}

impl RegistryError {
    pub fn text(&self, language: crate::settings::Language) -> String {
        let mut text = format!("{}: {}", self.path.display(), self.message.text(language));
        if let Some(detail) = &self.detail {
            text.push_str(": ");
            text.push_str(detail);
        }
        text
    }
}

impl Registry {
    pub fn load(user_dir: &Path, vault_root: Option<&Path>) -> Self {
        let mut registry = Self::default();
        let mut seen = HashMap::new();
        let mut total = 0;

        load_directory(user_dir, &mut registry, &mut seen, &mut total);

        if let Some(vault_root) = vault_root {
            let vault_root_is_directory = match fs::symlink_metadata(vault_root) {
                Ok(metadata) => !metadata.file_type().is_symlink() && metadata.is_dir(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    push_error(
                        &mut registry,
                        format!(
                            "{}: could not inspect vault root: {error}",
                            vault_root.display()
                        ),
                    );
                    false
                }
            };
            if vault_root_is_directory {
                let private_dir = vault_root.join(".rista");
                let private_metadata = match fs::symlink_metadata(&private_dir) {
                    Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => None,
                    Ok(_) => Some(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        push_error(
                            &mut registry,
                            format!(
                                "{}: could not inspect vault plugin parent: {error}",
                                private_dir.display()
                            ),
                        );
                        None
                    }
                };
                if private_metadata.is_some() {
                    load_directory(
                        &private_dir.join("plugins"),
                        &mut registry,
                        &mut seen,
                        &mut total,
                    );
                }
            }
        }

        registry
    }
}

pub fn user_dir() -> PathBuf {
    crate::settings::config_dir().join("plugins")
}

pub fn bundled_launchers() -> Vec<Launcher> {
    ["claude", "codex", "vibe", "pi", "omp", "opencode"]
        .into_iter()
        .map(|name| Launcher {
            id: name.to_string(),
            name: Localized {
                en: name.to_string(),
                nb: name.to_string(),
            },
            program: name.to_string(),
            args: Vec::new(),
            working_directory: WorkingDirectory::Vault,
        })
        .collect()
}

fn load_directory(
    directory: &Path,
    registry: &mut Registry,
    seen: &mut HashMap<String, PathBuf>,
    total: &mut usize,
) {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => return,
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            push_error(
                registry,
                format!(
                    "{}: could not inspect plugin directory: {error}",
                    directory.display()
                ),
            );
            return;
        }
    }

    let mut paths = Vec::new();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            push_error(
                registry,
                format!(
                    "{}: could not read plugin directory: {error}",
                    directory.display()
                ),
            );
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                push_error(
                    registry,
                    format!(
                        "{}: could not read plugin directory entry: {error}",
                        directory.display()
                    ),
                );
                continue;
            }
        };
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
            if paths.len() > MAX_MANIFESTS_PER_DIRECTORY {
                push_error(
                    registry,
                    format!(
                        "{}: plugin directory exceeds the {}-manifest limit",
                        directory.display(),
                        MAX_MANIFESTS_PER_DIRECTORY
                    ),
                );
                return;
            }
        }
    }
    paths.sort();

    for path in paths {
        if *total >= MAX_MANIFESTS_TOTAL {
            push_error(
                registry,
                format!(
                    "{}: registry exceeds the {}-manifest limit",
                    path.display(),
                    MAX_MANIFESTS_TOTAL
                ),
            );
            return;
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => continue,
            Ok(metadata) => metadata,
            Err(error) => {
                push_error(
                    registry,
                    format!("{}: could not inspect manifest: {error}", path.display()),
                );
                continue;
            }
        };
        *total += 1;
        if metadata.len() > MAX_MANIFEST_BYTES {
            push_error(
                registry,
                format!(
                    "{}: manifest exceeds the {}-byte limit",
                    path.display(),
                    MAX_MANIFEST_BYTES
                ),
            );
            continue;
        }
        let file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(error) => {
                push_error(
                    registry,
                    format!("{}: could not read manifest: {error}", path.display()),
                );
                continue;
            }
        };
        let mut bytes = Vec::new();
        if let Err(error) = file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes) {
            push_error(
                registry,
                format!("{}: could not read manifest: {error}", path.display()),
            );
            continue;
        }
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            push_error(
                registry,
                format!(
                    "{}: manifest exceeds the {}-byte limit",
                    path.display(),
                    MAX_MANIFEST_BYTES
                ),
            );
            continue;
        }

        let manifest: Manifest = match serde_json::from_slice(&bytes) {
            Ok(manifest) => manifest,
            Err(error) => {
                push_error(
                    registry,
                    format!("{}: invalid manifest JSON: {error}", path.display()),
                );
                continue;
            }
        };
        if let Err(error) = validate_manifest(&manifest) {
            push_error(registry, format!("{}: {error}", path.display()));
            continue;
        }
        if let Some(previous) = seen.get(&manifest.id) {
            push_error(
                registry,
                format!(
                    "{}: duplicate manifest id {:?}; first loaded from {}",
                    path.display(),
                    manifest.id,
                    previous.display()
                ),
            );
            continue;
        }
        seen.insert(manifest.id.clone(), path);
        registry.plugins.push(manifest);
    }
}

fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    if manifest.api_version != API_VERSION {
        return Err(format!(
            "unsupported api_version {}; expected {}",
            manifest.api_version, API_VERSION
        ));
    }
    validate_id("manifest id", &manifest.id)?;
    validate_localized("manifest name", &manifest.name)?;
    if manifest.commands.len() > MAX_COMMANDS {
        return Err(format!("manifest exceeds the {MAX_COMMANDS}-command limit"));
    }

    let mut command_ids = HashSet::new();
    for command in &manifest.commands {
        validate_id("command id", &command.id)?;
        if !command_ids.insert(&command.id) {
            return Err(format!("duplicate command id {:?}", command.id));
        }
        validate_localized("command name", &command.name)?;
        validate_text("program", &command.program, MAX_PROGRAM_BYTES, false)?;
        if command.program.trim().is_empty() {
            return Err("program must be nonempty".to_string());
        }
        if command.args.len() > MAX_ARGUMENTS {
            return Err(format!(
                "command {:?} exceeds the {MAX_ARGUMENTS}-argument limit",
                command.id
            ));
        }
        let mut total_argument_bytes = 0;
        for argument in &command.args {
            validate_text("argument", argument, MAX_ARGUMENT_BYTES, true)?;
            total_argument_bytes += argument.len();
            if total_argument_bytes > MAX_ARGUMENTS_TOTAL_BYTES {
                return Err(format!(
                    "command {:?} exceeds the {MAX_ARGUMENTS_TOTAL_BYTES}-byte total argument limit",
                    command.id
                ));
            }
        }
    }

    if manifest.agents.len() > MAX_AGENTS {
        return Err(format!("manifest exceeds the {MAX_AGENTS}-agent limit"));
    }
    let mut agent_ids = HashSet::new();
    for agent in &manifest.agents {
        validate_id("agent id", &agent.id)?;
        if BUILTIN_AGENT_IDS.contains(&agent.id.as_str()) {
            return Err(format!(
                "agent id {:?} is reserved for a built-in agent",
                agent.id
            ));
        }
        if !agent_ids.insert(&agent.id) {
            return Err(format!("duplicate agent id {:?}", agent.id));
        }
        validate_localized("agent name", &agent.name)?;
        validate_text("agent program", &agent.program, MAX_PROGRAM_BYTES, false)?;
        if agent.program.trim().is_empty() {
            return Err("agent program must be nonempty".to_string());
        }
        if agent.args.len() > MAX_ARGUMENTS {
            return Err(format!(
                "agent {:?} exceeds the {MAX_ARGUMENTS}-argument limit",
                agent.id
            ));
        }
        let mut total_argument_bytes = 0;
        for argument in &agent.args {
            validate_text("agent argument", argument, MAX_ARGUMENT_BYTES, true)?;
            total_argument_bytes += argument.len();
            if total_argument_bytes > MAX_ARGUMENTS_TOTAL_BYTES {
                return Err(format!(
                    "agent {:?} exceeds the {MAX_ARGUMENTS_TOTAL_BYTES}-byte total argument limit",
                    agent.id
                ));
            }
        }
    }
    Ok(())
}

fn validate_id(field: &str, value: &str) -> Result<(), String> {
    validate_text(field, value, MAX_ID_BYTES, false)?;
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(byte))
    {
        return Err(format!(
            "{field} must start with a lowercase letter or digit and contain only lowercase letters, digits, '.', '_' or '-'"
        ));
    }
    Ok(())
}

fn validate_localized(field: &str, value: &Localized) -> Result<(), String> {
    validate_text(&format!("{field}.en"), &value.en, MAX_NAME_BYTES, false)?;
    validate_text(&format!("{field}.nb"), &value.nb, MAX_NAME_BYTES, false)?;
    Ok(())
}

fn validate_text(
    field: &str,
    value: &str,
    max_bytes: usize,
    allow_empty: bool,
) -> Result<(), String> {
    if value.contains('\0') {
        return Err(format!("{field} must not contain NUL bytes"));
    }
    if !allow_empty && value.is_empty() {
        return Err(format!("{field} must be nonempty"));
    }
    if value.len() > max_bytes {
        return Err(format!("{field} exceeds the {max_bytes}-byte limit"));
    }
    Ok(())
}

fn push_error(registry: &mut Registry, error: String) {
    if registry.errors.len() < MAX_ERRORS {
        let (path, detail) = error
            .split_once(": ")
            .map(|(path, detail)| (PathBuf::from(path), detail))
            .unwrap_or_else(|| (PathBuf::new(), error.as_str()));
        let (message, technical_detail) = localize_diagnostic(detail);
        registry.errors.push(RegistryError {
            path,
            message,
            detail: technical_detail,
        });
    }
}

fn localize_diagnostic(detail: &str) -> (Localized, Option<String>) {
    let (en, nb, technical_detail) = if detail.starts_with("could not inspect vault root") {
        (
            "Could not inspect vault root",
            "Kunne ikke undersøke hvelvroten",
            detail.strip_prefix("could not inspect vault root: "),
        )
    } else if detail.starts_with("could not inspect vault plugin parent") {
        (
            "Could not inspect the vault plugin folder",
            "Kunne ikke undersøke hvelvets utvidelsesmappe",
            detail.strip_prefix("could not inspect vault plugin parent: "),
        )
    } else if detail.starts_with("could not inspect plugin directory") {
        (
            "Could not inspect the extension folder",
            "Kunne ikke undersøke utvidelsesmappen",
            detail.strip_prefix("could not inspect plugin directory: "),
        )
    } else if detail.starts_with("could not read plugin directory entry") {
        (
            "Could not read an extension entry",
            "Kunne ikke lese en utvidelsesoppføring",
            detail.strip_prefix("could not read plugin directory entry: "),
        )
    } else if detail.starts_with("could not read plugin directory") {
        (
            "Could not read the extension folder",
            "Kunne ikke lese utvidelsesmappen",
            detail.strip_prefix("could not read plugin directory: "),
        )
    } else if detail.starts_with("plugin directory exceeds") {
        (
            "The extension folder contains too many manifests",
            "Utvidelsesmappen inneholder for mange manifester",
            None,
        )
    } else if detail.starts_with("registry exceeds") {
        (
            "Too many extensions were found",
            "For mange utvidelser ble funnet",
            None,
        )
    } else if detail.starts_with("could not inspect manifest") {
        (
            "Could not inspect an extension manifest",
            "Kunne ikke undersøke et utvidelsesmanifest",
            detail.strip_prefix("could not inspect manifest: "),
        )
    } else if detail.starts_with("manifest exceeds") {
        (
            "The extension manifest is too large",
            "Utvidelsesmanifestet er for stort",
            None,
        )
    } else if detail.starts_with("could not read manifest") {
        (
            "Could not read an extension manifest",
            "Kunne ikke lese et utvidelsesmanifest",
            detail.strip_prefix("could not read manifest: "),
        )
    } else if detail.starts_with("invalid manifest JSON") {
        (
            "The extension manifest contains invalid JSON",
            "Utvidelsesmanifestet inneholder ugyldig JSON",
            detail.strip_prefix("invalid manifest JSON: "),
        )
    } else if detail.starts_with("unsupported api_version") {
        (
            "The extension API version is unsupported",
            "API-versjonen for utvidelsen støttes ikke",
            None,
        )
    } else if detail.starts_with("duplicate manifest id") {
        (
            "The extension ID is already registered",
            "Utvidelses-ID-en er allerede registrert",
            None,
        )
    } else if detail.starts_with("duplicate command id") {
        (
            "The command ID is duplicated",
            "Kommando-ID-en er duplisert",
            None,
        )
    } else if detail.starts_with("program must") {
        (
            "The launcher program is invalid",
            "Startprogrammet er ugyldig",
            None,
        )
    } else if detail.starts_with("command ") && detail.contains("exceeds") {
        (
            "The launcher command exceeds a limit",
            "Startkommandoen overskrider en grense",
            None,
        )
    } else if detail.contains("must not be empty")
        || detail.contains("must be nonempty")
        || detail.contains("must not contain NUL")
        || detail.contains("exceeds the")
    {
        (
            "A manifest value is invalid",
            "En manifestverdi er ugyldig",
            None,
        )
    } else {
        (
            "The extension manifest could not be loaded",
            "Utvidelsesmanifestet kunne ikke lastes inn",
            None,
        )
    };
    (
        Localized {
            en: en.to_string(),
            nb: nb.to_string(),
        },
        technical_detail.map(str::to_string),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        bundled_launchers, registered_agents, user_dir, validate_manifest, Agent, Launcher,
        Localized, Manifest, Registry, WorkingDirectory, BUILTIN_VIBE_AGENT_ID,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rista-extension-tests-{}-{}",
                std::process::id(),
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create temporary test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn valid_manifest(id: &str, english: &str) -> Manifest {
        Manifest {
            api_version: 1,
            id: id.to_string(),
            name: Localized {
                en: english.to_string(),
                nb: format!("{english} nb"),
            },
            commands: vec![Launcher {
                id: "open".to_string(),
                name: Localized {
                    en: "Open".to_string(),
                    nb: "Åpne".to_string(),
                },
                program: "example-tool".to_string(),
                args: Vec::new(),
                working_directory: WorkingDirectory::Vault,
            }],
            agents: Vec::new(),
        }
    }

    fn write_manifest(path: &Path, manifest: &Manifest) {
        fs::write(
            path,
            serde_json::to_vec(manifest).expect("serialize manifest fixture"),
        )
        .expect("write manifest fixture");
    }

    #[test]
    fn loads_valid_manifest_with_default_launcher_fields() {
        let temp = TempDir::new();
        let user_dir = temp.path().join("plugins");
        fs::create_dir_all(&user_dir).expect("create plugin directory");
        let path = user_dir.join("valid.json");
        fs::write(
            &path,
            r#"{"api_version":1,"id":"sample.plugin","name":{"en":"Sample","nb":"Eksempel"},"commands":[{"id":"open","name":{"en":"Open","nb":"Åpne"},"program":"example-tool"}]}"#,
        )
        .expect("write manifest fixture");

        let registry = Registry::load(&user_dir, None);

        assert_eq!(registry.plugins.len(), 1);
        assert!(registry.errors.is_empty());
        assert_eq!(registry.plugins[0].commands[0].args, Vec::<String>::new());
        assert_eq!(
            registry.plugins[0].commands[0].working_directory,
            WorkingDirectory::Vault
        );
        assert!(registry.plugins[0].agents.is_empty());
    }

    #[test]
    fn loads_and_validates_configured_agents() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"api_version":1,"id":"sample","name":{"en":"Sample","nb":"Eksempel"},"commands":[],"agents":[{"id":"assistant","name":{"en":"Assistant","nb":"Assistent"},"program":"agent-cli","args":["--stdio"]}]}"#,
        )
        .expect("deserialize agent manifest");

        validate_manifest(&manifest).expect("validate agent manifest");

        assert_eq!(manifest.agents.len(), 1);
        assert_eq!(manifest.agents[0].args, ["--stdio"]);
    }

    #[test]
    fn lists_all_builtin_agents_and_reserves_their_ids() {
        let mut manifest = valid_manifest("sample", "Sample");
        manifest.agents.push(Agent {
            id: "assistant".into(),
            name: Localized {
                en: "Assistant".into(),
                nb: "Assistent".into(),
            },
            program: "agent-cli".into(),
            args: Vec::new(),
        });
        let agents = registered_agents(vec![manifest.clone()]);
        let expected = [
            ("vibe", "Vibe", "vibe-acp", &[][..]),
            ("rista-codex", "Codex", "codex-acp", &[][..]),
            ("rista-claude", "Claude", "claude-agent-acp", &[][..]),
            ("rista-opencode", "OpenCode", "opencode", &["acp"][..]),
            (
                "rista-grok",
                "Grok (via OpenCode)",
                "opencode",
                &["acp"][..],
            ),
        ];
        assert_eq!(agents.len(), expected.len() + 1);
        for (index, (id, name, program, args)) in expected.iter().enumerate() {
            assert_eq!(agents[index].0, None);
            assert_eq!(agents[index].1.id, *id);
            assert_eq!(agents[index].1.name.en, *name);
            assert_eq!(agents[index].1.name.nb, *name);
            assert_eq!(agents[index].1.program, *program);
            assert_eq!(agents[index].1.args, *args);
            assert!(super::builtin_setup_help(id).is_some());

            manifest.agents[0].id = (*id).into();
            assert!(validate_manifest(&manifest)
                .expect_err("reject reserved built-in agent id")
                .contains("reserved"));
        }
        assert_eq!(agents[expected.len()].0.as_deref(), Some("sample"));
        assert_eq!(agents[expected.len()].1.id, "assistant");
        manifest.agents[0].id = "custom-agent".into();
        assert!(validate_manifest(&manifest).is_ok());

        for (id, command) in [
            ("vibe", "uv tool install mistral-vibe"),
            (
                "rista-codex",
                "npm install -g @agentclientprotocol/codex-acp",
            ),
            (
                "rista-claude",
                "npm install -g @agentclientprotocol/claude-agent-acp",
            ),
            ("rista-opencode", "npm install -g opencode-ai"),
            ("rista-grok", "xai/"),
        ] {
            let setup = super::builtin_setup_help(id).expect("localized setup help");
            assert!(setup.en.contains(command));
            assert!(setup.nb.contains(command));
        }
    }

    #[test]
    fn launch_errors_distinguish_missing_and_denied_executables() {
        let missing = super::launch_error_text(
            BUILTIN_VIBE_AGENT_ID,
            "vibe-acp",
            std::io::ErrorKind::NotFound,
            "missing",
            crate::settings::Language::English,
            false,
        );
        assert!(missing.contains("not found in the app PATH"));
        assert!(missing.contains("uv tool install mistral-vibe"));
        assert!(!missing.contains("not installed"));

        let denied = super::launch_error_text(
            "custom",
            "agent-cli",
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
            crate::settings::Language::English,
            true,
        );
        assert!(denied.contains("Permission was denied"));
        assert!(!denied.contains("not found"));

        let other = super::launch_error_text(
            "custom",
            "agent-cli",
            std::io::ErrorKind::Other,
            "unavailable",
            crate::settings::Language::English,
            true,
        );
        assert!(other.contains("Could not start `agent-cli`"));
        assert!(!other.contains("Permission was denied"));

        let custom_missing = super::launch_error_text(
            "custom",
            "agent-cli",
            std::io::ErrorKind::NotFound,
            "missing",
            crate::settings::Language::Norwegian,
            true,
        );
        assert!(custom_missing.contains("i appens PATH"));
        assert!(custom_missing.contains("absolutte filbanen"));
    }

    #[test]
    fn rejects_duplicate_agent_ids_and_unknown_agent_fields() {
        let mut manifest = valid_manifest("sample", "Sample");
        let agent = Agent {
            id: "assistant".into(),
            name: Localized {
                en: "Assistant".into(),
                nb: "Assistent".into(),
            },
            program: "agent-cli".into(),
            args: Vec::new(),
        };
        manifest.agents = vec![agent.clone(), agent];

        assert!(validate_manifest(&manifest)
            .expect_err("duplicate agent id")
            .contains("duplicate agent id"));
        assert!(serde_json::from_str::<Agent>(
            r#"{"id":"assistant","name":{"en":"Assistant","nb":"Assistent"},"program":"agent-cli","extra":true}"#
        )
        .is_err());
    }

    #[test]
    fn rejects_unsupported_versions_and_unknown_fields() {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path()).expect("create plugin directory");
        fs::write(
            temp.path().join("a-version.json"),
            r#"{"api_version":2,"id":"sample","name":{"en":"Sample","nb":"Eksempel"},"commands":[]}"#,
        )
        .expect("write unsupported version fixture");
        fs::write(
            temp.path().join("b-unknown.json"),
            r#"{"api_version":1,"id":"other","name":{"en":"Other","nb":"Annen"},"commands":[],"entrypoint":"run"}"#,
        )
        .expect("write unknown field fixture");

        let registry = Registry::load(temp.path(), None);

        assert!(registry.plugins.is_empty());
        assert_eq!(registry.errors.len(), 2);
        let english = crate::settings::Language::English;
        assert!(registry.errors[0].text(english).contains("unsupported"));
        assert!(registry.errors[1].text(english).contains("invalid JSON"));
    }

    #[test]
    fn rejects_malformed_manifests() {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path()).expect("create plugin directory");
        fs::write(temp.path().join("broken.json"), "{").expect("write malformed fixture");
        fs::write(
            temp.path().join("oversized.json"),
            vec![b' '; super::MAX_MANIFEST_BYTES as usize + 1],
        )
        .expect("write oversized fixture");

        let registry = Registry::load(temp.path(), None);

        assert!(registry.plugins.is_empty());
        assert_eq!(registry.errors.len(), 2);
        let english = crate::settings::Language::English;
        assert!(registry.errors[0].text(english).contains("broken.json"));
        assert!(registry.errors[0].text(english).contains("invalid JSON"));
        assert!(registry.errors[1].text(english).contains("oversized.json"));
        assert!(registry.errors[1].text(english).contains("too large"));
    }

    #[test]
    fn rejects_duplicate_ids_deterministically_and_keeps_user_plugin_over_vault() {
        let temp = TempDir::new();
        let user_dir = temp.path().join("user");
        let vault_root = temp.path().join("vault");
        let vault_plugins = vault_root.join(".rista/plugins");
        fs::create_dir_all(&user_dir).expect("create user plugin directory");
        fs::create_dir_all(&vault_plugins).expect("create vault plugin directory");
        write_manifest(&user_dir.join("z.json"), &valid_manifest("same-id", "Zed"));
        write_manifest(
            &user_dir.join("a.json"),
            &valid_manifest("same-id", "Alpha"),
        );
        write_manifest(
            &vault_plugins.join("vault.json"),
            &valid_manifest("same-id", "Vault"),
        );

        let registry = Registry::load(&user_dir, Some(&vault_root));

        assert_eq!(registry.plugins.len(), 1);
        assert_eq!(registry.plugins[0].name.en, "Alpha");
        assert_eq!(registry.errors.len(), 2);
        let english = crate::settings::Language::English;
        assert!(registry.errors[0].text(english).contains("z.json"));
        assert!(registry.errors[1].text(english).contains("vault.json"));
    }

    #[test]
    fn diagnostics_have_localized_headings_and_technical_details() {
        let temp = TempDir::new();
        fs::create_dir_all(temp.path()).expect("create plugin directory");
        fs::write(
            temp.path().join("invalid.json"),
            r#"{"api_version":2,"id":"sample","name":{"en":"Sample","nb":"Eksempel"},"commands":[]}"#,
        )
        .expect("write invalid fixture");

        let registry = Registry::load(temp.path(), None);

        let english = registry.errors[0].text(crate::settings::Language::English);
        let norwegian = registry.errors[0].text(crate::settings::Language::Norwegian);
        assert!(english.contains("unsupported"));
        assert!(norwegian.contains("støttes ikke"));
        assert_ne!(english, norwegian);
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinked_plugin_files_and_directories() {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new();
        let real_dir = temp.path().join("real");
        let linked_dir = temp.path().join("linked");
        let outside_dir = temp.path().join("outside");
        let vault_root = temp.path().join("vault");
        fs::create_dir_all(&real_dir).expect("create real plugin directory");
        fs::create_dir_all(&outside_dir).expect("create outside plugin directory");
        write_manifest(&real_dir.join("good.json"), &valid_manifest("good", "Good"));
        write_manifest(
            &outside_dir.join("outside.json"),
            &valid_manifest("outside", "Outside"),
        );
        symlink(&real_dir, &linked_dir).expect("create plugin directory symlink");
        symlink(
            outside_dir.join("outside.json"),
            real_dir.join("linked.json"),
        )
        .expect("create manifest symlink");
        fs::create_dir_all(vault_root.join(".rista")).expect("create vault private directory");
        symlink(&outside_dir, vault_root.join(".rista/plugins"))
            .expect("create vault plugin directory symlink");

        let registry = Registry::load(&linked_dir, None);
        let real_registry = Registry::load(&real_dir, None);
        let missing_user_dir = temp.path().join("missing");
        let vault_registry = Registry::load(&missing_user_dir, Some(&vault_root));

        assert!(registry.plugins.is_empty());
        assert_eq!(real_registry.plugins.len(), 1);
        assert!(vault_registry.plugins.is_empty());
    }

    #[test]
    fn loading_a_manifest_never_executes_its_launcher() {
        let temp = TempDir::new();
        let user_dir = temp.path().join("plugins");
        fs::create_dir_all(&user_dir).expect("create plugin directory");
        let marker = temp.path().join("launcher-was-run");
        let manifest = Manifest {
            api_version: 1,
            id: "declarative".to_string(),
            name: Localized {
                en: "Declarative".to_string(),
                nb: "Deklarativ".to_string(),
            },
            commands: vec![Launcher {
                id: "touch-marker".to_string(),
                name: Localized {
                    en: "Touch marker".to_string(),
                    nb: "Lag markør".to_string(),
                },
                program: "/usr/bin/touch".to_string(),
                args: vec![marker.to_string_lossy().into_owned()],
                working_directory: WorkingDirectory::Vault,
            }],
            agents: Vec::new(),
        };
        write_manifest(&user_dir.join("declarative.json"), &manifest);

        let registry = Registry::load(&user_dir, None);

        assert_eq!(registry.plugins.len(), 1);
        assert!(!marker.exists());
    }

    #[test]
    fn bundled_launchers_are_declarative_and_have_both_locales() {
        let launchers = bundled_launchers();
        let names = launchers
            .iter()
            .map(|launcher| launcher.program.as_str())
            .collect::<Vec<_>>();

        assert_eq!(names, ["claude", "codex", "vibe", "pi", "omp", "opencode"]);
        assert!(launchers
            .iter()
            .all(|launcher| launcher.name.en == launcher.name.nb && launcher.args.is_empty()));
    }

    #[test]
    fn localized_text_selects_the_requested_language() {
        let localized = Localized {
            en: "English".to_string(),
            nb: "Norsk".to_string(),
        };

        assert_eq!(
            localized.text(crate::settings::Language::English),
            "English"
        );
        assert_eq!(
            localized.text(crate::settings::Language::Norwegian),
            "Norsk"
        );
    }

    #[test]
    fn user_directory_uses_the_settings_configuration_root() {
        assert_eq!(user_dir(), crate::settings::config_dir().join("plugins"));
    }
}
