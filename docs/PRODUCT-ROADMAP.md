# Product roadmap

This page consolidates open validation gaps and product decisions for the native application. It has no delivery dates and makes no feature commitments. The checked-in v0.1.0 baseline is described in [Architecture](ARCHITECTURE.md); the pre-GPUI plans are retained in the [archive](archive/pre-gpui/README.md), not carried forward as commitments.

## Current native baseline

Rísta is a local-first Rust 2021 single GPUI binary using `gpui-kit` 0.7. The v0.1.0 baseline includes vault and standalone Markdown editing, native document opening, tabs, autosave and guarded external-change checks, Markdown preview, `.base` query/formula views, note/link graph, vault-local history/trash, and theme/source/split/preview settings.

The package was tested on Apple Silicon macOS and declares macOS 13 as its minimum; not every OS version has been tested. Linux and Windows are intended targets, but builds, packages, and keyboard mappings are unverified. The v0.1.0 and v0.1.1 macOS packages are ad-hoc signed and not notarized. Release-pipeline changes target v0.1.2; their Apple acceptance and installed-artifact behavior remain unverified.

The verified native baseline does not include WYSIWYG editing, PDF/DOCX export, a terminal, AI features, or cloud services.

## Shipped since v0.2.0

These are merged on `main` and not yet in a release:

- Quick Capture and an Inbox triage view.
- Recent vaults in the command palette.
- Typewriter mode.
- A timeline view for `.base` files.
- History preview before restoring a version.
- A link health report for broken links and orphans.
- Type-aware property controls.
- A report of unused image attachments.
- Project-wide replace from the find dialog.
- Incremental vault indexing.
- A readable local graph with optional coloured nodes.
- A list of installed agents in the agent panel.
- Daily note carry-over and Go to date.
- Schedule tasks to a later daily or weekly note.
- Agent change review, with accept all, reject all, and undo.
- Faster drawing of large boards when zoomed out.

## Open validation gaps

These are areas to investigate or validate, not promised releases:

- Cross-platform build, packaging, and keyboard-mapping verification for Linux and Windows.
- Validate the v0.1.2 Developer ID signing, notarization, stapling, and Gatekeeper checks against the published app and DMG; no Apple acceptance is claimed until that run passes.
- Large-vault indexing, watcher, search, and `.base` performance profiling.
- Accessibility, keyboard navigation, text input, and assistive-technology validation.
- Product decisions about editing modes and export formats, including whether any WYSIWYG or PDF/DOCX workflow belongs in scope.
- Broader file-lifecycle and structured-note regression coverage from the [test cases](TEST-CASES.md).

Use the [release checklist](RELEASE-CHECKLIST.md) to record candidate-specific validation. Do not infer completion from this roadmap.
