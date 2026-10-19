use crate::search_service::{SearchOptions, SearchService, SearchSort};
use crate::settings::{Language, Settings};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const MAX_NOTE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
const MAX_URL_BYTES: usize = 64 * 1024;
const MAX_LINK_SCAN_BYTES: usize = 16 * 1024 * 1024;
const SKIP_DIRS: &[&str] = &[".git", ".rista", "node_modules", "target", "dist", ".build"];

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) enum RequestOrigin {
    Terminal,
    Url,
    Legacy,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) enum Command {
    Open {
        vault: PathBuf,
        note: Option<PathBuf>,
    },
    OpenPaths {
        paths: Vec<PathBuf>,
    },
    New {
        vault: PathBuf,
        name: PathBuf,
        template: Option<String>,
    },
    Daily {
        vault: PathBuf,
    },
    Append {
        vault: PathBuf,
        note: PathBuf,
        text: Option<String>,
    },
    Inbox {
        vault: PathBuf,
        text: Option<String>,
    },
    Search {
        vault: PathBuf,
        query: String,
    },
    Links {
        vault: Option<PathBuf>,
        note: PathBuf,
    },
    Backlinks {
        vault: Option<PathBuf>,
        note: PathBuf,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct CliRequest {
    pub(crate) command: Command,
    pub(crate) origin: RequestOrigin,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct CliResponse {
    pub(crate) result: Result<Vec<String>, CliError>,
}

impl CliResponse {
    pub(crate) fn success(lines: Vec<String>) -> Self {
        Self { result: Ok(lines) }
    }

    pub(crate) fn failure(error: CliError) -> Self {
        Self { result: Err(error) }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct CliError {
    pub(crate) english: String,
    pub(crate) norwegian: String,
}

impl CliError {
    pub(crate) fn new(english: impl Into<String>, norwegian: impl Into<String>) -> Self {
        Self {
            english: english.into(),
            norwegian: norwegian.into(),
        }
    }

    pub(crate) fn message(&self, language: Language) -> &str {
        match language {
            Language::English => &self.english,
            Language::Norwegian => &self.norwegian,
        }
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.english)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Startup {
    Help,
    Version,
    Headless(CliRequest),
    Gui(Vec<CliRequest>),
}

pub(crate) fn validate_gui_startup(requests: &[CliRequest]) -> Result<(), CliError> {
    for request in requests {
        if let (RequestOrigin::Terminal, Command::Open { vault, note }) =
            (&request.origin, &request.command)
        {
            let (root, note) = resolve_open_target(vault, note.as_deref())?;
            validate_readable_path(&root)?;
            if let Some(note) = note {
                validate_readable_path(&note)?;
            }
        }
    }
    Ok(())
}

fn validate_readable_path(path: &Path) -> Result<(), CliError> {
    if path.is_dir() {
        let mut entries = fs::read_dir(path).map_err(io_error)?;
        if let Some(entry) = entries.next() {
            entry.map_err(io_error)?;
        }
    } else {
        let mut file = fs::File::open(path).map_err(io_error)?;
        let mut byte = [0u8; 1];
        file.read(&mut byte).map_err(io_error)?;
    }
    Ok(())
}

pub(crate) fn validate_open_path(path: &Path) -> Result<PathBuf, CliError> {
    let path = path.canonicalize().map_err(io_error)?;
    validate_readable_path(&path)?;
    Ok(path)
}

pub(crate) fn help(language: Language) -> &'static str {
    match language {
        Language::English => {
            "Rísta command line\n\
             Usage: rista [PATH ...] | rista <command>\n\
             Commands:\n\
               open <vault> [note]\n\
               new <vault> <name> [--template T]\n\
               daily <vault>\n\
               append <vault> <note> [text|-]\n\
               inbox <vault> [text|-]\n\
               search <vault> <query>\n\
               links <note> [--vault <vault>]\n\
               backlinks <note> [--vault <vault>]\n\
             Omitted append or inbox text or '-' reads at most 2 MiB from stdin.\n\
             Read-only search and link results describe files on disk.\n\
             Use '--' before a path named like a command."
        }
        Language::Norwegian => {
            "Rísta kommandolinje\n\
             Bruk: rista [STI ...] | rista <kommando>\n\
             Kommandoer:\n\
               open <hvelv> [notat]\n\
               new <hvelv> <navn> [--template M]\n\
               daily <hvelv>\n\
               append <hvelv> <notat> [tekst|-]\n\
               inbox <hvelv> [tekst|-]\n\
               search <hvelv> <søk>\n\
               links <notat> [--vault <hvelv>]\n\
               backlinks <notat> [--vault <hvelv>]\n\
             Utelatt append- eller inbox-tekst eller '-' leser maksimalt 2 MiB fra stdin.\n\
             Søke- og lenkeresultater viser filer på disk.\n\
             Bruk '--' foran en sti med navn som en kommando."
        }
    }
}

pub(crate) fn parse_argv(args: Vec<OsString>) -> Result<Startup, CliError> {
    let Some(first) = args.first().and_then(|arg| arg.to_str()) else {
        if args.is_empty() {
            return Ok(Startup::Gui(Vec::new()));
        }
        return Err(CliError::new(
            "Command name must be valid UTF-8",
            "Kommandonavnet må være gyldig UTF-8",
        ));
    };

    if first == "--help" || first == "-h" {
        return Ok(Startup::Help);
    }
    if first == "--version" || first == "-V" {
        return Ok(Startup::Version);
    }
    if first == "--" {
        return Ok(Startup::Gui(vec![CliRequest {
            command: Command::OpenPaths {
                paths: args.into_iter().skip(1).map(PathBuf::from).collect(),
            },
            origin: RequestOrigin::Legacy,
        }]));
    }

    match first {
        "open" | "new" | "daily" | "append" | "inbox" | "search" | "links" | "backlinks" => {
            let command = parse_command(first, &args[1..])?;
            let request = CliRequest {
                command,
                origin: RequestOrigin::Terminal,
            };
            return Ok(
                if is_read_only(&request.command)
                    || matches!(
                        &request.command,
                        Command::New { .. }
                            | Command::Daily { .. }
                            | Command::Append { .. }
                            | Command::Inbox { .. }
                    )
                {
                    Startup::Headless(request)
                } else {
                    Startup::Gui(vec![request])
                },
            );
        }
        _ => {}
    }

    let mut requests = Vec::new();
    let mut paths = Vec::new();
    for arg in args {
        let value = arg.to_str();
        if let Some(value) = value.filter(|value| value.starts_with("rista:")) {
            requests.push(parse_url(value)?);
        } else if let Some(value) = value.filter(|value| value.starts_with("file:")) {
            paths.push(parse_file_url(value)?);
        } else {
            paths.push(PathBuf::from(arg));
        }
    }
    if !paths.is_empty() {
        requests.push(CliRequest {
            command: Command::OpenPaths { paths },
            origin: RequestOrigin::Legacy,
        });
    }
    Ok(Startup::Gui(requests))
}

fn parse_command(command: &str, args: &[OsString]) -> Result<Command, CliError> {
    let arity = |min: usize, max: usize| {
        if (min..=max).contains(&args.len()) {
            Ok(())
        } else {
            Err(CliError::new(
                format!("Invalid arguments for '{command}'. Run 'rista --help'."),
                format!("Ugyldige argumenter for '{command}'. Kjør 'rista --help'."),
            ))
        }
    };
    let text_arg = |index: usize| {
        args.get(index)
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                CliError::new(
                    "Argument must be valid UTF-8",
                    "Argumentet må være gyldig UTF-8",
                )
            })
    };
    let path_arg = |index: usize| {
        args.get(index).map(PathBuf::from).ok_or_else(|| {
            CliError::new(
                format!("Missing argument for '{command}'"),
                format!("Mangler argument for '{command}'"),
            )
        })
    };

    match command {
        "open" => {
            arity(1, 2)?;
            Ok(Command::Open {
                vault: path_arg(0)?,
                note: args.get(1).map(PathBuf::from),
            })
        }
        "new" => {
            if args.len() < 2 {
                return Err(CliError::new(
                    "Usage: rista new <vault> <name> [--template T]",
                    "Bruk: rista new <hvelv> <navn> [--template M]",
                ));
            }
            let mut template = None;
            let mut index = 2;
            while index < args.len() {
                if text_arg(index)? != "--template" || template.is_some() || index + 1 >= args.len()
                {
                    return Err(CliError::new(
                        "Expected one '--template T' option",
                        "Forventet ett '--template M'-valg",
                    ));
                }
                template = Some(text_arg(index + 1)?.to_owned());
                index += 2;
            }
            Ok(Command::New {
                vault: path_arg(0)?,
                name: path_arg(1)?,
                template,
            })
        }
        "daily" => {
            arity(1, 1)?;
            Ok(Command::Daily {
                vault: path_arg(0)?,
            })
        }
        "append" => {
            arity(2, 3)?;
            let text = args
                .get(2)
                .map(|_| text_arg(2).map(str::to_owned))
                .transpose()?
                .filter(|text| text != "-");
            Ok(Command::Append {
                vault: path_arg(0)?,
                note: path_arg(1)?,
                text,
            })
        }
        "inbox" => {
            arity(1, 2)?;
            let text = args
                .get(1)
                .map(|_| text_arg(1).map(str::to_owned))
                .transpose()?
                .filter(|text| text != "-");
            Ok(Command::Inbox {
                vault: path_arg(0)?,
                text,
            })
        }
        "search" => {
            arity(2, 2)?;
            Ok(Command::Search {
                vault: path_arg(0)?,
                query: text_arg(1)?.to_owned(),
            })
        }
        "links" | "backlinks" => {
            arity(1, 3)?;
            let mut vault = None;
            let mut index = 1;
            while index < args.len() {
                if text_arg(index)? != "--vault" || vault.is_some() || index + 1 >= args.len() {
                    return Err(CliError::new(
                        "Expected one '--vault <vault>' option",
                        "Forventet ett '--vault <hvelv>'-valg",
                    ));
                }
                vault = Some(PathBuf::from(&args[index + 1]));
                index += 2;
            }
            let note = path_arg(0)?;
            if command == "links" {
                Ok(Command::Links { vault, note })
            } else {
                Ok(Command::Backlinks { vault, note })
            }
        }
        _ => Err(CliError::new(
            "Unknown command. Run 'rista --help'.",
            "Ukjent kommando. Kjør 'rista --help'.",
        )),
    }
}

pub(crate) fn parse_url(value: &str) -> Result<CliRequest, CliError> {
    if value.len() > MAX_URL_BYTES {
        return Err(CliError::new(
            "The Rísta URL exceeds the 64 KiB limit",
            "Rísta-URL-en overskrider grensen på 64 KiB",
        ));
    }
    let url = url::Url::parse(value)
        .map_err(|_| CliError::new("The Rísta URL is invalid", "Rísta-URL-en er ugyldig"))?;
    if url.scheme() != "rista"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || !url.path().is_empty()
    {
        return Err(CliError::new(
            "The Rísta URL contains an unsupported authority or fragment",
            "Rísta-URL-en inneholder en ugyldig autoritet eller fragment",
        ));
    }
    let command = url.host_str().ok_or_else(|| {
        CliError::new(
            "The Rísta URL is missing an action",
            "Rísta-URL-en mangler en handling",
        )
    })?;
    let raw_query = url.query().unwrap_or_default();
    let mut fields = HashMap::new();
    for pair in raw_query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode_query_component(key)?;
        let value = decode_query_component(value)?;
        if fields.insert(key.clone(), value).is_some() {
            return Err(CliError::new(
                format!("Duplicate URL field '{key}'"),
                format!("Duplisert URL-felt '{key}'"),
            ));
        }
    }
    let allowed: &[&str] = match command {
        "open" => &["vault", "file"],
        "new" => &["vault", "name", "template"],
        "daily" => &["vault"],
        "append" => &["vault", "file", "text"],
        "inbox" => &["vault", "text"],
        "search" => &["vault", "query"],
        "links" | "backlinks" => &["vault", "file"],
        _ => {
            return Err(CliError::new(
                "Unknown Rísta URL action",
                "Ukjent Rísta-URL-handling",
            ))
        }
    };
    if let Some(field) = fields
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(CliError::new(
            format!("Unknown URL field '{field}'"),
            format!("Ukjent URL-felt '{field}'"),
        ));
    }
    let required = |name: &str| {
        fields.get(name).cloned().ok_or_else(|| {
            CliError::new(
                format!("Missing URL field '{name}'"),
                format!("Mangler URL-felt '{name}'"),
            )
        })
    };
    let path_field = |name: &str| required(name).map(PathBuf::from);
    let parsed = match command {
        "open" => Command::Open {
            vault: path_field("vault")?,
            note: fields.get("file").map(PathBuf::from),
        },
        "new" => Command::New {
            vault: path_field("vault")?,
            name: path_field("name")?,
            template: fields.get("template").cloned(),
        },
        "daily" => Command::Daily {
            vault: path_field("vault")?,
        },
        "append" => Command::Append {
            vault: path_field("vault")?,
            note: path_field("file")?,
            text: Some(required("text")?),
        },
        "inbox" => Command::Inbox {
            vault: path_field("vault")?,
            text: Some(required("text")?),
        },
        "search" => Command::Search {
            vault: path_field("vault")?,
            query: required("query")?,
        },
        "links" => Command::Links {
            vault: Some(path_field("vault")?),
            note: path_field("file")?,
        },
        "backlinks" => Command::Backlinks {
            vault: Some(path_field("vault")?),
            note: path_field("file")?,
        },
        _ => unreachable!(),
    };
    let vault = match &parsed {
        Command::Open { vault, .. }
        | Command::New { vault, .. }
        | Command::Daily { vault }
        | Command::Append { vault, .. }
        | Command::Inbox { vault, .. }
        | Command::Search { vault, .. } => Some(vault),
        Command::Links { vault, .. } | Command::Backlinks { vault, .. } => vault.as_ref(),
        Command::OpenPaths { .. } => None,
    };
    if vault.is_some_and(|vault| vault.as_os_str().is_empty() || !vault.is_absolute()) {
        return Err(CliError::new(
            "The URL vault must be an absolute local path",
            "Hvelvstien i URL-en må være en absolutt lokal sti",
        ));
    }
    Ok(CliRequest {
        command: parsed,
        origin: RequestOrigin::Url,
    })
}

pub(crate) fn parse_file_url(value: &str) -> Result<PathBuf, CliError> {
    let url =
        url::Url::parse(value).map_err(|_| CliError::new("Invalid file URL", "Ugyldig fil-URL"))?;
    if url.scheme() != "file"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(CliError::new(
            "Unsupported file URL",
            "Fil-URL-en støttes ikke",
        ));
    }
    url.to_file_path()
        .map_err(|_| CliError::new("Invalid local file URL", "Ugyldig lokal fil-URL"))
}

