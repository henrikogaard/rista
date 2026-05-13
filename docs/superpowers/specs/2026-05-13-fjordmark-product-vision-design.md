# Fjordmark Product Vision

**Date:** 2026-05-13
**Status:** Approved
**Scope:** Full product feature roadmap for Fjordmark, a local-first Markdown editor

---

## Identity

Fjordmark is a **beautiful, focused markdown editor**. It is not a knowledge base, not a PKM tool, not a second brain. It edits markdown files in a folder. The design language is Nordic/Scandinavian -- dark, minimal, purposeful. No cloud, no accounts, no telemetry. Files are just files.

**General-purpose audience:** writers, developers, students, anyone who works with Markdown. Features should benefit the broadest set of users; niche additions (Vim mode, LaTeX) are opt-in via settings.

---

## Current state

### What works

- CodeMirror 6 markdown editor with custom highlight theme (dark + light)
- Four view modes: Markdown, Split, WYSIWYG (Toast UI), Preview
- Dual-pane workspace with tab dragging between panes
- Auto-save (800ms debounce)
- File tree sidebar (collapsible, resizable)
- File watching via Chokidar
- Toolbar: undo/redo, headings, lists, blockquote, callouts, bold/italic/strike/code, link, image, table
- Find & Replace (custom implementation)
- PDF export via hidden BrowserWindow + printToPDF
- Image paste from clipboard (base64 data URLs)
- Mermaid and D2 diagram rendering with theme-aware caching
- Comprehensive settings panel (atmosphere, interface, editor, preview fonts/colors)
- Dark/light theme with full CSS variable architecture
- Welcome screen with aurora gradient
- Insights panel (word/char/paragraph stats, TOC)
- Status bar (mode, word count, line count, cursor position)
- Native app menu with keyboard shortcuts

### What's broken or incomplete

