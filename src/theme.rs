//! Rísta theme palettes and shared theme helpers.
//!
//! Mineral panes on a dark canvas, a teal accent, and warm paper light.
//! Colors live in the JSON theme configs below; application code reads only
//! semantic tokens from `cx.theme()`.

use gpui_kit::component::theme::{Theme, ThemeConfig, ThemeMode, ThemeSet};
use gpui_kit::*;

pub const NIGHT_THEME_NAME: &str = "Graphite Night";
pub const DAY_THEME_NAME: &str = "Graphite Day";

const RISTA_THEMES: &str = r##"{
  "name": "Slate",
  "author": "Rísta",
  "themes": [
    {
      "name": "Slate Night",
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
      "name": "Slate Day",
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
  {"name":"Gruvbox Night","mode":"dark","canvas":"#1d2021","surface":"#282828","popover":"#3c3836","muted":"#3c3836","border":"#504945","accent":"#fabd2f","selection":"#504945","on_accent":"#282828","foreground":"#ebdbb2","secondary":"#d5c4a1","muted_foreground":"#a89984","red":"#fb4934","green":"#b8bb26","yellow":"#fabd2f","blue":"#83a598","magenta":"#d3869b","cyan":"#8ec07c","orange":"#fe8019"},
  {"name":"Gruvbox Day","mode":"light","canvas":"#f2e5bc","surface":"#fbf1c7","popover":"#f9f5d7","muted":"#ebdbb2","border":"#d5c4a1","accent":"#9d6a00","selection":"#d5c4a1","on_accent":"#ffffff","foreground":"#3c3836","secondary":"#504945","muted_foreground":"#665c54","red":"#9d0006","green":"#79740e","yellow":"#b57614","blue":"#076678","magenta":"#8f3f71","cyan":"#427b58","orange":"#af3a03"},
  {"name":"Nord Night","mode":"dark","canvas":"#242933","surface":"#2e3440","popover":"#3b4252","muted":"#3b4252","border":"#434c5e","accent":"#88c0d0","selection":"#434c5e","on_accent":"#2e3440","foreground":"#eceff4","secondary":"#e5e9f0","muted_foreground":"#a5b1c2","red":"#bf616a","green":"#a3be8c","yellow":"#ebcb8b","blue":"#81a1c1","magenta":"#b48ead","cyan":"#8fbcbb","orange":"#d08770"},
  {"name":"Nord Day","mode":"light","canvas":"#e5e9f0","surface":"#eceff4","popover":"#ffffff","muted":"#d8dee9","border":"#bbc5d5","accent":"#466b94","selection":"#cbd5e3","on_accent":"#ffffff","foreground":"#2e3440","secondary":"#3b4252","muted_foreground":"#59677c","red":"#a33b48","green":"#4f6b35","yellow":"#8c651c","blue":"#466b94","magenta":"#825f85","cyan":"#327273","orange":"#a65638"},
  {"name":"Solarized Night","mode":"dark","canvas":"#00212b","surface":"#002b36","popover":"#073642","muted":"#073642","border":"#27535e","accent":"#2aa198","selection":"#164956","on_accent":"#002b36","foreground":"#93a1a1","secondary":"#93a1a1","muted_foreground":"#839496","red":"#dc322f","green":"#859900","yellow":"#b58900","blue":"#268bd2","magenta":"#d33682","cyan":"#2aa198","orange":"#cb4b16"},
  {"name":"Solarized Day","mode":"light","canvas":"#eee8d5","surface":"#fdf6e3","popover":"#fffaf0","muted":"#eee8d5","border":"#cfc8b7","accent":"#007d75","selection":"#d8e1d8","on_accent":"#ffffff","foreground":"#586e75","secondary":"#586e75","muted_foreground":"#657b83","red":"#c32a28","green":"#667b00","yellow":"#946b00","blue":"#006eaf","magenta":"#b52b70","cyan":"#007d75","orange":"#af4011"},
  {"name":"Catppuccin Night","mode":"dark","canvas":"#11111b","surface":"#1e1e2e","popover":"#313244","muted":"#313244","border":"#45475a","accent":"#cba6f7","selection":"#45475a","on_accent":"#1e1e2e","foreground":"#cdd6f4","secondary":"#bac2de","muted_foreground":"#a6adc8","red":"#f38ba8","green":"#a6e3a1","yellow":"#f9e2af","blue":"#89b4fa","magenta":"#f5c2e7","cyan":"#94e2d5","orange":"#fab387"},
  {"name":"Catppuccin Day","mode":"light","canvas":"#dce0e8","surface":"#eff1f5","popover":"#ffffff","muted":"#e6e9ef","border":"#bcc0cc","accent":"#8839ef","selection":"#ccd0da","on_accent":"#ffffff","foreground":"#4c4f69","secondary":"#5c5f77","muted_foreground":"#6c6f85","red":"#d20f39","green":"#407515","yellow":"#91610a","blue":"#1e66f5","magenta":"#a92bbb","cyan":"#087e91","orange":"#ba4806"},
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
        if let Some(foreground) = palette.get("foreground") {
            for (key, value) in [
                ("foreground", foreground),
                ("popover.foreground", foreground),
                ("sidebar.foreground", &palette["secondary"]),
                ("tab.active.foreground", foreground),
                ("group_box.foreground", &palette["secondary"]),
                ("secondary.foreground", &palette["secondary"]),
                ("accent.foreground", &palette["secondary"]),
                ("muted.foreground", &palette["muted_foreground"]),
                ("danger.background", &palette["red"]),
                ("danger.foreground", &palette["on_accent"]),
                ("danger.hover.background", &palette["red"]),
                ("warning.background", &palette["yellow"]),
                ("warning.foreground", &palette["on_accent"]),
                ("success.background", &palette["green"]),
                ("success.foreground", &palette["on_accent"]),
                ("base.red", &palette["red"]),
                ("base.green", &palette["green"]),
                ("base.yellow", &palette["yellow"]),
                ("base.blue", &palette["blue"]),
                ("base.magenta", &palette["magenta"]),
                ("base.cyan", &palette["cyan"]),
                ("base.orange", &palette["orange"]),
            ] {
                theme["colors"][key] = value.clone();
            }
            theme["highlight"]["editor.foreground"] = foreground.clone();
            theme["highlight"]["editor.line_number"] = palette["border"].clone();
            theme["highlight"]["editor.invisible"] = palette["border"].clone();
            theme["highlight"]["error"] = palette["red"].clone();
            theme["highlight"]["warning"] = palette["yellow"].clone();
            theme["highlight"]["success"] = palette["green"].clone();
            for (key, value) in [
                ("comment", &palette["muted_foreground"]),
                ("string", &palette["green"]),
                ("number", &palette["yellow"]),
                ("function", &palette["blue"]),
                ("type", &palette["cyan"]),
                ("operator", &palette["secondary"]),
                ("property", &palette["accent"]),
                ("constant", &palette["orange"]),
                ("punctuation", &palette["secondary"]),
                ("keyword", &palette["accent"]),
                ("title", &palette["accent"]),
                ("link_text", &palette["accent"]),
                ("link_uri", &palette["muted_foreground"]),
                ("emphasis", &palette["yellow"]),
                ("emphasis.strong", foreground),
                ("hint", &palette["muted_foreground"]),
                ("predictive", &palette["muted_foreground"]),
            ] {
                theme["highlight"]["syntax"][key]["color"] = value.clone();
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
    let mut names = cx
        .global::<ThemeCatalog>()
        .themes
        .iter()
        .filter(|t| t.mode == mode)
        .map(|t| t.name.to_string())
        .collect::<Vec<_>>();
    let preferred = if mode == ThemeMode::Dark {
        NIGHT_THEME_NAME
    } else {
        DAY_THEME_NAME
    };
    names.sort_by_key(|name| name != preferred);
    names
}

fn selected_theme(catalog: &ThemeCatalog, name: &str, mode: ThemeMode) -> std::rc::Rc<ThemeConfig> {
    let name = match name {
        "Rísta Night" => "Slate Night",
        "Rísta Day" => "Slate Day",
        other => other,
    };
    let preferred = if mode == ThemeMode::Dark {
        NIGHT_THEME_NAME
    } else {
        DAY_THEME_NAME
    };
    catalog
        .themes
        .iter()
        .find(|theme| theme.name.as_ref() == name && theme.mode == mode)
        .or_else(|| {
            catalog
                .themes
                .iter()
                .find(|theme| theme.name.as_ref() == preferred && theme.mode == mode)
        })
        .or_else(|| catalog.themes.iter().find(|theme| theme.mode == mode))
        .or_else(|| catalog.themes.first())
        .expect("theme catalog must not be empty")
        .clone()
}

pub fn apply(settings: &crate::settings::Settings, cx: &mut App) {
    let catalog = cx.global::<ThemeCatalog>();
    let dark = selected_theme(catalog, &settings.dark_theme, ThemeMode::Dark);
    let light = selected_theme(catalog, &settings.light_theme, ThemeMode::Light);
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
    let selected = selected_theme(catalog, name, mode);
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
    use super::{builtin_json, parse_custom, ThemeMode, ThemeSet};

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

    fn css_variables(css: &str, selector: &str) -> std::collections::HashMap<String, String> {
        let selector_start = css.find(selector).unwrap();
        let block_start = selector_start + css[selector_start..].find('{').unwrap();
        let block_end = block_start + css[block_start..].find('}').unwrap();
        css[block_start + 1..block_end]
            .lines()
            .filter_map(|line| {
                let (name, value) = line.trim().strip_prefix("--rista-")?.split_once(": ")?;
                Some((
                    name.trim_end_matches(';').to_owned(),
                    value.trim_end_matches(';').to_owned(),
                ))
            })
            .collect()
    }

    #[test]
    fn builtin_palettes_are_complete_and_exportable() {
        let json = builtin_json();
        let themes = json["themes"].as_array().unwrap();
        assert_eq!(themes.len(), 18);
        let names: std::collections::HashSet<_> =
            themes.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names.len(), 18);
        for mode in ["dark", "light"] {
            let matching: Vec<_> = themes.iter().filter(|t| t["mode"] == mode).collect();
            assert_eq!(matching.len(), 9);
        }
        assert_eq!(parse_custom(&json.to_string()).unwrap().themes.len(), 18);

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
        let ts = include_str!("../design/tokens.ts");
        let ts_body = ts
            .split_once("export const rista = ")
            .unwrap()
            .1
            .split_once(" as const;")
            .unwrap()
            .0;
        let ts_exports: serde_json::Value = serde_json::from_str(ts_body).unwrap();
        assert_eq!(ts_exports, exports["rista"]);
        for (mode, name) in [("dark", "Graphite Night"), ("light", "Graphite Day")] {
            let theme = themes.iter().find(|theme| theme["name"] == name).unwrap();
            assert_eq!(exports["rista"]["themes"][mode], exported_theme(theme));
        }
        let css = include_str!("../design/tokens.css");
        for (selector, mode, name) in [
            (":root", "dark", "Graphite Night"),
            (r#"[data-rista-theme="dark"]"#, "dark", "Graphite Night"),
            (r#"[data-rista-theme="light"]"#, "light", "Graphite Day"),
        ] {
            let theme = themes.iter().find(|theme| theme["name"] == name).unwrap();
            let expected = exported_theme(theme)["colors"].as_object().unwrap().clone();
            let actual = css_variables(css, selector);
            assert_eq!(actual.len(), expected.len());
            assert_eq!(exports["rista"]["themes"][mode], exported_theme(theme));
            for (key, value) in expected {
                assert_eq!(actual[&key.replace('.', "-")], value.as_str().unwrap());
            }
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
            assert_eq!(exports["rista"]["palettes"][&name], expected);
            let selector = format!(
                r#"[data-rista-theme="{}"]"#,
                name.to_lowercase().replace('í', "i").replace(' ', "-")
            );
            let css_palette = css_variables(css, &selector);
            let expected_colors = exported_theme(theme)["colors"].as_object().unwrap().clone();
            assert_eq!(css_palette.len(), expected_colors.len());
            for (key, value) in expected_colors {
                assert_eq!(
                    css_palette[&key.replace('.', "-")],
                    value.as_str().unwrap(),
                    "{name} CSS token {key}"
                );
            }
        }
        for (alias, name) in [("rista-night", "Slate Night"), ("rista-day", "Slate Day")] {
            let theme = themes.iter().find(|theme| theme["name"] == name).unwrap();
            let mut expected = exported_theme(theme);
            expected["mode"] = theme["mode"].clone();
            assert_eq!(exports["rista"]["palettes"][alias], expected);
            let alias_css = css_variables(css, &format!(r#"[data-rista-theme="{alias}"]"#));
            let expected_colors = exported_theme(theme)["colors"].as_object().unwrap().clone();
            assert_eq!(alias_css.len(), expected_colors.len());
            for (key, value) in expected_colors {
                assert_eq!(
                    alias_css[&key.replace('.', "-")],
                    value.as_str().unwrap(),
                    "{alias} CSS token {key}"
                );
            }
        }
        for (name, accent, red, syntax_string) in [
            ("Gruvbox Night", "#fabd2f", "#fb4934", "#b8bb26"),
            ("Nord Day", "#466b94", "#a33b48", "#4f6b35"),
            ("Solarized Night", "#2aa198", "#dc322f", "#859900"),
            ("Catppuccin Day", "#8839ef", "#d20f39", "#407515"),
        ] {
            let theme = themes.iter().find(|theme| theme["name"] == name).unwrap();
            assert_eq!(theme["colors"]["primary.background"], accent);
            assert_eq!(theme["colors"]["base.red"], red);
            assert_eq!(
                theme["highlight"]["syntax"]["string"]["color"],
                syntax_string
            );
        }
        for name in [
            "Gruvbox Night",
            "Gruvbox Day",
            "Nord Night",
            "Nord Day",
            "Solarized Night",
            "Solarized Day",
            "Catppuccin Night",
            "Catppuccin Day",
        ] {
            let theme = themes.iter().find(|theme| theme["name"] == name).unwrap();
            assert_eq!(
                theme["colors"]["warning.foreground"], theme["colors"]["primary.foreground"],
                "{name}"
            );
        }

        assert!(css.contains(
            ":root, [data-rista-theme=\"dark\"], [data-rista-theme=\"graphite-night\"] {"
        ));
        assert!(css.contains("[data-rista-theme=\"light\"], [data-rista-theme=\"graphite-day\"] {"));
        for selector in [
            "dark",
            "light",
            "rista-night",
            "rista-day",
            "slate-night",
            "slate-day",
            "fjord-night",
            "fjord-day",
            "rose-night",
            "rose-day",
            "graphite-night",
            "graphite-day",
            "mono-night",
            "mono-day",
            "gruvbox-night",
            "gruvbox-day",
            "nord-night",
            "nord-day",
            "solarized-night",
            "solarized-day",
            "catppuccin-night",
            "catppuccin-day",
            "dark",
            "light",
        ] {
            assert!(css.contains(&format!(r#"[data-rista-theme="{selector}"]"#)));
        }
    }

    #[test]
    fn theme_selection_uses_exact_mode_and_graphite_fallback() {
        let set: ThemeSet = serde_json::from_value(builtin_json()).unwrap();
        let catalog = super::ThemeCatalog {
            themes: set.themes.into_iter().map(std::rc::Rc::new).collect(),
            errors: Vec::new(),
        };
        assert_eq!(
            super::selected_theme(&catalog, "Rísta Night", ThemeMode::Dark)
                .name
                .as_ref(),
            "Slate Night"
        );
        assert_eq!(
            super::selected_theme(&catalog, "Fjord Night", ThemeMode::Light)
                .name
                .as_ref(),
            "Graphite Day"
        );
        assert_eq!(
            super::selected_theme(&catalog, "Removed custom", ThemeMode::Dark)
                .name
                .as_ref(),
            "Graphite Night"
        );
        let incomplete = super::ThemeCatalog {
            themes: vec![catalog
                .themes
                .iter()
                .find(|theme| theme.name.as_ref() == "Fjord Night")
                .unwrap()
                .clone()],
            errors: Vec::new(),
        };
        assert_eq!(
            super::selected_theme(&incomplete, "Missing", ThemeMode::Light)
                .name
                .as_ref(),
            "Fjord Night"
        );
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
