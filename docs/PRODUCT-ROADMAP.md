# Product roadmap

This page consolidates open validation gaps and product decisions for the native application. It has no delivery dates and makes no feature commitments. The checked-in v0.1.0 baseline is described in [Architecture](ARCHITECTURE.md); the pre-GPUI plans are retained in the [archive](archive/pre-gpui/README.md), not carried forward as commitments.

## Current native baseline

Rísta is a local-first Rust 2021 single GPUI binary using `gpui-kit` 0.7. The v0.1.0 baseline includes vault and standalone Markdown editing, native document opening, tabs, autosave and guarded external-change checks, Markdown preview, `.base` query/formula views, note/link graph, vault-local history/trash, and theme/source/split/preview settings.

The package was tested on Apple Silicon macOS and declares macOS 13 as its minimum; not every OS version has been tested. Linux and Windows are intended targets, but builds, packages, and keyboard mappings are unverified. The current macOS package is ad hoc signed and not notarized; the repository is private and the update feed needs publicly accessible hosting for unauthenticated update clients.

The verified native baseline does not include WYSIWYG editing, PDF/DOCX export, a terminal, AI features, or cloud services.

## Open validation gaps

These are areas to investigate or validate, not promised releases:

- Cross-platform build, packaging, and keyboard-mapping verification for Linux and Windows.
- Developer ID signing, notarization, installation behavior, and public hosting for packages and the Sparkle update feed.
- Large-vault indexing, watcher, search, and `.base` performance profiling.
- Accessibility, keyboard navigation, text input, and assistive-technology validation.
- Product decisions about editing modes and export formats, including whether any WYSIWYG or PDF/DOCX workflow belongs in scope.
- Broader file-lifecycle and structured-note regression coverage from the [test cases](TEST-CASES.md).

Use the [release checklist](RELEASE-CHECKLIST.md) to record candidate-specific validation. Do not infer completion from this roadmap.