fn decode_query_component(value: &str) -> Result<String, CliError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len()
                    || !bytes[index + 1].is_ascii_hexdigit()
                    || !bytes[index + 2].is_ascii_hexdigit()
                {
                    return Err(CliError::new(
                        "Malformed percent-encoding in URL",
                        "Ugyldig prosentkoding i URL",
                    ));
                }
                let high = (bytes[index + 1] as char).to_digit(16).unwrap();
                let low = (bytes[index + 2] as char).to_digit(16).unwrap();
                decoded.push((high * 16 + low) as u8);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| {
        CliError::new(
            "The Rísta URL contains a query value that is not valid UTF-8",
            "Rísta-URL-en inneholder en spørringsverdi som ikke er gyldig UTF-8",
        )
    })
}

pub(crate) fn normalize_request_paths(request: &CliRequest) -> CliRequest {
    let mut normalized = request.clone();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let absolute = |path: &Path| {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        canonicalize_existing_ancestor(&path).unwrap_or(path)
    };
    match &mut normalized.command {
        Command::Open { vault, .. }
        | Command::New { vault, .. }
        | Command::Daily { vault }
        | Command::Append { vault, .. }
        | Command::Inbox { vault, .. }
        | Command::Search { vault, .. } => *vault = absolute(vault),
        Command::Links { vault, .. } | Command::Backlinks { vault, .. } => {
            *vault = Some(
                vault
                    .as_deref()
                    .map(absolute)
                    .unwrap_or_else(|| absolute(&cwd)),
            );
        }
        Command::OpenPaths { paths } => {
            for path in paths {
                *path = absolute(path);
            }
        }
    }
    normalized
}

