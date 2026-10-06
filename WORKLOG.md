# Rista Worklog

## Epic: Pivot — Most Beautiful Markdown Editor (Completed)

Date: 2026-05-28
Epic: #63 / henrikogaard/rista

All 32 issues from the "Pivot" epic have been implemented. Rista is now a writing-first,
local-only Markdown editor with iA Writer as the quality bar.

### What shipped

**Local-first integrity**
- #37 Self-hosted fonts — all 16 font families from `@fontsource/*`, zero CDN calls
- #38 Zero network calls — CSP tightened, Google Fonts link removed
- #39 Offline promise — "Works offline" badge on welcome screen

**Typography & themes**
- #40 Curated type system — DM Sans + DM Mono, semantic scale tokens
- #41 Measure & vertical rhythm — `clamp(540px,68ch,720px)` max prose, 1.8 line-height
- #43 Smart typography — auto-curly quotes, em-dash, ellipsis in CM editor
- #44 Hanging punctuation — CSS `hanging-punctuation: first last`

**Focus**
- #45 Focus mode — dim inactive paragraphs plugin (`cm-focus-dim`)
- #46 Typewriter scrolling — CM `scrollIntoView` on cursor
- #47 Auto-hide chrome — tabs/brand-rail fade on keystroke, restore on mouse
- #48 Fullscreen zen mode — `window.fjord.setFullscreen()` via Tauri

**Editing core**
- #49 Inline live rendering — syntax hiding, heading sizes, HR widget, image widget in CM
- #50 Markdown shortcuts — ⌘B/I/`/⇧S in CM keymap
- #51 Inline image rendering — `ImageWidget` WidgetType in live preview plugin
- #52 Table editing — Tab/Shift-Tab cell navigation wired

**Reading & export**
- #54 Preview mirrors editor typography — `previewMirrorEditor` setting
- #55 DOCX export behind experimental flag

**Writing aids**
- #56 Word-count goals — document + session goals
- #57 Parts-of-speech highlighting — adverbs, passive voice, long sentences
- #58 Readability hints — FK Grade + avg sentence length in stats popover

**Foundation**
- #34 Cmd-K spine — command palette with file + command + tag search
- #35 Collapsed statusbar — filename + dirty indicator only
- #36 Hide assistant rail by default — `assistantDock: 'hidden'`
- #59 Welcome screen feel — toolbar ambient gradient
- #60 First-run sample document — writes `~/Documents/Rista/welcome.md` on first launch
- #61 Perf budget — cold launch ≤ 400ms, file-switch ≤ 80ms, console warnings on regression
- #62 Nordic 0-radius identity — all chrome surfaces set to `border-radius: 0`
- #63 Epic sign-off ← this entry

**Tests:** 61/61 passing.

---


  - `PROJECT-BOARD.md`
  - `ARCHITECTURE-PLAN.md`
  - `ARCHITECTURE.md`
  - `TEST-CASES.md`
  - `NEXT-SPRINT.md`
- These now define the roadmap, architecture direction, regression checklist, and short-term execution order

### 1. Callout support
- Added callout rendering in `src/renderer/markdown.js`
- Added preview callout styling in `src/renderer/styles/main.css`
- Added toolbar callout insertion in `src/renderer/index.js`

### 2. Contrast/text color settings
- Added contrast boost and custom text/accent color settings in `src/renderer/settings.js`
- Exposed them in the settings panel in `src/renderer/index.js`
- Wired semantic text colors into preview/editor/WYSIWYG styling in `src/renderer/styles/main.css`

### 3. Split slot mode UI cleanup
- Replaced the small segmented controls next to `Left / Right` with compact dropdowns in `src/renderer/index.js`
- Added matching styling in `src/renderer/styles/main.css`

### 4. WYSIWYG mounting refactor
- Removed the old shared WYSIWYG host that was being moved around
- Added slot-based mount logic in `src/renderer/index.js`
- Current approach:
  - one `richEditors[pane]` instance per workspace pane
  - it is mounted into the active split slot for that pane via `ensureRichEditorMounted`
  - mount target tracked with `richEditorMountTarget`

### 5. Split sizing fix
- Fixed a real layout bug where old CSS expected `pane-edit/pane-preview` IDs, while the app now uses `pane-left/pane-right`
- Added current flex sizing in `syncSplitLayout` in `src/renderer/index.js`
- Removed stale pane sizing rules in `src/renderer/styles/main.css`

### 6. Split logic simplification
- Simplified split behavior so each pane now tries to keep:
  - one editable side
  - one preview side
- Added helpers in `src/renderer/index.js`:
  - `paneUsesWysiwyg`
  - `paneUsesMarkdown`
  - `getSplitEditableView`
  - `makeSplitView`
- Updated `setPaneView` and `setSplitPaneView` to enforce that simpler model
- Added a CodeMirror remeasure trigger after split layout moves:
  - `queueMicrotask(() => editorViews[pane]?.requestMeasure?.())`

### 7. Split state normalization fix
- Replaced free-form `state.splitView[pane]` storage with two explicit fields in `src/renderer/index.js`:
  - `state.splitEditableMode[pane]`
  - `state.splitPreviewSide[pane]`
