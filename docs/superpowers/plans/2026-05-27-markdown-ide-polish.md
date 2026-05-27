# Markdown IDE Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn Rista’s settings, welcome screen, widgets, terminal, and AI configuration into a compact Markdown IDE experience.

**Architecture:** Keep the vanilla JS renderer and existing module boundaries. Add small focused helpers where the current files need reusable UI primitives: font picker rendering in `shell.js`, provider/tool metadata in `ai-providers.js`, local tool probing through `src-tauri/src/main.rs`, and assistant docking through settings plus the existing widget/AI chat modules.

**Tech Stack:** Vanilla JS renderer, CSS tokens in `src/renderer/styles/main.css`, Tauri Rust commands, Node test runner contract tests.

---

## File Structure

- Modify `tests/ui-contract.test.js`: add contract tests for each polish area before implementation.
- Modify `src/renderer/settings.js`: settings defaults, sanitization, font option metadata, assistant docking, provider defaults.
- Modify `src/renderer/shell.js`: settings tab IA, reusable font picker UI, AI & Tools settings, welcome screen rewrite.
- Modify `src/renderer/styles/main.css`: IDE welcome surface, custom font picker, settings layout, widget chrome, terminal polish, assistant rail.
- Modify `src/renderer/right-panel.js`: widget header/tab markup classes and tooltip semantics.
- Modify `src/renderer/terminal-drawer.js`: shell metadata display, command row structure, duration/exit code output.
- Modify `src/renderer/tauri-api.js`: expose shell/tool discovery commands.
- Modify `src-tauri/src/main.rs`: shell metadata, command timing, safe local CLI discovery, provider bridge additions.
- Modify `src/renderer/ai-providers.js`: provider registry for OpenAI, Anthropic, OpenRouter, Ollama, opencode variants, and custom OpenAI-compatible endpoints.
- Modify `src/renderer/ai-chat.js`: provider config handling and assistant docking compatibility.
- Create `src/renderer/assistant-rail.js`: optional dedicated assistant rail host that mounts the existing AI chat panel.

## Task 1: Settings IA And Typography Picker

**Files:**
- Modify: `tests/ui-contract.test.js`
- Modify: `src/renderer/settings.js`
- Modify: `src/renderer/shell.js`
- Modify: `src/renderer/styles/main.css`

- [ ] **Step 1: Write the failing contract test**

Add this test near the existing settings tests in `tests/ui-contract.test.js`:

```js
test('settings are organized as Markdown IDE preferences with custom font pickers', () => {
  const shell = read('src/renderer/shell.js')
  const settings = read('src/renderer/settings.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(shell, /\{ id: 'appearance', label: 'Appearance' \}/)
  assert.match(shell, /\{ id: 'typography', label: 'Typography' \}/)
  assert.match(shell, /\{ id: 'editor', label: 'Editor' \}/)
  assert.match(shell, /\{ id: 'workspace', label: 'Workspace' \}/)
  assert.match(shell, /\{ id: 'ai-tools', label: 'AI & Tools' \}/)
  assert.match(shell, /\{ id: 'shortcuts', label: 'Shortcuts' \}/)
  assert.match(shell, /function renderFontPicker/)
  assert.match(shell, /data-font-picker=/)
  assert.match(shell, /data-font-option=/)
  assert.match(shell, /settings-font-picker/)
  assert.doesNotMatch(shell, /renderSelectSetting\('uiFont'/)
  assert.doesNotMatch(shell, /renderSelectSetting\('editorFont'/)
  assert.match(settings, /assistantDock: 'right-sidebar'/)
  assert.match(css, /\.settings-font-picker/)
  assert.match(css, /\.settings-font-option/)
  assert.match(css, /\.settings-section-title/)
})
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
npm test -- tests/ui-contract.test.js
```

Expected: FAIL on the new settings IA/font picker assertions.

- [ ] **Step 3: Update setting defaults**

In `src/renderer/settings.js`, add assistant docking and richer font metadata without changing stored values:

```js
export const ASSISTANT_DOCK_OPTIONS = [
  { value: 'right-sidebar', label: 'Right sidebar' },
  { value: 'left-sidebar', label: 'Left sidebar' },
  { value: 'right-rail', label: 'Dedicated right rail' },
  { value: 'left-rail', label: 'Dedicated left rail' },
]
```

Add to `DEFAULT_SETTINGS`:

```js
assistantDock: 'right-sidebar',
```

In `sanitize(settings)`, after the existing merged settings object is created, clamp invalid docking values:

```js
if (!ASSISTANT_DOCK_OPTIONS.some(option => option.value === next.assistantDock)) {
  next.assistantDock = DEFAULT_SETTINGS.assistantDock
}
```

- [ ] **Step 4: Reorganize settings tabs and render font pickers**

