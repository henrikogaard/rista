# Rísta Architecture Plan

## Purpose

This document defines the target architecture for Rísta as it evolves into a fast, robust, visually polished Markdown IDE for writing, note organization, and long-term knowledge work.

It is intended to support the roadmap in:
- [PRODUCT-ROADMAP.md](./PRODUCT-ROADMAP.md)
- [PROJECT-BOARD.md](./PROJECT-BOARD.md)

## Architecture Goals

1. Keep editing fast and predictable.
2. Make state explicit and layout derivable from state.
3. Keep file-based workflows simple and local-first.
4. Support both lightweight writing and structured note systems.
5. Scale to larger note folders without turning the renderer brittle.
6. Keep the codebase small, modular, and understandable.

## Core Principles

### 1. State Drives UI

The UI should be rendered from explicit state, not inferred from previous DOM arrangement.

That means:
- pane modes are explicit
- workspace split is explicit
- tab ownership is explicit
- surface ownership is explicit

The DOM should be an output of state, never the hidden source of truth.

### 2. Canonical Ownership

Each major surface should have a clear owner:
- one CodeMirror instance per workspace pane
- one WYSIWYG instance per workspace pane
- one standalone surface container per pane
- one split layout container per pane

No surface should have ambiguous "home" containers.

### 3. Separate Standalone and Split Layouts

Standalone `markdown`, `preview`, and `wysiwyg` should not rely on split slots.
Split view should be its own rendering path.

This is essential for robustness.

### 4. Files Stay As Files

The architecture should remain compatible with plain Markdown folders:
- no hidden database dependency
- no complex metadata requirement
- file paths remain stable identity keys

### 5. Performance By Structure

The app should not depend on later optimization tricks to feel fast.
The architecture itself should reduce unnecessary work:
- scoped rerenders
- incremental tree updates
- independent debounce pipelines
- explicit ownership of expensive operations

## Target High-Level Architecture

### Main Process

Responsibilities:
- Window lifecycle
- menus and app commands
- file and folder dialogs
- file system access
- file watching
- export flows
- app metadata

Files:
- [src/main/main.js](../src/main/main.js)
- [src/main/preload.js](../src/main/preload.js)

Design direction:
- keep main process thin
- expose narrow, structured IPC methods
- keep file logic deterministic and path-based

### Preload Layer

Responsibilities:
- expose the `window.fjord.*` API
- normalize IPC results
- attach event listeners safely

Design direction:
- prefer structured return shapes over bare booleans
- provide clean unsubscribe behavior
- keep renderer-facing contracts stable

### Renderer

Responsibilities:
- app shell
- state
- pane and workspace layout
- tabs
- editors
- preview
- explorer/tree
- dialogs
- settings UI
- command palette
- document insights

The renderer should be split into modules instead of continuing to grow in one file.

## Proposed Renderer Module Layout

Target structure:

```text
src/renderer/
├── index.js
├── app/
│   ├── bootstrap.js
│   ├── app-shell.js
│   ├── dom.js
│   └── events.js
├── state/
│   ├── app-state.js
│   ├── workspace-state.js
│   ├── tabs-state.js
│   ├── pane-state.js
│   └── settings-state.js
├── workspace/
│   ├── workspace-controller.js
│   ├── pane-layout.js
│   ├── tabs-controller.js
│   ├── tree-controller.js
│   └── focus-controller.js
├── editors/
│   ├── markdown-editor.js
│   ├── wysiwyg-editor.js
│   ├── preview-controller.js
│   ├── editor-surfaces.js
│   └── editor-commands.js
├── features/
│   ├── insights.js
│   ├── search.js
│   ├── command-palette.js
│   ├── images.js
│   └── export.js
├── ui/
│   ├── toolbar.js
│   ├── dialogs.js
│   ├── settings-panel.js
│   └── welcome-screen.js
├── markdown.js
├── settings.js
├── theme.js
└── styles/
    └── main.css
```

This does not need to happen in one large refactor. It can be staged.

## Target State Model

The app should have one canonical state tree.

Example target shape:

```js
const state = {
  workspace: {
    folderPath: null,
    mode: 'single', // 'single' | 'dual'
    focusedPane: 'primary',
  },
  panes: {
    primary: {
      mode: 'markdown', // 'markdown' | 'preview' | 'wysiwyg' | 'split'
      split: {
        editableMode: 'markdown', // 'markdown' | 'wysiwyg'
        previewSide: 'right', // 'left' | 'right'
      },
      activeTabId: null,
    },
    secondary: {
      mode: 'markdown',
      split: {
        editableMode: 'markdown',
        previewSide: 'right',
      },
      activeTabId: null,
    },
  },
  tabs: {
    byId: {},
    allIds: [],
    groups: {
      primary: [],
      secondary: [],
    },
  },
  tree: {
    nodes: [],
    expandedPaths: new Set(),
  },
  ui: {
    toolbarVisible: true,
    sidebarVisible: true,
    insightsOpen: false,
    settingsOpen: false,
    commandDialog: null,
  },
  settings: {},
}
```

## Target Pane Rendering Model

Each workspace pane should have:

1. A standalone surface container
2. A split layout container
3. A stable tab header
4. A stable toolbar

The active pane mode decides which container is visible.

### Standalone Modes

For:
- `markdown`
- `preview`
- `wysiwyg`

Only the standalone container is active.

### Split Mode

For:
- `split`

Only the split layout is active.

The split layout should derive from:
- `editableMode`
- `previewSide`

There should never be arbitrary split slot state beyond that.

## Editor Surface Strategy

