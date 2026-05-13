import { state, $, settingsValue } from './state.js'
import { toggleTheme, getTheme } from './theme.js'
import { applySettings, getSettings, updateSetting, resetSettings, FONT_OPTIONS } from './settings.js'
import { clearDiagramCache, initDiagrams } from './diagrams.js'
import { sunIcon, moonIcon, gearIcon, closeIcon } from './icons.js'
import { closeCommandDialog, submitCommandDialog } from './commands.js'
import { updateEditorTheme } from './editor.js'
import { PANE_KEYS, editorViews, richEditors, syncingRichEditor } from './state.js'

// ── Callback registration ────────────────────────────────────────
let _callbacks = {}
export function registerShellCallbacks(cbs) { Object.assign(_callbacks, cbs) }

// ── Welcome screen HTML ──────────────────────────────────────────
export function buildWelcome() {
  const hasFolder = Boolean(state.folderPath)
  return `
    <div class="welcome" id="welcome">
      <div class="welcome__content">
        <div class="welcome__logo">fjord<span>mark</span></div>
        <div class="welcome__sub">${hasFolder ? 'Choose a note from the explorer or create a new markdown file in this folder' : 'Open a folder to start writing'}</div>
        ${hasFolder ? '' : `
          <div class="welcome__btn" id="welcome-open-btn">
            <svg viewBox="0 0 16 16"><path d="M2 5h4l2-2h6a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/></svg>
            Open folder…
          </div>
        `}
      </div>
    </div>
  `
}

// ── Settings form helpers ────────────────────────────────────────
export function renderSelectSetting(key, label, options) {
  return `
    <label class="settings-field">
      <span class="settings-field__label">${label}</span>
      <select class="settings-select" data-setting="${key}">
        ${options.map(option => `<option value="${option.value}"${option.value === settingsValue(key) ? ' selected' : ''}>${option.label}</option>`).join('')}
      </select>
    </label>
  `
}

export function renderRangeSetting(key, label, min, max, step, unit) {
  const value = settingsValue(key)
  return `
    <label class="settings-field">
      <span class="settings-field__row">
        <span class="settings-field__label">${label}</span>
        <span class="settings-field__value" id="${key}-value">${formatSettingValue(value, unit)}</span>
      </span>
      <input
        class="settings-range"
        type="range"
        min="${min}"
        max="${max}"
        step="${step}"
        value="${value}"
        data-setting="${key}"
        data-unit="${unit}"
      >
    </label>
  `
}

export function renderTextSetting(key, label, placeholder) {
  return `
    <label class="settings-field">
      <span class="settings-field__label">${label}</span>
      <input
        class="settings-input"
        type="text"
        placeholder="${placeholder}"
        value="${escapeAttribute(settingsValue(key) || '')}"
        data-setting="${key}"
      >
    </label>
  `
}

export function renderToggleSetting(key, label, description) {
  return `
    <label class="settings-toggle">
      <div class="settings-toggle__copy">
        <span class="settings-field__label">${label}</span>
        <span class="settings-toggle__description">${description}</span>
      </div>
      <input class="settings-toggle__input" type="checkbox" ${settingsValue(key) ? 'checked' : ''} data-setting="${key}">
      <span class="settings-toggle__ui"></span>
    </label>
  `
}