In `src/renderer/shell.js`, replace `SETTINGS_TABS` with:

```js
const SETTINGS_TABS = [
  { id: 'appearance', label: 'Appearance' },
  { id: 'typography', label: 'Typography' },
  { id: 'editor', label: 'Editor' },
  { id: 'workspace', label: 'Workspace' },
  { id: 'ai-tools', label: 'AI & Tools' },
  { id: 'shortcuts', label: 'Shortcuts' },
]
```

Add a helper after `renderSelectSetting`:

```js
function fontLabelFor(key) {
  const group = key.replace(/Font$/, '')
  const options = FONT_OPTIONS[group] || []
  const current = settingsValue(key)
  return options.find(option => option.value === current)?.label || 'Custom'
}

export function renderFontPicker(key, label, options, sample = 'Aa Markdown') {
  const current = settingsValue(key)
  return `
    <section class="settings-field settings-font-picker" data-font-picker="${key}">
      <div class="settings-field__row">
        <span class="settings-field__label">${label}</span>
        <span class="settings-field__value">${fontLabelFor(key)}</span>
      </div>
      <div class="settings-font-picker__trigger" data-font-picker-trigger="${key}" role="button" tabindex="0">
        <span class="settings-font-picker__sample" style="font-family: ${escapeAttribute(current)}">${escapeAttribute(sample)}</span>
        <span class="settings-font-picker__chevron">⌄</span>
      </div>
      <div class="settings-font-picker__list" data-font-picker-list="${key}">
        ${options.map(option => `
          <div
            class="settings-font-option${option.value === current ? ' active' : ''}"
            data-font-setting="${key}"
            data-font-option="${escapeAttribute(option.value)}"
            role="button"
            tabindex="0"
          >
            <span class="settings-font-option__label">${option.label}</span>
            <span class="settings-font-option__sample" style="font-family: ${escapeAttribute(option.value)}">${escapeAttribute(sample)}</span>
          </div>
        `).join('')}
      </div>
    </section>
  `
}
```

In settings page markup, move existing controls into the new tab ids and use:

```js
${renderFontPicker('uiFont', 'Interface font', FONT_OPTIONS.ui)}
${renderFontPicker('explorerFont', 'Explorer font', FONT_OPTIONS.explorer)}
${renderFontPicker('editorFont', 'Editor font', FONT_OPTIONS.editor, 'const note = "# Markdown"')}
${renderFontPicker('previewFont', 'Preview font', FONT_OPTIONS.preview, 'Heading and body text')}
```

- [ ] **Step 5: Wire font picker clicks**

In `handleSettingsClick(event)`, before preset handling, add:

```js
const fontTrigger = event.target.closest('[data-font-picker-trigger]')
if (fontTrigger) {
  const picker = fontTrigger.closest('[data-font-picker]')
  picker?.classList.toggle('open')
  return
}

const fontOption = event.target.closest('[data-font-option]')
if (fontOption) {
  updateSetting(fontOption.dataset.fontSetting, fontOption.dataset.fontOption)
  syncSettingsForm()
  return
}
```

In `handleSettingsKeydown(event)`, include `[data-font-picker-trigger], [data-font-option]` in the actionable selector.

- [ ] **Step 6: Add CSS**

Add to `src/renderer/styles/main.css` near settings styles:

```css
.settings-section-title {
  margin: 0 0 10px;
  color: var(--text2);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.settings-font-picker { position: relative; }
.settings-font-picker__trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: 30px;
  padding: 0 9px;
  border: 1px solid var(--surface-edge);
  border-radius: var(--radius-xs);
  background: var(--surface-bg-1);
  color: var(--text1);
  cursor: pointer;
}
.settings-font-picker__sample { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.settings-font-picker__chevron { color: var(--text3); }
.settings-font-picker__list {
  display: none;
  margin-top: 4px;
  border: 1px solid var(--surface-edge);
  border-radius: var(--radius-sm);
  background: var(--surface-bg-2);
  box-shadow: var(--shadow-pop);
  overflow: hidden;
}
.settings-font-picker.open .settings-font-picker__list { display: block; }
.settings-font-option {
  display: grid;
  grid-template-columns: minmax(110px, 0.8fr) minmax(120px, 1fr);
  gap: 8px;
  align-items: center;
  padding: 6px 9px;
  color: var(--text2);
  cursor: pointer;
}
.settings-font-option:hover,
.settings-font-option.active {
  color: var(--text1);
  background: var(--accent-dim);
}
.settings-font-option__label { font-size: 11px; }
.settings-font-option__sample {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text3);
  font-size: 11px;
}
```

- [ ] **Step 7: Run tests and commit**

Run:

```bash
npm test -- tests/ui-contract.test.js
npm run build
git diff --check
```

Expected: all pass.

Commit:

