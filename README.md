# Rísta

<p align="center">
  <img src="./public/icon.svg" width="132" height="132" alt="Rísta logo" />
</p>

<p align="center">
  <strong>A local-first Markdown editor with a quiet, modern-mythic interface.</strong>
</p>

<p align="center">
  Open a folder. Write in plain Markdown. Keep your files on disk, under your control.
</p>

<p align="center">
  <code>Rust</code> · <code>GPUI</code> · <code>gpui-kit</code> · <code>notify</code>
</p>

---

## Overview

Rísta is a small, fast native desktop editor for people who want Markdown files to stay as files.
There is no cloud sync, no account system, and no telemetry layer between you and your notes.

The interface is compact and dark by default. The name references runes, but the product stays restrained: carved geometry, mineral color, terse language, and low-noise local writing tools.

Rísta is written entirely in Rust on [GPUI](https://gpui.rs) via the
[`gpui-kit`](https://gpui-kit.com) component set — one codebase for macOS, Linux, and
Windows, developed macOS-first.

## Highlights

- Local-first folder workflow — a folder is a vault, every `.md` a note
- File tree sidebar with create / rename / delete and context menus
- Multi-tab editing with source, split, and preview view modes
- Live Markdown preview with Obsidian extensions (below)
- Autosave on every pause, plus file watching for external changes
- Find in note (`⌘F`) and project search (`⌘⇧F`)
- Command palette (`⌘K`) over notes and commands
- Dark and light themes, system-aware; settings sheet (`⌘,`)
- Zen mode (`⌘⇧⏎`) — everything but the words

## Obsidian compatibility

- YAML frontmatter renders as a properties block in preview
- `[[wikilinks]]` and `[[note|aliases]]` resolve against the vault
- `![[note]]` embeds link the note; `![[image.png]]` embeds render the image
- `> [!note]` callouts render with titled boxes
- `^block-ids` are hidden anchors, not trailing syntax
- Daily note (`⌘⇧D`) creates/opens `YYYY-MM-DD.md` at vault root

## Getting started

```bash
cargo run
```

Release build:

```bash
cargo build --release
```

The app restores the last opened folder on launch. Settings live in
`~/Library/Application Support/no.ogard.rista/settings.json` on macOS
(the platform equivalents on Linux/Windows via the `directories` crate).

## Keyboard shortcuts

| Action | macOS | Linux / Windows |
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
| Open folder | `⌘O` | `Ctrl+O` |
| Zen mode | `⌘⇧Enter` | `Ctrl+Shift+Enter` |

## Project structure

```
rista/
├── src/
│   ├── main.rs            # App bootstrap, keymap, menus, window
│   ├── app.rs             # Workspace: sidebar, tabs, palette, dialogs, actions
│   ├── vault.rs           # Vault model: file index, tree items, watcher (notify)
│   ├── document.rs        # Document: EditorState, autosave, preview sync
│   ├── preview.rs         # Markdown preview + Obsidian preprocessing
│   ├── search.rs          # Project search dialog
│   ├── settings.rs        # Settings model + persistence
│   ├── settings_panel.rs  # Settings sheet UI
│   ├── theme.rs           # Rísta Night / Rísta Day theme packs
│   └── actions.rs         # GPUI action definitions (shortcuts)
├── design/                # Framework-agnostic design tokens (JSON/CSS/TS)
├── public/                # App icon + brand assets
├── Cargo.toml
└── DESIGN.md              # Design system reference
```

## Design

See [DESIGN.md](./DESIGN.md) for the design system — color, type, spacing, and
component tokens, exported in framework-agnostic form under `design/`.
