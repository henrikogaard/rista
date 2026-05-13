# Command Palette + Session Restore — Design Spec

> Date: 2026-05-13
> Scope: Two independent features shipped together

---

## Feature 1: Command Palette (Cmd/Ctrl+K)

### Purpose

A floating search/command input that lets users quick-open files and execute editor commands without leaving the keyboard.

### Module

`src/renderer/command-palette.js` (~250 lines)

### Command Registry

A flat array of command objects, built dynamically each time the palette opens:

```js
{ id: string, label: string, category: 'file' | 'command', hint?: string, action: () => void }
```

- **File entries** — built from `state.tree`, recursively flattened to all `.md` files. Label is the filename, hint is the relative directory path.
- **Command entries** — a static list registered at boot:
  - View toggles: Markdown, Split, Preview, WYSIWYG
  - Insert actions: Heading 1-6, List (bullet/ordered/task), Link, Image, Table, Callout (note/warning/tip)
  - Toggles: Sidebar, Toolbar, Theme, Find & Replace
  - File operations: Save, Save As, New File, Open Folder, Export PDF, Close Tab
  - Settings: Open Settings

### Fuzzy Matching

Simple substring scoring, no external dependency:
1. Lowercase query and candidate label
2. Split query into whitespace-separated tokens
3. For each token, find its index in the label; if not found, candidate is excluded
4. Score = sum of (label.length - matchIndex) for each token — earlier matches score higher
5. Files score a small bonus when the filename portion (not path) matches a token

Results are sorted: highest score first. Files and commands are grouped separately in the list (files first, then commands), each group sorted by score.

### UI Structure

Reuses the existing command-dialog overlay (`.command-dialog-overlay`, z-index 70). The palette is a new element injected by `buildShell()`:

```html
<div class="cmd-palette" id="cmd-palette">
  <input class="cmd-palette__input" id="cmd-palette-input"
         placeholder="Search files and commands..." autocomplete="off" spellcheck="false">
  <div class="cmd-palette__list" id="cmd-palette-list">
    <!-- dynamically rendered -->
    <div class="cmd-palette__group-header">Files</div>
    <div class="cmd-palette__item cmd-palette__item--active" data-index="0">
      <span class="cmd-palette__item-label">README.md</span>
      <span class="cmd-palette__item-hint">docs/</span>
    </div>
    <!-- ... -->
    <div class="cmd-palette__group-header">Commands</div>
    <div class="cmd-palette__item" data-index="N">
      <span class="cmd-palette__item-label">Toggle sidebar</span>
      <span class="cmd-palette__item-hint">Cmd+B</span>
    </div>
  </div>
</div>
```

### Positioning & Styling

- `position: fixed; top: 20%; left: 50%; transform: translateX(-50%); width: min(520px, calc(100vw - 40px))`
- Background: `var(--surface-bg-2)`, border: `1px solid var(--surface-edge)`, `border-radius: 12px`
- Shadow: `var(--shadow-lg)`
- Backdrop-filter: `blur(24px)` (matches existing dialog style)
- Max list height: `min(360px, 50vh)`, overflow-y auto
- Item height: ~36px, hover/active highlight with `var(--accent-dim)`
- Hidden by default, shown via `.app.cmd-palette-open` class toggle (same pattern as settings/command dialog)

### Keyboard Navigation

- `Up/Down` arrows move the active item (wraps around), scrolls into view
- `Enter` executes the active item's action and closes the palette
- `Escape` closes the palette
- Typing filters the list in real-time (debounced at 0ms — synchronous filter on each keystroke)
- `Tab` does nothing (prevent focus leaving the input)

### State

Add `commandPaletteOpen: false` to `state` in `state.js`.

### Integration Points

| File | Change |
|------|--------|
| `state.js` | Add `commandPaletteOpen: false` to state object |
| `shell.js` | Add palette HTML container inside `buildShell()` |
| `index.js` | Add `Cmd+K` shortcut, import `toggleCommandPalette`; add `Escape` handler for palette |
| `main.css` | Add `.cmd-palette` styles (~60 lines) |
| `command-palette.js` | New module: registry, fuzzy search, render, keyboard handling |

No changes to `commands.js`, `tabs.js`, or `workspace.js` — the palette imports their exported functions directly.

### Empty State

When the query has no matches, show a centered message: "No results" in `var(--text3)`.

When no folder is open, file entries are empty — only command entries appear.

---

## Feature 2: Session Restore

### Purpose

Persist which folder is open, which files are open as tabs, and which tab is focused. On relaunch, restore the previous workspace state automatically.

### Module

`src/renderer/session.js` (~80 lines)

### localStorage Key

`fjordmark-session`

### Persisted Shape

```js
{
  folderPath: string | null,
  openFiles: string[],        // absolute file paths, in tab order
  focusedFile: string | null,  // path of the active tab
  paneView: string,            // primary pane view mode ('split', 'markdown', 'preview', 'wysiwyg')
}
```

### Save Triggers

A debounced `saveSession()` (200ms) is called after any tab state change:
- `openFile()` — new file opened
- `activateTab()` — focus changed
- `closeTab()` — tab removed
- `openFolder()` — folder changed (saves new empty session or clears)
- `beforeunload` event — safety net, calls `saveSession.flush()` (immediate, not debounced)

### Restore Flow

`restoreSession()` is called in `index.js` after `buildShell()` and `syncFolderUi()`:

1. Read `fjordmark-session` from localStorage, parse JSON. If missing/invalid, return (fresh start).
2. If `folderPath` exists:
   a. Call `window.fjord.readFolder(folderPath)` — if it throws or returns empty, clear session and return.
   b. Set `state.folderPath = folderPath`, update tree, start file watcher, sync folder UI.
3. For each path in `openFiles`:
   a. Call `window.fjord.stat(path)` — if null (file deleted), skip silently.
   b. Call `openFile({ path, name: basename(path) })` to open it as a tab.
4. If `focusedFile` matches an open tab, `activateTab()` it.
5. If `paneView` is set, apply it to the primary pane.

### Edge Cases

- **Folder deleted since last session:** Clear session, show welcome screen.
- **Some files deleted:** Open the ones that still exist, skip the rest. No error dialogs.
- **Empty session:** Normal fresh start — welcome screen.
- **Folder path exists but is empty:** Open folder, show empty tree. No tabs to restore.

### Integration Points

| File | Change |
|------|--------|
| `session.js` | New module: `saveSession()`, `restoreSession()`, debounce logic |
| `index.js` | Import and call `restoreSession()` after boot; add `beforeunload` listener |
| `tabs.js` | Import `saveSession`, call it in `openFile`, `activateTab`, `closeTab`, `openFolder` |

No changes to `state.js` — session reads from `state`, doesn't add properties to it.

---

## Non-Goals

- No unsaved buffer recovery (crash recovery for dirty tabs) — deferred to a future milestone
- No workspace split restore (dual pane layout) — restores single-pane only for simplicity
- No MRU (most-recently-used) file list beyond the open tabs
- No command palette plugin system or custom command registration