```bash
git add tests/ui-contract.test.js src/renderer/settings.js src/renderer/shell.js src/renderer/styles/main.css
git commit -m "feat: reorganize settings typography"
```

## Task 2: Utility Welcome Screen

**Files:**
- Modify: `tests/ui-contract.test.js`
- Modify: `src/renderer/shell.js`
- Modify: `src/renderer/styles/main.css`

- [ ] **Step 1: Write the failing test**

Add:

```js
test('welcome screen is an IDE start surface instead of a marketing hero', () => {
  const shell = read('src/renderer/shell.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(shell, /welcome__start/)
  assert.match(shell, /welcome__actions/)
  assert.match(shell, /welcome__workspace-panel/)
  assert.match(shell, /welcome__status-strip/)
  assert.doesNotMatch(shell, /welcome__hero-panel/)
  assert.doesNotMatch(shell, /welcome__aurora/)
  assert.doesNotMatch(shell, /welcome__mesh/)
  assert.doesNotMatch(css, /\.welcome__aurora/)
  assert.doesNotMatch(css, /\.welcome__mesh/)
  assert.match(css, /\.welcome__start/)
  assert.match(css, /\.welcome__actions/)
})
```

- [ ] **Step 2: Run the failing test**

Run:

```bash
npm test -- tests/ui-contract.test.js
```

Expected: FAIL because the old hero classes still exist.

- [ ] **Step 3: Replace `buildWelcome()` markup**

In `src/renderer/shell.js`, replace only the returned welcome markup with:

```js
return `
  <div class="welcome" id="welcome">
    <div class="welcome__content welcome__start">
      <header class="welcome__masthead">
        <div>
          <div class="welcome__eyebrow">Local Markdown workspace</div>
          <div class="welcome__logo">Rísta</div>
          <div class="welcome__sub">${hasFolder ? 'Choose a note, open Graph, or start a new Markdown file.' : 'Open a folder to work with plain local files.'}</div>
        </div>
        <div class="welcome__actions">
          ${hasFolder ? `
            <div class="welcome__btn" id="welcome-new-file-btn" role="button" tabindex="0">New note</div>
          ` : `
            <div class="welcome__btn" id="welcome-open-btn" role="button" tabindex="0">
              <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2 5h4l2-2h6a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/></svg>
              Open folder…
            </div>
          `}
        </div>
      </header>
      <section class="welcome__quick-grid" aria-label="Rísta workspace status">
        <div class="welcome__quick-card"><span class="welcome__quick-kicker">Source</span><strong>Local files</strong><span>Plain Markdown on disk.</span></div>
        <div class="welcome__quick-card"><span class="welcome__quick-kicker">Graph</span><strong>Workspace links</strong><span>Backlinks and local graph ready.</span></div>
        <div class="welcome__quick-card"><span class="welcome__quick-kicker">AI</span><strong>Review first</strong><span>File edits stay explicit.</span></div>
      </section>
      ${workspaceHtml}
      <div class="welcome__status-strip" aria-hidden="true">
        <span>Markdown-first</span>
        <span>Local graph ready</span>
        <span>Autosave armed</span>
      </div>
    </div>
  </div>
`
```

In `refreshShellWelcome()`, also bind `welcome-new-file-btn`:

```js
$('welcome-new-file-btn')?.addEventListener('click', () => _callbacks.createNewFile?.())
```

In `registerShellCallbacks` call from `src/renderer/index.js`, pass `createNewFile` if it is not already available to shell callbacks.

- [ ] **Step 4: Replace welcome CSS**

Remove CSS blocks for `.welcome__hero-panel`, `.welcome__hero`, `.welcome__aurora`, `.welcome__mesh`, `.welcome__rune`, and `.welcome__hero-lines`. Add:

```css
.welcome__start {
  width: min(760px, calc(100vw - 48px));
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.welcome__masthead {
  display: flex;
  justify-content: space-between;
  gap: 18px;
  padding: 16px 0 12px;
  border-bottom: 1px solid var(--surface-edge);
}
.welcome__eyebrow {
  margin-bottom: 6px;
  color: var(--text3);
  font-family: var(--mono);
  font-size: 10px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.welcome__actions {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  flex-shrink: 0;
}
```

- [ ] **Step 5: Run tests and commit**

Run:

```bash
npm test -- tests/ui-contract.test.js
npm run build
git diff --check
```

Commit:

```bash
git add tests/ui-contract.test.js src/renderer/shell.js src/renderer/index.js src/renderer/styles/main.css
git commit -m "style: simplify welcome start surface"
```

## Task 3: Docked Widget Chrome

**Files:**
- Modify: `tests/ui-contract.test.js`
- Modify: `src/renderer/right-panel.js`
- Modify: `src/renderer/styles/main.css`

