# Fjordmark

A local-first Markdown editor. Nordic. Fast. Yours.

Open a folder. Start writing. No cloud, no accounts, no telemetry.

---

## Stack

- **Electron** — native window, file system access
- **CodeMirror 6** — editor with Markdown syntax highlighting
- **unified / remark** — Markdown → HTML pipeline
- **Chokidar** — file system watcher for live sync

## Getting started

```bash
npm install
npm run dev
```

## Scripts

| Command | Description |
|---|---|
| `npm run dev` | Start in development mode with live reload |
| `npm run build` | Bundle renderer for production |
| `npm start` | Start without rebuilding |
| `npm run package` | Build distributable (mac/win/linux) |

## Keyboard shortcuts

| Action | Shortcut |
|---|---|
| Save | `⌘S` / `Ctrl+S` |
| Toggle sidebar | `⌘B` / `Ctrl+B` |
| Toggle toolbar | `⌘\` / `Ctrl+\` |

## Features

- Split edit / preview
- Light and dark mode (persisted)
- Hide/show toolbar — for distraction-free writing
- Statistics popover (words, characters, paragraphs, read time)
- Table of contents popover
- Auto-save on idle
- File tree with folder collapse
- Multi-tab editing
- macOS native titlebar

## License

MIT
