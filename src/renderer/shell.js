import { state, $, settingsValue } from './state.js'
import { buildRightPanelContainer } from './right-panel.js'
import { buildTerminalDrawer } from './terminal-drawer.js'
import { toggleTheme, getTheme } from './theme.js'
import { applySettings, getSettings, setSettings, updateSetting, resetSettings, APP_ICON_VARIANTS, ASSISTANT_DOCK_OPTIONS, FONT_OPTIONS, THEME_PRESETS } from './settings.js'
import { getAllBindings, setBinding, resetBinding, findConflict, formatKeyEvent } from './keybindings.js'
import { clearDiagramCache, initDiagrams } from './diagrams.js'
import { sunIcon, moonIcon, gearIcon, toolbarIcon, sidebarIcon, workspaceSplitIcon, closeIcon, terminalIcon, rightSidebarIcon } from './icons.js'
import { closeCommandDialog, submitCommandDialog } from './commands.js'
import { updateEditorTheme } from './editor.js'
import { PANE_KEYS, editorViews, richEditors, syncingRichEditor } from './state.js'
import { showContextMenu } from './context-menu.js'
import { renderPinnedProjectsHtml, renderRecentProjectsHtml, removeRecentProject, togglePinnedProject, unpinProject } from './recent-projects.js'
import { PROVIDERS } from './ai-providers.js'
import { buildAssistantRail } from './assistant-rail.js'

// ── Callback registration ────────────────────────────────────────
let _callbacks = {}
export function registerShellCallbacks(cbs) { Object.assign(_callbacks, cbs) }

const SETTINGS_TABS = [
  { id: 'appearance', label: 'Appearance' },
  { id: 'typography', label: 'Typography' },
  { id: 'editor', label: 'Editor' },
  { id: 'workspace', label: 'Workspace' },
  { id: 'ai-tools', label: 'AI & Tools' },
  { id: 'shortcuts', label: 'Shortcuts' },
]

let activeSettingsTab = 'appearance'
let activeKeybindingCapture = null
let localAiTools = []

function folderName(folderPath) {
  return String(folderPath || '').split(/[\\/]/).filter(Boolean).pop() || 'No folder open'
}

export function syncWorkspaceChrome() {
  const nameEl = $('brandrail-workspace-name')
  const pathEl = $('brandrail-workspace-path')
  const railEl = $('brandrail')
  if (!nameEl || !pathEl || !railEl) return
  const hasFolder = Boolean(state.folderPath)
  const hasSingleFile = Boolean(state.singleFilePath)
  railEl.classList.toggle('has-workspace', hasFolder || hasSingleFile)
  if (hasSingleFile && !hasFolder) {
    nameEl.textContent = folderName(state.singleFilePath)
    pathEl.textContent = 'Single file'
    railEl.title = state.singleFilePath
    return
  }
  nameEl.textContent = hasFolder ? folderName(state.folderPath) : 'No workspace'
  pathEl.textContent = hasFolder ? state.folderPath : 'Open a folder or create a new window'
  railEl.title = hasFolder ? state.folderPath : ''
}

