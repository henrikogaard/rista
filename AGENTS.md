# AGENTS.md — Rista

Handoff document for coding agents. Read this before touching any file.

---

## Agent operating rules

These rules apply before any code change.

- **Think before coding** — state assumptions when the request is ambiguous. If there are multiple reasonable interpretations, name them before choosing. Ask when the wrong assumption would cause rework.
- **Simplicity first** — implement the smallest change that solves the request. Do not add speculative abstractions, configurability, or extra features.
- **Surgical changes** — touch only files and lines needed for the task. Match the existing style. Do not refactor adjacent code, reformat unrelated sections, or delete pre-existing dead code unless explicitly asked.
- **Clean up your own trail** — remove imports, variables, functions, or tests made obsolete by your own change. Leave unrelated existing cleanup as a note, not an edit.
- **Verify the goal** — for each non-trivial task, define what proves success before or while implementing. Prefer tests for logic changes, builds for integration changes, and rendered checks for UI changes.
- **Every changed line must justify itself** — if a line cannot be traced back to the user request, the repo's conventions, or required verification, do not change it.

---

## What is Rísta

A **local-first, MIT-licensed Markdown editor** written in Rust on GPUI.
The design language is Nordic/Scandinavian — dark, minimal, purposeful.

Users open a folder. All `.md` files in that folder become a "vault".
No cloud. No accounts. No telemetry. Files are just files.

---

## Design principles

- **Paired palettes** — Graphite, Slate, Fjord, Rose, Mono, Gruvbox, Nord,
  Solarized, and Catppuccin Night/Day, defined in `src/theme.rs`, with user JSON
  themes. All colors are theme tokens.
- **Compact density** — tight spacing, small type (11–14px), nothing wastes vertical space.
- **No gradients in UI chrome** — flat mineral surfaces only.
- **Quiet chrome** — borderless toolbar controls, one-pixel separators, text-only status bar.
- **Core by default** — writing workflows come first. The shipped native baseline also includes the note/link graph and calendar `.base` views; treat the current architecture documentation as the source of truth for shipped modules.

---

## Tech stack

| Layer | Technology |
|---|---|
| Language | Rust 2021 |
| UI framework | GPUI via `gpui-kit` (`gpui-component` + `gpui-base`) |
| Editor | `gpui_kit::component::input::Editor` (Rope + tree-sitter) |
| Markdown preview | `gpui_kit::component::text::TextView` + `markdown` crate, custom `MarkdownPlugin`s |
| File watching | `notify` crate, 250ms debounce |
| Settings | `serde`/`serde_json` under `directories` config dir |
| Async | `smol` timers inside `cx.spawn` / `cx.spawn_in` |

Single Rust crate and GPUI binary; the previous web renderer is not part of the current app.

The packaged app is tested on Apple Silicon macOS and declares macOS 13 as its minimum. Not every OS version has been tested. Linux and Windows are intended targets, but their builds, packages, and keyboard mappings are not verified.

---

## Project structure

```
rista/
├── src/
│   ├── main.rs            # Bootstrap, menus, key bindings, file URLs, reopen
│   ├── app.rs             # Workspace, tabs, vault UI, and interactions
│   ├── document.rs        # Editor, dirty baseline, conflicts, autosave/preview
│   ├── vault.rs           # Indexes, aliases, and filesystem watcher
│   ├── preview.rs         # Markdown preview and MarkdownPlugin pipeline
│   ├── bases.rs           # .base queries, formulas, views, relations, rollups
│   ├── properties.rs      # Frontmatter properties
│   ├── history.rs         # .rista/history and trash
│   ├── graph.rs           # Note/link graph
│   ├── slash.rs           # Slash-command completions
│   ├── emoji.rs           # Emoji support
│   ├── decorations.rs     # Editor decorations
│   ├── settings.rs        # Persisted and runtime settings
│   ├── settings_panel.rs  # Settings UI
│   ├── theme.rs           # Theme packs and theme helpers
│   └── actions.rs         # Keyboard action types
├── design/                # Generated tokens.json / tokens.css / tokens.ts
├── public/                # App icon and brand assets
├── Cargo.toml
├── DESIGN.md
├── README.md
└── docs/README.md         # Active documentation index and legacy inventory
```

On macOS, settings are stored at `~/Library/Application Support/no.ogard.rista/settings.json`. Runtime preferences include theme, source, split, and preview modes.

See [docs/README.md](docs/README.md) for current references and the disposition of superseded documents.

---

## GPUI conventions in this codebase

- All GPUI surface comes through the umbrella crate: `use gpui_kit::*;` plus
  targeted `use gpui_kit::{base, component, prelude}` imports.
- Theme access needs `use gpui_kit::component::ActiveTheme;` → `cx.theme()`.
- Text emphasis needs `use gpui_kit::base::StyledExt;` (`.font_semibold()` etc.).
- Button variants need `use gpui_kit::component::button::ButtonVariants as _;`,
  sizes need `component::Sizable`, builder conditionals need `prelude::FluentBuilder`.
- Async: `cx.spawn(async move |this: WeakEntity<T>, cx| { ... })` —
  the closure itself is `async`, and update via `this.update(&mut *cx, ...)`.
  When you need `&mut Window` after an await (file pickers, opening docs):
  `cx.spawn_in(window, async move |view, window| { window.update(|w, cx| ...) })`.
- Overlays: `window.open_dialog(cx, Fn(Dialog,..))`, `open_alert_dialog`,
  `open_sheet_at(Placement::Right, cx, Fn(Sheet,..))` — overlay closures are
  `Fn`, so clone captured values inside.
- Every focusable root implements `Focusable` (`focus_handle`) and renders
  `.track_focus(&handle).key_context("...")` with `.on_action` handlers.

---

## Theme tokens

Defined in `src/theme.rs` as JSON theme packs (Zed-compatible shape):
`colors` = gpui-component semantic tokens, `highlight.syntax` = tree-sitter
palette. Never hardcode a color in UI code — always `cx.theme().<token>`.

`design/tokens.{json,css,ts}` are generated exports for non-Rust consumers
(marketing site); regenerate them when the theme packs change (see DESIGN.md).

---

## Keyboard shortcuts

The verified bindings are macOS command-key shortcuts. Linux and Windows mappings have not been verified.

| Action | macOS |
|---|---|
| Save | `⌘S` |
| Save as | `⌘⇧S` |
| Toggle sidebar | `⌘B` |
| Find in note | `⌘F` |
| Project search | `⌘⇧F` |
| Command palette | `⌘K` |
| Settings | `⌘,` |
| Daily note | `⌘⇧D` |
| New file | `⌘N` |
| Open standalone file (notes or read-only attachment preview) | `⌘O` |
| Open vault | `⌘⇧O` |
| Zen mode | `⌘⇧Enter` |

Bindings live in `main.rs` (`cx.bind_keys`); actions in `src/actions.rs`;
handlers are `Workspace::on_*` methods in `app.rs`.

## Running locally

```bash
cargo run -- /absolute/path
cargo build --release
cargo fmt --check
cargo clippy
cargo test
```

## Contribution conventions

- **Commits**: conventional commits — `feat:`, `fix:`, `style:`, `refactor:`
- **Rust style**: `cargo fmt` enforced; clippy warnings fixed, not allowed
- **File naming**: snake_case for Rust files, kebab-case for docs
- **Generated files**: `design/tokens.*` are generated — edit `src/theme.rs` instead
