> **Historical, pre-GPUI document.** Preserved as an archive; paths, commands, feature descriptions, and plans below are not current product guidance. See the [active documentation index](../../README.md).

# Rísta Product Roadmap

## Vision

Rísta should become a fast, local-first Markdown IDE for thought and writing:

- lightweight enough for "open folder and write"
- structured enough for serious note systems like PARA, GTD, and Zettelkasten
- visually calm, elegant, and distinctly Nordic
- robust under large folders, long documents, and daily use

The product should serve two overlapping audiences:

1. People who just want to open a folder and write Markdown.
2. People who want to organize notes, projects, references, and documents over time.

## Product Principles

1. Speed first.
   Opening files, switching tabs, previewing, searching, and moving around the workspace should feel immediate.

2. Files remain real files.
   Markdown files on disk stay the source of truth. No lock-in and no hidden database-first architecture.

3. Robustness over cleverness.
   State must be explicit, transitions predictable, and failures recoverable.

4. Beautiful but purposeful.
   Rísta should feel premium, atmospheric, and minimal without sacrificing usability.

5. Progressive complexity.
   A new user can ignore advanced features. A power user can build a serious note workflow on top.

## Strategic Goals

1. Build a stable editing core.
2. Make the app fast under real-world note vaults.
3. Make the app trustworthy for long-term writing and organization.
4. Add knowledge-work features that support note systems without forcing one.
5. Polish the visual and interaction design into a premium Markdown IDE.

## Phases

1. Core Stability and Architecture
2. Performance and Scalability
3. Robustness and Trust
4. Workspace and Knowledge Features
5. PARA / GTD / Zettelkasten Support
6. Power Tools and Advanced Writing
7. Visual Design and Product Polish
8. Packaging, Testing, and Release Discipline

## Phase 1: Core Stability and Architecture

Goal:
Make the app structurally reliable so new features do not destabilize editing, layout, or tabs.

### Milestone 1.1: Renderer Refactor

Tasks:
- Split [src/renderer/index.js](../src/renderer/index.js) into focused modules.
- Keep a thin renderer entry file.
- Move DOM builders out of state/behavior code.
- Add shared DOM and event helpers.

Suggested modules:
- `app-shell.js`
- `workspace.js`
- `tabs.js`
- `pane-layout.js`
- `commands.js`
- `tree-view.js`
- `insights.js`
- `dialogs.js`

### Milestone 1.2: Formal State Model

Tasks:
- Define one explicit app state contract for workspace, panes, tabs, settings, and focus.
- Add transition helpers for pane mode, workspace mode, tab movement, and activation.
- Remove implicit DOM-driven state assumptions.
- Derive layout from state only.

### Milestone 1.3: Dedicated Layout Paths

Tasks:
- Keep standalone and split layouts fully separate.
- Keep standalone `markdown`, `preview`, and `wysiwyg` in dedicated containers.
- Keep split mode in dedicated split containers only.
- Centralize surface mounting logic for CodeMirror, WYSIWYG, and preview.

### Milestone 1.4: File Identity Cleanup

Tasks:
- Replace filename-based identity checks with full path checks everywhere.
- Fix explorer highlighting to use path-based identity.
- Audit tab lookup, file watcher updates, and save-as behavior for duplicate filenames.

### Milestone 1.5: Architecture Documentation

Tasks:
- Keep [WORKLOG.md](./WORKLOG.md) current.
- Add `ARCHITECTURE.md`.
- Document renderer module ownership.
- Document pane and workspace transition rules.

## Phase 2: Performance and Scalability

Goal:
Make Rísta feel instant, even with large note folders and long documents.

### Milestone 2.1: Separate Expensive Update Pipelines

Tasks:
- Split debounce timing for autosave, preview rendering, stats, and heading extraction.
- Avoid recomputing all derived data on every keystroke.
- Update only the active pane when possible.

### Milestone 2.2: Preview Optimization

Tasks:
- Render preview only when document content changes.
- Skip preview rerender on selection-only updates.
- Consider chunked rendering for large documents.
- Benchmark with long notes, heavy tables, and many callouts/headings.