pub(crate) fn is_read_only(command: &Command) -> bool {
    matches!(
        command,
        Command::Search { .. } | Command::Links { .. } | Command::Backlinks { .. }
    )
}

pub(crate) fn preflight_open_buffer(
    dirty: bool,
    conflicted: bool,
    editor_bytes: &[u8],
    disk_bytes: &[u8],
) -> Result<(), CliError> {
    if dirty || conflicted || editor_bytes != disk_bytes {
        return Err(CliError::new(
            "The note has unsaved or conflicting edits in Rísta; save or close it before writing",
            "Notatet har ulagrede eller motstridende endringer i Rísta; lagre eller lukk det før du skriver",
        ));
    }
    Ok(())
}

pub(crate) fn run_read_only(command: &Command) -> Result<Vec<String>, CliError> {
    match command {
        Command::Search { vault, query } if query.len() <= MAX_NOTE_BYTES => search(vault, query),
        Command::Search { .. } => Err(size_error()),
        Command::Links { vault, note } => links(vault.as_deref(), note),
        Command::Backlinks { vault, note } => backlinks(vault.as_deref(), note),
        _ => Err(CliError::new(
            "This command is not a read-only query",
            "Denne kommandoen er ikke et skrivebeskyttet søk",
        )),
    }
}

fn search(vault: &Path, query: &str) -> Result<Vec<String>, CliError> {
    let root = canonical_vault(vault)?;
    let paths = walk_files(&root)?;
    for path in &paths {
        if is_markdown_or_base(path) {
            read_bounded_note(path)?;
        }
    }
    let response = SearchService::new()
        .search(
            &root,
            &paths,
            query,
            SearchOptions {
                match_case: false,
                sort: SearchSort::Filename,
                limit: usize::MAX,
            },
        )
        .map_err(|error| {
            CliError::new(
                format!("Search failed: {}", error.message(false)),
                format!("Søket mislyktes: {}", error.message(true)),
            )
        })?;
    Ok(response
        .results
        .into_iter()
        .map(|result| display_path(&result.path))
        .collect())
}

fn links(vault: Option<&Path>, note: &Path) -> Result<Vec<String>, CliError> {
    let root = canonical_vault(vault.unwrap_or(Path::new(".")))?;
    let target = resolve_note(&root, note)?;
    let index = NoteIndex::load(&root)?;
    let mut resolved = Vec::new();
    for (url, wiki) in crate::vault::local_link_targets(&read_bounded_note(&target)?) {
        let candidate = if wiki {
            index.resolve_wiki(&url)?
        } else {
            index.resolve_markdown(&url, target.parent().unwrap_or(&root))?
        };
        if let Some(path) = candidate {
            resolved.push(path);
        }
    }
    sort_dedup_paths(&mut resolved);
    Ok(resolved.iter().map(|path| display_path(path)).collect())
}

fn backlinks(vault: Option<&Path>, note: &Path) -> Result<Vec<String>, CliError> {
    let root = canonical_vault(vault.unwrap_or(Path::new(".")))?;
    let target = resolve_note(&root, note)?;
    let index = NoteIndex::load(&root)?;
    let mut sources = Vec::new();
    for (source, text, targets) in &index.notes {
        if source == &target {
            continue;
        }
        for (url, wiki) in targets {
            let candidate = if *wiki {
                index.resolve_wiki(url)?
            } else {
                index.resolve_markdown(url, source.parent().unwrap_or(&root))?
            };
            if candidate.as_deref() == Some(target.as_path()) {
                sources.push(source.clone());
                break;
            }
        }
        let _ = text;
    }
    sort_dedup_paths(&mut sources);
    Ok(sources.iter().map(|path| display_path(path)).collect())
}

pub(crate) fn canonical_vault(vault: &Path) -> Result<PathBuf, CliError> {
    let root = vault.canonicalize().map_err(|error| {
        CliError::new(
            format!("Could not open vault '{}': {error}", vault.display()),
            format!("Kunne ikke åpne hvelvet '{}': {error}", vault.display()),
        )
    })?;
    if !root.is_dir() {
        return Err(CliError::new(
            format!("Vault is not a folder: {}", root.display()),
            format!("Hvelvet er ikke en mappe: {}", root.display()),
        ));
    }
    Ok(root)
}

pub(crate) fn resolve_open_target(
    vault: &Path,
    note: Option<&Path>,
) -> Result<(PathBuf, Option<PathBuf>), CliError> {
    let root = canonical_vault(vault)?;
    let note = note.map(|note| resolve_note(&root, note)).transpose()?;
    Ok((root, note))
}

fn resolve_note(root: &Path, note: &Path) -> Result<PathBuf, CliError> {
    let path = if note.is_absolute() {
        note.canonicalize().map_err(|error| {
            CliError::new(
                format!("Could not read note '{}': {error}", note.display()),
                format!("Kunne ikke lese notatet '{}': {error}", note.display()),
            )
        })?
    } else {
        note_path(root, note, false)?
            .canonicalize()
            .map_err(|error| {
                CliError::new(
                    format!("Could not read note '{}': {error}", note.display()),
                    format!("Kunne ikke lese notatet '{}': {error}", note.display()),
                )
            })?
    };
    if !path.starts_with(root) || !path.is_file() || !is_markdown(&path) {
        return Err(CliError::new(
            "Note must be a Markdown file inside the vault",
            "Notatet må være en Markdown-fil i hvelvet",
        ));
    }
    Ok(path)
}

fn note_path(root: &Path, note: &Path, add_extension: bool) -> Result<PathBuf, CliError> {
    if note.components().any(|part| {
        matches!(
            part,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) && !note.is_absolute()
    {
        return Err(CliError::new(
            "Note path must not traverse outside the vault",
            "Notatstien kan ikke gå utenfor hvelvet",
        ));
    }
    let mut note = if note.is_absolute() {
        if note
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(CliError::new(
                "Note path must not traverse outside the vault",
                "Notatstien kan ikke gå utenfor hvelvet",
            ));
        }
        let canonical = canonicalize_existing_ancestor(note)?;
        canonical
            .strip_prefix(root)
            .map(Path::to_path_buf)
            .map_err(|_| {
                CliError::new(
                    "Note path is outside the vault",
                    "Notatstien er utenfor hvelvet",
                )
            })?
    } else {
        note.to_path_buf()
    };
    if note.components().any(|part| {
        matches!(
            part,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(CliError::new(
            "Note path must not traverse outside the vault",
            "Notatstien kan ikke gå utenfor hvelvet",
        ));
    }
    if add_extension && note.extension().is_none() {
        note.set_extension("md");
    }
    Ok(root.join(note))
}

fn canonicalize_existing_ancestor(path: &Path) -> Result<PathBuf, CliError> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    let canonical = loop {
        match ancestor.canonicalize() {
            Ok(path) => break path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let Some(name) = ancestor.file_name() else {
                    return Err(io_error(error));
                };
                missing.push(name.to_os_string());
                let Some(parent) = ancestor.parent() else {
                    return Err(io_error(error));
                };
                ancestor = parent;
            }
            Err(error) => return Err(io_error(error)),
        }
    };
    Ok(missing
        .into_iter()
        .rev()
        .fold(canonical, |path, component| path.join(component)))
}

fn walk_files(root: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut out = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = pending.pop() {
        let canonical = dir.canonicalize().map_err(|error| {
            CliError::new(
                format!("Could not scan '{}': {error}", dir.display()),
                format!("Kunne ikke skanne '{}': {error}", dir.display()),
            )
        })?;
        if !canonical.starts_with(root) || !visited.insert(canonical) {
            continue;
        }
        let entries = fs::read_dir(&dir).map_err(|error| {
            CliError::new(
                format!("Could not scan '{}': {error}", dir.display()),
                format!("Kunne ikke skanne '{}': {error}", dir.display()),
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                CliError::new(
                    format!("Could not scan vault entry: {error}"),
                    format!("Kunne ikke skanne oppføring i hvelvet: {error}"),
                )
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                CliError::new(
                    format!("Could not inspect '{}': {error}", path.display()),
                    format!("Kunne ikke undersøke '{}': {error}", path.display()),
                )
            })?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || SKIP_DIRS.contains(&name.as_ref()) {
                    continue;
                }
                pending.push(path);
            } else if metadata.is_file() {
                out.push(path);
            }
        }
    }
    out.sort_by_key(|left| relative_key(root, left));
    Ok(out)
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

fn is_markdown_or_base(path: &Path) -> bool {
    is_markdown(path)
        || path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("base"))
}

