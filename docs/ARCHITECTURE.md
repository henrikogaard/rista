# Fjordmark Architecture

## Goal

This is the short, implementation-facing architecture reference for Fjordmark.

For deeper planning context, see:
- [ARCHITECTURE-PLAN.md](/Users/henrik/Repos/Fjordmark/docs/ARCHITECTURE-PLAN.md)
- [PRODUCT-ROADMAP.md](/Users/henrik/Repos/Fjordmark/docs/PRODUCT-ROADMAP.md)

## High-Level Structure

Fjordmark has three layers:

1. Main process
   Electron window lifecycle, menus, dialogs, file access, watching, export.

2. Preload layer
   Safe renderer-facing API on `window.fjord.*`.

3. Renderer
   State, layout, panes, editors, preview, tree, settings, dialogs.

## Architecture Rules

### 1. State drives layout

The DOM must be an output of state.
Do not rely on previous DOM placement as hidden state.

### 2. Standalone and split layouts stay separate

Standalone:
- Markdown
- Preview
- WYSIWYG

Split:
- left/right layout with one editable side and one preview side

Do not reuse split slots as the standalone rendering path.

### 3. Surface ownership must be explicit

Per workspace pane:
- one CodeMirror instance
- one WYSIWYG instance
- one standalone surface container
- one split layout container

Each surface should always have a clear canonical mount target.

### 4. File identity is path-based

Use full file paths for:
- tab identity
- tree highlighting
- watcher updates
- save-as updates

Never rely on filenames alone.

### 5. Keep main process thin

The main process should do:
- window lifecycle
- menus
- dialogs
- file system access
- watcher events
- export

Renderer should own application behavior and UI state.

## Recommended Renderer Module Boundaries

Target direction:

- `index.js`
  Bootstrapping only

- `app-shell.js`
  App shell and global UI mounting

- `pane-layout.js`
  Pane mode rendering and surface ownership

- `workspace.js`
  Workspace split, pane focus, pane state

- `tabs.js`
  Tab open/close/activate/move logic

- `tree-view.js`
  Explorer rendering and file activation

- `commands.js`
  Toolbar and command handlers

- `insights.js`
  Stats and headings

- `dialogs.js`
  Command dialogs and overlays

## Target State Shape

Keep one explicit state tree for:

- workspace
- panes
- tabs
- tree
- UI flags
- settings

The important rule:
mode transitions should go through dedicated state helpers, not scattered direct mutations.

## Pane Rendering Model

Each pane should support:

### Standalone mode

One dedicated full-width surface:
- Markdown editor
- Preview
- WYSIWYG editor

### Split mode

One dedicated split layout:
- editable side: `markdown` or `wysiwyg`
- preview side: `left` or `right`

No arbitrary left/right split state beyond those two values.

## Performance Rules

### Separate expensive pipelines

Keep independent scheduling for:
- autosave
- preview rendering
- stats
- heading extraction

### Avoid full rerenders

Prefer targeted updates:
- active pane only
- changed tab only
- affected tree node only

### Keep tree updates incremental

Do not rebuild the entire tree for every file change.

## Robustness Rules

### Safer IPC

Prefer structured return shapes over bare booleans.

### Conflict awareness

Dirty tabs should not silently lose to external file changes.

### Recovery readiness

Design state and tabs so session restore and crash recovery can be added cleanly.

## Near-Term Priorities

1. Validate pane/surface stability after current fixes
2. Refactor renderer into modules
3. Add `TEST-CASES.md`
4. Fix path-based identity issues
5. Fix preload unsubscribe behavior
6. Rebuild PDF export correctly

