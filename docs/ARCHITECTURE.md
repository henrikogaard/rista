# Architecture

Rísta v0.1.0 is a Rust 2021 single-binary desktop application built with GPUI through `gpui-kit` 0.7. The runtime is a native workspace and editor; the pre-GPUI renderer architecture is retained only in the [archive](archive/pre-gpui/README.md).

## Runtime map

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Bootstraps the app, themes, menus, key bindings, application-level file URLs, and workspace recreation after the last window closes. |
| `src/app.rs` | Workspace, tabs, vault UI, dialogs, and application interactions. |
| `src/document.rs` | Editor state, dirty baseline, save/conflict handling, autosave, and preview synchronization. |
| `src/vault.rs` | Vault indexing and the 250 ms filesystem watcher; note and image indexes include cached wikilink aliases. |
| `src/folder.rs`, `src/folder_dashboard.rs` | Direct-child filesystem snapshots, folder introductions, dashboard rendering, breadcrumbs, and right inspector. |
| `src/extensions.rs`, `src/tools_panel.rs` | Bounded declarative JSON manifests, command registration, and per-launch process confirmation. |
| `src/terminal_session.rs`, `src/terminal.rs` | Portable PTY lifecycle, bounded VT screen/scrollback, and native terminal input/rendering. |
| `src/preview.rs` | Markdown preview and its `MarkdownPlugin` processing pipeline. |
| `src/bases.rs` | YAML `.base` queries and formulas; table, cards, gallery, kanban, board, and calendar views; relations and rollups. |
| `src/properties.rs` | Frontmatter properties. |
| `src/history.rs` | Vault-local history under `.rista/history` and trash. |
| `src/graph.rs` | Note and link graph. |
| `src/slash.rs` | Slash-command completions. |
| `src/emoji.rs` | Emoji support. |
| `src/decorations.rs` | Editor decorations. |
| `src/settings.rs` | Persisted settings and runtime theme, source, split, and preview preferences. |

On macOS, settings are stored at `~/Library/Application Support/no.ogard.rista/settings.json`.

## Files and workspace lifecycle

Users can open a vault with **⌘⇧O** or a standalone Markdown file with **⌘O**. The app registers `.md` and `.markdown` documents for native opening. Application-level URL handling accepts file-open events, and Finder/Dock reopen events recreate a workspace after the final window has closed; no recovery click is required.

A vault watcher observes filesystem changes with a 250 ms debounce. The standalone-document poll runs every 500 ms but invalidates only when the observed file metadata changes; it does not force an unconditional full content read or render each interval.

Folder dashboards are workspace destinations, not document tabs. They
participate in navigation history and restore from `active_folder`; open document
buffers remain intact while a folder is active. The filesystem snapshot refreshes
after watcher-driven document reloads. Introductions prefer an open note buffer,
otherwise read `<folder>.md` or `README.md` from disk. Tree disclosure never opens
a page, and opening a page never reveals/expands its tree ancestors.

Dashboard view controls persist in the introduction's `dashboard` frontmatter.
The local extension API registers tool commands without rebuilding; process
execution requires explicit confirmation and is not sandboxed. See
[folder pages](folder-pages.md) and [extensions](extensions.md) for schemas and limits.

## Save and conflict behavior

Documents keep a byte baseline for the last observed on-disk content. Before writing, save compares current bytes with that baseline and rejects an external edit or deletion, including a same-length edit. Dirty-document lifecycle flushes abort when a save fails.

The comparison is a guarded check followed by a write, not an atomic compare-and-swap. Do not promise that it eliminates every concurrent-write race or guarantees zero data loss.

## Preview and structured notes

### Moving tabs between windows

**Move to New Window** / **Flytt til nytt vindu** is available from the tab context menu, command palette and File menu. It saves pending edits with the normal conflict guard, opens the saved document in a new window, and removes the original tab only after the destination loads matching content. Save failures, conflicts or a destination mismatch keep the original tab. The last source tab may move without closing its window; an empty split pane collapses normally.

The destination inherits the source vault, pane mode, pin state and caret. Other tabs and windows are left in place. **Open in New Window** continues to keep both copies open. A move reopens the saved document rather than transferring a window-bound editor: prior Undo/Redo history, selection range and scroll position do not transfer. Subsequent edits have normal Undo/Redo. Both resulting window sessions participate in normal session restore.

The preview pipeline supports headings, local images, cover/banner content, callouts, wiki-note transclusion, base embeds, and a subset of math notation. Math support is not full TeX. `.base` views provide query/formula-backed structured collections and can relate or roll up note properties.

### Image sizing

Rendered images in a document's Preview or Split pane have a corner handle.
Dragging preserves the displayed aspect ratio; hold Shift to change width and
height independently. Double-click the handle to remove explicit dimensions;
Escape cancels a drag. A completed drag writes one `|widthxheight` suffix in the
original wiki embed or Markdown image label, preserving its target and title.
For example, `![[photo.png]]` becomes `![[photo.png|320x180]]`, and
`![Caption](photo.png)` becomes `![Caption|320x180](photo.png)`.

The drag updates only the rendered preview until release, then uses one normal
editor replacement for Undo/Redo, autosave and conflict protection. Exact source
spans distinguish repeated images. A changed document revision or conflict
cancels the edit instead of applying stale offsets. Generated, nested and
transcluded previews remain read-only; source syntax can still size those
images. Image files themselves are never modified.

## Platform and release scope

The v0.1.0 package was tested on Apple Silicon macOS. Its declared minimum is macOS 13; not every OS version has been tested. Linux and Windows are intended targets, but their builds, packages, and keyboard mappings are not verified. See the [release checklist](RELEASE-CHECKLIST.md) and [test cases](TEST-CASES.md) for validation work.
