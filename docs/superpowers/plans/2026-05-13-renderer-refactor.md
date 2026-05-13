# Renderer Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the 2317-line `src/renderer/index.js` into focused modules so each file has one responsibility, making the codebase easier to extend and maintain.

**Architecture:** Extract logical groups from index.js into separate modules. Each module exports the functions it owns and imports what it needs from other modules. A thin `index.js` remains as the entry point that wires everything together at boot. State is shared via a `state.js` module that exports the state object and accessor helpers.

**Tech Stack:** Vanilla JS, ES modules, esbuild bundling

---

## File Structure

After the refactor, the renderer directory will look like:

```
src/renderer/
├── index.js          # Thin entry: imports, init, boot, keyboard shortcuts (~100 lines)
├── state.js          # App state object + pane/tab accessor helpers (~120 lines)
├── icons.js          # SVG icon functions (~20 lines)
├── shell.js          # buildShell(), buildWelcome(), settings form rendering (~350 lines)
├── workspace.js      # Editor UI construction, pane layout, mount/destroy, workspace sync (~400 lines)
├── tabs.js           # Tab operations: activate, close, move, drag, render (~200 lines)
├── commands.js       # Editor commands: undo/redo, wrap, insert heading/list/link/table/callout/image, command dialogs (~350 lines)
├── find-replace.js   # Find & Replace panel logic (~120 lines)
├── preview.js        # Preview rendering, stats, ToC, PDF export, image paste (~150 lines)
├── editor.js         # (existing) CodeMirror setup — unchanged
├── markdown.js       # (existing) unified pipeline — unchanged
├── diagrams.js       # (existing) diagram rendering — unchanged
├── theme.js          # (existing) theme toggle — unchanged
├── settings.js       # (existing) settings persistence — unchanged
├── tree-view.js      # (existing) file tree rendering — unchanged
└── styles/
    └── main.css      # unchanged
```

**Key design decision:** `state.js` exports a mutable `state` object and all accessor/helper functions that operate purely on state (no DOM). Any module that needs state imports it from `state.js`. DOM helpers (`$`, `el`) are also exported from a shared location.

---

### Task 1: Extract `state.js` — state object and accessor helpers

**Files:**
- Create: `src/renderer/state.js`
- Modify: `src/renderer/index.js`

- [ ] **Step 1: Create `src/renderer/state.js`**

Extract the state object (lines 14-56), editor/timer refs (lines 58-79), and all pure state accessor functions (lines 81-191) into a new module:

```js
// src/renderer/state.js

// ── DOM helpers ───────────────────────────────────────────────────
export const $ = id => document.getElementById(id)
export const el = (tag, cls, html) => {
  const e = document.createElement(tag)
  if (cls) e.className = cls
  if (html) e.innerHTML = html
  return e
}

// ── App state ────────────────────────────────────────────────────
export const state = {
  folderPath: null,
  tree: [],
  expandedFolders: new Set(),
  appMeta: { name: 'Fjordmark', version: '' },
  tabs: [],
  tabGroups: {
    primary: [],
    secondary: [],
  },
  splitSnapshot: {
    primary: [],
    secondary: [],
    activePrimary: null,
    activeSecondary: null,
    focusedPane: 'primary',
  },
  activeTab: null,
  secondaryTab: null,
  focusedPane: 'primary',
  workspaceMode: 'single',
  paneView: {
    primary: 'split',
    secondary: 'split',
  },
  splitEditableMode: {
    primary: 'markdown',
    secondary: 'markdown',
  },
  splitPreviewSide: {
    primary: 'right',
    secondary: 'right',
  },
  toolbarVisible: true,
  sidebarVisible: true,
  insightsOpen: false,
  insightsSections: {
    stats: true,
    headings: true,
  },
  settingsOpen: false,
  commandDialog: null,
}

export const PANE_KEYS = ['primary', 'secondary']

export const editorViews = {
  primary: null,
  secondary: null,
}

export const richEditors = {
  primary: null,
  secondary: null,
}

export const richEditorMountTarget = {
  primary: null,
  secondary: null,
}

export const saveTimers = {
  primary: null,
  secondary: null,
}

export const syncingRichEditor = {
  primary: false,
  secondary: false,
}

export let draggedTab = null
export function setDraggedTab(value) { draggedTab = value }

// ── Pure state accessors ─────────────────────────────────────────
export function getPaneView(pane = state.focusedPane) {
  return state.paneView[pane] || 'split'
}

export function getSplitEditableView(pane = state.focusedPane) {
  return state.splitEditableMode[pane] === 'wysiwyg' ? 'wysiwyg' : 'markdown'
}

export function getSplitPreviewSide(pane = state.focusedPane) {
  return state.splitPreviewSide[pane] === 'left' ? 'left' : 'right'
}

export function makeSplitView(editableView = 'markdown', previewSlot = 'right') {
  const nextView = editableView === 'wysiwyg' ? 'wysiwyg' : 'markdown'
  return previewSlot === 'left'
    ? { left: 'preview', right: nextView }
    : { left: nextView, right: 'preview' }
}

export function getSplitView(pane = state.focusedPane) {
  return makeSplitView(getSplitEditableView(pane), getSplitPreviewSide(pane))
}

export function paneUsesWysiwyg(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'wysiwyg') return true
  if (paneView !== 'split') return false
  return getSplitEditableView(pane) === 'wysiwyg'
}

export function paneUsesMarkdown(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'markdown') return true
  if (paneView !== 'split') return false
  return getSplitEditableView(pane) === 'markdown'
}

export function getWysiwygMountSlot(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'wysiwyg') return 'single'
  if (paneView !== 'split') return null
  if (getSplitEditableView(pane) !== 'wysiwyg') return null
  return getSplitPreviewSide(pane) === 'left' ? 'right' : 'left'
}

export function getTabForPane(pane = state.focusedPane) {
  return pane === 'secondary' ? state.secondaryTab : state.activeTab
}

export function setTabForPane(pane, tab) {
  if (pane === 'secondary') state.secondaryTab = tab
  else state.activeTab = tab
}

export function getFocusedTab() {
  return getTabForPane(state.focusedPane)
}

export function getFocusedEditor() {
  return editorViews[state.focusedPane] || null
}

export function getGroupTabs(pane) {
  return state.tabGroups[pane]
}

export function hasTabInPane(tab, pane) {
  return getGroupTabs(pane).includes(tab)
}

export function addTabToPane(tab, pane) {
  const group = getGroupTabs(pane)
  if (!group.includes(tab)) group.push(tab)
}

export function removeTabFromPane(tab, pane) {
  const group = getGroupTabs(pane)
  const index = group.indexOf(tab)
  if (index >= 0) group.splice(index, 1)
}

export function getTabPane(tab, pane = state.focusedPane) {
  if (!tab) return null
  if (hasTabInPane(tab, pane)) return pane
  if (hasTabInPane(tab, 'primary')) return 'primary'
  if (hasTabInPane(tab, 'secondary')) return 'secondary'
  return null
}

export function isTabOpenAnywhere(tab) {
  return PANE_KEYS.some(pane => hasTabInPane(tab, pane))
}

export function cleanSplitSnapshot() {
  const validTabs = new Set(state.tabs)
  state.splitSnapshot.primary = state.splitSnapshot.primary.filter(tab => validTabs.has(tab))
  state.splitSnapshot.secondary = state.splitSnapshot.secondary.filter(tab => validTabs.has(tab))
  if (state.splitSnapshot.activePrimary && !validTabs.has(state.splitSnapshot.activePrimary)) state.splitSnapshot.activePrimary = null
  if (state.splitSnapshot.activeSecondary && !validTabs.has(state.splitSnapshot.activeSecondary)) state.splitSnapshot.activeSecondary = null
}

export function storeSplitSnapshot() {
  state.splitSnapshot = {
    primary: [...state.tabGroups.primary],
    secondary: [...state.tabGroups.secondary],
    activePrimary: state.activeTab,
    activeSecondary: state.secondaryTab,
    focusedPane: state.focusedPane,
  }
  cleanSplitSnapshot()
}
```

