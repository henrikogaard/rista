#!/usr/bin/env python3
"""Export the bundled Rust palettes for framework-agnostic consumers."""
import copy
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
source = (ROOT / "src/theme.rs").read_text()


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
    for key, value in re.findall(r'theme\["highlight"\]\["([^"]+)"\] = palette\["([^"]+)"\]', source):
        theme["highlight"][key] = palette[value]
    syntax_keys = re.search(r'for key in \[(.*?)\] \{\s*theme\["highlight"\]\["syntax"\]', source, re.S)[1]
    for key in re.findall(r'"([^"]+)"', syntax_keys):
        theme["highlight"]["syntax"][key]["color"] = palette["accent"]
    themes.append(theme)


def tokens(theme):
    return {"name": theme["name"], "colors": {
        **theme["colors"],
        **{f"highlight.{k}": v for k, v in theme["highlight"].items() if k != "syntax"},
        **{f"syntax.{k}": v["color"] for k, v in theme["highlight"]["syntax"].items()},
    }}


def slug(theme):
    return theme["name"].lower().replace("í", "i").replace(" ", "-")


data = {
    "themes": {t["mode"]: tokens(t) for t in originals},
    "palettes": {slug(t): {"mode": t["mode"], **tokens(t)} for t in themes},
}
header = "Rísta design tokens — generated from src/theme.rs. Do not edit by hand."
(ROOT / "design/tokens.json").write_text(json.dumps({"rista": data}, indent=2) + "\n")
(ROOT / "design/tokens.ts").write_text(
    f"// {header}\n\nexport const rista = " + json.dumps(data, indent=2)
    + " as const;\n\nexport default rista;\n"
)
css = [f"/* {header} */", ""]
for index, theme in enumerate(themes):
    selectors = [f'[data-rista-theme="{slug(theme)}"]']
    if index < 2:
        selectors.insert(0, f'[data-rista-theme="{theme["mode"]}"]')
    if index == 0:
        selectors.insert(0, ":root")
    css.append(", ".join(selectors) + " {")
    css.extend(f'  --rista-{k.replace(".", "-")}: {v};' for k, v in tokens(theme)["colors"].items())
    css.extend(["}", ""])
(ROOT / "design/tokens.css").write_text("\n".join(css))
