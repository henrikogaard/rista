# Markdown IDE Polish Design

## Summary

Rista should feel like a compact local-first Markdown IDE: utility surfaces are docked and precise, settings are organized like a serious preferences panel, the welcome screen is operational rather than promotional, and AI/Graph stay prominent as core knowledge-work tools.

The approved visual direction is **IDE Console**: dense, quiet, pane-based, and intentionally less decorative.

## Goals

- Make the app read as a Markdown IDE, not a marketing page wrapped around an editor.
- Reorganize settings into clearer sections with consistent controls.
- Fix inconsistent font selection by replacing native select popovers with an app-styled font picker pattern.
- Polish right sidebar and widget chrome so panels feel docked, movable, and purposeful.
- Keep AI Chat and Graph easy to reach and visually important.
- Allow the AI assistant to dock beside either sidebar, with a user preference for placement.
- Improve the terminal into a pleasant local shell surface that uses the user's configured shell when the platform exposes one.
- Expand AI provider settings to support OpenAI, Anthropic, OpenRouter, opencode variants, and custom OpenAI-compatible/local endpoints.

## Non-Goals

- Do not add a framework; the renderer remains vanilla JS.
- Do not implement a full pseudo-terminal emulator in the first pass.
- Do not copy, inspect, or manage external CLI credentials directly.
- Do not replace the existing AI Review safety flow.
- Do not make visual changes that conflict with compact density or borderless toolbar principles.

## Information Architecture

Settings will be reorganized into these sections:

- **Appearance**: theme preset, contrast, surface atmosphere, app icon.
- **Typography**: UI, explorer, editor, and preview font families and sizes.
- **Editor**: spellcheck, Vim mode, frontmatter visibility, document banners, default editing behavior.
- **Workspace**: default view, sidebar widths, auto-save, pinned/recent workspaces.
- **AI & Tools**: provider, model, API key/base URL, local CLI discovery, assistant docking.
- **Shortcuts**: editable hotkeys only.

This keeps high-frequency visual controls together and moves provider/tool configuration out of general behavior settings.

## Typography And Font Picker

Native font `<select>` menus look inconsistent across platforms and can clash with the dark UI. Replace them with a reusable app-styled picker:

- A compact trigger row showing the current font label and a sample.
- A popover/listbox rendered inside Rista styling, not the OS select menu.
- Font rows show label, optional category, and a short preview string.
- The same picker is reused for UI, explorer, editor, and preview fonts.
- Custom font stack input remains available as an advanced field.

The picker should use existing CSS tokens only. It should not introduce hardcoded colors in JS.

## Welcome Screen

The welcome screen should become a start surface, not a hero page.

Remove or heavily reduce:

- Large faux illustration.
- Aurora/mesh background treatment.
- Marketing-card composition.
- Oversized identity copy.

Replace with:

- Compact Rista identity.
- Primary actions: Open folder, New markdown file when a workspace is open, and Open recent workspace.
- Pinned and recent workspaces as the main content.
- Small operational status row: local files, graph/index state, autosave, AI availability where known.
- A simple layout that hints at the actual IDE: workspace list, recent notes, and utility status.

## Right Sidebar And Widgets

Widgets should feel like docked IDE panes.

Changes:

- Tighten widget headers: icon, title, active/collapsed state, drag handle, close action.
- Make widget action buttons consistent in size and hover behavior.
- Reduce visual noise in borders and stacked backgrounds.
- Keep empty states compact and specific.
- Add clearer tab strip affordances and tooltips for right-sidebar widgets.
- Keep left widgets workspace-oriented and right widgets inspector/context-oriented by default.

The widget system already supports left/right placement and dragging, so this is mostly presentation and settings.

## AI Chat And Graph Placement

AI Chat and Graph are key surfaces for Rista.

Design:

- Keep Graph available as a major right-side widget/modal surface.
- Add a user setting for assistant docking:
  - Right sidebar
  - Left sidebar
  - Dedicated assistant rail on the right
  - Dedicated assistant rail on the left