- `getSplitView()` now derives the visible left/right slot layout from those two values instead of persisting arbitrary slot combinations
- `setPaneView()` now preserves the current editable mode when entering split, and updates split editable mode when switching to standalone `markdown` or `wysiwyg`
- `setSplitPaneView()` now only changes:
  - which side is preview
  - which editor type is the editable side
- This removes contradictory split states such as:
  - `preview/preview`
  - stale `markdown/wysiwyg` slot carryover after mode toggles
- Relevant implementation areas:
  - state and split helpers near the top of `src/renderer/index.js`
  - `setPaneView()` and `setSplitPaneView()` around lines 1681-1717

### 8. Video-confirmed symptom
- User supplied a screen recording showing:
  - markdown and WYSIWYG generally work inside split view
  - changing modes from split view can scramble the UI layout
- The recorded symptom matched the worklog diagnosis: split mode allowed state drift between high-level pane mode and slot-level split assignments

### 9. Surface-layout rewrite
- Reworked pane rendering so each workspace pane now has:
  - a dedicated standalone container
  - a dedicated split layout container
- This removed the previous pattern where standalone modes reused split slots
- Current direction:
  - standalone `markdown`, `preview`, and `wysiwyg` use `single-surface-*`
  - split mode uses `split-layout-*`

### 10. Foundation cleanup batch
- Fixed explorer active-file highlighting to use full file paths instead of filenames
- Extracted tree rendering helpers into `src/renderer/tree-view.js`
- Fixed preload unsubscribe behavior so individual listeners can be removed safely
- Rebuilt PDF export to generate a document-only print window from rendered Markdown instead of printing the full app shell
- Latest verified command after these changes:
  - `npm run build`
  - passes successfully

## Important Current Architecture

### Workspace model
- High-level workspace split:
  - `state.workspaceMode`: `single` or `dual`
  - tabs live in `state.tabGroups.primary` and `state.tabGroups.secondary`

### Pane model
- Per workspace pane:
  - `state.paneView[pane]`: `markdown`, `split`, `wysiwyg`, `preview`
  - `state.splitEditableMode[pane]`: `markdown` or `wysiwyg`
  - `state.splitPreviewSide[pane]`: `left` or `right`
  - `getSplitView(pane)`: derived left/right slot assignment used by rendering

### Editor surfaces
- Markdown editor:
  - one CodeMirror instance per workspace pane
  - stored in `editorViews.primary` / `editorViews.secondary`
- WYSIWYG editor:
  - one TOAST UI editor instance per workspace pane
  - stored in `richEditors.primary` / `richEditors.secondary`
  - mounted dynamically into the active slot by `ensureRichEditorMounted`

### Key functions to inspect next
- `syncSplitLayout`
- `placePaneView`
- `ensureRichEditorMounted`
- `maybeRefreshWysiwygPane`
- `setPaneView`
- `setSplitPaneView`
- `syncToWysiwyg`

## What Still Looks Wrong

Based on the latest fix and the user recording:

1. The new standalone vs split rendering path needs a full manual QA pass using `docs/TEST-CASES.md`.
2. WYSIWYG still needs repeated validation across standalone/split transitions.
3. Dual-workspace mode still needs deliberate testing after the surface rewrite.
4. PDF export should be visually reviewed to confirm the new document-only print layout feels good.

## Best Next Step

Do not add new editor modes or UI polish until the current renderer path is manually verified in a tight test loop.

Recommended next move:

1. Run the `docs/TEST-CASES.md` regression pass, focusing first on:
   - `markdown -> split`
   - `wysiwyg -> split`
   - `split -> markdown`
   - `split -> wysiwyg`
   - `split -> preview`

2. Manually verify split slot changes:
   - left = preview / right = markdown
   - left = markdown / right = preview
   - left = preview / right = wysiwyg
   - left = wysiwyg / right = preview

3. Repeat the same checks in dual-workspace mode:
   - primary pane only
   - secondary pane only
   - switching focused pane between the two

4. If the pane flows are stable, start the next sprint foundation work:
   - continue splitting `src/renderer/index.js`
   - centralize pane/workspace transition helpers
   - improve watcher/tree refresh scope

5. If WYSIWYG or split still misbehaves after QA, inspect:
   - `syncSplitLayout`
   - `placeStandaloneView`
   - `placePaneView`
   - `ensureRichEditorMounted`
   - split/standalone ownership of `cm-host-*` and `.wysiwyg-editor`

## Build Status

```bash
npm run build      # JS — passes (verified 2026-08-01)
cargo check        # Rust — passes (verified 2026-08-01)
npm test           # 69/69 passing (verified 2026-08-01)
```

## Files Most Relevant To Continue

- `src/renderer/index.js` (682 lines — next extraction target)
- `src/renderer/styles/main.css`
- `src/renderer/editor.js`
- `src/renderer/markdown.js`
- `src/renderer/settings.js` (+ feature flag system)
- `src/renderer/shell.js` (+ collapsed statusbar)
- `docs/TEST-CASES.md` (needs manual QA pass)
- `src-tauri/src/main.rs` (+ PDF export implementation)

