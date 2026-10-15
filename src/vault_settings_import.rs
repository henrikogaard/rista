use crate::{
    bookmarks::{Bookmark, Bookmarks},
    settings::{LinkFormat, Settings},
};
use regex::RegexBuilder;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Take},
    path::{Component, Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024;
const MAX_JSON_VALUES: usize = 20_000;
const MAX_FILTERS: usize = 1_000;
const MAX_FILTER_BYTES: usize = 4096;
const SOURCE_FILES: &[&str] = &[
    ".obsidian/daily-notes.json",
    ".obsidian/templates.json",
    ".obsidian/plugins/templater-obsidian/data.json",
    ".obsidian/plugins/periodic-notes/data.json",
    ".obsidian/app.json",
    ".obsidian/bookmarks.json",
    ".obsidian/hotkeys.json",
    ".obsidian/community-plugins.json",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportCategory {
    DailyNotes,
    PeriodicNotes,
    Templates,
    Attachments,
    Links,
    IgnoreFilters,
    Bookmarks,
    Hotkeys,
    CommunityPlugins,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceState {
    Missing,
    Loaded,
    ReadError,
    InvalidJson,
    TooLarge,
    UnsafePath,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImportProblem {
    ReadFailed,
    InvalidJson,
    TooLarge,
    UnsafePath,
    TooManyEntries,
    InvalidFieldType,
    UnsupportedValue,
    UnsafeConfiguredPath,
    TemplateNotFound,
    InvalidRegex,
    UnsupportedHotkey,
    UnsupportedCommand,
    Precedence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportWarning {
    pub source: String,
    pub category: ImportCategory,
    pub problem: ImportProblem,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreviewChange {
    pub category: ImportCategory,
    pub field: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ImportedField {
    TemplatesDir,
    DailyDir,
    DailyFormat,
    AttachmentsDir,
    LinkFormat,
    IgnoreFilters,
    PeriodDir(String),
    PeriodFormat(String),
    PeriodTemplate(String),
    FolderTemplate(String),
}

#[derive(Clone, Debug)]
pub(crate) struct ImportPlan {
    pub root: PathBuf,
    baseline: Settings,
    proposed: Settings,
    changed_fields: BTreeSet<ImportedField>,
    pub preview: Vec<PreviewChange>,
    pub warnings: Vec<ImportWarning>,
    pub sources: BTreeMap<String, SourceState>,
    pub bookmarks: Vec<Bookmark>,
    pub bookmarks_skipped: usize,
    pub hotkeys: BTreeMap<String, Option<String>>,
    pub hotkey_baseline: BTreeMap<String, Option<String>>,
    pub unsupported_plugins: Vec<String>,
}

impl ImportPlan {
    pub(crate) fn has_sources(&self) -> bool {
        self.sources
            .values()
            .any(|state| *state != SourceState::Missing)
    }

    pub(crate) fn apply_settings(&self, current: &mut Settings) -> Result<(), Vec<String>> {
        let conflicts: Vec<_> = self
            .changed_fields
            .iter()
            .filter(|field| !same_field(&self.baseline, current, field))
            .map(|field| format!("{field:?}"))
            .collect();
        if !conflicts.is_empty() {
            return Err(conflicts);
        }
        for field in &self.changed_fields {
            copy_field(current, &self.proposed, field);
        }
        Ok(())
    }

    pub(crate) fn has_settings_changes(&self) -> bool {
        !self.changed_fields.is_empty() || !self.bookmarks.is_empty()
    }

    pub(crate) fn validate_targets(&self) -> bool {
        if self.root.canonicalize().ok().as_deref() != Some(self.root.as_path()) {
            return false;
        }
        let safe = |relative: &str| {
            relative.is_empty() || checked_vault_path(&self.root, relative).is_some()
        };
        self.changed_fields.iter().all(|field| match field {
            ImportedField::TemplatesDir
            | ImportedField::DailyDir
            | ImportedField::PeriodDir(_)
            | ImportedField::PeriodTemplate(_)
            | ImportedField::FolderTemplate(_) => {
                let value = match field {
                    ImportedField::TemplatesDir => &self.proposed.templates_dir,
                    ImportedField::DailyDir => &self.proposed.daily_dir,
                    ImportedField::PeriodDir(period) => period_dir(&self.proposed, period),
                    ImportedField::PeriodTemplate(period) => self
                        .proposed
                        .period_templates
                        .get(period)
                        .map(String::as_str)
                        .unwrap_or_default(),
                    ImportedField::FolderTemplate(folder) => {
                        if !safe(folder) {
                            return false;
                        }
                        self.proposed
                            .folder_templates
                            .get(folder)
                            .map(String::as_str)
                            .unwrap_or_default()
                    }
                    _ => unreachable!(),
                };
                safe(value)
            }
            ImportedField::DailyFormat | ImportedField::PeriodFormat(_) => {
                let period = match field {
                    ImportedField::DailyFormat => "daily",
                    ImportedField::PeriodFormat(period) => period,
                    _ => unreachable!(),
                };
                let format = period_format(&self.proposed, period);
                valid_period_format(&self.root, format)
            }
            ImportedField::AttachmentsDir => {
                let value = &self.proposed.attachments_dir;
                if value == "." {
                    true
                } else if let Some(relative) = value.strip_prefix("./") {
                    validate_relative(relative, true).is_ok()
                } else {
                    safe(value)
                }
            }
            ImportedField::LinkFormat | ImportedField::IgnoreFilters => true,
        })
    }
}

#[derive(Clone)]
struct SourceFile {
    bytes: Vec<u8>,
    value: Value,
}

pub(crate) fn plan_import(root: &Path, settings: &Settings) -> Result<ImportPlan, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("Could not resolve vault: {error}"))?;
    if !root.is_dir() {
        return Err("The selected vault is not a folder".to_string());
    }

    let mut plan = ImportPlan {
        root: root.clone(),
        baseline: settings.clone(),
        proposed: settings.clone(),
        changed_fields: BTreeSet::new(),
        preview: Vec::new(),
        warnings: Vec::new(),
        sources: BTreeMap::new(),
        bookmarks: Vec::new(),
        bookmarks_skipped: 0,
        hotkeys: BTreeMap::new(),
        hotkey_baseline: BTreeMap::new(),
        unsupported_plugins: Vec::new(),
    };
    let mut total = 0u64;
    let mut values = BTreeMap::new();
    for relative in SOURCE_FILES {
        let category = category_for_source(relative);
        match read_source(&root, relative, &mut total) {
            Ok(None) => {
                plan.sources
                    .insert((*relative).to_string(), SourceState::Missing);
            }
            Err(problem) => {
                let (state, kind) = match problem {
                    ReadProblem::TooLarge => (SourceState::TooLarge, ImportProblem::TooLarge),
                    ReadProblem::UnsafePath => (SourceState::UnsafePath, ImportProblem::UnsafePath),
                    ReadProblem::Read => (SourceState::ReadError, ImportProblem::ReadFailed),
                    ReadProblem::TotalLimit => (SourceState::TooLarge, ImportProblem::TooLarge),
                };
                plan.sources.insert((*relative).to_string(), state);
                plan.warnings.push(ImportWarning {
                    source: (*relative).to_string(),
                    category,
                    problem: kind,
                    detail: None,
                });
            }
            Ok(Some(bytes)) => {
                let value = match serde_json::from_slice::<Value>(&bytes) {
                    Ok(value) => value,
                    Err(_) => {
                        plan.sources
                            .insert((*relative).to_string(), SourceState::InvalidJson);
                        plan.warnings.push(ImportWarning {
                            source: (*relative).to_string(),
                            category,
                            problem: ImportProblem::InvalidJson,
                            detail: None,
                        });
                        continue;
                    }
                };
                if json_value_count(&value, MAX_JSON_VALUES + 1) > MAX_JSON_VALUES {
                    plan.sources
                        .insert((*relative).to_string(), SourceState::Loaded);
                    plan.warnings.push(ImportWarning {
                        source: (*relative).to_string(),
                        category,
                        problem: ImportProblem::TooManyEntries,
                        detail: None,
                    });
                    continue;
                }
                plan.sources
                    .insert((*relative).to_string(), SourceState::Loaded);
                values.insert((*relative).to_string(), SourceFile { bytes, value });
            }
        }
    }

    for (relative, is_array) in [
        (".obsidian/daily-notes.json", false),
        (".obsidian/templates.json", false),
        (".obsidian/plugins/templater-obsidian/data.json", false),
        (".obsidian/plugins/periodic-notes/data.json", false),
        (".obsidian/app.json", false),
        (".obsidian/bookmarks.json", false),
        (".obsidian/hotkeys.json", false),
        (".obsidian/community-plugins.json", true),
    ] {
        if let Some(source) = values.get(relative) {
            let valid = if is_array {
                source.value.is_array()
            } else {
                source.value.is_object()
            };
            if !valid {
                warn(
                    &mut plan,
                    relative,
                    category_for_source(relative),
                    ImportProblem::InvalidFieldType,
                    None,
                );
            }
        }
    }

    let daily_source = values
        .get(".obsidian/daily-notes.json")
        .map(|source| &source.value);
    if let Some(value) = daily_source {
        import_folder_field(
            &mut plan,
            value,
            "folder",
            ImportedField::DailyDir,
            ImportCategory::DailyNotes,
            ".obsidian/daily-notes.json",
        );
        import_string_field(
            &mut plan,
            value,
            "format",
            ImportedField::DailyFormat,
            ImportCategory::DailyNotes,
            ".obsidian/daily-notes.json",
        );
    }

    let templates_folder = values.get(".obsidian/templates.json").and_then(|source| {
        import_folder_field(
            &mut plan,
            &source.value,
            "folder",
            ImportedField::TemplatesDir,
            ImportCategory::Templates,
            ".obsidian/templates.json",
        )
    });

    let templater_active = values
        .get(".obsidian/community-plugins.json")
        .and_then(|plugins| plugins.value.as_array())
        .is_some_and(|plugins| {
            plugins
                .iter()
                .any(|id| id.as_str() == Some("templater-obsidian"))
        });
    let mut templater_folder = None;
    if let Some(source) = values.get(".obsidian/plugins/templater-obsidian/data.json") {
        let source_name = ".obsidian/plugins/templater-obsidian/data.json";
        let folder_templates_enabled =
            if let Some(mode) = source.value.get("trigger_on_file_creation_mode") {
                match mode.as_str() {
                    Some("none") => false,
                    Some("folder") => true,
                    Some("regex") => {
                        warn(
                            &mut plan,
                            source_name,
                            ImportCategory::Templates,
                            ImportProblem::UnsupportedValue,
                            Some("trigger_on_file_creation_mode".to_string()),
                        );
                        false
                    }
                    _ => {
                        warn(
                            &mut plan,
                            source_name,
                            ImportCategory::Templates,
                            ImportProblem::InvalidFieldType,
                            Some("trigger_on_file_creation_mode".to_string()),
                        );
                        false
                    }
                }
            } else {
                match source.value.get("enable_folder_templates") {
                    None => false,
                    Some(Value::Bool(enabled)) => *enabled,
                    Some(_) => {
                        warn(
                            &mut plan,
                            source_name,
                            ImportCategory::Templates,
                            ImportProblem::InvalidFieldType,
                            Some("enable_folder_templates".to_string()),
                        );
                        false
                    }
                }
            };
        if let Some(folder) = source
            .value
            .get("templates_folder")
            .filter(|_| templater_active)
        {
            if let Some(folder) = validate_folder_value(
                folder,
                &root,
                &mut plan,
                source_name,
                ImportCategory::Templates,
                "templates_folder",
            ) {
                templater_folder = Some(folder.clone());
                if templates_folder
                    .as_deref()
                    .is_some_and(|regular| regular != folder)
                {
                    warn(
                        &mut plan,
                        source_name,
                        ImportCategory::Templates,
                        ImportProblem::Precedence,
                        None,
                    );
                }
                set_string(
                    &mut plan,
                    ImportedField::TemplatesDir,
                    folder,
                    ImportCategory::Templates,
                    "templates folder",
                );
            }
        }
        if folder_templates_enabled
            && templater_active
            && (source.value.get("templates_folder").is_none() || templater_folder.is_some())
        {
            import_folder_templates(
                &mut plan,
                &source.value,
                &root,
                source_name,
                templater_folder.as_deref().unwrap_or_default(),
            );
        }
        if templater_active {
            for key in [
                "enable_system_commands",
                "enable_user_scripts",
                "user_scripts",
                "user_scripts_folder",
                "script_files",
            ] {
                let unsupported = source.value.get(key).is_some_and(|value| match value {
                    Value::Bool(enabled) => *enabled,
                    Value::String(value) => !value.is_empty(),
                    Value::Array(items) => !items.is_empty(),
                    Value::Object(items) => !items.is_empty(),
                    _ => false,
                });
                if unsupported {
                    warn(
                        &mut plan,
                        source_name,
                        ImportCategory::Templates,
                        ImportProblem::UnsupportedValue,
                        Some(key.to_string()),
                    );
                }
            }
        }
    }

    let selected_templates_dir = templater_folder
        .clone()
        .unwrap_or_else(|| plan.proposed.templates_dir.clone());
    let daily_template = daily_source.and_then(|value| {
        string_field(
            &mut plan,
            value,
            "template",
            ImportCategory::DailyNotes,
            ".obsidian/daily-notes.json",
        )
    });
    if let Some(template) = daily_template {
        import_period_template(
            &mut plan,
            "daily",
            &template,
            &selected_templates_dir,
            &root,
            ImportCategory::DailyNotes,
            ".obsidian/daily-notes.json",
        );
    }

    if let Some(source) = values.get(".obsidian/plugins/periodic-notes/data.json") {
        for period in ["weekly", "monthly", "quarterly", "yearly"] {
            let Some(period_settings) = source.value.get(period) else {
                continue;
            };
            import_folder_field(
                &mut plan,
                period_settings,
                "folder",
                ImportedField::PeriodDir(period.to_string()),
                ImportCategory::PeriodicNotes,
                ".obsidian/plugins/periodic-notes/data.json",
            );
            import_string_field(
                &mut plan,
                period_settings,
                "format",
                ImportedField::PeriodFormat(period.to_string()),
                ImportCategory::PeriodicNotes,
                ".obsidian/plugins/periodic-notes/data.json",
            );
            if let Some(template) = string_field(
                &mut plan,
                period_settings,
                "template",
                ImportCategory::PeriodicNotes,
                ".obsidian/plugins/periodic-notes/data.json",
            ) {
                import_period_template(
                    &mut plan,
                    period,
                    &template,
                    &selected_templates_dir,
                    &root,
                    ImportCategory::PeriodicNotes,
                    ".obsidian/plugins/periodic-notes/data.json",
                );
            }
        }
    }

    if let Some(source) = values.get(".obsidian/app.json") {
        if let Some(value) = source
            .value
            .get("attachmentFolderPath")
            .and_then(Value::as_str)
        {
            let normalized = if value == "." {
                Some(".".to_string())
            } else if let Some(note_relative) = value.strip_prefix("./") {
                validate_relative(note_relative, true).ok().map(|path| {
                    if path.is_empty() {
                        "./".to_string()
                    } else {
                        format!("./{path}")
                    }
                })
            } else {
                validate_relative(value, true)
                    .ok()
                    .filter(|path| validate_existing_path(&root, path))
            };
            if let Some(value) = normalized {
                set_imported_field(
                    &mut plan,
                    ImportedField::AttachmentsDir,
                    value,
                    ImportCategory::Attachments,
                    "attachmentFolderPath",
                );
            } else {
                warn(
                    &mut plan,
                    ".obsidian/app.json",
                    ImportCategory::Attachments,
                    ImportProblem::UnsafeConfiguredPath,
                    Some("attachmentFolderPath".to_string()),
                );
            }
        } else if source.value.get("attachmentFolderPath").is_some() {
            warn(
                &mut plan,
                ".obsidian/app.json",
                ImportCategory::Attachments,
                ImportProblem::InvalidFieldType,
                Some("attachmentFolderPath".to_string()),
            );
        }
        if let Some(value) = string_field(
            &mut plan,
            &source.value,
            "newLinkFormat",
            ImportCategory::Links,
            ".obsidian/app.json",
        ) {
            let format = match value.as_str() {
                "shortest" => Some(LinkFormat::Shortest),
                "relative" => Some(LinkFormat::Relative),
                "absolute" => Some(LinkFormat::Absolute),
                _ => None,
            };
            if let Some(format) = format {
                if plan.proposed.link_format != format {
                    plan.proposed.link_format = format;
                    plan.changed_fields.insert(ImportedField::LinkFormat);
                    plan.preview.push(PreviewChange {
                        category: ImportCategory::Links,
                        field: "newLinkFormat".to_string(),
                        value,
                    });
                }
            } else {
                warn(
                    &mut plan,
                    ".obsidian/app.json",
                    ImportCategory::Links,
                    ImportProblem::UnsupportedValue,
                    Some("newLinkFormat".to_string()),
                );
            }
        }
        if let Some(filters) = source.value.get("userIgnoreFilters") {
            if let Some(filters) = filters.as_array() {
                if filters.len() > MAX_FILTERS {
                    warn(
                        &mut plan,
                        ".obsidian/app.json",
                        ImportCategory::IgnoreFilters,
                        ImportProblem::TooManyEntries,
                        Some("userIgnoreFilters".to_string()),
                    );
                } else {
                    let mut accepted = Vec::new();
                    let mut invalid = false;
                    for filter in filters {
                        let Some(filter) = filter.as_str() else {
                            invalid = true;
                            warn(
                                &mut plan,
                                ".obsidian/app.json",
                                ImportCategory::IgnoreFilters,
                                ImportProblem::InvalidFieldType,
                                Some("userIgnoreFilters".to_string()),
                            );
                            continue;
                        };
                        match validate_filter(filter) {
                            Ok(filter) => accepted.push(filter),
                            Err(problem) => {
                                invalid = true;
                                warn(
                                    &mut plan,
                                    ".obsidian/app.json",
                                    ImportCategory::IgnoreFilters,
                                    problem,
                                    Some(filter.to_string()),
                                );
                            }
                        }
                    }
                    if !invalid && plan.proposed.user_ignore_filters != accepted {
                        plan.proposed.user_ignore_filters = accepted.clone();
                        plan.changed_fields.insert(ImportedField::IgnoreFilters);
                        plan.preview.push(PreviewChange {
                            category: ImportCategory::IgnoreFilters,
                            field: "userIgnoreFilters".to_string(),
                            value: if accepted.is_empty() {
                                "(none)".to_string()
                            } else {
                                accepted.join(", ")
                            },
                        });
                    }
                }
            } else {
                warn(
                    &mut plan,
                    ".obsidian/app.json",
                    ImportCategory::IgnoreFilters,
                    ImportProblem::InvalidFieldType,
                    Some("userIgnoreFilters".to_string()),
                );
            }
        }
    }

    if let Some(source) = values.get(".obsidian/bookmarks.json") {
        if let Ok(raw) = std::str::from_utf8(&source.bytes) {
            let mut preview_store: Bookmarks = settings.bookmarks.clone();
            match preview_store.import_vault(&root, raw) {
                Ok(result) => {
                    plan.bookmarks = result.items;
                    plan.bookmarks_skipped = result.skipped;
                    if !plan.bookmarks.is_empty() {
                        plan.preview.push(PreviewChange {
                            category: ImportCategory::Bookmarks,
                            field: "bookmarks".to_string(),
                            value: plan.bookmarks.len().to_string(),
                        });
                    }
                }
                Err(_) => warn(
                    &mut plan,
                    ".obsidian/bookmarks.json",
                    ImportCategory::Bookmarks,
                    ImportProblem::InvalidJson,
                    None,
                ),
            }
        }
    }

    if let Some(source) = values.get(".obsidian/hotkeys.json") {
        import_hotkeys(&mut plan, &source.value);
    }

    if let Some(source) = values.get(".obsidian/community-plugins.json") {
        match source.value.as_array() {
            Some(ids) => {
                for id in ids {
                    match id.as_str() {
                        Some(id) if !id.is_empty() => plan.unsupported_plugins.push(id.to_string()),
                        _ => warn(
                            &mut plan,
                            ".obsidian/community-plugins.json",
                            ImportCategory::CommunityPlugins,
                            ImportProblem::InvalidFieldType,
                            None,
                        ),
                    }
                }
                plan.unsupported_plugins.sort();
                plan.unsupported_plugins.dedup();
                if !plan.unsupported_plugins.is_empty() {
                    plan.preview.push(PreviewChange {
                        category: ImportCategory::CommunityPlugins,
                        field: "enabled community plugins".to_string(),
                        value: plan.unsupported_plugins.join(", "),
                    });
                }
            }
            None => warn(
                &mut plan,
                ".obsidian/community-plugins.json",
                ImportCategory::CommunityPlugins,
                ImportProblem::InvalidFieldType,
                None,
            ),
        }
    }

    Ok(plan)
}

#[derive(Clone, Copy)]
enum ReadProblem {
    TooLarge,
    UnsafePath,
    Read,
    TotalLimit,
}

fn read_source(
    root: &Path,
    relative: &str,
    total: &mut u64,
) -> Result<Option<Vec<u8>>, ReadProblem> {
    let mut path = root.to_path_buf();
    let parts: Vec<_> = Path::new(relative).components().collect();
    for (index, part) in parts.iter().enumerate() {
        let Component::Normal(part) = part else {
            return Err(ReadProblem::UnsafePath);
        };
        path.push(part);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(ReadProblem::Read),
        };
        if metadata.file_type().is_symlink()
            || (index + 1 != parts.len() && !metadata.is_dir())
            || (index + 1 == parts.len() && !metadata.is_file())
        {
            return Err(ReadProblem::UnsafePath);
        }
        if path
            .canonicalize()
            .map_or(true, |canonical| !canonical.starts_with(root))
        {
            return Err(ReadProblem::UnsafePath);
        }
    }
    let mut file = File::open(&path).map_err(|_| ReadProblem::Read)?;
    let bytes = read_limited(&mut file, MAX_FILE_BYTES + 1).map_err(|_| ReadProblem::Read)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(ReadProblem::TooLarge);
    }
    *total = total.saturating_add(bytes.len() as u64);
    if *total > MAX_TOTAL_BYTES {
        return Err(ReadProblem::TotalLimit);
    }
    Ok(Some(bytes))
}

fn read_limited(reader: &mut File, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut bounded: Take<&mut File> = reader.take(limit);
    bounded.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn json_value_count(value: &Value, limit: usize) -> usize {
    fn count(value: &Value, remaining: &mut usize) {
        if *remaining == 0 {
            return;
        }
        *remaining -= 1;
        match value {
            Value::Array(items) => items.iter().for_each(|item| count(item, remaining)),
            Value::Object(items) => items.values().for_each(|item| count(item, remaining)),
            _ => {}
        }
    }
    let mut remaining = limit;
    count(value, &mut remaining);
    limit - remaining
}

fn category_for_source(path: &str) -> ImportCategory {
    match path {
        ".obsidian/daily-notes.json" => ImportCategory::DailyNotes,
        ".obsidian/templates.json" | ".obsidian/plugins/templater-obsidian/data.json" => {
            ImportCategory::Templates
        }
        ".obsidian/plugins/periodic-notes/data.json" => ImportCategory::PeriodicNotes,
        ".obsidian/app.json" => ImportCategory::Attachments,
        ".obsidian/bookmarks.json" => ImportCategory::Bookmarks,
        ".obsidian/hotkeys.json" => ImportCategory::Hotkeys,
        _ => ImportCategory::CommunityPlugins,
    }
}

fn warn(
    plan: &mut ImportPlan,
    source: &str,
    category: ImportCategory,
    problem: ImportProblem,
    detail: Option<String>,
) {
    plan.warnings.push(ImportWarning {
        source: source.to_string(),
        category,
        problem,
        detail,
    });
}

fn string_field(
    plan: &mut ImportPlan,
    object: &Value,
    field: &str,
    category: ImportCategory,
    source: &str,
) -> Option<String> {
    match object.get(field) {
        None => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            warn(
                plan,
                source,
                category,
                ImportProblem::InvalidFieldType,
                Some(field.to_string()),
            );
            None
        }
    }
}

fn validate_relative(value: &str, allow_empty: bool) -> Result<String, ImportProblem> {
    if value.len() > MAX_FILTER_BYTES {
        return Err(ImportProblem::UnsupportedValue);
    }
    if value.is_empty() || value == "." {
        return if allow_empty {
            Ok(String::new())
        } else {
            Err(ImportProblem::UnsafeConfiguredPath)
        };
    }
    if value.starts_with('/')
        || value.starts_with('\\')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(ImportProblem::UnsafeConfiguredPath);
    }
    let mut components = Vec::new();
    for component in Path::new(value).components() {
        match component {
            Component::Normal(part) => components.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            _ => return Err(ImportProblem::UnsafeConfiguredPath),
        }
    }
    if components.is_empty() {
        Ok(String::new())
    } else {
        Ok(components.join("/"))
    }
}

fn unwrap_template_reference(value: &str) -> Result<&str, ImportProblem> {
    let value = value.trim();
    let starts_wrapper = value.starts_with("[[");
    let ends_wrapper = value.ends_with("]]");
    match (starts_wrapper, ends_wrapper) {
        (true, true) => {
            let reference = value[2..value.len() - 2].trim();
            if reference.is_empty() || reference.contains(['|', '#']) {
                Err(ImportProblem::UnsupportedValue)
            } else {
                Ok(reference)
            }
        }
        (false, false) => Ok(value),
        _ => Err(ImportProblem::UnsupportedValue),
    }
}

fn validate_existing_path(root: &Path, relative: &str) -> bool {
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            continue;
        };
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => return false,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return false,
        }
        if path
            .canonicalize()
            .is_ok_and(|canonical| !canonical.starts_with(root))
        {
            return false;
        }
    }
    true
}

