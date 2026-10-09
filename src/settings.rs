//! Persisted user settings — a small JSON file, no database.

use gpui_kit::component::theme::ThemeMode;
use gpui_kit::{App, WindowAppearance};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    Norwegian,
}

impl Language {
    pub fn text(self, english: &'static str, norwegian: &'static str) -> &'static str {
        match self {
            Self::English => english,
            Self::Norwegian => norwegian,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    #[default]
    Source,
    Split,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PropertiesVisibility {
    #[default]
    Expanded,
    Collapsed,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TreeSort {
    #[default]
    Name,
    Modified,
    Type,
    Size,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub terminal_shell: String,
    pub terminal_font: String,
    pub terminal_font_size: f32,
    pub expanded_folders: std::collections::HashMap<String, Vec<String>>,
    pub folder_file_list: bool,
    pub preview_follows_source: bool,
    pub show_other_files: bool,
    pub language: Language,
    pub inspector_open: bool,
    pub graph_dock_open: bool,
    pub agent_open: bool,
    pub active_folder: Option<PathBuf>,
    pub appearance: Appearance,
    #[serde(deserialize_with = "deserialize_theme_name")]
    pub dark_theme: String,
    #[serde(deserialize_with = "deserialize_theme_name")]
    pub light_theme: String,
    pub properties_visibility: PropertiesVisibility,
    /// Editor font family — a writing-first set, resolved by name.
    pub editor_font_family: String,
    pub editor_font_size: f32,
    pub ui_font_size: f32,
    pub soft_wrap: bool,
    pub show_line_numbers: bool,
    pub tab_size: usize,
    pub view_mode: ViewMode,
    pub sidebar_collapsed: bool,
    /// the reference-editor style: reveal markup around the cursor (live preview).
    /// Kept off — the raw buffer stays honest.
    pub focus_mode: bool,
    /// The folder that was open last — reopened on launch.
    pub last_vault: Option<std::path::PathBuf>,
    /// Vault-relative folder pasted/dropped images are copied into
    /// (the "attachment folder path").
    pub attachments_dir: String,
    /// Vault-relative folder the template picker and the daily-note
    /// template live in (the template folder setting).
    #[serde(default = "default_templates_dir")]
    pub templates_dir: String,
    /// Vault-relative folder daily notes are created in (the daily-notes "new file location"; empty = vault root).
    #[serde(default)]
    pub daily_dir: String,
    /// Daily-note filename pattern in Moment syntax, e.g.
    /// `YYYY-MM-DD` → `2026-10-05.md` (the daily-notes
    /// "date format").
    #[serde(default = "default_daily_format")]
    pub daily_format: String,
    /// Vault-relative folder "New unique note" creates notes in (empty = vault root).
    #[serde(default)]
    pub unique_note_dir: String,
    /// Unique-note filename prefix in Moment syntax, e.g.
    /// `YYYYMMDDHHmm` → `202610091430.md` (the Zettelkasten prefix).
    #[serde(default = "default_unique_note_format")]
    pub unique_note_format: String,
    /// Periodic notes (the Periodic Notes plugin's settings): folder
    /// (empty = vault root) and Moment filename format per period.
    #[serde(default)]
    pub weekly_dir: String,
    #[serde(default = "default_weekly_format")]
    pub weekly_format: String,
    #[serde(default)]
    pub monthly_dir: String,
    #[serde(default = "default_monthly_format")]
    pub monthly_format: String,
    #[serde(default)]
    pub quarterly_dir: String,
    #[serde(default = "default_quarterly_format")]
    pub quarterly_format: String,
    #[serde(default)]
    pub yearly_dir: String,
    #[serde(default = "default_yearly_format")]
    pub yearly_format: String,
    /// Starred notes — absolute paths, shown pinned at the sidebar top.
    pub starred: Vec<String>,
    /// Which sidebar panes are expanded — persists across launches.
    pub panes: SidebarPanes,
    /// Document tabs open when the app closed — restored on launch.
    pub open_tabs: Vec<String>,
    /// Active tab's path (one of `open_tabs`) when the app closed.
    pub active_tab: Option<String>,
    /// Which `.base` view (index into `views:`) each opened `.base`
    /// file was last left on — note path → view index.
    pub base_views: std::collections::HashMap<String, usize>,
    /// Interactive header sort per `.base` view —
    /// `{path}::{view_name}` → (column header, descending).
    pub base_sorts: std::collections::HashMap<String, (String, bool)>,
    /// Pinned document tabs — survive close-others/close-right and
    /// show a pin glyph instead of the × button.
    pub pinned_tabs: Vec<String>,
    /// Readable line length — cap the editor at a centered column
    /// (the editor setting).
    pub readable_width: bool,
    /// File-tree ordering — the explorer offers name/modified.
    pub tree_sort: TreeSort,
}

fn default_true() -> bool {
    true
}

fn default_templates_dir() -> String {
    "templates".to_string()
}

fn default_daily_format() -> String {
    "YYYY-MM-DD".to_string()
}

fn default_unique_note_format() -> String {
    "YYYYMMDDHHmm".to_string()
}

/// ISO weeks — Monday-first like the calendar. Periodic Notes' own
/// default, `gggg-[W]ww`, also works (the calendar then starts Sunday).
fn default_weekly_format() -> String {
    "GGGG-[W]WW".to_string()
}

fn default_monthly_format() -> String {
    "YYYY-MM".to_string()
}

fn default_quarterly_format() -> String {
    "YYYY-[Q]Q".to_string()
}

fn default_yearly_format() -> String {
    "YYYY".to_string()
}

/// A periodic-note cadence. `Day` is the daily note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Day,
    Week,
    Month,
    Quarter,
    Year,
}

impl Period {
    pub const ALL: [Period; 5] = [
        Period::Day,
        Period::Week,
        Period::Month,
        Period::Quarter,
        Period::Year,
    ];

    /// `daily`, `weekly`, … — also the template file stem
    /// (`<templates_dir>/weekly.md`).
    pub fn name(self) -> &'static str {
        match self {
            Period::Day => "daily",
            Period::Week => "weekly",
            Period::Month => "monthly",
            Period::Quarter => "quarterly",
            Period::Year => "yearly",
        }
    }

    /// `date` moved by `n` periods (negative goes back); month steps
    /// clamp to the month's last day.
    pub fn shift(self, date: chrono::NaiveDate, n: i32) -> chrono::NaiveDate {
        let months = |m: i32| {
            let step = chrono::Months::new(m.unsigned_abs());
            if m >= 0 {
                date.checked_add_months(step)
            } else {
                date.checked_sub_months(step)
            }
            .unwrap_or(date)
        };
        match self {
            Period::Day => date + chrono::Duration::days(n as i64),
            Period::Week => date + chrono::Duration::weeks(n as i64),
            Period::Month => months(n),
            Period::Quarter => months(n * 3),
            Period::Year => months(n * 12),
        }
    }
}

impl Settings {
    /// Folder and Moment filename format for `period`'s notes.
    pub fn period(&self, period: Period) -> (&str, &str) {
        match period {
            Period::Day => (&self.daily_dir, &self.daily_format),
            Period::Week => (&self.weekly_dir, &self.weekly_format),
            Period::Month => (&self.monthly_dir, &self.monthly_format),
            Period::Quarter => (&self.quarterly_dir, &self.quarterly_format),
            Period::Year => (&self.yearly_dir, &self.yearly_format),
        }
    }

