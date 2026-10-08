#!/usr/bin/env python3
"""Export the bundled Rust palettes for framework-agnostic consumers."""
import copy
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
source = (ROOT / "src/theme.rs").read_text()
base_source = source.split('if let Some(foreground) = palette.get("foreground")')[0]


def constant(name):
    return json.loads(re.search(rf'const {name}: &str = r##"(.*?)"##;', source, re.S)[1])


themes = constant("RISTA_THEMES")["themes"]
originals = copy.deepcopy(themes)
mappings = re.search(r'for \(key, value\) in \[(.*?)\] \{', source, re.S)[1]
for palette in constant("RISTA_PALETTES"):
    theme = copy.deepcopy(next(t for t in originals if t["mode"] == palette["mode"]))
    theme["name"] = palette["name"]
    for key, value in re.findall(r'\("([^"]+)",\s*"([^"]+)"\)', mappings):
        theme["colors"][key] = palette[value]
    for key, value in re.findall(r'theme\["highlight"\]\["([^"]+)"\] = palette\["([^"]+)"\]', base_source):
        theme["highlight"][key] = palette[value]
    syntax_keys = re.search(r'for key in \[(.*?)\] \{\s*theme\["highlight"\]\["syntax"\]', source, re.S)[1]
    for key in re.findall(r'"([^"]+)"', syntax_keys):
        theme["highlight"]["syntax"][key]["color"] = palette["accent"]
    for key, value in palette.get("colors", {}).items():
        theme["colors"][key] = value
    for key, value in palette.get("highlight", {}).items():
        theme["highlight"][key] = value
    for key, value in palette.get("syntax", {}).items():
        theme["highlight"]["syntax"][key]["color"] = value
    if "foreground" in palette:
        foreground = palette["foreground"]
        for key, value in {
            "foreground": foreground, "popover.foreground": foreground,
            "sidebar.foreground": palette["secondary"],
            "tab.active.foreground": foreground, "group_box.foreground": palette["secondary"],
            "secondary.foreground": palette["secondary"], "accent.foreground": palette["secondary"],
            "muted.foreground": palette["muted_foreground"],
            "danger.background": palette["red"], "danger.foreground": palette["on_accent"],
            "danger.hover.background": palette["red"],
            "warning.background": palette["yellow"], "warning.foreground": palette["on_accent"],
            "success.background": palette["green"], "success.foreground": palette["on_accent"],
            "base.red": palette["red"], "base.green": palette["green"],
            "base.yellow": palette["yellow"], "base.blue": palette["blue"],
            "base.magenta": palette["magenta"], "base.cyan": palette["cyan"],
            "base.orange": palette["orange"],
        }.items():
            theme["colors"][key] = value
        theme["highlight"].update({
            "editor.foreground": foreground, "editor.line_number": palette["border"],
            "editor.invisible": palette["border"],
            "error": palette["red"], "warning": palette["yellow"], "success": palette["green"],
        })
        for key, value in {
            "comment": palette["muted_foreground"], "string": palette["green"],
            "number": palette["yellow"], "function": palette["blue"], "type": palette["cyan"],
            "operator": palette["secondary"], "property": palette["accent"],
            "constant": palette["orange"], "punctuation": palette["secondary"],
            "keyword": palette["accent"], "title": palette["accent"],
            "link_text": palette["accent"], "link_uri": palette["muted_foreground"],
            "emphasis": palette["yellow"], "emphasis.strong": foreground,
            "hint": palette["muted_foreground"], "predictive": palette["muted_foreground"],
        }.items():
            theme["highlight"]["syntax"][key]["color"] = value
    themes.append(theme)


def tokens(theme):
    return {"name": theme["name"], "colors": {
        **theme["colors"],
        **{f"highlight.{k}": v for k, v in theme["highlight"].items() if k != "syntax"},
        **{f"syntax.{k}": v["color"] for k, v in theme["highlight"]["syntax"].items()},
    }}


def slug(theme):
    return theme["name"].lower().replace("í", "i").replace(" ", "-")


defaults = {
    mode: next(t for t in themes if t["name"] == f"Graphite {'Night' if mode == 'dark' else 'Day'}")
    for mode in ("dark", "light")
}
palette_tokens = {slug(t): {"mode": t["mode"], **tokens(t)} for t in themes}
for alias, name in (("rista-night", "Slate Night"), ("rista-day", "Slate Day")):
    theme = next(t for t in themes if t["name"] == name)
    palette_tokens[alias] = {"mode": theme["mode"], **tokens(theme)}
data = {
    "themes": {mode: tokens(theme) for mode, theme in defaults.items()},
    "palettes": palette_tokens,
}
header = "Rísta design tokens — generated from src/theme.rs. Do not edit by hand."
(ROOT / "design/tokens.json").write_text(json.dumps({"rista": data}, indent=2) + "\n")
(ROOT / "design/tokens.ts").write_text(
    f"// {header}\n\nexport const rista = " + json.dumps(data, indent=2)
    + " as const;\n\nexport default rista;\n"
)
css = [f"/* {header} */", ""]
for theme in themes:
    selectors = [f'[data-rista-theme="{slug(theme)}"]']
    if theme["name"] == "Graphite Night":
        selectors.insert(0, ":root")
        selectors.insert(1, f'[data-rista-theme="{theme["mode"]}"]')
    elif theme["name"] == "Graphite Day":
        selectors.insert(0, f'[data-rista-theme="{theme["mode"]}"]')
    elif theme["name"] == "Slate Night":
        selectors.append('[data-rista-theme="rista-night"]')
    elif theme["name"] == "Slate Day":
        selectors.append('[data-rista-theme="rista-day"]')
    css.append(", ".join(selectors) + " {")
    css.extend(f'  --rista-{k.replace(".", "-")}: {v};' for k, v in tokens(theme)["colors"].items())
    css.extend(["}", ""])
(ROOT / "design/tokens.css").write_text("\n".join(css))
