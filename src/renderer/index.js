import { initTheme, toggleTheme, getTheme } from './theme.js'
import { createEditor, updateEditorDoc, updateEditorTheme } from './editor.js'
import { renderMarkdown, extractHeadings, getStats } from './markdown.js'
import { applySettings, getSettings, updateSetting, resetSettings, FONT_OPTIONS } from './settings.js'
import { undo, redo } from '@codemirror/commands'
import ToastEditor from '@toast-ui/editor'

// ── Init theme before any paint ──────────────────────────────────
initTheme()
applySettings()

// ── App state ────────────────────────────────────────────────────
const state = {
  folderPath: null,
  tree: [],
  expandedFolders: new Set(),
  appMeta: { name: 'Fjordmark', version: '' },
  tabs: [],          // [{ path, name, content, dirty }]
  tabGroups: {
    primary: [],
    secondary: [],
  },
  splitSnapshot: {
    primary: [],
    secondary: [],
    activePrimary: null,
    activeSecondary: null,
    focusedPane: 'primary',
  },
  activeTab: null,
  secondaryTab: null,
  focusedPane: 'primary',
  workspaceMode: 'single', // 'single' | 'dual'
  paneView: {
    primary: 'split',
    secondary: 'split',
  },     // 'markdown' | 'split' | 'wysiwyg' | 'preview'
  splitEditableMode: {
    primary: 'markdown',
    secondary: 'markdown',
  },
  splitPreviewSide: {
    primary: 'right',
    secondary: 'right',
  },
  toolbarVisible: true,
  sidebarVisible: true,
  insightsOpen: false,
  insightsSections: {
    stats: true,
    headings: true,
  },
  settingsOpen: false,
  commandDialog: null,
}

const PANE_KEYS = ['primary', 'secondary']
const editorViews = {
  primary: null,
  secondary: null,
}
const richEditors = {
  primary: null,
  secondary: null,
}
const richEditorMountTarget = {
  primary: null,
  secondary: null,
}
const saveTimers = {
  primary: null,
  secondary: null,
}
const syncingRichEditor = {
  primary: false,
  secondary: false,
}
let draggedTab = null

function getPaneView(pane = state.focusedPane) {
  return state.paneView[pane] || 'split'
}

function getSplitView(pane = state.focusedPane) {
  return makeSplitView(getSplitEditableView(pane), getSplitPreviewSide(pane))
}

function paneUsesWysiwyg(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'wysiwyg') return true
  if (paneView !== 'split') return false
  return getSplitEditableView(pane) === 'wysiwyg'
}

function paneUsesMarkdown(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'markdown') return true
  if (paneView !== 'split') return false
  return getSplitEditableView(pane) === 'markdown'
}

function getWysiwygMountSlot(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'wysiwyg') return 'single'
  if (paneView !== 'split') return null
  if (getSplitEditableView(pane) !== 'wysiwyg') return null
  return getSplitPreviewSide(pane) === 'left' ? 'right' : 'left'
}

function getSplitEditableView(pane = state.focusedPane) {
  return state.splitEditableMode[pane] === 'wysiwyg' ? 'wysiwyg' : 'markdown'
}

function getSplitPreviewSide(pane = state.focusedPane) {
  return state.splitPreviewSide[pane] === 'left' ? 'left' : 'right'
}

function makeSplitView(editableView = 'markdown', previewSlot = 'right') {
  const nextView = editableView === 'wysiwyg' ? 'wysiwyg' : 'markdown'
  return previewSlot === 'left'
    ? { left: 'preview', right: nextView }
    : { left: nextView, right: 'preview' }
}

function getTabForPane(pane = state.focusedPane) {
  return pane === 'secondary' ? state.secondaryTab : state.activeTab
}

function setTabForPane(pane, tab) {
  if (pane === 'secondary') state.secondaryTab = tab
  else state.activeTab = tab
}

function getFocusedTab() {
  return getTabForPane(state.focusedPane)
}

function getFocusedEditor() {
  return editorViews[state.focusedPane] || null
}

function getGroupTabs(pane) {
  return state.tabGroups[pane]
}

function hasTabInPane(tab, pane) {
  return getGroupTabs(pane).includes(tab)
}

function addTabToPane(tab, pane) {
  const group = getGroupTabs(pane)
  if (!group.includes(tab)) group.push(tab)
}

function removeTabFromPane(tab, pane) {
  const group = getGroupTabs(pane)
  const index = group.indexOf(tab)
  if (index >= 0) group.splice(index, 1)
}

function getTabPane(tab, pane = state.focusedPane) {
  if (!tab) return null
  if (hasTabInPane(tab, pane)) return pane
  if (hasTabInPane(tab, 'primary')) return 'primary'
  if (hasTabInPane(tab, 'secondary')) return 'secondary'
  return null
}

function isTabOpenAnywhere(tab) {
  return PANE_KEYS.some(pane => hasTabInPane(tab, pane))
}

function cleanSplitSnapshot() {
  const validTabs = new Set(state.tabs)
  state.splitSnapshot.primary = state.splitSnapshot.primary.filter(tab => validTabs.has(tab))
  state.splitSnapshot.secondary = state.splitSnapshot.secondary.filter(tab => validTabs.has(tab))
  if (state.splitSnapshot.activePrimary && !validTabs.has(state.splitSnapshot.activePrimary)) state.splitSnapshot.activePrimary = null
  if (state.splitSnapshot.activeSecondary && !validTabs.has(state.splitSnapshot.activeSecondary)) state.splitSnapshot.activeSecondary = null
}

function storeSplitSnapshot() {
  state.splitSnapshot = {
    primary: [...state.tabGroups.primary],
    secondary: [...state.tabGroups.secondary],
    activePrimary: state.activeTab,
    activeSecondary: state.secondaryTab,
    focusedPane: state.focusedPane,
  }
  cleanSplitSnapshot()
}

function syncWorkspaceSplitToggle() {
  $('workspace-split-toggle')?.classList.toggle('active', state.workspaceMode === 'dual')
  document.querySelectorAll('.workspace-toolbar [data-action="toggle-workspace-split"]').forEach(node => {
    node.classList.toggle('active', state.workspaceMode === 'dual')
  })
}

function syncToolbarToggle() {
  const node = $('toolbar-toggle')
  if (!node) return
  node.classList.toggle('active', state.toolbarVisible)
  node.title = state.toolbarVisible ? 'Hide toolbars' : 'Show toolbars'
}

// ── DOM refs (built below) ────────────────────────────────────────
const $ = id => document.getElementById(id)
const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html) e.innerHTML = html; return e }
const settingsValue = key => getSettings()[key]

// ── Build app shell ───────────────────────────────────────────────
function buildShell() {
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
  $('toolbar-toggle').addEventListener('click', toggleToolbar)
  $('workspace-split-toggle').addEventListener('click', toggleWorkspaceSplit)
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
    $('theme-btn').innerHTML = t === 'dark' ? sunIcon() : moonIcon()
    PANE_KEYS.forEach(pane => {
      if (editorViews[pane]) updateEditorTheme(editorViews[pane], t === 'dark')
      if (richEditors[pane]) {
        const markdown = richEditors[pane].getMarkdown()
        destroyRichEditor(pane)
        ensureRichEditorMounted(pane)
        if (richEditors[pane]) {
          syncingRichEditor[pane] = true
          richEditors[pane].setMarkdown(markdown)
          syncingRichEditor[pane] = false
        }
      }
    })
  })
  $('theme-btn').innerHTML = getTheme() === 'dark' ? sunIcon() : moonIcon()
  syncToolbarToggle()

  $('sidebar-toggle').addEventListener('click', toggleSidebar)
  $('open-folder-btn').addEventListener('click', openFolder)
  $('collapse-all-btn')?.addEventListener('click', collapseAllFolders)
  $('sidebar-resizer')?.addEventListener('pointerdown', startSidebarResize)
  $('welcome-open-btn')?.addEventListener('click', openFolder)

  // Watch for file changes from main process
  if (window.fjord) {
    window.fjord.appMeta?.().then(meta => {
      if (!meta) return
      state.appMeta = meta
      syncAppMeta()
    }).catch(() => {})

    window.fjord.onCommand?.(({ command }) => handleAppCommand(command))

    window.fjord.onFileChange(({ event, path: p }) => {
      const tab = state.tabs.find(t => t.path === p)
      if (tab && !tab.dirty) loadFileIntoTab(tab)
      refreshTree()
    })
  }
}

function syncAppMeta() {
  const meta = $('settings-app-meta')
  if (!meta) return
  meta.textContent = state.appMeta.version
    ? `${state.appMeta.name} v${state.appMeta.version}`
    : state.appMeta.name
}