### Milestone 2.3: Explorer Performance

Tasks:
- Avoid rebuilding the entire tree on every file change.
- Patch tree updates incrementally.
- Lazy render deep folder branches.
- Preserve expanded folder state efficiently.

### Milestone 2.4: Watcher Efficiency

Tasks:
- Handle file system events incrementally.
- Reload only affected tab/tree nodes.
- Batch bursts of watcher events.

### Milestone 2.5: Search and Navigation Speed

Tasks:
- Audit the current find implementation.
- Replace it with a stronger, scalable search flow.
- Prepare for large-folder command palette and file search performance.

## Phase 3: Robustness and Trust

Goal:
Users should trust Rísta with real notes and documents.

### Milestone 3.1: Regression Matrix

Tasks:
- Create `TEST-CASES.md`.
- Cover all pane and workspace transitions.
- Cover tab open/close, folder switching, save-as, and theme switching.

### Milestone 3.2: Automated Testing Foundation

Tasks:
- Add unit tests for markdown helpers, stats, headings, and pane transitions.
- Add renderer tests for tabs, mode switching, and path identity.
- Add smoke tests for app boot and folder open.

### Milestone 3.3: External File Conflict Handling

Tasks:
- Detect when a dirty tab changed on disk.
- Show conflict UI with options to reload or keep local changes.
- Prevent silent overwrites.

### Milestone 3.4: Session Recovery

Tasks:
- Persist open tabs and focused file.
- Restore the last session on relaunch.
- Optionally restore unsaved buffers after crash.

### Milestone 3.5: Safer IPC Contracts

Tasks:
- Normalize preload/main success and error return shapes.
- Replace ambiguous booleans with structured responses where useful.
- Fix unsubscribe behavior for file and command listeners.

### Milestone 3.6: Export Reliability

Tasks:
- Rebuild PDF export to render document content only.
- Add print/export styling.
- Ensure export matches preview output, not the full app shell.

## Phase 4: Workspace and Knowledge Features

Goal:
Support both casual writing and structured note workflows.

### Milestone 4.1: Command Palette

Tasks:
- Add `Cmd/Ctrl+K`.
- Search files, commands, views, and insertions.
- Add fuzzy matching and keyboard navigation.
- Make it fast for large folders.

### Milestone 4.2: Better Find and Replace

Tasks:
- Replace the current custom find flow with a stronger implementation.
- Support next/previous, replace one, replace all, and case sensitivity.
- Evaluate CodeMirror-native search integration.

### Milestone 4.3: Better Image Handling

Tasks:
- Save pasted images to local `_assets/` near the file.
- Keep optional base64 fallback if needed.
- Generate clean relative paths.
- Handle duplicate filenames safely.

### Milestone 4.4: Document Navigation

Tasks:
- Improve the headings/outline panel.
- Make heading navigation reliable.
- Show clearer hierarchy and structure.

### Milestone 4.5: Note Organization Helpers

Tasks:
- Improve new note and new file flows.
- Add templates for common note/document types.
- Support rename and move flows later.

## Phase 5: PARA / GTD / Zettelkasten Support

Goal:
Let structured thinkers use Rísta deeply without forcing a workflow on casual users.

### Milestone 5.1: Workspace Templates

Tasks:
- Offer optional starter folder structures for:
  - PARA
  - GTD
  - Zettelkasten
  - Plain notes

### Milestone 5.2: Metadata and Linking Basics

Tasks:
- Improve internal link insertion.
- Optionally support wiki links.
- Improve tag and frontmatter awareness without making metadata mandatory.

### Milestone 5.3: Zettelkasten Helpers

Tasks:
- Quick-create linked note from selection.
- Optional note ID insertion.
- Backlink discovery across folder.
- Unlinked mention search later.

### Milestone 5.4: PARA Workspace UX

Tasks:
- Fast navigation between `Projects`, `Areas`, `Resources`, and `Archive`.
- Pinned folders and recent files.
- Workspace shortcuts for common review flows.

### Milestone 5.5: GTD-Oriented Aids

