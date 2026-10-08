# Changelog

## v0.1.5 — 2026-10-08

- Dock the local graph beside the editor; it follows the active note.
- Add a native Agent panel for Agent Client Protocol agents: streaming responses, active-note context, permission prompts, and reviewable file changes. Includes a Vibe preset; agents can also be configured in JSON extensions.
- Rewrite the README as a feature overview with light and dark screenshots.

## v0.1.2 — 2026-10-07

- Prepare the macOS release pipeline for Developer ID signing, notarization, and stapling of the app and DMG; Apple acceptance and installed-artifact validation are still pending.
- Add a compressed DMG alongside the ZIP, with checksums generated after final packaging.

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