fn valid_period_format(root: &Path, format: &str) -> bool {
    if format.is_empty() || format.len() > MAX_FILTER_BYTES {
        return false;
    }
    let sample = crate::bases::format_moment(
        chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        format,
    );
    validate_relative(&sample, false)
        .ok()
        .and_then(|relative| checked_vault_path(root, &relative))
        .is_some()
}

fn validate_markdown_template(root: &Path, relative: &str) -> Result<(), ImportProblem> {
    let relative = validate_relative(relative, false)?;
    if !Path::new(&relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        return Err(ImportProblem::UnsupportedValue);
    }
    let path = checked_vault_path(root, &relative).ok_or(ImportProblem::UnsafeConfiguredPath)?;
    let canonical = path.canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ImportProblem::TemplateNotFound
        } else {
            ImportProblem::ReadFailed
        }
    })?;
    if !canonical.starts_with(root) {
        return Err(ImportProblem::UnsafeConfiguredPath);
    }
    if !canonical.is_file() {
        return Err(ImportProblem::TemplateNotFound);
    }
    Ok(())
}

fn resolve_template_reference(
    root: &Path,
    relative: &str,
    fallback_dir: &str,
) -> Result<String, ImportProblem> {
    let candidate = if Path::new(relative).extension().is_none() {
        format!("{relative}.md")
    } else {
        relative.to_string()
    };
    if Path::new(&candidate)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("md"))
    {
        return Err(ImportProblem::UnsupportedValue);
    }
    match validate_markdown_template(root, &candidate) {
        Ok(()) => return Ok(candidate),
        Err(ImportProblem::TemplateNotFound) => {}
        Err(problem) => return Err(problem),
    }
    if !candidate.contains('/') && !fallback_dir.is_empty() {
        let fallback = format!("{fallback_dir}/{candidate}");
        validate_markdown_template(root, &fallback)?;
        return Ok(fallback);
    }
    validate_markdown_template(root, &candidate)?;
    unreachable!()
}

