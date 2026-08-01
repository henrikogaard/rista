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
  <code>Tauri</code> · <code>CodeMirror 6</code> · <code>Toast UI Editor</code> · <code>unified</code> · <code>notify</code>
</p>

---

## Overview

Rísta is a small, fast desktop editor for people who want Markdown files to stay as files.
There is no cloud sync, no account system, and no telemetry layer between you and your notes.

The interface is compact and dark by default. The name references runes, but the product stays restrained: carved geometry, mineral color, terse language, and low-noise local writing tools.

## Highlights

- Local-first folder workflow for Markdown projects
- Multi-tab editing with single-pane and dual-pane workspaces
- Markdown, split preview, preview-only, and WYSIWYG writing modes
- Auto-save while you write
- Find and replace with keyboard shortcut support
- Command dialogs for links, images, and tables
- Image paste from clipboard directly into the document
- Light and dark themes
- Settings panel for typography, layout, and visual atmosphere
- File watching for live updates when Markdown files change on disk

## Obsidian Compatibility

Rísta keeps Obsidian-style Markdown as plain text on disk while improving preview behavior for common vault syntax:

- YAML properties round-trip and can be hidden in rendered modes
- Wikilinks, aliases, note embeds, tags, and local attachments work with existing vault files
- Obsidian callouts render in preview, including open and closed fold markers
- Image attachment embeds such as `![[image.png|Alt text]]` render as local images
- Obsidian block references such as `^block-id` become hidden anchors instead of visible trailing syntax

Unsupported Obsidian-specific syntax remains editable as Markdown and falls back to readable text.

## Getting Started

```bash
npm install
npm run dev
```

Production build:

```bash
npm run package
```

## Scripts

| Command | Description |
|---|---|
| `npm run dev` | Build the renderer and launch Tauri |
| `npm run watch` | Rebuild renderer assets on file changes |
| `npm run build` | Create production renderer assets in `dist/app` |
| `npm run package` | Build the desktop app with Tauri |
| `npm run version:check` | Validate semantic version formatting |
| `npm run version:current` | Print the current version |
| `npm run version:set` | Set an explicit version |
| `npm run version:patch` | Bump the patch version |
| `npm run version:minor` | Bump the minor version |
| `npm run version:major` | Bump the major version |

## Keyboard Shortcuts

| Action | macOS | Windows/Linux |
|---|---|---|
| Save | `Cmd+S` | `Ctrl+S` |
| Save as | `Cmd+Shift+S` | `Ctrl+Shift+S` |
| New file | `Cmd+N` | `Ctrl+N` |
| Toggle sidebar | `Cmd+B` | `Ctrl+B` |
| Toggle toolbar | `Cmd+\` | `Ctrl+\` |
| Find and replace | `Cmd+F` | `Ctrl+F` |
| Open settings | `Cmd+,` | `Ctrl+,` |

## Stack

- Tauri 2 for the desktop shell and native integration
- Rust commands for filesystem, watcher, terminal, D2, export, and AI transport
- CodeMirror 6 for the Markdown editing surface
- Toast UI Editor for WYSIWYG editing
- unified, remark-gfm, remark-rehype, and rehype-stringify for Markdown rendering
- esbuild for bundling

## Project Structure

```text
rista/
├── public/
│   ├── icon.svg
│   └── index.html
├── src/
│   └── renderer/
│       ├── editor.js
│       ├── index.js
│       ├── markdown.js
│       ├── settings.js
│       ├── tauri-api.js
│       ├── theme.js
│       └── styles/
│           └── main.css
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       └── main.rs
├── dist/
├── package.json
└── README.md
```

## Design Notes

- Borderless toolbar controls are intentional and part of the visual identity
- Theme colors live in CSS variables and are switched via `[data-theme]`
- The layout is compact by default and optimized for writing density
- UI chrome stays flat; atmospheric treatments belong to the welcome surface, not the app shell
- Norse influence stays abstract and professional rather than decorative

## Current Tauri Notes

Rísta exports to PDF via its own Tauri backend (generates HTML from the preview pipeline, writes to a temp file, and opens it in the system browser). HTML, DOCX, and static-site export are also implemented.

## License

MIT