- [ ] **Step 2: Update index.js imports**

Replace the state/accessor code in `index.js` lines 14-191 with imports from `state.js`. Remove the `$`, `el` helpers (line 208-209) and import them from `state.js` instead.

- [ ] **Step 3: Build and verify**

```bash
npm run build
```

Expected: Build succeeds with no errors.

- [ ] **Step 4: Commit**

```bash
git add src/renderer/state.js src/renderer/index.js
git commit -m "refactor: extract state and accessor helpers into state.js"
```

---

### Task 2: Extract `icons.js` — SVG icon functions

**Files:**
- Create: `src/renderer/icons.js`
- Modify: `src/renderer/index.js`

- [ ] **Step 1: Create `src/renderer/icons.js`**

Move the icon functions (lines 2120-2134 of original index.js):

```js
// src/renderer/icons.js
export function sunIcon() {
  return `<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="2.8"/><path d="M8 1.25v2.1M8 12.65v2.1M1.25 8h2.1M12.65 8h2.1M3.35 3.35l1.45 1.45M11.2 11.2l1.45 1.45M3.35 12.65l1.45-1.45M11.2 4.8l1.45-1.45"/></svg>`
}
export function moonIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M11.9 10.6A5.8 5.8 0 0 1 5.4 4.1c0-.44.05-.87.15-1.28A6.2 6.2 0 1 0 13.2 10.45c-.41.1-.84.15-1.3.15z"/></svg>`
}
export function gearIcon() {
  return `<svg viewBox="0 0 16 16"><line x1="3" y1="4" x2="13" y2="4"/><line x1="3" y1="8" x2="13" y2="8"/><line x1="3" y1="12" x2="13" y2="12"/><circle cx="6" cy="4" r="1.4" fill="currentColor" stroke="none"/><circle cx="10" cy="8" r="1.4" fill="currentColor" stroke="none"/><circle cx="7" cy="12" r="1.4" fill="currentColor" stroke="none"/></svg>`
}
export function closeIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/></svg>`
}
export function chevronIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M4 6l4 4 4-4"/></svg>`
}
```

- [ ] **Step 2: Update index.js**

Replace the icon function definitions with `import { sunIcon, moonIcon, gearIcon, closeIcon, chevronIcon } from './icons.js'`.

- [ ] **Step 3: Build and verify**

```bash
npm run build
```

- [ ] **Step 4: Commit**

```bash
git add src/renderer/icons.js src/renderer/index.js
git commit -m "refactor: extract icon SVG functions into icons.js"
```

---

### Task 3: Extract `find-replace.js`

**Files:**
- Create: `src/renderer/find-replace.js`
- Modify: `src/renderer/index.js`

- [ ] **Step 1: Create `src/renderer/find-replace.js`**

Move the find/replace state and functions (lines 2136-2238 of original):

```js
// src/renderer/find-replace.js
import { $, state, getFocusedEditor } from './state.js'

let findMatches = []
let currentMatchIndex = -1

export function toggleFindReplace() {
  const panel = $('find-replace')
  panel.classList.toggle('hidden')
  if (!panel.classList.contains('hidden')) {
    $('find-input').focus()
  }
}

export function updateFind() {
  const view = getFocusedEditor()
  if (!view) return
  const query = $('find-input').value
  if (!query) {
    findMatches = []
    currentMatchIndex = -1
    updateFindCount()
    return
  }

  const text = view.state.doc.toString()
  findMatches = []
  const regex = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi')
  let match
  while ((match = regex.exec(text)) !== null) {
    findMatches.push({ from: match.index, to: match.index + match[0].length })
  }
  currentMatchIndex = findMatches.length > 0 ? 0 : -1
  updateFindCount()
  highlightMatch()
}

