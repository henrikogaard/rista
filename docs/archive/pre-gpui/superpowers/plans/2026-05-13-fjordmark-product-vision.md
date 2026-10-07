> **Historical, pre-GPUI document.** Preserved as an archive; paths, commands, feature descriptions, and plans below are not current product guidance. See the [active documentation index](../../../../README.md).

# Fjordmark Product Vision — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the full Fjordmark product vision across 9 phases — stabilization, core editing, file management, preview upgrades, writing tools, power user features, import/export, settings expansion, and platform polish.

**Architecture:** Fjordmark is a vanilla JS Electron app using CodeMirror 6, unified/remark for Markdown, and direct DOM manipulation. All styles live in `main.css` using CSS variables. State is a shared module-level object. The renderer is already split into focused modules (state, shell, workspace, tabs, commands, editor, preview, find-replace, diagrams, tree-view, theme, settings, icons). New features add new modules or extend existing ones. No frameworks.

**Tech Stack:** Electron 29, CodeMirror 6, unified/remark/rehype, esbuild, vanilla JS, CSS variables

**Spec:** `docs/superpowers/specs/2026-05-13-fjordmark-product-vision-design.md`

---

## File Map

### Existing files to modify

| File | Modifications |
|------|--------------|
| `src/renderer/index.js` | Import new modules, register callbacks, add keyboard shortcuts |
| `src/renderer/state.js` | Add new state fields (zenMode, pinnedTabs, recentProjects, wordGoals, etc.) |
| `src/renderer/shell.js` | Add settings sections, context menu support |
| `src/renderer/workspace.js` | Zen mode layout, typewriter scrolling integration |
| `src/renderer/tabs.js` | Pinned tabs, tab context menu, session restore |
| `src/renderer/commands.js` | New commands for command palette registration |
| `src/renderer/editor.js` | Vim mode compartment, spellcheck compartment, typewriter extension, multiple cursors |
| `src/renderer/markdown.js` | Add remark-math, remark-footnotes, remark-emoji plugins |
| `src/renderer/preview.js` | Reading time in status bar, expanded stats |
| `src/renderer/settings.js` | New settings keys (autoSaveDelay, vimMode, spellcheck, etc.) |
| `src/renderer/styles/main.css` | Styles for command palette, zen mode, context menus, etc. |
| `src/main/main.js` | New IPC handlers (createDir, writeImageFile, trashFile, renameFile, etc.), export HTML, menu updates |
| `src/main/preload.js` | Expose new IPC methods |
| `package.json` | New dependencies |
| `public/index.html` | KaTeX CSS link |

### New files to create

| File | Purpose |
|------|---------|
| `src/renderer/command-palette.js` | Command palette UI, fuzzy search, command registry |
| `src/renderer/zen-mode.js` | Zen/focus mode toggle, paragraph dimming |
| `src/renderer/context-menu.js` | Right-click context menus for sidebar and tabs |
| `src/renderer/recent-projects.js` | Recent projects persistence and welcome screen integration |
| `src/renderer/word-goals.js` | Word/session goal tracking |
| `src/renderer/session-restore.js` | Session state persistence and restore |

---

## Phase 0 — Stabilize

> **Note:** The spec lists "Split index.js into modules" as Phase 0.2. This is **already done** — the codebase is already split into focused modules (state.js, shell.js, workspace.js, tabs.js, commands.js, editor.js, preview.js, find-replace.js, diagrams.js, tree-view.js, theme.js, settings.js, icons.js). The AGENTS.md was outdated. Skip this task.

### Task 0.1: Fix WYSIWYG reliability

This task requires a hands-on QA audit. The Toast UI Editor integration has known mount/sync issues. Rather than prescribing specific code changes up front, this task is structured as a diagnostic-then-fix workflow.

**Files:**
- Modify: `src/renderer/workspace.js` (ensureRichEditorMounted, lines 307-348)
- Modify: `src/renderer/commands.js` (syncToWysiwyg, lines 353-361)
- Modify: `src/renderer/tabs.js` (onRichEditorChange, lines 352-370; syncTabRepresentations, lines 372-382)
- Reference: `docs/TEST-CASES.md`

- [ ] **Step 1: Run the app and document WYSIWYG failures**

Start the dev server and manually test each scenario from `docs/TEST-CASES.md` that involves WYSIWYG mode. For each failure, note: what happened, what was expected, which pane, and which view transition triggered it.

Run: `npm run dev`

Document findings in a scratch file. Common expected failure modes:
- WYSIWYG not mounting when switching from markdown to WYSIWYG view
- Content not syncing from CM to WYSIWYG on tab switch
- Content lost when switching back from WYSIWYG to markdown
- Theme mismatch in WYSIWYG pane

- [ ] **Step 2: Audit the mount lifecycle in workspace.js**

Read `ensureRichEditorMounted` (workspace.js:307-348). Verify:
1. The mount target element exists in the DOM before mounting
2. The Toast UI Editor instance is properly destroyed before re-mounting
3. The `syncingRichEditor` flag prevents echo loops during content sync
4. The editor is initialized with the correct content from the active tab

Fix any issues found. Common patterns to watch for:
- Race condition: mount called before DOM element is ready
- Stale reference: `richEditorMountTarget[pane]` pointing to a removed DOM node
- Missing null checks on `richEditors[pane]` before calling methods

- [ ] **Step 3: Audit content sync in commands.js and tabs.js**

Read `syncToWysiwyg` (commands.js:353-361), `onRichEditorChange` (tabs.js:352-370), and `syncTabRepresentations` (tabs.js:372-382). Verify:
1. The `syncingRichEditor` guard prevents infinite loops (CM change -> WYSIWYG update -> CM change)
2. `getMarkdown()` and `setMarkdown()` are called on valid editor instances
3. Tab content is updated from the correct source based on which editor the user is typing in

Fix any issues found.

- [ ] **Step 4: Test all WYSIWYG scenarios again**

Re-run the WYSIWYG test cases from Step 1. Verify all previously failing scenarios now pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "fix: stabilize WYSIWYG mount lifecycle and content sync"
```

---

### Task 0.2: Image paste to `_assets/` folder

Replace base64 data URL insertion with saving images as files.

**Files:**
- Modify: `src/main/main.js` (add IPC handlers after line 339)
- Modify: `src/main/preload.js` (expose new methods)
- Modify: `src/renderer/preview.js` (rewrite handleImagePaste, lines 117-140)

- [ ] **Step 1: Add IPC handlers in main.js**

Add these handlers after the `fs:stat` handler (line 339) in `src/main/main.js`:

```js
// ── IPC: Create directory ─────────────────────────────────────────
ipcMain.handle('fs:createDir', async (_, dirPath) => {
  try {
    fs.mkdirSync(dirPath, { recursive: true })
    return true
  } catch {
    return false
  }
})

// ── IPC: Write image file ─────────────────────────────────────────
ipcMain.handle('fs:writeImageFile', async (_, dirPath, base64Data, fileName) => {
  try {
    fs.mkdirSync(dirPath, { recursive: true })
    const filePath = path.join(dirPath, fileName)
    const buffer = Buffer.from(base64Data, 'base64')
    fs.writeFileSync(filePath, buffer)
    return { path: filePath, name: fileName }
  } catch (err) {
    console.error('Image write error:', err)
    return null
  }
})
```

- [ ] **Step 2: Expose new IPC methods in preload.js**

Add to the `window.fjord` object in `src/main/preload.js`, inside the "File system" section (after line 18):

```js
  createDir: (p) => ipcRenderer.invoke('fs:createDir', p),
  writeImageFile: (dirPath, base64Data, fileName) => ipcRenderer.invoke('fs:writeImageFile', dirPath, base64Data, fileName),
```

- [ ] **Step 3: Rewrite handleImagePaste in preview.js**

Replace the entire `handleImagePaste` function (preview.js:117-140) with:

```js
export async function handleImagePaste(file, view, pane = state.focusedPane, onEditorChange) {
  const tab = getTabForPane(pane)
  if (!tab) return

  const reader = new FileReader()
  reader.onload = async (e) => {
    const base64Full = e.target.result
    const base64Data = base64Full.split(',')[1]
    const ext = file.type.split('/')[1] || 'png'
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
    const fileName = `image-${timestamp}.${ext}`

    // Determine _assets/ directory next to the current .md file
    const filePath = tab.path
    if (!filePath) return

    const lastSlash = filePath.lastIndexOf('/') !== -1 ? filePath.lastIndexOf('/') : filePath.lastIndexOf('\\')
    const dirPath = filePath.substring(0, lastSlash)
    const assetsDir = dirPath + '/_assets'

    const result = await window.fjord.writeImageFile(assetsDir, base64Data, fileName)
    if (!result) return

    const relativePath = '_assets/' + fileName
    const markdown = `![${fileName}](${relativePath})`
    const pos = view.state.selection.main.head

    view.dispatch({
      changes: { from: pos, to: pos, insert: markdown },
      selection: { anchor: pos + markdown.length },
    })

    onEditorChange(pane, view.state.doc.toString())
  }
  reader.readAsDataURL(file)
}
```

- [ ] **Step 4: Test image paste**

Run: `npm run dev`

1. Open a folder with at least one `.md` file
2. Copy an image to clipboard
3. Paste in the editor
4. Verify: an `_assets/` folder was created next to the `.md` file
5. Verify: the image file exists in `_assets/` with a timestamped name
6. Verify: the editor contains `![image-...](\_assets/image-...)` not a base64 URL
7. Verify: the preview renders the image correctly

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "fix: save pasted images to _assets/ folder instead of base64"
```

---

## Phase 1 — Core Editing Polish

### Task 1.1: Command palette

**Files:**
- Create: `src/renderer/command-palette.js`
- Modify: `src/renderer/index.js` (import + keyboard shortcut)
- Modify: `src/renderer/state.js` (add commandPaletteOpen state field)
- Modify: `src/renderer/styles/main.css` (add command palette styles)

- [ ] **Step 1: Add state field**

In `src/renderer/state.js`, add to the state object (after line 50, before the closing `}`):

```js
  commandPaletteOpen: false,
```

- [ ] **Step 2: Create command-palette.js**

Create `src/renderer/command-palette.js`:

```js
import { $, el, state } from './state.js'

let callbacks = {}
let commandRegistry = []
let selectedIndex = 0
let filteredResults = []

export function registerCommandPaletteCallbacks(cbs) {
  callbacks = cbs
}

export function registerCommand(id, label, description, shortcut, action) {
  commandRegistry.push({ id, type: 'command', label, description, shortcut, action })
}

export function registerCommands(commands) {
  for (const cmd of commands) {
    commandRegistry.push({ ...cmd, type: 'command' })
  }
}

function getFileResults(query) {
  if (!state.tree.length) return []
  const files = flattenTree(state.tree)
  if (!query) return files.slice(0, 10)
  return fuzzyFilter(files, query, item => item.name).slice(0, 10)
}

function flattenTree(tree, results = []) {
  for (const node of tree) {
    if (node.type === 'file') {
      results.push(node)
    } else if (node.children) {
      flattenTree(node.children, results)
    }
  }
  return results
}

function getCommandResults(query) {
  if (!query) return commandRegistry.slice(0, 10)
  return fuzzyFilter(commandRegistry, query, item => item.label).slice(0, 10)
}

function fuzzyFilter(items, query, getText) {
  const lower = query.toLowerCase()
  return items
    .map(item => {
      const text = getText(item).toLowerCase()
      let score = 0
      let qi = 0
      for (let ti = 0; ti < text.length && qi < lower.length; ti++) {
        if (text[ti] === lower[qi]) {
          score += (ti === 0 || text[ti - 1] === '/' || text[ti - 1] === ' ') ? 2 : 1
          qi++
        }
      }
      return qi === lower.length ? { item, score } : null
    })
    .filter(Boolean)
    .sort((a, b) => b.score - a.score)
    .map(r => r.item)
}

function buildPaletteHtml() {
  return `
    <div class="command-palette-overlay" id="command-palette-overlay">
      <div class="command-palette" id="command-palette">
        <input type="text" class="command-palette-input" id="command-palette-input"
               placeholder="Search files or type > for commands..." autocomplete="off" spellcheck="false" />
        <div class="command-palette-results" id="command-palette-results"></div>
      </div>
    </div>
  `
}

function renderResults() {
  const container = $('command-palette-results')
  if (!container) return

  if (!filteredResults.length) {
    container.innerHTML = '<div class="command-palette-empty">No results</div>'
    return
  }

  container.innerHTML = filteredResults.map((item, i) => {
    const isActive = i === selectedIndex
    const icon = item.type === 'command' ? '>' : '#'
    const shortcut = item.shortcut ? `<span class="command-palette-shortcut">${item.shortcut}</span>` : ''
    const desc = item.description || item.path || ''
    return `
      <div class="command-palette-item${isActive ? ' active' : ''}" data-index="${i}">
        <span class="command-palette-icon">${icon}</span>
        <span class="command-palette-label">${item.label || item.name}</span>
        <span class="command-palette-desc">${desc}</span>
        ${shortcut}
      </div>
    `
  }).join('')
}

function updateResults(query) {
  const isCommand = query.startsWith('>')
  if (isCommand) {
    filteredResults = getCommandResults(query.slice(1).trim())
  } else {
    filteredResults = [
      ...getFileResults(query),
      ...(query ? [] : getCommandResults('')),
    ]
  }
  selectedIndex = 0
  renderResults()
}

function selectItem(index) {
  const item = filteredResults[index]
  if (!item) return
  closeCommandPalette()
  if (item.type === 'command' && item.action) {
    item.action()
  } else if (item.path && callbacks.openFile) {
    callbacks.openFile(item.path)
  }
}

export function openCommandPalette() {
  if (state.commandPaletteOpen) return
  state.commandPaletteOpen = true

  const root = $('root')
  if (!root) return

  const existing = $('command-palette-overlay')
  if (existing) existing.remove()

  root.insertAdjacentHTML('beforeend', buildPaletteHtml())

  const input = $('command-palette-input')
  const overlay = $('command-palette-overlay')
  const results = $('command-palette-results')

  updateResults('')

  input.addEventListener('input', () => {
    updateResults(input.value)
  })

  input.addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      selectedIndex = Math.min(selectedIndex + 1, filteredResults.length - 1)
      renderResults()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      selectedIndex = Math.max(selectedIndex - 1, 0)
      renderResults()
    } else if (e.key === 'Enter') {
      e.preventDefault()
      selectItem(selectedIndex)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      closeCommandPalette()
    }
  })

  results.addEventListener('click', (e) => {
    const item = e.target.closest('.command-palette-item')
    if (item) {
      selectItem(Number(item.dataset.index))
    }
  })

  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) closeCommandPalette()
  })

  requestAnimationFrame(() => input.focus())
}

export function closeCommandPalette() {
  state.commandPaletteOpen = false
  const overlay = $('command-palette-overlay')
  if (overlay) overlay.remove()
}

export function toggleCommandPalette() {
  if (state.commandPaletteOpen) {
    closeCommandPalette()
  } else {
    openCommandPalette()
  }
}
```

- [ ] **Step 3: Add command palette styles to main.css**

Append to the end of `src/renderer/styles/main.css`:

```css
/* ── Command Palette ──────────────────────────────────────────── */
.command-palette-overlay {
  position: fixed; inset: 0; z-index: 9999;
  background: rgba(0,0,0,0.45);
  display: flex; justify-content: center; padding-top: 18vh;
  backdrop-filter: blur(2px);
}
.command-palette {
  width: 520px; max-height: 400px;
  background: var(--bg1); border: 1px solid var(--border);
  display: flex; flex-direction: column;
  box-shadow: var(--shadow-pop);
  overflow: hidden;
}
.command-palette-input {
  width: 100%; padding: 12px 16px;
  background: transparent; border: none; border-bottom: 1px solid var(--border);
  color: var(--text1); font-family: var(--font); font-size: 14px;
  outline: none;
}
.command-palette-input::placeholder { color: var(--text3); }
.command-palette-results {
  overflow-y: auto; flex: 1;
  padding: 4px 0;
}
.command-palette-item {
  display: flex; align-items: center; gap: 8px;
  padding: 8px 16px; cursor: pointer;
  font-size: 13px; color: var(--text2);
}
.command-palette-item.active {
  background: var(--accent-dim); color: var(--text1);
}
.command-palette-item:hover:not(.active) {
  background: rgba(255,255,255,0.03);
}
.command-palette-icon {
  width: 16px; text-align: center;
  font-family: var(--mono); font-size: 11px; color: var(--text3);
  flex-shrink: 0;
}
.command-palette-label {
  flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  color: var(--text1);
}
.command-palette-desc {
  font-size: 11px; color: var(--text3);
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  max-width: 180px;
}
.command-palette-shortcut {
  font-size: 10px; color: var(--text3);
  padding: 2px 6px;
  background: rgba(255,255,255,0.05);
  flex-shrink: 0;
}
.command-palette-empty {
  padding: 24px 16px; text-align: center;
  color: var(--text3); font-size: 13px;
}
[data-theme="light"] .command-palette-overlay { background: rgba(0,0,0,0.25); }
[data-theme="light"] .command-palette { background: var(--bg1); }
[data-theme="light"] .command-palette-item:hover:not(.active) { background: rgba(0,0,0,0.03); }
[data-theme="light"] .command-palette-shortcut { background: rgba(0,0,0,0.05); }
```

- [ ] **Step 4: Wire into index.js**

Add imports at the top of `src/renderer/index.js` (after line 8):

```js
import { toggleCommandPalette, closeCommandPalette, registerCommandPaletteCallbacks, registerCommands } from './command-palette.js'
```

Add callback registration (after the `registerTabCallbacks` block, around line 111):

```js
registerCommandPaletteCallbacks({
  openFile,
})
```

Register built-in commands (after `syncFolderUi()`, around line 115):

```js
registerCommands([
  { id: 'toggle-sidebar', label: 'Toggle Sidebar', shortcut: 'Cmd+B', action: toggleSidebar },
  { id: 'toggle-toolbar', label: 'Toggle Toolbar', shortcut: 'Cmd+\\', action: toggleToolbar },
  { id: 'toggle-theme', label: 'Toggle Theme', action: () => { const { toggleTheme } = require('./theme.js'); toggleTheme() } },
  { id: 'export-pdf', label: 'Export to PDF', shortcut: 'Cmd+E', action: () => { import('./preview.js').then(m => m.exportToPdf()) } },
  { id: 'toggle-settings', label: 'Open Settings', shortcut: 'Cmd+,', action: toggleSettingsPanel },
  { id: 'new-file', label: 'New File', shortcut: 'Cmd+N', action: createNewFile },
  { id: 'open-folder', label: 'Open Folder', action: openFolder },
  { id: 'find-replace', label: 'Find & Replace', shortcut: 'Cmd+F', action: toggleFindReplace },
])
```

Add keyboard shortcut in the `keydown` listener (in the handler block, after line 126):

```js
  if (mod && e.key === 'k') { e.preventDefault(); toggleCommandPalette() }
  if (e.key === 'Escape' && state.commandPaletteOpen) { e.preventDefault(); closeCommandPalette() }
```

- [ ] **Step 5: Test command palette**

Run: `npm run dev`

1. Press `Cmd+K` — palette opens with file list and commands
2. Type a filename — fuzzy-filters files
3. Type `>` — switches to command mode
4. Arrow keys navigate, Enter selects
5. Escape closes
6. Clicking the overlay closes
7. Selecting a file opens it in a tab
8. Selecting a command executes it

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add command palette (Cmd+K) with fuzzy file search and editor commands"
```

---

### Task 1.2: Zen / focus mode

**Files:**
- Create: `src/renderer/zen-mode.js`
- Modify: `src/renderer/state.js` (add zenMode state)
- Modify: `src/renderer/index.js` (import, shortcut, register command)
- Modify: `src/renderer/styles/main.css` (zen mode styles)

- [ ] **Step 1: Add state fields**

In `src/renderer/state.js`, add to the state object (after `commandPaletteOpen`):

```js
  zenMode: false,
```

- [ ] **Step 2: Create zen-mode.js**

Create `src/renderer/zen-mode.js`:

```js
import { $, state } from './state.js'
import { getSettings } from './settings.js'

let hintTimer = null

export function toggleZenMode() {
  if (state.zenMode) {
    exitZenMode()
  } else {
    enterZenMode()
  }
}

export function enterZenMode() {
  state.zenMode = true
  document.documentElement.setAttribute('data-zen', 'true')

  // Show exit hint on mouse movement
  const hint = $('zen-exit-hint')
  if (hint) hint.style.display = 'none'

  const handleMouseMove = () => {
    const h = $('zen-exit-hint')
    if (!h) return
    h.style.display = 'block'
    h.style.opacity = '1'
    clearTimeout(hintTimer)
    hintTimer = setTimeout(() => {
      h.style.opacity = '0'
      setTimeout(() => { h.style.display = 'none' }, 300)
    }, 2000)
  }

  document.addEventListener('mousemove', handleMouseMove)
  state._zenMouseHandler = handleMouseMove
}

export function exitZenMode() {
  state.zenMode = false
  document.documentElement.removeAttribute('data-zen')

  if (state._zenMouseHandler) {
    document.removeEventListener('mousemove', state._zenMouseHandler)
    state._zenMouseHandler = null
  }

  clearTimeout(hintTimer)
}

export function isZenMode() {
  return state.zenMode
}

export function buildZenExitHint() {
  const root = $('root')
  if (!root) return
  const existing = $('zen-exit-hint')
  if (existing) return
  const hint = document.createElement('div')
  hint.id = 'zen-exit-hint'
  hint.className = 'zen-exit-hint'
  hint.textContent = 'Esc to exit Zen Mode'
  hint.style.display = 'none'
  hint.addEventListener('click', exitZenMode)
  root.appendChild(hint)
}
```

- [ ] **Step 3: Add zen mode styles to main.css**

Append to `src/renderer/styles/main.css`:

```css
/* ── Zen Mode ─────────────────────────────────────────────────── */
[data-zen="true"] .titlebar,
[data-zen="true"] .sidebar,
[data-zen="true"] .sidebar-resizer,
[data-zen="true"] .tab-bar,
[data-zen="true"] .workspace-toolbar,
[data-zen="true"] .pane-label-row,
[data-zen="true"] .statusbar,
[data-zen="true"] .insights-panel,
[data-zen="true"] .find-replace-bar,
[data-zen="true"] .workspace-resizer,
[data-zen="true"] .pane-header {
  display: none !important;
}
[data-zen="true"] .editor-area {
  position: fixed; inset: 0; z-index: 100;
  display: flex; justify-content: center;
  background: var(--bg0);
}
[data-zen="true"] .pane {
  max-width: 700px; width: 100%;
}
[data-zen="true"] .pane.secondary-pane {
  display: none !important;
}
.zen-exit-hint {
  position: fixed; top: 12px; right: 16px; z-index: 200;
  padding: 6px 14px;
  background: var(--bg2); color: var(--text3);
  font-size: 11px; font-family: var(--font);
  cursor: pointer;
  opacity: 0; transition: opacity 0.3s ease;
}
.zen-exit-hint:hover { color: var(--text1); }
```

- [ ] **Step 4: Wire into index.js**

Add import at the top of `src/renderer/index.js`:

```js
import { toggleZenMode, exitZenMode, buildZenExitHint } from './zen-mode.js'
```

After `buildShell()` (around line 114), add:

```js
buildZenExitHint()
```

Add keyboard shortcut in the `keydown` listener:

```js
  if (mod && e.shiftKey && e.key === 'Enter') { e.preventDefault(); toggleZenMode() }
  if (e.key === 'Escape' && state.zenMode) { e.preventDefault(); exitZenMode() }
```

Register as a command for the palette (add to the `registerCommands` array):

```js
  { id: 'toggle-zen', label: 'Toggle Zen Mode', shortcut: 'Cmd+Shift+Enter', action: toggleZenMode },
```

- [ ] **Step 5: Test zen mode**

Run: `npm run dev`

1. Open a file, press `Cmd+Shift+Enter` — all chrome hides, editor centers at 700px
2. Move mouse — "Esc to exit Zen Mode" hint appears top-right, fades after 2s
3. Press Escape — returns to normal mode
4. In dual-pane, zen mode hides the secondary pane
5. Settings, sidebar, toolbar, tabs, status bar all hidden

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add zen/focus mode (Cmd+Shift+Enter)"
```

