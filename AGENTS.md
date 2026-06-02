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

## What is Fjordmark

A **local-first, open-source Markdown editor** built with Electron.
The design language is Nordic/Scandinavian — dark, minimal, purposeful.
Inspired by AuraDocs, Panda writer, and TipTap.

Users open a folder. All `.md` files in that folder become a "project".
No cloud. No accounts. No telemetry. Files are just files.

---

## Design principles

- **Borderless buttons** — toolbar uses `<div>` not `<button>`. Never revert this. Browser default button styles break the aesthetic badly.
- **Two themes** — dark (default) and light, toggled via `[data-theme]` on `<html>`. All colors are CSS variables. Never hardcode hex values in component code.
- **Compact density** — inspired by AuraDocs. Tight spacing, small type (10–13px), nothing wastes vertical space.
- **Toolbar is hideable** — users who write plain Markdown can hide it permanently. Respect this.
- **No gradients in UI chrome** — only the welcome/aurora cover screen uses gradient. Surfaces are flat.

---

## Tech stack

| Layer | Technology |
|---|---|
| Shell | Electron 29 |
| Editor engine | CodeMirror 6 |
| Markdown pipeline | unified + remark-gfm + remark-rehype + rehype-stringify |
| File watching | Chokidar 3 |
| Bundler | esbuild (no React, no framework — vanilla JS) |
| Fonts | DM Sans + DM Mono (Google Fonts) |

**No React. No Vue. No framework.** The renderer is vanilla JS with direct DOM manipulation.
Keep it that way unless there is a very strong reason to change — the bundle stays small and fast.

---

## Project structure

```
fjordmark/
├── public/
│   └── index.html          # HTML shell, CSP header, loads fonts + CSS + dist/renderer.js
├── src/
│   ├── main/
│   │   ├── main.js         # Electron main process — window, IPC handlers, Chokidar
│   │   └── preload.js      # contextBridge — exposes window.fjord.* to renderer
│   └── renderer/
│       ├── index.js        # App entry — UI construction, state, event wiring
│       ├── editor.js       # CodeMirror 6 setup, highlight theme, createEditor()
│       ├── markdown.js     # unified pipeline, renderMarkdown(), getStats(), extractHeadings()
│       ├── theme.js        # initTheme(), toggleTheme(), persisted to localStorage
│       └── styles/
│           └── main.css    # All CSS — tokens, both themes, layout, components
├── dist/                   # esbuild output (gitignored)
├── package.json
└── README.md
```

---

## IPC API (window.fjord.*)

Defined in `preload.js`, implemented in `main.js`.

```js
window.fjord.openFolder()              // → string | null (folder path)
window.fjord.readFolder(path)          // → FileTree[]
window.fjord.readFile(path)            // → string (file content)
window.fjord.writeFile(path, content)  // → boolean
window.fjord.watchFolder(path)         // → boolean (starts Chokidar)
window.fjord.stat(path)                // → { mtime, size } | null
window.fjord.onFileChange(cb)          // → unsubscribe fn — cb({ event, path })
```

`FileTree` node shape:
```ts
{ type: 'file' | 'folder', name: string, path: string, children?: FileTree[] }
```

---

## App state (src/renderer/index.js)

```js
const state = {
  folderPath: null,       // string | null
  tree: [],               // FileTree[]
  tabs: [],               // Tab[]
  activeTab: null,        // Tab | null
  view: 'split',          // 'edit' | 'split' | 'preview'
  toolbarVisible: true,
  sidebarVisible: true,
  activePopover: null,    // 'stats' | 'toc' | null
}
```

Tab shape:
```ts
{ path: string, name: string, content: string, dirty: boolean }
```

Auto-save fires 800ms after the last keystroke via a debounced `saveActive()`.

---

## CSS architecture

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
| Toggle sidebar | `⌘B` | `Ctrl+B` |
| Toggle toolbar | `⌘\` | `Ctrl+\` |

Add new shortcuts in the `keydown` listener at the bottom of `index.js`.

---

## What is NOT yet built (priority order)

### 1. Aurora welcome screen
The empty state / no-folder-open screen should have a gradient mesh header (like AuraDocs — teal/purple/green aurora waves) above the "Open folder" CTA. Currently it's just text. This is purely CSS/SVG, no logic needed.

### 2. WYSIWYG mode
Currently only raw Markdown editing exists. A WYSIWYG toggle should switch the editor pane from CodeMirror to a rich-text view. **Recommended approach:** use a hidden CodeMirror instance as the source of truth, and render a ProseMirror or `contenteditable` WYSIWYG on top. Sync on blur/change.

### 3. Image paste from clipboard
Wire up `paste` event in the CM host div. If `clipboardData` contains an image:
1. Convert to base64 or save to a local `_assets/` subfolder next to the markdown file
2. Insert `![image](path)` at cursor
For local files, saving to `_assets/` alongside the `.md` is more portable than base64.

### 4. Command palette (⌘K)
A floating search/command input that:
- Lists all files in the current folder for quick-open
- Lists editor commands (toggle view, insert table, etc.)
- Fuzzy search across both
Trigger: `⌘K` / `Ctrl+K`. Dismiss: `Escape`.

### 5. Find & replace (⌘F)
A bar that slides in above the editor (not a modal). Wire into CodeMirror's built-in search extension: `@codemirror/search` — `openSearchPanel`, `closeSearchPanel`.

### 6. Vim mode
Optional. `@codemirror/vim` can be added as an extension to the CodeMirror instance in `editor.js`. Should be toggled in settings, persisted to localStorage.

### 7. Settings panel
A slide-in panel (not a new window) for:
- Font size (editor)
- Vim mode toggle
- Auto-save delay
- Default view (edit/split/preview)
- Spellcheck toggle
Persist all settings to `localStorage`.

### 8. Export to PDF
Use Electron's `webContents.printToPDF()` from the main process. Add IPC handler `export:pdf`. Trigger from a menu item or toolbar button. Print the preview pane HTML, not the raw editor.

---

## Known issues / technical debt

- `editorCmd('undo'/'redo')` uses dynamic `import()` which is slightly awkward. Better to import the commands statically at the top of `index.js` and call them directly.
- The CodeMirror theme uses `dark: true` which is hardcoded. When light mode is active, CodeMirror needs a separate theme object. Create `fjordThemeLight` in `editor.js` and swap it via a `Compartment` when the theme toggles.
- Popover positioning is `position: absolute` inside the toolbar's right group. This works for the current layout but will clip if the window is very narrow. Consider a proper popover library or `@floating-ui/dom` if this becomes an issue.
- There is no empty-tab-bar state UI. If all tabs are closed, the editor area is blank. Should show the welcome screen again.

---

## Running locally

```bash
npm install
npm run dev      # starts esbuild watcher + Electron
```

For production build:
```bash
npm run package  # outputs to release/ via electron-builder
```

---

## Contribution conventions

- **Commits**: conventional commits — `feat:`, `fix:`, `style:`, `refactor:`
- **No TypeScript** for now — keep the DX simple, add JSDoc types if needed
- **No CSS-in-JS** — all styles go in `main.css`
- **File naming**: kebab-case for all files
