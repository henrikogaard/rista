> **Historical, pre-GPUI document.** Preserved as an archive; paths, commands, feature descriptions, and plans below are not current product guidance. See the [active documentation index](../../../../README.md).

# D2 & Mermaid Diagram Support

**Date:** 2026-05-13
**Status:** Approved

## Summary

Add rendering support for D2 and Mermaid fenced code blocks in Fjordmark's preview pane. When a user writes a ` ```d2 ` or ` ```mermaid ` code block, the preview renders an SVG diagram. The editor pane continues to show raw source code.

## Decisions

- **Rendering approach:** Post-HTML DOM rewriting (Approach A). After `renderMarkdown()` sets `innerHTML`, a second pass finds diagram code blocks and replaces them with rendered SVGs.
- **Mermaid engine:** `mermaid` npm package, renders in-browser in the renderer process.
- **D2 engine:** Local `d2` CLI tool, invoked via IPC from main process. Requires user to have `d2` installed.
- **Error display:** Inline styled error blocks (matching callout aesthetic) when rendering fails or `d2` is not installed.
- **Caching:** In-memory `Map<hash(lang+source), svg>` per session. Busted on theme change.

## Architecture

### New file: `src/renderer/diagrams.js`

Exports:
- `initDiagrams()` -- initialize mermaid with theme-aware config
- `processDiagrams(previewElement, theme)` -- find and render diagram blocks in a preview element
- `clearDiagramCache()` -- bust cache on theme toggle

Internal flow of `processDiagrams()`:
1. Query `pre > code[class*="language-d2"], pre > code[class*="language-mermaid"]`
2. For each match, extract text content and language
3. Check cache by `hash(language + source + theme)` -- if hit, use cached SVG
4. **Mermaid:** call `mermaid.render(uniqueId, source)` in-browser
5. **D2:** call `window.fjord.renderD2(source, themeId)` via IPC
6. On success: replace `<pre>` with `<div class="diagram diagram--{lang}">` containing SVG
7. On error: replace `<pre>` with `<div class="diagram diagram--error">` with styled error message
8. Store result in cache

### IPC: `renderD2(source, themeId)`

Defined in `preload.js`, implemented in `main.js`.

Main process handler:
1. Spawn `d2 - - --theme {themeId}` (stdin->stdout mode)
2. Pipe `source` to stdin, close stdin
3. Capture stdout (SVG) or stderr (error)
4. 5-second timeout to prevent runaway processes
5. If `d2` not found on PATH: return `{ error: "D2 not installed..." }`
6. Return `{ svg }` or `{ error }`

### Modified files

**`src/main/main.js`** -- add `render:d2` IPC handler.

**`src/main/preload.js`** -- expose `window.fjord.renderD2(source, themeId)`.

**`src/renderer/index.js`** -- call `initDiagrams()` at startup, call `processDiagrams()` after each `refreshPreview()`, call `clearDiagramCache()` on theme toggle.

**`src/renderer/styles/main.css`** -- add diagram container styles.

**`package.json`** -- add `mermaid` dependency.

### Unchanged files

`markdown.js`, `editor.js`, `theme.js`, `settings.js`, `tree-view.js` -- no modifications needed.

## CSS

```css
.diagram {
  display: block;
  margin: 12px 0;
  text-align: center;
  overflow-x: auto;
}
.diagram svg {
  max-width: 100%;
  height: auto;
}
.diagram--error {
  background: var(--bg2);
  border-left: 3px solid var(--amber);
  padding: 12px 16px;
  margin: 12px 0;
  font-family: var(--mono);
  font-size: 12px;
  color: var(--text2);
  text-align: left;
  white-space: pre-wrap;
}
```

## Theme Awareness

**Mermaid:** Initialized with `'dark'` or `'default'` base theme plus color overrides from CSS variables. Re-initialized on theme toggle.

**D2:** CLI `--theme` flag. Dark mode -> theme 200 (Dark Mauve). Light mode -> theme 0 (default). Theme ID passed via IPC.

Cache busted for both engines on theme change.

## Error States

| Condition | Display |
|---|---|
| Mermaid syntax error | `.diagram--error` with mermaid's error message |
| D2 syntax error | `.diagram--error` with d2 stderr output |
| `d2` CLI not installed | `.diagram--error` with "D2 is not installed. Install from https://d2lang.com" |
| `d2` CLI timeout (>5s) | `.diagram--error` with "D2 rendering timed out" |