function updateFindCount() {
  const count = $('find-count')
  if (findMatches.length === 0) {
    count.textContent = 'No results'
  } else {
    count.textContent = `${currentMatchIndex + 1}/${findMatches.length}`
  }
}

function highlightMatch() {
  const view = getFocusedEditor()
  if (!view) return
  if (currentMatchIndex < 0 || !findMatches[currentMatchIndex]) return
  const match = findMatches[currentMatchIndex]
  view.dispatch({
    selection: { anchor: match.from, head: match.to },
    scrollIntoView: true,
  })
}

export function findNext() {
  if (findMatches.length === 0) return
  currentMatchIndex = (currentMatchIndex + 1) % findMatches.length
  updateFindCount()
  highlightMatch()
}

export function findPrev() {
  if (findMatches.length === 0) return
  currentMatchIndex = (currentMatchIndex - 1 + findMatches.length) % findMatches.length
  updateFindCount()
  highlightMatch()
}

export function replaceOne(onEditorChange) {
  const view = getFocusedEditor()
  if (!view || currentMatchIndex < 0) return
  const match = findMatches[currentMatchIndex]
  const replacement = $('replace-input').value
  view.dispatch({
    changes: { from: match.from, to: match.to, insert: replacement },
  })
  onEditorChange(state.focusedPane, view.state.doc.toString())
  updateFind()
}

export function replaceAll(onEditorChange) {
  const view = getFocusedEditor()
  if (!view || findMatches.length === 0) return
  const replacement = $('replace-input').value
  const changes = findMatches.map(match => ({
    from: match.from,
    to: match.to,
    insert: replacement,
  })).reverse()

  view.dispatch({ changes })
  onEditorChange(state.focusedPane, view.state.doc.toString())
  updateFind()
}

export function handleFindKeydown(e) {
  if (e.key === 'Enter') findNext()
  if (e.key === 'Escape') toggleFindReplace()
}
```

- [ ] **Step 2: Update index.js**

Replace the find/replace code with imports. Wire `replaceOne` and `replaceAll` in the event setup, passing `onEditorChange` as a callback.

- [ ] **Step 3: Build and verify**

```bash
npm run build
```

- [ ] **Step 4: Commit**

```bash
git add src/renderer/find-replace.js src/renderer/index.js
git commit -m "refactor: extract find-replace into its own module"
```

---

### Task 4: Extract `preview.js` — preview rendering, stats, ToC, export

**Files:**
- Create: `src/renderer/preview.js`
- Modify: `src/renderer/index.js`

- [ ] **Step 1: Create `src/renderer/preview.js`**

Move preview rendering (refreshPreview), stats/ToC rendering, PDF export, image paste, and WYSIWYG sync:

```js
// src/renderer/preview.js
import { $, state, getTabForPane, getFocusedTab, paneUsesWysiwyg } from './state.js'
import { renderMarkdown, extractHeadings, getStats } from './markdown.js'
import { processDiagrams } from './diagrams.js'
import { getTheme } from './theme.js'
import { getSettings } from './settings.js'

export async function refreshPreview(pane, markdown) {
  const html = await renderMarkdown(markdown)
  const theme = getTheme()
  ;['single', 'left', 'right'].forEach(slot => {
    const p = $(`preview-${slot}-${pane}`)
    if (p) p.innerHTML = html
  })
  const diagramPromises = ['single', 'left', 'right'].map(slot => {
    const p = $(`preview-${slot}-${pane}`)
    return p ? processDiagrams(p, theme) : null
  }).filter(Boolean)
  await Promise.all(diagramPromises)
}

export function updateActiveMetrics() {
  const tab = getFocusedTab()
  const markdown = tab?.content || ''
  const words = $('st-words')
  const lines = $('st-lines')
  updateCursorStatus()
  if (!tab) {
    if (words) words.textContent = '\u2014'
    if (lines) lines.textContent = '\u2014'
    renderStatsPopover(getStats(''))
    renderTocPopover([])
    return
  }
  const stats = getStats(markdown)
  if (words) words.textContent = `${stats.words} words`
  if (lines) lines.textContent = `${markdown.split('\n').length} lines`
  renderStatsPopover(stats)
  renderTocPopover(extractHeadings(markdown))
}