fn read_bounded_note(path: &Path) -> Result<String, CliError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| {
            CliError::new(
                format!("Could not read '{}': {error}", path.display()),
                format!("Kunne ikke lese '{}': {error}", path.display()),
            )
        })?
        .take((MAX_NOTE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CliError::new(
                format!("Could not read '{}': {error}", path.display()),
                format!("Kunne ikke lese '{}': {error}", path.display()),
            )
        })?;
    if bytes.len() > MAX_NOTE_BYTES {
        return Err(CliError::new(
            format!("Note exceeds the 2 MiB limit: {}", path.display()),
            format!("Notatet overskrider grensen på 2 MiB: {}", path.display()),
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        CliError::new(
            format!("Note is not valid UTF-8: {}", path.display()),
            format!("Notatet er ikke gyldig UTF-8: {}", path.display()),
        )
    })
}

fn relative_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn sort_dedup_paths(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}

type LinkTarget = (String, bool);
type IndexedNote = (PathBuf, String, Vec<LinkTarget>);

struct NoteIndex {
    root: PathBuf,
    notes: Vec<IndexedNote>,
    by_relative: HashMap<String, Vec<PathBuf>>,
    by_basename: HashMap<String, Vec<PathBuf>>,
    aliases: HashMap<String, Vec<PathBuf>>,
}

impl NoteIndex {
    fn load(root: &Path) -> Result<Self, CliError> {
        let paths = walk_files(root)?
            .into_iter()
            .filter(|path| is_markdown(path))
            .collect::<Vec<_>>();
        let mut total = 0usize;
        let mut notes = Vec::with_capacity(paths.len());
        let mut by_relative: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut by_basename: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut aliases: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in paths {
            let text = read_bounded_note(&path)?;
            total = total.checked_add(text.len()).ok_or_else(|| {
                CliError::new(
                    "Link scan exceeds the 16 MiB limit",
                    "Lenkesøket overskrider grensen på 16 MiB",
                )
            })?;
            if total > MAX_LINK_SCAN_BYTES {
                return Err(CliError::new(
                    "Link scan exceeds the 16 MiB limit",
                    "Lenkesøket overskrider grensen på 16 MiB",
                ));
            }
            let key = relative_key(root, &path).to_lowercase();
            insert_candidate(&mut by_relative, key.clone(), path.clone());
            insert_candidate(
                &mut by_relative,
                key.strip_suffix(".markdown")
                    .or_else(|| key.strip_suffix(".md"))
                    .unwrap_or(&key)
                    .to_owned(),
                path.clone(),
            );
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                insert_candidate(&mut by_basename, stem.to_lowercase(), path.clone());
                if path.extension().is_some() {
                    insert_candidate(
                        &mut by_basename,
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_lowercase(),
                        path.clone(),
                    );
                }
            }
            for alias in crate::properties::frontmatter_aliases(&text) {
                insert_candidate(&mut aliases, alias.to_lowercase(), path.clone());
            }
            let targets = crate::vault::local_link_targets(&text);
            notes.push((path, text, targets));
        }
        Ok(Self {
            root: root.to_path_buf(),
            notes,
            by_relative,
            by_basename,
            aliases,
        })
    }

    fn resolve_wiki(&self, target: &str) -> Result<Option<PathBuf>, CliError> {
        let raw = target
            .split(['#', '?', '^', '|'])
            .next()
            .unwrap_or(target)
            .trim()
            .trim_start_matches('/');
        if raw.is_empty() || raw.starts_with("//") {
            return Ok(None);
        }
        let decoded = self.decode_relative(&self.root, raw)?;
        if !decoded.starts_with(&self.root) {
            return Err(CliError::new(
                "Local wiki link escapes the vault",
                "Den lokale wikilenken går utenfor hvelvet",
            ));
        }
        let relative = relative_key(&self.root, &decoded).to_lowercase();
        let mut candidates = lookup_candidates(&self.by_relative, &relative);
        if candidates.is_empty() && Path::new(raw).extension().is_none() {
            candidates = lookup_candidates(&self.by_relative, &format!("{relative}.md"));
            if candidates.is_empty() {
                candidates = lookup_candidates(&self.by_relative, &format!("{relative}.markdown"));
            }
        }
        if let Some(path) = one_candidate(candidates, "Wiki target", "Wikimål")? {
            return Ok(Some(path));
        }
        let basename = decoded
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let stem = Path::new(&basename)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let candidates = lookup_candidates(&self.by_basename, &basename)
            .into_iter()
            .chain(lookup_candidates(&self.by_basename, &stem))
            .collect();
        if let Some(path) = one_candidate(candidates, "Wiki basename", "Wikinavn")? {
            return Ok(Some(path));
        }
        let alias = raw.to_lowercase();
        let candidates = lookup_candidates(&self.aliases, &alias)
            .into_iter()
            .chain(lookup_candidates(&self.aliases, &stem))
            .collect();
        one_candidate(candidates, "Wiki alias", "Wiki-alias")
    }

    fn resolve_markdown(&self, target: &str, from_dir: &Path) -> Result<Option<PathBuf>, CliError> {
        let raw = target.trim();
        let raw = raw
            .strip_prefix('<')
            .and_then(|inner| inner.strip_suffix('>'))
            .unwrap_or(raw);
        if raw.is_empty()
            || raw.starts_with('#')
            || raw.starts_with("//")
            || raw.starts_with("mailto:")
        {
            return Ok(None);
        }
        if let Ok(parsed) = url::Url::parse(raw) {
            if parsed.scheme() != "file" {
                return Ok(None);
            }
            let Ok(path) = parsed.to_file_path() else {
                return Ok(None);
            };
            return self.resolve_markdown_path(path);
        }
        if raw.contains(':') {
            return Ok(None);
        }
        let path_only = raw.split(['?', '#']).next().unwrap_or(raw);
        let candidate = self.decode_relative(from_dir, path_only)?;
        if !candidate.starts_with(&self.root) {
            return Err(CliError::new(
                "Local link escapes the vault",
                "Den lokale lenken går utenfor hvelvet",
            ));
        }
        self.resolve_markdown_path(candidate)
    }

    fn decode_relative(&self, base: &Path, target: &str) -> Result<PathBuf, CliError> {
        let base_url = url::Url::from_directory_path(base).map_err(|_| {
            CliError::new(
                "Could not resolve a local link",
                "Kunne ikke løse en lokal lenke",
            )
        })?;
        let url = base_url.join(target).map_err(|_| {
            CliError::new(
                format!("Invalid local link target: {target}"),
                format!("Ugyldig lokal lenkemål: {target}"),
            )
        })?;
        if url.scheme() != "file" {
            return Err(CliError::new(
                "Only local file links are supported",
                "Bare lokale fillenker støttes",
            ));
        }
        url.to_file_path().map_err(|_| {
            CliError::new(
                "Could not decode a local link path",
                "Kunne ikke dekode en lokal lenkesti",
            )
        })
    }

    fn resolve_markdown_path(&self, candidate: PathBuf) -> Result<Option<PathBuf>, CliError> {
        if !is_markdown(&candidate) {
            return Ok(None);
        }
        let canonical = match candidate.canonicalize() {
            Ok(path) => path,
            Err(_) => return Ok(None),
        };
        if !canonical.starts_with(&self.root) {
            return Err(CliError::new(
                "Local link escapes the vault",
                "Den lokale lenken går utenfor hvelvet",
            ));
        }
        if !canonical.is_file() {
            return Ok(None);
        }
        Ok(self
            .notes
            .iter()
            .find(|(path, _, _)| path == &canonical)
            .map(|(path, _, _)| path.clone()))
    }
}

