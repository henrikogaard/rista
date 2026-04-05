# Fjordmark

<p align="center">
  <img src="./public/icon.svg" width="132" height="132" alt="Fjordmark logo" />
</p>

<p align="center">
  <strong>A local-first Markdown editor with a Nordic, low-noise interface.</strong>
</p>

<p align="center">
  Open a folder. Write in plain Markdown. Keep your files on disk, under your control.
</p>

<p align="center">
  <code>Electron</code> · <code>CodeMirror 6</code> · <code>Toast UI Editor</code> · <code>unified</code> · <code>Chokidar</code>
</p>

---

## Overview

Fjordmark is a small, fast desktop editor for people who want Markdown files to stay as files.
There is no cloud sync, no account system, and no telemetry layer between you and your notes.

The interface is intentionally compact and dark by default, with a Scandinavian-inspired visual language:
flat surfaces, restrained color, borderless controls, and as little chrome as possible.

## Highlights

- Local-first folder workflow for Markdown projects
- Multi-tab editing with single-pane and dual-pane workspaces
- Markdown, split preview, preview-only, and WYSIWYG writing modes
- Auto-save while you write
- Find and replace with keyboard shortcut support
- Command dialogs for links, images, and tables
- Image paste from clipboard directly into the document
- PDF export from the desktop app
- Light and dark themes
- Settings panel for typography, layout, and visual atmosphere
- File watching for live updates when Markdown files change on disk

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
| `npm run dev` | Start esbuild in watch mode and launch Electron |
| `npm run watch` | Rebuild the renderer on file changes |
| `npm run build` | Create a production renderer bundle |
| `npm start` | Launch Electron without rebuilding |
| `npm run package` | Build the app for distribution with `electron-builder` |
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

- Electron 29 for the desktop shell and native integration
- CodeMirror 6 for the Markdown editing surface
- Toast UI Editor for WYSIWYG editing
- unified, remark-gfm, remark-rehype, and rehype-stringify for Markdown rendering
- Chokidar 3 for folder watching
- esbuild for bundling

## Project Structure

```text
fjordmark/
├── public/
│   ├── icon.svg
│   └── index.html
├── src/
│   ├── main/
│   │   ├── main.js
│   │   └── preload.js
│   └── renderer/
│       ├── editor.js
│       ├── index.js
│       ├── markdown.js
│       ├── settings.js
│       ├── theme.js
│       └── styles/
│           └── main.css
├── dist/
├── package.json
└── README.md
```

## Design Notes

- Borderless toolbar controls are intentional and part of the visual identity
- Theme colors live in CSS variables and are switched via `[data-theme]`
- The layout is compact by default and optimized for writing density
- UI chrome stays flat; the aurora treatment belongs to the welcome atmosphere, not the app shell

## License

MIT