// ── Welcome screen HTML ──────────────────────────────────────────
export function buildWelcome() {
  const hasFolder = Boolean(state.folderPath)
  const pinnedHtml = hasFolder ? '' : renderPinnedProjectsHtml()
  const recentHtml = hasFolder ? '' : renderRecentProjectsHtml()
  const workspaceHtml = pinnedHtml || recentHtml
    ? `
      <div class="welcome__workspace-panel">
        <div class="welcome__panel-head">
          <span>Workspaces</span>
          <span>⌘K quick open</span>
        </div>
        ${pinnedHtml}
        ${recentHtml}
      </div>
    `
    : ''
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
          <span>Works offline</span>
        </div>
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

export function renderSettingsTabs() {
  return `
    <nav class="settings-tabs" aria-label="Settings sections">
      ${SETTINGS_TABS.map(tab => `
        <div
          class="settings-tab${tab.id === activeSettingsTab ? ' active' : ''}"
          data-settings-tab="${tab.id}"
          role="button"
          tabindex="0"
        >${tab.label}</div>
      `).join('')}
    </nav>
  `
}

export function renderPresetPicker(key, label, presets) {
  const current = settingsValue(key)
  return `
    <section class="settings-field settings-field--presets">
      <div class="settings-field__row">
        <span class="settings-field__label">${label}</span>
        <span class="settings-field__value">${presets.find(preset => preset.value === current)?.label || presets[0]?.label || ''}</span>
      </div>
      <div class="settings-preset-grid" data-preset-group="${key}">
        ${presets.map(preset => `
          <div
            class="settings-preset${preset.value === current ? ' active' : ''}"
            data-preset-setting="${key}"
            data-preset-value="${preset.value}"
            role="button"
            tabindex="0"
            aria-label="Use ${escapeAttribute(preset.label)} preset"
          >
            <span class="settings-swatch settings-swatch--${escapeAttribute(preset.value)}">
              <span></span><span></span><span></span>
            </span>
            <span class="settings-preset__label">${preset.label}</span>
          </div>
        `).join('')}
      </div>
    </section>
  `
}

function renderAppIconPicker() {
  const currentVariant = settingsValue('appIconVariant')
  const currentTheme = settingsValue('appIconTheme')
  return `
    <section class="settings-field settings-field--app-icon">
      <div class="settings-field__row">
        <span class="settings-field__label">App icon</span>
        <span class="settings-field__value">${APP_ICON_VARIANTS.find(icon => icon.value === currentVariant)?.label || 'Aurora Gradient'}</span>
      </div>
      <div class="settings-icon-theme" role="group" aria-label="App icon theme">
        ${[
          { value: 'auto', label: 'Auto' },
          { value: 'dark', label: 'Dark' },
          { value: 'light', label: 'Light' },
        ].map(option => `
          <div
            class="settings-icon-theme__option${option.value === currentTheme ? ' active' : ''}"
            data-app-icon-theme="${option.value}"
            role="button"
            tabindex="0"
          >${option.label}</div>
        `).join('')}
      </div>
      <div class="settings-icon-grid">
        ${APP_ICON_VARIANTS.map(icon => `
          <div
            class="settings-icon-option${icon.value === currentVariant ? ' active' : ''}"
            data-app-icon-variant="${icon.value}"
            role="button"
            tabindex="0"
            aria-label="Use ${escapeAttribute(icon.label)} app icon"
          >
            <span class="settings-icon-option__preview" aria-hidden="true">
              <img src="logos/rista-split-rune-${icon.value}-dark.svg" alt="">
              <img src="logos/rista-split-rune-${icon.value}-light.svg" alt="">
            </span>
            <span class="settings-icon-option__label">${icon.label}</span>
          </div>
        `).join('')}
      </div>
    </section>
  `
}

function renderLocalToolRows() {
  if (!localAiTools.length) {
    return '<div class="settings-muted">Run discovery to check local AI tools.</div>'
  }

  return localAiTools.map(tool => `
    <div class="local-tool-row${tool.available ? ' is-available' : ''}">
      <div class="local-tool-row__main">
        <span class="local-tool-row__label">${escapeAttribute(tool.label || tool.id || tool.command)}</span>
        <span class="local-tool-row__meta">${escapeAttribute(tool.path || tool.command || '')}</span>
      </div>
      <div class="local-tool-row__status">
        <span class="local-tool-row__pill">${tool.available ? 'Available' : 'Missing'}</span>
        ${tool.version ? `<span class="local-tool-row__version">${escapeAttribute(tool.version)}</span>` : ''}
      </div>
    </div>
  `).join('')
}

function renderLocalToolDiscovery() {
  return `
    <section class="settings-field" data-local-tool-discovery>
      <div class="settings-field__row">
        <span class="settings-field__label">Local AI tools</span>
        <span class="settings-btn settings-btn--muted" data-local-tool-refresh role="button" tabindex="0">Refresh</span>
      </div>
      <div class="local-tool-list">${renderLocalToolRows()}</div>
    </section>
  `
}

function renderKeybindingRows() {
  return Object.values(getAllBindings()).map(binding => `
    <div class="keybinding-row">
      <div class="keybinding-row__copy">
        <div class="keybinding-row__label">${binding.label}</div>
        <div class="keybinding-row__meta">Default: ${binding.default}</div>
      </div>
      <div class="keybinding-row__actions">
        <button
          type="button"
          class="keybinding-capture${activeKeybindingCapture === binding.id ? ' is-listening' : ''}"
          data-keybinding-capture="${binding.id}"
        >${activeKeybindingCapture === binding.id ? 'Press keys...' : binding.current}</button>
        <button
          type="button"
          class="settings-btn settings-btn--muted keybinding-reset"
          data-keybinding-reset="${binding.id}"
          ${binding.isOverridden ? '' : 'disabled'}
        >Reset</button>
      </div>
    </div>
  `).join('')
}

function syncKeybindingList() {
  const list = $('keybinding-list')
  if (!list) return
  list.innerHTML = renderKeybindingRows()
}

function syncPinnedWorkspaceList() {
  const list = $('pinned-workspaces-list')
  if (!list) return
  list.innerHTML = renderPinnedProjectsHtml({ empty: true })
}

function syncLocalToolDiscovery() {
  const list = document.querySelector('[data-local-tool-discovery] .local-tool-list')
  if (!list) return
  list.innerHTML = renderLocalToolRows()
}

async function refreshLocalAiTools() {
  try {
    localAiTools = await window.fjord.discoverLocalAiTools()
  } catch {
    localAiTools = []
  }
  syncSettingsForm()
}

function renderProviderOptions() {
  return Object.entries(PROVIDERS).map(([value, provider]) => ({
    value,
    label: provider.label,
  }))
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
  if (['hideFrontmatterInRenderedModes', 'showDocumentBanners'].includes(input.dataset.setting)) refreshRenderedDocuments()
  if (input.dataset.setting === 'assistantDock') _callbacks.syncAssistantRail?.()
  if (['typewriterScrolling', 'spellcheck', 'vimMode', 'smartTypography', 'focusMode', 'livePreview', 'posHighlight'].includes(input.dataset.setting)) _callbacks.applyEditorSettings?.()
}

function refreshRenderedDocuments() {
  _callbacks.refreshAllPreviews?.()
}

function resolveAppIconTheme(settings = getSettings()) {
  if (settings.appIconTheme === 'dark' || settings.appIconTheme === 'light') return settings.appIconTheme
  return getTheme() === 'light' ? 'light' : 'dark'
}

export function applySelectedAppIcon(settings = getSettings()) {
  window.fjord?.setAppIcon?.(settings.appIconVariant, resolveAppIconTheme(settings)).catch(() => {})
}

function handleSettingsClick(event) {
  const localToolRefresh = event.target.closest('[data-local-tool-refresh]')
  if (localToolRefresh) {
    refreshLocalAiTools()
    return
  }

  const capture = event.target.closest('[data-keybinding-capture]')
  if (capture) {
    activeKeybindingCapture = activeKeybindingCapture === capture.dataset.keybindingCapture
      ? null
      : capture.dataset.keybindingCapture
    syncKeybindingList()
    return
  }

  const reset = event.target.closest('[data-keybinding-reset]')
  if (reset) {
    resetBinding(reset.dataset.keybindingReset)
    activeKeybindingCapture = null
    syncSettingsForm()
    return
  }

  const tab = event.target.closest('[data-settings-tab]')
  if (tab) {
    setSettingsTab(tab.dataset.settingsTab)
    return
  }

  const openPath = event.target.closest('[data-open-path]')
  if (openPath) {
    closeSettingsPanel()
    _callbacks.openRecentProject?.(openPath.dataset.openPath)
    return
  }

  const openNewPath = event.target.closest('[data-open-new-path]')
  if (openNewPath) {
    _callbacks.openWorkspaceInNewWindow?.(openNewPath.dataset.openNewPath)
    return
  }

  const unpinPath = event.target.closest('[data-unpin-path]')
  if (unpinPath) {
    unpinProject(unpinPath.dataset.unpinPath)
    syncPinnedWorkspaceList()
    return
  }

  const iconVariant = event.target.closest('[data-app-icon-variant]')
  if (iconVariant) {
    const next = updateSetting('appIconVariant', iconVariant.dataset.appIconVariant)
    syncSettingsForm()
    applySelectedAppIcon(next)
    return
  }

  const iconTheme = event.target.closest('[data-app-icon-theme]')
  if (iconTheme) {
    const next = updateSetting('appIconTheme', iconTheme.dataset.appIconTheme)
    syncSettingsForm()
    applySelectedAppIcon(next)
    return
  }

  const fontTrigger = event.target.closest('[data-font-picker-trigger]')
  if (fontTrigger) {
    const picker = fontTrigger.closest('[data-font-picker]')
    picker?.classList.toggle('open')
    return
  }

  const fontOption = event.target.closest('[data-font-option]')
  if (fontOption) {
    const fontSetting = fontOption.dataset.fontSetting
    setSettings({
      [fontSetting]: fontOption.dataset.fontOption,
      [`${fontSetting}Custom`]: '',
    })
    fontOption.closest('[data-font-picker]')?.classList.remove('open')
    syncSettingsForm()
    return
  }

  const preset = event.target.closest('[data-preset-setting]')
  if (preset) {
    updateSetting(preset.dataset.presetSetting, preset.dataset.presetValue)
    syncSettingsForm()
  }
}

function handleSettingsKeydown(event) {
  if (activeKeybindingCapture) {
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'Escape') {
      activeKeybindingCapture = null
      syncKeybindingList()
      return
    }

    const combo = formatKeyEvent(event)
    if (!combo) return
    const conflict = findConflict(activeKeybindingCapture, combo)
    if (conflict) {
      const binding = getAllBindings()[conflict]
      alert(`${combo} is already assigned to ${binding?.label || conflict}`)
      return
    }

    setBinding(activeKeybindingCapture, combo)
    activeKeybindingCapture = null
    syncSettingsForm()
    return
  }

  if (event.key !== 'Enter' && event.key !== ' ') return
  const target = event.target.closest('[data-settings-tab], [data-preset-setting], [data-font-picker-trigger], [data-font-option], [data-app-icon-variant], [data-app-icon-theme], [data-open-path], [data-open-new-path], [data-unpin-path], [data-local-tool-refresh]')
  if (!target) return
  event.preventDefault()
  target.click()
}

