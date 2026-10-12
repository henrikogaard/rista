//! User-authored, data-only style snippets.
//!
//! Snippets deliberately stay below the rendering layer: this module owns
//! validation, safe loading, deterministic precedence, and immutable style
//! resolution. The application decides where each resolved role is painted.

use gpui_kit::component::theme::{ThemeConfig, ThemeConfigColors};
use gpui_kit::Hsla;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

pub const MAX_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_ENTRIES: usize = 128;
const MAX_TEXT: usize = 128;

const RESERVED_STEMS: &[&str] = &["wide", "narrow", "readable"];
const ROLE_ALIASES: &[&str] = &[
    "foreground",
    "muted.foreground",
    "link",
    "info.background",
    "warning",
    "danger",
    "success",
    "accent.background",
    "background",
    "border",
];
const CALLOUT_KINDS: &[&str] = &[
    "note",
    "abstract",
    "summary",
    "tldr",
    "info",
    "todo",
    "tip",
    "hint",
    "important",
    "success",
    "check",
    "done",
    "question",
    "help",
    "faq",
    "warning",
    "caution",
    "attention",
    "failure",
    "fail",
    "missing",
    "danger",
    "error",
    "bug",
    "example",
    "quote",
    "cite",
];

static COLOR_TOKENS: LazyLock<BTreeSet<String>> = LazyLock::new(|| {
    serde_json::to_value(ThemeConfigColors::default())
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
});

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Typography {
    pub body_font: Option<String>,
    pub editor_font: Option<String>,
    pub code_font: Option<String>,
    pub heading_font: Option<String>,
    pub body_size: Option<f32>,
    pub line_height: Option<f32>,
    pub heading_sizes: Option<Vec<Option<f32>>>,
    pub heading_weights: Option<Vec<Option<u16>>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SnippetFile {
    pub colors: BTreeMap<String, String>,
    pub light_colors: BTreeMap<String, String>,
    pub dark_colors: BTreeMap<String, String>,
    pub typography: Option<Typography>,
    pub readable_width: Option<f32>,
    pub callout_tokens: BTreeMap<String, String>,
    pub tag_token: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snippet {
    pub name: String,
    pub filename: String,
    pub file: SnippetFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WarningReason {
    InvalidJson,
    InvalidUtf8,
    UnknownField,
    UnknownColorToken,
    InvalidColor,
    InvalidValue,
    Oversized,
    Unreadable,
    Symlink,
    NotRegularFile,
    ReservedName,
    TooManyEntries,
    DuplicateName,
}

impl WarningReason {
    pub fn text(&self) -> (&'static str, &'static str) {
        match self {
            Self::InvalidJson => ("Invalid JSON", "Ugyldig JSON"),
            Self::InvalidUtf8 => ("The file is not UTF-8", "Filen er ikke UTF-8"),
            Self::UnknownField => ("Unknown field", "Ukjent felt"),
            Self::UnknownColorToken => ("Unknown color token", "Ukjent fargetoken"),
            Self::InvalidColor => ("Invalid color", "Ugyldig farge"),
            Self::InvalidValue => ("Invalid value", "Ugyldig verdi"),
            Self::Oversized => ("File is larger than 64 KiB", "Filen er større enn 64 KiB"),
            Self::Unreadable => ("File could not be read", "Filen kunne ikke leses"),
            Self::Symlink => (
                "Symbolic links are not allowed",
                "Symbolske lenker er ikke tillatt",
            ),
            Self::NotRegularFile => (
                "Only regular files are supported",
                "Bare vanlige filer støttes",
            ),
            Self::ReservedName => ("This filename is reserved", "Dette filnavnet er reservert"),
            Self::TooManyEntries => (
                "The snippet limit is 128 files",
                "Utdragsgrensen er 128 filer",
            ),
            Self::DuplicateName => ("Duplicate snippet name", "Duplikatnavn for utdrag"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnippetWarning {
    pub filename: String,
    pub reason: WarningReason,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    entries: BTreeMap<String, Snippet>,
    warnings: Vec<SnippetWarning>,
}

impl Catalog {
    pub fn entries(&self) -> &BTreeMap<String, Snippet> {
        &self.entries
    }

    pub fn warnings(&self) -> &[SnippetWarning] {
        &self.warnings
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResolvedStyle {
    pub colors: BTreeMap<String, Hsla>,
    pub typography: Typography,
    pub callout_tokens: BTreeMap<String, String>,
    pub tag_token: Option<String>,
    pub readable_width: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ValidationError {
    UnknownColorToken,
    InvalidColor,
    InvalidValue,
}

impl ValidationError {
    fn reason(&self) -> WarningReason {
        match self {
            Self::UnknownColorToken => WarningReason::UnknownColorToken,
            Self::InvalidColor => WarningReason::InvalidColor,
            Self::InvalidValue => WarningReason::InvalidValue,
        }
    }
}

pub fn snippets_dir() -> PathBuf {
    crate::settings::config_dir().join("snippets")
}

pub(crate) fn color_token_for_role(role: &str) -> &str {
    match role {
        "warning" => "warning.background",
        "danger" => "danger.background",
        "success" => "success.background",
        _ => role,
    }
}

pub fn parse(
    raw: &[u8],
    name: impl Into<String>,
    filename: impl Into<String>,
) -> Result<Snippet, WarningReason> {
    if raw.len() as u64 > MAX_FILE_BYTES {
        return Err(WarningReason::Oversized);
    }
    let text = std::str::from_utf8(raw).map_err(|_| WarningReason::InvalidUtf8)?;
    let value =
        serde_json::from_str::<serde_json::Value>(text).map_err(|_| WarningReason::InvalidJson)?;
    let file: SnippetFile = serde_json::from_value(value).map_err(|error| {
        if error.to_string().contains("unknown field") {
            WarningReason::UnknownField
        } else {
            WarningReason::InvalidValue
        }
    })?;
    validate(&file).map_err(|error| error.reason())?;
    Ok(Snippet {
        name: name.into(),
        filename: filename.into(),
        file,
    })
}

fn validate(file: &SnippetFile) -> Result<(), ValidationError> {
    for map in [&file.colors, &file.light_colors, &file.dark_colors] {
        for (token, value) in map {
            if !COLOR_TOKENS.contains(token) {
                return Err(ValidationError::UnknownColorToken);
            }
            if !valid_hex(value) || gpui_kit::component::try_parse_color(value).is_err() {
                return Err(ValidationError::InvalidColor);
            }
        }
    }
    if let Some(typography) = &file.typography {
        for font in [
            &typography.body_font,
            &typography.editor_font,
            &typography.code_font,
            &typography.heading_font,
        ]
        .into_iter()
        .flatten()
        {
            if font.trim().is_empty() || font.len() > MAX_TEXT {
                return Err(ValidationError::InvalidValue);
            }
        }
        if typography
            .body_size
            .is_some_and(|value| !value.is_finite() || !(8.0..=72.0).contains(&value))
            || typography
                .line_height
                .is_some_and(|value| !value.is_finite() || !(1.0..=3.0).contains(&value))
            || invalid_heading_values(typography.heading_sizes.as_deref(), 120.0)
            || invalid_weights(typography.heading_weights.as_deref())
        {
            return Err(ValidationError::InvalidValue);
        }
    }
    if file
        .readable_width
        .is_some_and(|value| !value.is_finite() || !(240.0..=2000.0).contains(&value))
    {
        return Err(ValidationError::InvalidValue);
    }
    if file
        .callout_tokens
        .keys()
        .any(|kind| !CALLOUT_KINDS.contains(&kind.as_str()))
    {
        return Err(ValidationError::InvalidValue);
    }
    for value in file.callout_tokens.values().chain(file.tag_token.iter()) {
        if !ROLE_ALIASES.contains(&value.as_str()) {
            return Err(ValidationError::InvalidValue);
        }
    }
    Ok(())
}

fn invalid_heading_values(values: Option<&[Option<f32>]>, max: f32) -> bool {
    values.is_some_and(|values| {
        values.len() != 6
            || values
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || !(8.0..=max).contains(value))
    })
}

fn invalid_weights(values: Option<&[Option<u16>]>) -> bool {
    values.is_some_and(|values| {
        values.len() != 6
            || values
                .iter()
                .flatten()
                .any(|value| !(100..=900).contains(value))
    })
}

fn valid_hex(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn scan(path: &Path) -> Catalog {
    let mut catalog = Catalog::default();
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            catalog.warnings.push(SnippetWarning {
                filename: "snippets".to_string(),
                reason: WarningReason::Unreadable,
            });
            return catalog;
        }
    };
    let mut files = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) if entry.path().extension().is_some_and(|ext| ext == "json") => {
                files.push(entry)
            }
            Ok(_) => {}
            Err(_) => catalog.warnings.push(SnippetWarning {
                filename: "snippets".to_string(),
                reason: WarningReason::Unreadable,
            }),
        }
    }
    files.sort_by_key(|entry| entry.file_name());
    for (index, entry) in files.into_iter().enumerate() {
        let path = entry.path();
        let filename = entry.file_name().to_string_lossy().to_string();
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        if index >= MAX_ENTRIES {
            catalog.warnings.push(SnippetWarning {
                filename,
                reason: WarningReason::TooManyEntries,
            });
            continue;
        }
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                catalog.warnings.push(SnippetWarning {
                    filename,
                    reason: WarningReason::Unreadable,
                });
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            catalog.warnings.push(SnippetWarning {
                filename,
                reason: WarningReason::Symlink,
            });
            continue;
        }
        if !metadata.is_file() {
            catalog.warnings.push(SnippetWarning {
                filename,
                reason: WarningReason::NotRegularFile,
            });
            continue;
        }
        if metadata.len() > MAX_FILE_BYTES {
            catalog.warnings.push(SnippetWarning {
                filename,
                reason: WarningReason::Oversized,
            });
            continue;
        }
        if RESERVED_STEMS.contains(&name.as_str()) {
            catalog.warnings.push(SnippetWarning {
                filename,
                reason: WarningReason::ReservedName,
            });
            continue;
        }
        let raw = match std::fs::File::open(&path).and_then(|file| {
            let mut raw = Vec::new();
            file.take(MAX_FILE_BYTES + 1).read_to_end(&mut raw)?;
            Ok(raw)
        }) {
            Ok(raw) if raw.len() as u64 <= MAX_FILE_BYTES => raw,
            Ok(_) => {
                catalog.warnings.push(SnippetWarning {
                    filename,
                    reason: WarningReason::Oversized,
                });
                continue;
            }
            Err(_) => {
                catalog.warnings.push(SnippetWarning {
                    filename,
                    reason: WarningReason::Unreadable,
                });
                continue;
            }
        };
        match parse(&raw, name.clone(), filename.clone()) {
            Ok(snippet) => match catalog.entries.entry(name) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(snippet);
                }
                std::collections::btree_map::Entry::Occupied(_) => {
                    catalog.warnings.push(SnippetWarning {
                        filename,
                        reason: WarningReason::DuplicateName,
                    });
                }
            },
            Err(reason) => catalog.warnings.push(SnippetWarning { filename, reason }),
        }
    }
    catalog
}

pub fn resolve(
    catalog: &Catalog,
    enabled: &[String],
    classes: &[String],
    dark: bool,
) -> ResolvedStyle {
    let enabled = enabled.iter().collect::<BTreeSet<_>>();
    let mut resolved = ResolvedStyle::default();
    for (name, snippet) in &catalog.entries {
        if enabled.contains(name) {
            apply_snippet(&mut resolved, snippet, dark);
        }
    }
    let mut applied = HashSet::new();
    for class in classes {
        if applied.insert(class) {
            if let Some(snippet) = catalog.entries.get(class) {
                apply_snippet(&mut resolved, snippet, dark);
            }
        }
    }
    resolved
}

fn apply_snippet(resolved: &mut ResolvedStyle, snippet: &Snippet, dark: bool) {
    let file = &snippet.file;
    for (token, value) in &file.colors {
        if let Ok(color) = gpui_kit::component::try_parse_color(value) {
            resolved.colors.insert(token.clone(), color);
        }
    }
    let mode_colors = if dark {
        &file.dark_colors
    } else {
        &file.light_colors
    };
    for (token, value) in mode_colors {
        if let Ok(color) = gpui_kit::component::try_parse_color(value) {
            resolved.colors.insert(token.clone(), color);
        }
    }
    if let Some(typography) = &file.typography {
        merge_typography(&mut resolved.typography, typography);
    }
    if file.readable_width.is_some() {
        resolved.readable_width = file.readable_width;
    }
    resolved.callout_tokens.extend(file.callout_tokens.clone());
    if file.tag_token.is_some() {
        resolved.tag_token = file.tag_token.clone();
    }
}

fn merge_typography(target: &mut Typography, source: &Typography) {
    for (target, source) in [
        (&mut target.body_font, &source.body_font),
        (&mut target.editor_font, &source.editor_font),
        (&mut target.code_font, &source.code_font),
        (&mut target.heading_font, &source.heading_font),
    ] {
        if source.is_some() {
            *target = source.clone();
        }
    }
    for (target, source) in [
        (&mut target.body_size, &source.body_size),
        (&mut target.line_height, &source.line_height),
    ] {
        if source.is_some() {
            *target = *source;
        }
    }
    if source.heading_sizes.is_some() {
        target.heading_sizes = source.heading_sizes.clone();
    }
    if source.heading_weights.is_some() {
        target.heading_weights = source.heading_weights.clone();
    }
}

pub fn patch_theme(
    theme: &ThemeConfig,
    catalog: &Catalog,
    enabled: &[String],
    dark: bool,
) -> ThemeConfig {
    let enabled = enabled.iter().collect::<BTreeSet<_>>();
    let mut value = match serde_json::to_value(theme) {
        Ok(value) => value,
        Err(_) => return theme.clone(),
    };
    let Some(colors) = value
        .get_mut("colors")
        .and_then(|value| value.as_object_mut())
    else {
        return theme.clone();
    };
    for (name, snippet) in &catalog.entries {
        if !enabled.contains(name) {
            continue;
        }
        for (token, color) in &snippet.file.colors {
            colors.insert(token.clone(), serde_json::Value::String(color.clone()));
        }
        let mode_colors = if dark {
            &snippet.file.dark_colors
        } else {
            &snippet.file.light_colors
        };
        for (token, color) in mode_colors {
            colors.insert(token.clone(), serde_json::Value::String(color.clone()));
        }
    }
    serde_json::from_value(value).unwrap_or_else(|_| theme.clone())
}

#[derive(Clone)]
pub struct StyleSnippetCatalog {
    pub snapshot: Arc<Catalog>,
    pub watcher_warning: bool,
}

impl StyleSnippetCatalog {
    pub fn new(snapshot: Catalog) -> Self {
        Self {
            snapshot: Arc::new(snapshot),
            watcher_warning: false,
        }
    }

    pub fn with_watcher_warning(mut self) -> Self {
        self.watcher_warning = true;
        self
    }
}

impl gpui_kit::Global for StyleSnippetCatalog {}

#[derive(Clone)]
pub struct StyleSnippetWatcher {
    watcher: Arc<std::sync::Mutex<notify::RecommendedWatcher>>,
    directory_watched: Arc<std::sync::atomic::AtomicBool>,
}

impl StyleSnippetWatcher {
    pub fn new(watcher: notify::RecommendedWatcher, directory_watched: bool) -> Self {
        Self {
            watcher: Arc::new(std::sync::Mutex::new(watcher)),
            directory_watched: Arc::new(std::sync::atomic::AtomicBool::new(directory_watched)),
        }
    }

    pub fn watch_directory(&self, directory: &Path) -> bool {
        use notify::Watcher;
        use std::sync::atomic::Ordering;

        if !directory.is_dir() {
            if self.directory_watched.swap(false, Ordering::Relaxed) {
                if let Ok(mut watcher) = self.watcher.lock() {
                    let _ = watcher.unwatch(directory);
                }
            }
            return false;
        }
        if self.directory_watched.load(Ordering::Relaxed) {
            return true;
        }
        if let Ok(mut watcher) = self.watcher.lock() {
            if watcher
                .watch(directory, notify::RecursiveMode::NonRecursive)
                .is_ok()
            {
                self.directory_watched.store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }
}

impl gpui_kit::Global for StyleSnippetWatcher {}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_file(raw: &str) -> Snippet {
        parse(raw.as_bytes(), "paper", "paper.json").expect("valid snippet")
    }

    #[test]
    fn strict_schema_rejects_unknown_fields_and_bad_bounds() {
        assert_eq!(
            parse(r#"{"unknown":true}"#.as_bytes(), "bad", "bad.json"),
            Err(WarningReason::UnknownField)
        );
        assert_eq!(
            parse(r#"{"readable_width":239}"#.as_bytes(), "bad", "bad.json"),
            Err(WarningReason::InvalidValue)
        );
        assert_eq!(
            parse(
                r##"{"colors":{"not-a-token":"#fff000"}}"##.as_bytes(),
                "bad",
                "bad.json"
            ),
            Err(WarningReason::UnknownColorToken)
        );
    }

    #[test]
    fn semantic_color_roles_use_existing_theme_tokens() {
        assert_eq!(color_token_for_role("warning"), "warning.background");
        assert_eq!(color_token_for_role("danger"), "danger.background");
        assert_eq!(color_token_for_role("success"), "success.background");
        assert_eq!(color_token_for_role("link"), "link");
    }

    #[test]
    fn resolve_uses_sorted_globals_then_each_class_once_and_mode_overlay() {
        let mut catalog = Catalog::default();
        catalog.entries.insert(
            "a".into(),
            parse_file(r##"{"colors":{"foreground":"#111111"},"dark_colors":{"foreground":"#222222"},"readable_width":500}"##),
        );
        catalog.entries.insert(
            "b".into(),
            parse_file(r##"{"colors":{"foreground":"#333333"},"light_colors":{"foreground":"#444444"},"readable_width":600}"##),
        );
        let resolved = resolve(
            &catalog,
            &["b".into(), "a".into()],
            &["b".into(), "a".into(), "b".into()],
            true,
        );
        assert_eq!(resolved.readable_width, Some(500.));
        assert_eq!(
            resolved.colors["foreground"],
            gpui_kit::component::try_parse_color("#222222").unwrap()
        );
    }

    #[test]
    fn theme_overlay_leaves_catalog_theme_unchanged_and_disable_restores_it() {
        let theme = ThemeConfig::default();
        let before = serde_json::to_value(&theme).unwrap();
        let mut catalog = Catalog::default();
        catalog.entries.insert(
            "paper".into(),
            parse_file(r##"{"colors":{"foreground":"#112233"}}"##),
        );

        let enabled = patch_theme(&theme, &catalog, &["paper".into()], true);
        let disabled = patch_theme(&theme, &catalog, &[], true);

        assert_eq!(serde_json::to_value(&theme).unwrap(), before);
        assert_eq!(serde_json::to_value(disabled).unwrap(), before);
        assert_eq!(
            serde_json::to_value(enabled).unwrap()["colors"]["foreground"],
            "#112233"
        );
    }

    #[test]
    fn removed_or_invalid_snippets_fall_back_to_default_note_style() {
        let mut catalog = Catalog::default();
        catalog.entries.insert(
            "paper".into(),
            parse_file(r##"{"colors":{"foreground":"#112233"}}"##),
        );
        let local = resolve(&catalog, &[], &["paper".into()], true);
        assert!(local.colors.contains_key("foreground"));
        assert!(resolve(&catalog, &[], &[], true).colors.is_empty());
        assert_eq!(
            parse(
                r#"{"typography":{"unexpected":true}}"#.as_bytes(),
                "bad",
                "bad.json"
            ),
            Err(WarningReason::UnknownField)
        );

        catalog.entries.remove("paper");
        assert!(
            resolve(&catalog, &["paper".into()], &["paper".into()], true)
                .colors
                .is_empty()
        );
    }

    #[test]
    fn parse_example_round_trips() {
        let example = include_str!("../docs/style-snippets.md")
            .split("```json\n")
            .nth(1)
            .and_then(|block| block.split("```").next())
            .expect("documentation JSON example");
        let snippet = parse_file(example.trim());
        let json = serde_json::to_string_pretty(&snippet.file).unwrap();
        assert_eq!(parse_file(&json).file, snippet.file);
    }

    #[test]
    fn scan_rejects_symlinks_and_oversized_files() {
        let dir = std::env::temp_dir().join(format!("rista-style-snippets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("large.json"),
            vec![b'x'; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        std::fs::write(
            dir.join("ok.json"),
            br##"{"colors":{"foreground":"#111111"}}"##,
        )
        .unwrap();
        std::fs::write(dir.join("invalid.json"), b"{").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("ok.json"), dir.join("link.json")).unwrap();
        let catalog = scan(&dir);
        assert!(catalog.entries.contains_key("ok"));
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.reason == WarningReason::Oversized));
        assert!(!catalog.entries.contains_key("invalid"));
        assert!(catalog.warnings.iter().any(|warning| {
            warning.filename == "invalid.json" && warning.reason == WarningReason::InvalidJson
        }));
        #[cfg(unix)]
        assert!(catalog
            .warnings
            .iter()
            .any(|warning| warning.reason == WarningReason::Symlink));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_reports_entries_beyond_the_catalog_limit() {
        let dir =
            std::env::temp_dir().join(format!("rista-style-snippet-limit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for index in 0..=MAX_ENTRIES {
            std::fs::write(dir.join(format!("{index:03}.json")), br#"{"colors":{}}"#).unwrap();
        }

        let catalog = scan(&dir);

        assert_eq!(catalog.entries.len(), MAX_ENTRIES);
        assert!(catalog.warnings.iter().any(|warning| {
            warning.filename == format!("{:03}.json", MAX_ENTRIES)
                && warning.reason == WarningReason::TooManyEntries
        }));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