function escapeAttribute(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function formatSettingValue(value, unit) {
  if (unit === 'px') return `${value}${unit}`
  if (unit === '%') return `${value}${unit}`
  return Number(value).toFixed(2).replace(/\.00$/, '')
}

function handleSettingsInput(event) {
  const input = event.target.closest('[data-setting]')
  if (!input) return
  const value = input.type === 'checkbox' ? input.checked : input.value
  const next = updateSetting(input.dataset.setting, value)
  if (input.type !== 'checkbox') updateSettingValueLabel(input.dataset.setting, value, input.dataset.unit || '')
  if (input.dataset.setting.endsWith('Color')) input.value = next[input.dataset.setting] || ''
}

function updateSettingValueLabel(key, value, unit) {
  const label = $(`${key}-value`)
  if (label) label.textContent = formatSettingValue(value, unit)
}

function syncSettingsForm() {
  const settings = getSettings()
  document.querySelectorAll('#settings-panel [data-setting]').forEach(input => {
    const key = input.dataset.setting
    if (input.type === 'checkbox') {
      input.checked = Boolean(settings[key])
    } else {
      input.value = settings[key]
      updateSettingValueLabel(key, settings[key], input.dataset.unit || '')
    }
  })
}

export function toggleSettingsPanel() {
  state.settingsOpen ? closeSettingsPanel() : openSettingsPanel()
}

export function openSettingsPanel() {
  state.settingsOpen = true
  $('app')?.classList.add('settings-open')
  syncSettingsForm()
}

export function closeSettingsPanel() {
  state.settingsOpen = false
  $('app')?.classList.remove('settings-open')
}

function syncAppMeta() {
  const meta = $('settings-app-meta')
  if (!meta) return
  meta.textContent = state.appMeta.version
    ? `${state.appMeta.name} v${state.appMeta.version}`
    : state.appMeta.name
}

// ── Build app shell ───────────────────────────────────────────────
export function buildShell() {
  document.getElementById('root').innerHTML = `
    <div class="app" id="app">

      <!-- Titlebar -->
      <div class="titlebar" id="titlebar">
        <div class="titlebar__spacer"></div>
        <div class="titlebar__right">
          <div class="theme-btn" id="toolbar-toggle" title="Hide toolbars">
            <svg viewBox="0 0 16 16"><path d="M2 4.5h12M2 8h12M2 11.5h12"/></svg>
          </div>
          <div class="theme-btn" id="workspace-split-toggle" title="Toggle split view">
            <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1.5"/><line x1="8" y1="3" x2="8" y2="13"/></svg>
          </div>
          <div class="theme-btn theme-btn--settings" id="settings-btn" title="Settings">
            ${gearIcon()}
          </div>
          <div class="theme-btn theme-btn--theme" id="theme-btn" title="Toggle theme">
            ${sunIcon()}
          </div>
          <div class="ic" id="sidebar-toggle" title="Toggle sidebar">
            <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1"/><line x1="6" y1="3" x2="6" y2="13"/></svg>
          </div>
        </div>
      </div>

      <!-- Layout -->
      <div class="layout">

        <!-- Sidebar -->
        <div class="sidebar" id="sidebar">
          <div class="sidebar__header">
            <span class="sidebar__label" id="sidebar-label">Explorer</span>
            <div class="sidebar__collapse-all" id="collapse-all-btn" title="Collapse all">Collapse all</div>
          </div>
          <div class="sidebar__open-btn" id="open-folder-btn">
            <svg viewBox="0 0 16 16"><path d="M2 5h4l2-2h6a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/></svg>
            Open folder…
          </div>
          <div class="file-tree" id="file-tree"></div>
        </div>
        <div class="sidebar-resizer" id="sidebar-resizer" title="Resize explorer"></div>

        <!-- Editor area -->
        <div class="editor-area">
          <!-- Welcome / editor wrapper -->
          <div id="editor-wrapper" style="flex:1;display:flex;flex-direction:column;overflow:hidden">
            ${buildWelcome()}
          </div>

        </div>
      </div>

      <!-- Statusbar -->
      <div class="statusbar">
        <div class="st"><div class="st-dot"></div><span id="st-mode">Markdown</span></div>
        <div class="st" id="st-words">—</div>
        <div class="st" id="st-lines">—</div>
        <div class="st" id="st-cursor">Ln 1, Col 1</div>
        <div class="st" style="margin-left:auto;color:var(--text3)">fjordmark</div>
      </div>

      <div class="settings-overlay" id="settings-overlay"></div>
      <aside class="settings-panel" id="settings-panel">
        <div class="settings-panel__header">
          <div>
            <div class="settings-panel__eyebrow">Preferences</div>
            <div class="settings-panel__title">Settings</div>
          </div>
          <div class="theme-btn" id="settings-close-btn" title="Close settings">
            ${closeIcon()}
          </div>
        </div>

        <div class="settings-panel__body">
          <section class="settings-group">
            <div class="settings-group__title">Atmosphere</div>
            ${renderToggleSetting('ambientBackground', 'Show ambient background', 'Keep the aurora background visible while editing')}
            ${renderRangeSetting('ambientIntensity', 'Background strength', 0, 100, 1, '%')}
            ${renderRangeSetting('surfaceOpacity', 'Translucency', 45, 100, 1, '%')}
            ${renderRangeSetting('surfaceBlur', 'Glass blur', 0, 32, 1, 'px')}
          </section>

          <section class="settings-group">
            <div class="settings-group__title">Interface</div>
            ${renderRangeSetting('contrastBoost', 'Contrast boost', 0, 40, 1, '%')}
            ${renderTextSetting('textColor', 'Primary text color', 'Optional hex color, e.g. #f2f5ff')}
            ${renderTextSetting('mutedTextColor', 'Secondary text color', 'Optional hex color, e.g. #a7b0c0')}
            ${renderTextSetting('subtleTextColor', 'Subtle text color', 'Optional hex color, e.g. #6d7483')}
            ${renderTextSetting('accentColor', 'Accent color', 'Optional hex color, e.g. #7ba3cc')}
            ${renderSelectSetting('uiFont', 'App font preset', FONT_OPTIONS.ui)}
            ${renderTextSetting('uiFontCustom', 'Custom app font stack', "Example: 'Atkinson Hyperlegible', system-ui, sans-serif")}
            ${renderRangeSetting('uiFontSize', 'App size', 11, 16, 1, 'px')}
            ${renderSelectSetting('explorerFont', 'Explorer font preset', FONT_OPTIONS.explorer)}
            ${renderTextSetting('explorerFontCustom', 'Custom explorer font stack', "Example: 'Inter', system-ui, sans-serif")}
            ${renderRangeSetting('explorerFontSize', 'Explorer size', 11, 16, 1, 'px')}
          </section>

          <section class="settings-group">
            <div class="settings-group__title">Editor</div>
            ${renderSelectSetting('editorFont', 'Editor font preset', FONT_OPTIONS.editor)}
            ${renderTextSetting('editorFontCustom', 'Custom editor font stack', "Example: 'JetBrains Mono', 'SF Mono', monospace")}
            ${renderRangeSetting('editorFontSize', 'Editor size', 12, 18, 1, 'px')}
            ${renderRangeSetting('editorLineHeight', 'Editor spacing', 1.4, 2.1, 0.05, '')}
            ${renderTextSetting('editorTextColor', 'Editor text color', 'Optional hex color, e.g. #e7ecf7')}
          </section>

          <section class="settings-group">
            <div class="settings-group__title">Preview</div>
            ${renderSelectSetting('previewFont', 'Preview font preset', FONT_OPTIONS.preview)}
            ${renderTextSetting('previewFontCustom', 'Custom preview font stack', "Example: 'Source Serif 4', Georgia, serif")}
            ${renderRangeSetting('previewFontSize', 'Preview size', 12, 18, 1, 'px')}
            ${renderRangeSetting('previewLineHeight', 'Preview spacing', 1.4, 2.1, 0.05, '')}
            ${renderTextSetting('previewTextColor', 'Preview text color', 'Optional hex color, e.g. #f1f4fa')}
          </section>
        </div>

        <div class="settings-panel__footer">
          <div class="settings-panel__meta" id="settings-app-meta">Fjordmark</div>
          <button class="settings-btn settings-btn--muted" id="settings-reset-btn">Reset</button>
          <button class="settings-btn" id="settings-done-btn">Done</button>
        </div>
      </aside>

      <div class="command-dialog-overlay" id="command-dialog-overlay"></div>
      <div class="command-dialog" id="command-dialog">
        <form class="command-dialog__panel" id="command-dialog-form">
          <div class="command-dialog__header">
            <div>
              <div class="command-dialog__eyebrow" id="command-dialog-eyebrow">Insert</div>
              <div class="command-dialog__title" id="command-dialog-title">Command</div>
            </div>
            <div class="theme-btn" id="command-dialog-close" title="Close">
              ${closeIcon()}
            </div>
          </div>
          <div class="command-dialog__body" id="command-dialog-body"></div>
          <div class="command-dialog__footer">
            <button type="button" class="settings-btn settings-btn--muted" id="command-dialog-cancel">Cancel</button>
            <button type="submit" class="settings-btn" id="command-dialog-submit">Insert</button>
          </div>
        </form>
      </div>

    </div>
  `

  // Wire up controls
  $('settings-btn').addEventListener('click', toggleSettingsPanel)
  $('toolbar-toggle').addEventListener('click', () => _callbacks.toggleToolbar?.())
  $('workspace-split-toggle').addEventListener('click', () => _callbacks.toggleWorkspaceSplit?.())
  $('settings-close-btn').addEventListener('click', closeSettingsPanel)
  $('settings-done-btn').addEventListener('click', closeSettingsPanel)
  $('settings-reset-btn').addEventListener('click', () => {
    resetSettings()
    syncSettingsForm()
  })
  $('settings-overlay').addEventListener('click', closeSettingsPanel)
  $('settings-panel').addEventListener('input', handleSettingsInput)
  $('command-dialog-close').addEventListener('click', closeCommandDialog)
  $('command-dialog-cancel').addEventListener('click', closeCommandDialog)
  $('command-dialog-overlay').addEventListener('click', closeCommandDialog)
  $('command-dialog-form').addEventListener('submit', submitCommandDialog)

  $('theme-btn').addEventListener('click', () => {
    const t = toggleTheme()
    applySettings(getSettings())
    clearDiagramCache()
    initDiagrams(t)
    $('theme-btn').innerHTML = t === 'dark' ? sunIcon() : moonIcon()
    PANE_KEYS.forEach(pane => {
      if (editorViews[pane]) updateEditorTheme(editorViews[pane], t === 'dark')
      if (richEditors[pane]) {
        const markdown = richEditors[pane].getMarkdown()
        _callbacks.destroyRichEditor?.(pane)
        _callbacks.ensureRichEditorMounted?.(pane)
        if (richEditors[pane]) {
          syncingRichEditor[pane] = true
          richEditors[pane].setMarkdown(markdown)
          syncingRichEditor[pane] = false
        }
      }
    })
  })
  $('theme-btn').innerHTML = getTheme() === 'dark' ? sunIcon() : moonIcon()
  _callbacks.syncToolbarToggle?.()

  $('sidebar-toggle').addEventListener('click', () => _callbacks.toggleSidebar?.())
  $('open-folder-btn').addEventListener('click', () => _callbacks.openFolder?.())
  $('collapse-all-btn')?.addEventListener('click', () => _callbacks.collapseAllFolders?.())
  $('sidebar-resizer')?.addEventListener('pointerdown', e => _callbacks.startSidebarResize?.(e))
  $('welcome-open-btn')?.addEventListener('click', () => _callbacks.openFolder?.())

  // Watch for file changes from main process
  if (window.fjord) {
    window.fjord.appMeta?.().then(meta => {
      if (!meta) return
      state.appMeta = meta
      syncAppMeta()
    }).catch(() => {})

    window.fjord.onCommand?.(({ command }) => _callbacks.handleAppCommand?.(command))

    window.fjord.onFileChange(({ event, path: p }) => {
      const tab = state.tabs.find(t => t.path === p)
      if (tab && !tab.dirty) _callbacks.loadFileIntoTab?.(tab)
      _callbacks.refreshTree?.()
    })
  }
}