- Default to the right side for this implementation.
- Dedicated assistant rail should behave like an IDE activity panel: persistent, resizable, and not mixed into the regular widget stack unless the user chooses that mode.
- Do not remove the current widget-based AI chat path until the dedicated rail is stable.

## Terminal

The terminal should become a polished local shell drawer.

Current behavior already runs commands through the user's shell on the Tauri side, using `$SHELL` on Unix-like systems and `cmd` on Windows. The first polish pass should:

- Display the detected shell name in the terminal header.
- Use the workspace folder as cwd, falling back gracefully when no workspace is open.
- Improve command/output styling with prompt, exit code, duration, and clearer stderr/error lines.
- Preserve command history and keyboard behavior.
- Add quick commands or chips for common project tasks when scripts are detected.
- Respect user font settings with the editor/mono font.

Adopting full Oh My Zsh prompt theming requires an interactive PTY rather than one-shot shell command execution. That should be a follow-up if we want true prompt themes, colors, shell startup hooks, and long-running interactive programs. The first pass can still feel much better while remaining simple and reliable.

## AI Providers And Local Tools

AI settings should support:

- OpenAI API key.
- Anthropic API key.
- OpenRouter API key and base URL.
- opencode Go / opencode Zen as local CLI-style providers where installed.
- Custom OpenAI-compatible endpoint for local inference or alternate hosted APIs.
- Existing Ollama/local endpoint flow.

Provider settings should store:

- Provider id.
- Model.
- Base URL where applicable.
- API key where applicable.
- Whether the provider supports tool/function calling.
- Whether the provider is a local CLI provider or HTTP provider.

Local CLI discovery should be explicit and safe:

- Probe known commands with non-destructive checks such as `command -v codex`, `codex --version`, `command -v opencode`, and similar.
- Show availability in Settings -> AI & Tools.
- Never read credential files or expose tokens.
- Use the CLI's existing authentication if the user has already logged in outside Rista.
- Route AI-initiated destructive local actions through review/confirmation.

Codex CLI can be discovered this way. On the current machine, `codex` is available at `/opt/homebrew/bin/codex` and reports `codex-cli 0.130.0`. A signed-in Codex CLI can be invoked by Rista as the same OS user, but Rista should not manage its ChatGPT OAuth grant or generated API keys.

## Data Flow

- Settings remain persisted in localStorage through the existing settings module.
- New provider definitions live in the AI provider module and are rendered in the AI & Tools settings section.
- Tool discovery should call a small Tauri command that runs safe probes and returns structured availability data.
- Assistant docking preference controls whether AI Chat is mounted as a regular widget or as a dedicated side rail.
- Terminal metadata should come from Tauri where needed: shell path/name, command duration, exit code.

## Error Handling

- Font picker falls back to the configured stack even if the first font is unavailable.
- Provider settings show missing API key/base URL states without blocking non-AI app use.
- Local CLI probes fail closed and display "not found" or "unavailable" rather than throwing UI errors.
- Terminal commands still show stdout, stderr, exit code, and launcher errors separately.
- Assistant rail falls back to widget mode if layout restoration encounters an unsupported stored docking value.

## Testing

Add or update contract tests for:

- Settings tab structure and renamed sections.
- Typography picker replacing native font selects for font settings.
- AI & Tools settings containing OpenAI, Anthropic, OpenRouter, opencode, custom OpenAI-compatible, and local provider options.
- Assistant docking preference values.
- Widget chrome classes for docked IDE pane styling.
- Welcome screen no longer using marketing hero/aurora classes.
- Terminal displaying shell metadata, exit code/duration hooks, and improved command row structure.

Manual verification:

- Run unit/contract tests.
- Run production build.
- Launch the app and inspect settings, welcome screen, terminal drawer, right sidebar, graph, and AI chat in dark and light themes.

## Implementation Order

1. Settings reorganization and typography picker.
2. Welcome screen rewrite.
3. Right sidebar/widget visual polish.
4. Terminal shell metadata and visual polish.
5. AI provider settings expansion and local CLI discovery.
6. Assistant docking preference and optional dedicated assistant rail.