function updateSettingValueLabel(key, value, unit) {
  const label = $(`${key}-value`)
  if (label) label.textContent = formatSettingValue(value, unit)
}

function setSettingsTab(tabId) {
  activeSettingsTab = SETTINGS_TABS.some(tab => tab.id === tabId) ? tabId : 'appearance'
  syncSettingsTabs()
}

export function attachWelcomeProjectHandlers(welcomeEl, handlers = {}) {
  if (!welcomeEl) return
  welcomeEl.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' && event.key !== ' ') return
    const action = event.target.closest('#welcome-open-btn, #welcome-new-file-btn, [data-pin-path], [data-remove-path], [data-open-path], [data-open-new-path]')
    if (!action) return
    event.preventDefault()
    action.click()
  })
  welcomeEl.addEventListener('click', (event) => {
    const pinBtn = event.target.closest('[data-pin-path]')
    if (pinBtn) {
      event.stopPropagation()
      togglePinnedProject(pinBtn.dataset.pinPath)
      handlers.refreshWelcome?.()
      return
    }

    const removeBtn = event.target.closest('[data-remove-path]')
    if (removeBtn) {
      event.stopPropagation()
      removeRecentProject(removeBtn.dataset.removePath)
      handlers.refreshWelcome?.()
      return
    }

    const openNewPath = event.target.closest('[data-open-new-path]')
    if (openNewPath) {
      event.stopPropagation()
      handlers.openProjectNewWindow?.(openNewPath.dataset.openNewPath)
      return
    }

    const openPath = event.target.closest('[data-open-path]')
    if (openPath) {
      event.stopPropagation()
      handlers.openProject?.(openPath.dataset.openPath)
      return
    }

    const recentItem = event.target.closest('.recent-item')
    if (recentItem) {
      const folderPath = recentItem.dataset.path
      if (folderPath) handlers.openProject?.(folderPath)
    }
  })
}

