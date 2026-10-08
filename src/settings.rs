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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub show_other_files: bool,
    pub language: Language,
    pub inspector_open: bool,
    pub active_folder: Option<PathBuf>,
    pub appearance: Appearance,
    pub dark_theme: String,
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
            show_other_files: false,
            language: Language::default(),
            inspector_open: false,
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
}