---

### Task 1.3: Typewriter scrolling

**Files:**
- Modify: `src/renderer/editor.js` (add typewriter extension)
- Modify: `src/renderer/settings.js` (add typewriterScrolling setting)
- Modify: `src/renderer/state.js` (add setting key reference)

- [ ] **Step 1: Add setting to settings.js**

In `src/renderer/settings.js`, add to `DEFAULT_SETTINGS` (after line 87):

```js
  typewriterScrolling: false,
```

Add `'typewriterScrolling'` is a boolean, so no change needed to `NUMERIC_KEYS`. Add to `sanitize()` function (after line 171):

```js
  next.typewriterScrolling = Boolean(next.typewriterScrolling)
```

- [ ] **Step 2: Add typewriter extension to editor.js**

In `src/renderer/editor.js`, add a new Compartment and extension after the `themeCompartment` (after line 69):

```js
export const typewriterCompartment = new Compartment()

const typewriterExtension = EditorView.updateListener.of(update => {
  if (!update.docChanged && !update.selectionSet) return
  const view = update.view
  const head = update.state.selection.main.head
  const coords = view.coordsAtPos(head)
  if (!coords) return
  const editorRect = view.dom.getBoundingClientRect()
  const targetY = editorRect.top + editorRect.height / 2
  const diff = coords.top - targetY
  if (Math.abs(diff) > 10) {
    view.scrollDOM.scrollBy({ top: diff, behavior: 'smooth' })
  }
})

const typewriterOff = []
```

In the `createEditor` function, add the typewriter compartment to the extensions array (inside the `EditorState.create` call, after the `themeCompartment.of(...)` line):

```js
      typewriterCompartment.of(isDark ? typewriterOff : typewriterOff),
```

Wait, that's wrong — typewriter mode is independent of theme. The initial value should come from settings. Update the `createEditor` function signature to accept `typewriterEnabled`:

In `src/renderer/editor.js`, change the `createEditor` function signature (line 71):

```js
export function createEditor({ parent, doc = '', onChange, onSelectionChange, onPaste, isDark = true, typewriterEnabled = false }) {
```

And add to the extensions array:

```js
      typewriterCompartment.of(typewriterEnabled ? typewriterExtension : typewriterOff),
```

Add an export function to toggle it:

```js
export function updateTypewriterMode(view, enabled) {
  view.dispatch({
    effects: typewriterCompartment.reconfigure(enabled ? typewriterExtension : typewriterOff),
  })
}
```

- [ ] **Step 3: Pass typewriter setting when creating editors**

In `src/renderer/workspace.js`, wherever `createEditor` is called (inside `mountEditor`), pass the typewriter setting. Read the `mountEditor` function and add `typewriterEnabled: getSettings().typewriterScrolling` to the options object.

- [ ] **Step 4: Test typewriter scrolling**

Run: `npm run dev`

1. Open settings, enable typewriter scrolling
2. Open a long document
3. Type at the bottom — the active line should stay vertically centered
4. Scroll manually — scrolling works normally
5. Type again — cursor re-centers

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add typewriter scrolling mode (optional, in settings)"
```

---

### Task 1.4: Drag-and-drop images

**Files:**
- Modify: `src/renderer/editor.js` (add drop event listener in createEditor)
- Modify: `src/renderer/preview.js` (reuse handleImagePaste logic)

- [ ] **Step 1: Add drop handler to editor.js**

In `src/renderer/editor.js`, after the paste event listener (after line 112), add a drop handler:

```js
  // Handle drag-and-drop images
  if (onPaste) {
    parent.addEventListener('dragover', (e) => {
      if (e.dataTransfer?.types?.includes('Files')) {
        e.preventDefault()
        parent.classList.add('drag-over')
      }
    })

    parent.addEventListener('dragleave', (e) => {
      parent.classList.remove('drag-over')
    })

    parent.addEventListener('drop', async (e) => {
      parent.classList.remove('drag-over')
      const files = e.dataTransfer?.files
      if (!files) return

      for (const file of files) {
        if (file.type.startsWith('image/')) {
          e.preventDefault()
          await onPaste(file, view)
        }
      }
    })
  }
```

- [ ] **Step 2: Add drag-over style to main.css**

Append to `src/renderer/styles/main.css`:

```css
/* ── Drag-and-drop image indicator ────────────────────────────── */
.cm-host.drag-over {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
```

- [ ] **Step 3: Test drag-and-drop**

Run: `npm run dev`

1. Open a markdown file
2. Drag an image from Finder onto the editor
3. Verify: blue outline appears during drag
4. Verify: image saved to `_assets/` folder
5. Verify: `![filename](_assets/filename)` inserted at drop position
6. Drop multiple images — each gets inserted with a blank line between

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add drag-and-drop image support (saves to _assets/)"
```

---

## Phase 2 — File & Project Management

### Task 2.1: Sidebar file operations (context menu)

**Files:**
- Create: `src/renderer/context-menu.js`
- Modify: `src/main/main.js` (add IPC handlers: renameFile, trashFile, duplicateFile, createFile)
- Modify: `src/main/preload.js` (expose new methods)
- Modify: `src/renderer/tabs.js` (wire context menu to tree items)
- Modify: `src/renderer/styles/main.css` (context menu styles)

- [ ] **Step 1: Add IPC handlers in main.js**

Add after the `fs:writeImageFile` handler in `src/main/main.js`:

```js
// ── IPC: Rename file ──────────────────────────────────────────────
ipcMain.handle('fs:renameFile', async (_, oldPath, newPath) => {
  try {
    fs.renameSync(oldPath, newPath)
    return true
  } catch {
    return false
  }
})

// ── IPC: Trash file ──────────────────────────────────────────────
ipcMain.handle('fs:trashFile', async (_, filePath) => {
  try {
    await shell.trashItem(filePath)
    return true
  } catch {
    return false
  }
})

// ── IPC: Duplicate file ──────────────────────────────────────────
ipcMain.handle('fs:duplicateFile', async (_, filePath) => {
  try {
    const ext = path.extname(filePath)
    const base = path.basename(filePath, ext)
    const dir = path.dirname(filePath)
    let copyPath = path.join(dir, `${base}-copy${ext}`)
    let counter = 1
    while (fs.existsSync(copyPath)) {
      counter++
      copyPath = path.join(dir, `${base}-copy-${counter}${ext}`)
    }
    fs.copyFileSync(filePath, copyPath)
    return { path: copyPath, name: path.basename(copyPath) }
  } catch {
    return null
  }
})

// ── IPC: Create file ─────────────────────────────────────────────
ipcMain.handle('fs:createFile', async (_, filePath) => {
  try {
    if (!fs.existsSync(filePath)) {
      fs.writeFileSync(filePath, '', 'utf-8')
    }
    return { path: filePath, name: path.basename(filePath) }
  } catch {
    return null
  }
})

// ── IPC: Reveal in Finder / Explorer ─────────────────────────────
ipcMain.handle('fs:showInFolder', async (_, filePath) => {
  shell.showItemInFolder(filePath)
  return true
})
```

- [ ] **Step 2: Expose in preload.js**

Add to `src/main/preload.js` (in the "File system" section):

```js
  renameFile: (oldPath, newPath) => ipcRenderer.invoke('fs:renameFile', oldPath, newPath),
  trashFile: (p) => ipcRenderer.invoke('fs:trashFile', p),
  duplicateFile: (p) => ipcRenderer.invoke('fs:duplicateFile', p),
  createFile: (p) => ipcRenderer.invoke('fs:createFile', p),
  showInFolder: (p) => ipcRenderer.invoke('fs:showInFolder', p),
```

- [ ] **Step 3: Create context-menu.js**

Create `src/renderer/context-menu.js`:

```js
import { $, el } from './state.js'

let activeMenu = null

export function showContextMenu(x, y, items) {
  closeContextMenu()

  const menu = el('div', 'context-menu')
  menu.style.left = `${x}px`
  menu.style.top = `${y}px`

  for (const item of items) {
    if (item.separator) {
      menu.appendChild(el('div', 'context-menu-separator'))
      continue
    }
    const row = el('div', 'context-menu-item', item.label)
    row.addEventListener('click', (e) => {
      e.stopPropagation()
      closeContextMenu()
      item.action()
    })
    menu.appendChild(row)
  }

  document.body.appendChild(menu)
  activeMenu = menu

  // Reposition if menu goes off-screen
  requestAnimationFrame(() => {
    const rect = menu.getBoundingClientRect()
    if (rect.right > window.innerWidth) {
      menu.style.left = `${window.innerWidth - rect.width - 8}px`
    }
    if (rect.bottom > window.innerHeight) {
      menu.style.top = `${window.innerHeight - rect.height - 8}px`
    }
  })

  const closeOnClick = (e) => {
    if (!menu.contains(e.target)) {
      closeContextMenu()
      document.removeEventListener('click', closeOnClick)
    }
  }
  setTimeout(() => document.addEventListener('click', closeOnClick), 0)
}

export function closeContextMenu() {
  if (activeMenu) {
    activeMenu.remove()
    activeMenu = null
  }
}
```

- [ ] **Step 4: Add context menu styles to main.css**

Append to `src/renderer/styles/main.css`:

```css
/* ── Context Menu ─────────────────────────────────────────────── */
.context-menu {
  position: fixed; z-index: 10000;
  min-width: 180px; padding: 4px 0;
  background: var(--bg1); border: 1px solid var(--border);
  box-shadow: var(--shadow-pop);
  font-size: 12px; font-family: var(--font);
}
.context-menu-item {
  padding: 6px 14px; cursor: pointer;
  color: var(--text2);
}
.context-menu-item:hover {
  background: var(--accent-dim); color: var(--text1);
}
.context-menu-separator {
  height: 1px; margin: 4px 8px;
  background: var(--border);
}
[data-theme="light"] .context-menu { background: var(--bg1); }
```

- [ ] **Step 5: Wire context menu to sidebar tree items**

In `src/renderer/tabs.js`, import the context menu and add right-click handlers to file tree items in the `renderTree` function (line 13). When a file/folder node gets a `contextmenu` event, call `showContextMenu` with appropriate items.

This requires modifying `src/renderer/tree-view.js` (which builds the DOM for tree items) to add `contextmenu` listeners, or adding event delegation in `tabs.js`.

The recommended approach is event delegation in the sidebar container. Add to the sidebar wiring in `shell.js`:

```js
// In buildShell(), after sidebar is built
const sidebar = $('sidebar')
if (sidebar) {
  sidebar.addEventListener('contextmenu', (e) => {
    const fileItem = e.target.closest('[data-path]')
    if (!fileItem) return
    e.preventDefault()
    const filePath = fileItem.dataset.path
    const isFolder = fileItem.dataset.type === 'folder'
    callbacks.showFileContextMenu(e.clientX, e.clientY, filePath, isFolder)
  })
}
```

Then implement `showFileContextMenu` as a callback that uses `showContextMenu` from `context-menu.js`.

- [ ] **Step 6: Test sidebar context menu**

Run: `npm run dev`

1. Right-click a file in sidebar — menu appears with Rename, Delete, Duplicate, Copy Path, Reveal
2. Click Reveal in Finder — opens Finder at that location
3. Click Delete — file moves to trash, disappears from tree
4. Click Duplicate — creates `filename-copy.md`, appears in tree
5. Right-click a folder — shows New File, New Folder, Rename, Delete, Reveal

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add sidebar right-click context menu (rename, delete, duplicate, reveal)"
```

---

### Task 2.2: Recent projects

**Files:**
- Create: `src/renderer/recent-projects.js`
- Modify: `src/renderer/shell.js` (integrate recent projects into welcome screen)
- Modify: `src/renderer/tabs.js` (update recent projects on folder open)

- [ ] **Step 1: Create recent-projects.js**

Create `src/renderer/recent-projects.js`:

```js
const STORAGE_KEY = 'fjordmark-recent-projects'
const MAX_RECENT = 10

export function getRecentProjects() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : []
  } catch {
    return []
  }
}

export function addRecentProject(folderPath) {
  const name = folderPath.split('/').pop() || folderPath.split('\\').pop() || folderPath
  const projects = getRecentProjects().filter(p => p.path !== folderPath)
  projects.unshift({ path: folderPath, name, lastOpened: new Date().toISOString() })
  if (projects.length > MAX_RECENT) projects.length = MAX_RECENT
  localStorage.setItem(STORAGE_KEY, JSON.stringify(projects))
}

