> **Historical, pre-GPUI document.** Preserved as an archive; paths, commands, feature descriptions, and plans below are not current product guidance. See the [active documentation index](../../README.md).

# Rista Test Cases

## Purpose

This document is the core manual regression checklist for Rista.

Use it:
- before major merges
- before packaging
- after any pane/layout/editor refactor
- after file watcher, tab, or theme changes

## Test Environment

Recommended test setup:
- one small Markdown folder
- one larger Markdown folder with nested folders
- at least two files with the same filename in different folders
- at least one long document with:
  - headings
  - tables
  - callouts
  - task lists
  - links
  - images

## 1. App Boot

- Launch app
- Confirm app window opens without console-visible failure
- Confirm welcome screen appears when no folder is open
- Confirm app theme renders correctly on first load

## 2. Folder Open

- Open a folder from welcome screen
- Open a folder from menu
- Confirm tree renders
- Confirm watcher starts without visible issues
- Confirm switching to another folder resets workspace correctly

## 3. File Open And Tabs

- Open one file from tree
- Open a second file from tree
- Switch between tabs
- Reopen an already open file
- Close active tab
- Close inactive tab
- Close final tab and confirm empty/welcome state appears correctly

## 4. Duplicate Filename Safety

- Open two files with the same filename from different folders
- Confirm correct file opens each time
- Confirm explorer highlights the correct file path
- Confirm saving one does not affect the other

## 5. Standalone Pane Modes

For a loaded file, verify:

### Markdown

- Switch to `MD`
- Confirm CodeMirror is visible
- Confirm text content appears
- Confirm cursor can be placed
- Confirm typing updates content
- Confirm autosave still works

### Preview

- Switch to `Preview`
- Confirm preview fills the pane width
- Confirm no empty split geometry remains
- Confirm headings, tables, and callouts render correctly

### WYSIWYG

- Switch to `WYSIWYG`
- Confirm editor fills the pane width
- Confirm content renders
- Confirm edits update content

## 6. Standalone Mode Transitions

Verify all of these:

- `Markdown -> Preview`
- `Preview -> Markdown`
- `Markdown -> WYSIWYG`
- `WYSIWYG -> Markdown`
- `Preview -> WYSIWYG`
- `WYSIWYG -> Preview`

For each transition, confirm:
- no empty pane
- no leftover column spacing
- no stale editor surface remains
- content remains correct

## 7. Split Mode Basics

- Switch to `Split`
- Confirm split layout appears
- Confirm resizer appears
- Confirm left/right selectors render correctly
- Confirm one side can be editor and one side preview

## 8. Split Mode Combinations

Verify:

- left = `Markdown`, right = `Preview`
- left = `Preview`, right = `Markdown`
- left = `WYSIWYG`, right = `Preview`
- left = `Preview`, right = `WYSIWYG`

For each combination, confirm:
- correct content appears
- editor side is editable
- preview side is read-only
- no nested or ghost columns appear
- switching sides reclaims previous space correctly

## 9. Split To Standalone Transitions

Verify:

- `Split -> Markdown`
- `Split -> Preview`
- `Split -> WYSIWYG`

Confirm:
- split layout fully disappears
- standalone layout fills width
- no stale slot geometry remains

## 10. Standalone To Split Transitions

Verify:

- `Markdown -> Split`
- `Preview -> Split`
- `WYSIWYG -> Split`

Confirm:
- split layout appears correctly
- correct editable mode is preserved
- preview side appears where expected

## 11. Workspace Dual-Pane Mode

- Enable workspace split
- Confirm secondary pane appears
- Open a file in secondary pane
- Switch focus between panes
- Close secondary tab
- Return to single workspace mode

Confirm:
- focus state updates correctly
- secondary pane cleanup is correct
- no layout residue remains when collapsing back to single mode

## 12. Drag And Drop Tabs

- Drag a tab from primary to secondary
- Drag a tab back
- Confirm active tab and focus behavior remain correct
- Confirm content remains in sync after moves

## 13. Markdown Editing

- Type plain text
- Undo and redo
- Insert headings, lists, blockquotes
- Insert callout syntax
- Insert table
- Insert link and image syntax

Confirm:
- changes update preview
- changes save correctly

## 14. WYSIWYG Editing

- Edit text
- Change headings
- Insert table
- Insert link
- Insert image
- Toggle back to Markdown

Confirm:
- markdown round-trips correctly
- no major formatting corruption appears

## 15. Preview Accuracy

- Confirm preview matches markdown structure
- Confirm tables render correctly
- Confirm callouts render correctly
- Confirm task lists render correctly
- Confirm images render correctly

## 16. Theme Switching

- Toggle dark/light theme in Markdown
- Toggle dark/light theme in Preview
- Toggle dark/light theme in WYSIWYG
- Toggle theme while in Split

Confirm:
- text remains legible
- CodeMirror updates correctly
- WYSIWYG remount remains correct
- no layout reset bug appears

## 17. Settings Panel

- Open settings
- Change editor font size
- Change preview typography
- Change app font
- Change text colors
- Reset settings

Confirm:
- updates apply correctly
- reset restores expected defaults

## 18. File Watcher Behavior

- Edit a clean file externally
- Confirm tab reloads
- Confirm tree refreshes correctly
- Edit a dirty file externally
- Confirm no silent overwrite happens

## 19. Save And Save As

- Save edited file
- Save As to a new file
- Confirm tab updates to the new path
- Confirm explorer updates correctly

## 20. Search / Find And Replace

- Open find panel
- Search for common term
- Search for missing term
- Replace one
- Replace all

Confirm:
- result count updates correctly
- selection moves correctly

## 21. Image Paste

- Paste image from clipboard into Markdown
- Confirm Markdown image syntax is inserted
- Confirm preview updates
- Confirm saved document still references expected image data/path

## 22. PDF Export

- Export current document
- Confirm PDF file is created
- Confirm output contains document content
- Confirm output does not incorrectly include full app chrome

## Release Gate

Before shipping a milestone:

- all core pane transitions pass
- duplicate filename handling passes
- theme switching passes
- save/save-as passes
- watcher behavior passes
- export passes
