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

A **local-first, open-source Markdown editor** written in Rust on GPUI.
The design language is Nordic/Scandinavian — dark, minimal, purposeful.

Users open a folder. All `.md` files in that folder become a "vault".
No cloud. No accounts. No telemetry. Files are just files.

---

## Design principles

- **Two themes** — Rísta Night (dark, default) and Rísta Day (light), defined in `src/theme.rs`. All colors are theme tokens.
- **Compact density** — tight spacing, small type (11–14px), nothing wastes vertical space.
- **No gradients in UI chrome** — flat mineral surfaces only.
- **Quiet chrome** — borderless toolbar controls, one-pixel separators, text-only status bar.
- **Core by default** — writing features come first. Non-core modules (graph, agents, calendar, terminal, publish) are out of scope until a settings/flag system justifies them.

---

## Tech stack

| Layer | Technology |
|---|---|
| UI framework | GPUI via `gpui-kit` (`gpui-component` + `gpui-base`) |
| Editor | `gpui_kit::component::input::Editor` (Rope + tree-sitter) |
| Markdown preview | `gpui_kit::component::text::TextView` + `markdown` crate, custom `MarkdownPlugin`s |
| File watching | `notify` crate, 250ms debounce |
| Settings | `serde`/`serde_json` under `directories` config dir |
| Async | `smol` timers inside `cx.spawn` / `cx.spawn_in` |

Single crate, single binary — there is no web renderer, no JS, no IPC bridge.
Platforms: macOS (primary), Linux, Windows (same code path).

---

## Project structure

```
rista/
├── src/
│   ├── main.rs            # Bootstrap: themes, keymap, menus, window
│   ├── app.rs             # Workspace view: sidebar tree, tabs, palette,
│   │                      # dialogs, zen mode, action handlers, render
│   ├── vault.rs           # Vault entity: root, notes/images index,
│   │                      # TreeItem builder, notify watcher → VaultEvent
│   ├── document.rs        # Document entity: EditorState, autosave (800ms),
│   │                      # preview sync (150ms), external-change check
│   ├── preview.rs         # MarkdownNode render + the reference editor preprocessing:
│   │                      # frontmatter, [[wikilinks]], ![[embeds]], ^block-id,
│   │                      # > [!callouts]
│   ├── search.rs          # Project-search dialog (⌘⇧F)
│   ├── settings.rs        # Settings model, load/save, theme resolution
│   ├── settings_panel.rs  # Settings sheet (Placement::Right)
│   ├── theme.rs           # RÍSTA_THEMES JSON packs + install/change helpers
│   └── actions.rs         # actions!(rista, ...) keyboard action types
├── design/                # tokens.json / tokens.css / tokens.ts (generated)
├── public/                # icon + brand assets
├── Cargo.toml
├── DESIGN.md              # Design system (colors, type, components)
└── README.md
```

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

| Action | macOS | Linux/Windows |
|---|---|---|
| Save | `⌘S` | `Ctrl+S` |
| Save as | `⌘⇧S` | `Ctrl+Shift+S` |
| Toggle sidebar | `⌘B` | `Ctrl+B` |
| Find in note | `⌘F` | `Ctrl+F` |
| Project search | `⌘⇧F` | `Ctrl+Shift+F` |
| Command palette | `⌘K` | `Ctrl+K` |
| Settings | `⌘,` | `Ctrl+,` |
| Daily note | `⌘⇧D` | `Ctrl+Shift+D` |
| New file | `⌘N` | `Ctrl+N` |
| Open file | `⌘O` | `Ctrl+O` |
| Open folder | `⌘⇧O` | `Ctrl+Shift+O` |
| Zen mode | `⌘⇧Enter` | `Ctrl+Shift+Enter` |

Bindings live in `main.rs` (`cx.bind_keys`); actions in `src/actions.rs`;
handlers are `Workspace::on_*` methods in `app.rs`.

## Running locally

```bash
cargo run              # debug build + launch
cargo build --release  # optimized binary in target/release/rista
cargo clippy           # lint (keep clean)
cargo fmt              # required before commit
```

## Contribution conventions

- **Commits**: conventional commits — `feat:`, `fix:`, `style:`, `refactor:`
- **Rust style**: `cargo fmt` enforced; clippy warnings fixed, not allowed
- **File naming**: snake_case for Rust files, kebab-case for docs
- **Generated files**: `design/tokens.*` are generated — edit `src/theme.rs` instead