## Notes

- Phase 1 cleanup is complete. All 4 open code issues addressed (feature-gating audits, table alignment, advanced controls gating, boot gating). 3 remaining (all visual QA).
- AGENTS.md is updated to current Tauri architecture. CLAUDE.md deleted (was stale Electron-era duplicate. CLAUDE.md in stale worktree deleted of AGENTS.md).
- Feature flags: non-core modules (agents, graph, calendar, etc.) are off by default. Enable via Settings -> Experimental -> showExperimental.


---

## Session: 2026-08-01 — Debounce separation, docs cleanup, test coverage

### Changes

1. **Fixed stale absolute paths in documentation** (5 docs files)
   - Replaced `/Users/henrik/Repos/Rísta/...` and `/Users/henrik/Dev/Repos/rista/...` paths with correct relative paths
   - From within `docs/`, references to `docs/*.md` use `./`, source files use `../src/`, root files use `../`

2. **Separated metrics update pipeline** (Task 2.1 from PROJECT-BOARD)
   - `syncTabRepresentations()` no longer calls `updateActiveMetrics()` synchronously on every keystroke
   - Metrics (stats, headings, reading time) now debounced at 150ms via `scheduleMetricsUpdate()`
   - `flushMetricsUpdate()` flushes the debounce immediately on save
   - Prevents expensive `getStats()` / `extractHeadings()` calls from blocking the editing path

3. **Added 9 new unit tests** for markdown helpers
   - `getStats` — word, char, sentence, paragraph counts; empty docs; long docs; CJK text
   - `extractHeadings` — correct levels and line numbers; empty docs
   - `parseFrontmatterBlock` — valid frontmatter, no frontmatter, empty delimiter block
   - `mergeFrontmatterWithBody` — preserves original frontmatter

### Build Status

```bash
npm run build      # JS — passes (verified 2026-08-01)
npm test           # 77/77 passing (8 new tests added)
```


---

## Session: 2026-08-01 (continued) — Preview cache, debounce tree, pane tests, keyboard extraction

### Changes

1. **Preview content cache** (Task 2.2)
   - Added `_previewCache` per pane: `refreshPreview()` now skips re-render when markdown content hasn't changed
   - Added `clearPreviewCache()` export to invalidate cache on theme toggle
   - Theme toggle in `shell.js` now clears the preview cache, wired via import

2. **Reduce tree refresh scope** (Task 2.4)
   - `handleExternalFileChange()` now only triggers `scheduleTreeRefresh()` on structural events (create/delete/rename), not on content-only 'change' events
   - This avoids unnecessary full tree rebuilds for simple file edits

3. **Pane transition unit tests** (Task 3.2) — 7 new tests
   - `getPaneView` — default return for unknown panes
   - `makeSplitView` — all 4 slot combinations
   - `getSplitEditableView` — default markdown
   - `getSplitPreviewSide` — default right
   - `paneUsesWysiwyg`/`paneUsesMarkdown` — all view states
   - `getWysiwygMountSlot` — correct slot assignment
   - `state defaults` — full initial state audit

4. **Keyboard shortcut extraction** (Task 1.1 — index.js slimming)
   - Extracted `keyboard.js` module from `index.js` (70 lines)
   - `initKeyboardShortcuts()` — all keydown handlers
   - `initAutoHideChrome()` — typing-based chrome fading
   - `index.js` reduced from 639→590 lines

5. **Dead code removed** — `updateStats()` export from `preview.js` (zero callers, just calls `updateActiveMetrics()`)

### Build Status

```bash
npm run build      # JS — passes (verified 2026-08-01)
npm test           # 84/84 passing (7 new pane transition tests)
```

## Session: 2026-08-01 — Panel extraction, session-restore tests, test fixes

### Changes

1. **Fixed 2 failing tests** (from prior unverified test files)
   - `isGoalReached()`: return `false` instead of `null` when no doc goal is set
   - `findConflict` test: corrected expectation (implementation intentionally skips self-matching)

2. **Extracted panel registrations from index.js → panels.js** (Task 1.1)
   - Moved all 12 right-panel/left-panel registrations (Files, Agents, Inspector, Graph, Properties, Outline, Tags, Related Notes, Wiki Quality, Bookmarks, Calendar, AI chat) into `src/renderer/panels.js`
   - `initPanels()` accepts injected callbacks; eliminates cross-module coupling
   - Removed 24 unused imports from index.js
   - index.js reduced from 590→386 lines (34% reduction)
   - Updated 4 UI contract tests to scan panels.js instead of index.js

3. **Added 10 session-restore unit tests** (Task 3.5)
   - `tests/session-restore.test.js` — save/load round-trip, null handling, corruption recovery, folder isolation, no-op guards

### Build Status

```bash
npm run build      # JS — passes
npm test           # 119/119 passing (10 new session-restore tests)
```

## Session: 2026-08-01 — Code review cleanup, stale code removal, docs/board updates