fn insert_candidate(map: &mut HashMap<String, Vec<PathBuf>>, key: String, path: PathBuf) {
    let candidates = map.entry(key).or_default();
    if !candidates.contains(&path) {
        candidates.push(path);
    }
}

fn lookup_candidates(map: &HashMap<String, Vec<PathBuf>>, key: &str) -> Vec<PathBuf> {
    map.get(key).cloned().unwrap_or_default()
}

fn one_candidate(
    mut candidates: Vec<PathBuf>,
    english_context: &str,
    norwegian_context: &str,
) -> Result<Option<PathBuf>, CliError> {
    sort_dedup_paths(&mut candidates);
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.pop()),
        _ => Err(CliError::new(
            format!("{english_context} is ambiguous"),
            format!("{norwegian_context} er tvetydig"),
        )),
    }
}

pub(crate) fn read_stdin_limited(input: &mut impl Read) -> Result<String, CliError> {
    let mut bytes = Vec::new();
    input
        .take((MAX_NOTE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CliError::new(
                format!("Could not read standard input: {error}"),
                format!("Kunne ikke lese standard input: {error}"),
            )
        })?;
    if bytes.len() > MAX_NOTE_BYTES {
        return Err(CliError::new(
            "Standard input exceeds the 2 MiB limit",
            "Standard input overskrider grensen på 2 MiB",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        CliError::new(
            "Standard input is not valid UTF-8",
            "Standard input er ikke gyldig UTF-8",
        )
    })
}

pub(crate) fn resolve_stdin(
    request: &mut CliRequest,
    input: &mut impl Read,
) -> Result<(), CliError> {
    if let Command::Append { text, .. } | Command::Inbox { text, .. } = &mut request.command {
        if text.is_none() {
            *text = Some(read_stdin_limited(input)?);
        }
    }
    Ok(())
}

pub(crate) fn settings_language(settings: &Settings) -> Language {
    settings.language
}

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_destination(
    command: &Command,
    settings: &Settings,
    now: NaiveDateTime,
) -> Result<(PathBuf, PathBuf), CliError> {
    let (vault, relative, allow_file) = match command {
        Command::New { vault, name, .. } => (vault, name, true),
        Command::Daily { vault } => {
            let name = crate::bases::format_moment(now, &settings.daily_format);
            let mut relative = PathBuf::from(&settings.daily_dir);
            relative.push(name);
            let root = canonical_vault(vault)?;
            let destination = note_path(&root, &relative, true)?;
            validate_markdown_destination(&destination)?;
            let allow_file = fs::symlink_metadata(&destination).is_ok();
            validate_destination(&root, &destination, allow_file)?;
            return Ok((root, destination));
        }
        Command::Append { vault, note, .. } => (vault, note, true),
        Command::Inbox { vault, text } => {
            let root = canonical_vault(vault)?;
            let destination = inbox_destination(&root, text.as_deref(), now)?;
            return Ok((root, destination));
        }
        _ => {
            return Err(CliError::new(
                "This command does not write a note",
                "Denne kommandoen skriver ikke et notat",
            ))
        }
    };
    let root = canonical_vault(vault)?;
    let destination = note_path(&root, relative, true)?;
    validate_markdown_destination(&destination)?;
    validate_destination(&root, &destination, allow_file)?;
    Ok((root, destination))
}

pub(crate) fn execute_write(
    command: &Command,
    settings: &Settings,
    now: NaiveDateTime,
    stdin: &mut impl Read,
) -> Result<PathBuf, CliError> {
    match command {
        Command::New {
            vault,
            name,
            template,
        } => {
            let root = canonical_vault(vault)?;
            let destination = note_path(&root, name, true)?;
            validate_markdown_destination(&destination)?;
            validate_destination(&root, &destination, false)?;
            let text = match template {
                Some(template) => {
                    let template_path =
                        configured_template_path(&root, &settings.templates_dir, template, false)?;
                    let template_text = read_bounded_note(&template_path)?;
                    expand_cli_template(&root, &destination, &template_text, now)?
                }
                None => String::new(),
            };
            create_parent_directories(&root, &destination)?;
            create_exclusive(&destination, text.as_bytes())?;
            Ok(destination)
        }
        Command::Daily { vault } => {
            let root = canonical_vault(vault)?;
            let name = crate::bases::format_moment(now, &settings.daily_format);
            let mut relative = PathBuf::from(&settings.daily_dir);
            relative.push(name);
            let destination = note_path(&root, &relative, true)?;
            validate_markdown_destination(&destination)?;
            if fs::symlink_metadata(&destination).is_ok() {
                validate_destination(&root, &destination, true)?;
                return Ok(destination);
            }
            validate_destination(&root, &destination, false)?;
            let template_path =
                configured_template_path(&root, &settings.templates_dir, "daily.md", true)?;
            let template_text = match fs::symlink_metadata(&template_path) {
                Ok(_) => read_bounded_note(&template_path)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
                Err(error) => return Err(io_error(error)),
            };
            let text = expand_cli_template(&root, &destination, &template_text, now)?;
            create_parent_directories(&root, &destination)?;
            create_exclusive(&destination, text.as_bytes())?;
            Ok(destination)
        }
        Command::Append { vault, note, text } => {
            let root = canonical_vault(vault)?;
            let destination = note_path(&root, note, true)?;
            validate_markdown_destination(&destination)?;
            let text = match text {
                Some(text) => text.as_bytes().to_vec(),
                None => crate::cli::read_stdin_limited(stdin)?.into_bytes(),
            };
            if text.len() > MAX_NOTE_BYTES {
                return Err(size_error());
            }
            validate_destination(&root, &destination, true)?;
            create_parent_directories(&root, &destination)?;
            append_atomically(&root, &destination, &text)?;
            Ok(destination)
        }
        Command::Inbox { vault, text } => {
            let root = canonical_vault(vault)?;
            let text = match text {
                Some(text) => text.clone(),
                None => read_stdin_limited(stdin)?,
            };
            let destination = inbox_destination(&root, Some(&text), now)?;
            create_parent_directories(&root, &destination)?;
            create_exclusive(&destination, crate::inbox::content(&text, now).as_bytes())?;
            Ok(destination)
        }
        _ => Err(CliError::new(
            "This command does not write a note",
            "Denne kommandoen skriver ikke et notat",
        )),
    }
}

pub(crate) fn execute_write_coordinated(
    request: &mut CliRequest,
    settings: &Settings,
    now: NaiveDateTime,
    stdin: &mut impl Read,
) -> Result<Vec<String>, CliError> {
    *request = normalize_request_paths(request);
    resolve_stdin(request, stdin)?;
    crate::cli_ipc::coordinate_write(&crate::settings::config_path(), request, || {
        execute_write(&request.command, settings, now, stdin)
            .map(|path| vec![path.display().to_string()])
    })
}

/// The next free capture path in `<vault>/Inbox/`, checked like any other
/// new-note destination.
fn inbox_destination(
    root: &Path,
    text: Option<&str>,
    now: NaiveDateTime,
) -> Result<PathBuf, CliError> {
    let text = text.ok_or_else(|| {
        CliError::new(
            "Inbox text was not provided",
            "Teksten til innboksen ble ikke oppgitt",
        )
    })?;
    if text.trim().is_empty() {
        return Err(CliError::new("Nothing to capture", "Ingenting å lagre"));
    }
    let destination = crate::inbox::free_path(&root.join(crate::inbox::INBOX_DIR), text, now);
    validate_destination(root, &destination, false)?;
    Ok(destination)
}

fn io_error(error: io::Error) -> CliError {
    CliError::new(
        format!("File operation failed: {error}"),
        format!("Filoperasjonen mislyktes: {error}"),
    )
}

fn size_error() -> CliError {
    CliError::new(
        "Note or appended text exceeds the 2 MiB limit",
        "Notatet eller teksten overskrider grensen på 2 MiB",
    )
}

fn validate_markdown_destination(path: &Path) -> Result<(), CliError> {
    if !is_markdown(path) {
        return Err(CliError::new(
            "Note paths must end in .md or .markdown",
            "Notatstier må ende på .md eller .markdown",
        ));
    }
    Ok(())
}

fn validate_destination(root: &Path, destination: &Path, allow_file: bool) -> Result<(), CliError> {
    let relative = destination.strip_prefix(root).map_err(|_| {
        CliError::new(
            "Destination is outside the vault",
            "Målet er utenfor hvelvet",
        )
    })?;
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(CliError::new(
                "Destination contains an unsafe path component",
                "Målet inneholder en utrygg stikomponent",
            ));
        };
        current.push(name);
        let final_component = index + 1 == components.len();
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(CliError::new(
                        "Symbolic links are not allowed in note paths",
                        "Symbolske lenker er ikke tillatt i notatstier",
                    ));
                }
                if final_component {
                    if !allow_file {
                        return Err(CliError::new(
                            "The destination already exists",
                            "Målet finnes allerede",
                        ));
                    }
                    if !metadata.is_file() {
                        return Err(CliError::new(
                            "The note destination is not a regular file",
                            "Notatmålet er ikke en vanlig fil",
                        ));
                    }
                } else if !metadata.is_dir() {
                    return Err(CliError::new(
                        "A note path parent is not a directory",
                        "En overordnet notatsti er ikke en mappe",
                    ));
                }
                let canonical = current.canonicalize().map_err(io_error)?;
                if !canonical.starts_with(root) {
                    return Err(CliError::new(
                        "A note path escapes the vault",
                        "En notatsti går utenfor hvelvet",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
    }
    Ok(())
}

fn create_parent_directories(root: &Path, destination: &Path) -> Result<(), CliError> {
    let parent = destination.parent().ok_or_else(|| {
        CliError::new(
            "The note destination has no parent directory",
            "Notatmålet mangler en overordnet mappe",
        )
    })?;
    let relative = parent.strip_prefix(root).map_err(|_| {
        CliError::new(
            "Destination is outside the vault",
            "Målet er utenfor hvelvet",
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(CliError::new(
                "Destination contains an unsafe path component",
                "Målet inneholder en utrygg stikomponent",
            ));
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(CliError::new(
                    "A note path parent is unsafe",
                    "En overordnet notatsti er utrygg",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(io_error)?;
            }
            Err(error) => return Err(io_error(error)),
        }
        let canonical = current.canonicalize().map_err(io_error)?;
        if !canonical.starts_with(root) {
            return Err(CliError::new(
                "A note path escapes the vault",
                "En notatsti går utenfor hvelvet",
            ));
        }
    }
    Ok(())
}

fn configured_template_path(
    root: &Path,
    directory: &str,
    name: &str,
    allow_missing: bool,
) -> Result<PathBuf, CliError> {
    let directory = Path::new(directory);
    let name = Path::new(name);
    if name.as_os_str().is_empty()
        || name.is_absolute()
        || directory.is_absolute()
        || name
            .components()
            .chain(directory.components())
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CliError::new(
            "Template path must stay inside the configured templates folder",
            "Malstien må være innenfor den konfigurerte malmappen",
        ));
    }
    let mut relative = directory.to_path_buf();
    relative.push(name);
    let path = note_path(root, &relative, true)?;
    validate_destination(root, &path, true)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => return Ok(path),
        Err(error) => return Err(io_error(error)),
    };
    if !metadata.is_file() || !is_markdown(&path) {
        return Err(CliError::new(
            "Template must be a regular Markdown file inside the templates folder",
            "Malen må være en vanlig Markdown-fil i malmappen",
        ));
    }
    Ok(path)
}