pub(crate) fn checked_vault_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let relative = validate_relative(relative, false).ok()?;
    validate_existing_path(&root, &relative).then(|| root.join(relative))
}

fn validate_folder_value(
    value: &Value,
    root: &Path,
    plan: &mut ImportPlan,
    source: &str,
    category: ImportCategory,
    field: &str,
) -> Option<String> {
    let Some(value) = value.as_str() else {
        warn(
            plan,
            source,
            category,
            ImportProblem::InvalidFieldType,
            Some(field.to_string()),
        );
        return None;
    };
    let value = match validate_relative(value, true) {
        Ok(value) if validate_existing_path(root, &value) => value,
        _ => {
            warn(
                plan,
                source,
                category,
                ImportProblem::UnsafeConfiguredPath,
                Some(field.to_string()),
            );
            return None;
        }
    };
    Some(value)
}

fn import_folder_field(
    plan: &mut ImportPlan,
    object: &Value,
    field: &str,
    target: ImportedField,
    category: ImportCategory,
    source: &str,
) -> Option<String> {
    let value = object.get(field)?;
    let root = plan.root.clone();
    let value = validate_folder_value(value, &root, plan, source, category, field)?;
    if matches!(
        &target,
        ImportedField::DailyDir | ImportedField::PeriodDir(_)
    ) {
        set_imported_field(plan, target, value.clone(), category, field);
    } else {
        set_string(plan, target, value.clone(), category, field);
    }
    Some(value)
}