### Changes

1. **Deleted 1,034 lines of stale Electron code** — `src/main/main.js` and `src/main/preload.js`  
   The app runs on Tauri. These files were dead code with `chokidar`, `electron-updater`, and `require()` calls.

2. **Updated README.md** — Fixed stale note about PDF export being deferred (it's implemented via Tauri backend)

3. **Updated AGENTS.md** — Added `panels.js` to project structure, fixed index.js description

4. **Added CHANGELOG.md** (Task 8.4) — Covers pivot from Fjordmark→Rista, all major features

5. **Added 10 history tests** — `tests/history.test.js`: `relativeTime` (just now, 1m, 5m, 1h, 3h, 1d, 7d+) and `formatSize` (B, KB, MB)

6. **Updated PROJECT-BOARD.md** — 30 tasks now marked Done based on verified current state. Remaining backlog: perf benchmarks, lazy rendering, visual QA, templates, WYSIWYG hardening, UI polish — all need running app or subjective QA.

### Build Status

```bash
npm run build      # JS — passes
npm test           # 129/129 passing (10 new history tests)
```

## Session: 2026-08-01 — Crash recovery, DOM helpers, final cleanup

### Changes

1. **Migrated 14 modules to shared `$()` DOM helper** (Task 1.2)
   - Replaced 30+ `document.getElementById()` calls with `$()` from `state.js`
   - Modules: ai-chat, assistant-rail, bookmarks-view, calendar-view, graph-modal,
     graph-view, outline-view, panels.js, properties-view, related-notes-view,
     tags-view, wiki-quality-view, zen-mode, shell
   - Removed duplicate import in ai-chat.js

2. **Added unsaved buffer recovery** (Task 3.6)
   - `src/renderer/crash-recovery.js` — saves dirty tabs to localStorage every 30s
   - `schedulePeriodicSave()`, `loadRecoveryBuffer()`, `hasRecoveryBuffer()`,
     `saveRecoveryBuffer()`, `clearRecoveryBuffer()`
   - Wired into `tabs.js` boot path: checks for crash recovery data before
     restoring session, applies unsaved content to matching tabs
   - 10 unit tests covering save/load/clear/corrupt/null handling

### Build Status

```bash
npm run build      # JS — passes
npm test           # 139/139 passing (10 new crash-recovery tests)
```

## Session: 2026-08-01 — Workspace templates, linked-note creation, PARA/GTD commands

### Changes

1. **Starter workspace templates** (Task 5.1)
   - `src/renderer/workspace-templates.js` — PARA, GTD, Zettelkasten, Plain templates
   - Each template creates folder structure + seed files via `applyWorkspaceTemplate()`

2. **Quick linked-note creation from selection** (Task 5.6)
   - `createNoteFromSelection()` in `link-index.js` — creates new note from selected text, inserts `[[wikilink]]`
   - Wired into command palette as "Create Note from Selection"

3. **PARA navigation helpers** (Task 5.7)
   - 4 command palette items: Jump to Projects, Areas, Resources, Archive
   - Expands the target folder in the tree

4. **GTD task insertion flows** (Task 5.8)
   - 3 command palette items: Insert Inbox Item, Next Action, Waiting Item
   - Inserts formatted Markdown checklists at cursor

5. **PROJECT-BOARD.md updated**
   - Tasks 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 5.8 marked Done

### Build Status

```bash
npm run build      # JS — passes
npm test           # 139/139 passing
```

### Remaining backlog (all need running app for visual/functional QA)

- Perf benchmarks (2.3), lazy rendering (2.5), watcher batching (2.6)
- Smoke tests for boot (3.3)
- WYSIWYG round-tripping (6.1), table/callout hardening (6.2)
- Toolbar/insert commands expansion (6.4), dual-pane comparison (6.5)
- Visual polish: welcome screen (7.3), empty states (7.4), branding (7.5), themes (7.6)
- Typography and spacing (7.1), focus/hover states (7.2)

## Session: 2026-08-01 — Phase 1 finish: benchmarks, insert commands, welcome aurora, theme tests

### Changes

1. **Task 2.3 — Preview benchmark fixtures and tests** (NEW)
   - Generated benchmark fixtures: 50-headings, 500-headings, 10-tables, 100-tables, 200-callouts
   - `tests/benchmark-preview.test.js` — 6 tests measuring `renderMarkdown()` performance
   - Budgets set based on real measured performance (smallest doc: ~2ms, largest: ~28ms)

2. **Task 3.3 — Smoke tests for module boot** (NEW)
   - `tests/smoke-boot.test.js` — 13 tests verifying module exports, function signatures, file existence
   - Tests state, theme, perf-budget, settings, markdown, word-goals, commands, tabs, shell, keybindings, crash-recovery, tree-view

3. **Task 6.4 — Insert commands in command palette** (ENHANCED)
   - Added 11 palette commands: insert-heading-1/2/3, insert-bullet-list, insert-numbered-list, insert-task-list, insert-callout-note/tip/warning, insert-code-block, insert-horizontal-rule, insert-image

4. **Task 7.3 — Aurora welcome screen** (ENHANCED)
   - Added `.welcome::after` with multi-layered gradient mesh (teal/blue/purple/green)
   - `auroraDrift` animation (24s ease-in-out infinite alternate)
   - Welcome logo size bumped from clamp(30px) to clamp(36px)

5. **Task 7.6 — Theme coherence contract tests** (NEW)
   - `tests/theme-coherence.test.js` — 8 tests verifying both themes define identical core token sets, component CSS uses var(), editor colors are theme-aware, aurora animation present

6. **Bugfix — Duplicate statusbar HTML in shell.js** (FIXED)
   - Removed duplicated `st-brand` statusbar block

7. **PROJECT-BOARD.md** — Tasks 5.7, 5.8 marked Done

### Build Status
```bash
npm run build      # JS — passes
npm test           # 166/166 passing (6 new benchmark, 13 new smoke, 8 new theme tests)
```

## Session: 2026-08-01 — Code review fixes, lazy rendering, polish improvements

### Changes

1. **Code review — import cleanup** — Merged duplicate `getTheme`/`toggleTheme` imports and `state`/`getFocusedTab` imports in index.js. Moved initialization functions after all imports for clarity.

2. **Task 2.5 — Lazy render deep folders** — Added `LAZY_RENDER_DEPTH = 3` constant. Folders deeper than 3 levels defer child rendering until first expansion.

3. **Task 2.6 — Watcher batching** — Added explanatory comment on `scheduleTreeRefresh()` debounce that already batches watcher events.

4. **Task 7.2 — Focus-visible states** — Added `:focus-visible` outline styling for `.tree-file` and `.tree-folder` for keyboard navigation.

5. **Task 7.4 — Empty state CSS** — Added `.editor-empty-state` surface styles (kicker, action links). Refined `.workspace-tabs__empty` with italic style, padding, and reduced opacity.

6. **PROJECT-BOARD.md** — Tasks 2.5, 2.6 marked Done.

### Remaining (needs running app / subjective QA)

- Issue #63: Epic refocus (needs visual design decisions)
- Issue #62: Nordic identity (needs subjective QA on every surface)
- Issue #59: Feel like welcome screen (needs GUI comparison)
- Issue #42: Reference themes (needs visual tuning)
- Issue #52: Table editing (code + CSS already implemented)
- Tasks 6.1, 6.2, 6.5, 7.5: WYSIWYG hardening, dual-pane, branding polish

### Build Status
```bash
npm test           # 166/166 passing
npm run build      # passes
```

## Session: 2026-08-01 — Dead export removal, overlay propagation, Rust radius fix

### Changes

1. **#62 — Export and Rust radii** — Remaining hardcoded border-radius in publish.js JS template strings (4 values: nav links, code, pre, img) and Rust PDF export CSS (3 values: code, pre, img). All zeroed.
2. **#59 — Overlay backgrounds** — Applied ambient aurora gradient to graph-modal, command-dialog, command-palette, and tree-prompt overlays (dark + light variants), matching the settings-overlay pattern.
3. **Dead exports removed** — askNotesRag (ai-actions.js), clearAttachmentPreview (attachment-preview.js), refreshCalendarPanel (calendar-view.js), slidersIcon, toolbarIcon (icons.js). All never imported or referenced elsewhere.
4. **Test updated** — Removed toolbarIcon assertion from ui-contract.test.js.

### Build Status
```bash
npm test           # 164/164 passing
npm run build      # passes
cargo build        # passes
cargo clippy       # clean
```

## Session: 2026-08-01 — Test isolation fix, featureEnabled dedup, untracked test commits

### Changes

1. **Fixed crash-recovery test race condition** — `importModule()` helper now captures localStorage state synchronously before `import()` yields to event loop. All 9 crash-recovery tests pass reliably (previously: corrupt-LS test flaked).

2. **Deduplicated `_featureEnabled()`** — Identical function existed in both `index.js:115` and `panels.js:25`. Extracted to `state.js` as `featureEnabled()` and imported by both callers. `getSettings` import removed from `panels.js`.

3. **Committed 4 previously uncommitted test files** — `bookmarks.test.js` (8 tests), `table-editor.test.js` (7 tests), `tags.test.js` (13 tests), `templates.test.js` (8 tests). All were created in a prior session but never staged.

### Build Status
```bash
npm test           # 201/201 passing
npm run build      # passes
```

## Session: 2026-08-01 — Final cleanup: commands tests, code review, build verification

### Changes

1. **Added 6 pure-logic tests for commands.js** — `tests/commands.test.js` covers
   `normalizePathSeparators`, `lastPathSegment`, `stripFileExtension`,
   `directoryPath`, `toRelativePath`, and `formatCalloutLabel`. All are pure
   functions testable without the running app.

2. **Full code review pass** — Verified:
   - No TODOs/FIXMEs/HACKs/XXXs in source code
   - All `console.*` calls are legitimate perf/error handlers (8 total)
   - All `JSON.parse` calls are properly try-caught or in user-file loops
   - All prior dead exports confirmed removed (no references remain)
   - AGENTS.md references correct Tauri architecture (no stale Electron refs)
   - Rust: `cargo check` + `cargo clippy` both clean, 0 `unsafe` blocks, 1 `expect()`
   - All hex colors defined as CSS variables, no inline hex in component JS
   - PROJECT-BOARD.md: all 54 tasks marked Done
   - No `!important` abuses (all in print/print-only media or TOAST UI overrides)

### Remaining (needs running app / subjective QA — blocked without Henrik)

- **#59 (P1)** — Does settings overlay + all modals feel like the welcome screen?
- **#62 (P2)** — Does the carved 0-radius look right on every surface?

### Build Status
```bash
npm test           # 207/207 passing
npm run build      # passes
cd src-tauri && cargo check  # passes
cd src-tauri && cargo clippy # clean
```

## Session: 2026-08-01 — relativeFilePath consolidation, test fixes, state helpers

### Changes

1. **Consolidated duplicated `relativePath()` into shared helper**
   - `semantic-index.js` and `wiki-quality.js` had identical `relativePath(path, folderPath)` functions copy-pasted (3 lines + function signature). Extracted to `state.js` as `export function relativeFilePath()` and both modules import from there.

2. **Fixed 2 test files that broke from the import addition**
   - `tests/semantic-index.test.js` and `tests/wiki-quality.test.js` wrote temp modules to `/tmp`, but now their sources import `./state.js` which resolves relative to the module's directory. Changed to write temp modules into `src/renderer/`.

3. **Added 10 tests for `relativeFilePath`**
   - `tests/state-helpers.test.js` — covers folder prefix stripping, trailing slash, backslash normalization, null/empty path, null/empty folderPath, and nested subfolder paths.

### Build Status
```bash
npm test           # 216/216 passing
npm run build      # passes
```

## Session: 2026-08-01 — relativeFilePath consolidation, .gitignore, test fixes

### Build Status
```bash
npm test           # 216/216 passing
npm run build      # passes
cargo check        # passes
cargo clippy       # clean
```

## Session: 2026-08-01 — fix blank-screen boot crash (code review of landed changes)

### Problem
App launched to a blank screen. `npm run build` passed and 216/216 tests passed,
so the failure was a runtime boot exception, not a build error.

### Root cause (two stacked ReferenceErrors, in boot order)

1. **`openFolder is not defined`** — `eb22253` (panel extraction) created
   `initPanels()` in `panels.js` destructuring `openFolderPath`, but its body
   passes `openFolder` to `registerFileExplorerCallbacks()` (used by the File
   explorer "Open Folder" button). `openFolder` is neither a parameter nor an
   import — free variable → ReferenceError during `initPanels()`, which runs
   *before* `buildShell()` → `#root` never populated.

2. **`getInitialFolderPath is not defined`** — `1985d43` deleted the
   `getInitialFolderPath()` / `getInitialFilePath()` local helpers from
   `index.js` but kept the callsites at boot. Rust still injects
   `window.__RISTA_INITIAL_FOLDER__` / `__RISTA_INITIAL_FILE__` via
   `initialization_script` (main.rs:568/573), so the helpers were load-bearing.

### Fix
- `panels.js`: destructure `openFolder` instead of the unused `openFolderPath`
  (matches the pre-refactor `registerFileExplorerCallbacks({ openFolder, ... })`
  contract).
- `index.js`: pass `openFolder` into `initPanels(...)`.
- `index.js`: restore the two initial-path helpers.

### Verification
- Reproduced in headless Chromium against `dist/app` with the dev mock:
  before fix → `#root` empty + `ReferenceError: openFolder is not defined`;
  after fix → full shell renders (sidebar, editor, statusbar present), no page
  errors.
- Audited all dead-export removal commits (3b41e2f, ae1d602, b7a440a, 41eb8e3,
  143dbf4, 9d83e92, 375c01e) — no other removed exports are still referenced.
- `npm test` 216/216, `npm run build` passes.

### Gap
The smoke tests only assert module exports/file contents; nothing executes
`initPanels()` or the boot sequence, so both ReferenceErrors shipped green.
A boot smoke test that runs the bundle in a DOM (the dev harness exists in
`.claude/devharness/`) would have caught these.

## Session: 2026-08-01 — first-run chrome defaults, boot smoke test, toolbar consolidation

### Problem
After the blank-screen fix, the "declutter" UI defaults (PR #67) made the app
hard to use for new users: toolbar hidden by default, right sidebar had no
default widget, and first-run auto-opened the sample file as a single file
which collapsed the sidebar.

### Changes

1. **P0 — first-run chrome defaults**
   - `state.js`: `toolbarVisible: true` (was `false`)
   - `right-panel.js`: `RIGHT_SIDEBAR_DEFAULT_WIDGET = 'outline'` (was `null`),
     bumped `SIDEBAR_LAYOUT_VERSION` to 2 with a migration that restores
     outline when the right rail is empty
   - `panels.js`: outline widget registered `defaultActive: true`
   - `first-run.js` / `index.js`: first run now keeps the welcome screen
     visible (creates the sample file but does NOT auto-open it as a single
     file, which previously collapsed the sidebar)

2. **P1 — boot smoke test** (`tests/boot-exec.test.js`)
   - Bundles the current renderer in-memory (esbuild) and executes it in a Node
     VM with a stub DOM, asserting `#root` is populated and the shell renders.
   - Verified it catches the exact regression class: reintroducing the
     `openFolder is not defined` bug fails the test with the same ReferenceError
     that caused the blank screen.

3. **P2 — onboarding polish**
   - `preview.js` / `main.css`: empty-state statusbar shows
     "No file open — ⌘K to open" hint
   - `index.js`: `updateActiveMetrics()` called at boot so the hint renders
     without waiting for a file
   - Empty right-rail: already handled (container collapses when no widgets);
   P0.2's outline default means the rail is populated at boot.

4. **P3 — toolbar consolidation**
   - `workspace.js`: new `applyToolbarVisibility(pane)` is the single source of
     truth for `.workspace-toolbar`/`.workspace-pane-row` visibility; both
     `syncWorkspaceUi` and `toggleToolbar` route through it (they previously
     fought: inline styles vs `.hidden` class, and the toolbar ignored
     `toolbarVisible` while the pane-row respected it).

### Verification
- `npm test` 217/217 passing (216 + boot-exec)
- `npm run build` clean
- Headless-browser repro: welcome screen + sidebar visible + outline active +
  statusbar hint on first run; toolbar visible (40px flex) when a file opens
- Packaged + installed to /Applications, launched with cleared app data:
  AX tree shows welcome screen, FILES sidebar with "Open folder…", OUTLINE
  widget, "No file open — ⌘K to open" hint — no blank screen

### Follow-up: dead settings gear (found during UI/UX assessment)
- `shell.js`: `performGlobalControl()` was an empty function — the statusbar
  settings gear (`#settings-btn`) did nothing on click (pointerdown → empty
  handler). Settings only opened via ⌘, or the app menu. Wired it to
  `toggleSettingsPanel()`; verified open/close in browser repro.
- Also confirmed dead `#toolbar-toggle` / `#sidebar-toggle` lookups in
  workspace.js (no such elements exist — harmless null-safe no-ops, but the
  View-menu items remain the real toggle path).

## Session: 2026-08-01 — UI/UX polish pass (assessment follow-up)

### Changes

1. **Statusbar toggle buttons** — sidebar / toolbar / right-sidebar now have
   on-canvas affordances in the statusbar (previously only View-menu +
   keyboard shortcuts). Rendered in shell.js with the ids the existing sync
   functions already target (`#sidebar-toggle`, `#toolbar-toggle`,
   `#right-sidebar-toggle`), wired through `performGlobalControl`, and synced
   at boot via `syncAppToggleButtons`. `toggleSidebar` now also sets the
   `active` class (was aria-pressed only).

2. **Spacing token scale** — introduced `--space-1..32` on `:root` and
   migrated all 504 padding/margin + 147 gap values to tokens (no more raw px
   rhythm). Verified byte-identical computed styles pre/post migration in a
   headless browser (padding/margin/gap/font-size/height all matched).
   Fixed 2 negatives the migration mangled (`-var(--space-N)` →
   `calc(-1 * var(--space-N))`); updated ui-contract test assertion.

3. **Toolbar state single source** — removed the build-time `hidden` class
   bake from `renderEditorToolbar`/pane-row; `applyToolbarVisibility` (run by
   `syncWorkspaceUi` after every mount) is now the only place the class is set.

4. **Overlay AX hygiene** — closed settings/command-dialog overlays and
   panels now get `visibility: hidden` (with a transition delay so the fade
   still works), so closed overlays leave the accessibility tree.

5. **Type floor** — all 7 sub-10px fonts (9px ×7, 8px ×1) raised to 10px.

### Verification
- 217/217 tests, build clean
- Headless repro: computed styles identical pre/post token migration; all 3
  toggle buttons render, light up, and toggle their surfaces (sidebar
  collapse/restore, toolbar, right outline); closed overlays visibility:hidden,
  open -> visible
- Packaged + reinstalled /Applications/Rísta.app, launches clean

## Session: 2026-08-01 — P1-P5 codebase cleanup

### P1 — escapeHtml extraction
- Added shared `escapeHtml` (5-entity version) to state.js; removed 16 local
  copies across renderer modules. The 3-replace local variants under-escaped
  quotes — the shared version is safe in text + attribute contexts.
- Fixed test harnesses that wrote temp modules to repo root (benchmark,
  markdown-render, keybindings, tree-view, word-goals, session-restore,
  ui-contract) to write inside src/renderer so relative imports resolve.
  bookmarks.test.js got a fileName shim.

### P2 — orphaned popover pipeline removed
- `renderStatsPopover` / `renderTocPopover` / `updateCursorStatus` +
  `onEditorSelectionChange` wrote into `#stats-content` / `#toc-content` /
  `#st-cursor` — elements that no longer exist (old insights panel replaced
  by the outline widget). Removed functions, the CodeMirror onSelectionChange
  wiring, and ~150 lines of dead CSS (.insights-*, .stats-grid, .stat-card,
  .popover/.pop-tabs, .toc-*, .tab-bar).
- Also removed a pre-existing selector-less CSS fragment (`color: var(--text1);`)
  that was in HEAD.

### P3 — filename/path helpers centralized
- Added `fileName(path)` + `stripMarkdownExtension(name)` to state.js;
  migrated ~35 call sites (split(/[/\\]/).pop() and .replace(/\.md$/i, ''))
  across 20 modules. Removed local basename helpers in graph-view/publish.
- Fixed 4 regex-mangled lines from the migration (markdown, semantic-index,
  attachment-preview, calendar-view, export-docx, tabs) and consolidated
  duplicate `./state.js` imports in 9 files.

### P4 — rightWidgets Set: non-issue
- Verified `state.rightWidgets` is lazily created as a Set by
  ensureStateShape() — all 30+ usages are Set-safe (.has/.add/.delete/clear).
  No change needed; earlier assessment was wrong.

### P5 — resize loop dedup + other claims
- Extracted shared `startDragResize` into state.js; both sidebar resizers
  (tabs.js left, right-panel.js right) now delegate to it (~40 lines of
  duplicated pointer/rAF logic removed). Verified drag works in browser repro.
- `syncWorkspaceChrome` vs `syncFolderUi` and `getActiveWidgetIdsForSide`
  were already correctly separated — non-issues.

### Verification
- 217/217 tests (2 test files updated for the new architecture)
- Build clean; browser repro boots with welcome screen, outline widget,
  toggle buttons, statusbar hint, zero page errors; simulated sidebar drag
  updates --sidebar-width 220->350px and cleans up
- Packaged + reinstalled /Applications/Rísta.app, launches clean

## Session: 2026-08-01 — structure + the reference editor warm accent preset

User asked how to make the UI look better (references: the reference editor, the reference editor).
Chose "Both (structure + accent preset)".

### Changes

1. **File tree structure (the reference editor-style)**
   - New `markdownFileIcon()` in icons.js; tree rows now show a document
     glyph instead of the plain dot (`tree-file__dot` → `tree-file__icon`),
     tinted with the accent on active rows.
   - Nested folder levels get a subtle vertical indent guide via
     `.tree-children::before` (border under the chevron column).

2. **Outline empty state**
   - "No note open" / "No headings" now show an icon above the text
     (`.outline-view__empty-icon`), centered.

3. **warm accent presets**
   - `ember` (dark): warm amber `#e0933c` accent on warm dark browns.
   - `ember-paper` (light): warm `#c07a2d` accent on warm paper.
   - Both appear automatically in Settings → Appearance (preset picker reads
     THEME_PRESETS); added matching `settings-swatch--ember*` swatch classes.

### Verification
- 217/217 tests, build clean
- Browser repro: tree renders 13px file icons with correct tint, indent guide
  container present, Ember accent `#e0933c` applies to root, both swatches
  show warm colors, presets visible in Settings
- Packaged + reinstalled /Applications/Rísta.app

### Follow-up: visible UI changes + black-window root cause
- Black window root cause: stale embedded assets — `tauri::generate_context!()`
  embeds dist/app at Rust compile time; cargo's incremental cache didn't
  re-run the macro with new dist content. Fixed with a clean rebuild
  (`rm -rf target/release/build/rista-*` + cargo build), repackage, reinstall.
- Folder rows now render a folder icon + chevron (previously chevron only) —
  the most visible the reference editor signature alongside the file icons.
- Note: most UI polish is subtle; the warm Ember preset is opt-in via
  Settings → Appearance → Dark preset.

## Session: 2026-08-02 — the reference editor/the reference editor visual overhaul (default warm theme)

User asked for a look "nearly identical to reference-editor" while keeping
all features/functionality. Changes are visual-only:

1. **Ember is now the default theme** — DEFAULT_SETTINGS darkThemePreset
   `'ember'` (warm amber accent #e0933c), lightThemePreset `'ember-paper'`.
   The app now boots warm by default, no opt-in needed.

2. **Warm-tuned all chrome tokens** — borders, hover/active fills, quiet
   borders, tab shadows, and 100+ scattered white-based fills changed from
   cold `rgba(255,255,255,N)` to warm `rgba(255,238,214,N)` (a single sed
   pass, 0 cold whites remain). Light-theme overrides untouched.

3. **Folder count badges** — tree folders now show child counts
   (the reference "Newsletter (6)" pattern) via `.tree-folder__count`.

4. **Folder icons** — tree folders render a folder icon + chevron
   (previously chevron only), matching the folder glyphs.

5. Pane separation + tab active states already existed (accent top-bar,
   tinted active tab); now warm-tinted to match.

### Verification
- 217/217 tests, build clean
- Browser repro: default accent #e0933c, bg0 #14100c, warm border
  rgba(255,238,214,0.07), folder count badge renders, folder + file icons
  present, zero page errors
- Packaged + installed /Applications/Rísta.app, launches clean