Tasks:
- Better task list handling in Markdown.
- Quick insertion for inbox items and action states.
- Optional future filters for next actions and waiting items.

## Phase 6: Power Tools and Advanced Writing

Goal:
Make Rísta feel like a real Markdown IDE.

### Milestone 6.1: WYSIWYG Hardening

Tasks:
- Improve WYSIWYG consistency with preview.
- Ensure markdown round-tripping is safe.
- Audit tables, links, blockquotes, callouts, and task lists.

### Milestone 6.2: Vim Mode

Tasks:
- Add optional Vim mode.
- Persist the setting.
- Keep it isolated from standard editing behavior.

### Milestone 6.3: Editor Preferences

Tasks:
- Add settings for autosave delay, default mode, spellcheck, wrapping, and typography.
- Keep settings compact and clear.

### Milestone 6.4: Better Insert Tools

Tasks:
- Extend toolbar and command actions for common Markdown structures.
- Keep the flows keyboard-friendly.

### Milestone 6.5: Multi-Document Workflows

Tasks:
- Improve dual-pane workspace behavior.
- Make side-by-side comparison reliable.
- Make drag-and-drop between panes solid.

## Phase 7: Visual Design and Product Polish

Goal:
Turn Rísta into a visually distinctive, premium-feeling Markdown IDE.

### Milestone 7.1: Visual System

Tasks:
- Refine typography hierarchy.
- Improve empty states and onboarding surfaces.
- Tighten spacing consistency.
- Improve focus and active states.

### Milestone 7.2: Welcome and Workspace Atmosphere

Tasks:
- Finish the aurora welcome screen.
- Improve empty-file and empty-tab experiences.
- Add stronger workspace opening rhythm and visual framing.

### Milestone 7.3: Interaction Polish

Tasks:
- Smooth pane transitions.
- Improve hover, selection, and dialog interactions.
- Keep motion subtle and purposeful.

### Milestone 7.4: Theme Quality

Tasks:
- Deeply tune both dark and light themes.
- Audit all surfaces for contrast and consistency.

### Milestone 7.5: Branding

Tasks:
- Refine logo usage in app and docs.
- Improve About panel presentation where possible.
- Make the app memorable without becoming noisy.

## Phase 8: Packaging, Testing, and Release Discipline

Goal:
Ship improvements confidently and sustainably.

### Milestone 8.1: Release Structure

Tasks:
- Define milestones and release notes format.
- Keep changelogs concise and useful.
- Group work by milestone rather than random fixes.

### Milestone 8.2: Quality Gates

Tasks:
- Require build pass before merge/release.
- Require regression pass for pane/workspace features.
- Add benchmark checks for performance-sensitive changes later.

### Milestone 8.3: Packaging Quality

Tasks:
- Audit app icon, About panel, menus, startup behavior, and packaging outputs.
- Test macOS, Windows, and Linux build paths where relevant.

## Proposed Release Milestones

### v0.2: Stable Core

Includes:
- renderer refactor
- explicit pane/workspace state
- split and standalone stability
- path-safe tabs/tree
- regression checklist
- basic tests

### v0.3: Fast Workspace

Includes:
- preview and tree performance improvements
- command palette
- better find/replace
- safer watcher behavior
- stronger file conflict handling

### v0.4: Notes System Edition

Includes:
- templates
- link helpers
- PARA / GTD / Zettelkasten starter support
- improved navigation and organization tools

### v0.5: Markdown IDE

Includes:
- hardened WYSIWYG
- power-user editing features
- better export
- session restore
- polished dual-pane workflow

### v1.0: Premium Local-First Notes IDE

Includes:
- stable editing core
- fast large-workspace handling
- refined visual system
- trustworthy long-term note workflows

## Immediate Priority Order

1. Freeze major feature additions until pane/surface stability is validated.
2. Refactor the renderer into modules.
3. Add `TEST-CASES.md` and start tracking regression coverage.
4. Fix infrastructure issues:
   - path-based explorer highlighting
   - preload unsubscribe behavior
   - document-only PDF export
5. Improve preview and tree performance.
6. Build the command palette.
