# Rísta Design System

Quiet, modern-mythic. Nordic restraint: mineral surfaces, restrained accents,
terse type, nothing ornamental. Files are the product; chrome recedes.

The tokens below are the single source of truth in `src/theme.rs`
(Rísta Night / Rísta Day), exported framework-agnostically:

- `design/tokens.json` — canonical values
- `design/tokens.css` — CSS custom properties (`--rista-*`), usable by any web surface
- `design/tokens.ts` — typed constants for TypeScript consumers

Regenerate after editing `src/theme.rs` (see "Generating token exports").

---

## Principles

1. **Dark first.** Rísta Night is the default; Day is a full peer, not an inversion.
2. **Quiet chrome.** Borderless pane cards and toolbar controls; canvas gutters
   separate the panes. Resize grips appear only on hover or drag. No gradients.
3. **Compact density.** Small type, tight spacing, no wasted vertical space.
   Default UI text 13px, editor 14px, chrome labels 11–12px.
4. **Semantic tokens only.** Never hardcode a color in Rust UI code —
   read `cx.theme()` values. New colors go in the theme packs.
5. **Files stay files.** No cloud, accounts, sync UI, or telemetry affordances.

## Color

Palette intent per token group (values in `design/tokens.json`):

| Token | Night | Day | Role |
|---|---|---|---|
| `background` | `#0a0b0e` | `#f6f4ef` | Canvas |
| `foreground` | `#d8dce5` | `#363b47` | Primary text |
| `accent.background` | `#29443f` | `#e9e6dd` | Interactive fills |
| `ring` / `caret` | `#6fd8c8` / `#7ee0d0` | `#5b7fa3` / `#4a6b8f` | Focus + cursor |
| `primary.background` | `#6fd8c8` | `#5b7fa3` | Primary action |
| `sidebar.background` | `#121419` | `#ffffff` | Vault rail |
| `muted.foreground` | `#82899a` | `#727887` | Secondary text |
| `success` / `warning` / `danger` | muted green/amber/red | — | Semantic only |

Night uses a restrained teal accent; Day uses blue-grey.

## Typography

- UI: system font (SF on macOS), 11–13px. Section labels uppercase, semibold, muted.
- Editor: monospace stack (SF Mono default), 14px default, configurable 12–22.
- Preview: headings weight 600+, tight leading; body matches UI font size.

## Spacing & radius

- Base spacing unit 4px; standard gaps 8/12/16px (`gap_2/3/4`).
- Radius follows `radius` (8px) / `radius.lg` (12px) — no custom radii.
- Sidebar default 260px, resizable 200–420px. Split panes ≥ 280px each.
- Files and document headers are 44px high; pane gutters are 8px.
- Sidebar detail sections scroll within at most 45% of the pane height so the
  file tree retains room. Each section keeps its own collapse control.

## Components

- **TitleBar** — flat canvas chrome with traffic-light spacing, vault name
  beside the sidebar toggle, and right-side search, theme, and settings actions.
- **File tree** — `TreeItem` folders before files, depth ≤ 12, dotfiles and
  `.git/node_modules/target/dist` hidden. Context menu: New file here / Rename / Delete.
- **Tabs** — one per open note, close on click; dirty state implicit (autosave).
- **Command palette** — centered `Command` dialog, Commands then Notes sections.
- **Dialogs** — `open_dialog` for palette/search; `open_alert_dialog` for
  destructive confirms; `open_sheet_at(Placement::Right)` for settings.
- **Status bar** — relative path, word count, view mode. Text only, no chrome.
- **Zen mode** — hides everything but editor+preview.

## Markdown preview rendering

Frontmatter → properties card. `[[links]]` → accent-colored links.
`![[note]]` → note link; `![[image]]` → inline image. Callouts → titled boxes
with `IconName` glyphs. `^block-id` → hidden anchor (stripped).

## Generating token exports

```bash
python3 - <<'EOF'
import re, json
from pathlib import Path
src = open('src/theme.rs').read()
j = re.search(r'const RISTA_THEMES: &str = r##"(.*?)"##;', src, re.S).group(1)
themes = {
    t["mode"]: {
        "name": t["name"],
        "colors": {
            **t["colors"],
            **{
                f"highlight.{name}": value
                for name, value in t["highlight"].items()
                if name != "syntax"
            },
            **{
                f"syntax.{name}": value["color"]
                for name, value in t["highlight"]["syntax"].items()
            },
        },
    }
    for t in json.loads(j)["themes"]
}
tokens = {"themes": themes}
Path("design/tokens.json").write_text(json.dumps({"rista": tokens}, indent=2) + "\n")
header = "Rísta design tokens — generated from src/theme.rs. Do not edit by hand."
Path("design/tokens.ts").write_text(
    f"// {header}\n\nexport const rista = "
    + json.dumps(tokens, indent=2)
    + " as const;\n\nexport default rista;\n"
)
css = [f"/* {header} */", ""]
for mode, theme in themes.items():
    selector = ':root, ' if mode == "dark" else ''
    css.append(selector + f'[data-rista-theme="{mode}"] {{')
    for name, value in theme["colors"].items():
        css.append(f'  --rista-{name.replace(".", "-")}: {value};')
    css.extend(["}", ""])
Path("design/tokens.css").write_text("\n".join(css))
EOF
```