fn expand_cli_template(
    root: &Path,
    destination: &Path,
    source: &str,
    now: NaiveDateTime,
) -> Result<String, CliError> {
    let index = if crate::templater::include_references(source).is_empty() {
        None
    } else {
        Some(NoteIndex::load(root)?)
    };
    if crate::templater::requires_interactive_context(source) {
        return Err(CliError::new(
            "This template requires an interactive prompt, clipboard, or selection unavailable in the CLI",
            "Denne malen krever en interaktiv forespørsel, utklippstavle eller et utvalg som ikke er tilgjengelig i kommandolinjen",
        ));
    }
    if let Some(index) = &index {
        let mut pending = crate::templater::include_references(source);
        let mut visited = HashMap::<PathBuf, ()>::new();
        while let Some(reference) = pending.pop() {
            let reference = include_reference_name(&reference);
            let Some(path) = index.resolve_wiki(reference)? else {
                return Err(CliError::new(
                    format!("Template include was not found: {reference}"),
                    format!("Malinkluderingen ble ikke funnet: {reference}"),
                ));
            };
            if visited.insert(path.clone(), ()).is_none() {
                let included = index
                    .notes
                    .iter()
                    .find(|(note, _, _)| note == &path)
                    .map(|(_, text, _)| text)
                    .ok_or_else(|| {
                        CliError::new(
                            "Template include could not be read",
                            "Malinkluderingen kunne ikke leses",
                        )
                    })?;
                if crate::templater::requires_interactive_context(included) {
                    return Err(CliError::new(
                        "An included template requires an interactive prompt, clipboard, or selection unavailable in the CLI",
                        "En inkludert mal krever en interaktiv forespørsel, utklippstavle eller et utvalg som ikke er tilgjengelig i kommandolinjen",
                    ));
                }
                pending.extend(crate::templater::include_references(included));
            }
        }
    }

    let title = destination
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let relative = destination
        .strip_prefix(root)
        .map_err(|_| {
            CliError::new(
                "Destination is outside the vault",
                "Målet er utenfor hvelvet",
            )
        })?
        .to_string_lossy();
    let absolute = destination.to_string_lossy();
    let included = index.as_ref().map(|index| {
        move |name: &str| {
            let target = index
                .resolve_wiki(include_reference_name(name))
                .ok()
                .flatten()?;
            index
                .notes
                .iter()
                .find(|(path, _, _)| path == &target)
                .map(|(_, text, _)| text.clone())
        }
    });
    let mut context = crate::templater::TemplateCtx::new(title, now);
    context.rel_path = &relative;
    context.abs_path = &absolute;
    context.frontmatter = source;
    context.include = included
        .as_ref()
        .map(|include| include as &dyn Fn(&str) -> Option<String>);
    crate::templater::expand_bounded(source, &context, MAX_NOTE_BYTES)
        .map(|expanded| expanded.text)
        .map_err(|()| size_error())
}

fn include_reference_name(reference: &str) -> &str {
    let reference = reference.trim();
    let reference = reference.strip_prefix('!').unwrap_or(reference).trim();
    reference
        .strip_prefix("[[")
        .and_then(|name| name.strip_suffix("]]"))
        .unwrap_or(reference)
}

