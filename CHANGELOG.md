# Changelog

## v0.1.1 — 2026-10-07

- Added the MIT license with copyright attributed to Henrik Øgård, also included in the macOS bundle.
- Replaced the multicolour icon with the carved R identity, light/dark brand assets, and a full-resolution macOS iconset.
- Rewrote current product, architecture, release, and QA documentation; archived superseded plans with clear historical labels.

## v0.1.0 — 2026-10-07

First release of the native Rust/GPUI application.

- Single-binary Rust 2021 app built with `gpui-kit` 0.7.
- Native opening for `.md` and `.markdown` standalone documents and vault workflows, including Finder/Dock reopen after the last window closes.
- Multi-tab editing with autosave, byte-baseline external-change protection, and Markdown preview.
- Vault indexing, wikilink aliases, note/link graph, vault-local history and trash, and YAML `.base` views with queries, formulas, relations, and rollups.
- Preview support for headings, local images, cover/banner content, callouts, wiki-note transclusion, base embeds, and a subset of math notation.
- Versioned release assets: `Rista-0.1.0.zip`, `SHA256SUMS`, and `appcast.xml`.

The package was tested on Apple Silicon macOS and declares macOS 13 as its minimum; not every OS version has been tested. This build is ad hoc signed and not notarized; Linux and Windows packages are not verified. See the [documentation index](docs/README.md) for architecture, release, test, and roadmap notes.