fn import_string_field(
    plan: &mut ImportPlan,
    object: &Value,
    field: &str,
    target: ImportedField,
    category: ImportCategory,
    source: &str,
) {
    if let Some(value) = string_field(plan, object, field, category, source) {
        if matches!(
            &target,
            ImportedField::DailyFormat | ImportedField::PeriodFormat(_)
        ) && !valid_period_format(&plan.root, &value)
        {
            warn(
                plan,
                source,
                category,
                ImportProblem::UnsafeConfiguredPath,
                Some(field.to_string()),
            );
            return;
        }
        set_imported_field(plan, target, value, category, field);
    }
}

fn set_string(
    plan: &mut ImportPlan,
    field: ImportedField,
    value: String,
    category: ImportCategory,
    label: &str,
) {
    set_imported_field(plan, field, value, category, label);
}

fn set_imported_field(
    plan: &mut ImportPlan,
    field: ImportedField,
    value: String,
    category: ImportCategory,
    label: &str,
) {
    let previous = match &field {
        ImportedField::TemplatesDir => &mut plan.proposed.templates_dir,
        ImportedField::DailyDir => &mut plan.proposed.daily_dir,
        ImportedField::DailyFormat => &mut plan.proposed.daily_format,
        ImportedField::AttachmentsDir => &mut plan.proposed.attachments_dir,
        ImportedField::PeriodDir(period) => period_dir_mut(&mut plan.proposed, period),
        ImportedField::PeriodFormat(period) => period_format_mut(&mut plan.proposed, period),
        _ => return,
    };
    if *previous != value {
        *previous = value.clone();
        let field_label = match &field {
            ImportedField::DailyDir => "daily directory".to_string(),
            ImportedField::DailyFormat => "daily format".to_string(),
            ImportedField::PeriodDir(period) => format!("{period} directory"),
            ImportedField::PeriodFormat(period) => format!("{period} format"),
            _ => label.to_string(),
        };
        plan.changed_fields.insert(field);
        plan.preview.push(PreviewChange {
            category,
            field: field_label,
            value,
        });
    }
}