    pub fn period_mut(&mut self, period: Period) -> (&mut String, &mut String) {
        match period {
            Period::Day => (&mut self.daily_dir, &mut self.daily_format),
            Period::Week => (&mut self.weekly_dir, &mut self.weekly_format),
            Period::Month => (&mut self.monthly_dir, &mut self.monthly_format),
            Period::Quarter => (&mut self.quarterly_dir, &mut self.quarterly_format),
            Period::Year => (&mut self.yearly_dir, &mut self.yearly_format),
        }
    }
}

/// Expanded/collapsed state of the optional sidebar sections.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarPanes {
    #[serde(default = "default_true")]
    pub starred: bool,
    #[serde(default = "default_true")]
    pub outline: bool,
    #[serde(default = "default_true")]
    pub backlinks: bool,
    #[serde(default = "default_true")]
    pub outgoing: bool,
    #[serde(default = "default_true")]
    pub tasks: bool,
    #[serde(default = "default_true")]
    pub calendar: bool,
    #[serde(default = "default_true")]
    pub tags: bool,
}

impl Default for SidebarPanes {
    fn default() -> Self {
        Self {
            starred: true,
            outline: true,
            backlinks: true,
            outgoing: true,
            tasks: true,
            calendar: true,
            tags: true,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            terminal_shell: String::new(),
            terminal_font: if cfg!(target_os = "macos") {
                "Menlo"
            } else {
                "monospace"
            }
            .into(),
            terminal_font_size: 13.,
            expanded_folders: Default::default(),
            folder_file_list: false,
            preview_follows_source: true,
            show_other_files: false,
            language: Language::default(),
            inspector_open: false,
            graph_dock_open: false,
            agent_open: false,
            active_folder: None,
            appearance: Appearance::Dark,
            dark_theme: crate::theme::NIGHT_THEME_NAME.to_string(),
            light_theme: crate::theme::DAY_THEME_NAME.to_string(),
            properties_visibility: PropertiesVisibility::default(),
            editor_font_family: "SF Mono".to_string(),
            editor_font_size: 14.0,
            ui_font_size: 13.0,
            soft_wrap: true,
            show_line_numbers: true,
            tab_size: 4,
            view_mode: ViewMode::Source,
            sidebar_collapsed: false,
            focus_mode: false,
            last_vault: None,
            attachments_dir: "attachments".to_string(),
            templates_dir: default_templates_dir(),
            daily_dir: String::new(),
            daily_format: default_daily_format(),
            unique_note_dir: String::new(),
            unique_note_format: default_unique_note_format(),
            weekly_dir: String::new(),
            weekly_format: default_weekly_format(),
            monthly_dir: String::new(),
            monthly_format: default_monthly_format(),
            quarterly_dir: String::new(),
            quarterly_format: default_quarterly_format(),
            yearly_dir: String::new(),
            yearly_format: default_yearly_format(),
            starred: Vec::new(),
            panes: SidebarPanes::default(),
            open_tabs: Vec::new(),
            active_tab: None,
            base_views: std::collections::HashMap::new(),
            base_sorts: std::collections::HashMap::new(),
            pinned_tabs: Vec::new(),
            readable_width: false,
            tree_sort: TreeSort::default(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    directories::ProjectDirs::from("no", "ogard", "rista")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn deserialize_theme_name<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    Ok(match name.as_str() {
        "Rísta Night" => "Slate Night".into(),
        "Rísta Day" => "Slate Day".into(),
        _ => name,
    })
}

pub fn config_path() -> PathBuf {
    config_dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_path(), json);
        }
    }