- **WYSIWYG mode** is unreliable -- does not consistently mount or sync
- **`index.js` is 2317 lines** -- monolithic, all UI logic in one file
- **Image paste uses base64** -- bloats `.md` files, not portable
- **`@codemirror/search` not used** -- find/replace is a custom implementation (works, but consider migrating to CM's native search for regex support and maintainability -- not urgent)

---

## Phase 0 -- Stabilize

Prerequisite work. No new features until these are resolved.

### 0.1 Fix or remove WYSIWYG mode

The Toast UI Editor integration is unreliable. Two options:

**Option A (recommended): Fix it.** Audit the mount/unmount lifecycle, ensure proper sync between CM and Toast UI via `getMarkdown()`/`setMarkdown()`, handle pane switching and tab switching edge cases, match themes properly. Add integration tests via TEST-CASES.md.

**Option B: Remove it.** Strip Toast UI entirely, remove the WYSIWYG view option. Re-approach later with a different strategy (e.g., ProseMirror-based, or CM decorations that provide inline rendering). This simplifies the codebase significantly but removes a headline feature.

**Decision:** Fix it (Option A). WYSIWYG is a meaningful differentiator. If it proves intractable, fall back to Option B.

### 0.2 Split `index.js` into modules

Break the 2317-line monolith into focused modules. Proposed structure:

```
src/renderer/
  index.js          # App entry, state initialization, module wiring
  state.js          # State object, accessor helpers, state change notifications
  tabs.js           # Tab creation, switching, closing, pinning, context menu
  panes.js          # Pane management, split/dual workspace, resizers
  toolbar.js        # Toolbar construction, command dispatch
  sidebar.js        # File tree rendering, context menu, resize
  dialogs.js        # Command dialog (link/image/table), command palette
  shortcuts.js      # All keyboard shortcut registration
  statusbar.js      # Status bar construction and updates
  welcome.js        # Welcome screen rendering
  find-replace.js   # Find & Replace bar logic
```

The `state` object stays shared. Each module exports an `init(state)` function that constructs its DOM and wires its events. Modules communicate via a simple event bus or direct function calls -- no framework.

### 0.3 Image paste to `_assets/`

Replace base64 data URL insertion with:

1. On paste/drop of an image, determine the directory of the current `.md` file
2. Create `_assets/` subfolder if it doesn't exist (IPC: `createDir`)
3. Write the image file with a timestamped name (e.g., `image-2026-05-13-143022.png`)
4. Insert `![image](relative-path-to-assets/filename.png)` at cursor
5. Same logic for drag-and-drop images (Phase 1)

Requires new IPC handlers:
- `window.fjord.createDir(path)` -- create directory
- `window.fjord.writeImageFile(dirPath, buffer, filename)` -- write binary file

---

## Phase 1 -- Core editing polish

### 1.1 Command palette (Cmd+K)

A floating search/command input that unifies file navigation and editor commands.

**Sources:**
- All `.md` files in the current project (fuzzy match on filename and relative path)
- Editor commands: Toggle View, Toggle Sidebar, Toggle Toolbar, Insert Table, Insert Link, Insert Image, Export PDF, Toggle Theme, Toggle Zen Mode, etc.
- Recently opened files (weighted higher in results)

**Behavior:**
- Trigger: `Cmd+K` (Mac) / `Ctrl+K` (Win/Linux)
- Centered floating overlay, 500px wide, dark glass background
- Text input at top, results list below (max 10 visible, scrollable)
- Fuzzy matching with highlight on matched characters
- Arrow keys navigate, Enter selects, Escape dismisses
- Typing filters in real-time
- Commands prefixed with `>` to distinguish from file search (e.g., `>export pdf`)

**UI:**
- Same borderless aesthetic as the rest of Fjordmark
- Results show: icon (file/command), name, path/description, keyboard shortcut if applicable
- Subtle accent highlight on selected item

### 1.2 Zen / focus mode

Hides all chrome and centers the writing area for distraction-free writing.

**What gets hidden:** sidebar, toolbar, tab bar, pane headers, status bar, insights panel, find/replace bar.

**What remains:** the editor, centered in a single column (max-width 700px, centered horizontally).

**Optional paragraph dimming:** paragraphs other than the one containing the cursor are rendered at reduced opacity (0.3). Controlled by a setting, off by default.

**Toggle:** `Cmd+Shift+Enter` (Mac) / `Ctrl+Shift+Enter` (Win/Linux), also available via command palette.

**Exit:** press Escape, or use the same shortcut again. A subtle "Exit Zen Mode" hint appears in the top-right corner on mouse movement, fades after 2 seconds.

**Behavior:**
- Only the focused pane enters Zen mode (in dual-pane, the secondary pane is hidden)
- View mode is forced to edit-only (no split/preview) while in Zen
- Scroll position and cursor preserved on enter/exit
- Auto-save continues to work

### 1.3 Typewriter scrolling

Keeps the active line vertically centered in the viewport as the user types.

**Behavior:**
- When enabled, after each keystroke or cursor movement, the view scrolls so the cursor line is at the vertical center of the editor viewport
- Smooth scroll animation (CSS `scroll-behavior: smooth` or CM's `scrollIntoView` with margin)
- Only active during editing, not when manually scrolling (re-engages on next keystroke)

**Setting:** toggle in Settings > Editor, off by default. Some users find it disorienting.

**Scope:** active in edit and split modes, not in preview-only.

### 1.4 Drag-and-drop images

Drop image files from the OS file manager onto the editor.

**Behavior:**
- Listens for `dragover`/`drop` events on the CM host div
- Accepts image MIME types (png, jpg, gif, webp, svg)
- Saves to `_assets/` using the same mechanism as clipboard paste (Phase 0.3)
- Inserts `![filename](relative-path)` at the drop position
- Shows a visual drop zone indicator (subtle border highlight on the editor area) during drag

**Multiple files:** if multiple images are dropped, insert them sequentially with blank lines between.

---

## Phase 2 -- File & project management

### 2.1 Sidebar file operations

Right-click context menu on files and folders in the sidebar.

**File context menu:**
- Rename (inline edit in sidebar)
- Delete (move to OS trash via Electron's `shell.trashItem()`)
- Duplicate (creates `filename-copy.md`)
- Copy Relative Path
- Copy Absolute Path
- Reveal in Finder / Explorer (`shell.showItemInFolder()`)

**Folder context menu:**
- New File (creates untitled.md inside the folder)
- New Folder
- Rename
- Delete (recursive, to trash)
- Collapse/Expand
- Reveal in Finder / Explorer

**New file/folder buttons:** small `+` icons at the top of the sidebar, next to the "Collapse All" button.

Requires new IPC handlers:
- `window.fjord.renameFile(oldPath, newPath)`
- `window.fjord.trashFile(path)` -- moves to OS trash
- `window.fjord.duplicateFile(path)` -- copies to `path-copy.md`
- `window.fjord.createFile(path)` -- creates empty file

### 2.2 Recent projects

Remember recently opened folders and show them on the welcome screen.

**Storage:** localStorage key `fjordmark-recent-projects`, array of `{ path, name, lastOpened }` objects, max 10 entries.

**Welcome screen integration:**
- Below the "Open Folder" CTA, show a "Recent" section
- Each entry: folder name (bold), full path (muted), relative time (e.g., "2 days ago")
- Click to open; hover shows full path
- Small X button to remove from list

**Behavior:**
- Updated every time a folder is opened
- Most recent first
- If a path no longer exists, show it grayed out with "(not found)" and remove on click

### 2.3 Pinned tabs

Prevent accidental closure of important files.

**Behavior:**
- Pin via right-click > Pin Tab (no default shortcut -- users can bind one via custom shortcuts in Phase 5.4)
- Pinned tabs display a small pin icon instead of the close button
- Pinned tabs are always left-aligned in the tab bar, before unpinned tabs
- Cmd+W skips pinned tabs (closes the next unpinned tab instead)
- Unpin via right-click > Unpin Tab
- Pinned state persisted per-session (lost on app quit unless session restore is implemented)

### 2.4 Tab context menu

Right-click on any tab to access tab operations.

**Menu items:**
- Close
- Close Others
- Close All
- Close Tabs to the Right
- Pin / Unpin
- Copy Path
- Copy Relative Path
- Reveal in Finder / Explorer

---

## Phase 3 -- Preview & rendering upgrades

### 3.1 Syntax highlighting in code blocks

Fenced code blocks in the preview pane currently render as monochrome `<pre><code>`.

**Approach:** integrate Shiki (or highlight.js) into the unified/rehype pipeline via `rehype-highlight` or `rehype-shiki`.

**Requirements:**
- Language detection from the fence info string (```js, ```python, etc.)
- Theme-matched: use a dark color scheme in dark mode, light in light mode
- Support for the same languages CodeMirror recognizes via `@codemirror/language-data`
- Fallback: if no language is specified, render as plain monochrome (no guessing)

### 3.2 LaTeX / math rendering

Support `$inline math$` and `$$block math$$` syntax.

**Pipeline additions:**
- `remark-math` -- parses math delimiters in the AST
- `rehype-katex` -- renders math nodes to HTML using KaTeX

**Requirements:**
- KaTeX CSS loaded in the preview pane
- Theme-aware: KaTeX text color inherits from `--text1`
- Error display: invalid LaTeX shows the raw source with a red underline, not a crash

### 3.3 Footnotes

Support `[^1]` reference-style footnotes.

**Implementation:** `remark-gfm` may partially support this already. If not, add `remark-footnotes`.

**Rendering:** numbered superscript links in the body, a "Footnotes" section at the bottom of the preview with back-links.

### 3.4 Emoji shortcodes

Convert `:emoji_name:` shortcodes to Unicode emoji in the preview.

**Implementation:** `remark-emoji` plugin in the unified pipeline.

**Scope:** preview only. The raw editor continues to show the shortcode text.

---

## Phase 4 -- Writing tools

### 4.1 Word / session goals

Set a target word count for a document or a writing session.

**Document goals:**
- Set via command palette or a small target icon in the status bar
- Persisted per-document in localStorage keyed by file path
- Status bar shows `342 / 1000` when a goal is active
- Subtle celebration when goal is reached (status bar text turns green briefly, then returns to normal)

**Session goals:**
- Set via command palette: "Set session goal: 500 words"
- Counts words written since the goal was set (delta, not total)
- Resets on app restart
- Shown in status bar alongside document goal if both are active

### 4.2 Spellcheck

Toggle browser-native spellcheck on the CodeMirror editor.

**Implementation:** CodeMirror's `EditorView.contentAttributes` facet can set `spellcheck: "true"` on the content div. This enables the browser's built-in spellcheck (red underlines, right-click suggestions).

**Setting:** toggle in Settings > Editor. Off by default (some users find it distracting in Markdown where code/syntax triggers false positives).

**Language:** follows the system locale. An optional language override in settings could be added later.

### 4.3 Reading time in status bar

Reading time is already calculated in the Insights panel. Surface it in the status bar for quick reference.

**Format:** `~3 min read` shown after the word count.

**Setting:** configurable WPM in Settings > Editor (default 200).

### 4.4 Text statistics expansion

Expand the Insights panel stats with:

- Flesch-Kincaid readability score (grade level)
- Sentence count
- Average sentence length (words per sentence)
- Average word length (characters per word)

All computed from the same text the existing stats use. Updated on each content change (debounced with the same timer as auto-save).

---

## Phase 5 -- Power user features

### 5.1 Vim mode

Optional Vim keybindings via `@codemirror/vim`.

**Implementation:**
- Install `@codemirror/vim` as a dependency
- Add as a CodeMirror extension, toggled via a Compartment (same pattern as theme swapping)
- When enabled, the status bar shows the current Vim mode: `NORMAL`, `INSERT`, `VISUAL`, `COMMAND`

**Setting:** toggle in Settings > Editor. Off by default. Changing the setting takes effect immediately (no restart).

### 5.2 Multiple cursors

CodeMirror 6 supports multiple cursors natively. Expose the keybindings:

- `Cmd+D` -- select next occurrence of current selection
- `Cmd+Shift+L` -- select all occurrences of current selection
- `Alt+Click` -- add cursor at click position
- `Cmd+U` -- undo last cursor addition

These may already partially work via CM's default keymap. Verify and document them. If any are missing, add them via custom keybindings.

### 5.3 Snippets / templates

Insert boilerplate markdown from templates.

**Template sources:**
1. **Built-in templates:** bundled with the app (Blog Post, Meeting Notes, Daily Note, README, Changelog, empty table, etc.)
2. **User templates:** `.md` files in a `_templates/` folder at the project root

**Insertion:**
- Via command palette: type `template:` or `snippet:` to filter template results
- Templates can include placeholder tokens like `{{title}}`, `{{date}}`, `{{cursor}}` which are resolved on insertion (date = today, cursor = where the cursor lands)

**New file from template:**
- "New File from Template" in the File menu and command palette
- Creates a new file pre-filled with the selected template content

### 5.4 Custom keyboard shortcuts

Allow users to rebind keyboard shortcuts.

**UI:** a "Keyboard Shortcuts" section in the settings panel. Shows a searchable list of all commands with their current binding. Click a binding to record a new one.

**Storage:** persisted to localStorage under `fjordmark-keybindings`. Stores only overrides (delta from defaults).

**Conflict detection:** warn if a new binding conflicts with an existing one.

**Note:** this is the most complex settings feature. Could be deferred to a later phase if implementation bandwidth is limited.

---

## Phase 6 -- Import & export

### 6.1 Export to HTML

Export the rendered preview as a standalone `.html` file.

**Implementation:** reuse the infrastructure from PDF export (which already builds a standalone HTML document with inline styles). Instead of passing it to `printToPDF()`, save it directly as a file.

**Trigger:** File menu > Export > HTML, or command palette.

### 6.2 Export to DOCX

Export to Microsoft Word format for collaboration with non-Markdown users.

**Approach options:**
- **Library-based:** use `docx` npm package to programmatically build a DOCX from the parsed AST
- **Pandoc shell-out:** if pandoc is installed, shell out to `pandoc -f markdown -t docx`

**Recommendation:** library-based (no external dependency). The DOCX doesn't need to be pixel-perfect -- basic formatting (headings, lists, bold/italic, tables, images) is sufficient.

**Trigger:** File menu > Export > DOCX, or command palette.

### 6.3 Import from HTML / DOCX / rich text

Convert non-Markdown content into clean Markdown.

**Rich text paste:**
- Intercept paste events that contain `text/html`
- Convert to Markdown using the existing `htmlToMarkdown()` function
- Insert as Markdown text in the editor

**DOCX import:**
- Drag-and-drop or File > Import > DOCX
- Convert to Markdown via a library (e.g., `mammoth` for DOCX-to-HTML, then `htmlToMarkdown()`)
- Open as a new tab with the converted content

### 6.4 Print

Direct print via `window.print()` for quick hardcopy without PDF intermediary.

**Implementation:**
- Add `@media print` styles to `main.css`
- Hide all chrome (sidebar, toolbar, tabs, status bar) in print
- Print the preview pane content
- Trigger: `Cmd+P` (standard OS shortcut) or File > Print

---

## Phase 7 -- Settings & preferences expansion

### 7.1 New settings

Add to the existing settings panel:

**Editor group:**
- Auto-save delay: slider, 200ms to 5000ms (currently hardcoded 800ms)
- Tab indentation: spaces vs tabs
- Indent width: 2 / 4 / 8
- Soft wrap: on/off (currently always soft-wrapped)
- Show line numbers: on/off
- Typewriter scrolling: on/off
- Vim mode: on/off
- Spellcheck: on/off
- Word goal default: number input

**Preview group:**
- Code block syntax theme: match app / custom selection

**Interface group:**
- Show status bar: on/off
- Default view mode on file open: edit / split / preview
- Reading speed (WPM): number input (default 200)

**Zen mode group (new):**
- Paragraph dimming: on/off
- Column width: narrow (600px) / medium (700px) / wide (800px)

### 7.2 Settings export/import

Allow users to export all settings as a JSON file and import them on another machine.

**Export:** Settings > Export Settings -- saves a `.json` file via save dialog.
**Import:** Settings > Import Settings -- opens file picker, validates JSON, applies settings.

No cloud sync. This is manual and explicit.

---

## Phase 8 -- Platform polish

### 8.1 Native OS integration

**macOS proxy icon:**
- Set the window's `representedFilename` to the active file's path
- Allows dragging the titlebar icon to reveal the file path or drop into other apps

**File associations:**
- Register `.md` and `.markdown` file extensions in `electron-builder` config
- Handle `open-file` event in the main process to open the file in a new tab

### 8.2 Minimap

A narrow document overview on the right edge of the editor.

**Implementation:** CodeMirror doesn't have a built-in minimap. Options:
- Canvas-based: render a scaled-down representation of the document on a `<canvas>` element
- CSS-based: a narrow div with the document content at very small font size (8px), overflow hidden, click to scroll

**Behavior:**
- Shows proportional position of the viewport
- Click anywhere to jump to that position
- Drag to scroll
- Toggle in settings, off by default

### 8.3 Session restore

Remember the workspace state across app restarts.

**What's persisted (per project, keyed by folder path):**
- Open tabs (file paths) and their order
- Active tab
- Scroll position per tab
- View mode per tab
- Sidebar collapsed state and width
- Split ratios
- Dual-pane state

**Storage:** localStorage under `fjordmark-session-{folderPathHash}`.

**Behavior:**
- On open folder, check for saved session
- If found, restore all tabs and state
- If any files no longer exist, skip them silently
- "Reopen Last Session" available in command palette for manual restore

### 8.4 Auto-update

Seamless version updates via Electron's autoUpdater.

**Behavior:**
- Check for updates on app launch (after 5 second delay) and every 4 hours
- If update available, show a subtle indicator in the status bar: "Update available"
- Click to download and install on next restart
- No modal dialogs, no forced updates
- Update source: GitHub Releases

---

## Implementation priority

| Order | Phase | Description | Rationale |
|---|---|---|---|
| 1 | Phase 0 | Stabilize (WYSIWYG, split index.js, image paste) | Debt blocks everything else |
| 2 | Phase 1 | Core editing (command palette, zen mode, typewriter, drag-drop) | Biggest UX impact |
| 3 | Phase 2 | File management (sidebar ops, recent projects, pinned tabs) | Rounds out basics |
| 4 | Phase 7 | Settings expansion | Supports new features with toggles |
| 5 | Phase 3 | Preview upgrades (syntax highlighting, LaTeX, footnotes) | Quality for a markdown editor |
| 6 | Phase 4 | Writing tools (goals, spellcheck, stats) | Depth for writers |
| 7 | Phase 5 | Power user (vim, multi-cursor, snippets, custom shortcuts) | Advanced audience |
| 8 | Phase 6 | Import & export (HTML, DOCX, print) | Interop with the outside world |
| 9 | Phase 8 | Platform polish (OS integration, minimap, session restore, auto-update) | Finishing touches |

---

## Constraints

- **No frameworks.** Vanilla JS, direct DOM manipulation. Keep the bundle small.
- **No CSS-in-JS.** All styles in `main.css`. All colors via CSS variables.
- **No cloud.** No accounts, no telemetry, no sync services.
- **No PKM features.** No wikilinks, backlinks, graph view, tags, or knowledge-base tooling.
- **Borderless buttons.** Toolbar uses `<div>`, never `<button>`. Browser defaults break the aesthetic.
- **All settings in localStorage.** No config files on disk (except user templates in `_templates/`).
- **Electron 29.** Don't introduce dependencies that require a newer Electron version without good reason.