fn period_dir_mut<'a>(settings: &'a mut Settings, period: &str) -> &'a mut String {
    match period {
        "weekly" => &mut settings.weekly_dir,
        "monthly" => &mut settings.monthly_dir,
        "quarterly" => &mut settings.quarterly_dir,
        "yearly" => &mut settings.yearly_dir,
        _ => &mut settings.daily_dir,
    }
}

fn period_format_mut<'a>(settings: &'a mut Settings, period: &str) -> &'a mut String {
    match period {
        "weekly" => &mut settings.weekly_format,
        "monthly" => &mut settings.monthly_format,
        "quarterly" => &mut settings.quarterly_format,
        "yearly" => &mut settings.yearly_format,
        _ => &mut settings.daily_format,
    }
}

fn import_period_template(
    plan: &mut ImportPlan,
    period: &str,
    template: &str,
    templates_dir: &str,
    root: &Path,
    category: ImportCategory,
    source: &str,
) {
    let relative = if template.is_empty() {
        String::new()
    } else {
        let template = match unwrap_template_reference(template) {
            Ok(template) => template,
            Err(problem) => {
                warn(
                    plan,
                    source,
                    category,
                    problem,
                    Some("template".to_string()),
                );
                return;
            }
        };
        let template = match validate_relative(template, false) {
            Ok(template) => template,
            Err(problem) => {
                warn(
                    plan,
                    source,
                    category,
                    problem,
                    Some("template".to_string()),
                );
                return;
            }
        };
        let template = if Path::new(&template).extension().is_none() {
            format!("{template}.md")
        } else if Path::new(&template)
            .extension()
            .is_some_and(|ext| ext != "md")
        {
            warn(
                plan,
                source,
                category,
                ImportProblem::UnsupportedValue,
                Some("template extension".to_string()),
            );
            return;
        } else {
            template
        };
        let combined = match resolve_template_reference(root, &template, templates_dir) {
            Ok(path) => path,
            Err(problem) => {
                warn(
                    plan,
                    source,
                    category,
                    problem,
                    Some("template".to_string()),
                );
                return;
            }
        };
        if let Err(problem) = validate_markdown_template(root, &combined) {
            warn(
                plan,
                source,
                category,
                problem,
                Some("template".to_string()),
            );
            return;
        }
        combined
    };
    let key = period.to_string();
    if plan.proposed.period_templates.get(period) != Some(&relative) {
        plan.proposed
            .period_templates
            .insert(key.clone(), relative.clone());
        plan.changed_fields
            .insert(ImportedField::PeriodTemplate(key));
        plan.preview.push(PreviewChange {
            category,
            field: format!("{period} template"),
            value: if relative.is_empty() {
                "(none)".to_string()
            } else {
                relative
            },
        });
    }
}

fn import_folder_templates(
    plan: &mut ImportPlan,
    value: &Value,
    root: &Path,
    source: &str,
    template_root: &str,
) {
    let Some(folder_templates) = value.get("folder_templates") else {
        return;
    };
    let Some(folder_templates) = folder_templates.as_array() else {
        warn(
            plan,
            source,
            ImportCategory::Templates,
            ImportProblem::InvalidFieldType,
            Some("folder_templates".to_string()),
        );
        return;
    };
    if folder_templates.len() > 2_000 {
        warn(
            plan,
            source,
            ImportCategory::Templates,
            ImportProblem::TooManyEntries,
            Some("folder_templates".to_string()),
        );
        return;
    }
    for entry in folder_templates {
        let Some(entry) = entry.as_object() else {
            warn(
                plan,
                source,
                ImportCategory::Templates,
                ImportProblem::UnsafeConfiguredPath,
                Some("folder_templates".to_string()),
            );
            continue;
        };
        let (Some(folder), Some(template)) = (
            entry.get("folder").and_then(Value::as_str),
            entry.get("template").and_then(Value::as_str),
        ) else {
            warn(
                plan,
                source,
                ImportCategory::Templates,
                ImportProblem::UnsafeConfiguredPath,
                Some("folder_templates".to_string()),
            );
            continue;
        };
        let Ok(folder) = validate_relative(folder, true) else {
            warn(
                plan,
                source,
                ImportCategory::Templates,
                ImportProblem::UnsupportedValue,
                Some("folder".to_string()),
            );
            continue;
        };
        let Ok(template) = unwrap_template_reference(template)
            .and_then(|template| validate_relative(template, false))
        else {
            warn(
                plan,
                source,
                ImportCategory::Templates,
                ImportProblem::UnsafeConfiguredPath,
                Some(folder),
            );
            continue;
        };
        let template = match resolve_template_reference(root, &template, template_root) {
            Ok(template) => template,
            Err(problem) => {
                warn(
                    plan,
                    source,
                    ImportCategory::Templates,
                    problem,
                    Some(folder),
                );
                continue;
            }
        };
        if !validate_existing_path(root, &folder) {
            warn(
                plan,
                source,
                ImportCategory::Templates,
                ImportProblem::UnsafeConfiguredPath,
                Some(folder),
            );
            continue;
        }
        if let Err(problem) = validate_markdown_template(root, &template) {
            warn(
                plan,
                source,
                ImportCategory::Templates,
                problem,
                Some(folder),
            );
            continue;
        }
        if plan.proposed.folder_templates.get(&folder) != Some(&template) {
            plan.proposed
                .folder_templates
                .insert(folder.clone(), template.clone());
            plan.changed_fields
                .insert(ImportedField::FolderTemplate(folder.clone()));
            plan.preview.push(PreviewChange {
                category: ImportCategory::Templates,
                field: format!("folder template {folder}"),
                value: template,
            });
        }
    }
}

fn validate_filter(filter: &str) -> Result<String, ImportProblem> {
    if filter.is_empty() || filter.len() > MAX_FILTER_BYTES {
        return Err(ImportProblem::UnsupportedValue);
    }
    if let Some(pattern) = filter.strip_prefix('/').and_then(|s| s.strip_suffix('/')) {
        let mut builder = RegexBuilder::new(pattern);
        builder.size_limit(256 * 1024).dfa_size_limit(256 * 1024);
        builder.build().map_err(|_| ImportProblem::InvalidRegex)?;
        return Ok(filter.to_string());
    }
    if filter.contains('*') || filter.contains('?') || filter.contains('\\') {
        return Err(ImportProblem::UnsupportedValue);
    }
    validate_relative(filter, false)
}

