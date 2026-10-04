# Rísta Design System

Quiet, modern-mythic. Nordic restraint: mineral surfaces, muted blue accents,
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
2. **Quiet chrome.** Borderless toolbar controls; one-pixel separators; no
   gradients anywhere (the old web app allowed one on the welcome surface — the
   native app does not need it).
3. **Compact density.** Small type, tight spacing, no wasted vertical space.
   Default UI text 13px, editor 14px, chrome labels 11–12px.
4. **Semantic tokens only.** Never hardcode a color in Rust UI code —
   read `cx.theme()` values. New colors go in the theme packs.
5. **Files stay files.** No cloud, accounts, sync UI, or telemetry affordances.

## Color

Palette intent per token group (values in `design/tokens.json`):

| Token | Night | Day | Role |
|---|---|---|---|
| `background` | `#1a1d24` | `#f6f4ef` | Deepest surface |
| `foreground` | `#d8dce5` | `#363b47` | Primary text |
| `accent.background` | `#232834` | `#e9e6dd` | Interactive fills |
| `ring` / `caret` | `#88b3d4` / `#9fc2dd` | `#5b7fa3` / `#4a6b8f` | Focus + cursor |
| `primary.background` | `#6b86a8` | `#5b7fa3` | Primary action |
| `sidebar.background` | `#1f232c` | `#f0eee7` | Vault rail |
| `muted.foreground` | `#7a8194` | `#8a8fa0` | Secondary text |
| `success` / `warning` / `danger` | muted green/amber/red | — | Semantic only |

Accent is desaturated Nordic blue-grey — never electric blue, never warm brand color.

## Typography

- UI: system font (SF on macOS), 11–13px. Section labels uppercase, semibold, muted.
- Editor: monospace stack (SF Mono default), 14px default, configurable 12–22.
- Preview: headings weight 600+, tight leading; body matches UI font size.

## Spacing & radius

- Base spacing unit 4px; standard gaps 8/12/16px (`gap_2/3/4`).
- Radius follows gpui-kit `radius`/`radius.lg` — no custom radii.
- Sidebar default 240px, resizable 170–420px. Split panes ≥ 280px each.

## Components

- **TitleBar** — custom chrome with traffic-light spacing, vault name centered,
  right-side actions (search ⌘K, zen, theme, settings).
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
src = open('src/theme.rs').read()
j = re.search(r'const RISTA_THEMES: &str = r##"(.*?)"##;', src, re.S).group(1)
json.dump({"rista": json.loads(j)}, open('design/tokens.json','w'), indent=2)
EOF
```

then rebuild `tokens.css`/`tokens.ts` by flattening `colors` + `highlight.syntax`
into `--rista-<token>` names (see git history for the generator snippet).