    /// Resolve the appearance pref into a concrete mode — `System`
    /// follows the OS appearance reported by the windowing layer.
    pub fn theme_mode(&self, cx: &mut App) -> ThemeMode {
        match self.appearance {
            Appearance::Dark => ThemeMode::Dark,
            Appearance::Light => ThemeMode::Light,
            Appearance::System => match cx.window_appearance() {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
                _ => ThemeMode::Light,
            },
        }
    }
}

/// Editor font candidates for the settings picker.
pub const EDITOR_FONTS: &[&str] = &[
    "SF Mono",
    "Menlo",
    "JetBrains Mono",
    "iA Writer Mono",
    "New York",
    "Charter",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_settings_keep_defaults_and_new_preferences_roundtrip() {
        let mut settings: Settings = serde_json::from_str(r#"{"appearance":"light"}"#).unwrap();
        assert_eq!(
            settings.properties_visibility,
            PropertiesVisibility::Expanded
        );
        assert_eq!(settings.dark_theme, crate::theme::NIGHT_THEME_NAME);
        assert!(!settings.show_other_files);
        settings.show_other_files = true;
        settings.tree_sort = TreeSort::Type;
        settings.dark_theme = "Rose Night".into();
        settings.light_theme = "Fjord Day".into();
        settings.properties_visibility = PropertiesVisibility::Hidden;
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.dark_theme, "Rose Night");
        assert!(restored.show_other_files);
        assert_eq!(restored.tree_sort, TreeSort::Type);
        assert_eq!(restored.light_theme, "Fjord Day");
        assert_eq!(restored.properties_visibility, PropertiesVisibility::Hidden);
    }

    #[test]
    fn theme_defaults_migrate_only_legacy_original_names() {
        let defaults: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.dark_theme, "Graphite Night");
        assert_eq!(defaults.light_theme, "Graphite Day");
        let migrated: Settings =
            serde_json::from_str(r#"{"dark_theme":"Rísta Night","light_theme":"Rísta Day"}"#)
                .unwrap();
        assert_eq!(migrated.dark_theme, "Slate Night");
        assert_eq!(migrated.light_theme, "Slate Day");
        let custom: Settings =
            serde_json::from_str(r#"{"dark_theme":"Fjord Night","light_theme":"Rose Day"}"#)
                .unwrap();
        assert_eq!(custom.dark_theme, "Fjord Night");
        assert_eq!(custom.light_theme, "Rose Day");
        let preserved: Settings = serde_json::from_str(
            r#"{"dark_theme":"Mono Night","light_theme":"Custom Quiet","appearance":"light"}"#,
        )
        .unwrap();
        assert_eq!(preserved.dark_theme, "Mono Night");
        assert_eq!(preserved.light_theme, "Custom Quiet");
        let graphite: Settings =
            serde_json::from_str(r#"{"dark_theme":"Graphite Night","light_theme":"Graphite Day"}"#)
                .unwrap();
        assert_eq!(graphite.dark_theme, "Graphite Night");
        assert_eq!(graphite.light_theme, "Graphite Day");
    }
}

#[cfg(test)]
mod period_tests {
    use super::Period;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn periods_shift_by_their_own_length() {
        assert_eq!(Period::Day.shift(d(2026, 12, 31), 1), d(2027, 1, 1));
        assert_eq!(Period::Week.shift(d(2026, 10, 9), -1), d(2026, 10, 2));
        assert_eq!(Period::Month.shift(d(2026, 1, 31), 1), d(2026, 2, 28));
        assert_eq!(Period::Quarter.shift(d(2026, 11, 15), 1), d(2027, 2, 15));
        assert_eq!(Period::Year.shift(d(2028, 2, 29), -1), d(2027, 2, 28));
    }
}
