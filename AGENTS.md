# AGENTS.md — Fjordmark

Handoff document for Codex. Read this before touching any file.

---

## Agent operating rules

These rules apply before any code change.

- **Think before coding** — state assumptions when the request is ambiguous. If there are multiple reasonable interpretations, name them before choosing. Ask when the wrong assumption would cause rework.
- **Simplicity first** — implement the smallest change that solves the request. Do not add speculative abstractions, configurability, or extra features.
- **Surgical changes** — touch only files and lines needed for the task. Match the existing style. Do not refactor adjacent code, reformat unrelated sections, or delete pre-existing dead code unless explicitly asked.
- **Clean up your own trail** — remove imports, variables, functions, CSS, or tests made obsolete by your own change. Leave unrelated existing cleanup as a note, not an edit.
- **Verify the goal** — for each non-trivial task, define what proves success before or while implementing. Prefer tests for logic changes, builds for integration changes, and rendered checks for UI changes.
- **Every changed line must justify itself** — if a line cannot be traced back to the user request, the repo’s conventions, or required verification, do not change it.

---

## What is Rísta

A **local-first, open-source Markdown editor** built with Tauri.
The design language is Nordic/Scandinavian — dark, minimal, purposeful.

Users open a folder. All `.md` files in that folder become a "project".
No cloud. No accounts. No telemetry. Files are just files.

---

## Design principles

- **Borderless buttons** — toolbar uses `<div>` not `<button>`. Never revert this.
- **Two themes** — dark (default) and light, toggled via `[data-theme]` on `<html>`. All colors are CSS variables.
- **Compact density** — Tight spacing, small type (11–13px), nothing wastes vertical space.
- **Toolbar is hideable** — users who write plain Markdown can hide it permanently.
- **No gradients in UI chrome** — only the welcome surface uses gradient.
- **Core by default, all else opt-in** — writing features come first. Non-core modules (graph, agents, calendar, etc.) are off by default behind the Experimental feature flags in Settings.

---

## Tech stack

| Layer | Technology |
|---|---|
| Shell | Tauri 2 |
| Editor engine | CodeMirror 6 + Toast UI Editor (WYSIWYG) |
| Markdown pipeline | unified + remark-gfm + remark-rehype + rehype-stringify |
| File watching | Rust notify crate |
| Bundler | esbuild (no React, no framework — vanilla JS) |
| Fonts | DM Sans + DM Mono (self-hosted via @fontsource) |

**No React. No Vue. No framework.** The renderer is vanilla JS with direct DOM manipulation.
Keep it that way unless there is a very strong reason to change.

---

## Project structure

```
rista/
├── public/
│   ├── index.html          # HTML shell, CSP header, loads fonts + CSS + dist/app/renderer.js
│   └── fonts.css           # Self-hosted @fontsource imports
├── src/
│   └── renderer/
│       ├── index.js        # App entry — UI construction, state, event wiring, panel registration
│       ├── workspace.js    # Pane layout, toolbar, split mode, editor mounting
│       ├── shell.js        # App shell HTML builder, settings panel, statusbar, welcome screen
│       ├── editor.js       # CodeMirror 6 setup, themes, plugins (focus, typewriter, live preview, POS)
│       ├── command-palette.js  # Cmd-K fuzzy search over files + commands
│       ├── markdown.js     # unified pipeline, renderMarkdown(), getStats(), extractHeadings()
│       ├── preview.js      # Preview rendering, PDF/HTML export, image paste
│       ├── settings.js     # Settings model, defaults, validation, feature flags (#64)
│       ├── theme.js        # Dark/light theme management, persisted to localStorage
│       ├── state.js        # Central app state, pane helpers, tab helpers
│       ├── tauri-api.js    # window.fjord.* bridge to Tauri Rust commands
│       ├── tabs.js         # Tab management, auto-save, file switching
│       ├── right-panel.js  # Right sidebar widget system registration
│       ├── commands.js     # Editor commands (wraps CodeMirror + WYSIWYG actions)
│       └── styles/
│           └── main.css    # All CSS — tokens, both themes, layout, all components
├── src-tauri/
│   ├── src/main.rs         # Rust backend — file ops, watcher, export, terminal, commands
│   ├── Cargo.toml
│   └── tauri.conf.json
├── dist/                   # esbuild output (gitignored)
├── tests/                  # Node test files
├── package.json
└── README.md
```