fn create_exclusive(destination: &Path, bytes: &[u8]) -> Result<(), CliError> {
    if bytes.len() > MAX_NOTE_BYTES {
        return Err(size_error());
    }
    let parent = destination.parent().ok_or_else(|| {
        CliError::new(
            "The note destination has no parent directory",
            "Notatmålet mangler en overordnet mappe",
        )
    })?;
    let temporary = temporary_path(parent);
    let mut owns_temporary = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(io_error)?;
        owns_temporary = true;
        file.write_all(bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::hard_link(&temporary, destination).map_err(io_error)?;
        let _ = fs::remove_file(&temporary);
        Ok(())
    })();
    if result.is_err() && owns_temporary {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn append_atomically(root: &Path, destination: &Path, text: &[u8]) -> Result<(), CliError> {
    let baseline = match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(CliError::new(
                    "The note is not a regular file",
                    "Notatet er ikke en vanlig fil",
                ));
            }
            let bytes = read_bounded_bytes(destination)?;
            if bytes.len() > MAX_NOTE_BYTES {
                return Err(size_error());
            }
            Some((bytes, metadata.permissions()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(io_error(error)),
    };
    let mut replacement = baseline
        .as_ref()
        .map(|(bytes, _)| bytes.clone())
        .unwrap_or_default();
    replacement.extend_from_slice(text);
    if replacement.len() > MAX_NOTE_BYTES {
        return Err(size_error());
    }
    if let Some((bytes, _)) = &baseline {
        prepare_snapshot_directory(root, destination)?;
        crate::history::snapshot_before_write_checked(root, destination, bytes, &replacement)
            .map_err(|error| {
                CliError::new(
                    format!("Could not save the pre-append history snapshot: {error}"),
                    format!("Kunne ikke lagre historikkbildet før tilføying: {error}"),
                )
            })?;
    }
    let parent = destination.parent().ok_or_else(|| {
        CliError::new(
            "The note destination has no parent directory",
            "Notatmålet mangler en overordnet mappe",
        )
    })?;
    let temporary = temporary_path(parent);
    let mut owns_temporary = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(io_error)?;
        owns_temporary = true;
        if let Some((_, permissions)) = &baseline {
            file.set_permissions(permissions.clone())
                .map_err(io_error)?;
        }
        file.write_all(&replacement).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        if let Some((bytes, _)) = &baseline {
            if read_bounded_bytes(destination)? != *bytes {
                return Err(CliError::new(
                    "The note changed on disk while appending; no changes were written",
                    "Notatet ble endret på disk under tilføying; ingen endringer ble skrevet",
                ));
            }
            fs::rename(&temporary, destination).map_err(io_error)?;
        } else {
            fs::hard_link(&temporary, destination).map_err(io_error)?;
        }
        if baseline.is_none() {
            let _ = fs::remove_file(&temporary);
        }
        Ok(())
    })();
    if result.is_err() && owns_temporary {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(crate) fn read_bounded_bytes(path: &Path) -> Result<Vec<u8>, CliError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(io_error)?
        .take((MAX_NOTE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > MAX_NOTE_BYTES {
        return Err(size_error());
    }
    Ok(bytes)
}

fn prepare_snapshot_directory(root: &Path, destination: &Path) -> Result<(), CliError> {
    let relative = destination.strip_prefix(root).map_err(|_| {
        CliError::new(
            "Destination is outside the vault",
            "Målet er utenfor hvelvet",
        )
    })?;
    let mut snapshot = root.join(".rista").join("history");
    let mut relative_dir = relative.to_path_buf();
    relative_dir.set_extension("");
    snapshot.push(relative_dir);
    let mut current = root.to_path_buf();
    for component in snapshot
        .strip_prefix(root)
        .unwrap_or(&snapshot)
        .components()
    {
        let Component::Normal(name) = component else {
            return Err(CliError::new(
                "History path contains an unsafe component",
                "Historikkstien inneholder en utrygg komponent",
            ));
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(CliError::new(
                    "History path is not a safe directory",
                    "Historikkstien er ikke en trygg mappe",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(io_error)?;
            }
            Err(error) => return Err(io_error(error)),
        }
        if !current.canonicalize().map_err(io_error)?.starts_with(root) {
            return Err(CliError::new(
                "History path escapes the vault",
                "Historikkstien går utenfor hvelvet",
            ));
        }
    }
    Ok(())
}

fn temporary_path(parent: &Path) -> PathBuf {
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".rista-cli-{}-{id}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        loop {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("rista-cli-{}-{now}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create test directory: {error}"),
            }
        }
    }

    fn request(command: Command) -> CliRequest {
        CliRequest {
            command,
            origin: RequestOrigin::Terminal,
        }
    }

    #[test]
    fn parses_commands_and_legacy_path_escape() {
        let parsed = parse_argv(vec![
            "new".into(),
            "/vault".into(),
            "Río de Janeiro".into(),
            "--template".into(),
            "daily".into(),
        ])
        .unwrap();
        assert_eq!(
            parsed,
            Startup::Headless(request(Command::New {
                vault: "/vault".into(),
                name: "Río de Janeiro".into(),
                template: Some("daily".into()),
            }))
        );
        assert_eq!(
            parse_argv(vec!["--".into(), "open".into()]).unwrap(),
            Startup::Gui(vec![CliRequest {
                command: Command::OpenPaths {
                    paths: vec![PathBuf::from("open")]
                },
                origin: RequestOrigin::Legacy,
            }])
        );
        assert!(
            parse_argv(vec!["append".into(), "/vault".into(), "note.md".into()])
                .unwrap()
                .eq(&Startup::Headless(request(Command::Append {
                    vault: "/vault".into(),
                    note: "note.md".into(),
                    text: None,
                })))
        );
    }

    #[test]
    fn parses_inbox_text_argument_stdin_marker_and_url() {
        assert_eq!(
            parse_argv(vec!["inbox".into(), "/vault".into(), "Buy milk".into()]).unwrap(),
            Startup::Headless(request(Command::Inbox {
                vault: "/vault".into(),
                text: Some("Buy milk".into()),
            }))
        );
        for args in [
            vec!["inbox".into(), "/vault".into()],
            vec!["inbox".into(), "/vault".into(), "-".into()],
        ] {
            assert_eq!(
                parse_argv(args).unwrap(),
                Startup::Headless(request(Command::Inbox {
                    vault: "/vault".into(),
                    text: None,
                }))
            );
        }
        assert!(parse_argv(vec!["inbox".into()]).is_err());
        assert!(parse_argv(vec!["inbox".into(), "/v".into(), "a".into(), "b".into()]).is_err());
        assert_eq!(
            parse_url("rista://inbox?vault=%2Ftmp%2FNotes&text=hei%20verden")
                .unwrap()
                .command,
            Command::Inbox {
                vault: "/tmp/Notes".into(),
                text: Some("hei verden".into()),
            }
        );
        assert!(parse_url("rista://inbox?vault=%2Ftmp%2FNotes").is_err());
        assert!(parse_url("rista://inbox?vault=%2Ftmp&text=x&file=a.md").is_err());
    }

    #[test]
    fn inbox_creates_one_note_per_capture_from_argument_or_stdin() {
        let root = temp_dir();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let command = Command::Inbox {
            vault: root.clone(),
            text: Some("Idea: ship it\nmore".into()),
        };
        let settings = Settings::default();
        let first = execute_write(&command, &settings, now, &mut io::Cursor::new([])).unwrap();
        let inbox = root.canonicalize().unwrap().join("Inbox");
        assert_eq!(first, inbox.join("2026-03-04 0930 Idea ship it.md"));
        assert_eq!(
            fs::read_to_string(&first).unwrap(),
            "---\ncreated: 2026-03-04T09:30:00\n---\nIdea: ship it\nmore\n"
        );
        let second = execute_write(&command, &settings, now, &mut io::Cursor::new([])).unwrap();
        assert_eq!(second, inbox.join("2026-03-04 0930 Idea ship it 2.md"));
        let piped = execute_write(
            &Command::Inbox {
                vault: root.clone(),
                text: None,
            },
            &settings,
            now,
            &mut io::Cursor::new("\nfrom stdin".as_bytes()),
        )
        .unwrap();
        assert_eq!(piped, inbox.join("2026-03-04 0930.md"));
        assert!(execute_write(
            &Command::Inbox {
                vault: root.clone(),
                text: Some("  \n".into()),
            },
            &settings,
            now,
            &mut io::Cursor::new([]),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_urls_strictly_and_decodes_unicode_and_spaces() {
        let parsed = parse_url(
            "rista://append?vault=%2Ftmp%2FNotes&file=R%C3%ADo%20de%20Janeiro.md&text=hei%20verden",
        )
        .unwrap();
        assert_eq!(
            parsed.command,
            Command::Append {
                vault: "/tmp/Notes".into(),
                note: "Río de Janeiro.md".into(),
                text: Some("hei verden".into()),
            }
        );
        assert_eq!(parsed.origin, RequestOrigin::Url);
        for invalid in [
            "rista://open?vault=/x&vault=/y",
            "rista://open?vault=/x&file=a#fragment",
            "rista://open:user@host?vault=/x",
            "rista://search?vault=/x&query=q&unexpected=true",
            "rista://daily?vault=%",
            "rista://daily?vault=relative",
            "rista://open?vault=%2Ftmp&file=%FF",
            "rista://nope?vault=/x",
        ] {
            assert!(parse_url(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn client_path_normalization_anchors_optional_vault_to_callers_cwd() {
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let normalized = normalize_request_paths(&request(Command::Links {
            vault: None,
            note: "Río plan.md".into(),
        }));
        assert_eq!(
            normalized.command,
            Command::Links {
                vault: Some(cwd.clone()),
                note: "Río plan.md".into(),
            }
        );
        let normalized = normalize_request_paths(&request(Command::Daily {
            vault: "relative-vault".into(),
        }));
        assert_eq!(
            normalized.command,
            Command::Daily {
                vault: cwd.join("relative-vault"),
            }
        );
    }

    #[test]
    fn template_names_stay_inside_the_configured_templates_root() {
        let root = canonical_vault(&temp_dir()).unwrap();
        fs::create_dir(root.join("Templates")).unwrap();
        fs::write(root.join("Templates/daily.md"), "daily").unwrap();
        assert!(configured_template_path(&root, "Templates", "daily.md", false).is_ok());
        assert!(configured_template_path(&root, "Templates", "../outside.md", false).is_err());
        assert!(configured_template_path(
            &root,
            "Templates",
            root.join("outside.md").to_str().unwrap(),
            false
        )
        .is_err());
        fs::write(root.join("root-template.md"), "root").unwrap();
        assert!(configured_template_path(&root, "", "root-template.md", false).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolves_links_and_backlinks_with_aliases_and_percent_escapes() {
        let root = temp_dir();
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(
            root.join("nested/Río plan.md"),
            "---\naliases: [Plan alternativo]\n---\n",
        )
        .unwrap();
        fs::write(
            root.join("source.md"),
            "[plan](nested/R%C3%ADo%20plan.md?view=1#top) [[nested/Río plan]] [[Plan alternativo]]",
        )
        .unwrap();
        let root = canonical_vault(&root).unwrap();
        assert_eq!(
            links(Some(&root), Path::new("source.md")).unwrap(),
            vec![display_path(&root.join("nested/Río plan.md"))]
        );
        assert_eq!(
            backlinks(Some(&root), Path::new("nested/Río plan.md")).unwrap(),
            vec![display_path(&root.join("source.md"))]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_ambiguous_wiki_targets() {
        let root = temp_dir();
        fs::create_dir_all(root.join("one")).unwrap();
        fs::create_dir_all(root.join("two")).unwrap();
        fs::write(root.join("one/Same.md"), "one").unwrap();
        fs::write(root.join("two/Same.md"), "two").unwrap();
        fs::write(root.join("source.md"), "[[Same]]").unwrap();
        let err = links(Some(&root), Path::new("source.md")).unwrap_err();
        assert!(err.english.contains("ambiguous"));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn link_scan_does_not_follow_symlink_escapes() {
        use std::os::unix::fs::symlink;
        let root = temp_dir();
        let outside = temp_dir();
        fs::write(outside.join("secret.md"), "secret").unwrap();
        symlink(&outside, root.join("external")).unwrap();
        fs::write(root.join("source.md"), "[[secret]]").unwrap();
        assert!(links(Some(&root), Path::new("source.md"))
            .unwrap()
            .is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn writes_refuse_symlink_ancestors() {
        use std::os::unix::fs::symlink;
        let root = temp_dir();
        let outside = temp_dir();
        symlink(&outside, root.join("external")).unwrap();
        let command = Command::New {
            vault: root.clone(),
            name: "external/escaped".into(),
            template: None,
        };
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        assert!(execute_write(
            &command,
            &Settings::default(),
            now,
            &mut io::Cursor::new([]),
        )
        .is_err());
        assert!(!outside.join("escaped.md").exists());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn search_uses_disk_paths_and_stdin_is_bounded() {
        let root = temp_dir();
        fs::write(root.join("one.md"), "needle").unwrap();
        fs::write(root.join("two.md"), "other").unwrap();
        assert_eq!(
            search(&root, "needle").unwrap(),
            vec![display_path(&root.canonicalize().unwrap().join("one.md"))]
        );
        let mut input = io::Cursor::new("exact bytes".as_bytes());
        assert_eq!(read_stdin_limited(&mut input).unwrap(), "exact bytes");
        let mut too_large = io::Cursor::new(vec![b'x'; MAX_NOTE_BYTES + 1]);
        assert!(read_stdin_limited(&mut too_large).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn open_buffer_preflight_rejects_dirty_conflicted_and_unannounced_edits() {
        assert!(preflight_open_buffer(false, false, b"saved", b"saved").is_ok());
        assert!(preflight_open_buffer(true, false, b"saved", b"saved").is_err());
        assert!(preflight_open_buffer(false, true, b"saved", b"saved").is_err());
        assert!(preflight_open_buffer(false, false, b"pending", b"saved").is_err());
    }

    #[test]
    fn creates_nested_notes_without_overwriting_and_rejects_escape() {
        let root = temp_dir();
        let settings = Settings::default();
        let command = Command::New {
            vault: root.clone(),
            name: PathBuf::from("Rísta notes/Meeting notes"),
            template: None,
        };
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let created = execute_write(&command, &settings, now, &mut io::Cursor::new([])).unwrap();
        assert_eq!(fs::read(&created).unwrap(), b"");
        assert!(execute_write(&command, &settings, now, &mut io::Cursor::new([])).is_err());
        assert!(execute_write(
            &Command::New {
                vault: root.clone(),
                name: PathBuf::from("../outside"),
                template: None,
            },
            &settings,
            now,
            &mut io::Cursor::new([]),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn append_preserves_exact_bytes_and_snapshots_the_previous_note() {
        let root = temp_dir();
        let note = root.join("note.md");
        fs::write(&note, b"line one\r\nline two").unwrap();
        let command = Command::Append {
            vault: root.clone(),
            note: "note.md".into(),
            text: Some("\n追加".into()),
        };
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        execute_write(
            &command,
            &Settings::default(),
            now,
            &mut io::Cursor::new([]),
        )
        .unwrap();
        assert_eq!(
            fs::read(&note).unwrap(),
            "line one\r\nline two\n追加".as_bytes()
        );
        let snapshots = crate::history::snapshots(&root, &note);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(fs::read(&snapshots[0].1).unwrap(), b"line one\r\nline two");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn append_preserves_non_utf8_original_bytes() {
        let root = temp_dir();
        let note = root.join("note.md");
        fs::write(&note, [0xff, 0xfe]).unwrap();
        let command = Command::Append {
            vault: root.clone(),
            note: "note.md".into(),
            text: Some("x".into()),
        };
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        execute_write(
            &command,
            &Settings::default(),
            now,
            &mut io::Cursor::new([]),
        )
        .unwrap();

        assert_eq!(fs::read(&note).unwrap(), [0xff, 0xfe, b'x']);
        let snapshots = crate::history::snapshots(&root, &note);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(fs::read(&snapshots[0].1).unwrap(), [0xff, 0xfe]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn daily_uses_configured_date_format_and_template() {
        let root = temp_dir();
        fs::create_dir_all(root.join("Templates")).unwrap();
        fs::write(root.join("Templates/daily.md"), "{{title}} — {{date}}").unwrap();
        let mut settings = Settings::default();
        settings.templates_dir = "Templates".into();
        settings.daily_dir = "Journal".into();
        settings.daily_format = "YYYY/MM/DD".into();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let path = execute_write(
            &Command::Daily {
                vault: root.clone(),
            },
            &settings,
            now,
            &mut io::Cursor::new([]),
        )
        .unwrap();
        assert_eq!(
            path,
            root.canonicalize().unwrap().join("Journal/2026/03/04.md")
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "04 — 2026-03-04");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cli_template_rejects_interactive_dependencies_before_creating_note() {
        let root = temp_dir();
        fs::create_dir_all(root.join("Templates")).unwrap();
        fs::write(
            root.join("Templates/interactive.md"),
            "<% tp.file.selection() %>",
        )
        .unwrap();
        fs::write(
            root.join("Templates/prompt.md"),
            "<% tp.system.prompt(\"Name?\") %>",
        )
        .unwrap();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 4)
            .unwrap()
            .and_hms_opt(9, 30, 0)
            .unwrap();
        let mut settings = Settings::default();
        settings.templates_dir = "Templates".into();
        for template in ["interactive", "prompt"] {
            let command = Command::New {
                vault: root.clone(),
                name: format!("new {template}").into(),
                template: Some(template.into()),
            };
            assert!(execute_write(&command, &settings, now, &mut io::Cursor::new([])).is_err());
            assert!(!root.join(format!("new {template}.md")).exists());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
