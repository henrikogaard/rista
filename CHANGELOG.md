# Changelog

## v0.1.0 (unreleased)

### Added
- Local-first folder-based Markdown editing via Tauri desktop shell
- Multi-tab editing with single/dual pane workspaces
- Markdown, split preview, preview-only, and WYSIWYG writing modes
- CodeMirror 6 editor with live preview, focus mode, and typewriter scrolling
- Auto-save on keystroke with debounced save pipeline
- Self-hosted fonts (16 families via @fontsource) — zero CDN calls
- Strict CSP with zero external network origins
- Extended-markdown compatibility: callouts, wikilinks, block references, attachment embeds
- Command palette (Cmd+K) with file + command + tag search
- Word count goals (document + session)
- Readability hints: FK Grade, adverb/passive/complex sentence highlighting
- Smart typography: auto-curly quotes, em-dash, ellipsis
- Image paste from clipboard into local `_assets/` folder
- Light and dark themes with CSS variable system
- Inline table editing with tab navigation
- Find and replace with keyboard shortcuts
- File watcher with external conflict detection
- Session restore (reopens previous tabs on relaunch)
- Version history snapshots stored in `.rista/history/`
- PDF, HTML, DOCX export
- Settings panel with typography, layout, and visual controls
- First-run sample document

### Architecture
- Tauri 2 desktop shell with Rust backend (notify watcher, file ops, export)
- Vanilla JS renderer (CodeMirror 6 + Toast UI Editor WYSIWYG)
- esbuild bundler, no React/Vue/framework
- 16 self-hosted font families via @fontsource

### Changed
- Pivot from Electron (Fjordmark) to Tauri (Rista)
- Feature-flags system: 14 non-core modules off by default behind `showExperimental`
- Statusbar collapsed to metrics and settings only
- PDF export moved from Electron Chromium to Rust HTML pipeline

### Fixed
- Clippy warnings in Rust backend
- Preload unsubscribe behavior (per-listener not removeAllListeners)
- Path-based tree identity (not filename-based)
- Stale Electron source files removed
- Various stale docs references