---

## IPC API (window.fjord.*)

Defined in `src/renderer/tauri-api.js`, calls Tauri `invoke` to Rust commands.

```js
window.fjord.openFolder()              // → string | null (folder path)
window.fjord.readFolder(path)          // → FileTree[]
window.fjord.readFile(path)            // → string (file content)
window.fjord.writeFile(path, content)  // → boolean
window.fjord.watchFolder(path)         // → boolean (starts Rust watcher)
window.fjord.stat(path)                // → { mtime, size } | null
window.fjord.onFileChange(cb)          // → unsubscribe fn
window.fjord.exportPdf(payload)        // → bool (writes HTML to temp file, opens in browser)
window.fjord.exportHtml(payload)       // → bool (saves standalone HTML)
window.fjord.saveDocx(base64, name)    // → bool
```

---

## Feature flags / survivor set

The app distinguishes **core** (on by default) and **non-core** (off by default) modules.
All switches live in Settings → Experimental.

**Core (on by default):** Editor, file tree, preview, command palette, find/replace, export (HTML/DOCX), themes, settings, outline, properties.

**Non-core (off by default):** Graph view, calendar, bookmarks, tags, AI agents, inspector, wikilink index, semantic index, wiki quality, related notes, diagram builder, terminal, publish.

Gate variable in settings.js: each has a `feature*` boolean. Panels are wrapped with `_featureEnabled()` guards in index.js.
The `showExperimental` master toggle must be on for any non-core module to appear.

---

Everything lives in `src/renderer/styles/main.css`.

**Theme tokens** — defined on `:root` (dark) and `[data-theme="light"]`:
- `--bg0` through `--bg5` — background layers (bg0 = deepest)
- `--border`, `--border2`, `--border3` — border opacities
- `--text1`, `--text2`, `--text3` — text hierarchy
- `--accent`, `--accent-hi`, `--accent-dim` — interactive blue-gray
- `--green`, `--red`, `--amber` — semantic
- `--shadow-pop`, `--shadow-sm` — elevation

**Never hardcode a color in JS or inline styles.** Always use a CSS variable.

---


## Keyboard shortcuts

| Action | Mac | Windows/Linux |
|---|---|---|
| Save | `⌘S` | `Ctrl+S` |
| Save as | `⌘⇧S` | `Ctrl+Shift+S` |
| Toggle sidebar | `⌘B` | `Ctrl+B` |
| Toggle toolbar | `⌘\` | `Ctrl+\` |
| Find & replace | `⌘F` | `Ctrl+F` |
| Project search | `⌘⇧F` | `Ctrl+Shift+F` |
| Command palette | `⌘K` | `Ctrl+K` |
| Settings | `⌘,` | `Ctrl+,` |
| Terminal | `⌘J` | `Ctrl+J` |
| Daily note | `⌘⇧D` | `Ctrl+Shift+D` |
| New file | `⌘N` | `Ctrl+N` |
| Open folder | `⌘O` | `Ctrl+O` |
| Zen mode | `⌘⇧Enter` | `Ctrl+Shift+Enter` |

## Running locally

```bash
npm install
npm run dev        # builds renderer + launches Tauri dev
npm run build      # production renderer build
npm run package    # packages the desktop app
npm test           # runs 68+ tests
```

## Contribution conventions

- **Commits**: conventional commits — `feat:`, `fix:`, `style:`, `refactor:`
- **No TypeScript** for now — keep the DX simple, add JSDoc types if needed
- **No CSS-in-JS** — all styles go in `main.css`
- **File naming**: kebab-case for all files
