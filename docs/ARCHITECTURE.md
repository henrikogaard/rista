# Architecture

Rísta v0.1.0 is a Rust 2021 single-binary desktop application built with GPUI through `gpui-kit` 0.7. The runtime is a native workspace and editor; the pre-GPUI renderer architecture is retained only in the [archive](archive/pre-gpui/README.md).

## Runtime map

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | Bootstraps the app, themes, menus, key bindings, application-level file URLs, and workspace recreation after the last window closes. |
| `src/app.rs` | Workspace, tabs, vault UI, dialogs, and application interactions. |
| `src/document.rs` | Editor state, dirty baseline, save/conflict handling, autosave, and preview synchronization. |
| `src/vault.rs` | Vault indexing and the 250 ms filesystem watcher; note and image indexes include cached wikilink aliases. |
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

## Save and conflict behavior

Documents keep a byte baseline for the last observed on-disk content. Before writing, save compares current bytes with that baseline and rejects an external edit or deletion, including a same-length edit. Dirty-document lifecycle flushes abort when a save fails.

The comparison is a guarded check followed by a write, not an atomic compare-and-swap. Do not promise that it eliminates every concurrent-write race or guarantees zero data loss.

## Preview and structured notes

The preview pipeline supports headings, local images, cover/banner content, callouts, wiki-note transclusion, base embeds, and a subset of math notation. Math support is not full TeX. `.base` views provide query/formula-backed structured collections and can relate or roll up note properties.

## Platform and release scope

The v0.1.0 package was tested on Apple Silicon macOS. Its declared minimum is macOS 13; not every OS version has been tested. Linux and Windows are intended targets, but their builds, packages, and keyboard mappings are not verified. See the [release checklist](RELEASE-CHECKLIST.md) and [test cases](TEST-CASES.md) for validation work.