function buildWelcome() {
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

function renderSelectSetting(key, label, options) {
  return `
    <label class="settings-field">
      <span class="settings-field__label">${label}</span>
      <select class="settings-select" data-setting="${key}">
        ${options.map(option => `<option value="${option.value}"${option.value === settingsValue(key) ? ' selected' : ''}>${option.label}</option>`).join('')}
      </select>
    </label>
  `
}

function renderRangeSetting(key, label, min, max, step, unit) {
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

function renderTextSetting(key, label, placeholder) {
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

function renderToggleSetting(key, label, description) {
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

function toggleSettingsPanel() {
  state.settingsOpen ? closeSettingsPanel() : openSettingsPanel()
}

function openSettingsPanel() {
  state.settingsOpen = true
  $('app')?.classList.add('settings-open')
  syncSettingsForm()
}

function closeSettingsPanel() {
  state.settingsOpen = false
  $('app')?.classList.remove('settings-open')
}

function openCommandDialog(type, options = {}) {
  state.commandDialog = { type, pane: state.focusedPane, ...options }
  const app = $('app')
  const body = $('command-dialog-body')
  const title = $('command-dialog-title')
  const eyebrow = $('command-dialog-eyebrow')
  const submit = $('command-dialog-submit')
  if (!app || !body || !title || !eyebrow || !submit) return

  if (type === 'link') {
    eyebrow.textContent = 'Insert'
    title.textContent = 'Link'
    submit.textContent = 'Insert link'
    body.innerHTML = `
      <label class="command-field">
        <span class="command-field__label">URL</span>
        <input class="command-field__input" id="command-link-url" name="url" type="text" placeholder="https://example.com" value="${options.url || ''}" />
      </label>
      <label class="command-field">
        <span class="command-field__label">Text</span>
        <input class="command-field__input" id="command-link-text" name="text" type="text" placeholder="Link text" value="${options.text || ''}" />
      </label>
    `
  } else if (type === 'image') {
    eyebrow.textContent = 'Insert'
    title.textContent = 'Image'
    submit.textContent = 'Insert image'
    body.innerHTML = `
      <label class="command-field">
        <span class="command-field__label">Image URL</span>
        <input class="command-field__input" id="command-image-url" name="url" type="text" placeholder="https://example.com/image.png" value="${options.url || ''}" />
      </label>
      <label class="command-field">
        <span class="command-field__label">Alt text</span>
        <input class="command-field__input" id="command-image-alt" name="alt" type="text" placeholder="Describe the image" value="${options.alt || ''}" />
      </label>
    `
  } else if (type === 'table') {
    eyebrow.textContent = 'Insert'
    title.textContent = 'Table'
    submit.textContent = 'Insert table'
    body.innerHTML = `
      <div class="command-grid">
        <label class="command-field">
          <span class="command-field__label">Columns</span>
          <input class="command-field__input" id="command-table-columns" name="columns" type="number" min="2" max="8" step="1" value="${options.columns || 3}" />
        </label>
        <label class="command-field">
          <span class="command-field__label">Rows</span>
          <input class="command-field__input" id="command-table-rows" name="rows" type="number" min="2" max="20" step="1" value="${options.rows || 2}" />
        </label>
      </div>
    `
  }

  app.classList.add('command-dialog-open')
  requestAnimationFrame(() => body.querySelector('input')?.focus())
}

function closeCommandDialog() {
  state.commandDialog = null
  $('app')?.classList.remove('command-dialog-open')
}

function renderSplitSlotSelector(pane, slot) {
  const splitView = getSplitView(pane)
  const currentView = splitView[slot] || 'preview'
  const currentLabel = currentView === 'markdown' ? 'MD' : currentView === 'wysiwyg' ? 'WYSIWYG' : 'Preview'
  return `
    <div class="split-slot-control" data-pane="${pane}">
      <div class="split-slot-control__label">${slot === 'left' ? 'left' : 'right'}</div>
      <div class="dd split-slot-dd" id="dd-sv-${pane}-${slot}">
        <div class="split-slot-trigger" data-action="toggle-dropdown" data-dropdown="dd-sv-${pane}-${slot}">
          <span>${currentLabel}</span>
          <svg class="arr" viewBox="0 0 8 6"><path d="M1 1.5l3 3 3-3"/></svg>
        </div>
        <div class="dd-menu dd-menu--compact" id="ddm-sv-${pane}-${slot}">
          <div class="dd-item${currentView === 'markdown' ? ' active' : ''}" data-action="set-split-view" data-slot="${slot}" data-slot-view="markdown">Markdown</div>
          <div class="dd-item${currentView === 'wysiwyg' ? ' active' : ''}" data-action="set-split-view" data-slot="${slot}" data-slot-view="wysiwyg">WYSIWYG</div>
          <div class="dd-item${currentView === 'preview' ? ' active' : ''}" data-action="set-split-view" data-slot="${slot}" data-slot-view="preview">Preview</div>
        </div>
      </div>
    </div>
  `
}

function renderEditorToolbar(pane) {
  const paneView = getPaneView(pane)
  return `
    <div class="workspace-toolbar${state.toolbarVisible ? '' : ' hidden'}" data-pane="${pane}">
      <div class="ic" title="Undo" data-action="editor-cmd" data-cmd="undo">
        <svg viewBox="0 0 16 16"><path d="M3 7h6a4 4 0 1 1 0 8H5"/><path d="M3 4L1 7l2 3"/></svg>
      </div>
      <div class="ic" title="Redo" data-action="editor-cmd" data-cmd="redo">
        <svg viewBox="0 0 16 16"><path d="M13 7H7a4 4 0 1 0 0 8h4"/><path d="M13 4l2 3-2 3"/></svg>
      </div>
      <div class="tb-sep"></div>

      <div class="dd" id="dd-h-${pane}">
        <div class="ic hd" data-action="toggle-dropdown" data-dropdown="dd-h-${pane}">
          <span class="t">H</span>
          <svg class="arr" viewBox="0 0 8 6"><path d="M1 1.5l3 3 3-3"/></svg>
        </div>
        <div class="dd-menu" id="ddm-h-${pane}">
          <div class="dd-item" data-action="insert-heading" data-level="1">Heading 1</div>
          <div class="dd-item" data-action="insert-heading" data-level="2">Heading 2</div>
          <div class="dd-item" data-action="insert-heading" data-level="3">Heading 3</div>
          <div class="dd-item divider" data-action="insert-heading" data-level="0">Paragraph</div>
        </div>
      </div>

      <div class="dd" id="dd-l-${pane}">
        <div class="ic" data-action="toggle-dropdown" data-dropdown="dd-l-${pane}">
          <svg viewBox="0 0 16 16"><circle cx="3" cy="5" r="1.1" fill="currentColor" stroke="none"/><circle cx="3" cy="8.5" r="1.1" fill="currentColor" stroke="none"/><circle cx="3" cy="12" r="1.1" fill="currentColor" stroke="none"/><line x1="6.5" y1="5" x2="14" y2="5"/><line x1="6.5" y1="8.5" x2="14" y2="8.5"/><line x1="6.5" y1="12" x2="11" y2="12"/></svg>
        </div>
        <div class="dd-menu" id="ddm-l-${pane}">
          <div class="dd-item" data-action="insert-list" data-list-type="bullet">Bullet list</div>
          <div class="dd-item" data-action="insert-list" data-list-type="ordered">Numbered list</div>
          <div class="dd-item" data-action="insert-list" data-list-type="task">Task list</div>
        </div>
      </div>

      <div class="ic" title="Blockquote" data-action="wrap-selection" data-prefix="> ">
        <svg viewBox="0 0 16 16"><path d="M3 5h10M3 8h7M3 11h5"/></svg>
      </div>
      <div class="dd" id="dd-c-${pane}">
        <div class="ic" title="Callout" data-action="toggle-dropdown" data-dropdown="dd-c-${pane}">
          <svg viewBox="0 0 16 16"><path d="M3 3.5h10v7H7l-3.5 2.5V3.5Z"/><line x1="5.5" y1="6" x2="10.5" y2="6"/><line x1="5.5" y1="8.5" x2="9" y2="8.5"/></svg>
        </div>
        <div class="dd-menu" id="ddm-c-${pane}">
          <div class="dd-item" data-action="insert-callout" data-callout-type="note">Note</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="info">Info</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="tip">Tip</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="warning">Warning</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="danger">Danger</div>
        </div>
      </div>
      <div class="tb-sep"></div>

      <div class="ic" title="Bold" data-action="wrap-inline" data-before="**" data-after="**"><span class="t">B</span></div>
      <div class="ic" title="Italic" data-action="wrap-inline" data-before="*" data-after="*"><span class="t t-i">I</span></div>
      <div class="ic" title="Strikethrough" data-action="wrap-inline" data-before="~~" data-after="~~">
        <svg viewBox="0 0 16 16"><line x1="3" y1="8" x2="13" y2="8"/><path d="M5.5 5.5c0-1.1 1-2 2.5-2s2.5.9 2.5 2M5.5 10.5c0 1.1 1 2 2.5 2s2.5-.9 2.5-2"/></svg>
      </div>
      <div class="ic" title="Inline code" data-action="wrap-inline" data-before="\`" data-after="\`">
        <svg viewBox="0 0 16 16"><path d="M5.5 5L2 8l3.5 3M10.5 5L14 8l-3.5 3"/></svg>
      </div>
      <div class="ic" title="Link" data-action="insert-link">
        <svg viewBox="0 0 16 16"><path d="M6.5 9.5a3.5 3.5 0 0 0 5 0l2-2a3.5 3.5 0 0 0-5-5l-1 1"/><path d="M9.5 6.5a3.5 3.5 0 0 0-5 0l-2 2a3.5 3.5 0 0 0 5 5l1-1"/></svg>
      </div>
      <div class="tb-sep"></div>

      <div class="ic" title="Table" data-action="insert-table">
        <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1"/><line x1="2" y1="7" x2="14" y2="7"/><line x1="7" y1="3" x2="7" y2="13"/></svg>
      </div>
      <div class="ic" title="Image" data-action="insert-image">
        <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M2 10l3.5-3.5 2.5 2.5 2-2 4 4"/><circle cx="11.5" cy="5.5" r="1" fill="currentColor" stroke="none"/></svg>
      </div>
      <div class="ic" title="Find & Replace" data-action="toggle-find-replace">
        <svg viewBox="0 0 16 16"><circle cx="6" cy="6" r="3.5" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M9.5 9.5l3 3" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/></svg>
      </div>

      <div class="workspace-toolbar__right">
        <div class="ic${state.workspaceMode === 'dual' ? ' active' : ''}" data-action="toggle-workspace-split" title="Toggle split workspace">
          <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1.5"/><line x1="8" y1="3" x2="8" y2="13"/></svg>
        </div>
        <div class="vseg">
          <div class="vb${paneView === 'markdown' ? ' active' : ''}" data-action="set-view" data-view="markdown">MD</div>
          <div class="vb${paneView === 'split' ? ' active' : ''}" data-action="set-view" data-view="split">Split</div>
          <div class="vb${paneView === 'wysiwyg' ? ' active' : ''}" data-action="set-view" data-view="wysiwyg">WYSIWYG</div>
          <div class="vb${paneView === 'preview' ? ' active' : ''}" data-action="set-view" data-view="preview">Preview</div>
        </div>
        <div class="ic${state.insightsOpen ? ' active' : ''}" data-action="toggle-insights" title="Document insights">
          <svg viewBox="0 0 16 16"><rect x="2" y="9" width="3" height="5" rx="0.5" fill="currentColor" stroke="none"/><rect x="6.5" y="5" width="3" height="9" rx="0.5" fill="currentColor" stroke="none"/><rect x="11" y="2" width="3" height="12" rx="0.5" fill="currentColor" stroke="none"/><line x1="2" y1="4" x2="14" y2="4"/><line x1="5" y1="8" x2="14" y2="8"/></svg>
        </div>
      </div>
    </div>
    <div class="workspace-pane-row${state.toolbarVisible ? '' : ' hidden'}" data-pane="${pane}">
      <div class="pane-label" id="pl-source-${pane}">${paneView === 'split' ? renderSplitSlotSelector(pane, 'left') : 'source'}</div>
      <div class="pane-label" id="pl-preview-${pane}">${paneView === 'split' ? renderSplitSlotSelector(pane, 'right') : 'preview'}</div>
      <div class="hide-toolbar-btn" data-action="toggle-toolbar">${state.toolbarVisible ? 'hide toolbar' : 'show toolbar'}</div>
    </div>
  `
}

function buildEditorUI() {
  destroyEditors()

  $('editor-wrapper').innerHTML = `
    <!-- Find & Replace -->
    <div class="find-replace-panel hidden" id="find-replace">
      <input type="text" class="find-replace-input" id="find-input" placeholder="Find…">
      <input type="text" class="find-replace-input" id="replace-input" placeholder="Replace with…">
      <div class="find-replace-buttons">
        <button class="find-btn" id="find-next-btn">Next</button>
        <button class="find-btn" id="find-prev-btn">Prev</button>
        <button class="replace-btn" id="replace-one-btn">Replace</button>
        <button class="replace-all-btn" id="replace-all-btn">Replace All</button>
      </div>
      <div class="find-count" id="find-count"></div>
      <button class="find-btn" id="find-close-btn" style="margin-left:auto">Close</button>
    </div>
    <!-- Panes -->
    <div class="panes" id="panes">
      <div class="panes-workspace" id="panes-workspace">
        <section class="workspace-pane workspace-pane--primary" id="workspace-primary" data-pane="primary">
          <div class="workspace-pane__header">
            <div class="workspace-pane__label">Editor A</div>
            <div class="workspace-tabs" id="tabs-primary"></div>
          </div>
          ${renderEditorToolbar('primary')}
          <div class="panes-main" id="panes-main-primary">
            <div class="single-surface" id="single-surface-primary">
              <div class="preview-pane preview-pane--single" id="preview-single-primary"></div>
              <div class="cm-host" id="cm-host-primary"></div>
            </div>
            <div class="split-layout" id="split-layout-primary">
              <div class="pane" id="pane-left-primary">
                <div class="view-slot" id="view-slot-left-primary">
                  <div class="preview-pane preview-pane--slot" id="preview-left-primary"></div>
                </div>
              </div>
              <div class="split-resizer" id="split-resizer-primary" data-pane="primary" title="Resize split view"></div>
              <div class="pane" id="pane-right-primary">
                <div class="view-slot" id="view-slot-right-primary">
                  <div class="preview-pane preview-pane--slot" id="preview-right-primary"></div>
                </div>
              </div>
            </div>
          </div>
        </section>
        <div class="workspace-resizer hidden" id="workspace-resizer" title="Resize document split"></div>
        <section class="workspace-pane workspace-pane--secondary hidden" id="workspace-secondary" data-pane="secondary">
          <div class="workspace-pane__header">
            <div class="workspace-pane__label">Editor B</div>
            <div class="workspace-tabs" id="tabs-secondary"></div>
          </div>
          ${renderEditorToolbar('secondary')}
          <div class="panes-main" id="panes-main-secondary">
            <div class="single-surface" id="single-surface-secondary">
              <div class="preview-pane preview-pane--single" id="preview-single-secondary"></div>
              <div class="cm-host" id="cm-host-secondary"></div>
            </div>
            <div class="split-layout" id="split-layout-secondary">
              <div class="pane" id="pane-left-secondary">
                <div class="view-slot" id="view-slot-left-secondary">
                  <div class="preview-pane preview-pane--slot" id="preview-left-secondary"></div>
                </div>
              </div>
              <div class="split-resizer" id="split-resizer-secondary" data-pane="secondary" title="Resize split view"></div>
              <div class="pane" id="pane-right-secondary">
                <div class="view-slot" id="view-slot-right-secondary">
                  <div class="preview-pane preview-pane--slot" id="preview-right-secondary"></div>
                </div>
              </div>
            </div>
          </div>
          <div class="workspace-pane__empty" id="workspace-secondary-empty">
            Open another markdown file to compare side by side
          </div>
        </section>
      </div>
      <aside class="insights-panel" id="insights-panel">
        <div class="insights-panel__header">
          <div class="insights-panel__title">Document Insights</div>
          <div class="ic" id="insights-close-btn" data-action="toggle-insights" title="Close insights">
            <svg viewBox="0 0 16 16"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/></svg>
          </div>
        </div>
        <div class="insights-panel__body">
          <section class="insights-section${state.insightsSections.stats ? ' open' : ''}" id="insights-section-stats">
            <button class="insights-section__header" data-action="toggle-insights-section" data-section="stats">
              <span class="insights-section__title">Statistics</span>
              <span class="insights-section__chevron">${chevronIcon()}</span>
            </button>
            <div class="stats-grid" id="stats-content"></div>
          </section>
          <section class="insights-section${state.insightsSections.headings ? ' open' : ''}" id="insights-section-headings">
            <button class="insights-section__header" data-action="toggle-insights-section" data-section="headings">
              <span class="insights-section__title">Headings</span>
              <span class="insights-section__chevron">${chevronIcon()}</span>
            </button>
            <div id="toc-content"></div>
          </section>
        </div>
      </aside>
    </div>
  `

  mountEditor('primary')
  if (state.workspaceMode === 'dual') mountEditor('secondary')

  // Wire up welcome open button (may still be in DOM briefly)
  document.querySelectorAll('#welcome-open-btn').forEach(b => b.addEventListener('click', openFolder))
  wireEditorUiEvents()
  syncWorkspaceUi()
  syncSplitLayout()
  syncFocusedPaneUi()
  refreshAllPreviews()
  updateActiveMetrics()
}

function destroyEditors() {
  PANE_KEYS.forEach(pane => {
    clearTimeout(saveTimers[pane])
    saveTimers[pane] = null
    if (editorViews[pane]) {
      editorViews[pane].destroy()
      editorViews[pane] = null
    }
    destroyRichEditor(pane)
  })
}

function mountEditor(pane) {
  const host = $(`cm-host-${pane}`)
  if (!host || editorViews[pane]) return
  editorViews[pane] = createEditor({
    parent: host,
    doc: getTabForPane(pane)?.content || '',
    onChange: content => onEditorChange(pane, content),
    onSelectionChange: editorState => onEditorSelectionChange(pane, editorState),
    onPaste: file => handleImagePaste(file, editorViews[pane], pane),
    isDark: getTheme() === 'dark',
  })
}

function destroyRichEditor(pane) {
  if (richEditors[pane]) {
    richEditors[pane].destroy()
    richEditors[pane] = null
  }
  document.querySelectorAll(`#single-surface-${pane} .wysiwyg-editor, #view-slot-left-${pane} .wysiwyg-editor, #view-slot-right-${pane} .wysiwyg-editor`).forEach(node => {
    node.remove()
  })
  richEditorMountTarget[pane] = null
}

function ensureRichEditorMounted(pane) {
  const slot = getWysiwygMountSlot(pane)
  if (!slot) {
    destroyRichEditor(pane)
    return null
  }

  const slotHost = slot === 'single' ? $(`single-surface-${pane}`) : $(`view-slot-${slot}-${pane}`)
  if (!slotHost) return null

  const mountTarget = `${pane}:${slot}`
  if (richEditors[pane] && richEditorMountTarget[pane] === mountTarget) {
    return richEditors[pane]
  }

  const markdown = richEditors[pane]?.getMarkdown() ?? getTabForPane(pane)?.content ?? ''
  destroyRichEditor(pane)

  const host = document.createElement('div')
  host.className = 'wysiwyg-editor'
  host.id = `wysiwyg-editor-${pane}`
  slotHost.appendChild(host)

  richEditors[pane] = new ToastEditor({
    el: host,
    initialValue: markdown,
    initialEditType: 'wysiwyg',
    hideModeSwitch: true,
    previewStyle: 'tab',
    usageStatistics: false,
    toolbarItems: [],
    autofocus: false,
    theme: getTheme() === 'dark' ? 'dark' : undefined,
    events: {
      focus: () => focusPane(pane),
      change: () => onRichEditorChange(pane),
    },
  })
  richEditors[pane].setHeight('100%')
  richEditorMountTarget[pane] = mountTarget
  return richEditors[pane]
}

function ensureEditorForPane(pane) {
  if (!$(`workspace-primary`)) buildEditorUI()
  if (!editorViews[pane]) mountEditor(pane)
  if (paneUsesWysiwyg(pane)) ensureRichEditorMounted(pane)
}

function refreshAllPreviews() {
  PANE_KEYS.forEach(pane => {
    const tab = getTabForPane(pane)
    refreshPreview(pane, tab?.content || '')
    maybeRefreshWysiwygPane(pane)
  })
}

function maybeRefreshWysiwygPane(pane) {
  if (!paneUsesWysiwyg(pane)) return
  ensureRichEditorMounted(pane)
  syncToWysiwyg(pane)
}

function syncWorkspaceUi() {
  const dual = state.workspaceMode === 'dual'
  const secondaryPane = $('workspace-secondary')
  const secondaryEmpty = $('workspace-secondary-empty')
  const workspaceResizer = $('workspace-resizer')
  const primaryPane = $('workspace-primary')

  if (primaryPane) primaryPane.style.flexBasis = dual ? 'var(--document-split-ratio)' : '100%'
  if (secondaryPane) secondaryPane.classList.toggle('hidden', !dual)
  if (workspaceResizer) workspaceResizer.classList.toggle('hidden', !dual)
  syncWorkspaceSplitToggle()
  if (secondaryEmpty) secondaryEmpty.style.display = dual && !state.secondaryTab ? 'flex' : 'none'

  PANE_KEYS.forEach(pane => {
    const tab = getTabForPane(pane)
    const empty = $(`workspace-${pane}-empty`)
    const panesMain = $(`panes-main-${pane}`)
    if (pane === 'secondary' && !dual) return
    if (panesMain) panesMain.style.display = tab ? 'flex' : 'none'
    if (empty && pane === 'secondary') empty.style.display = tab ? 'none' : 'flex'
  })
}

function syncFocusedPaneUi() {
  const app = $('app')
  if (app) app.dataset.focusedPane = state.focusedPane
  PANE_KEYS.forEach(pane => {
    const workspace = $(`workspace-${pane}`)
    workspace?.classList.toggle('focused', state.focusedPane === pane)
  })
}

function syncSplitLayout() {
  const dual = state.workspaceMode === 'dual'
  PANE_KEYS.forEach(pane => {
    const active = pane === 'primary' || dual
    const tab = getTabForPane(pane)
    const paneView = getPaneView(pane)
    const splitView = getSplitView(pane)
    const singleSurface = $(`single-surface-${pane}`)
    const splitLayout = $(`split-layout-${pane}`)
    const leftPane = $(`pane-left-${pane}`)
    const rightPane = $(`pane-right-${pane}`)
    const splitResizer = $(`split-resizer-${pane}`)
    const sourceLabel = $(`pl-source-${pane}`)
    const previewLabel = $(`pl-preview-${pane}`)
    if (!active || !tab) return
    if (singleSurface) singleSurface.style.display = paneView === 'split' ? 'none' : 'flex'
    if (splitLayout) splitLayout.style.display = paneView === 'split' ? 'flex' : 'none'
    if (paneView === 'split') {
      if (leftPane) leftPane.classList.toggle('hidden', false)
      if (rightPane) rightPane.classList.toggle('hidden', false)
      if (leftPane) leftPane.style.display = 'flex'
      if (rightPane) rightPane.style.display = 'flex'
      if (splitResizer) splitResizer.style.display = ''
      if (leftPane) leftPane.style.flex = '0 0 var(--split-ratio)'
      if (rightPane) rightPane.style.flex = '1 1 calc(100% - var(--split-ratio))'
      placePaneView(pane, 'left', splitView.left)
      placePaneView(pane, 'right', splitView.right)
    } else {
      if (splitResizer) splitResizer.style.display = 'none'
      placePaneView(pane, 'left', null)
      placePaneView(pane, 'right', null)
      placeStandaloneView(pane, paneView)
    }
    if (sourceLabel) {
      sourceLabel.style.display = ''
      sourceLabel.innerHTML = paneView === 'split' ? renderSplitSlotSelector(pane, 'left') : paneView
    }
    if (previewLabel) {
      previewLabel.style.display = paneView === 'split' ? '' : 'none'
      previewLabel.innerHTML = paneView === 'split' ? renderSplitSlotSelector(pane, 'right') : 'preview'
    }
    if (paneUsesWysiwyg(pane)) maybeRefreshWysiwygPane(pane)
    else destroyRichEditor(pane)
    if (paneUsesMarkdown(pane)) queueMicrotask(() => editorViews[pane]?.requestMeasure?.())
  })
}

function placeStandaloneView(pane, viewType) {
  const surface = $(`single-surface-${pane}`)
  if (!surface) return

  const preview = $(`preview-single-${pane}`)
  const cmHost = $(`cm-host-${pane}`)

  if (preview) preview.style.display = viewType === 'preview' ? 'block' : 'none'

  if (viewType === 'markdown' && cmHost) {
    surface.appendChild(cmHost)
    cmHost.style.display = 'block'
    queueMicrotask(() => editorViews[pane]?.requestMeasure?.())
  } else if (cmHost) {
    surface.appendChild(cmHost)
    cmHost.style.display = 'none'
  }
}

function placePaneView(pane, slot, viewType) {
  const slotHost = $(`view-slot-${slot}-${pane}`)
  if (!slotHost) return

  const preview = $(`preview-${slot}-${pane}`)
  const cmHost = $(`cm-host-${pane}`)

  if (preview) preview.style.display = viewType === 'preview' ? 'block' : 'none'

  if (viewType === 'markdown' && cmHost) {
    slotHost.appendChild(cmHost)
    cmHost.style.display = 'block'
    queueMicrotask(() => editorViews[pane]?.requestMeasure?.())
  }
}

function updateActiveMetrics() {
  const tab = getFocusedTab()
  const markdown = tab?.content || ''
  const words = $('st-words')
  const lines = $('st-lines')
  updateCursorStatus()
  if (!tab) {
    if (words) words.textContent = '—'
    if (lines) lines.textContent = '—'
    renderStatsPopover(getStats(''))
    renderTocPopover([])
    return
  }
  const stats = getStats(markdown)
  if (words) words.textContent = `${stats.words} words`
  if (lines) lines.textContent = `${markdown.split('\n').length} lines`
  renderStatsPopover(stats)
  renderTocPopover(extractHeadings(markdown))
}

function updateCursorStatus(editorState = getFocusedEditor()?.state) {
  const cursor = $('st-cursor')
  if (!cursor) return
  if (!editorState) {
    cursor.textContent = 'Ln 1, Col 1'
    return
  }
  const head = editorState.selection.main.head
  const line = editorState.doc.lineAt(head)
  const lineNumber = line.number
  const column = head - line.from + 1
  cursor.textContent = `Ln ${lineNumber}, Col ${column}`
}

function onEditorSelectionChange(pane, editorState) {
  if (pane !== state.focusedPane) return
  updateCursorStatus(editorState)
}

function wireEditorUiEvents() {
  $('find-input')?.addEventListener('keyup', updateFind)
  $('find-input')?.addEventListener('keydown', handleFindKeydown)
  $('find-next-btn')?.addEventListener('click', findNext)
  $('find-prev-btn')?.addEventListener('click', findPrev)
  $('replace-one-btn')?.addEventListener('click', replaceOne)
  $('replace-all-btn')?.addEventListener('click', replaceAll)
  $('find-close-btn')?.addEventListener('click', toggleFindReplace)
  $('split-resizer-primary')?.addEventListener('pointerdown', startSplitResize)
  $('split-resizer-secondary')?.addEventListener('pointerdown', startSplitResize)
  $('workspace-resizer')?.addEventListener('pointerdown', startWorkspaceSplitResize)
  document.querySelectorAll('.workspace-toolbar').forEach(node => {
    node.addEventListener('click', handleToolbarClick)
  })
  document.querySelectorAll('.workspace-pane-row').forEach(node => {
    node.addEventListener('click', handleToolbarClick)
  })
  $('insights-panel')?.addEventListener('click', handleToolbarClick)
  document.querySelectorAll('.workspace-pane').forEach(node => {
    node.addEventListener('pointerdown', () => focusPane(node.dataset.pane))
  })
  document.querySelectorAll('.workspace-tabs').forEach(node => {
    node.addEventListener('dragover', handleTabDragOver)
    node.addEventListener('dragleave', handleTabDragLeave)
    node.addEventListener('drop', handleTabDrop)
  })
}

function handleToolbarClick(event) {
  const control = event.target.closest('[data-action]')
  if (!control) return
  const pane = control.closest('[data-pane]')?.dataset.pane
  if (pane) focusPane(pane)

  const { action } = control.dataset

  if (action === 'editor-cmd') editorCmd(control.dataset.cmd)
  if (action === 'toggle-dropdown') toggleDd(control.dataset.dropdown)
  if (action === 'insert-heading') insertHeading(Number(control.dataset.level))
  if (action === 'insert-list') insertList(control.dataset.listType)
  if (action === 'wrap-selection') wrapSelection(control.dataset.prefix || '')
  if (action === 'wrap-inline') wrapInline(control.dataset.before || '', control.dataset.after || '')
  if (action === 'insert-link') insertLink()
  if (action === 'insert-image') insertImage()
  if (action === 'insert-table') insertTable()
  if (action === 'insert-callout') insertCallout(control.dataset.calloutType || 'note')
  if (action === 'toggle-find-replace') toggleFindReplace()
  if (action === 'export-pdf') exportToPdf()
  if (action === 'set-view') setPaneView(pane || state.focusedPane, control.dataset.view)
  if (action === 'set-split-view') setSplitPaneView(pane || state.focusedPane, control.dataset.slot, control.dataset.slotView)
  if (action === 'toggle-toolbar') toggleToolbar()
  if (action === 'toggle-workspace-split') toggleWorkspaceSplit()
  if (action === 'toggle-insights') toggleInsightsPanel()
  if (action === 'toggle-insights-section') toggleInsightsSection(control.dataset.section)
}

// ── File tree ─────────────────────────────────────────────────────
function renderTree(items, container, depth = 0) {
  container.innerHTML = ''
  items.forEach(item => {
    if (item.type === 'folder') {
      const isOpen = state.expandedFolders.has(item.path)
      const f = el('div', `tree-folder${isOpen ? ' open' : ''}`)
      f.style.paddingLeft = `${10 + depth * 14}px`
      f.title = item.name
      f.innerHTML = `<svg viewBox="0 0 6 10"><path d="M1 1l4 4-4 4" stroke-width="1.5" stroke="currentColor" fill="none" stroke-linecap="round"/></svg>${item.name}`
      const children = el('div')
      children.style.display = isOpen ? 'block' : 'none'
      if (item.children) renderTree(item.children, children, depth + 1)
      f.addEventListener('click', () => {
        const nextOpen = !f.classList.contains('open')
        f.classList.toggle('open', nextOpen)
        children.style.display = nextOpen ? 'block' : 'none'
        if (nextOpen) state.expandedFolders.add(item.path)
        else state.expandedFolders.delete(item.path)
      })
      container.appendChild(f)
      container.appendChild(children)
    } else {
      const fi = el('div', 'tree-file')
      fi.style.paddingLeft = `${24 + depth * 14}px`
      fi.title = item.name
      fi.innerHTML = `<div class="tree-file__dot"></div>${item.name}`
      fi.addEventListener('click', () => openFile(item))
      container.appendChild(fi)
    }
  })
}

async function refreshTree() {
  if (!state.folderPath || !window.fjord) return
  state.tree = await window.fjord.readFolder(state.folderPath)
  const folderPaths = collectFolderPaths(state.tree)
  if (state.expandedFolders.size === 0) {
    folderPaths.forEach(path => state.expandedFolders.add(path))
  } else {
    const validPaths = new Set(folderPaths)
    state.expandedFolders.forEach(path => {
      if (!validPaths.has(path)) state.expandedFolders.delete(path)
    })
  }
  renderTree(state.tree, $('file-tree'))
  highlightActiveFile()
}

function syncFolderUi() {
  const openFolderBtn = $('open-folder-btn')
  if (openFolderBtn) {
    openFolderBtn.style.display = state.folderPath ? 'none' : ''
  }
  const sidebarLabel = $('sidebar-label')
  if (sidebarLabel) {
    sidebarLabel.textContent = state.folderPath ? state.folderPath.split('/').pop() : 'Explorer'
  }
  const collapseAllBtn = $('collapse-all-btn')
  if (collapseAllBtn) {
    collapseAllBtn.style.display = state.folderPath ? '' : 'none'
  }
}

function startSidebarResize(event) {
  if (state.sidebarVisible === false) return
  event.preventDefault()
  const resizer = $('sidebar-resizer')
  document.body.classList.add('is-resizing-sidebar')
  resizer?.setPointerCapture?.(event.pointerId)

  const onMove = moveEvent => {
    const width = Math.min(420, Math.max(180, moveEvent.clientX))
    updateSetting('sidebarWidth', width)
  }

  const onUp = () => {
    document.body.classList.remove('is-resizing-sidebar')
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

function startSplitResize(event) {
  const pane = event.currentTarget?.dataset?.pane || 'primary'
  if (getPaneView(pane) !== 'split') return
  event.preventDefault()
  const resizer = event.currentTarget
  const panesMain = $(`panes-main-${pane}`)
  if (!panesMain) return

  document.body.classList.add('is-resizing-split')
  resizer?.setPointerCapture?.(event.pointerId)

  const onMove = moveEvent => {
    const rect = panesMain.getBoundingClientRect()
    const ratio = ((moveEvent.clientX - rect.left) / rect.width) * 100
    updateSetting('splitRatio', Math.min(80, Math.max(20, ratio)))
  }

  const onUp = () => {
    document.body.classList.remove('is-resizing-split')
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

function startWorkspaceSplitResize(event) {
  if (state.workspaceMode !== 'dual') return
  event.preventDefault()
  const resizer = $('workspace-resizer')
  const workspace = $('panes-workspace')
  if (!workspace) return

  document.body.classList.add('is-resizing-split')
  resizer?.setPointerCapture?.(event.pointerId)

  const onMove = moveEvent => {
    const rect = workspace.getBoundingClientRect()
    const ratio = ((moveEvent.clientX - rect.left) / rect.width) * 100
    updateSetting('documentSplitRatio', Math.min(75, Math.max(25, ratio)))
  }

  const onUp = () => {
    document.body.classList.remove('is-resizing-split')
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

function focusPane(pane) {
  if (!pane) return
  if (pane === 'secondary' && state.workspaceMode !== 'dual') return
  state.focusedPane = pane
  syncFocusedPaneUi()
  updateActiveMetrics()
}

function collectFolderPaths(items, result = []) {
  items.forEach(item => {
    if (item.type === 'folder') {
      result.push(item.path)
      if (item.children) collectFolderPaths(item.children, result)
    }
  })
  return result
}

function collapseAllFolders() {
  state.expandedFolders.clear()
  renderTree(state.tree, $('file-tree'))
  highlightActiveFile()
}

function highlightActiveFile() {
  document.querySelectorAll('.tree-file').forEach(f => f.classList.remove('active'))
  const openNames = new Set([state.activeTab?.name, state.secondaryTab?.name].filter(Boolean))
  if (openNames.size === 0) return
  document.querySelectorAll('.tree-file').forEach(f => {
    if (openNames.has(f.textContent.trim())) f.classList.add('active')
  })
}

// ── Open folder ───────────────────────────────────────────────────
async function openFolder() {
  if (!window.fjord) return
  const p = await window.fjord.openFolder()
  if (!p) return
  state.folderPath = p
  state.tabs = []
  state.tabGroups.primary = []
  state.tabGroups.secondary = []
  state.splitSnapshot = {
    primary: [],
    secondary: [],
    activePrimary: null,
    activeSecondary: null,
    focusedPane: 'primary',
  }
  state.activeTab = null
  state.secondaryTab = null
  state.focusedPane = 'primary'
  state.workspaceMode = 'single'
  state.expandedFolders.clear()
  syncFolderUi()
  await window.fjord.watchFolder(p)
  await refreshTree()
  showWelcomeScreen()
}

async function createNewFile() {
  if (!window.fjord || !state.folderPath) {
    alert('Open a folder first')
    return
  }
  const created = await window.fjord.newMarkdownFile(state.folderPath)
  if (!created) return
  await refreshTree()
  await openFile(created)
}

// ── Open file ─────────────────────────────────────────────────────
async function openFile(item) {
  if (!window.fjord) return
  if (!$('workspace-primary')) buildEditorUI()
  const targetPane = state.workspaceMode === 'dual' ? state.focusedPane : 'primary'

  const existing = state.tabs.find(t => t.path === item.path)
  if (existing) {
    activateTab(existing, targetPane)
    return
  }

  const content = await window.fjord.readFile(item.path)
  const tab = { path: item.path, name: item.name, content, dirty: false }
  state.tabs.push(tab)
  activateTab(tab, targetPane)
}

async function loadFileIntoTab(tab) {
  if (!window.fjord) return
  tab.content = await window.fjord.readFile(tab.path)
  tab.dirty = false
  PANE_KEYS.forEach(pane => {
    if (getTabForPane(pane) === tab) {
      if (editorViews[pane]) updateEditorDoc(editorViews[pane], tab.content)
      refreshPreview(pane, tab.content)
      maybeRefreshWysiwygPane(pane)
    }
  })
  renderTabs()
  updateActiveMetrics()
}

// ── Tabs ──────────────────────────────────────────────────────────
function activateTab(tab, pane = 'primary') {
  if (!$('workspace-primary')) buildEditorUI()
  if (pane === 'secondary' && state.workspaceMode !== 'dual') {
    state.workspaceMode = 'dual'
  }

  addTabToPane(tab, pane)
  setTabForPane(pane, tab)
  focusPane(pane)
  ensureEditorForPane(pane)
  if (editorViews[pane]) updateEditorDoc(editorViews[pane], tab.content)
  refreshPreview(pane, tab.content)
  maybeRefreshWysiwygPane(pane)
  syncWorkspaceUi()
  syncSplitLayout()
  renderTabs()
  highlightActiveFile()
  updateActiveMetrics()
}

function renderTabs() {
  PANE_KEYS.forEach(pane => {
    const container = $(`tabs-${pane}`)
    if (!container) return
    container.innerHTML = ''
    container.dataset.pane = pane

    const group = getGroupTabs(pane)
    group.forEach(tab => {
      const isFocused = pane === state.focusedPane && getTabForPane(pane) === tab
      const t = el('div', `tab${isFocused ? ' active' : ''}`)
      t.draggable = state.workspaceMode === 'dual'
      t.dataset.pane = pane
      t.title = tab.name
      t.innerHTML = `
        <div class="tab__dot"></div>
        <span class="tab__name">${tab.name}${tab.dirty ? ' ·' : ''}</span>
        <div class="tab__close">✕</div>
      `
      t.addEventListener('click', () => activateTab(tab, pane))
      t.addEventListener('dragstart', event => handleTabDragStart(event, tab, pane))
      t.addEventListener('dragend', handleTabDragEnd)
      t.querySelector('.tab__close').addEventListener('click', event => {
        event.stopPropagation()
        closeTab(tab, pane)
      })
      container.appendChild(t)
    })

    container.classList.toggle('is-empty', group.length === 0)
    if (group.length === 0) {
      container.innerHTML = `<div class="workspace-tabs__empty">${pane === 'primary' ? 'Open a markdown file' : 'Open another file'}</div>`
    }
  })
}

function closeTab(tab, pane = getTabPane(tab)) {
  if (!pane) return
  const group = getGroupTabs(pane)
  const idx = group.indexOf(tab)
  if (idx < 0) return

  removeTabFromPane(tab, pane)
  const next = group[idx] || group[idx - 1] || null
  setTabForPane(pane, next)
  if (editorViews[pane]) updateEditorDoc(editorViews[pane], next?.content || '')
  refreshPreview(pane, next?.content || '')
  maybeRefreshWysiwygPane(pane)

  if (!isTabOpenAnywhere(tab)) {
    const globalIndex = state.tabs.indexOf(tab)
    if (globalIndex >= 0) state.tabs.splice(globalIndex, 1)
    cleanSplitSnapshot()
  }

  if (!state.activeTab && state.secondaryTab) {
    state.tabGroups.primary = [...state.tabGroups.secondary]
    state.activeTab = state.secondaryTab
    state.tabGroups.secondary = []
    state.secondaryTab = null
    state.workspaceMode = 'single'
    state.focusedPane = 'primary'
  }

  if (state.workspaceMode === 'dual' && state.tabGroups.secondary.length === 0 && !state.secondaryTab) {
    state.focusedPane = 'primary'
  }

  if (!state.activeTab && !state.secondaryTab) {
    showWelcomeScreen()
  } else {
    if (state.focusedPane === 'secondary' && !state.secondaryTab) state.focusedPane = 'primary'
    syncWorkspaceUi()
    syncSplitLayout()
    syncFocusedPaneUi()
    renderTabs()
    highlightActiveFile()
    updateActiveMetrics()
  }
}

function moveTabToPane(tab, fromPane, toPane) {
  if (!tab || !fromPane || !toPane || fromPane === toPane) return
  removeTabFromPane(tab, fromPane)
  addTabToPane(tab, toPane)

  if (getTabForPane(fromPane) === tab) {
    const sourceTabs = getGroupTabs(fromPane)
    setTabForPane(fromPane, sourceTabs[sourceTabs.length - 1] || null)
    if (editorViews[fromPane]) updateEditorDoc(editorViews[fromPane], getTabForPane(fromPane)?.content || '')
    refreshPreview(fromPane, getTabForPane(fromPane)?.content || '')
    maybeRefreshWysiwygPane(fromPane)
  }

  setTabForPane(toPane, tab)
  ensureEditorForPane(toPane)
  if (editorViews[toPane]) updateEditorDoc(editorViews[toPane], tab.content)
  refreshPreview(toPane, tab.content)
  maybeRefreshWysiwygPane(toPane)
  focusPane(toPane)
  storeSplitSnapshot()
  syncWorkspaceUi()
  syncSplitLayout()
  syncFocusedPaneUi()
  renderTabs()
  highlightActiveFile()
  updateActiveMetrics()
}

function handleTabDragStart(event, tab, pane) {
  if (state.workspaceMode !== 'dual') return
  draggedTab = { tab, pane }
  event.dataTransfer.effectAllowed = 'move'
  event.dataTransfer.setData('text/plain', tab.path)
  event.currentTarget.classList.add('dragging')
}

function handleTabDragEnd(event) {
  event.currentTarget.classList.remove('dragging')
  document.querySelectorAll('.workspace-tabs').forEach(node => node.classList.remove('is-drop-target'))
  draggedTab = null
}

function handleTabDragOver(event) {
  if (!draggedTab || state.workspaceMode !== 'dual') return
  event.preventDefault()
  event.dataTransfer.dropEffect = 'move'
  event.currentTarget.classList.add('is-drop-target')
}

function handleTabDragLeave(event) {
  event.currentTarget.classList.remove('is-drop-target')
}

function handleTabDrop(event) {
  if (!draggedTab || state.workspaceMode !== 'dual') return
  event.preventDefault()
  const targetPane = event.currentTarget.dataset.pane
  event.currentTarget.classList.remove('is-drop-target')
  moveTabToPane(draggedTab.tab, draggedTab.pane, targetPane)
}

function showWelcomeScreen() {
  destroyEditors()
  const wrapper = $('editor-wrapper')
  if (!wrapper) return
  wrapper.innerHTML = buildWelcome()
  $('welcome-open-btn')?.addEventListener('click', openFolder)
  renderTabs()
  updateActiveMetrics()
}

// ── Editor changes ────────────────────────────────────────────────
function onEditorChange(pane, content) {
  const tab = getTabForPane(pane)
  if (!tab) return
  tab.content = content
  tab.dirty = true
  syncTabRepresentations(tab, pane, { source: 'markdown' })

  // Auto-save after 800ms idle
  clearTimeout(saveTimers[pane])
  saveTimers[pane] = setTimeout(() => saveTab(tab), 800)
}

function onRichEditorChange(pane) {
  const tab = getTabForPane(pane)
  const editor = richEditors[pane]
  if (!tab || !editor) return
  if (syncingRichEditor[pane]) return

  const markdown = editor.getMarkdown()
  if (tab.content === markdown) return

  tab.content = markdown
  tab.dirty = true
  syncingRichEditor[pane] = true
  if (editorViews[pane]) updateEditorDoc(editorViews[pane], markdown)
  syncingRichEditor[pane] = false
  syncTabRepresentations(tab, pane, { source: 'wysiwyg' })

  clearTimeout(saveTimers[pane])
  saveTimers[pane] = setTimeout(() => saveTab(tab), 800)
}

function syncTabRepresentations(tab, sourcePane, { source } = {}) {
  renderTabs()
  PANE_KEYS.forEach(pane => {
    if (getTabForPane(pane) !== tab) return
    if (source !== 'markdown' && editorViews[pane]) updateEditorDoc(editorViews[pane], tab.content)
    refreshPreview(pane, tab.content)
    if (source !== 'wysiwyg') maybeRefreshWysiwygPane(pane)
  })
  highlightActiveFile()
  updateActiveMetrics()
}

async function saveTab(tab) {
  if (!tab || !window.fjord) return
  const ok = await window.fjord.writeFile(tab.path, tab.content)
  if (ok) { tab.dirty = false; renderTabs() }
}

async function saveActive() {
  const tab = getFocusedTab()
  if (!tab) return
  const ok = await window.fjord.writeFile(tab.path, tab.content)
  if (ok) { tab.dirty = false; renderTabs() }
}

async function saveActiveAs() {
  if (!window.fjord) return
  const tab = getFocusedTab()
  const currentPath = tab?.path || null
  const content = tab?.content || ''
  const saved = await window.fjord.saveFileAs(currentPath, content, state.folderPath)
  if (!saved) return

  await refreshTree()

  if (tab) {
    tab.path = saved.path
    tab.name = saved.name
    tab.dirty = false
    activateTab(tab, state.focusedPane)
    renderTabs()
    return
  }

  await openFile(saved)
}

function handleAppCommand(command) {
  if (command === 'file:new') createNewFile()
  if (command === 'file:open-folder') openFolder()
  if (command === 'file:save') saveActive()
  if (command === 'file:save-as') saveActiveAs()
  if (command === 'file:export-pdf') exportToPdf()
  if (command === 'file:close-tab' && getFocusedTab()) closeTab(getFocusedTab())
}

// ── Preview ───────────────────────────────────────────────────────
async function refreshPreview(pane, markdown) {
  const html = await renderMarkdown(markdown)
  ;['single', 'left', 'right'].forEach(slot => {
    const p = $(`preview-${slot}-${pane}`)
    if (p) p.innerHTML = html
  })
}

// ── Stats ─────────────────────────────────────────────────────────
function updateStats(markdown) {
  void markdown
  updateActiveMetrics()
}

function renderStatsPopover(s) {
  const c = $('stats-content')
  if (!c) return
  c.innerHTML = `
    <div class="stat-card"><div class="num">${s.words}</div><div class="row"><span class="lbl">Words</span></div></div>
    <div class="stat-card"><div class="num">${s.chars}</div><div class="row"><span class="lbl">Characters</span></div></div>
    <div class="stat-card"><div class="num">${s.paragraphs}</div><div class="row"><span class="lbl">Paragraphs</span></div></div>
    <div class="stat-card"><div class="num" style="font-size:15px">${s.readMin < 1 ? '< 1' : s.readMin} min</div><div class="row"><span class="lbl">Read time</span></div></div>
  `
}

function renderTocPopover(headings) {
  const c = $('toc-content')
  if (!c) return
  if (!headings.length) {
    c.innerHTML = `<div class="toc-empty"><div class="icon">🏔️</div><div class="msg">No headers yet</div></div>`
    return
  }
  c.innerHTML = headings.map(h =>
    `<div class="toc-item h${h.level}">${h.text}</div>`
  ).join('')
}

// ── View (edit / split / preview) ────────────────────────────────
function setPaneView(pane, view) {
  if (!pane) return
  const previousView = getPaneView(pane)
  if (view === 'split') {
    state.splitEditableMode[pane] = previousView === 'wysiwyg'
      ? 'wysiwyg'
      : previousView === 'markdown'
        ? 'markdown'
        : getSplitEditableView(pane)
  }
  if (view === 'markdown' || view === 'wysiwyg') {
    state.splitEditableMode[pane] = view
  }
  state.paneView[pane] = view
  const toolbar = document.querySelector(`.workspace-toolbar[data-pane="${pane}"]`)
  ;['markdown', 'split', 'wysiwyg', 'preview'].forEach(viewName => {
    toolbar?.querySelectorAll(`.vb[data-view="${viewName}"]`).forEach(node => {
      node.classList.toggle('active', viewName === view)
    })
  })
  if (view === 'wysiwyg') {
    syncToWysiwyg(pane)
    setTimeout(() => richEditors[pane]?.focus(), 0)
  }
  syncSplitLayout()
}

function setSplitPaneView(pane, slot, view) {
  if (!pane || !slot) return
  if (view === 'preview') {
    state.splitPreviewSide[pane] = slot
  } else {
    state.splitEditableMode[pane] = view === 'wysiwyg' ? 'wysiwyg' : 'markdown'
    state.splitPreviewSide[pane] = slot === 'left' ? 'right' : 'left'
  }
  syncSplitLayout()
  if (paneUsesWysiwyg(pane)) maybeRefreshWysiwygPane(pane)
}

// ── Toolbar toggle ────────────────────────────────────────────────
function toggleToolbar() {
  state.toolbarVisible = !state.toolbarVisible
  document.querySelectorAll('.workspace-toolbar').forEach(node => {
    node.classList.toggle('hidden', !state.toolbarVisible)
  })
  document.querySelectorAll('.workspace-pane-row').forEach(node => {
    node.classList.toggle('hidden', !state.toolbarVisible)
  })
  document.querySelectorAll('.hide-toolbar-btn').forEach(node => {
    node.textContent = state.toolbarVisible ? 'hide toolbar' : 'show toolbar'
  })
  syncToolbarToggle()
}

// ── Sidebar toggle ────────────────────────────────────────────────
function toggleSidebar() {
  state.sidebarVisible = !state.sidebarVisible
  const sb = $('sidebar')
  if (sb) sb.classList.toggle('collapsed', !state.sidebarVisible)
}

// ── Dropdown ─────────────────────────────────────────────────────
function toggleDd(id) {
  const menuId = id.replace(/^dd-/, 'ddm-')
  const menu = $(menuId)
  if (!menu) return
  const was = menu.classList.contains('open')
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
  if (!was) menu.classList.add('open')
}

// ── Insights panel ────────────────────────────────────────────────
function toggleInsightsPanel() {
  state.insightsOpen = !state.insightsOpen
  $('insights-panel')?.classList.toggle('open', state.insightsOpen)
  document.querySelectorAll('[data-action="toggle-insights"]').forEach(node => {
    node.classList.toggle('active', state.insightsOpen)
  })
}

function toggleWorkspaceSplit() {
  state.workspaceMode = state.workspaceMode === 'dual' ? 'single' : 'dual'

  if (state.workspaceMode === 'dual') {
    cleanSplitSnapshot()
    const snapshotPrimary = state.splitSnapshot.primary.filter(tab => state.tabs.includes(tab))
    const snapshotSecondary = state.splitSnapshot.secondary.filter(tab => state.tabs.includes(tab))

    if (snapshotPrimary.length || snapshotSecondary.length) {
      const assigned = new Set([...snapshotPrimary, ...snapshotSecondary])
      const unassigned = state.tabs.filter(tab => !assigned.has(tab))
      state.tabGroups.primary = [...snapshotPrimary, ...unassigned]
      state.tabGroups.secondary = [...snapshotSecondary]
      state.activeTab = state.tabGroups.primary.includes(state.splitSnapshot.activePrimary)
        ? state.splitSnapshot.activePrimary
        : state.tabGroups.primary[0] || null
      state.secondaryTab = state.tabGroups.secondary.includes(state.splitSnapshot.activeSecondary)
        ? state.splitSnapshot.activeSecondary
        : state.tabGroups.secondary[0] || null
      state.focusedPane = state.splitSnapshot.focusedPane === 'secondary' && state.secondaryTab ? 'secondary' : 'primary'
    } else {
      if (!state.activeTab && state.tabs[0]) state.activeTab = state.tabs[0]
      state.tabGroups.primary = state.activeTab ? [state.activeTab, ...state.tabs.filter(tab => tab !== state.activeTab)] : []
      state.tabGroups.secondary = state.activeTab ? [state.activeTab] : []
      state.secondaryTab = state.activeTab
      state.focusedPane = 'secondary'
    }
    ensureEditorForPane('secondary')
  } else {
    storeSplitSnapshot()
    const mergedActiveTab = getFocusedTab() || state.activeTab || state.secondaryTab
    state.tabGroups.primary = [...new Set([...state.tabGroups.primary, ...state.tabGroups.secondary])]
    state.activeTab = mergedActiveTab && state.tabGroups.primary.includes(mergedActiveTab)
      ? mergedActiveTab
      : state.tabGroups.primary[0] || null
    state.secondaryTab = null
    state.tabGroups.secondary = []
    if (editorViews.secondary) {
      editorViews.secondary.destroy()
      editorViews.secondary = null
    }
    destroyRichEditor('secondary')
    clearTimeout(saveTimers.secondary)
    saveTimers.secondary = null
    state.focusedPane = 'primary'
  }

  syncWorkspaceUi()
  syncSplitLayout()
  syncFocusedPaneUi()
  renderTabs()
  refreshAllPreviews()
  updateActiveMetrics()
}

function toggleInsightsSection(section) {
  if (!section || !(section in state.insightsSections)) return
  state.insightsSections[section] = !state.insightsSections[section]
  $(`insights-section-${section}`)?.classList.toggle('open', state.insightsSections[section])
}

function getFocusedWysiwygEditor() {
  if (!paneUsesWysiwyg(state.focusedPane)) return null
  return richEditors[state.focusedPane] || ensureRichEditorMounted(state.focusedPane)
}

function runWysiwygCommand(action) {
  const editor = getFocusedWysiwygEditor()
  if (!editor) return false
  editor.focus()
  action(editor)
  return true
}

function insertMarkdownAtSelection(pane, text, selectLength = 0) {
  const view = editorViews[pane]
  if (!view) return
  const { state: s, dispatch } = view
  const sel = s.selection.main
  dispatch(s.update({
    changes: { from: sel.from, to: sel.to, insert: text },
    selection: { anchor: sel.from + text.length - selectLength, head: sel.from + text.length },
  }))
  view.focus()
}

function insertMarkdownTable(pane, columns, rows) {
  const safeColumns = Math.max(2, Math.min(8, Number(columns) || 3))
  const safeRows = Math.max(2, Math.min(20, Number(rows) || 2))
  const header = `| ${Array.from({ length: safeColumns }, (_, i) => `Column ${i + 1}`).join(' | ')} |`
  const divider = `| ${Array.from({ length: safeColumns }, () => '---').join(' | ')} |`
  const bodyRows = Array.from({ length: safeRows - 1 }, () =>
    `| ${Array.from({ length: safeColumns }, () => 'Cell').join(' | ')} |`,
  )
  insertMarkdownAtSelection(pane, `\n${[header, divider, ...bodyRows].join('\n')}\n`)
}

function submitCommandDialog(event) {
  event.preventDefault()
  const dialog = state.commandDialog
  if (!dialog) return

  const pane = dialog.pane || state.focusedPane
  const form = event.currentTarget
  const data = new FormData(form)

  if (dialog.type === 'link') {
    const url = String(data.get('url') || '').trim()
    const text = String(data.get('text') || '').trim()
    if (!url) return
    if (paneUsesWysiwyg(pane) && richEditors[pane]) {
      focusPane(pane)
      richEditors[pane].focus()
      richEditors[pane].exec('addLink', { linkUrl: url, linkText: text || richEditors[pane].getSelectedText() || url })
    } else {
      insertMarkdownAtSelection(pane, `[${text || 'link'}](${url})`, text ? url.length + 3 : 4)
    }
  }

  if (dialog.type === 'image') {
    const url = String(data.get('url') || '').trim()
    const alt = String(data.get('alt') || '').trim()
    if (!url) return
    if (paneUsesWysiwyg(pane) && richEditors[pane]) {
      focusPane(pane)
      richEditors[pane].focus()
      richEditors[pane].exec('addImage', { imageUrl: url, altText: alt || 'Image' })
    } else {
      insertMarkdownAtSelection(pane, `![${alt || 'alt'}](${url})`, url.length + 1)
    }
  }

  if (dialog.type === 'table') {
    const columns = Number(data.get('columns') || 3)
    const rows = Number(data.get('rows') || 2)
    if (paneUsesWysiwyg(pane) && richEditors[pane]) {
      focusPane(pane)
      richEditors[pane].focus()
      richEditors[pane].exec('addTable', { columnCount: columns, rowCount: rows })
    } else {
      insertMarkdownTable(pane, columns, rows)
    }
  }

  closeCommandDialog()
}

// ── Editor commands ───────────────────────────────────────────────
function editorCmd(cmd) {
  if (runWysiwygCommand(() => {
    if (cmd === 'undo') richEditors[state.focusedPane].exec('undo')
    if (cmd === 'redo') richEditors[state.focusedPane].exec('redo')
  })) return
  const view = getFocusedEditor()
  if (!view) return
  if (cmd === 'undo') undo(view)
  if (cmd === 'redo') redo(view)
}

function wrapInline(before, after) {
  if (runWysiwygCommand(() => {
    if (before === '**' && after === '**') richEditors[state.focusedPane].exec('bold')
    else if (before === '*' && after === '*') richEditors[state.focusedPane].exec('italic')
    else if (before === '~~' && after === '~~') richEditors[state.focusedPane].exec('strike')
    else if (before === '`' && after === '`') richEditors[state.focusedPane].exec('code')
  })) return
  const view = getFocusedEditor()
  if (!view) return
  const { state: s, dispatch } = view
  const sel = s.selection.main
  const selected = s.sliceDoc(sel.from, sel.to)
  dispatch(s.update({
    changes: { from: sel.from, to: sel.to, insert: `${before}${selected || 'text'}${after}` },
    selection: { anchor: sel.from + before.length, head: sel.from + before.length + (selected || 'text').length },
  }))
  view.focus()
}

function wrapSelection(prefix) {
  if (runWysiwygCommand(() => {
    if (prefix === '> ') richEditors[state.focusedPane].exec('blockQuote')
  })) return
  const view = getFocusedEditor()
  if (!view) return
  const { state: s, dispatch } = view
  const line = s.doc.lineAt(s.selection.main.from)
  dispatch(s.update({ changes: { from: line.from, insert: prefix } }))
  view.focus()
}

function insertHeading(level) {
  if (runWysiwygCommand(() => {
    if (level === 0) richEditors[state.focusedPane].exec('paragraph')
    else richEditors[state.focusedPane].exec('heading', { level })
  })) {
    document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
    return
  }
  const view = getFocusedEditor()
  if (!view) return
  const prefix = level === 0 ? '' : '#'.repeat(level) + ' '
  const { state: s, dispatch } = view
  const line = s.doc.lineAt(s.selection.main.from)
  const cleaned = line.text.replace(/^#{1,6}\s*/, '')
  dispatch(s.update({ changes: { from: line.from, to: line.to, insert: prefix + cleaned } }))
  view.focus()
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
}

function insertList(type) {
  if (runWysiwygCommand(() => {
    if (type === 'ordered') richEditors[state.focusedPane].exec('orderedList')
    else if (type === 'bullet') richEditors[state.focusedPane].exec('bulletList')
    else if (type === 'task') richEditors[state.focusedPane].exec('taskList')
  })) {
    document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
    return
  }
  if (!getFocusedEditor()) return
  const prefixes = { bullet: '- ', ordered: '1. ', task: '- [ ] ' }
  wrapSelection(prefixes[type])
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
}

function insertLink() {
  const pane = state.focusedPane
  const selected = paneUsesWysiwyg(pane) && richEditors[pane]
    ? richEditors[pane].getSelectedText()
    : (() => {
        const view = editorViews[pane]
        if (!view) return ''
        const sel = view.state.selection.main
        return view.state.sliceDoc(sel.from, sel.to)
      })()
  openCommandDialog('link', { text: selected })
}

async function insertImage() {
  const picked = await window.fjord?.pickImageFile?.()
  if (picked?.path) {
    insertImageReference(picked.path, picked.name)
    return
  }
  openCommandDialog('image')
}

function insertTable() {
  openCommandDialog('table', { columns: 3, rows: 2 })
}

function insertCallout(type) {
  const pane = state.focusedPane
  const label = formatCalloutLabel(type)
  const snippet = `> [!${String(type || 'note').toUpperCase()}] ${label}\n> `

  if (paneUsesWysiwyg(pane) && richEditors[pane]) {
    focusPane(pane)
    richEditors[pane].focus()
    const selectedText = richEditors[pane].getSelectedText().trim()
    const content = selectedText
      ? `> [!${String(type || 'note').toUpperCase()}] ${label}\n${selectedText.split('\n').map(line => `> ${line}`).join('\n')}\n`
      : snippet
    richEditors[pane].replaceSelection(content)
    document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
    return
  }

  insertMarkdownAtSelection(pane, snippet)
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
}

function formatCalloutLabel(type) {
  return String(type || 'note')
    .replace(/[-_]+/g, ' ')
    .replace(/\b\w/g, letter => letter.toUpperCase()) || 'Note'
}

function insertImageReference(filePath, fileName = '') {
  const pane = state.focusedPane
  const tab = getTabForPane(pane)
  const alt = stripFileExtension(fileName || lastPathSegment(filePath) || 'Image')
  const imagePath = tab?.path ? toRelativePath(tab.path, filePath) : normalizePathSeparators(filePath)

  if (paneUsesWysiwyg(pane) && richEditors[pane]) {
    focusPane(pane)
    richEditors[pane].focus()
    richEditors[pane].exec('addImage', { imageUrl: imagePath, altText: alt || 'Image' })
    return
  }

  insertMarkdownAtSelection(pane, `![${alt || 'Image'}](${imagePath})`, imagePath.length + 1)
}

function normalizePathSeparators(value = '') {
  return String(value).replace(/\\/g, '/')
}

function lastPathSegment(value = '') {
  const normalized = normalizePathSeparators(value)
  const parts = normalized.split('/').filter(Boolean)
  return parts[parts.length - 1] || ''
}

function stripFileExtension(value = '') {
  return String(value).replace(/\.[^.]+$/, '')
}

function directoryPath(filePath = '') {
  const normalized = normalizePathSeparators(filePath)
  const index = normalized.lastIndexOf('/')
  return index >= 0 ? normalized.slice(0, index) : ''
}

function toRelativePath(fromFilePath, toFilePath) {
  const fromDir = directoryPath(fromFilePath)
  const fromParts = normalizePathSeparators(fromDir).split('/').filter(Boolean)
  const toParts = normalizePathSeparators(toFilePath).split('/').filter(Boolean)

  if (!fromParts.length) return normalizePathSeparators(toFilePath)

  let shared = 0
  while (
    shared < fromParts.length &&
    shared < toParts.length &&
    fromParts[shared] === toParts[shared]
  ) {
    shared += 1
  }

  const upward = Array.from({ length: fromParts.length - shared }, () => '..')
  const downward = toParts.slice(shared)
  const joined = [...upward, ...downward].join('/')
  return joined || `./${lastPathSegment(toFilePath)}`
}

// ── Icons ─────────────────────────────────────────────────────────
function sunIcon() {
  return `<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="2.8"/><path d="M8 1.25v2.1M8 12.65v2.1M1.25 8h2.1M12.65 8h2.1M3.35 3.35l1.45 1.45M11.2 11.2l1.45 1.45M3.35 12.65l1.45-1.45M11.2 4.8l1.45-1.45"/></svg>`
}
function moonIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M11.9 10.6A5.8 5.8 0 0 1 5.4 4.1c0-.44.05-.87.15-1.28A6.2 6.2 0 1 0 13.2 10.45c-.41.1-.84.15-1.3.15z"/></svg>`
}
function gearIcon() {
  return `<svg viewBox="0 0 16 16"><line x1="3" y1="4" x2="13" y2="4"/><line x1="3" y1="8" x2="13" y2="8"/><line x1="3" y1="12" x2="13" y2="12"/><circle cx="6" cy="4" r="1.4" fill="currentColor" stroke="none"/><circle cx="10" cy="8" r="1.4" fill="currentColor" stroke="none"/><circle cx="7" cy="12" r="1.4" fill="currentColor" stroke="none"/></svg>`
}
function closeIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/></svg>`
}
function chevronIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M4 6l4 4 4-4"/></svg>`
}

// ── Find & Replace ──────────────────────────────────────────────────
let findMatches = []
let currentMatchIndex = -1

function toggleFindReplace() {
  const panel = $('find-replace')
  panel.classList.toggle('hidden')
  if (!panel.classList.contains('hidden')) {
    $('find-input').focus()
  }
}

function updateFind() {
  const view = getFocusedEditor()
  if (!view) return
  const query = $('find-input').value
  if (!query) {
    findMatches = []
    currentMatchIndex = -1
    updateFindCount()
    return
  }

  const text = view.state.doc.toString()
  findMatches = []
  let idx = 0
  const regex = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi')
  let match
  while ((match = regex.exec(text)) !== null) {
    findMatches.push({ from: match.index, to: match.index + match[0].length })
  }
  currentMatchIndex = findMatches.length > 0 ? 0 : -1
  updateFindCount()
  highlightMatch()
}

function updateFindCount() {
  const count = $('find-count')
  if (findMatches.length === 0) {
    count.textContent = 'No results'
  } else {
    count.textContent = `${currentMatchIndex + 1}/${findMatches.length}`
  }
}

function highlightMatch() {
  const view = getFocusedEditor()
  if (!view) return
  if (currentMatchIndex < 0 || !findMatches[currentMatchIndex]) return
  const match = findMatches[currentMatchIndex]
  view.dispatch({
    selection: { anchor: match.from, head: match.to },
    scrollIntoView: true,
  })
}

function findNext() {
  if (findMatches.length === 0) return
  currentMatchIndex = (currentMatchIndex + 1) % findMatches.length
  updateFindCount()
  highlightMatch()
}

function findPrev() {
  if (findMatches.length === 0) return
  currentMatchIndex = (currentMatchIndex - 1 + findMatches.length) % findMatches.length
  updateFindCount()
  highlightMatch()
}

function replaceOne() {
  const view = getFocusedEditor()
  if (!view || currentMatchIndex < 0) return
  const match = findMatches[currentMatchIndex]
  const replacement = $('replace-input').value
  view.dispatch({
    changes: { from: match.from, to: match.to, insert: replacement },
  })
  onEditorChange(state.focusedPane, view.state.doc.toString())
  updateFind()
}

function replaceAll() {
  const view = getFocusedEditor()
  if (!view || findMatches.length === 0) return
  const replacement = $('replace-input').value
  const changes = findMatches.map(match => ({
    from: match.from,
    to: match.to,
    insert: replacement,
  })).reverse()

  view.dispatch({
    changes,
  })
  onEditorChange(state.focusedPane, view.state.doc.toString())
  updateFind()
}

function handleFindKeydown(e) {
  if (e.key === 'Enter') findNext()
  if (e.key === 'Escape') toggleFindReplace()
}

// ── Image Paste Handler ─────────────────────────────────────────────
async function handleImagePaste(file, view, pane = state.focusedPane) {
  if (!getTabForPane(pane)) return

  // Convert image to base64
  const reader = new FileReader()
  reader.onload = (e) => {
    const base64 = e.target.result
    const ext = file.type.split('/')[1] || 'png'
    const fileName = `image-${Date.now()}.${ext}`

    // Insert markdown image syntax at cursor
    const pos = view.state.selection.main.head
    const markdown = `![](data:${file.type};base64,${base64.split(',')[1]})`

    view.dispatch({
      changes: { from: pos, to: pos, insert: markdown },
      selection: { anchor: pos + markdown.length },
    })

    // Update editor content
    onEditorChange(pane, view.state.doc.toString())
  }
  reader.readAsDataURL(file)
}

// ── PDF Export ──────────────────────────────────────────────────────
async function exportToPdf() {
  const tab = getFocusedTab()
  if (!tab) {
    alert('No file open')
    return
  }
  try {
    const success = await window.fjord.exportPdf(tab.name)
    if (success) {
      // Success notification could be added here
    } else {
      alert('Failed to export PDF')
    }
  } catch (err) {
    alert('Error exporting PDF: ' + err.message)
  }
}

function syncToWysiwyg(pane = state.focusedPane) {
  const editor = richEditors[pane] || ensureRichEditorMounted(pane)
  const tab = getTabForPane(pane)
  if (!editor || !tab) return
  if (editor.getMarkdown() === tab.content) return
  syncingRichEditor[pane] = true
  editor.setMarkdown(tab.content || '')
  syncingRichEditor[pane] = false
}

// ── Boot ─────────────────────────────────────────────────────────
buildShell()
syncFolderUi()

// Keyboard shortcuts
document.addEventListener('keydown', e => {
  const mod = e.metaKey || e.ctrlKey
  if (mod && e.key === 's') { e.preventDefault(); saveActive() }
  if (mod && e.shiftKey && e.key.toLowerCase() === 's') { e.preventDefault(); saveActiveAs() }
  if (mod && e.key === 'n') { e.preventDefault(); createNewFile() }
  if (mod && e.key === 'b') { e.preventDefault(); toggleSidebar() }
  if (mod && e.key === '\\') { e.preventDefault(); toggleToolbar() }
  if (mod && e.key === 'f') { e.preventDefault(); toggleFindReplace() }
  if (mod && e.key === ',') { e.preventDefault(); toggleSettingsPanel() }
  if (e.key === 'Escape' && state.commandDialog) { e.preventDefault(); closeCommandDialog() }
  if (e.key === 'Escape' && state.settingsOpen) { e.preventDefault(); closeSettingsPanel() }
})
