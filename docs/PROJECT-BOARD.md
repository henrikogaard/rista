# Rísta Project Board

This document turns the product roadmap into concrete epics, milestones, issue-style tasks, and acceptance criteria.

## Board Structure

- `Epic`: a large outcome area
- `Milestone`: a grouped delivery target
- `Task`: a concrete implementation item
- `Status`: `Backlog`, `Ready`, `In Progress`, `Blocked`, `Done`

## Epic 1: Stable Editing Core

Outcome:
Rísta becomes structurally reliable. Pane modes, workspace split, tabs, and editor surfaces behave predictably.

### Milestone 1: Renderer Refactor

#### Task 1.1
Title: Split renderer entry into dedicated modules
Status: Done
Acceptance criteria:
- [src/renderer/index.js](../src/renderer/index.js) is reduced to bootstrapping and wiring
- app shell, panes, tabs, commands, and tree logic live in separate files
- build still passes

#### Task 1.2
Title: Introduce shared DOM/event helper utilities
Status: Done
Acceptance criteria:
- repetitive DOM lookup and event code is centralized
- helpers do not obscure behavior

#### Task 1.3
Title: Add `ARCHITECTURE.md`
Status: Done
Acceptance criteria:
- renderer module responsibilities are documented
- pane/workspace rendering model is described

### Milestone 2: Explicit State Model

#### Task 1.4
Title: Define formal pane and workspace state contract
Status: Done
Acceptance criteria:
- pane mode, split mode, workspace mode, tabs, and focus are explicitly documented in code
- layout is derived from state only

#### Task 1.5
Title: Create transition helpers for pane/workspace changes
Status: Done
Acceptance criteria:
- pane/workspace transitions happen through dedicated functions
- no layout-critical state mutation is scattered across unrelated handlers

#### Task 1.6
Title: Remove remaining DOM-driven implicit state behavior
Status: Backlog
Acceptance criteria:
- the UI no longer relies on previous DOM placement to determine current behavior

### Milestone 3: Surface Ownership Cleanup

#### Task 1.7
Title: Centralize CodeMirror surface mounting
Status: Done
Acceptance criteria:
- Markdown editor host has one canonical owner per pane
- standalone and split mode both work reliably

#### Task 1.8
Title: Centralize WYSIWYG surface mounting
Status: Done
Acceptance criteria:
- WYSIWYG mounts cleanly in standalone and split modes
- stale hosts are not left behind

#### Task 1.9
Title: Centralize preview surface rendering
Status: Done
Acceptance criteria:
- standalone preview and split preview use clearly separated containers
- preview state does not distort future pane layouts

## Epic 2: Speed and Scalability

Outcome:
The app remains fast under large folders, long documents, and frequent edits.

### Milestone 4: Efficient Update Pipelines

#### Task 2.1
Title: Separate debounce pipelines for save, preview, stats, and headings
Status: Done
Acceptance criteria:
- saving does not unnecessarily throttle preview or insights
- preview and stats do not recompute on selection-only changes

#### Task 2.2
Title: Prevent redundant preview rerenders
Status: Done
Acceptance criteria:
- preview updates only when document content changes
- no extra rerender happens when toggling focus only

#### Task 2.3
Title: Benchmark preview performance on large documents
Status: Backlog
Acceptance criteria:
- benchmark fixtures exist
- results are documented for long docs, many headings, and tables

### Milestone 5: Explorer and Watcher Performance

#### Task 2.4
Title: Make tree updates incremental
Status: Done
Acceptance criteria:
- tree is not rebuilt completely for every file change
- expanded state is preserved

#### Task 2.5
Title: Lazy render deep folder branches
Status: Backlog
Acceptance criteria:
- large folders open faster
- inactive deep branches do not fully render immediately

#### Task 2.6
Title: Batch watcher event bursts
Status: Backlog
Acceptance criteria:
- multiple file changes do not cause UI thrash

## Epic 3: Robustness and Trust

Outcome:
Users can rely on Rísta for real work without fear of silent corruption or weird state loss.

### Milestone 6: Regression Safety