function syncSettingsTabs() {
  document.querySelectorAll('[data-settings-tab]').forEach(node => {
    node.classList.toggle('active', node.dataset.settingsTab === activeSettingsTab)
  })
  document.querySelectorAll('[data-settings-section]').forEach(node => {
    node.classList.toggle('active', node.dataset.settingsSection === activeSettingsTab)
  })
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
  document.querySelectorAll('#settings-panel [data-preset-setting]').forEach(node => {
    const key = node.dataset.presetSetting
    node.classList.toggle('active', node.dataset.presetValue === settings[key])
  })
  document.querySelectorAll('[data-preset-group]').forEach(group => {
    const key = group.dataset.presetGroup
    const valueLabel = group.closest('.settings-field')?.querySelector('.settings-field__value')
    const theme = key === 'lightThemePreset' ? 'light' : 'dark'
    const preset = THEME_PRESETS[theme].find(option => option.value === settings[key])
    if (valueLabel && preset) valueLabel.textContent = preset.label
  })
  document.querySelectorAll('#settings-panel [data-font-picker]').forEach(picker => {
    const key = picker.dataset.fontPicker
    const value = settings[key]
    const valueLabel = picker.querySelector('.settings-field__value')
    const sample = picker.querySelector('.settings-font-picker__sample')
    if (valueLabel) valueLabel.textContent = fontLabelFor(key)
    if (sample) sample.style.fontFamily = value
    picker.querySelectorAll('[data-font-option]').forEach(node => {
      node.classList.toggle('active', node.dataset.fontOption === value)
    })
  })
  document.querySelectorAll('#settings-panel [data-app-icon-variant]').forEach(node => {
    node.classList.toggle('active', node.dataset.appIconVariant === settings.appIconVariant)
  })
  document.querySelectorAll('#settings-panel [data-app-icon-theme]').forEach(node => {
    node.classList.toggle('active', node.dataset.appIconTheme === settings.appIconTheme)
  })
  const iconValueLabel = document.querySelector('.settings-field--app-icon .settings-field__value')
  const icon = APP_ICON_VARIANTS.find(option => option.value === settings.appIconVariant)
  if (iconValueLabel && icon) iconValueLabel.textContent = icon.label
  syncKeybindingList()
  syncPinnedWorkspaceList()
  syncLocalToolDiscovery()
  syncSettingsTabs()
}

export function toggleSettingsPanel() {
  state.settingsOpen ? closeSettingsPanel() : openSettingsPanel()
}

export function openSettingsPanel() {
  state.settingsOpen = true
  $('app')?.classList.add('settings-open')
  setSettingsTab(activeSettingsTab)
  syncSettingsForm()
}

export function closeSettingsPanel() {
  state.settingsOpen = false
  activeKeybindingCapture = null
  $('app')?.classList.remove('settings-open')
}

function syncAppMeta() {
  const meta = $('settings-app-meta')
  if (!meta) return
  meta.textContent = state.appMeta.version
    ? `${state.appMeta.name} v${state.appMeta.version}`
    : state.appMeta.name
}

const GLOBAL_CONTROL_SELECTOR = '#workspace-split-toggle, #settings-btn, #sidebar-toggle, #terminal-toggle, #right-sidebar-toggle'
function handleWindowDragRegionMouseDown(event) {
  if (event.button !== 0) return
  if (event.target.closest('input, textarea, select, button, [role="button"], [data-action], a')) return
  const startDrag = window.fjord?.startWindowDrag
  if (typeof startDrag !== 'function') return
  event.preventDefault()
  event.stopPropagation()
  Promise.resolve(startDrag()).catch(() => {})
}

function handleGlobalControlPointerDown(event) {
  const control = event.target.closest(GLOBAL_CONTROL_SELECTOR)
  if (!control) return
  event.preventDefault()
  event.stopPropagation()
  performGlobalControl(control.id)
}