fn import_hotkeys(plan: &mut ImportPlan, value: &Value) {
    let Some(object) = value.as_object() else {
        warn(
            plan,
            ".obsidian/hotkeys.json",
            ImportCategory::Hotkeys,
            ImportProblem::InvalidFieldType,
            None,
        );
        return;
    };
    if object.len() > 2_000 {
        warn(
            plan,
            ".obsidian/hotkeys.json",
            ImportCategory::Hotkeys,
            ImportProblem::TooManyEntries,
            None,
        );
        return;
    }
    for (source_command, shortcuts) in object {
        let Some(native_command) = map_hotkey_command(source_command) else {
            warn(
                plan,
                ".obsidian/hotkeys.json",
                ImportCategory::Hotkeys,
                ImportProblem::UnsupportedCommand,
                Some(source_command.clone()),
            );
            continue;
        };
        let Some(shortcuts) = shortcuts.as_array() else {
            warn(
                plan,
                ".obsidian/hotkeys.json",
                ImportCategory::Hotkeys,
                ImportProblem::InvalidFieldType,
                Some(source_command.clone()),
            );
            continue;
        };
        if shortcuts.is_empty() {
            plan.hotkeys.insert(native_command.to_string(), None);
            continue;
        }
        if shortcuts.len() != 1 {
            warn(
                plan,
                ".obsidian/hotkeys.json",
                ImportCategory::Hotkeys,
                ImportProblem::UnsupportedHotkey,
                Some(source_command.clone()),
            );
            continue;
        }
        match format_shortcut(&shortcuts[0]) {
            Some(shortcut) => {
                plan.hotkeys
                    .insert(native_command.to_string(), Some(shortcut));
            }
            None => warn(
                plan,
                ".obsidian/hotkeys.json",
                ImportCategory::Hotkeys,
                ImportProblem::UnsupportedHotkey,
                Some(source_command.clone()),
            ),
        }
    }
    if !plan.hotkeys.is_empty() {
        plan.preview.push(PreviewChange {
            category: ImportCategory::Hotkeys,
            field: "shortcuts".to_string(),
            value: plan.hotkeys.len().to_string(),
        });
    }
}

fn map_hotkey_command(source: &str) -> Option<&'static str> {
    let native = match source {
        "app:open-daily-note" => "open_daily_note",
        "editor:save-file" => "save",
        "app:open-quick-switcher" => "quick_open",
        "global-search:open" => "open_project_search",
        "file-explorer:new-file" => "new_file",
        "editor:toggle-bold" => "toggle_bold",
        "editor:toggle-italic" => "toggle_italic",
        "editor:toggle-source" => "view_source",
        "editor:toggle-split" => "view_split",
        "app:open-settings" => "open_settings",
        "app:close" => "close_tab",
        _ => return None,
    };
    crate::hotkeys::command(native).map(|_| match native {
        "open_daily_note" => "open_daily_note",
        "save" => "save",
        "quick_open" => "quick_open",
        "open_project_search" => "open_project_search",
        "new_file" => "new_file",
        "toggle_bold" => "toggle_bold",
        "toggle_italic" => "toggle_italic",
        "view_source" => "view_source",
        "view_split" => "view_split",
        "open_settings" => "open_settings",
        "close_tab" => "close_tab",
        _ => unreachable!(),
    })
}

fn format_shortcut(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "key" | "modifiers"))
    {
        return None;
    }
    let key = object.get("key")?.as_str()?;
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return None;
    }
    let modifiers = object.get("modifiers")?.as_array()?;
    let mut result = Vec::new();
    for modifier in modifiers {
        let modifier = modifier.as_str()?;
        let mapped = match modifier {
            // Obsidian's "Mod" is Cmd on macOS and Ctrl elsewhere, like GPUI's "secondary".
            "Mod" => "secondary",
            "Cmd" => "cmd",
            "Ctrl" => "ctrl",
            "Alt" => "alt",
            "Shift" => "shift",
            _ => return None,
        };
        if !result.contains(&mapped) {
            result.push(mapped);
        }
    }
    let key = match key {
        "ArrowUp" => "up",
        "ArrowDown" => "down",
        "ArrowLeft" => "left",
        "ArrowRight" => "right",
        "Enter" => "enter",
        "Escape" => "escape",
        "Backspace" => "backspace",
        "Tab" => "tab",
        " " => "space",
        key => key,
    };
    let key = key.to_lowercase();
    result.push(&key);
    Some(result.join("-"))
}

fn same_field(baseline: &Settings, current: &Settings, field: &ImportedField) -> bool {
    match field {
        ImportedField::TemplatesDir => baseline.templates_dir == current.templates_dir,
        ImportedField::DailyDir => baseline.daily_dir == current.daily_dir,
        ImportedField::DailyFormat => baseline.daily_format == current.daily_format,
        ImportedField::AttachmentsDir => baseline.attachments_dir == current.attachments_dir,
        ImportedField::LinkFormat => baseline.link_format == current.link_format,
        ImportedField::IgnoreFilters => baseline.user_ignore_filters == current.user_ignore_filters,
        ImportedField::PeriodDir(period) => {
            period_dir(baseline, period) == period_dir(current, period)
        }
        ImportedField::PeriodFormat(period) => {
            period_format(baseline, period) == period_format(current, period)
        }
        ImportedField::PeriodTemplate(period) => {
            baseline.period_templates.get(period) == current.period_templates.get(period)
        }
        ImportedField::FolderTemplate(folder) => {
            baseline.folder_templates.get(folder) == current.folder_templates.get(folder)
        }
    }
}

fn copy_field(current: &mut Settings, proposed: &Settings, field: &ImportedField) {
    match field {
        ImportedField::TemplatesDir => current.templates_dir = proposed.templates_dir.clone(),
        ImportedField::DailyDir => current.daily_dir = proposed.daily_dir.clone(),
        ImportedField::DailyFormat => current.daily_format = proposed.daily_format.clone(),
        ImportedField::AttachmentsDir => current.attachments_dir = proposed.attachments_dir.clone(),
        ImportedField::LinkFormat => current.link_format = proposed.link_format,
        ImportedField::IgnoreFilters => {
            current.user_ignore_filters = proposed.user_ignore_filters.clone()
        }
        ImportedField::PeriodDir(period) => {
            *period_dir_mut(current, period) = period_dir(proposed, period).to_string()
        }
        ImportedField::PeriodFormat(period) => {
            *period_format_mut(current, period) = period_format(proposed, period).to_string()
        }
        ImportedField::PeriodTemplate(period) => {
            if let Some(value) = proposed.period_templates.get(period) {
                current
                    .period_templates
                    .insert(period.clone(), value.clone());
            } else {
                current.period_templates.remove(period);
            }
        }
        ImportedField::FolderTemplate(folder) => {
            if let Some(value) = proposed.folder_templates.get(folder) {
                current
                    .folder_templates
                    .insert(folder.clone(), value.clone());
            } else {
                current.folder_templates.remove(folder);
            }
        }
    }
}

fn period_dir<'a>(settings: &'a Settings, period: &str) -> &'a str {
    match period {
        "weekly" => &settings.weekly_dir,
        "monthly" => &settings.monthly_dir,
        "quarterly" => &settings.quarterly_dir,
        "yearly" => &settings.yearly_dir,
        _ => &settings.daily_dir,
    }
}