### Markdown Editor

Target:
- one CodeMirror instance per pane
- one stable mount target for standalone mode
- one stable mount target for split mode

Approach options:

Option A:
- keep one CodeMirror instance per pane and move it between canonical containers only

Option B:
- create separate markdown hosts for standalone and split, but preserve document/state through controlled synchronization

Recommendation:
- start with Option A, but only if ownership stays explicit and limited to known containers

### WYSIWYG Editor

Target:
- one WYSIWYG instance per pane
- separate canonical mount targets for standalone and split

Design requirement:
- destruction and remounting must be explicit and safe
- round-trip synchronization must be predictable

### Preview

Target:
- preview should be a pure render output
- no hidden internal state
- preview content is regenerated from document content only

Preview surfaces should be separate for:
- standalone preview
- split preview slots

## Data Flow Plan

### File Open Flow

1. User opens a file from tree/palette/recent list
2. Renderer requests content through preload API
3. Tab state is created or activated
4. Active pane state updates
5. Pane layout renders from state
6. Editor/preview surfaces sync from tab content

### Edit Flow

1. User edits Markdown or WYSIWYG
2. Active tab content updates
3. Dirty state updates
4. Preview refresh is scheduled
5. Stats/headings refresh is scheduled
6. Autosave is scheduled independently

These pipelines should be decoupled.

### External File Change Flow

1. Main process watcher emits path change
2. Renderer matches by full path
3. If tab is clean, reload
4. If tab is dirty, mark conflict
5. Tree updates incrementally

## Performance Architecture Plan

### Render Scope

Avoid full UI rerenders where possible.

Prefer targeted updates:
- active pane only
- affected preview only
- affected tree node only
- changed settings fields only

### Independent Debounce Pipelines

Keep separate schedulers for:
- autosave
- preview render
- stats
- headings
- search indexing later

### Incremental Tree Updates

The file tree should move toward:
- path-indexed nodes
- partial refreshes
- lazy expansion rendering

### Search and Command Palette Readiness

Future search performance will be better if the architecture supports:
- cached file list
- cached recent files
- lightweight command registry
- optional background indexing later

## Robustness Plan

### Path-Based Identity Everywhere

Use full paths for:
- active tree highlighting
- tab identity
- watcher updates
- link target resolution
- recent files

Never rely on visible filename labels as unique identifiers.

### Safer IPC

Move toward structured return shapes like:

```js
{ ok: true, data: ... }
{ ok: false, error: { code, message } }
```

This makes failures easier to handle predictably.

### Conflict Handling

Add explicit dirty/conflict states for tabs so external file updates do not silently overwrite local work.

### Recovery

Plan for:
- open tab restore
- focused pane restore
- optional unsaved buffer restore

## Note-System Architecture Considerations

Rísta should support PARA/GTD/Zettelkasten by layering tools on top of plain files.

That means:
- avoid hard-coding one note methodology
- support templates and link helpers as optional features
- keep metadata optional
- support backlinks and note creation as conveniences, not requirements

### PARA Support

Architecture implications:
- good folder navigation
- pinned folders/files
- recent files
- quick open

### GTD Support

Architecture implications:
- good task-list ergonomics
- quick insert commands
- reliable search/filtering later

### Zettelkasten Support

Architecture implications:
- local link graph features
- backlink discovery
- fast note creation from references
- optional note IDs

## Visual Architecture Considerations

The design system should stay centralized in CSS tokens and consistent DOM structures.

### Key rules

- No hardcoded colors in JS
- Theme decisions belong in CSS variables and theme helpers
- Shared UI patterns should use reusable CSS classes, not one-off inline behavior
- Motion should remain minimal and intentional

### UI Surfaces To Standardize

- titlebar controls
- toolbar groups
- workspace headers
- pane labels
- empty states
- dialogs
- settings sections
- command palette
- insights panel

## Testing Architecture Plan

### Documents To Add

- `TEST-CASES.md`
- `ARCHITECTURE.md` or evolve this file into it
- benchmark notes for large files/folders

### Automated Test Layers

1. Unit tests
   For markdown helpers, stats, headings, state transitions

2. Renderer behavior tests
   For tabs, pane mode switching, and tree identity logic

3. Smoke tests
   For boot, folder open, and core flows

### Critical Regression Areas

- standalone mode transitions
- split mode transitions
- workspace split toggles
- WYSIWYG/Markdown switching
- save-as
- file watcher updates
- theme switching

## Migration Plan

This should be implemented in stages.

### Stage 1

- finish stabilizing current pane rendering
- separate standalone and split containers cleanly
- document state transitions

### Stage 2

- split renderer code into modules
- move state helpers out of the main entry file
- centralize surface ownership

### Stage 3

- add tests and regression checklist
- fix path identity and preload listener behavior
- rebuild PDF export correctly

### Stage 4

- optimize preview and tree performance
- add command palette
- add image asset workflow

### Stage 5

- build note-system helpers and templates
- harden WYSIWYG and advanced editing
- polish visual system

## Immediate Architecture Priorities

1. Validate the new standalone vs split rendering path
2. Break [src/renderer/index.js](../src/renderer/index.js) into modules
3. Add `TEST-CASES.md`
4. Fix path-based file identity
5. Fix preload unsubscribe behavior
6. Rebuild PDF export to print document-only output

## Summary

The most important architectural decision for Rísta is this:

Treat the app as a state-driven document workspace, not a DOM-mutating editor shell.

If standalone and split surfaces stay separate, ownership stays explicit, and file identity remains path-based, the app can become both faster and much more robust.