- [ ] **Step 1: Write the failing test**

Add:

```js
test('widgets use docked IDE pane chrome', () => {
  const rightPanel = read('src/renderer/right-panel.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(rightPanel, /widget__header-main/)
  assert.match(rightPanel, /widget__header-controls/)
  assert.match(rightPanel, /aria-label="Move widget"/)
  assert.match(rightPanel, /aria-label="Collapse widget"/)
  assert.match(rightPanel, /aria-label="Close widget"/)
  assert.match(rightPanel, /right-sidebar__tab-label/)
  assert.match(css, /\.widget--active/)
  assert.match(css, /\.widget__header-main/)
  assert.match(css, /\.widget__header-controls/)
  assert.match(css, /\.right-sidebar__tab-label/)
})
```

- [ ] **Step 2: Run the failing test**

Run:

```bash
npm test -- tests/ui-contract.test.js
```

Expected: FAIL on missing classes/labels.

- [ ] **Step 3: Update tab and header markup**

In `renderTabs()`, use:

```js
return `<div class="right-sidebar__tab${active}" data-action="widget-tab" data-widget="${id}" title="${escapeHtml(title)}" aria-label="Toggle ${escapeHtml(title)} widget" role="button" tabindex="0">
  <span class="right-sidebar__tab-icon">${hooks?.icon || ''}</span>
  <span class="right-sidebar__tab-label">${escapeHtml(title)}</span>
</div>`
```

In `renderStack(side)`, update each widget wrapper/header string to include:

```js
node.innerHTML = `
  <div class="widget__header" data-action="widget-collapse" data-widget="${id}">
    <div class="widget__header-main">
      <span class="widget__drag-handle" data-action="widget-drag" aria-label="Move widget" role="button" tabindex="0">⋮⋮</span>
      <span class="widget__icon">${hooks?.icon || ''}</span>
      <span class="widget__title">${escapeHtml(hooks?.title || id)}</span>
    </div>
    <div class="widget__header-controls">
      <span class="widget__actions">${hooks?.headerActions?.() || ''}</span>
      <span class="widget__caret" aria-label="Collapse widget">▾</span>
      <span class="widget__close" aria-label="Close widget" data-action="widget-close">×</span>
    </div>
  </div>
  <div class="widget__body">${hooks.build()}</div>
`
```

Also add `widget--active` to active expanded widgets:

```js
const activeClass = state.collapsedWidgets.has(id) ? '' : ' widget--active'
```

- [ ] **Step 4: Add CSS**

Add:

```css
.widget--active { background: color-mix(in srgb, var(--surface-bg-1) 92%, transparent); }
.widget__header-main {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.widget__header-controls {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  margin-left: auto;
}
.right-sidebar__tab-label {
  display: none;
  max-width: 72px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 10px;
}
.right-sidebar.open .right-sidebar__tabs:hover .right-sidebar__tab {
  width: auto;
  max-width: 104px;
  padding: 0 7px;
}
.right-sidebar.open .right-sidebar__tabs:hover .right-sidebar__tab-label {
  display: inline;
}
```

- [ ] **Step 5: Run tests and commit**

Run:

```bash
npm test -- tests/ui-contract.test.js
npm run build
git diff --check
```

Commit:

```bash
git add tests/ui-contract.test.js src/renderer/right-panel.js src/renderer/styles/main.css
git commit -m "style: polish docked widget chrome"
```

## Task 4: Terminal Shell Polish

**Files:**
- Modify: `tests/ui-contract.test.js`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/renderer/tauri-api.js`
- Modify: `src/renderer/terminal-drawer.js`
- Modify: `src/renderer/styles/main.css`

- [ ] **Step 1: Write the failing test**

Add:

```js
test('terminal exposes shell metadata and command run details', () => {
  const terminal = read('src/renderer/terminal-drawer.js')
  const adapter = read('src/renderer/tauri-api.js')
  const tauriMain = read('src-tauri/src/main.rs')
  const css = read('src/renderer/styles/main.css')

  assert.match(adapter, /getShellInfo: \(\) => invoke\('get_shell_info'\)/)
  assert.match(tauriMain, /fn get_shell_info/)
  assert.match(tauriMain, /durationMs/)
  assert.match(terminal, /terminal-shell/)
  assert.match(terminal, /formatDuration/)
  assert.match(terminal, /terminal-line__meta/)
  assert.match(css, /\.terminal-shell/)
  assert.match(css, /\.terminal-line__meta/)
  assert.match(css, /\.terminal-quick-command/)
})
```

- [ ] **Step 2: Run the failing test**

Run:

```bash
npm test -- tests/ui-contract.test.js
```

Expected: FAIL on missing shell metadata.

- [ ] **Step 3: Add Tauri shell info and duration**

In `src-tauri/src/main.rs`, define:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShellInfo {
    path: String,
    name: String,
}

#[tauri::command]
fn get_shell_info() -> ShellInfo {
    let path = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(windows) { "cmd".into() } else { "/bin/sh".into() }
    });
    let name = Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&path)
        .to_string();
    ShellInfo { path, name }
}
```

