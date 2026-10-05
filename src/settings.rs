//! Persisted user settings — a small JSON file, no database.

use gpui_kit::component::theme::ThemeMode;
use gpui_kit::{App, WindowAppearance};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    /// Editor font family — a writing-first set, resolved by name.
    pub editor_font_family: String,
    pub editor_font_size: f32,
    pub ui_font_size: f32,
    pub soft_wrap: bool,
    pub show_line_numbers: bool,
    pub tab_size: usize,
    pub view_mode: ViewMode,
    pub sidebar_collapsed: bool,
    /// Obsidian-style: reveal markup around the cursor (live preview).
    /// Kept off — the raw buffer stays honest.
    pub focus_mode: bool,
    /// The folder that was open last — reopened on launch.
    pub last_vault: Option<std::path::PathBuf>,
    /// Vault-relative folder pasted/dropped images are copied into
    /// (Obsidian's "attachment folder path").
    pub attachments_dir: String,
    /// Starred notes — absolute paths, shown pinned at the sidebar top.
    pub starred: Vec<String>,
    /// Which sidebar panes are expanded — persists across launches.
    pub panes: SidebarPanes,
}

fn default_true() -> bool {
    true
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
            tasks: true,
            calendar: true,
            tags: true,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::Dark,
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
            starred: Vec::new(),
            panes: SidebarPanes::default(),
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