#### Task 3.1
Title: Add `TEST-CASES.md`
Status: Done
Acceptance criteria:
- core manual regression cases are documented
- includes standalone and split mode transitions

#### Task 3.2
Title: Add unit tests for markdown helpers and pane transitions
Status: Backlog
Acceptance criteria:
- markdown helpers have coverage
- pane transition logic has coverage

#### Task 3.3
Title: Add smoke tests for app boot and folder open
Status: Backlog
Acceptance criteria:
- boot and open-folder failures are caught automatically

### Milestone 7: Safer File Handling

#### Task 3.4
Title: Detect external file conflicts for dirty tabs
Status: Done
Acceptance criteria:
- user is warned before overwriting external changes
- conflict options are clear

#### Task 3.5
Title: Add session restore for open tabs
Status: Done
Acceptance criteria:
- relaunch can reopen previous tabs
- state restore is predictable

#### Task 3.6
Title: Add unsaved buffer recovery
Status: Backlog
Acceptance criteria:
- crash/relaunch can recover unsaved text when enabled

### Milestone 8: IPC and Identity Reliability

#### Task 3.7
Title: Fix preload unsubscribe behavior
Status: Done
Acceptance criteria:
- unsubscribing one listener does not remove unrelated listeners

#### Task 3.8
Title: Replace filename-based explorer identity with full paths
Status: Done
Acceptance criteria:
- duplicate filenames do not confuse the explorer

#### Task 3.9
Title: Normalize IPC return shapes
Status: Done
Acceptance criteria:
- renderer gets consistent success/error structures

## Epic 4: Workspace and Navigation

Outcome:
The app supports writing, navigating, and organizing notes efficiently.

### Milestone 9: Command Palette

#### Task 4.1
Title: Build command palette shell
Status: Done
Acceptance criteria:
- `Cmd/Ctrl+K` opens palette
- palette dismisses cleanly

#### Task 4.2
Title: Add file search to command palette
Status: Done
Acceptance criteria:
- files can be fuzzy-searched and opened from the palette

#### Task 4.3
Title: Add command search to command palette
Status: Done
Acceptance criteria:
- view toggles and insert actions are searchable

#### Task 4.4
Title: Add keyboard-first palette navigation
Status: Done
Acceptance criteria:
- arrows, enter, and escape work smoothly

### Milestone 10: Search and Navigation

#### Task 4.5
Title: Rebuild find and replace
Status: Done
Acceptance criteria:
- find next/prev, replace one/all work reliably
- behavior scales to large files

#### Task 4.6
Title: Improve outline/headings navigation
Status: Done
Acceptance criteria:
- heading hierarchy is clearer
- clicking headings navigates reliably

#### Task 4.7
Title: Add recent files and pinned files
Status: Done
Acceptance criteria:
- recently used files are accessible quickly
- users can pin important files

## Epic 5: Notes System Workflows

Outcome:
Rísta supports PARA, GTD, and Zettelkasten naturally without forcing complexity on everyone.

### Milestone 11: Templates and Starter Structures

#### Task 5.1
Title: Add starter workspace templates
Status: Backlog
Acceptance criteria:
- users can choose PARA, GTD, Zettelkasten, or plain notes
- templates create folders/files only, not hidden structures

#### Task 5.2
Title: Add note templates
Status: Backlog
Acceptance criteria:
- templates exist for daily, meeting, project, reference, and zettel notes

### Milestone 12: Linking and Knowledge Work

#### Task 5.3
Title: Improve internal link insertion
Status: Backlog
Acceptance criteria:
- inserting links to local notes is fast and discoverable

#### Task 5.4
Title: Add optional wiki-link support
Status: Done
Acceptance criteria:
- users can enable or ignore wiki links

#### Task 5.5
Title: Add backlink discovery
Status: Backlog
Acceptance criteria:
- users can inspect notes that reference the current note

#### Task 5.6
Title: Add quick linked-note creation from selection
Status: Backlog
Acceptance criteria:
- selected text can become a new linked note quickly

### Milestone 13: PARA and GTD Helpers

#### Task 5.7
Title: Add PARA navigation helpers
Status: Backlog
Acceptance criteria:
- users can jump between Projects, Areas, Resources, and Archive quickly