In `run_terminal_command`, record duration:

```rust
let started = std::time::Instant::now();
```

Add `duration_ms: started.elapsed().as_millis() as u64` to both success and error `TerminalResult` values, and add the `duration_ms` field to the `TerminalResult` struct using `#[serde(rename = "durationMs")]`.

Register `get_shell_info` in the Tauri invoke handler list.

- [ ] **Step 4: Expose adapter API**

In `src/renderer/tauri-api.js`, add:

```js
getShellInfo: () => invoke('get_shell_info'),
```

- [ ] **Step 5: Update terminal renderer**

In `src/renderer/terminal-drawer.js`, add shell state:

```js
let shellInfo = { name: 'shell', path: '' }
```

Add:

```js
function formatDuration(ms) {
  const value = Number(ms || 0)
  if (value < 1000) return `${value}ms`
  return `${(value / 1000).toFixed(1)}s`
}

async function syncShellInfo() {
  try {
    shellInfo = await window.fjord.getShellInfo()
  } catch {
    shellInfo = { name: 'shell', path: '' }
  }
  const el = $('terminal-shell')
  if (el) {
    el.textContent = shellInfo.name
    el.title = shellInfo.path || shellInfo.name
  }
}
```

In `buildTerminalDrawer()`, add:

```html
<div class="terminal-shell" id="terminal-shell">shell</div>
```

In `toggleTerminalDrawer()` and `openTerminalDrawer()`, call `syncShellInfo()`.

When a command completes, append metadata:

```js
appendTerminalOutput(`exit ${result.code ?? 0} · ${formatDuration(result.durationMs)}`, 'meta')
```

Update `appendTerminalOutput` to render meta:

```js
line.className = type === 'meta' ? 'terminal-line terminal-line__meta' : `terminal-line terminal-line--${type}`
```

- [ ] **Step 6: Add terminal CSS**

Add:

```css
.terminal-shell {
  color: var(--text2);
  font-family: var(--mono);
  font-size: 10px;
  border: 1px solid var(--surface-edge);
  padding: 2px 6px;
  border-radius: var(--radius-xs);
  background: var(--surface-bg-1);
}
.terminal-line__meta {
  color: var(--text3);
  font-size: 10px;
}
.terminal-quick-command {
  color: var(--text3);
  border: 1px solid var(--surface-edge);
  border-radius: var(--radius-xs);
  padding: 2px 6px;
}
```

- [ ] **Step 7: Run tests and commit**

Run:

```bash
npm test -- tests/ui-contract.test.js
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check
```

Commit:

```bash
git add tests/ui-contract.test.js src-tauri/src/main.rs src/renderer/tauri-api.js src/renderer/terminal-drawer.js src/renderer/styles/main.css
git commit -m "feat: polish terminal shell surface"
```

## Task 5: AI Providers And Safe Local Tool Discovery

**Files:**
- Modify: `tests/ui-contract.test.js`
- Modify: `src/renderer/ai-providers.js`
- Modify: `src/renderer/settings.js`
- Modify: `src/renderer/shell.js`
- Modify: `src/renderer/tauri-api.js`
- Modify: `src-tauri/src/main.rs`
- Modify: `src/renderer/ai-chat.js`

- [ ] **Step 1: Write the failing test**

Add:

```js
test('AI settings support expanded providers and safe local tool discovery', () => {
  const providers = read('src/renderer/ai-providers.js')
  const settings = read('src/renderer/settings.js')
  const shell = read('src/renderer/shell.js')
  const adapter = read('src/renderer/tauri-api.js')
  const tauriMain = read('src-tauri/src/main.rs')

  assert.match(providers, /openrouter/)
  assert.match(providers, /custom-openai-compatible/)
  assert.match(providers, /opencode-go/)
  assert.match(providers, /opencode-zen/)
  assert.match(providers, /supportsTools/)
  assert.match(providers, /transport: 'cli'/)
  assert.match(settings, /assistantDock/)
  assert.match(shell, /data-settings-section="ai-tools"/)
  assert.match(shell, /renderLocalToolDiscovery/)
  assert.match(shell, /data-local-tool-refresh/)
  assert.match(adapter, /discoverLocalAiTools: \(\) => invoke\('discover_local_ai_tools'\)/)
  assert.match(tauriMain, /fn discover_local_ai_tools/)
  assert.match(tauriMain, /codex --version/)
  assert.match(tauriMain, /opencode --version/)
})
```

- [ ] **Step 2: Run the failing test**

Run:

```bash
npm test -- tests/ui-contract.test.js
```

Expected: FAIL on missing provider/tool discovery support.

- [ ] **Step 3: Expand provider registry**

Replace `PROVIDERS` in `src/renderer/ai-providers.js` with entries shaped like:

```js
export const PROVIDERS = {
  openai: {
    label: 'OpenAI',
    defaultModel: 'gpt-4o',
    models: ['gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo'],
    defaultBaseUrl: 'https://api.openai.com',
    transport: 'http',
    apiKey: true,
    supportsTools: true,
    protocol: 'openai-compatible',
  },
  anthropic: {
    label: 'Anthropic',
    defaultModel: 'claude-sonnet-4-5-20250929',
    models: ['claude-opus-4-5-20251029', 'claude-sonnet-4-5-20250929', 'claude-haiku-4-5-20251001'],
    defaultBaseUrl: 'https://api.anthropic.com',
    transport: 'http',
    apiKey: true,
    supportsTools: true,
    protocol: 'anthropic',
  },
  openrouter: {
    label: 'OpenRouter',
    defaultModel: 'openai/gpt-4o-mini',
    models: ['openai/gpt-4o-mini', 'anthropic/claude-sonnet-4.5', 'meta-llama/llama-3.1-70b-instruct'],
    defaultBaseUrl: 'https://openrouter.ai/api',
    transport: 'http',
    apiKey: true,
    supportsTools: true,
    protocol: 'openai-compatible',
  },
  ollama: {
    label: 'Ollama',
    defaultModel: 'llama3',
    models: ['llama3', 'mistral', 'codellama'],
    defaultBaseUrl: 'http://localhost:11434',
    transport: 'http',
    apiKey: false,
    noApiKey: true,
    supportsTools: false,
    protocol: 'ollama',
  },
  'custom-openai-compatible': {
    label: 'Custom OpenAI-compatible',
    defaultModel: 'local-model',
    models: ['local-model'],
    defaultBaseUrl: 'http://localhost:8000',
    transport: 'http',
    apiKey: false,
    supportsTools: true,
    protocol: 'openai-compatible',
  },
  'opencode-go': {
    label: 'opencode Go',
    defaultModel: 'default',
    models: ['default'],
    defaultBaseUrl: '',
    transport: 'cli',
    command: 'opencode',
    apiKey: false,
    noApiKey: true,
    supportsTools: false,
    protocol: 'cli',
  },
  'opencode-zen': {
    label: 'opencode Zen',
    defaultModel: 'zen',
    models: ['zen'],
    defaultBaseUrl: '',
    transport: 'cli',
    command: 'opencode',
    apiKey: false,
    noApiKey: true,
    supportsTools: false,
    protocol: 'cli',
  },
}
```

- [ ] **Step 4: Add safe discovery command**

In `src-tauri/src/main.rs`, add:

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalAiTool {
    id: String,
    label: String,
    command: String,
    path: Option<String>,
    version: Option<String>,
    available: bool,
}

fn run_probe(command: &str, args: &[&str]) -> (Option<String>, Option<String>, bool) {
    let path = Command::new(if cfg!(windows) { "where" } else { "command" })
        .args(if cfg!(windows) { vec![command] } else { vec!["-v", command] })
        .output()
        .ok()
        .and_then(|output| if output.status.success() { Some(String::from_utf8_lossy(&output.stdout).trim().to_string()) } else { None });
    let version = Command::new(command)
        .args(args)
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        });
    let available = path.as_ref().map(|value| !value.is_empty()).unwrap_or(false);
    (path, version, available)
}

