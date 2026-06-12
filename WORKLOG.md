# Fjordmark Worklog

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
- Added Obsidian-style callout rendering in `src/renderer/markdown.js`
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

Latest verified command:

```bash
npm run build
```

Latest result:
- passes successfully
- re-verified after split state normalization fix

## Files Most Relevant To Continue

- `src/renderer/index.js`
- `src/renderer/styles/main.css`
- `src/renderer/editor.js`
- `src/renderer/markdown.js`
- `src/renderer/settings.js`

## Notes

- Do not trust the old worklog content that described WYSIWYG as complete. That was from a much earlier implementation and is no longer accurate.
- The repo has a dirty worktree with many intentional changes. Avoid resetting unrelated files.
- The user was considering continuing in another Codex chat and asked for this markdown handoff specifically so work can continue there.