function performGlobalControl(id) {
  if (id === 'workspace-split-toggle') _callbacks.toggleWorkspaceSplit?.()
  if (id === 'settings-btn') toggleSettingsPanel()
  if (id === 'sidebar-toggle') _callbacks.toggleSidebar?.()
  if (id === 'terminal-toggle') _callbacks.toggleTerminal?.()
  if (id === 'right-sidebar-toggle') _callbacks.toggleRightSidebar?.()
}

function handleGlobalControlKeydown(event) {
  if (event.key !== 'Enter' && event.key !== ' ') return
  const control = event.target.closest(GLOBAL_CONTROL_SELECTOR)
  if (!control) return
  event.preventDefault()
  performGlobalControl(control.id)
}

function refreshShellWelcome() {
  const wrapper = $('editor-wrapper')
  if (!wrapper || state.folderPath) return
  wrapper.innerHTML = buildWelcome()
  $('welcome-open-btn')?.addEventListener('click', () => _callbacks.openFolder?.())
  $('welcome-new-file-btn')?.addEventListener('click', () => _callbacks.createNewFile?.())
  attachWelcomeProjectHandlers($('welcome'), {
    openProject: (folderPath) => _callbacks.openRecentProject?.(folderPath),
    openProjectNewWindow: (folderPath) => _callbacks.openWorkspaceInNewWindow?.(folderPath),
    refreshWelcome: refreshShellWelcome,
  })
}

export function toggleAppTheme() {
  const t = toggleTheme()
  const settings = getSettings()
  const presetKey = t === 'light' ? settings.lightThemePreset : settings.darkThemePreset
  const preset = THEME_PRESETS[t].find(option => option.value === presetKey) || THEME_PRESETS[t][0]
  setSettings(preset.atmosphere)
  syncSettingsForm()
  applySelectedAppIcon()
  clearDiagramCache()
  initDiagrams(t)
  const themeBtn = $('theme-btn')
  if (themeBtn) themeBtn.innerHTML = t === 'dark' ? sunIcon() : moonIcon()
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
}

// ── File context menu ─────────────────────────────────────────────
function showFileContextMenu(x, y, filePath, isFolder) {
  const fileName = filePath.split('/').pop() || filePath.split('\\').pop()
  const items = []

  if (isFolder) {
    items.push({ label: 'New File\u2026', action: async () => {
      const name = prompt('File name:', 'untitled.md')
      if (!name) return
      const fullPath = filePath + '/' + name
      const result = await window.fjord.createFile(fullPath)
      if (result) _callbacks.refreshTree?.()
    }})
    items.push({ separator: true })
  }

  items.push({ label: 'Rename\u2026', action: async () => {
    const newName = prompt('Rename to:', fileName)
    if (!newName || newName === fileName) return
    const dir = filePath.substring(0, filePath.lastIndexOf('/'))
    const newPath = dir + '/' + newName
    const ok = await window.fjord.renameFile(filePath, newPath)
    if (ok) _callbacks.refreshTree?.()
  }})

  if (!isFolder) {
    items.push({ label: 'Duplicate', action: async () => {
      const result = await window.fjord.duplicateFile(filePath)
      if (result) _callbacks.refreshTree?.()
    }})
  }

  items.push({ label: 'Move to Trash', action: async () => {
    const ok = await window.fjord.trashFile(filePath)
    if (ok) _callbacks.refreshTree?.()
  }})

  items.push({ separator: true })
  items.push({ label: 'Copy Path', action: () => navigator.clipboard.writeText(filePath) })
  items.push({ label: 'Reveal in Finder', action: () => window.fjord.showInFolder(filePath) })

  showContextMenu(x, y, items)
}