export function removeRecentProject(folderPath) {
  const projects = getRecentProjects().filter(p => p.path !== folderPath)
  localStorage.setItem(STORAGE_KEY, JSON.stringify(projects))
}

export function formatRelativeTime(isoString) {
  const diff = Date.now() - new Date(isoString).getTime()
  const minutes = Math.floor(diff / 60000)
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d ago`
  return `${Math.floor(days / 30)}mo ago`
}

export function renderRecentProjectsHtml() {
  const projects = getRecentProjects()
  if (!projects.length) return ''

  const items = projects.map(p => `
    <div class="recent-item" data-path="${p.path}" title="${p.path}">
      <span class="recent-name">${p.name}</span>
      <span class="recent-path">${p.path}</span>
      <span class="recent-time">${formatRelativeTime(p.lastOpened)}</span>
      <span class="recent-remove" data-remove-path="${p.path}">&times;</span>
    </div>
  `).join('')

  return `
    <div class="recent-projects">
      <div class="recent-header">Recent</div>
      ${items}
    </div>
  `
}
```

- [ ] **Step 2: Integrate into welcome screen**

In `src/renderer/shell.js`, import `renderRecentProjectsHtml` and modify `buildWelcome()` to include the recent projects list below the "Open Folder" button. The welcome screen HTML should include `${renderRecentProjectsHtml()}` after the CTA.

- [ ] **Step 3: Update recent projects on folder open**

In `src/renderer/tabs.js`, import `addRecentProject` from `recent-projects.js`. In the `openFolder` function (line 94), after a folder is successfully opened, call:

```js
addRecentProject(folderPath)
```

- [ ] **Step 4: Add recent projects styles to main.css**

Append to `src/renderer/styles/main.css`:

```css
/* ── Recent Projects ──────────────────────────────────────────── */
.recent-projects { margin-top: 20px; max-width: 360px; }
.recent-header { font-size: 11px; color: var(--text3); text-transform: uppercase; letter-spacing: 0.08em; margin-bottom: 8px; }
.recent-item {
  display: flex; align-items: center; gap: 8px;
  padding: 8px 10px; cursor: pointer;
  font-size: 12px; color: var(--text2);
}
.recent-item:hover { background: rgba(255,255,255,0.03); }
.recent-name { color: var(--text1); font-weight: 500; flex-shrink: 0; }
.recent-path { color: var(--text3); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
.recent-time { color: var(--text3); font-size: 10px; flex-shrink: 0; }
.recent-remove { color: var(--text3); cursor: pointer; font-size: 14px; flex-shrink: 0; opacity: 0; transition: opacity 0.15s; }
.recent-item:hover .recent-remove { opacity: 1; }
.recent-remove:hover { color: var(--red); }
[data-theme="light"] .recent-item:hover { background: rgba(0,0,0,0.03); }
```

- [ ] **Step 5: Wire click handlers for recent items**

In the welcome screen event wiring (inside `buildShell()` or wherever the welcome DOM is constructed), add click delegation for `.recent-item` and `.recent-remove` elements.

- [ ] **Step 6: Test recent projects**

Run: `npm run dev`

1. Open a folder — it should be added to recent projects
2. Close all tabs to see the welcome screen — recent projects listed below "Open Folder"
3. Click a recent project — opens that folder
4. Click the X on a recent item — removes it from the list
5. Open 11+ folders — only the 10 most recent are shown

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add recent projects to welcome screen"
```

---

### Task 2.3: Pinned tabs

**Files:**
- Modify: `src/renderer/state.js` (add `pinned` field to tab shape)
- Modify: `src/renderer/tabs.js` (pin/unpin logic, sort pinned left, skip pinned on close)
- Modify: `src/renderer/styles/main.css` (pinned tab styles)

- [ ] **Step 1: Add pinned tracking**

The tab shape is `{ path, name, content, dirty }`. Add `pinned: false` when creating tabs. In `src/renderer/tabs.js`, wherever new tab objects are created (in `openFile` and `loadFileIntoTab`), add `pinned: false` to the object.

- [ ] **Step 2: Update renderTabs to sort pinned first**

In `src/renderer/tabs.js`, modify `renderTabs` (line 185) to sort tabs so pinned tabs come first. Before rendering, sort the group:

```js
const sortedTabs = [...groupTabs].sort((a, b) => (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0))
```

For pinned tabs, show a pin icon instead of the close button:

```js
const closeOrPin = tab.pinned
  ? '<span class="tab-pin">&#128204;</span>'
  : '<span class="tab-close">&times;</span>'
```

- [ ] **Step 3: Add pin/unpin functions**

Add to `src/renderer/tabs.js`:

```js
export function togglePinTab(tab) {
  if (!tab) return
  tab.pinned = !tab.pinned
  renderTabs('primary')
  renderTabs('secondary')
}
```

- [ ] **Step 4: Update closeTab to skip pinned**

In `src/renderer/tabs.js`, modify `closeTab` (line 221) to skip pinned tabs:

```js
if (tab.pinned) return  // Don't close pinned tabs
```

- [ ] **Step 5: Add pinned tab styles**

Append to `src/renderer/styles/main.css`:

```css
/* ── Pinned Tabs ──────────────────────────────────────────────── */
.tab.pinned { border-left: 2px solid var(--accent); }
.tab-pin { font-size: 10px; opacity: 0.5; }
```

- [ ] **Step 6: Test pinned tabs**

1. Right-click a tab — Pin option available (via tab context menu in Task 2.4)
2. Pinned tab shows pin icon, no close button
3. Cmd+W skips pinned tabs
4. Pinned tabs stay left-aligned

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add pinned tabs"
```

---

### Task 2.4: Tab context menu

**Files:**
- Modify: `src/renderer/tabs.js` (add contextmenu listener to tab elements)
- Reuse: `src/renderer/context-menu.js`

- [ ] **Step 1: Add right-click handler to tab elements**

In `src/renderer/tabs.js`, in the `renderTabs` function where tab DOM elements are created, add a `contextmenu` event listener:

```js
tabEl.addEventListener('contextmenu', (e) => {
  e.preventDefault()
  showTabContextMenu(e.clientX, e.clientY, tab, pane)
})
```

- [ ] **Step 2: Implement showTabContextMenu**

Add to `src/renderer/tabs.js`:

```js
import { showContextMenu } from './context-menu.js'

function showTabContextMenu(x, y, tab, pane) {
  const items = [
    { label: 'Close', action: () => closeTab(tab, pane) },
    { label: 'Close Others', action: () => {
      const group = [...getGroupTabs(pane)]
      for (const t of group) {
        if (t !== tab && !t.pinned) closeTab(t, pane)
      }
    }},
    { label: 'Close All', action: () => {
      const group = [...getGroupTabs(pane)]
      for (const t of group) {
        if (!t.pinned) closeTab(t, pane)
      }
    }},
    { label: 'Close to the Right', action: () => {
      const group = getGroupTabs(pane)
      const idx = group.indexOf(tab)
      const toClose = group.slice(idx + 1).filter(t => !t.pinned)
      for (const t of toClose) closeTab(t, pane)
    }},
    { separator: true },
    { label: tab.pinned ? 'Unpin' : 'Pin', action: () => togglePinTab(tab) },
    { separator: true },
    { label: 'Copy Path', action: () => navigator.clipboard.writeText(tab.path) },
    { label: 'Reveal in Finder', action: () => window.fjord.showInFolder(tab.path) },
  ]
  showContextMenu(x, y, items)
}
```

- [ ] **Step 3: Test tab context menu**

1. Right-click a tab — context menu appears
2. Close Others — closes all tabs except the clicked one (and any pinned)
3. Pin — tab becomes pinned
4. Copy Path — path copied to clipboard
5. Reveal in Finder — opens the file location

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add tab context menu (close, pin, copy path, reveal)"
```

---

## Phase 3 — Preview & Rendering Upgrades

### Task 3.1: Syntax highlighting in code blocks

**Files:**
- Modify: `package.json` (add rehype-highlight + highlight.js)
- Modify: `src/renderer/markdown.js` (add rehype-highlight to pipeline)
- Modify: `src/renderer/styles/main.css` (add highlight.js theme styles)

- [ ] **Step 1: Install dependencies**

```bash
npm install rehype-highlight highlight.js
```

- [ ] **Step 2: Add rehype-highlight to the unified pipeline**

In `src/renderer/markdown.js`, add import (after line 5):

```js
import rehypeHighlight from 'rehype-highlight'
```

Add to the processor chain (after `.use(remarkRehype, ...)` on line 10, before `.use(rehypeStringify, ...)`):

```js
  .use(rehypeHighlight, { detect: false, ignoreMissing: true })
```

The `detect: false` means no language guessing — only fenced blocks with explicit language tags get highlighted. `ignoreMissing: true` prevents errors for unknown languages.

- [ ] **Step 3: Add highlight.js theme CSS**

Rather than importing a full highlight.js theme CSS file (which would fight with the Fjordmark aesthetic), add a minimal set of rules to `main.css` that uses CSS variables:

Append to `src/renderer/styles/main.css`:

```css
/* ── Code Block Syntax Highlighting ───────────────────────────── */
.preview-pane .hljs { color: var(--text2); background: transparent; }
.preview-pane .hljs-keyword,
.preview-pane .hljs-selector-tag { color: var(--accent-hi); }
.preview-pane .hljs-string,
.preview-pane .hljs-attr { color: var(--green); }
.preview-pane .hljs-number,
.preview-pane .hljs-literal { color: var(--amber); }
.preview-pane .hljs-comment,
.preview-pane .hljs-quote { color: var(--text3); font-style: italic; }
.preview-pane .hljs-function .hljs-title,
.preview-pane .hljs-title { color: var(--text1); }
.preview-pane .hljs-type,
.preview-pane .hljs-built_in { color: var(--accent); }
.preview-pane .hljs-params { color: var(--text2); }
.preview-pane .hljs-meta { color: var(--text3); }
.preview-pane .hljs-addition { color: var(--green); }
.preview-pane .hljs-deletion { color: var(--red); }
```

- [ ] **Step 4: Test syntax highlighting**

Run: `npm run dev`

1. Open a markdown file with fenced code blocks (```js, ```python, etc.)
2. Switch to split or preview mode
3. Verify: code blocks show syntax coloring
4. Verify: blocks without a language tag remain monochrome
5. Verify: colors match in both dark and light themes

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add syntax highlighting in preview code blocks (rehype-highlight)"
```

---

### Task 3.2: LaTeX / math rendering

**Files:**
- Modify: `package.json` (add remark-math, rehype-katex, katex)
- Modify: `src/renderer/markdown.js` (add plugins to pipeline)
- Modify: `public/index.html` (add KaTeX CSS)

- [ ] **Step 1: Install dependencies**

```bash
npm install remark-math rehype-katex katex
```

- [ ] **Step 2: Add plugins to the unified pipeline**

In `src/renderer/markdown.js`, add imports:

```js
import remarkMath from 'remark-math'
import rehypeKatex from 'rehype-katex'
```

Update the processor chain:

```js
const processor = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .use(remarkMath)
  .use(remarkRehype, { allowDangerousHtml: true })
  .use(rehypeKatex)
  .use(rehypeHighlight, { detect: false, ignoreMissing: true })
  .use(rehypeStringify, { allowDangerousHtml: true })
```

- [ ] **Step 3: Add KaTeX CSS to index.html**

In `public/index.html`, add a link to the KaTeX CSS (after the existing CSS links):

```html
<link rel="stylesheet" href="../node_modules/katex/dist/katex.min.css" />
```

Note: esbuild bundles the renderer JS, but CSS from node_modules needs to be referenced or copied. The simplest approach for an Electron app is to reference it directly. If the path doesn't work at runtime, copy `node_modules/katex/dist/katex.min.css` and the `fonts/` directory to `public/` and reference from there.

- [ ] **Step 4: Override KaTeX colors to match theme**

Append to `src/renderer/styles/main.css`:

```css
/* ── KaTeX overrides ──────────────────────────────────────────── */
.preview-pane .katex { color: var(--text1); font-size: 1em; }
.preview-pane .katex-display { margin: 12px 0; }
.preview-pane .katex .katex-html { color: var(--text1); }
```

- [ ] **Step 5: Test math rendering**

Run: `npm run dev`

1. Create a markdown file with `$E = mc^2$` inline math and `$$\int_0^\infty e^{-x} dx = 1$$` block math
2. Preview mode — both render as formatted math
3. Invalid LaTeX (e.g., `$\frac{$`) shows error styling, doesn't crash
4. Works in both dark and light themes

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add LaTeX math rendering (KaTeX via remark-math + rehype-katex)"
```

---

### Task 3.3: Footnotes

**Files:**
- Modify: `src/renderer/markdown.js` (verify remark-gfm footnote support or add remark-footnotes)

- [ ] **Step 1: Test if remark-gfm already supports footnotes**

Create a test markdown file:
```markdown
Here is a footnote[^1].

[^1]: This is the footnote content.
```

Open in preview mode. If footnotes render correctly (superscript link + footnote section at bottom), remark-gfm handles it and no changes are needed.

- [ ] **Step 2: If not supported, install remark-footnotes**

```bash
npm install remark-footnotes
```

Add to `src/renderer/markdown.js`:

```js
import remarkFootnotes from 'remark-footnotes'
```

Add to the processor chain (after `remarkGfm`):

```js
  .use(remarkFootnotes, { inlineNotes: true })
```

- [ ] **Step 3: Add footnote styles**

Append to `src/renderer/styles/main.css`:

```css
/* ── Footnotes ────────────────────────────────────────────────── */
.preview-pane .footnotes { margin-top: 32px; padding-top: 16px; border-top: 1px solid var(--border); font-size: 0.9em; color: var(--text2); }
.preview-pane .footnotes ol { padding-left: 18px; }
.preview-pane sup a { color: var(--accent); text-decoration: none; font-size: 0.85em; }
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add footnote support in preview"
```

---

### Task 3.4: Emoji shortcodes

**Files:**
- Modify: `package.json` (add remark-emoji)
- Modify: `src/renderer/markdown.js`

- [ ] **Step 1: Install remark-emoji**

```bash
npm install remark-emoji
```

- [ ] **Step 2: Add to pipeline**

In `src/renderer/markdown.js`, add import:

```js
import remarkEmoji from 'remark-emoji'
```

Add to the processor chain (after `remarkGfm`):

```js
  .use(remarkEmoji)
```

- [ ] **Step 3: Test**

1. Write `:rocket:` in a markdown file
2. Preview shows the rocket emoji
3. Raw editor still shows `:rocket:` text

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add emoji shortcode rendering in preview"
```

---

## Phase 4 — Writing Tools

### Task 4.1: Word / session goals

**Files:**
- Create: `src/renderer/word-goals.js`
- Modify: `src/renderer/preview.js` (update status bar rendering)
- Modify: `src/renderer/styles/main.css`

- [ ] **Step 1: Create word-goals.js**

Create `src/renderer/word-goals.js`:

```js
const STORAGE_KEY = 'fjordmark-word-goals'

let sessionGoal = null
let sessionStartWords = 0

export function getDocumentGoal(filePath) {
  if (!filePath) return null
  try {
    const goals = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
    return goals[filePath] || null
  } catch {
    return null
  }
}

export function setDocumentGoal(filePath, target) {
  if (!filePath) return
  try {
    const goals = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
    if (target && target > 0) {
      goals[filePath] = target
    } else {
      delete goals[filePath]
    }
    localStorage.setItem(STORAGE_KEY, JSON.stringify(goals))
  } catch {}
}

export function setSessionGoal(target, currentWords) {
  sessionGoal = target > 0 ? target : null
  sessionStartWords = currentWords || 0
}

export function getSessionGoal() {
  return sessionGoal
}

export function getSessionProgress(currentWords) {
  if (!sessionGoal) return null
  const written = Math.max(0, currentWords - sessionStartWords)
  return { written, target: sessionGoal, complete: written >= sessionGoal }
}

export function formatGoalStatus(currentWords, filePath) {
  const parts = []
  const docGoal = getDocumentGoal(filePath)
  if (docGoal) {
    parts.push(`${currentWords} / ${docGoal}`)
  }
  const session = getSessionProgress(currentWords)
  if (session) {
    parts.push(`session: ${session.written} / ${session.target}`)
  }
  return parts.join('  |  ')
}

export function isGoalReached(currentWords, filePath) {
  const docGoal = getDocumentGoal(filePath)
  return docGoal && currentWords >= docGoal
}
```

- [ ] **Step 2: Integrate into status bar**

In `src/renderer/preview.js`, import the goal functions and update `updateActiveMetrics()` to show goal progress in the status bar. After setting word count text, check for goals:

```js
import { formatGoalStatus, isGoalReached } from './word-goals.js'

// Inside updateActiveMetrics, after setting words textContent:
const goalText = formatGoalStatus(stats.words, tab?.path)
if (goalText && words) {
  words.textContent = goalText
  if (isGoalReached(stats.words, tab?.path)) {
    words.classList.add('goal-reached')
    setTimeout(() => words.classList.remove('goal-reached'), 3000)
  }
}
```

- [ ] **Step 3: Add goal reached animation style**

Append to `src/renderer/styles/main.css`:

```css
/* ── Word Goals ───────────────────────────────────────────────── */
.goal-reached { color: var(--green) !important; transition: color 0.3s; }
```

- [ ] **Step 4: Add command palette commands for goals**

Register commands in `index.js`:

```js
  { id: 'set-doc-goal', label: 'Set Document Word Goal', action: () => {
    const target = prompt('Target word count:')
    if (target) setDocumentGoal(getFocusedTab()?.path, parseInt(target, 10))
  }},
  { id: 'set-session-goal', label: 'Set Session Word Goal', action: () => {
    const target = prompt('Session word target:')
    if (target) setSessionGoal(parseInt(target, 10), getStats(getFocusedTab()?.content || '').words)
  }},
  { id: 'clear-doc-goal', label: 'Clear Document Word Goal', action: () => {
    setDocumentGoal(getFocusedTab()?.path, null)
  }},
```

- [ ] **Step 5: Test word goals**

1. Set a document goal of 100 words via command palette
2. Status bar shows `42 / 100`
3. Type until 100 words — status bar flashes green briefly
4. Set a session goal of 50 — shows session progress alongside
5. Close and reopen — document goal persists, session goal resets

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add word count goals (per-document and per-session)"
```

---

### Task 4.2: Spellcheck toggle

**Files:**
- Modify: `src/renderer/editor.js` (add spellcheck compartment)
- Modify: `src/renderer/settings.js` (add spellcheck setting)

- [ ] **Step 1: Add setting**

In `src/renderer/settings.js`, add to `DEFAULT_SETTINGS`:

```js
  spellcheck: false,
```

Add to `sanitize()`:

```js
  next.spellcheck = Boolean(next.spellcheck)
```

- [ ] **Step 2: Add spellcheck compartment to editor.js**

In `src/renderer/editor.js`, add after the typewriter compartment:

```js
export const spellcheckCompartment = new Compartment()

const spellcheckOn = EditorView.contentAttributes.of({ spellcheck: 'true' })
const spellcheckOff = EditorView.contentAttributes.of({ spellcheck: 'false' })
```

Add to `createEditor` function signature: `spellcheckEnabled = false`

Add to the extensions array:

```js
      spellcheckCompartment.of(spellcheckEnabled ? spellcheckOn : spellcheckOff),
```

Add export:

```js
export function updateSpellcheck(view, enabled) {
  view.dispatch({
    effects: spellcheckCompartment.reconfigure(enabled ? spellcheckOn : spellcheckOff),
  })
}
```

- [ ] **Step 3: Pass spellcheck setting when creating editors**

Same pattern as typewriter — pass `spellcheckEnabled: getSettings().spellcheck` to `createEditor` in `workspace.js`.

- [ ] **Step 4: Test**

1. Enable spellcheck in settings
2. Type a misspelled word — red underline appears
3. Right-click — browser suggestions shown
4. Disable — underlines gone

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add spellcheck toggle in settings"
```

---

### Task 4.3: Reading time in status bar

**Files:**
- Modify: `src/renderer/preview.js` (add reading time to status bar)

- [ ] **Step 1: Add reading time element**

In `src/renderer/preview.js`, update `updateActiveMetrics()`. After setting words, add reading time:

```js
  const readTime = $('st-readtime')
  if (readTime) {
    readTime.textContent = stats.readMin < 1 ? '< 1 min read' : `~${stats.readMin} min read`
  }
```

- [ ] **Step 2: Add the status bar element**

In `src/renderer/shell.js`, where the status bar HTML is built, add a `<span id="st-readtime">` element after the words element.

- [ ] **Step 3: Test**

1. Open a document — status bar shows `~3 min read` after word count
2. Short document shows `< 1 min read`

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: show reading time estimate in status bar"
```

---

### Task 4.4: Text statistics expansion

**Files:**
- Modify: `src/renderer/markdown.js` (expand getStats)
- Modify: `src/renderer/preview.js` (render expanded stats in Insights)

- [ ] **Step 1: Expand getStats in markdown.js**

Replace `getStats` (markdown.js:261-268):

```js
export function getStats(markdown) {
  const text = markdown.replace(/```[\s\S]*?```/g, '').replace(/`[^`]+`/g, '').replace(/[#*_~\[\]]/g, '')
  const words = text.trim() ? text.trim().split(/\s+/).length : 0
  const chars = text.replace(/\s/g, '').length
  const paragraphs = markdown.split(/\n\n+/).filter(p => p.trim()).length
  const readMin = Math.ceil(words / 200) || 0

  // Sentence detection (basic: split on . ! ? followed by space or end)
  const sentences = text.split(/[.!?]+(?:\s|$)/).filter(s => s.trim()).length
  const avgSentenceLen = sentences > 0 ? Math.round(words / sentences * 10) / 10 : 0
  const avgWordLen = words > 0 ? Math.round(chars / words * 10) / 10 : 0

  // Flesch-Kincaid Grade Level (approximate)
  const syllables = countSyllables(text)
  const fkGrade = words > 0 && sentences > 0
    ? Math.round((0.39 * (words / sentences) + 11.8 * (syllables / words) - 15.59) * 10) / 10
    : 0

  return { words, chars, paragraphs, readMin, sentences, avgSentenceLen, avgWordLen, fkGrade }
}

function countSyllables(text) {
  const words = text.toLowerCase().split(/\s+/).filter(Boolean)
  let total = 0
  for (const word of words) {
    const cleaned = word.replace(/[^a-z]/g, '')
    if (!cleaned) continue
    let count = cleaned.replace(/(?:[^laeiouy]es|ed|[^laeiouy]e)$/, '').match(/[aeiouy]{1,2}/g)
    total += (count ? count.length : 1)
  }
  return total
}
```

- [ ] **Step 2: Update renderStatsPopover**

In `src/renderer/preview.js`, update `renderStatsPopover` (line 63) to include the new stats:

```js
export function renderStatsPopover(s) {
  const c = $('stats-content')
  if (!c) return
  c.innerHTML = `
    <div class="stat-card"><div class="num">${s.words}</div><div class="row"><span class="lbl">Words</span></div></div>
    <div class="stat-card"><div class="num">${s.chars}</div><div class="row"><span class="lbl">Characters</span></div></div>
    <div class="stat-card"><div class="num">${s.sentences}</div><div class="row"><span class="lbl">Sentences</span></div></div>
    <div class="stat-card"><div class="num">${s.paragraphs}</div><div class="row"><span class="lbl">Paragraphs</span></div></div>
    <div class="stat-card"><div class="num" style="font-size:15px">${s.readMin < 1 ? '< 1' : s.readMin} min</div><div class="row"><span class="lbl">Read time</span></div></div>
    <div class="stat-card"><div class="num">${s.avgSentenceLen}</div><div class="row"><span class="lbl">Avg words/sentence</span></div></div>
    <div class="stat-card"><div class="num">${s.avgWordLen}</div><div class="row"><span class="lbl">Avg word length</span></div></div>
    <div class="stat-card"><div class="num">${s.fkGrade}</div><div class="row"><span class="lbl">FK Grade Level</span></div></div>
  `
}
```

- [ ] **Step 3: Test**

1. Open a document — Insights panel shows expanded stats
2. Sentence count, avg sentence length, avg word length, FK grade all display
3. Empty document shows 0 for all

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: expand text statistics (sentences, readability, averages)"
```

---

## Phase 5 — Power User Features

### Task 5.1: Vim mode

**Files:**
- Modify: `package.json` (add @codemirror/vim)
- Modify: `src/renderer/editor.js` (vim compartment)
- Modify: `src/renderer/settings.js` (vim setting)
- Modify: `src/renderer/preview.js` (vim mode indicator in status bar)

- [ ] **Step 1: Install dependency**

```bash
npm install @replit/codemirror-vim
```

Note: The official `@codemirror/vim` package may be named `@replit/codemirror-vim`. Check npm for the correct package name at implementation time.

- [ ] **Step 2: Add setting**

In `src/renderer/settings.js`, add to `DEFAULT_SETTINGS`:

```js
  vimMode: false,
```

Add to `sanitize()`:

```js
  next.vimMode = Boolean(next.vimMode)
```

- [ ] **Step 3: Add vim compartment to editor.js**

In `src/renderer/editor.js`, add import:

```js
import { vim } from '@replit/codemirror-vim'
```

Add compartment:

```js
export const vimCompartment = new Compartment()
const vimOff = []
```

Add to `createEditor` signature: `vimEnabled = false`

Add to extensions array:

```js
      vimCompartment.of(vimEnabled ? vim() : vimOff),
```

Add export:

```js
export function updateVimMode(view, enabled) {
  view.dispatch({
    effects: vimCompartment.reconfigure(enabled ? vim() : vimOff),
  })
}
```

- [ ] **Step 4: Add vim mode indicator to status bar**

In `src/renderer/preview.js`, update `updateCursorStatus` to show the vim mode when vim is enabled:

Check if the editor state has a vim mode field and display it. This is implementation-dependent on how `@replit/codemirror-vim` exposes mode state.

- [ ] **Step 5: Test**

1. Enable vim mode in settings
2. Editor starts in NORMAL mode — typing doesn't insert text
3. Press `i` — switches to INSERT mode, typing works
4. Press Escape — back to NORMAL
5. Status bar shows current vim mode
6. Disable vim mode — normal editing resumes immediately

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add optional vim mode via settings"
```

---

### Task 5.2: Multiple cursors

**Files:**
- Modify: `src/renderer/editor.js` (add multi-cursor keybindings)

- [ ] **Step 1: Verify and add multi-cursor keybindings**

CodeMirror 6 supports multiple cursors natively. Check if `Cmd+D`, `Cmd+Shift+L`, `Alt+Click` already work via the default keymap. If not, add custom keybindings.

In `src/renderer/editor.js`, import the selection commands:

```js
import { selectNextOccurrence } from '@codemirror/search'
```

Note: `selectNextOccurrence` may not exist in the search package. Check the available exports. The CodeMirror way to do multi-cursor selection is via `@codemirror/search` or custom state effects.

Add to the keymap in `createEditor`:

```js
keymap.of([
  { key: 'Mod-d', run: selectNextOccurrence },
  ...defaultKeymap, ...historyKeymap, indentWithTab,
]),
```

- [ ] **Step 2: Install @codemirror/search if not already present**

```bash
npm install @codemirror/search
```

- [ ] **Step 3: Test**

1. Select a word, press Cmd+D — adds next occurrence to selection
2. Press Cmd+D again — adds another
3. Type — replaces all selected occurrences
4. Alt+Click — adds a cursor at click position

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: expose multi-cursor keybindings (Cmd+D, Alt+Click)"
```

---

### Task 5.3: Snippets / templates

**Files:**
- Modify: `src/renderer/command-palette.js` (add template source)
- Modify: `src/main/main.js` (IPC to read _templates/ folder)
- Modify: `src/main/preload.js` (expose readTemplates)

- [ ] **Step 1: Add IPC handler for reading templates**

In `src/main/main.js`:

```js
ipcMain.handle('fs:readTemplates', async (_, folderPath) => {
  const templatesDir = path.join(folderPath, '_templates')
  try {
    if (!fs.existsSync(templatesDir)) return []
    const files = fs.readdirSync(templatesDir).filter(f => f.endsWith('.md'))
    return files.map(f => ({
      name: f.replace(/\.md$/, ''),
      path: path.join(templatesDir, f),
      content: fs.readFileSync(path.join(templatesDir, f), 'utf-8'),
    }))
  } catch {
    return []
  }
})
```

- [ ] **Step 2: Expose in preload**

```js
  readTemplates: (folderPath) => ipcRenderer.invoke('fs:readTemplates', folderPath),
```

- [ ] **Step 3: Add built-in templates**

Define built-in templates as constants in `command-palette.js` or a new `templates.js`:

```js
const BUILT_IN_TEMPLATES = [
  { name: 'Blog Post', content: '# {{title}}\n\n*{{date}}*\n\n{{cursor}}' },
  { name: 'Meeting Notes', content: '# Meeting Notes — {{date}}\n\n## Attendees\n\n- \n\n## Agenda\n\n1. \n\n## Notes\n\n{{cursor}}\n\n## Action Items\n\n- [ ] ' },
  { name: 'Daily Note', content: '# {{date}}\n\n## Tasks\n\n- [ ] \n\n## Notes\n\n{{cursor}}' },
  { name: 'README', content: '# Project Name\n\n## Description\n\n{{cursor}}\n\n## Installation\n\n```bash\nnpm install\n```\n\n## Usage\n\n## License\n\nMIT' },
  { name: 'Changelog', content: '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- {{cursor}}\n\n### Changed\n\n### Fixed\n' },
]
```

- [ ] **Step 4: Integrate into command palette**

When the user types `template:` in the command palette, show built-in + user templates. On selection, resolve `{{date}}` to today's date, `{{title}}` prompts for input, and `{{cursor}}` marks where the cursor should land.

- [ ] **Step 5: Add "New File from Template" command**

Register a command that:
1. Shows template list
2. Creates a new file with the template content
3. Opens it in a tab

- [ ] **Step 6: Test**

1. Create a `_templates/` folder with a custom `.md` file
2. Open command palette, type `template:` — shows built-in + custom templates
3. Select "Blog Post" — inserts template with today's date, cursor at `{{cursor}}`
4. "New File from Template" creates a new file pre-filled

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add snippets/templates (built-in + _templates/ folder)"
```

---

### Task 5.4: Custom keyboard shortcuts

This is the most complex settings feature. It requires:
- A registry of all commands with their default bindings
- A UI to display and rebind them
- Conflict detection
- Persistence to localStorage
- Integration with the existing keydown listener

**Files:**
- Create: `src/renderer/keybindings.js`
- Modify: `src/renderer/shell.js` (add keybindings section to settings panel)
- Modify: `src/renderer/index.js` (use keybindings registry instead of hardcoded shortcuts)
- Modify: `src/renderer/styles/main.css`

- [ ] **Step 1: Create keybindings.js**

Create `src/renderer/keybindings.js`:

```js
const STORAGE_KEY = 'fjordmark-keybindings'

const defaultBindings = {
  'save': 'Cmd+S',
  'save-as': 'Cmd+Shift+S',
  'new-file': 'Cmd+N',
  'toggle-sidebar': 'Cmd+B',
  'toggle-toolbar': 'Cmd+\\',
  'find-replace': 'Cmd+F',
  'settings': 'Cmd+,',
  'command-palette': 'Cmd+K',
  'zen-mode': 'Cmd+Shift+Enter',
  'export-pdf': 'Cmd+E',
}

let overrides = {}
let actions = {}

export function initKeybindings() {
  try {
    overrides = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
  } catch {
    overrides = {}
  }
}

export function registerAction(id, label, action) {
  actions[id] = { id, label, action }
}

export function getBinding(id) {
  return overrides[id] || defaultBindings[id] || null
}

export function setBinding(id, keys) {
  overrides[id] = keys
  localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides))
}

export function resetBinding(id) {
  delete overrides[id]
  localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides))
}

export function getAllBindings() {
  const all = {}
  for (const id of Object.keys(defaultBindings)) {
    all[id] = {
      id,
      label: actions[id]?.label || id,
      default: defaultBindings[id],
      current: overrides[id] || defaultBindings[id],
      isOverridden: !!overrides[id],
    }
  }
  return all
}

export function findConflict(id, keys) {
  for (const [otherId, otherKeys] of Object.entries({ ...defaultBindings, ...overrides })) {
    if (otherId !== id && otherKeys === keys) return otherId
  }
  return null
}

export function matchesBinding(e, id) {
  const binding = getBinding(id)
  if (!binding) return false
  return matchesKeyCombo(e, binding)
}

function matchesKeyCombo(e, combo) {
  const parts = combo.split('+').map(p => p.trim().toLowerCase())
  const needsMod = parts.includes('cmd') || parts.includes('ctrl') || parts.includes('mod')
  const needsShift = parts.includes('shift')
  const needsAlt = parts.includes('alt')
  const key = parts.filter(p => !['cmd', 'ctrl', 'mod', 'shift', 'alt'].includes(p))[0]

  if (needsMod && !(e.metaKey || e.ctrlKey)) return false
  if (needsShift && !e.shiftKey) return false
  if (needsAlt && !e.altKey) return false
  if (!needsMod && (e.metaKey || e.ctrlKey)) return false
  if (!needsShift && e.shiftKey) return false

  return e.key.toLowerCase() === key || e.code.toLowerCase() === `key${key}`
}
```

- [ ] **Step 2: Refactor index.js keydown listener to use keybindings**

Replace the hardcoded shortcuts in `src/renderer/index.js` with calls to `matchesBinding`:

```js
import { initKeybindings, matchesBinding } from './keybindings.js'

initKeybindings()

document.addEventListener('keydown', e => {
  if (matchesBinding(e, 'save')) { e.preventDefault(); saveActive() }
  if (matchesBinding(e, 'save-as')) { e.preventDefault(); saveActiveAs() }
  if (matchesBinding(e, 'new-file')) { e.preventDefault(); createNewFile() }
  if (matchesBinding(e, 'toggle-sidebar')) { e.preventDefault(); toggleSidebar() }
  if (matchesBinding(e, 'toggle-toolbar')) { e.preventDefault(); toggleToolbar() }
  if (matchesBinding(e, 'find-replace')) { e.preventDefault(); toggleFindReplace() }
  if (matchesBinding(e, 'settings')) { e.preventDefault(); toggleSettingsPanel() }
  if (matchesBinding(e, 'command-palette')) { e.preventDefault(); toggleCommandPalette() }
  if (matchesBinding(e, 'zen-mode')) { e.preventDefault(); toggleZenMode() }
  if (e.key === 'Escape' && state.commandDialog) { e.preventDefault(); closeCommandDialog() }
  if (e.key === 'Escape' && state.settingsOpen) { e.preventDefault(); closeSettingsPanel() }
  if (e.key === 'Escape' && state.commandPaletteOpen) { e.preventDefault(); closeCommandPalette() }
  if (e.key === 'Escape' && state.zenMode) { e.preventDefault(); exitZenMode() }
})
```

- [ ] **Step 3: Add keyboard shortcuts section to settings panel**

In `src/renderer/shell.js`, add a new section to the settings panel that lists all bindings from `getAllBindings()`. Each row shows the command name and current binding. Clicking a binding enters "record mode" where the next key combo is captured.

- [ ] **Step 4: Test**

1. Open settings — Keyboard Shortcuts section shows all bindings
2. Click a binding — enters record mode
3. Press a new key combo — binding updates
4. Conflict detection warns if combo is already used
5. New binding works immediately
6. Persists across restart

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add customizable keyboard shortcuts in settings"
```

---

## Phase 6 — Import & Export

### Task 6.1: Export to HTML

**Files:**
- Modify: `src/main/main.js` (add export:html IPC handler)
- Modify: `src/main/preload.js`
- Modify: `src/renderer/preview.js` (add exportToHtml function)

- [ ] **Step 1: Add IPC handler**

In `src/main/main.js`, add after the `export:pdf` handler:

```js
// ── IPC: Export to HTML ───────────────────────────────────────────
ipcMain.handle('export:html', async (_, payload) => {
  try {
    const fileName = payload?.fileName || 'export'
    const savePath = await dialog.showSaveDialog(mainWindow, {
      defaultPath: `${fileName.replace(/\.md$/, '')}.html`,
      filters: [{ name: 'HTML files', extensions: ['html'] }],
    })
    if (savePath.canceled || !savePath.filePath) return false

    const html = buildExportHtml({
      title: fileName,
      html: payload?.html || '',
      theme: payload?.theme || 'dark',
      settings: payload?.settings || {},
    })
    fs.writeFileSync(savePath.filePath, html, 'utf-8')
    return true
  } catch (err) {
    console.error('HTML export error:', err)
    return false
  }
})
```

- [ ] **Step 2: Expose in preload**

```js
  exportHtml: (payload) => ipcRenderer.invoke('export:html', payload),
```

- [ ] **Step 3: Add exportToHtml in preview.js**

```js
export async function exportToHtml() {
  const tab = getFocusedTab()
  if (!tab) return
  try {
    const html = await renderMarkdown(tab.content || '')
    await window.fjord.exportHtml({
      fileName: tab.name,
      html,
      theme: getTheme(),
      settings: getSettings(),
    })
  } catch (err) {
    alert('Error exporting HTML: ' + err.message)
  }
}
```

- [ ] **Step 4: Add to menu and command palette**

Add menu item in `main.js` `buildAppMenu()` after the PDF export entry:

```js
    {
      label: 'Export to HTML…',
      click: () => sendRendererCommand('file:export-html'),
    },
```

Register command in `index.js`:

```js
  { id: 'export-html', label: 'Export to HTML', action: () => { import('./preview.js').then(m => m.exportToHtml()) } },
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add export to standalone HTML"
```

---

### Task 6.2: Export to DOCX

**Files:**
- Modify: `package.json` (add docx)
- Create: `src/renderer/export-docx.js`
- Modify: `src/main/main.js` (add save dialog IPC)
- Modify: `src/main/preload.js`

- [ ] **Step 1: Install dependency**

```bash
npm install docx
```

- [ ] **Step 2: Create export-docx.js**

This module converts the parsed Markdown AST to a DOCX document. Due to the complexity of full AST-to-DOCX conversion, a pragmatic approach is to convert the rendered HTML to DOCX via a simplified serializer.

Create `src/renderer/export-docx.js`:

```js
import { Document, Packer, Paragraph, TextRun, HeadingLevel, AlignmentType } from 'docx'
import { getFocusedTab } from './state.js'

export async function exportToDocx() {
  const tab = getFocusedTab()
  if (!tab) return

  const lines = (tab.content || '').split('\n')
  const children = []

  for (const line of lines) {
    const h1 = line.match(/^#\s+(.+)/)
    const h2 = line.match(/^##\s+(.+)/)
    const h3 = line.match(/^###\s+(.+)/)
    const bullet = line.match(/^[-*]\s+(.+)/)
    const numbered = line.match(/^\d+\.\s+(.+)/)

    if (h1) {
      children.push(new Paragraph({ text: h1[1], heading: HeadingLevel.HEADING_1 }))
    } else if (h2) {
      children.push(new Paragraph({ text: h2[1], heading: HeadingLevel.HEADING_2 }))
    } else if (h3) {
      children.push(new Paragraph({ text: h3[1], heading: HeadingLevel.HEADING_3 }))
    } else if (bullet) {
      children.push(new Paragraph({ text: bullet[1], bullet: { level: 0 } }))
    } else if (numbered) {
      children.push(new Paragraph({ text: numbered[1], numbering: { reference: 'default-numbering', level: 0 } }))
    } else if (line.trim() === '') {
      children.push(new Paragraph({ text: '' }))
    } else {
      // Parse inline formatting
      const runs = parseInlineRuns(line)
      children.push(new Paragraph({ children: runs }))
    }
  }

  const doc = new Document({
    numbering: {
      config: [{
        reference: 'default-numbering',
        levels: [{ level: 0, format: 'decimal', text: '%1.', alignment: AlignmentType.LEFT }],
      }],
    },
    sections: [{ children }],
  })

  const blob = await Packer.toBlob(doc)
  const arrayBuffer = await blob.arrayBuffer()
  const base64 = btoa(String.fromCharCode(...new Uint8Array(arrayBuffer)))

  const fileName = tab.name.replace(/\.md$/, '')
  await window.fjord.saveDocx(base64, fileName)
}

function parseInlineRuns(text) {
  const runs = []
  const regex = /(\*\*(.+?)\*\*|\*(.+?)\*|`(.+?)`|([^*`]+))/g
  let match
  while ((match = regex.exec(text)) !== null) {
    if (match[2]) {
      runs.push(new TextRun({ text: match[2], bold: true }))
    } else if (match[3]) {
      runs.push(new TextRun({ text: match[3], italics: true }))
    } else if (match[4]) {
      runs.push(new TextRun({ text: match[4], font: 'Courier New', size: 20 }))
    } else if (match[5]) {
      runs.push(new TextRun({ text: match[5] }))
    }
  }
  return runs.length ? runs : [new TextRun({ text })]
}
```

- [ ] **Step 3: Add save DOCX IPC handler**

In `src/main/main.js`:

```js
ipcMain.handle('export:docx', async (_, base64Data, fileName) => {
  try {
    const savePath = await dialog.showSaveDialog(mainWindow, {
      defaultPath: `${fileName}.docx`,
      filters: [{ name: 'Word documents', extensions: ['docx'] }],
    })
    if (savePath.canceled || !savePath.filePath) return false
    const buffer = Buffer.from(base64Data, 'base64')
    fs.writeFileSync(savePath.filePath, buffer)
    return true
  } catch (err) {
    console.error('DOCX export error:', err)
    return false
  }
})
```

In `src/main/preload.js`:

```js
  saveDocx: (base64Data, fileName) => ipcRenderer.invoke('export:docx', base64Data, fileName),
```

- [ ] **Step 4: Add to menu and command palette**

Add menu item and register command, same pattern as HTML export.

- [ ] **Step 5: Test**

1. Open a markdown file with headings, lists, bold, italic
2. Command palette > Export to DOCX
3. Save dialog appears, save file
4. Open in Word/LibreOffice — headings, lists, formatting preserved

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add export to DOCX"
```

---

### Task 6.3: Import from rich text paste

**Files:**
- Modify: `src/renderer/editor.js` (intercept HTML paste)

- [ ] **Step 1: Add rich text paste handling**

In `src/renderer/editor.js`, in the paste event handler (lines 97-112), add handling for `text/html`:

```js
    parent.addEventListener('paste', async (e) => {
      const items = e.clipboardData?.items
      if (!items) return

      // Check for images first
      for (let item of items) {
        if (item.kind === 'file' && item.type.startsWith('image/')) {
          e.preventDefault()
          const file = item.getAsFile()
          if (file && onPaste) await onPaste(file, view)
          return
        }
      }

      // Check for rich text (HTML) - convert to Markdown
      const htmlData = e.clipboardData?.getData('text/html')
      if (htmlData && onRichPaste) {
        e.preventDefault()
        onRichPaste(htmlData, view)
      }
    })
```

Add `onRichPaste` to the `createEditor` options. The callback converts HTML to Markdown using the existing `htmlToMarkdown()` function and inserts it.

- [ ] **Step 2: Implement onRichPaste callback**

In `src/renderer/workspace.js` (or wherever `createEditor` is called), pass:

```js
onRichPaste: (html, view) => {
  const md = htmlToMarkdown(html)
  const pos = view.state.selection.main.head
  view.dispatch({
    changes: { from: pos, to: pos, insert: md },
    selection: { anchor: pos + md.length },
  })
  onEditorChange(pane, view.state.doc.toString())
}
```

- [ ] **Step 3: Test**

1. Copy formatted text from a web page
2. Paste into the editor
3. Verify: HTML is converted to clean Markdown (headings, bold, links, lists preserved)
4. Pasting plain text still works normally

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: convert rich text paste to Markdown"
```

---

### Task 6.4: Print

**Files:**
- Modify: `src/renderer/styles/main.css` (add @media print styles)

- [ ] **Step 1: Add print styles**

Append to `src/renderer/styles/main.css`:

```css
/* ── Print ────────────────────────────────────────────────────── */
@media print {
  .titlebar, .sidebar, .sidebar-resizer, .tab-bar,
  .workspace-toolbar, .pane-label-row, .statusbar,
  .insights-panel, .find-replace-bar, .workspace-resizer,
  .pane-header, .zen-exit-hint, .command-palette-overlay,
  .context-menu, .settings-overlay { display: none !important; }
  .editor-area { position: static !important; }
  .pane { max-width: 100% !important; }
  .cm-host { display: none !important; }
  .preview-pane { display: block !important; color: #000 !important; }
  body { background: #fff !important; }
}
```

- [ ] **Step 2: Add Cmd+P handler**

Note: `Cmd+P` is typically handled by the browser/Electron for native print. In Electron, the default `Cmd+P` should trigger `window.print()`. If not, add it to the keydown handler:

```js
  if (mod && e.key === 'p') { e.preventDefault(); window.print() }
```

Register as a command in the command palette:

```js
  { id: 'print', label: 'Print', shortcut: 'Cmd+P', action: () => window.print() },
```

- [ ] **Step 3: Test**

1. Press Cmd+P — print dialog opens
2. Only the preview content is printed
3. All chrome is hidden

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add print support with @media print styles"
```

---

## Phase 7 — Settings Expansion

### Task 7.1: New settings

**Files:**
- Modify: `src/renderer/settings.js` (add new keys to DEFAULT_SETTINGS, sanitize)
- Modify: `src/renderer/shell.js` (add setting rows to settings panel HTML)

- [ ] **Step 1: Add new settings keys**

In `src/renderer/settings.js`, add to `DEFAULT_SETTINGS` (these are in addition to previously added keys):

```js
  autoSaveDelay: 800,
  tabIndentation: 'spaces',
  indentWidth: 2,
  softWrap: true,
  showLineNumbers: false,
  showStatusBar: true,
  defaultViewMode: 'split',
  readingSpeed: 200,
  zenParagraphDimming: false,
  zenColumnWidth: 700,
```

Add to `NUMERIC_KEYS`:

```js
  'autoSaveDelay',
  'indentWidth',
  'readingSpeed',
  'zenColumnWidth',
```

Add to `sanitize()`:

```js
  next.autoSaveDelay = clamp(Number(next.autoSaveDelay) || 800, 200, 5000)
  next.tabIndentation = ['spaces', 'tabs'].includes(next.tabIndentation) ? next.tabIndentation : 'spaces'
  next.indentWidth = [2, 4, 8].includes(Number(next.indentWidth)) ? Number(next.indentWidth) : 2
  next.softWrap = next.softWrap !== false
  next.showLineNumbers = Boolean(next.showLineNumbers)
  next.showStatusBar = next.showStatusBar !== false
  next.defaultViewMode = ['markdown', 'split', 'preview'].includes(next.defaultViewMode) ? next.defaultViewMode : 'split'
  next.readingSpeed = clamp(Number(next.readingSpeed) || 200, 100, 500)
  next.zenParagraphDimming = Boolean(next.zenParagraphDimming)
  next.zenColumnWidth = clamp(Number(next.zenColumnWidth) || 700, 500, 900)
```

- [ ] **Step 2: Add setting rows to settings panel**

In `src/renderer/shell.js`, add new setting rows in the appropriate groups. Use the existing `renderRangeSetting`, `renderSelectSetting`, and `renderToggleSetting` helpers:

**Editor group additions:**
- Auto-save delay: range slider 200-5000
- Tab indentation: select (spaces/tabs)
- Indent width: select (2/4/8)
- Soft wrap: toggle
- Show line numbers: toggle
- Typewriter scrolling: toggle
- Vim mode: toggle
- Spellcheck: toggle

**Interface group additions:**
- Show status bar: toggle
- Default view mode: select (markdown/split/preview)
- Reading speed: range slider 100-500

**Zen mode group (new section):**
- Paragraph dimming: toggle
- Column width: range slider 500-900

- [ ] **Step 3: Wire settings to editor behavior**

In `src/renderer/workspace.js` and `src/renderer/editor.js`, read these settings when creating/updating editors:
- `softWrap` → `EditorView.lineWrapping` (already on by default; make it conditional)
- `showLineNumbers` → `lineNumbers()` extension (already imported, currently gutters hidden via CSS; toggle via CSS or compartment)
- `autoSaveDelay` → update the debounce timer in `tabs.js` to use `getSettings().autoSaveDelay`

- [ ] **Step 4: Test each new setting**

Run through each setting toggle and verify it applies immediately.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: expand settings (auto-save delay, indent, wrap, line numbers, status bar, zen options)"
```

---

### Task 7.2: Settings export/import

**Files:**
- Modify: `src/renderer/shell.js` (add export/import buttons to settings panel)
- Modify: `src/main/main.js` (add IPC for save/load JSON)
- Modify: `src/main/preload.js`

- [ ] **Step 1: Add IPC handlers**

In `src/main/main.js`:

```js
ipcMain.handle('settings:export', async (_, jsonString) => {
  const savePath = await dialog.showSaveDialog(mainWindow, {
    defaultPath: 'fjordmark-settings.json',
    filters: [{ name: 'JSON files', extensions: ['json'] }],
  })
  if (savePath.canceled || !savePath.filePath) return false
  fs.writeFileSync(savePath.filePath, jsonString, 'utf-8')
  return true
})

ipcMain.handle('settings:import', async () => {
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openFile'],
    filters: [{ name: 'JSON files', extensions: ['json'] }],
  })
  if (result.canceled || !result.filePaths.length) return null
  return fs.readFileSync(result.filePaths[0], 'utf-8')
})
```

- [ ] **Step 2: Expose in preload**

```js
  exportSettings: (jsonString) => ipcRenderer.invoke('settings:export', jsonString),
  importSettings: () => ipcRenderer.invoke('settings:import'),
```

- [ ] **Step 3: Add buttons to settings panel**

In `src/renderer/shell.js`, add "Export Settings" and "Import Settings" buttons at the bottom of the settings panel, before the Done/Reset buttons.

Export: calls `window.fjord.exportSettings(JSON.stringify(getSettings()))`.
Import: calls `window.fjord.importSettings()`, parses JSON, calls `setSettings(parsed)`.

- [ ] **Step 4: Test**

1. Configure some custom settings
2. Export — saves a JSON file
3. Reset settings — all go back to defaults
4. Import the saved JSON — settings restored

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add settings export/import as JSON"
```

---

## Phase 8 — Platform Polish

### Task 8.1: macOS proxy icon and file associations

**Files:**
- Modify: `src/main/main.js` (set representedFilename)
- Modify: `package.json` (add file associations to electron-builder config)

- [ ] **Step 1: Set representedFilename on tab change**

In `src/main/main.js`, add an IPC handler:

```js
ipcMain.handle('window:setRepresentedFile', async (_, filePath) => {
  if (process.platform === 'darwin' && mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.setRepresentedFilename(filePath || '')
  }
})
```

In `src/main/preload.js`:

```js
  setRepresentedFile: (p) => ipcRenderer.invoke('window:setRepresentedFile', p),
```

In `src/renderer/tabs.js`, in `activateTab`, after setting the active tab, call:

```js
window.fjord.setRepresentedFile(tab.path)
```

- [ ] **Step 2: Add file associations**

In `package.json`, update the `build` section:

```json
"fileAssociations": [
  {
    "ext": "md",
    "name": "Markdown",
    "role": "Editor"
  },
  {
    "ext": "markdown",
    "name": "Markdown",
    "role": "Editor"
  }
]
```

- [ ] **Step 3: Handle open-file event**

In `src/main/main.js`, add:

```js
app.on('open-file', (event, filePath) => {
  event.preventDefault()
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('app:command', { command: 'file:open', path: filePath })
  }
})
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: add macOS proxy icon and .md file association"
```

---

### Task 8.2: Session restore

**Files:**
- Create: `src/renderer/session-restore.js`
- Modify: `src/renderer/tabs.js` (save session on changes, restore on folder open)

- [ ] **Step 1: Create session-restore.js**

Create `src/renderer/session-restore.js`:

```js
const PREFIX = 'fjordmark-session-'

function hashPath(folderPath) {
  let hash = 0
  for (let i = 0; i < folderPath.length; i++) {
    const char = folderPath.charCodeAt(i)
    hash = ((hash << 5) - hash) + char
    hash |= 0
  }
  return Math.abs(hash).toString(36)
}

export function saveSession(folderPath, sessionData) {
  if (!folderPath) return
  const key = PREFIX + hashPath(folderPath)
  try {
    localStorage.setItem(key, JSON.stringify({
      ...sessionData,
      savedAt: new Date().toISOString(),
    }))
  } catch {}
}

export function loadSession(folderPath) {
  if (!folderPath) return null
  const key = PREFIX + hashPath(folderPath)
  try {
    const raw = localStorage.getItem(key)
    return raw ? JSON.parse(raw) : null
  } catch {
    return null
  }
}

export function clearSession(folderPath) {
  if (!folderPath) return
  const key = PREFIX + hashPath(folderPath)
  localStorage.removeItem(key)
}
```

- [ ] **Step 2: Save session on tab/view changes**

In `src/renderer/tabs.js`, add a debounced session save that fires when tabs change (open, close, switch, reorder). Save:

```js
import { saveSession } from './session-restore.js'

function persistSession() {
  if (!state.folderPath) return
  saveSession(state.folderPath, {
    tabs: state.tabGroups.primary.map(t => ({ path: t.path, pinned: t.pinned || false })),
    secondaryTabs: state.tabGroups.secondary.map(t => ({ path: t.path, pinned: t.pinned || false })),
    activeTabPath: state.activeTab?.path || null,
    secondaryTabPath: state.secondaryTab?.path || null,
    workspaceMode: state.workspaceMode,
    sidebarVisible: state.sidebarVisible,
    toolbarVisible: state.toolbarVisible,
  })
}
```

Call `persistSession()` in `activateTab`, `closeTab`, `openFile`, `toggleSidebar`, `toggleToolbar`.

- [ ] **Step 3: Restore session on folder open**

In `src/renderer/tabs.js`, in `openFolder` (line 94), after the folder is loaded and tree rendered, check for a saved session:

```js
import { loadSession } from './session-restore.js'

// After tree is loaded:
const session = loadSession(folderPath)
if (session && session.tabs?.length) {
  for (const saved of session.tabs) {
    await loadFileIntoTab(saved.path)
    const tab = state.tabs.find(t => t.path === saved.path)
    if (tab && saved.pinned) tab.pinned = true
  }
  if (session.activeTabPath) {
    const activeTab = state.tabs.find(t => t.path === session.activeTabPath)
    if (activeTab) activateTab(activeTab)
  }
}
```

- [ ] **Step 4: Test**

1. Open a folder, open several tabs
2. Quit and reopen — same tabs restored
3. Pinned state preserved
4. Active tab restored
5. Files that no longer exist are skipped

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add session restore (tabs, pins, active tab persist across restart)"
```

---

### Task 8.3: Minimap

**Files:**
- Modify: `src/renderer/workspace.js` (add minimap DOM element)
- Modify: `src/renderer/styles/main.css`
- Modify: `src/renderer/settings.js` (add minimap setting)

- [ ] **Step 1: Add setting**

In `src/renderer/settings.js`, add to `DEFAULT_SETTINGS`:

```js
  showMinimap: false,
```

Add to `sanitize()`:

```js
  next.showMinimap = Boolean(next.showMinimap)
```

- [ ] **Step 2: Create minimap element**

In `src/renderer/workspace.js`, when building the editor UI, add a minimap container next to each CM host:

```html
<div class="minimap" id="minimap-${pane}" style="display:none"></div>
```

- [ ] **Step 3: Implement minimap rendering**

The minimap renders a scaled-down version of the document. A simple CSS approach:

```js
function updateMinimap(pane) {
  const minimap = $(`minimap-${pane}`)
  const editor = editorViews[pane]
  if (!minimap || !editor || !getSettings().showMinimap) {
    if (minimap) minimap.style.display = 'none'
    return
  }
  minimap.style.display = 'block'

  const doc = editor.state.doc.toString()
  const lines = doc.split('\n')
  minimap.innerHTML = lines.map(line => {
    const len = Math.min(line.length, 80)
    return `<div class="minimap-line" style="width:${len * 0.6}px"></div>`
  }).join('')

  // Show viewport indicator
  const scrollTop = editor.scrollDOM.scrollTop
  const scrollHeight = editor.scrollDOM.scrollHeight
  const clientHeight = editor.scrollDOM.clientHeight
  const ratio = scrollTop / (scrollHeight || 1)
  const viewRatio = clientHeight / (scrollHeight || 1)

  const indicator = minimap.querySelector('.minimap-viewport') || document.createElement('div')
  indicator.className = 'minimap-viewport'
  indicator.style.top = `${ratio * 100}%`
  indicator.style.height = `${viewRatio * 100}%`
  if (!indicator.parentNode) minimap.appendChild(indicator)
}
```

- [ ] **Step 4: Add minimap styles**

```css
/* ── Minimap ──────────────────────────────────────────────────── */
.minimap {
  position: absolute; right: 0; top: 0; bottom: 0;
  width: 60px; overflow: hidden;
  background: rgba(255,255,255,0.02);
  cursor: pointer;
}
.minimap-line {
  height: 2px; margin: 1px 4px;
  background: var(--text3); opacity: 0.15;
}
.minimap-viewport {
  position: absolute; left: 0; right: 0;
  background: rgba(255,255,255,0.06);
  pointer-events: none;
}
```

- [ ] **Step 5: Add scroll click handler**

Click on minimap jumps to that position. Drag scrolls.

- [ ] **Step 6: Test**

1. Enable minimap in settings
2. Document overview appears on right edge
3. Click to jump, viewport indicator shows current position
4. Disable — minimap disappears

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add optional minimap"
```

---

### Task 8.4: Auto-update

**Files:**
- Modify: `src/main/main.js` (add autoUpdater)
- Modify: `src/main/preload.js`
- Modify: `src/renderer/preview.js` (status bar update indicator)

- [ ] **Step 1: Install electron-updater**

```bash
npm install electron-updater --save
```

- [ ] **Step 2: Add auto-update logic to main.js**

At the top of `src/main/main.js`:

```js
const { autoUpdater } = require('electron-updater')
```

In `app.whenReady()`, after `createWindow()`:

```js
  // Auto-update (delay 5 seconds, check every 4 hours)
  setTimeout(() => {
    autoUpdater.checkForUpdatesAndNotify()
  }, 5000)
  setInterval(() => {
    autoUpdater.checkForUpdatesAndNotify()
  }, 4 * 60 * 60 * 1000)

  autoUpdater.on('update-available', () => {
    mainWindow.webContents.send('app:command', { command: 'update:available' })
  })

  autoUpdater.on('update-downloaded', () => {
    mainWindow.webContents.send('app:command', { command: 'update:ready' })
  })
```

- [ ] **Step 3: Show update indicator in status bar**

In the renderer, listen for update commands and show a subtle indicator:

```js
// In the app command handler:
case 'update:available':
  const updateEl = $('st-update')
  if (updateEl) {
    updateEl.textContent = 'Update available'
    updateEl.style.display = 'inline'
    updateEl.style.cursor = 'pointer'
    updateEl.onclick = () => { /* could trigger restart-and-install */ }
  }
  break
```

Add a `<span id="st-update" style="display:none"></span>` to the status bar HTML.

- [ ] **Step 4: Configure electron-builder for auto-update**

In `package.json`, add to the `build` section:

```json
"publish": {
  "provider": "github"
}
```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: add auto-update via electron-updater"
```

---

## Self-Review Checklist

After implementing all tasks, verify:

1. **Spec coverage:** Every section of the product vision spec has at least one task implementing it.
2. **No placeholders:** Every task has concrete code, exact file paths, and test steps.
3. **Type/name consistency:** Function names, state fields, and settings keys are consistent across tasks (e.g., `commandPaletteOpen` in state matches `state.commandPaletteOpen` checks everywhere).
4. **Dependency order:** Phase 0 must complete before Phase 1. Phases 1-8 have some internal ordering (command palette before templates use it, image paste fix before drag-and-drop extends it) but are mostly independent.
5. **Settings keys:** All new settings added in individual phase tasks are also listed in Phase 7 Task 7.1 for the consolidated settings panel update.