export function updateCursorStatus(editorState) {
  const cursor = $('st-cursor')
  if (!cursor) return
  if (!editorState) {
    editorState = (await import('./state.js')).getFocusedEditor()?.state
  }
  if (!editorState) {
    cursor.textContent = 'Ln 1, Col 1'
    return
  }
  const head = editorState.selection.main.head
  const line = editorState.doc.lineAt(head)
  cursor.textContent = `Ln ${line.number}, Col ${head - line.from + 1}`
}

function renderStatsPopover(s) {
  const c = $('stats-content')
  if (!c) return
  c.innerHTML = `
    <div class="stat-card"><div class="num">${s.words}</div><div class="row"><span class="lbl">Words</span></div></div>
    <div class="stat-card"><div class="num">${s.chars}</div><div class="row"><span class="lbl">Characters</span></div></div>
    <div class="stat-card"><div class="num">${s.paragraphs}</div><div class="row"><span class="lbl">Paragraphs</span></div></div>
    <div class="stat-card"><div class="num" style="font-size:15px">${s.readMin < 1 ? '< 1' : s.readMin} min</div><div class="row"><span class="lbl">Read time</span></div></div>
  `
}

function renderTocPopover(headings) {
  const c = $('toc-content')
  if (!c) return
  if (!headings.length) {
    c.innerHTML = `<div class="toc-empty"><div class="icon">\u{1F3D4}\uFE0F</div><div class="msg">No headers yet</div></div>`
    return
  }
  c.innerHTML = headings.map(h =>
    `<div class="toc-item h${h.level}">${h.text}</div>`
  ).join('')
}

export async function exportToPdf() {
  const tab = getFocusedTab()
  if (!tab) {
    alert('No file open')
    return
  }
  try {
    const html = await renderMarkdown(tab.content || '')
    const success = await window.fjord.exportPdf({
      fileName: tab.name,
      html,
      theme: getTheme(),
      settings: getSettings(),
    })
    if (!success) alert('Failed to export PDF')
  } catch (err) {
    alert('Error exporting PDF: ' + err.message)
  }
}