#[tauri::command]
fn discover_local_ai_tools() -> Vec<LocalAiTool> {
    let probes = [
        ("codex", "Codex CLI", "codex", vec!["--version"]),
        ("opencode", "opencode", "opencode", vec!["--version"]),
    ];
    probes
        .into_iter()
        .map(|(id, label, command, args)| {
            let (path, version, available) = run_probe(command, &args);
            LocalAiTool {
                id: id.to_string(),
                label: label.to_string(),
                command: command.to_string(),
                path,
                version,
                available,
            }
        })
        .collect()
}
```

Register the command in the invoke handler.

- [ ] **Step 5: Expose discovery to renderer**

In `src/renderer/tauri-api.js`, add:

```js
discoverLocalAiTools: () => invoke('discover_local_ai_tools'),
```

- [ ] **Step 6: Render AI & Tools settings**

In `src/renderer/shell.js`, add local state:

```js
let localAiTools = []
```

Add:

```js
function renderLocalToolDiscovery() {
  const rows = localAiTools.length
    ? localAiTools.map(tool => `
      <div class="local-tool-row">
        <span class="local-tool-row__name">${escapeAttribute(tool.label)}</span>
        <span class="local-tool-row__status">${tool.available ? 'Available' : 'Not found'}</span>
        <span class="local-tool-row__meta">${escapeAttribute(tool.version || tool.path || tool.command)}</span>
      </div>
    `).join('')
    : '<div class="settings-muted">Run discovery to check local AI tools.</div>'
  return `
    <section class="settings-field">
      <div class="settings-field__row">
        <span class="settings-field__label">Local AI tools</span>
        <span class="settings-btn settings-btn--muted" data-local-tool-refresh role="button" tabindex="0">Refresh</span>
      </div>
      <div class="local-tool-list">${rows}</div>
    </section>
  `
}
```

In `handleSettingsClick`, add:

```js
const localToolRefresh = event.target.closest('[data-local-tool-refresh]')
if (localToolRefresh) {
  refreshLocalAiTools()
  return
}
```

Add:

```js
async function refreshLocalAiTools() {
  try {
    localAiTools = await window.fjord.discoverLocalAiTools()
  } catch {
    localAiTools = []
  }
  syncSettingsForm()
}
```

Call `renderLocalToolDiscovery()` inside the `ai-tools` settings page.

- [ ] **Step 7: Update AI chat provider handling**

In `src/renderer/ai-chat.js`, pass `provider.protocol` and guard unsupported CLI transports:

```js
if (provider.transport === 'cli') {
  return {
    provider: providerKey,
    apiKey: '',
    model: s.aiModel || provider.defaultModel,
    baseUrl: '',
    label: provider.label,
    noApiKey: true,
    transport: 'cli',
  }
}
```

Before sending HTTP chat, show a status/error for CLI providers:

```js
if (config.transport === 'cli') {
  _messages.push({ role: '__status', content: `${config.label} discovery is available in Settings. Chat transport will be wired in a later pass.` })
  renderMessages()
  return
}
```

- [ ] **Step 8: Update Tauri AI HTTP provider bridge**

In `src-tauri/src/main.rs`, treat `"openrouter"` and `"custom-openai-compatible"` like OpenAI-compatible providers:

```rust
"openai" | "openrouter" | "custom-openai-compatible" => {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let mut payload = json!({ "model": model, "messages": messages });
    if let Some(tools) = tools {
        payload["tools"] = tools;
    }
    let request = client
        .post(&url)
        .header("content-type", "application/json")
        .bearer_auth(api_key);
    (url, payload, request)
}
```

Update response parsing match arm similarly:

```rust
"openai" | "openrouter" | "custom-openai-compatible" => {
    let message = json_value
        .pointer("/choices/0/message")
        .cloned()
        .unwrap_or(json!({}));
    json!({
        "text": message.get("content").and_then(Value::as_str).unwrap_or(""),
        "stop": json_value.pointer("/choices/0/finish_reason").cloned().unwrap_or(Value::Null)
    })
}
```

- [ ] **Step 9: Run tests and commit**

Run:

```bash
npm test -- tests/ui-contract.test.js
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
git diff --check
```

Commit:

```bash
git add tests/ui-contract.test.js src/renderer/ai-providers.js src/renderer/settings.js src/renderer/shell.js src/renderer/tauri-api.js src-tauri/src/main.rs src/renderer/ai-chat.js src/renderer/styles/main.css
git commit -m "feat: expand ai providers and tool discovery"
```

## Task 6: Assistant Docking And Dedicated Rail

**Files:**
- Modify: `tests/ui-contract.test.js`
- Create: `src/renderer/assistant-rail.js`
- Modify: `src/renderer/shell.js`
- Modify: `src/renderer/index.js`
- Modify: `src/renderer/ai-chat.js`
- Modify: `src/renderer/styles/main.css`

- [ ] **Step 1: Write the failing test**

Add:

```js
test('assistant can dock as widget or dedicated side rail', () => {
  const settings = read('src/renderer/settings.js')
  const shell = read('src/renderer/shell.js')
  const index = read('src/renderer/index.js')
  const assistantRail = read('src/renderer/assistant-rail.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(settings, /ASSISTANT_DOCK_OPTIONS/)
  assert.match(settings, /right-rail/)
  assert.match(settings, /left-rail/)
  assert.match(shell, /renderSelectSetting\('assistantDock'/)
  assert.match(index, /buildAssistantRail/)
  assert.match(assistantRail, /export function buildAssistantRail/)
  assert.match(assistantRail, /mountAssistantRail/)
  assert.match(assistantRail, /assistant-rail/)
  assert.match(css, /\.assistant-rail/)
  assert.match(css, /\.assistant-rail\.open/)
})
```

Because this test reads a new file, first create an empty `src/renderer/assistant-rail.js` before running it:

```bash
touch src/renderer/assistant-rail.js
npm test -- tests/ui-contract.test.js
```

Expected: FAIL on missing content.

- [ ] **Step 2: Refactor AI chat build/mount exports**

In `src/renderer/ai-chat.js`, export the panel functions:

```js
export function buildAiChatPanel() {
  return buildPanel()
}

export function mountAiChatPanel() {
  mountPanel()
}
```

If current function names differ, preserve existing `initAiChatPanel` behavior and delegate to the exported wrappers.

- [ ] **Step 3: Create assistant rail module**

In `src/renderer/assistant-rail.js`, add:

```js
import { getSettings } from './settings.js'
import { buildAiChatPanel, mountAiChatPanel } from './ai-chat.js'

export function buildAssistantRail() {
  return `
    <aside class="assistant-rail" id="assistant-rail" aria-label="AI assistant">
      <div class="assistant-rail__header">
        <span>Assistant</span>
        <span class="assistant-rail__mode" id="assistant-rail-mode">AI</span>
      </div>
      <div class="assistant-rail__body" id="assistant-rail-body"></div>
    </aside>
  `
}

export function syncAssistantRail() {
  const rail = document.getElementById('assistant-rail')
  if (!rail) return
  const dock = getSettings().assistantDock
  rail.classList.toggle('open', dock === 'right-rail' || dock === 'left-rail')
  rail.classList.toggle('assistant-rail--left', dock === 'left-rail')
  rail.classList.toggle('assistant-rail--right', dock !== 'left-rail')
}

export function mountAssistantRail() {
  const body = document.getElementById('assistant-rail-body')
  if (!body || body.firstChild) {
    syncAssistantRail()
    return
  }
  body.innerHTML = buildAiChatPanel()
  mountAiChatPanel()
  syncAssistantRail()
}
```

- [ ] **Step 4: Mount rail in shell/index**

Where the root shell layout is built in `src/renderer/shell.js`, include:

```js
${buildAssistantRail()}
```

Import `buildAssistantRail` into `shell.js`.

In `src/renderer/index.js`, import and call:

```js
import { mountAssistantRail, syncAssistantRail } from './assistant-rail.js'
```

After shell build/mount:

```js
mountAssistantRail()
```

When settings change, call `syncAssistantRail()` after `updateSetting('assistantDock', ...)`.

- [ ] **Step 5: Render assistant dock setting**

In AI & Tools settings in `src/renderer/shell.js`, add:

```js
${renderSelectSetting('assistantDock', 'Assistant placement', ASSISTANT_DOCK_OPTIONS)}
```

Import `ASSISTANT_DOCK_OPTIONS`.

- [ ] **Step 6: Add rail CSS**

Add:

```css
.assistant-rail {
  width: 0;
  opacity: 0;
  overflow: hidden;
  flex-shrink: 0;
  border-left: 1px solid var(--surface-edge);
  background: var(--surface-bg-1);
  display: flex;
  flex-direction: column;
  transition: width 0.18s ease, opacity 0.18s ease;
}
.assistant-rail.open {
  width: 340px;
  opacity: 1;
}
.assistant-rail--left {
  order: -1;
  border-left: none;
  border-right: 1px solid var(--surface-edge);
}
.assistant-rail__header {
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 10px;
  border-bottom: 1px solid var(--surface-edge);
  color: var(--text2);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.assistant-rail__body {
  flex: 1;
  min-height: 0;
  display: flex;
}
```

- [ ] **Step 7: Run tests and commit**

Run:

```bash
npm test -- tests/ui-contract.test.js
npm run build
git diff --check
```

Commit:

```bash
git add tests/ui-contract.test.js src/renderer/assistant-rail.js src/renderer/shell.js src/renderer/index.js src/renderer/ai-chat.js src/renderer/settings.js src/renderer/styles/main.css
git commit -m "feat: add assistant docking rail"
```

## Final Verification

- [ ] Run full contract suite:

```bash
npm test -- tests/ui-contract.test.js
```

Expected: all tests pass.

- [ ] Run production build:

```bash
npm run build
```

Expected: build completes without errors.

- [ ] Run Rust check:

```bash
cargo check --manifest-path src-tauri/Cargo.toml
```

Expected: check completes without errors.

- [ ] Run whitespace check:

```bash
git diff --check
```

Expected: no output.

- [ ] Launch the app:

```bash
npm run dev
```

Expected manual checks:

- Settings tabs are Appearance, Typography, Editor, Workspace, AI & Tools, Shortcuts.
- Font picker popovers are styled by Rista and do not use OS select popovers.
- Welcome screen is a compact start surface with recent/pinned workspaces.
- Right widgets have IDE pane chrome and still drag/collapse/close.
- Terminal shows shell name, command output, exit code, and duration.
- AI & Tools lists expanded providers and can refresh local tool discovery.
- Assistant placement setting can show widget mode or dedicated rail mode.
