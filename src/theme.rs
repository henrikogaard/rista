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
      "colors": {
        "background": "#1a1d24",
        "foreground": "#d8dce5",
        "border": "#2c313d",
        "ring": "#88b3d4",
        "caret": "#9fc2dd",
        "selection": "#2d4c68",
        "accent.background": "#232834",
        "accent.foreground": "#b8c0ce",
        "title_bar.background": "#161920",
        "title_bar.border": "#262b36",
        "tab.background": "#1a1d2400",
        "tab.active.background": "#22262f",
        "tab.active.foreground": "#e4e8f0",
        "tab_bar.background": "#161920",
        "panel.background": "#1e212a",
        "popover.background": "#232833",
        "popover.foreground": "#d8dce5",
        "popover.border": "#343a48",
        "sidebar.background": "#1e212a",
        "sidebar.foreground": "#c4c9d4",
        "sidebar.border": "#2c313d",
        "status_bar.background": "#161920",
        "status_bar.border": "#2c313d",
        "group_box.background": "#20242e",
        "group_box.foreground": "#c9cedb",
        "input.border": "#343b4a",
        "input.background": "#171a20",
        "list.head.background": "#1e212a",
        "list.even.background": "#1b1e26",
        "list.active.background": "#2d4c6866",
        "list.active.border": "#88b3d4",
        "list.hover.background": "#252a35",
        "muted.background": "#22262f",
        "muted.foreground": "#82899a",
        "primary.background": "#88b3d4",
        "primary.foreground": "#141820",
        "primary.hover.background": "#9fc2dd",
        "primary.active.background": "#78a6c8",
        "secondary.background": "#2a2f3b",
        "secondary.foreground": "#c4c9d4",
        "secondary.hover.background": "#333947",
        "secondary.active.background": "#3d4453",
        "danger.background": "#b26a6a",
        "danger.foreground": "#fbf4f4",
        "danger.hover.background": "#c07a7a",
        "warning.background": "#c9a26b",
        "warning.foreground": "#231d14",
        "success.background": "#9db883",
        "success.foreground": "#161a12",
        "info.background": "#88b3d4",
        "info.foreground": "#141820",
        "scrollbar.background": "#00000000",
        "scrollbar.thumb.background": "#3a4150aa",
        "base.red": "#c47a7a",
        "base.yellow": "#d3b380",
        "base.blue": "#88b3d4",
        "base.green": "#9db883",
        "base.magenta": "#b493c9",
        "base.cyan": "#8fc2bb",
        "base.orange": "#cf9578"
      },
      "highlight": {
        "editor.foreground": "#d8dce5",
        "editor.background": "#1a1d24",
        "editor.active_line.background": "#22262f80",
        "editor.line_number": "#4a5162",
        "editor.active_line_number": "#8a92a5",
        "editor.invisible": "#3a415066",
        "info": "#88b3d4",
        "warning": "#d3b380",
        "error": "#c47a7a",
        "success": "#9db883",
                "syntax": {
                "comment": {"color": "#6b7287"},
                "keyword": {"color": "#88b3d4"},
                "string": {"color": "#9db883"},
                "number": {"color": "#d3b380"},
                "function": {"color": "#b493c9"},
                "type": {"color": "#8fc2bb"},
                "operator": {"color": "#a8aebe"},
                "property": {"color": "#9fc2dd"},
                "constant": {"color": "#cf9578"},
                "punctuation": {"color": "#7a8194"},
                "link_text": {"color": "#9fc2dd"},
                "link_uri": {"color": "#6d7688"},
                "emphasis": {"color": "#d3b380"},
                "emphasis.strong": {"color": "#e0d5b8"},
                "title": {"color": "#9fc2dd"},
                "hint": {"color": "#8a92a5"},
                "predictive": {"color": "#4a5162"}
                }
        
      }
    },
    {
      "name": "Rísta Day",
      "mode": "light",
      "colors": {
        "background": "#f6f4ef",
        "foreground": "#363b47",
        "border": "#ddd9cf",
        "ring": "#5b7fa3",
        "caret": "#4a6b8f",
        "selection": "#cfdcec",
        "accent.background": "#e9e6dd",
        "accent.foreground": "#454b58",
        "title_bar.background": "#efede5",
        "title_bar.border": "#dedbd0",
        "tab.background": "#f6f4ef00",
        "tab.active.background": "#ffffff",
        "tab.active.foreground": "#2e3340",
        "tab_bar.background": "#efede5",
        "panel.background": "#f0eee7",
        "popover.background": "#ffffff",
        "popover.foreground": "#363b47",
        "popover.border": "#d8d4c8",
        "sidebar.background": "#f0eee7",
        "sidebar.foreground": "#454b58",
        "sidebar.border": "#ddd9cf",
        "status_bar.background": "#efede5",
        "status_bar.border": "#ddd9cf",
        "group_box.background": "#ebe9e1",
        "group_box.foreground": "#454b58",
        "input.border": "#d3cfc2",
        "input.background": "#ffffff",
        "list.head.background": "#f0eee7",
        "list.even.background": "#efede5",
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
        "editor.background": "#f6f4ef",
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