export async function handleImagePaste(file, view, pane = state.focusedPane, onEditorChange) {
  if (!getTabForPane(pane)) return
  const reader = new FileReader()
  reader.onload = (e) => {
    const base64 = e.target.result
    const ext = file.type.split('/')[1] || 'png'
    const pos = view.state.selection.main.head
    const markdown = `![](data:${file.type};base64,${base64.split(',')[1]})`
    view.dispatch({
      changes: { from: pos, to: pos, insert: markdown },
      selection: { anchor: pos + markdown.length },
    })
    onEditorChange(pane, view.state.doc.toString())
  }
  reader.readAsDataURL(file)
}
```

Note: `updateCursorStatus` uses a dynamic import fallback — this will be cleaned up once all modules are extracted. The simpler approach is to accept `editorState` as a parameter and let callers provide it.

- [ ] **Step 2: Update index.js**

Remove the preview/stats/export code and import from `preview.js`.

- [ ] **Step 3: Build and verify**

```bash
npm run build
```

- [ ] **Step 4: Commit**

```bash
git add src/renderer/preview.js src/renderer/index.js
git commit -m "refactor: extract preview, stats, ToC, and export into preview.js"
```

---

### Task 5: Extract `commands.js` — editor commands, insert helpers, command dialogs

**Files:**
- Create: `src/renderer/commands.js`
- Modify: `src/renderer/index.js`

- [ ] **Step 1: Create `src/renderer/commands.js`**

Move all editor command functions: `editorCmd`, `wrapInline`, `wrapSelection`, `insertHeading`, `insertList`, `insertLink`, `insertImage`, `insertTable`, `insertCallout`, `insertMarkdownAtSelection`, `insertMarkdownTable`, `insertImageReference`, path utility functions, `submitCommandDialog`, `openCommandDialog`, `closeCommandDialog`, and the WYSIWYG command helpers.

The module imports `undo`, `redo` from `@codemirror/commands` directly.

- [ ] **Step 2: Update index.js**

Remove the command code and import from `commands.js`. Wire command functions into toolbar click handler.

- [ ] **Step 3: Build and verify**

```bash
npm run build
```

- [ ] **Step 4: Commit**

```bash
git add src/renderer/commands.js src/renderer/index.js
git commit -m "refactor: extract editor commands and dialogs into commands.js"
```

---

### Task 6: Extract `tabs.js` — tab operations

**Files:**
- Create: `src/renderer/tabs.js`
- Modify: `src/renderer/index.js`

- [ ] **Step 1: Create `src/renderer/tabs.js`**

Move: `activateTab`, `renderTabs`, `closeTab`, `moveTabToPane`, tab drag handlers, `showWelcomeScreen`, `openFile`, `loadFileIntoTab`, `createNewFile`, `openFolder`, `saveTab`, `saveActive`, `saveActiveAs`, `onEditorChange`, `onRichEditorChange`, `syncTabRepresentations`.

This module is the largest extracted piece. It imports from `state.js`, `preview.js`, and `workspace.js`.

- [ ] **Step 2: Update index.js**

Remove tab code and import from `tabs.js`.

- [ ] **Step 3: Build and verify**

```bash
npm run build
```

- [ ] **Step 4: Commit**

```bash
git add src/renderer/tabs.js src/renderer/index.js
git commit -m "refactor: extract tab and file operations into tabs.js"
```

---

### Task 7: Extract `workspace.js` — pane layout, editor UI, mount/destroy

**Files:**
- Create: `src/renderer/workspace.js`
- Modify: `src/renderer/index.js`

- [ ] **Step 1: Create `src/renderer/workspace.js`**

Move: `buildEditorUI`, `destroyEditors`, `mountEditor`, `destroyRichEditor`, `ensureRichEditorMounted`, `ensureEditorForPane`, `refreshAllPreviews`, `maybeRefreshWysiwygPane`, `syncWorkspaceUi`, `syncFocusedPaneUi`, `syncSplitLayout`, `placeStandaloneView`, `placePaneView`, `setPaneView`, `setSplitPaneView`, `toggleToolbar`, `toggleSidebar`, `toggleInsightsPanel`, `toggleInsightsSection`, `toggleWorkspaceSplit`, `focusPane`, resize handlers, `renderEditorToolbar`, `renderSplitSlotSelector`, `syncToWysiwyg`, `syncWorkspaceSplitToggle`, `syncToolbarToggle`.

- [ ] **Step 2: Update index.js**

Remove workspace code and import from `workspace.js`.

- [ ] **Step 3: Build and verify**

```bash
npm run build
```

- [ ] **Step 4: Commit**

```bash
git add src/renderer/workspace.js src/renderer/index.js
git commit -m "refactor: extract workspace layout and pane management into workspace.js"
```

---

### Task 8: Clean up `index.js` as thin entry point

**Files:**
- Modify: `src/renderer/index.js`

- [ ] **Step 1: Verify index.js is thin**

After all extractions, `index.js` should contain only:
1. Imports from all modules
2. `initTheme()` / `applySettings()` / `initDiagrams()` calls
3. `buildShell()` call
4. `syncFolderUi()` call
5. Keyboard shortcuts `document.addEventListener('keydown', ...)`
6. `window.fjord.onFileChange` and `window.fjord.onCommand` wiring

Target: under 100 lines.

- [ ] **Step 2: Final build and manual test**

```bash
npm run build
```

Start the app and test:
- Open a folder
- Open a file
- Switch views (markdown, split, WYSIWYG, preview)
- Toggle theme
- Toggle workspace split
- Use find/replace
- Insert a heading, list, link, table
- Close a tab
- Export to PDF

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: finalize renderer module extraction — index.js is now thin entry point"
```
