# Changelog

## v0.2.0 — 2026-10-16

- Split the editor into panes with tab groups, drag tabs between panes, move tabs to new windows from the tab menu, open independent workspace windows, and save and restore workspace layouts.
- Add native JSON Canvas boards: text, file, link and group cards, edges with labels, pinch and scroll navigation, and source-preserving saves of unknown fields.
- Add advanced vault search operators and live query blocks embedded in notes.
- Add customizable hotkeys, bookmark groups and saved searches, offline spell checking, Vim mode, and style snippets.
- Open PDFs in read-only tabs, embed PDF pages in notes, and export formatted notes as PDF.
- Merge notes and extract headings with the transactional Note Composer, which also retargets links and Canvas references, with multi-file undo.
- Add the `rista` command-line tool and `rista://` URLs, import settings from an Obsidian vault with a read-only preview, and resize images by dragging in the preview.
- Read notes in a centred 720px column at 16px by default, with real link styling, aligned table columns, and callouts at body size.
- Calm the chrome: fewer title-bar buttons, a shorter sidebar header, recent notes first in the command palette, and Appearance first in Settings.
- Polish Canvas boards: tinted coloured cards, one title per file card, no properties table inside cards, a subtle dot grid, groups drawn behind their cards and edges, and boards that open fitted.
- Fix the editor font falling back to a proportional face when SF Mono is unavailable, pinch zoom after keyboard input on boards, and the original segmented tab and view-mode chrome.
- Run on Linux x86_64 with Ctrl shortcuts, a client-drawn title bar, and a desktop entry, tested on Ubuntu 24.04 with GNOME. aarch64 Linux builds and passes the tests but was not run. Linux is not yet a supported platform; see the README.

## v0.1.7 — 2026-10-08

- Make Graphite the default theme and rename the original palette Slate, preserving saved selections.
- Add light and dark Gruvbox, Nord, Solarized, and Catppuccin palettes, bringing the total to 18 variants.
- Show the native macOS About panel with the app logo, version, and Henrik Øgård's copyright credit.
- Align the right dock with the editor and let remaining panes fill its height when other panes close.
- Add Codex, Claude, OpenCode, and Grok-through-OpenCode agent presets alongside Vibe, with bilingual setup guidance and improved macOS executable discovery.
- Support agent-advertised model selection, pending-switch safeguards, and recovery from rejected model changes. Agent tools and authentication remain separate installations; authenticated provider sessions are not verified in this release.

## v0.1.6 — 2026-10-08

- Add paired Graphite and Mono monochrome palettes.
- Preserve terminal ANSI blue, magenta, and cyan colors across palettes.
- Refine explorer search and keep graph search outside the canvas.
- Align document and terminal tab icon and label spacing.
- Add bottom status-bar padding and fix preview-tab promotion and focus restoration.

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