#### Task 5.8
Title: Improve task/GTD insertion flows
Status: Backlog
Acceptance criteria:
- inbox, next action, waiting, and someday patterns are easy to insert

## Epic 6: Power Editing

Outcome:
Rísta feels like a serious Markdown IDE, not just a text editor.

### Milestone 14: WYSIWYG Hardening

#### Task 6.1
Title: Audit markdown round-tripping in WYSIWYG
Status: Backlog
Acceptance criteria:
- toggling between WYSIWYG and Markdown preserves structure reliably

#### Task 6.2
Title: Harden tables, callouts, and lists in WYSIWYG
Status: Backlog
Acceptance criteria:
- these structures render and round-trip cleanly

### Milestone 15: Advanced Editor Tools

#### Task 6.3
Title: Add optional Vim mode
Status: Done
Acceptance criteria:
- Vim mode is optional, persisted, and isolated from normal mode

#### Task 6.4
Title: Expand insert commands and toolbar tools
Status: Backlog
Acceptance criteria:
- common Markdown structures are easy to insert by mouse or keyboard

#### Task 6.5
Title: Improve dual-pane comparison workflows
Status: Backlog
Acceptance criteria:
- comparing two files side by side is smooth and reliable

## Epic 7: Visual Design and Product Quality

Outcome:
The app feels memorable, calm, and premium.

### Milestone 16: Visual System

#### Task 7.1
Title: Refine typography and spacing system
Status: Backlog
Acceptance criteria:
- UI hierarchy feels consistent across the app

#### Task 7.2
Title: Improve active, focus, and hover states
Status: Backlog
Acceptance criteria:
- state changes are clear without feeling loud

### Milestone 17: Atmosphere and Branding

#### Task 7.3
Title: Finish aurora welcome screen
Status: Backlog
Acceptance criteria:
- welcome screen feels premium and intentional

#### Task 7.4
Title: Improve empty-tab and empty-file states
Status: Backlog
Acceptance criteria:
- empty states feel informative and visually integrated

#### Task 7.5
Title: Refine logo and branding usage
Status: Backlog
Acceptance criteria:
- branding is polished across app, README, and About surfaces

### Milestone 18: Theme Quality

#### Task 7.6
Title: Deep tune dark and light themes
Status: Backlog
Acceptance criteria:
- editor, preview, WYSIWYG, explorer, and dialogs feel coherent in both themes

## Epic 8: Release and Platform Readiness

Outcome:
The app can be shipped with confidence and improved sustainably.

### Milestone 19: Export and Packaging

#### Task 8.1
Title: Rebuild PDF export as document-only export
Status: Done
Acceptance criteria:
- exported PDF contains only document content
- app chrome is not included

#### Task 8.2
Title: Audit packaging outputs and platform polish
Status: Done
Acceptance criteria:
- icons, menus, About panel, and packaged output are polished

### Milestone 20: Release Discipline

#### Task 8.3
Title: Define release checklist
Status: Done
Acceptance criteria:
- release checklist includes build, regression pass, and packaging verification

#### Task 8.4
Title: Define milestone-based changelog process
Status: Done
Acceptance criteria:
- changes are grouped by milestone/release, not random commits

## Recommended Build Order

1. Epic 1: Stable Editing Core
2. Epic 3: Robustness and Trust
3. Epic 2: Speed and Scalability
4. Epic 4: Workspace and Navigation
5. Epic 5: Notes System Workflows
6. Epic 6: Power Editing
7. Epic 7: Visual Design and Product Quality
8. Epic 8: Release and Platform Readiness

## Suggested Near-Term Sprint

Priority sprint:
- Task 1.1: Split renderer entry into dedicated modules
- Task 1.4: Define formal pane and workspace state contract
- Task 1.7: Centralize CodeMirror surface mounting
- Task 3.1: Add `TEST-CASES.md`
- Task 3.7: Fix preload unsubscribe behavior
- Task 3.8: Replace filename-based explorer identity with full paths
- Task 8.1: Rebuild PDF export as document-only export