// ── Build app shell ───────────────────────────────────────────────
export function buildShell() {
  document.getElementById('root').innerHTML = `
    <div class="app" id="app">
      <!-- Native macOS overlay chrome lives above every app column. -->
      <div class="brandrail" id="brandrail" data-tauri-drag-region="deep">
        <div class="brandrail__workspace" aria-label="Current workspace">
          <span class="brandrail__workspace-name" id="brandrail-workspace-name">No workspace</span>
          <span class="brandrail__workspace-path" id="brandrail-workspace-path">Open a folder or create a new window</span>
        </div>
      </div>

      <!-- Layout -->
      <div class="layout">

        <!-- Sidebar -->
        <div class="sidebar" id="sidebar">
          <div class="left-widget-stack widget-stack widget-stack--left" id="left-widget-stack"></div>
        </div>
        <div class="sidebar-resizer" id="sidebar-resizer" title="Resize explorer"></div>

        <!-- Editor area -->
        <div class="editor-area">
          <!-- Welcome / editor wrapper -->
          <div id="editor-wrapper" style="flex:1;display:flex;flex-direction:column;overflow:hidden">
            ${buildWelcome()}
          </div>

          ${buildTerminalDrawer()}

          <!-- Statusbar -->
          <div class="statusbar">
            <div class="statusbar__metrics">
              <div class="st st--mode"><div class="st-dot"></div><span id="st-mode">Markdown</span></div>
              <div class="st st--filename" id="st-filename" title="Current file">—</div>
              <div class="st" id="st-words">—</div>
              <div class="st st--optional" id="st-readtime">—</div>
              <span class="st st--update" id="st-update" hidden></span>
            </div>
            <div class="statusbar__controls">
              <div class="app-controls" id="app-controls" aria-label="Global controls">
                <!-- Panels -->
                <div class="theme-btn" id="sidebar-toggle" title="Toggle file explorer (⌘B)" aria-label="Toggle file explorer" role="button" tabindex="0">
                  ${sidebarIcon()}
                  <span class="control-label">Left sidebar</span>
                </div>
                <div class="theme-btn" id="terminal-toggle" title="Toggle terminal (⌘J)" aria-label="Toggle terminal" role="button" tabindex="0">
                  ${terminalIcon()}
                  <span class="control-label">Terminal</span>
                </div>
                <div class="theme-btn" id="right-sidebar-toggle" title="Toggle right widgets panel" aria-label="Toggle right widgets panel" role="button" tabindex="0">
                  ${rightSidebarIcon()}
                  <span class="control-label">Right sidebar</span>
                </div>
                <div class="app-controls__sep"></div>
                <!-- Layout -->
                <div class="theme-btn" id="workspace-split-toggle" title="Toggle workspace split layout" aria-label="Toggle workspace split layout" role="button" tabindex="0">
                  ${workspaceSplitIcon()}
                  <span class="control-label">Split view</span>
                </div>
                <div class="app-controls__sep"></div>
                <!-- App -->
                <div class="theme-btn theme-btn--settings" id="settings-btn" title="Settings (⌘,)" aria-label="Open settings" role="button" tabindex="0">
                  ${gearIcon()}
                  <span class="control-label">Settings</span>
                </div>
              </div>
              <div class="st st-brand">Rísta</div>
            </div>
          </div>
        </div>

        ${buildAssistantRail()}
        ${buildRightPanelContainer()}
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
          ${renderSettingsTabs()}
          <div class="settings-panel__content">

          <div class="settings-page active" data-settings-section="appearance">
            <section class="settings-group">
              <div class="settings-section-title">Theme presets</div>
              ${renderPresetPicker('darkThemePreset', 'Dark preset', THEME_PRESETS.dark)}
              ${renderPresetPicker('lightThemePreset', 'Light preset', THEME_PRESETS.light)}
              ${renderAppIconPicker()}
            </section>

            <section class="settings-group settings-group--compact">
              <div class="settings-section-title">Atmosphere</div>
              ${renderToggleSetting('ambientBackground', 'Ambient background', 'Keep the aurora field visible while editing')}
            </section>

            <details class="settings-advanced">
              <summary>Advanced appearance controls</summary>
              <div class="settings-advanced__body">
                ${renderRangeSetting('ambientIntensity', 'Background strength', 0, 100, 1, '%')}
                ${renderRangeSetting('surfaceOpacity', 'Surface opacity', 45, 100, 1, '%')}
                ${renderRangeSetting('surfaceBlur', 'Surface blur', 0, 32, 1, 'px')}
                ${renderRangeSetting('contrastBoost', 'Contrast boost', 0, 40, 1, '%')}
                ${renderTextSetting('textColor', 'Primary text color', 'Optional hex color, e.g. #f2f5ff')}
                ${renderTextSetting('mutedTextColor', 'Secondary text color', 'Optional hex color, e.g. #a7b0c0')}
                ${renderTextSetting('subtleTextColor', 'Subtle text color', 'Optional hex color, e.g. #6d7483')}
                ${renderTextSetting('accentColor', 'Accent color', 'Optional hex color, e.g. #7ba3cc')}
              </div>
            </details>
          </div>

          <div class="settings-page" data-settings-section="typography">
            <section class="settings-group">
              <div class="settings-section-title">Interface type</div>
              ${renderFontPicker('uiFont', 'Interface font', FONT_OPTIONS.ui)}
              ${renderTextSetting('uiFontCustom', 'Installed app font or stack', "Example: 'Atkinson Hyperlegible', system-ui, sans-serif")}
              ${renderRangeSetting('uiFontSize', 'App size', 11, 16, 1, 'px')}
              ${renderFontPicker('explorerFont', 'Explorer font', FONT_OPTIONS.explorer)}
              ${renderTextSetting('explorerFontCustom', 'Installed explorer font or stack', "Example: 'Aptos', system-ui, sans-serif")}
              ${renderRangeSetting('explorerFontSize', 'Explorer size', 11, 16, 1, 'px')}
            </section>

            <section class="settings-group">
              <div class="settings-section-title">Markdown editor</div>
              ${renderFontPicker('editorFont', 'Editor font', FONT_OPTIONS.editor, 'const note = "# Markdown"')}
              ${renderTextSetting('editorFontCustom', 'Installed editor font or stack', "Example: 'Berkeley Mono', 'SF Mono', monospace")}
              ${renderRangeSetting('editorFontSize', 'Editor size', 12, 18, 1, 'px')}
              ${renderRangeSetting('editorLineHeight', 'Editor spacing', 1.4, 2.1, 0.05, '')}
              ${renderTextSetting('editorTextColor', 'Editor text color', 'Optional hex color, e.g. #e7ecf7')}
            </section>

            <section class="settings-group">
              <div class="settings-section-title">Preview typography</div>
              ${renderToggleSetting('previewMirrorEditor', 'Mirror editor font', 'Use the same font, size, and spacing as the editor in Preview mode')}
              ${renderFontPicker('previewFont', 'Preview font', FONT_OPTIONS.preview, 'Heading and body text')}
              ${renderTextSetting('previewFontCustom', 'Installed preview font or stack', "Example: 'Iowan Old Style', Georgia, serif")}
              ${renderRangeSetting('previewFontSize', 'Preview size', 12, 18, 1, 'px')}
              ${renderRangeSetting('previewLineHeight', 'Preview spacing', 1.4, 2.1, 0.05, '')}
              ${renderTextSetting('previewTextColor', 'Preview text color', 'Optional hex color, e.g. #f1f4fa')}
            </section>
          </div>

          <div class="settings-page" data-settings-section="editor">
            <section class="settings-group">
              <div class="settings-section-title">Markdown editing</div>
              ${renderToggleSetting('typewriterScrolling', 'Typewriter scrolling', 'Keep cursor vertically centered while typing')}
              ${renderToggleSetting('livePreview', 'Live preview', 'Hide Markdown syntax marks on lines not being edited (heading marks, bold/italic, links)')}
              ${renderToggleSetting('posHighlight', 'Writing analysis', 'Subtly highlight adverbs, passive voice, and long sentences in the editor')}
              ${renderToggleSetting('focusMode', 'Focus mode', 'Dim lines outside the current paragraph while writing')}
              ${renderToggleSetting('smartTypography', 'Smart typography', 'Auto-convert dashes, ellipsis, and straight quotes to typographic characters')}
              ${renderToggleSetting('spellcheck', 'Spellcheck', 'Enable browser spellcheck in the editor')}
              ${renderToggleSetting('vimMode', 'Vim mode', 'Enable Vim keybindings in the editor')}
              ${renderToggleSetting('softWrap', 'Soft wrap', 'Wrap long lines instead of horizontal scrolling')}
              ${renderToggleSetting('showLineNumbers', 'Show line numbers', 'Display line numbers in the editor gutter')}
              ${renderRangeSetting('autoSaveDelay', 'Auto-save delay', 200, 5000, 100, 'ms')}
              ${renderSelectSetting('tabIndentation', 'Indentation style', [
                { value: 'spaces', label: 'Spaces' },
                { value: 'tabs', label: 'Tabs' },
              ])}
              ${renderSelectSetting('indentWidth', 'Indent width', [
                { value: '2', label: '2' },
                { value: '4', label: '4' },
                { value: '8', label: '8' },
              ])}
            </section>

            <section class="settings-group">
              <div class="settings-section-title">Rendered documents</div>
              ${renderToggleSetting('hideFrontmatterInRenderedModes', 'Hide document properties', 'Hide YAML properties in Preview and Rich Text while keeping them in the markdown file')}
              ${renderToggleSetting('showDocumentBanners', 'Show document banners', 'Render banner images from document properties in Preview and exports')}
            </section>

            <section class="settings-group">
              <div class="settings-section-title">Experimental</div>
              ${renderToggleSetting('docxExportEnabled', 'DOCX export', 'Enable Export to DOCX command in the command palette (experimental feature)')}
            </section>
          </div>

          <div class="settings-page" data-settings-section="workspace">
            <section class="settings-group">
              <div class="settings-section-title">Pinned workspaces</div>
              <div class="settings-group__hint">Pinned workspaces are shown on the welcome screen for quick access.</div>
              <div id="pinned-workspaces-list">
                ${renderPinnedProjectsHtml({ empty: true })}
              </div>
            </section>

            <section class="settings-group">
              <div class="settings-section-title">Behavior</div>
              ${renderToggleSetting('showStatusBar', 'Show status bar', 'Display the bottom status bar')}
              ${renderSelectSetting('defaultViewMode', 'Default view mode', [
                { value: 'markdown', label: 'Markdown' },
                { value: 'split', label: 'Split' },
                { value: 'preview', label: 'Preview' },
              ])}
              ${renderRangeSetting('readingSpeed', 'Reading speed', 100, 500, 10, 'wpm')}
              ${renderToggleSetting('zenParagraphDimming', 'Zen paragraph dimming', 'Dim paragraphs except the one with the cursor')}
              ${renderRangeSetting('zenColumnWidth', 'Zen column width', 500, 900, 10, 'px')}
              ${renderToggleSetting('showMinimap', 'Show minimap', 'Display a document overview on the right edge')}
            </section>

            <section class="settings-group">
              <div class="settings-section-title">Daily notes</div>
              <div class="settings-group__hint">Used by the Daily Note shortcut (⇧⌘D) and the calendar widget.</div>
              ${renderTextSetting('dailyNotesFolder', 'Folder', 'Subfolder within your project, e.g. daily')}
              ${renderTextSetting('dailyNoteTemplate', 'Template', 'Use {{date}} for the date placeholder')}
            </section>
          </div>

          <div class="settings-page" data-settings-section="ai-tools">
            <section class="settings-group">
              <div class="settings-section-title">AI provider</div>
              ${renderSelectSetting('aiProvider', 'Provider', renderProviderOptions())}
              ${renderSelectSetting('assistantDock', 'Assistant placement', ASSISTANT_DOCK_OPTIONS)}
              ${renderLocalToolDiscovery()}
            </section>

            <section class="settings-group">
              <div class="settings-section-title">Connection</div>
              <label class="settings-field">
                <span class="settings-field__label">API key</span>
                <input
                  class="settings-input"
                  type="password"
                  placeholder="sk-..."
                  value="${escapeAttribute(settingsValue('aiApiKey') || '')}"
                  data-setting="aiApiKey"
                >
              </label>
              ${renderTextSetting('aiModel', 'Model', 'Leave empty for provider default')}
              ${renderTextSetting('aiBaseUrl', 'Base URL', 'Leave empty for provider default')}
            </section>
          </div>

          <div class="settings-page" data-settings-section="shortcuts">
            <section class="settings-group">
              <div class="settings-section-title">Keyboard shortcuts</div>
              <div class="settings-group__hint">Click a shortcut, then press the new key combo. Press Escape to cancel capture.</div>
              <div class="keybinding-list" id="keybinding-list"></div>
            </section>
          </div>
          </div>
        </div>

        <div class="settings-panel__footer">
          <div class="settings-panel__meta" id="settings-app-meta">Rísta</div>
          <button class="settings-btn settings-btn--muted" id="settings-export-btn">Export</button>
          <button class="settings-btn settings-btn--muted" id="settings-import-btn">Import</button>
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
  $('brandrail')?.addEventListener('mousedown', handleWindowDragRegionMouseDown)
  $('app-controls')?.addEventListener('pointerdown', handleGlobalControlPointerDown, true)
  $('app-controls')?.addEventListener('keydown', handleGlobalControlKeydown)
  $('settings-close-btn').addEventListener('click', closeSettingsPanel)
  $('settings-done-btn').addEventListener('click', closeSettingsPanel)
  $('settings-reset-btn').addEventListener('click', () => {
    const next = resetSettings()
    syncSettingsForm()
    applySelectedAppIcon(next)
    refreshRenderedDocuments()
    _callbacks.syncAssistantRail?.()
  })
  $('settings-export-btn')?.addEventListener('click', async () => {
    await window.fjord.exportSettings(JSON.stringify(getSettings()))
  })
  $('settings-import-btn')?.addEventListener('click', async () => {
    const raw = await window.fjord.importSettings()
    if (!raw) return
    try {
      const parsed = JSON.parse(raw)
      const next = setSettings(parsed)
      syncSettingsForm()
      applySelectedAppIcon(next)
      refreshRenderedDocuments()
      _callbacks.syncAssistantRail?.()
    } catch {
      alert('Invalid settings file')
    }
  })
  $('settings-overlay').addEventListener('click', closeSettingsPanel)
  $('settings-panel').addEventListener('input', handleSettingsInput)
  syncWorkspaceChrome()
  $('command-dialog-close').addEventListener('click', closeCommandDialog)
  $('command-dialog-cancel').addEventListener('click', closeCommandDialog)
  $('command-dialog-overlay').addEventListener('click', closeCommandDialog)
  $('command-dialog-form').addEventListener('submit', submitCommandDialog)
  $('settings-panel').addEventListener('click', handleSettingsClick)
  $('settings-panel').addEventListener('keydown', handleSettingsKeydown)

  _callbacks.syncToolbarToggle?.()

  // File explorer's Open / Collapse-all / file-tree click handlers are wired
  // by file-explorer-view.js on widget mount, since they live inside the
  // Files widget body now and can move between sidebars.
  $('sidebar-resizer')?.addEventListener('pointerdown', e => _callbacks.startSidebarResize?.(e))
  $('welcome-open-btn')?.addEventListener('click', () => _callbacks.openFolder?.())
  $('welcome-new-file-btn')?.addEventListener('click', () => _callbacks.createNewFile?.())

  // Recent projects click handlers (delegation from welcome)
  attachWelcomeProjectHandlers($('welcome'), {
    openProject: (folderPath) => _callbacks.openRecentProject?.(folderPath),
    openProjectNewWindow: (folderPath) => _callbacks.openWorkspaceInNewWindow?.(folderPath),
    refreshWelcome: refreshShellWelcome,
  })

  // Watch for file changes from main process
  if (window.fjord) {
    window.fjord.appMeta?.().then(meta => {
      if (!meta) return
      state.appMeta = meta
      syncAppMeta()
    }).catch(() => {})

    window.fjord.onCommand?.(data => _callbacks.handleAppCommand?.(data.command, data))

    window.fjord.onFileChange(({ event, path: p }) => {
      _callbacks.handleExternalFileChange?.({ event, path: p })
    })
  }
}
