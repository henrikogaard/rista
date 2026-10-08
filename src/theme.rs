//! Rísta theme: two carved palettes — Night (default) and Day.
//!
//! Mineral panes on a dark canvas, a teal accent, and warm paper light.
//! Colors live in the JSON theme configs below; application code reads only
//! semantic tokens from `cx.theme()`.

use gpui_kit::component::theme::{Theme, ThemeConfig, ThemeMode, ThemeSet};
use gpui_kit::*;

pub const NIGHT_THEME_NAME: &str = "Rísta Night";
pub const DAY_THEME_NAME: &str = "Rísta Day";

const RISTA_THEMES: &str = r##"{
  "name": "Rísta",
  "author": "Rísta",
  "themes": [
    {
      "name": "Rísta Night",
      "mode": "dark",
      "radius": 8,
      "radius.lg": 12,
      "colors": {
        "background": "#0a0b0e",
        "foreground": "#d8dce5",
        "border": "#21252f",
        "ring": "#6fd8c8",
        "caret": "#7ee0d0",
        "selection": "#1d4a44",
        "accent.background": "#29443f",
        "accent.foreground": "#b8c0ce",
        "title_bar.background": "#0a0b0e",
        "title_bar.border": "#0a0b0e",
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
      "radius.lg": 12,
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
        "title_bar.border": "#f6f4ef",
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
        "muted.foreground": "#727887",
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

const RISTA_PALETTES: &str = r##"[
  {"name":"Fjord Night","mode":"dark","canvas":"#0d121b","surface":"#141c29","popover":"#1a2535","muted":"#202d40","border":"#2c3b50","accent":"#92bcf4","selection":"#293f60","on_accent":"#101d30"},
  {"name":"Fjord Day","mode":"light","canvas":"#edf3fa","surface":"#f9fbfe","popover":"#ffffff","muted":"#e0eaf7","border":"#ccd9eb","accent":"#315f9d","selection":"#cdddf4","on_accent":"#ffffff"},
  {"name":"Rose Night","mode":"dark","canvas":"#181216","surface":"#211a20","popover":"#2b212a","muted":"#342630","border":"#493440","accent":"#e5a1b5","selection":"#50303d","on_accent":"#2d1520"},
  {"name":"Rose Day","mode":"light","canvas":"#faf1f3","surface":"#fffafb","popover":"#ffffff","muted":"#f1e2e7","border":"#e4cdd5","accent":"#994963","selection":"#efd1dc","on_accent":"#ffffff"},
  {"name":"Graphite Night","mode":"dark","canvas":"#161618","surface":"#1e1e20","popover":"#2a2a2c","muted":"#2c2c2e","border":"#38383a","accent":"#f5f5f7","selection":"#3a3a3c","on_accent":"#1d1d1f","colors":{"foreground":"#e5e5ea","popover.foreground":"#e5e5ea","sidebar.foreground":"#d1d1d6","tab.active.foreground":"#f5f5f7","group_box.foreground":"#d1d1d6","secondary.foreground":"#d1d1d6","accent.foreground":"#d1d1d6","muted.foreground":"#8e8e93","input.background":"#1a1a1c","list.even.background":"#1b1b1d","primary.hover.background":"#ffffff","primary.active.background":"#d1d1d6","scrollbar.thumb.background":"#48484aaa","danger.background":"#ff453a","danger.foreground":"#ffffff","danger.hover.background":"#ff6961","warning.background":"#ffd60a","warning.foreground":"#1d1d1f","success.background":"#30d158","success.foreground":"#1d1d1f","base.red":"#ff453a","base.yellow":"#ffd60a","base.blue":"#0a84ff","base.green":"#30d158","base.magenta":"#bf5af2","base.cyan":"#64d2ff","base.orange":"#ff9f0a"},"highlight":{"editor.foreground":"#e5e5ea","editor.active_line.background":"#2c2c2e80","editor.line_number":"#48484a","editor.active_line_number":"#8e8e93","editor.invisible":"#3a3a3c66","warning":"#ffd60a","error":"#ff453a","success":"#30d158"},"syntax":{"comment":"#6c6c70","string":"#c7c7cc","number":"#d1d1d6","function":"#f5f5f7","type":"#d1d1d6","operator":"#aeaeb2","property":"#e5e5ea","constant":"#d1d1d6","punctuation":"#8e8e93","link_text":"#f5f5f7","link_uri":"#8e8e93","emphasis":"#e5e5ea","emphasis.strong":"#ffffff","title":"#ffffff","hint":"#8e8e93","predictive":"#48484a"}},
  {"name":"Graphite Day","mode":"light","canvas":"#f5f5f7","surface":"#ffffff","popover":"#ffffff","muted":"#ededf0","border":"#d2d2d7","accent":"#1d1d1f","selection":"#dcdce0","on_accent":"#ffffff","colors":{"foreground":"#1d1d1f","popover.foreground":"#1d1d1f","sidebar.foreground":"#3a3a3c","tab.active.foreground":"#1d1d1f","group_box.foreground":"#3a3a3c","secondary.foreground":"#3a3a3c","accent.foreground":"#3a3a3c","muted.foreground":"#6e6e73","input.background":"#ffffff","list.even.background":"#fafafa","primary.hover.background":"#3a3a3c","primary.active.background":"#000000","scrollbar.thumb.background":"#c7c7ccaa","danger.background":"#d70015","danger.foreground":"#ffffff","danger.hover.background":"#e5332a","warning.background":"#b25000","warning.foreground":"#ffffff","success.background":"#248a3d","success.foreground":"#ffffff","base.red":"#d70015","base.yellow":"#9a6700","base.blue":"#0066cc","base.green":"#248a3d","base.magenta":"#8944ab","base.cyan":"#0071a4","base.orange":"#c93400"},"highlight":{"editor.foreground":"#1d1d1f","editor.active_line.background":"#f5f5f7","editor.line_number":"#c7c7cc","editor.active_line_number":"#6e6e73","editor.invisible":"#d1d1d666","warning":"#b25000","error":"#d70015","success":"#248a3d"},"syntax":{"comment":"#8e8e93","string":"#3a3a3c","number":"#48484a","function":"#1d1d1f","type":"#48484a","operator":"#6e6e73","property":"#1d1d1f","constant":"#48484a","punctuation":"#8e8e93","link_text":"#1d1d1f","link_uri":"#8e8e93","emphasis":"#3a3a3c","emphasis.strong":"#000000","title":"#000000","hint":"#8e8e93","predictive":"#c7c7cc"}},
  {"name":"Mono Night","mode":"dark","canvas":"#000000","surface":"#0a0a0a","popover":"#1c1c1e","muted":"#1c1c1e","border":"#2c2c2e","accent":"#ffffff","selection":"#333336","on_accent":"#000000","colors":{"foreground":"#f2f2f7","popover.foreground":"#f2f2f7","sidebar.foreground":"#d1d1d6","tab.active.foreground":"#ffffff","group_box.foreground":"#d1d1d6","secondary.foreground":"#d1d1d6","accent.foreground":"#d1d1d6","muted.foreground":"#8e8e93","input.background":"#000000","list.even.background":"#0d0d0d","primary.hover.background":"#e5e5ea","primary.active.background":"#d1d1d6","scrollbar.thumb.background":"#3a3a3caa","danger.background":"#ff453a","danger.foreground":"#ffffff","danger.hover.background":"#ff6961","warning.background":"#ffd60a","warning.foreground":"#000000","success.background":"#30d158","success.foreground":"#000000","base.red":"#ff453a","base.yellow":"#ffd60a","base.blue":"#0a84ff","base.green":"#30d158","base.magenta":"#bf5af2","base.cyan":"#64d2ff","base.orange":"#ff9f0a"},"highlight":{"editor.foreground":"#f2f2f7","editor.active_line.background":"#1c1c1e80","editor.line_number":"#3a3a3c","editor.active_line_number":"#8e8e93","editor.invisible":"#2c2c2e66","warning":"#ffd60a","error":"#ff453a","success":"#30d158"},"syntax":{"comment":"#636366","string":"#c7c7cc","number":"#d1d1d6","function":"#ffffff","type":"#d1d1d6","operator":"#aeaeb2","property":"#f2f2f7","constant":"#d1d1d6","punctuation":"#8e8e93","link_text":"#ffffff","link_uri":"#8e8e93","emphasis":"#f2f2f7","emphasis.strong":"#ffffff","title":"#ffffff","hint":"#8e8e93","predictive":"#3a3a3c"}},
  {"name":"Mono Day","mode":"light","canvas":"#fafafa","surface":"#ffffff","popover":"#ffffff","muted":"#f2f2f2","border":"#e0e0e0","accent":"#000000","selection":"#e3e3e3","on_accent":"#ffffff","colors":{"foreground":"#111111","popover.foreground":"#111111","sidebar.foreground":"#333333","tab.active.foreground":"#000000","group_box.foreground":"#333333","secondary.foreground":"#333333","accent.foreground":"#333333","muted.foreground":"#6e6e73","input.background":"#ffffff","list.even.background":"#fcfcfc","primary.hover.background":"#333333","primary.active.background":"#1d1d1f","scrollbar.thumb.background":"#c7c7ccaa","danger.background":"#d70015","danger.foreground":"#ffffff","danger.hover.background":"#e5332a","warning.background":"#b25000","warning.foreground":"#ffffff","success.background":"#248a3d","success.foreground":"#ffffff","base.red":"#d70015","base.yellow":"#9a6700","base.blue":"#0066cc","base.green":"#248a3d","base.magenta":"#8944ab","base.cyan":"#0071a4","base.orange":"#c93400"},"highlight":{"editor.foreground":"#111111","editor.active_line.background":"#f7f7f7","editor.line_number":"#c7c7cc","editor.active_line_number":"#6e6e73","editor.invisible":"#d1d1d666","warning":"#b25000","error":"#d70015","success":"#248a3d"},"syntax":{"comment":"#8e8e93","string":"#333333","number":"#48484a","function":"#111111","type":"#48484a","operator":"#6e6e73","property":"#111111","constant":"#48484a","punctuation":"#8e8e93","link_text":"#000000","link_uri":"#8e8e93","emphasis":"#333333","emphasis.strong":"#000000","title":"#000000","hint":"#8e8e93","predictive":"#c7c7cc"}}
]"##;

pub fn builtin_json() -> serde_json::Value {
    let mut set: serde_json::Value = serde_json::from_str(RISTA_THEMES).unwrap();
    let originals = set["themes"].as_array().unwrap().clone();
    let palettes: Vec<serde_json::Value> = serde_json::from_str(RISTA_PALETTES).unwrap();
    for palette in palettes {
        let mut theme = originals
            .iter()
            .find(|t| t["mode"] == palette["mode"])
            .unwrap()
            .clone();
        theme["name"] = palette["name"].clone();
        for (key, value) in [
            ("background", "canvas"),
            ("title_bar.background", "canvas"),
            ("title_bar.border", "canvas"),
            ("status_bar.background", "canvas"),
            ("status_bar.border", "canvas"),
            ("sidebar.background", "surface"),
            ("tab_bar.background", "surface"),
            ("group_box.background", "surface"),
            ("list.head.background", "surface"),
            ("list.even.background", "surface"),
            ("popover.background", "popover"),
            ("input.background", "surface"),
            ("border", "border"),
            ("sidebar.border", "border"),
            ("input.border", "border"),
            ("popover.border", "border"),
            ("muted.background", "muted"),
            ("secondary.background", "muted"),
            ("secondary.hover.background", "border"),
            ("secondary.active.background", "selection"),
            ("tab.active.background", "muted"),
            ("list.hover.background", "muted"),
            ("accent.background", "muted"),
            ("selection", "selection"),
            ("list.active.background", "selection"),
            ("list.active.border", "accent"),
            ("ring", "accent"),
            ("caret", "accent"),
            ("primary.background", "accent"),
            ("primary.hover.background", "accent"),
            ("primary.active.background", "accent"),
            ("primary.foreground", "on_accent"),
            ("info.background", "accent"),
            ("info.foreground", "on_accent"),
            ("base.blue", "accent"),
        ] {
            theme["colors"][key] = palette[value].clone();
        }
        theme["highlight"]["editor.background"] = palette["surface"].clone();
        theme["highlight"]["editor.active_line.background"] = palette["muted"].clone();
        theme["highlight"]["editor.active_line_number"] = palette["accent"].clone();
        theme["highlight"]["info"] = palette["accent"].clone();
        for key in ["keyword", "property", "link_text", "title"] {
            theme["highlight"]["syntax"][key]["color"] = palette["accent"].clone();
        }
        if let Some(overrides) = palette["colors"].as_object() {
            for (key, value) in overrides {
                theme["colors"][key.as_str()] = value.clone();
            }
        }
        if let Some(overrides) = palette["highlight"].as_object() {
            for (key, value) in overrides {
                theme["highlight"][key.as_str()] = value.clone();
            }
        }
        if let Some(overrides) = palette["syntax"].as_object() {
            for (key, value) in overrides {
                theme["highlight"]["syntax"][key.as_str()]["color"] = value.clone();
            }
        }
        set["themes"].as_array_mut().unwrap().push(theme);
    }
    set
}

pub struct ThemeCatalog {
    pub themes: Vec<std::rc::Rc<ThemeConfig>>,
    pub errors: Vec<String>,
}
impl Global for ThemeCatalog {}

pub fn themes_dir() -> std::path::PathBuf {
    crate::settings::config_dir().join("themes")
}

pub fn install_themes(cx: &mut App) {
    let set: ThemeSet = serde_json::from_value(builtin_json()).expect("bundled themes must parse");
    let mut themes: Vec<_> = set.themes.into_iter().map(std::rc::Rc::new).collect();
    let mut errors = Vec::new();
    if let Ok(entries) = std::fs::read_dir(themes_dir()) {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let loaded = std::fs::read_to_string(&path)
                .map_err(anyhow::Error::from)
                .and_then(|raw| parse_custom(&raw));
            match loaded {
                Ok(set)
                    if set
                        .themes
                        .iter()
                        .all(|candidate| !themes.iter().any(|t| t.name == candidate.name)) =>
                {
                    themes.extend(set.themes.into_iter().map(std::rc::Rc::new));
                }
                _ => errors.push(
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                ),
            }
        }
    }
    cx.set_global(ThemeCatalog { themes, errors });
}

fn parse_custom(raw: &str) -> anyhow::Result<ThemeSet> {
    let json: serde_json::Value = serde_json::from_str(raw)?;
    for theme in json["themes"].as_array().into_iter().flatten() {
        for value in theme["colors"]
            .as_object()
            .into_iter()
            .flat_map(|colors| colors.values())
        {
            if let Some(color) = value.as_str() {
                anyhow::ensure!(valid_hex(color), "Invalid colour");
            }
        }
    }
    let set: ThemeSet = serde_json::from_str(raw)?;
    anyhow::ensure!(!set.themes.is_empty(), "No themes");
    let mut names = std::collections::HashSet::new();
    for theme in &set.themes {
        anyhow::ensure!(
            !theme.name.trim().is_empty() && names.insert(theme.name.clone()),
            "Invalid name"
        );
    }
    Ok(set)
}

fn valid_hex(color: &str) -> bool {
    color
        .strip_prefix('#')
        .is_some_and(|hex| matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn theme_names(mode: ThemeMode, cx: &App) -> Vec<String> {
    cx.global::<ThemeCatalog>()
        .themes
        .iter()
        .filter(|t| t.mode == mode)
        .map(|t| t.name.to_string())
        .collect()
}

pub fn apply(settings: &crate::settings::Settings, cx: &mut App) {
    let catalog = cx.global::<ThemeCatalog>();
    let select = |name: &str, mode| {
        catalog
            .themes
            .iter()
            .find(|t| t.name.as_ref() == name && t.mode == mode)
            .or_else(|| catalog.themes.iter().find(|t| t.mode == mode))
            .unwrap()
            .clone()
    };
    let dark = select(&settings.dark_theme, ThemeMode::Dark);
    let light = select(&settings.light_theme, ThemeMode::Light);
    Theme::update(cx, |theme| {
        theme.dark_theme = dark;
        theme.light_theme = light;
    });
    set_theme_mode(settings.theme_mode(cx), cx);
}

pub fn create_custom(
    settings: &crate::settings::Settings,
    cx: &mut App,
) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write;
    let mode = settings.theme_mode(cx);
    let name = if mode == ThemeMode::Dark {
        &settings.dark_theme
    } else {
        &settings.light_theme
    };
    let catalog = cx.global::<ThemeCatalog>();
    let selected = catalog
        .themes
        .iter()
        .find(|t| t.name.as_ref() == name && t.mode == mode)
        .or_else(|| catalog.themes.iter().find(|t| t.mode == mode))
        .unwrap();
    let mut theme = selected.as_ref().clone();
    std::fs::create_dir_all(themes_dir())?;
    for index in 1..10000 {
        let path = themes_dir().join(format!("custom-{index}.json"));
        let name = format!("Custom {index}");
        if catalog.themes.iter().any(|t| t.name.as_ref() == name) {
            continue;
        }
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        };
        theme.name = name.into();
        theme.is_default = false;
        let set = ThemeSet {
            name: "Custom".into(),
            themes: vec![theme],
            ..Default::default()
        };
        file.write_all(serde_json::to_string_pretty(&set)?.as_bytes())?;
        return Ok(path);
    }
    anyhow::bail!("No available theme filename")
}

/// Apply a mode from settings. `system` follows macOS appearance.
pub fn set_theme_mode(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use super::{builtin_json, parse_custom};

    fn exported_theme(theme: &serde_json::Value) -> serde_json::Value {
        let mut colors = theme["colors"].as_object().unwrap().clone();
        for (key, value) in theme["highlight"].as_object().unwrap() {
            if key != "syntax" {
                colors.insert(format!("highlight.{key}"), value.clone());
            }
        }
        for (key, value) in theme["highlight"]["syntax"].as_object().unwrap() {
            colors.insert(format!("syntax.{key}"), value["color"].clone());
        }
        serde_json::json!({"name": theme["name"], "colors": colors})
    }

    #[test]
    fn builtin_palettes_have_distinct_accents_and_exportable_colours() {
        let json = builtin_json();
        let themes = json["themes"].as_array().unwrap();
        assert_eq!(themes.len(), 10);
        for mode in ["dark", "light"] {
            let matching: Vec<_> = themes.iter().filter(|t| t["mode"] == mode).collect();
            let accents: std::collections::HashSet<_> = matching
                .iter()
                .map(|t| t["colors"]["primary.background"].as_str().unwrap())
                .collect();
            assert_eq!(accents.len(), 5);
        }
        assert_eq!(parse_custom(&json.to_string()).unwrap().themes.len(), 10);

        let graphite_night = themes
            .iter()
            .find(|theme| theme["name"] == "Graphite Night")
            .unwrap();
        assert_eq!(graphite_night["colors"]["base.blue"], "#0a84ff");
        assert_eq!(
            graphite_night["highlight"]["syntax"]["title"]["color"],
            "#ffffff"
        );

        let fjord_night = themes
            .iter()
            .find(|theme| theme["name"] == "Fjord Night")
            .unwrap();
        assert_eq!(fjord_night["colors"]["base.red"], "#c47a7a");
        assert_eq!(
            fjord_night["highlight"]["syntax"]["string"]["color"],
            "#9db883"
        );

        let exports: serde_json::Value =
            serde_json::from_str(include_str!("../design/tokens.json")).unwrap();
        for theme in &themes[..2] {
            let mode = theme["mode"].as_str().unwrap();
            assert_eq!(exports["rista"]["themes"][mode], exported_theme(theme));
        }
        for theme in themes {
            let name = theme["name"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .replace('í', "i")
                .replace(' ', "-");
            let mut expected = exported_theme(theme);
            expected["mode"] = theme["mode"].clone();
            assert_eq!(exports["rista"]["palettes"][name], expected);
        }

        let css = include_str!("../design/tokens.css");
        for selector in [
            "dark",
            "light",
            "rista-night",
            "rista-day",
            "fjord-night",
            "fjord-day",
            "rose-night",
            "rose-day",
            "graphite-night",
            "graphite-day",
            "mono-night",
            "mono-day",
        ] {
            assert!(css.contains(&format!(r#"[data-rista-theme="{selector}"]"#)));
        }
    }

    #[test]
    fn custom_themes_reject_invalid_colours_and_duplicate_names() {
        assert!(
            parse_custom(r##"{"themes":[{"name":"Bad","colors":{"background":"nope"}}]}"##)
                .is_err()
        );
        assert!(parse_custom(r#"{"themes":[{"name":"Same"},{"name":"Same"}]}"#).is_err());
        assert!(parse_custom(r#"{"themes":[]}"#).is_err());
        assert!(parse_custom(r##"{"themes":[{"name":"Mine","mode":"dark","colors":{"primary.background":"#88aacc"}}]}"##).is_ok());
    }
}
