//! Rísta theme: two carved palettes — Night (default) and Day.
//!
//! Nordic restraint: deep mineral slate, frost-blue accent, warm paper light.
//! Colors live in the JSON theme configs below; application code reads only
//! semantic tokens from `cx.theme()`.

use gpui_kit::component::theme::{Theme, ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::*;

#[allow(dead_code)]
pub const NIGHT_THEME_NAME: &str = "Rísta Night";
#[allow(dead_code)]
pub const DAY_THEME_NAME: &str = "Rísta Day";

const RISTA_THEMES: &str = r##"{
  "name": "Rísta",
  "author": "Rísta",
  "themes": [
    {
      "name": "Rísta Night",
      "mode": "dark",
      "radius": 8,
      "radius.lg": 10,
      "colors": {
        "background": "#0a0b0e",
        "foreground": "#d8dce5",
        "border": "#21252f",
        "ring": "#6fd8c8",
        "caret": "#7ee0d0",
        "selection": "#1d4a44",
        "accent.background": "#181b22",
        "accent.foreground": "#b8c0ce",
        "title_bar.background": "#0a0b0e",
        "title_bar.border": "#14161c",
        "tab.background": "#12141900",
        "tab.active.background": "#1d212b",
        "tab.active.foreground": "#e4e8f0",
        "tab_bar.background": "#121419",
        "popover.background": "#171a21",
        "popover.foreground": "#d8dce5",
        "popover.border": "#262c38",
        "sidebar.background": "#121419",
        "sidebar.foreground": "#c4c9d4",
        "sidebar.border": "#21252f",
        "status_bar.background": "#0a0b0e",
        "status_bar.border": "#0a0b0e",
        "group_box.background": "#121419",
        "group_box.foreground": "#c9cedb",
        "input.border": "#262c38",
        "input.background": "#0d0f13",
        "list.head.background": "#121419",
        "list.even.background": "#0f1217",
        "list.active.background": "#1d4a4466",
        "list.active.border": "#6fd8c8",
        "list.hover.background": "#191d26",
        "muted.background": "#161a21",
        "muted.foreground": "#82899a",
        "primary.background": "#6fd8c8",
        "primary.foreground": "#0c1916",
        "primary.hover.background": "#8ae0d2",
        "primary.active.background": "#5cc7b7",
        "secondary.background": "#1c2029",
        "secondary.foreground": "#c4c9d4",
        "secondary.hover.background": "#262c38",
        "secondary.active.background": "#303746",
        "danger.background": "#b26a6a",
        "danger.foreground": "#fbf4f4",
        "danger.hover.background": "#c07a7a",
        "warning.background": "#c9a26b",
        "warning.foreground": "#231d14",
        "success.background": "#9db883",
        "success.foreground": "#161a12",
        "info.background": "#6fd8c8",
        "info.foreground": "#0c1916",
        "scrollbar.background": "#00000000",
        "scrollbar.thumb.background": "#2e3442aa",
        "base.red": "#c47a7a",
        "base.yellow": "#d3b380",
        "base.blue": "#7fd8c8",
        "base.green": "#9db883",
        "base.magenta": "#b493c9",
        "base.cyan": "#92cfc2",
        "base.orange": "#cf9578"
      },
      "highlight": {
        "editor.foreground": "#d8dce5",
        "editor.background": "#121419",
        "editor.active_line.background": "#1a1e2780",
        "editor.line_number": "#3d4454",
        "editor.active_line_number": "#7ee0d0",
        "editor.invisible": "#2e344266",
        "info": "#6fd8c8",
        "warning": "#d3b380",
        "error": "#c47a7a",
        "success": "#9db883",
                "syntax": {
                "comment": {"color": "#6b7287"},
                "keyword": {"color": "#7fd8c8"},
                "string": {"color": "#9db883"},
                "number": {"color": "#d3b380"},
                "function": {"color": "#b493c9"},
                "type": {"color": "#92cfc2"},
                "operator": {"color": "#a8aebe"},
                "property": {"color": "#8ae0d2"},
                "constant": {"color": "#cf9578"},
                "punctuation": {"color": "#7a8194"},
                "link_text": {"color": "#8ae0d2"},
                "link_uri": {"color": "#6d7688"},
                "emphasis": {"color": "#d3b380"},
                "emphasis.strong": {"color": "#e0d5b8"},
                "title": {"color": "#8ae0d2"},
                "hint": {"color": "#7a8294"},
                "predictive": {"color": "#4a5162"}
                }
        
      }
    },
    {
      "name": "Rísta Day",
      "mode": "light",
      "radius": 8,
      "radius.lg": 10,
      "colors": {
        "background": "#f6f4ef",
        "foreground": "#363b47",
        "border": "#ddd9cf",
        "ring": "#5b7fa3",
        "caret": "#4a6b8f",
        "selection": "#cfdcec",
        "accent.background": "#e9e6dd",
        "accent.foreground": "#454b58",
        "title_bar.background": "#f6f4ef",
        "title_bar.border": "#e6e3d8",
        "tab.background": "#f6f4ef00",
        "tab.active.background": "#ffffff",
        "tab.active.foreground": "#2e3340",
        "tab_bar.background": "#ffffff",
        "popover.background": "#ffffff",
        "popover.foreground": "#363b47",
        "popover.border": "#d8d4c8",
        "sidebar.background": "#ffffff",
        "sidebar.foreground": "#454b58",
        "sidebar.border": "#ddd9cf",
        "status_bar.background": "#f6f4ef",
        "status_bar.border": "#f6f4ef",
        "group_box.background": "#ffffff",
        "group_box.foreground": "#454b58",
        "input.border": "#d3cfc2",
        "input.background": "#ffffff",
        "list.head.background": "#ffffff",
        "list.even.background": "#faf8f3",
        "list.active.background": "#cfdcec",
        "list.active.border": "#5b7fa3",
        "list.hover.background": "#e8e5dc",
        "muted.background": "#e9e6dd",
        "muted.foreground": "#8a8fa0",
        "primary.background": "#5b7fa3",
        "primary.foreground": "#f6f4ef",
        "primary.hover.background": "#6b8db0",
        "primary.active.background": "#4f7192",
        "secondary.background": "#e2dfd5",
        "secondary.foreground": "#454b58",
        "secondary.hover.background": "#d8d5ca",
        "secondary.active.background": "#cdcabc",
        "danger.background": "#a85f5f",
        "danger.foreground": "#fbf4f4",
        "danger.hover.background": "#b56e6e",
        "warning.background": "#b48c4f",
        "warning.foreground": "#f8f3e8",
        "success.background": "#7a9a62",
        "success.foreground": "#f4f8ec",
        "info.background": "#5b7fa3",
        "info.foreground": "#f6f4ef",
        "scrollbar.background": "#00000000",
        "scrollbar.thumb.background": "#b8b4a6aa",
        "base.red": "#a85f5f",
        "base.yellow": "#a8853f",
        "base.blue": "#5b7fa3",
        "base.green": "#6d8f55",
        "base.magenta": "#8a6aa8",
        "base.cyan": "#5a8f87",
        "base.orange": "#b06a48"
      },
      "highlight": {
        "editor.foreground": "#363b47",
        "editor.background": "#ffffff",
        "editor.active_line.background": "#ebe9e180",
        "editor.line_number": "#b8b4a6",
        "editor.active_line_number": "#6d7280",
        "editor.invisible": "#c9c5b666",
        "info": "#5b7fa3",
        "warning": "#a8853f",
        "error": "#a85f5f",
        "success": "#6d8f55",
                "syntax": {
                "comment": {"color": "#9a9eac"},
                "keyword": {"color": "#5b7fa3"},
                "string": {"color": "#6d8f55"},
                "number": {"color": "#a8853f"},
                "function": {"color": "#8a6aa8"},
                "type": {"color": "#5a8f87"},
                "operator": {"color": "#6d7280"},
                "property": {"color": "#4a6b8f"},
                "constant": {"color": "#b06a48"},
                "punctuation": {"color": "#8a8fa0"},
                "link_text": {"color": "#4a6b8f"},
                "link_uri": {"color": "#9a9eac"},
                "emphasis": {"color": "#a8853f"},
                "emphasis.strong": {"color": "#7a6a3a"},
                "title": {"color": "#4a6b8f"},
                "hint": {"color": "#9a9eac"},
                "predictive": {"color": "#b8b4a6"}
                }
        
      }
    }
  ]
}"##;

/// Register Rísta's palettes as the app's light and dark defaults.
///
/// Call after `gpui_kit::init`, then use [`set_theme_mode`] to apply.
pub fn install_themes(cx: &mut App) {
    let set: gpui_kit::component::theme::ThemeSet =
        serde_json::from_str(RISTA_THEMES).expect("rista theme JSON must parse");

    let mut night: Option<std::rc::Rc<ThemeConfig>> = None;
    let mut day: Option<std::rc::Rc<ThemeConfig>> = None;
    for theme in set.themes {
        match theme.mode {
            ThemeMode::Dark => night = Some(std::rc::Rc::new(theme)),
            _ => day = Some(std::rc::Rc::new(theme)),
        }
    }

    Theme::update(cx, |theme| {
        if let Some(day) = day {
            theme.light_theme = day;
        }
        if let Some(night) = night {
            theme.dark_theme = night;
        }
    });

    // Keep the registry in step so theme pickers list Rísta's pair.
    let registry = ThemeRegistry::global_mut(cx);
    let _ = registry.load_themes_from_str(RISTA_THEMES);
}

/// Apply a mode from settings. `system` follows macOS appearance.
pub fn set_theme_mode(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
}