fn period_format<'a>(settings: &'a Settings, period: &str) -> &'a str {
    match period {
        "weekly" => &settings.weekly_format,
        "monthly" => &settings.monthly_format,
        "quarterly" => &settings.quarterly_format,
        "yearly" => &settings.yearly_format,
        _ => &settings.daily_format,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rista-vault-settings-import-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".obsidian")).unwrap();
        root
    }

    fn write(root: &Path, path: &str, contents: &[u8]) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn plans_supported_fields_without_modifying_source_bytes() {
        let root = fixture();
        let source = br#"{"folder":"Daily","format":"YYYY/MM/DD","template":"daily"}"#;
        write(&root, ".obsidian/daily-notes.json", source);
        write(
            &root,
            ".obsidian/templates.json",
            br#"{"folder":"Templates"}"#,
        );
        write(&root, "Templates/daily.md", b"Daily template");
        write(
            &root,
            ".obsidian/app.json",
            br#"{"attachmentFolderPath":"./assets","newLinkFormat":"relative","userIgnoreFilters":["Archive","/private.*/"]}"#,
        );
        let settings = Settings::default();
        let plan = plan_import(&root, &settings).unwrap();
        let mut applied = settings.clone();
        plan.apply_settings(&mut applied).unwrap();
        assert_eq!(applied.daily_dir, "Daily");
        assert_eq!(applied.daily_format, "YYYY/MM/DD");
        assert_eq!(applied.templates_dir, "Templates");
        assert_eq!(applied.attachments_dir, "./assets");
        assert_eq!(applied.link_format, LinkFormat::Relative);
        assert_eq!(
            applied.period_templates.get("daily").map(String::as_str),
            Some("Templates/daily.md")
        );
        assert_eq!(applied.user_ignore_filters, ["Archive", "/private.*/"]);
        assert_eq!(
            fs::read(root.join(".obsidian/daily-notes.json")).unwrap(),
            source
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn templater_templates_take_precedence_only_when_the_plugin_is_enabled() {
        let root = fixture();
        write(
            &root,
            ".obsidian/templates.json",
            br#"{"folder":"Templates"}"#,
        );
        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"template":"daily"}"#,
        );
        write(
            &root,
            ".obsidian/plugins/templater-obsidian/data.json",
            br#"{"templates_folder":"Templater","enable_folder_templates":true,"folder_templates":[{"folder":"Notes","template":"daily"}]}"#,
        );
        write(&root, "Templates/daily.md", b"Regular template");
        write(&root, "Templater/daily.md", b"Templater template");
        write(
            &root,
            ".obsidian/community-plugins.json",
            br#"["templater-obsidian"]"#,
        );

        let plan = plan_import(&root, &Settings::default()).unwrap();
        assert_eq!(plan.proposed.templates_dir, "Templater");
        assert_eq!(
            plan.proposed
                .period_templates
                .get("daily")
                .map(String::as_str),
            Some("Templater/daily.md")
        );
        assert_eq!(
            plan.proposed
                .folder_templates
                .get("Notes")
                .map(String::as_str),
            Some("Templater/daily.md")
        );
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.problem == ImportProblem::Precedence));

        write(&root, ".obsidian/community-plugins.json", b"[]");
        let plan = plan_import(&root, &Settings::default()).unwrap();
        assert_eq!(plan.proposed.templates_dir, "Templates");
        assert_eq!(
            plan.proposed
                .period_templates
                .get("daily")
                .map(String::as_str),
            Some("Templates/daily.md")
        );
        assert!(!plan.proposed.folder_templates.contains_key("Notes"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn disabled_folder_templates_and_invalid_filter_arrays_preserve_existing_settings() {
        let root = fixture();
        write(
            &root,
            ".obsidian/plugins/templater-obsidian/data.json",
            br#"{"templates_folder":"Templates","trigger_on_file_creation_mode":"none","enable_folder_templates":true,"folder_templates":[{"folder":"Notes","template":"daily"}]}"#,
        );
        write(
            &root,
            ".obsidian/community-plugins.json",
            br#"["templater-obsidian"]"#,
        );
        write(
            &root,
            ".obsidian/app.json",
            br#"{"userIgnoreFilters":["valid",4]}"#,
        );
        write(&root, ".obsidian/daily-notes.json", br#"[]"#);
        let mut settings = Settings::default();
        settings.user_ignore_filters = vec!["existing".to_string()];
        let plan = plan_import(&root, &settings).unwrap();
        assert!(plan.proposed.folder_templates.is_empty());
        assert_eq!(plan.proposed.user_ignore_filters, ["existing"]);
        assert!(plan.warnings.iter().any(|warning| {
            warning.source == ".obsidian/daily-notes.json"
                && warning.problem == ImportProblem::InvalidFieldType
        }));
        assert!(plan.warnings.iter().any(|warning| {
            warning.category == ImportCategory::IgnoreFilters
                && warning.problem == ImportProblem::InvalidFieldType
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn target_validation_checks_only_imported_fields_and_safe_expanded_formats() {
        let root = fixture();
        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"folder":"Daily","format":"YYYY/MM/DD"}"#,
        );
        let mut settings = Settings::default();
        settings.templates_dir = "../preexisting-invalid".to_string();
        let plan = plan_import(&root, &settings).unwrap();
        assert!(plan.validate_targets());

        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"folder":"Daily","format":"../outside"}"#,
        );
        let plan = plan_import(&root, &settings).unwrap();
        assert!(plan.validate_targets());
        assert_eq!(plan.proposed.daily_format, settings.daily_format);
        assert!(plan.warnings.iter().any(|warning| {
            warning.problem == ImportProblem::UnsafeConfiguredPath
                && warning.detail.as_deref() == Some("format")
        }));

        write(
            &root,
            ".obsidian/plugins/periodic-notes/data.json",
            br#"{"weekly":{"folder":"Weeks","format":"../outside"}}"#,
        );
        let plan = plan_import(&root, &settings).unwrap();
        assert!(plan.validate_targets());
        assert_eq!(plan.proposed.weekly_dir, "Weeks");
        assert_eq!(plan.proposed.weekly_format, settings.weekly_format);
        assert!(plan.warnings.iter().any(|warning| {
            warning.category == ImportCategory::PeriodicNotes
                && warning.problem == ImportProblem::UnsafeConfiguredPath
                && warning.detail.as_deref() == Some("format")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn regex_and_missing_trigger_modes_do_not_import_folder_templates() {
        let root = fixture();
        write(
            &root,
            ".obsidian/community-plugins.json",
            br#"["templater-obsidian"]"#,
        );
        write(&root, "Templates/daily.md", b"Daily");
        let mut settings = Settings::default();
        settings.templates_dir = "Existing".to_string();
        settings.folder_templates.insert(
            "Existing folder".to_string(),
            "Existing template.md".to_string(),
        );

        write(
            &root,
            ".obsidian/plugins/templater-obsidian/data.json",
            br#"{"templates_folder":"Templates","trigger_on_file_creation_mode":"regex","enable_folder_templates":true,"folder_templates":[{"folder":"New folder","template":"daily"}]}"#,
        );
        let plan = plan_import(&root, &settings).unwrap();
        assert_eq!(plan.proposed.folder_templates, settings.folder_templates);
        assert!(plan.warnings.iter().any(|warning| {
            warning.problem == ImportProblem::UnsupportedValue
                && warning.detail.as_deref() == Some("trigger_on_file_creation_mode")
        }));

        write(
            &root,
            ".obsidian/plugins/templater-obsidian/data.json",
            br#"{"templates_folder":"Templates","folder_templates":[{"folder":"New folder","template":"daily"}]}"#,
        );
        let plan = plan_import(&root, &settings).unwrap();
        assert_eq!(plan.proposed.folder_templates, settings.folder_templates);
        assert!(!plan
            .changed_fields
            .contains(&ImportedField::FolderTemplate("New folder".to_string())));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn periodic_preview_labels_include_the_period() {
        let root = fixture();
        write(
            &root,
            ".obsidian/plugins/periodic-notes/data.json",
            br#"{"weekly":{"folder":"Weeks","format":"YYYY-[W]WW"},"monthly":{"folder":"Months","format":"MMMM-YYYY"}}"#,
        );
        let plan = plan_import(&root, &Settings::default()).unwrap();
        let fields = plan
            .preview
            .iter()
            .map(|change| change.field.as_str())
            .collect::<Vec<_>>();
        assert!(fields.contains(&"weekly directory"));
        assert!(fields.contains(&"weekly format"));
        assert!(fields.contains(&"monthly directory"));
        assert!(fields.contains(&"monthly format"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn template_references_must_resolve_to_markdown_files_inside_the_vault() {
        let root = fixture();
        let mut settings = Settings::default();
        settings
            .period_templates
            .insert("daily".to_string(), "Existing.md".to_string());
        write(&root, "Existing.md", b"Existing template");
        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"template":"[[Existing]]"}"#,
        );
        let plan = plan_import(&root, &settings).unwrap();
        let mut applied = settings.clone();
        plan.apply_settings(&mut applied).unwrap();
        assert_eq!(
            applied.period_templates.get("daily").map(String::as_str),
            Some("Existing.md")
        );

        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"template":"Missing"}"#,
        );
        let plan = plan_import(&root, &settings).unwrap();
        assert!(plan.warnings.iter().any(|warning| {
            warning.problem == ImportProblem::TemplateNotFound
                && warning.detail.as_deref() == Some("template")
        }));
        let mut applied = settings.clone();
        plan.apply_settings(&mut applied).unwrap();
        assert_eq!(
            applied.period_templates.get("daily").map(String::as_str),
            Some("Existing.md")
        );

        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"template":"../outside"}"#,
        );
        let plan = plan_import(&root, &settings).unwrap();
        assert!(plan
            .warnings
            .iter()
            .any(|warning| { warning.problem == ImportProblem::UnsafeConfiguredPath }));

        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"template":"Existing.txt"}"#,
        );
        let plan = plan_import(&root, &settings).unwrap();
        assert!(plan.warnings.iter().any(|warning| {
            warning.problem == ImportProblem::UnsupportedValue
                && warning.detail.as_deref() == Some("template extension")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_and_malformed_sources_are_distinct_and_preserve_settings() {
        let root = fixture();
        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"folder":17,"format":"YYYY-MM-DD"}"#,
        );
        write(&root, ".obsidian/templates.json", b"{broken");
        let mut settings = Settings::default();
        settings.daily_dir = "existing".to_string();
        let plan = plan_import(&root, &settings).unwrap();
        assert_eq!(
            plan.sources.get(".obsidian/templates.json"),
            Some(&SourceState::InvalidJson)
        );
        assert_eq!(
            plan.sources.get(".obsidian/app.json"),
            Some(&SourceState::Missing)
        );
        assert!(plan.warnings.iter().any(|warning| {
            warning.problem == ImportProblem::InvalidFieldType
                && warning.detail.as_deref() == Some("folder")
        }));
        let mut applied = settings.clone();
        plan.apply_settings(&mut applied).unwrap();
        assert_eq!(applied.daily_dir, "existing");
        assert_eq!(applied.daily_format, "YYYY-MM-DD");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_traversal_symlinks_and_oversized_files() {
        let root = fixture();
        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"folder":"../outside"}"#,
        );
        let plan = plan_import(&root, &Settings::default()).unwrap();
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.problem == ImportProblem::UnsafeConfiguredPath));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let other = root.parent().unwrap().join(format!(
                "rista-vault-settings-symlink-{}",
                std::process::id()
            ));
            fs::write(&other, b"{}").unwrap();
            fs::remove_file(root.join(".obsidian/daily-notes.json")).unwrap();
            symlink(&other, root.join(".obsidian/daily-notes.json")).unwrap();
            let plan = plan_import(&root, &Settings::default()).unwrap();
            assert_eq!(
                plan.sources.get(".obsidian/daily-notes.json"),
                Some(&SourceState::UnsafePath)
            );
            fs::remove_file(root.join(".obsidian/daily-notes.json")).unwrap();
            fs::remove_file(other).unwrap();
        }

        fs::write(
            root.join(".obsidian/daily-notes.json"),
            vec![b' '; (MAX_FILE_BYTES + 1) as usize],
        )
        .unwrap();
        let plan = plan_import(&root, &Settings::default()).unwrap();
        assert_eq!(
            plan.sources.get(".obsidian/daily-notes.json"),
            Some(&SourceState::TooLarge)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_stale_import_fields_but_preserves_unrelated_changes() {
        let root = fixture();
        write(
            &root,
            ".obsidian/daily-notes.json",
            br#"{"folder":"Daily"}"#,
        );
        let settings = Settings::default();
        let plan = plan_import(&root, &settings).unwrap();
        let mut latest = settings.clone();
        latest.appearance = crate::settings::Appearance::Light;
        plan.apply_settings(&mut latest).unwrap();
        assert_eq!(latest.daily_dir, "Daily");
        assert_eq!(latest.appearance, crate::settings::Appearance::Light);
        latest.daily_dir = "changed after preview".to_string();
        assert!(plan.apply_settings(&mut latest).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hotkey_mapping_is_explicit_and_empty_array_disables() {
        let root = fixture();
        write(
            &root,
            ".obsidian/hotkeys.json",
            br#"{"editor:save-file":[{"modifiers":["Mod","Shift"],"key":"S"}],"app:open-daily-note":[],"app:open-quick-switcher":[{"modifiers":["Mod"],"key":"O"}],"app:close":[{"modifiers":["Mod"],"key":"W"}],"editor:toggle-preview":[{"modifiers":["Mod"],"key":"E"}],"unknown:run":[{"modifiers":[],"key":"X"}]}"#,
        );
        let plan = plan_import(&root, &Settings::default()).unwrap();
        assert_eq!(
            plan.hotkeys.get("save"),
            Some(&Some("secondary-shift-s".to_string()))
        );
        assert_eq!(plan.hotkeys.get("open_daily_note"), Some(&None));
        assert_eq!(
            plan.hotkeys.get("quick_open"),
            Some(&Some("secondary-o".to_string()))
        );
        assert_eq!(
            plan.hotkeys.get("close_tab"),
            Some(&Some("secondary-w".to_string()))
        );
        assert!(plan.warnings.iter().any(|warning| {
            warning.problem == ImportProblem::UnsupportedCommand
                && warning.detail.as_deref() == Some("unknown:run")
        }));
        assert!(plan.warnings.iter().any(|warning| {
            warning.problem == ImportProblem::UnsupportedCommand
                && warning.detail.as_deref() == Some("editor:toggle-preview")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn regex_filters_are_compiled_and_unsupported_globs_are_rejected() {
        assert_eq!(validate_filter("Archive/Old.md").unwrap(), "Archive/Old.md");
        assert_eq!(validate_filter("/^private/").unwrap(), "/^private/");
        assert_eq!(
            validate_filter("**/*.md"),
            Err(ImportProblem::UnsupportedValue)
        );
        assert_eq!(validate_filter("/[/"), Err(ImportProblem::InvalidRegex));
    }
}
