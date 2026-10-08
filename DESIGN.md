# Rísta Design System

Quiet, modern-mythic. Nordic restraint: mineral surfaces, restrained accents,
terse type, nothing ornamental. Files are the product; chrome recedes.

The source of truth for UI tokens is `src/theme.rs`
(Rísta, Fjord, Rose, Graphite, and Mono light/dark palettes),
exported framework-agnostically:

- `design/tokens.json` — generated values
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

Folder dashboards use a single identity header, quiet breadcrumb/action rows,
and compact bordered content cards. Gallery media belongs to documents rather
than chrome. Pins keep a stable order; layout/filter controls remain local to the
folder. The terminal is a collapsible, resizable lower pane, not a competing
sidebar. ANSI red, green, and yellow use the selected palette's semantic colors,
while blue, magenta, and cyan use its base colors; explicit 256-color and
true-color escape sequences remain process-supplied content.

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
Fjord uses cool blue surfaces and accents; Rose uses warm plum surfaces and
rose accents. Graphite and Mono are monochrome: neutral grey (Graphite) or pure
black-and-white (Mono) surfaces with a near-black/white accent; semantic and
terminal ANSI colours use the system palette. Light and dark palettes are
selected independently in Settings.
Accent tokens apply to controls, links, selection, the caret, and syntax accents.

### Custom palettes

Settings → **Create theme…** copies the active palette into a JSON file in the
app configuration directory's `themes/` folder and reveals it in the file manager.
On macOS this is `~/Library/Application Support/no.ogard.rista/themes/`.
Edit the file in a text editor, then choose **Reload themes** and select its name.
Reload also picks up edits and removals; invalid files are reported without
discarding valid palettes. An unavailable selected palette falls back to the
built-in palette for that mode.

Each file is a theme set with `themes: [...]`. Each theme needs a unique `name`
and `mode` (`dark` or `light`). Use `#RRGGBB` or `#RRGGBBAA` for `colors`.
Start from the generated copy to retain all semantic and syntax tokens.
Useful keys: `background`, `sidebar.background`, `foreground`,
`primary.background`, `primary.foreground`, `info.background`, `ring`, `caret`,
`selection`, and `accent.background` (a **fill**, not a text colour).
Syntax colours live under `highlight.syntax`; retain that nested object.

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
- **Tabs** — single-click tree browsing reuses an italic preview tab. Double-click
  the file or tab to keep it; editing and pinning also keep it. Cmd-click opens a
  separate tab. Dirty, conflicted, and pinned documents cannot be replaced.
  Preview status is session-local; restored tabs are retained.
- **Command palette** — centered `Command` dialog, Commands then Notes sections.
- **Dialogs** — `open_dialog` for palette/search; `open_alert_dialog` for
  destructive confirms; `open_sheet_at(Placement::Right)` for settings.
- **Status bar** — relative path, word count, view mode. Text only, no chrome.
- **Terminal** — independent rounded canvas below the editor, matching its
  surface and gutters. The resize grip appears only on hover or drag.
- **Zen mode** — hides everything but editor+preview.

## Markdown preview rendering

Frontmatter renders as editable properties; `cover`/`banner` supplies a page
header image. `[[links]]` become navigable note links. `![[note]]` transcludes
note content, while `![[image]]` renders an inline image. Callouts become
titled, collapsible boxes. `^block-id` markers are hidden from prose.
Database embeds reuse native `.base` views. Math supports a Unicode-rendered
subset, not a full TeX engine. See `src/preview.rs` for the rendering pipeline.

Properties can default to expanded, collapsed, or hidden in Settings. This is
presentation only: hidden properties remain editable through the inspector.
Note icons use `:LiInbox:`, `:li-inbox:`, or `:lucide-inbox:` shortcodes. Bundled
SVGs inherit the surrounding text size and colour, including headings. Unknown
shortcodes remain text; code and frontmatter are not rewritten. Source files
are never converted to image markup.

## Brand identity

The **carved R** replaces the earlier multicolour rune. A single geometric
letter has a chamfered bowl and a separated diagonal leg: an incision rather
than an ornament. Warm paper and ink keep it legible in both app themes.

| Asset | Use |
|---|---|
| [`public/icon.svg`](public/icon.svg) | Master app icon, warm paper tile |
| [`public/icon.png`](public/icon.png) | 1024 × 1024 transparent PNG used by the macOS bundle |
| [`rista-icon-dark.svg`](public/logos/rista-icon-dark.svg) | Alternate dark tile; not an automatic OS appearance switch |
| [`rista-mark.svg`](public/logos/rista-mark.svg) | Ink mark on a light background |
| [`rista-mark-light.svg`](public/logos/rista-mark-light.svg) | Paper mark on a dark background |

- Ink: `#363b47`; paper: `#f6f4ef` (from Rísta Day foreground/background).
- Keep the SVG viewBox and built-in clear space. Do not stretch or rotate.
- Use the icon at 16px or larger; use the standalone mark at 24px or larger.
- Pair with the name **Rísta**, preserving the acute accent. Plain ASCII
  `Rista` is reserved for filenames that need portable URLs.
- Do not add gradients, multicolour fills, outlines around the letter, or
  extra runic details. The icon's small offset shadow is not UI chrome.
- The original brand artwork is covered by the repository's [MIT license](LICENSE).

After editing the master SVG, regenerate the committed app PNG on macOS:

```bash
sips -s format png public/icon.svg --out public/icon.png
```

The bundle script creates a full macOS iconset from that PNG. Inspect the
16px, 32px, 128px, and 512px results on light and dark backgrounds before
shipping. Do not regenerate the app's UI tokens when only brand assets change.

## Generating token exports

```bash
python3 scripts/export-theme-tokens.py
```

The existing `themes.dark`/`themes.light` exports and CSS selectors remain
compatible. `palettes` adds all ten named variants, selectable in CSS with e.g.
`data-rista-theme="fjord-night"`.
