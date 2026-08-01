import { getSettings } from './settings.js'

// ── DOM helpers ───────────────────────────────────────────────────
export const $ = id => document.getElementById(id)
export const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html) e.innerHTML = html; return e }
export const settingsValue = key => getSettings()[key]
export const featureEnabled = key => getSettings()[key] || getSettings().showExperimental

// Shared HTML escaping — used for any user-controlled text interpolated into
// templates. The 5-entity version is safe in both text and attribute contexts.
export function escapeHtml(value = '') {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

// Shared path helpers — both slash directions, cross-platform.
export function fileName(path) {
  return String(path || '').split(/[\\/]/).pop() || ''
}

export function stripMarkdownExtension(name) {
  return String(name || '').replace(/\.md$/i, '')
}

// Shared pointer-drag resize loop: rAF-throttled width apply, pointer capture,
// move/up/cancel cleanup. Used by the left and right sidebar resizers.
// opts: { min, max, initial, bodyClass, captureTarget, setWidth(px), onCommit(px) }
export function startDragResize(event, opts) {
  if (!opts || !opts.setWidth) return
  event.preventDefault()
  document.body.classList.add(opts.bodyClass)
  opts.captureTarget?.setPointerCapture?.(event.pointerId)
  let pendingWidth = opts.initial
  let frame = 0

  const applyWidth = () => {
    frame = 0
    opts.setWidth(pendingWidth)
  }

  const onMove = moveEvent => {
    const next = opts.computeWidth ? opts.computeWidth(moveEvent, pendingWidth) : pendingWidth
    pendingWidth = Math.min(opts.max ?? Infinity, Math.max(opts.min ?? 0, next))
    if (!frame) frame = requestAnimationFrame(applyWidth)
  }

  const onUp = () => {
    if (frame) {
      cancelAnimationFrame(frame)
      applyWidth()
    }
    document.body.classList.remove(opts.bodyClass)
    opts.onCommit?.(pendingWidth)
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

// ── App state ────────────────────────────────────────────────────
export const state = {
  folderPath: null,
  singleFilePath: null,
  tree: [],
  tagFilter: null,
  expandedFolders: new Set(),
  appMeta: { name: 'Rísta', version: '' },
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
  sidebarMode: 'explorer', // 'explorer' | 'agents'
  inspectorOpen: false,
  rightPanel: null,       // string | null — active right panel id
  settingsOpen: false,
  commandDialog: null,
  commandPaletteOpen: false,
  zenMode: false,
  _zenMouseHandler: null,
}

// ── Refs ──────────────────────────────────────────────────────────
export const PANE_KEYS = ['primary', 'secondary']
export const editorViews = {
  primary: null,
  secondary: null,
}
export const richEditors = {
  primary: null,
  secondary: null,
}
export const richEditorMountTarget = {
  primary: null,
  secondary: null,
}
export const saveTimers = {
  primary: null,
  secondary: null,
}
export const syncingRichEditor = {
  primary: false,
  secondary: false,
}
export let draggedTab = null
export function setDraggedTab(value) {
  draggedTab = value
}

// ── Pure state accessors ─────────────────────────────────────────
export function getPaneView(pane = state.focusedPane) {
  return state.paneView[pane] || 'split'
}

export function getSplitView(pane = state.focusedPane) {
  return makeSplitView(getSplitEditableView(pane), getSplitPreviewSide(pane))
}

export function paneUsesWysiwyg(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'wysiwyg') return true
  if (paneView !== 'split') return false
  return getSplitEditableView(pane) === 'wysiwyg'
}

export function paneUsesMarkdown(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'markdown') return true
  if (paneView !== 'split') return false
  return getSplitEditableView(pane) === 'markdown'
}

export function getWysiwygMountSlot(pane = state.focusedPane) {
  const paneView = getPaneView(pane)
  if (paneView === 'wysiwyg') return 'single'
  if (paneView !== 'split') return null
  if (getSplitEditableView(pane) !== 'wysiwyg') return null
  return getSplitPreviewSide(pane) === 'left' ? 'right' : 'left'
}

export function getSplitEditableView(pane = state.focusedPane) {
  return state.splitEditableMode[pane] === 'wysiwyg' ? 'wysiwyg' : 'markdown'
}

export function getSplitPreviewSide(pane = state.focusedPane) {
  return state.splitPreviewSide[pane] === 'left' ? 'left' : 'right'
}

export function makeSplitView(editableView = 'markdown', previewSlot = 'right') {
  const nextView = editableView === 'wysiwyg' ? 'wysiwyg' : 'markdown'
  return previewSlot === 'left'
    ? { left: 'preview', right: nextView }
    : { left: nextView, right: 'preview' }
}

export function getTabForPane(pane = state.focusedPane) {
  return pane === 'secondary' ? state.secondaryTab : state.activeTab
}

export function setTabForPane(pane, tab) {
  if (pane === 'secondary') state.secondaryTab = tab
  else state.activeTab = tab
}

export function getFocusedTab() {
  return getTabForPane(state.focusedPane)
}

export function getFocusedEditor() {
  return editorViews[state.focusedPane] || null
}

export function getGroupTabs(pane) {
  return state.tabGroups[pane]
}

export function hasTabInPane(tab, pane) {
  return getGroupTabs(pane).includes(tab)
}

export function addTabToPane(tab, pane) {
  const group = getGroupTabs(pane)
  if (!group.includes(tab)) group.push(tab)
}

export function removeTabFromPane(tab, pane) {
  const group = getGroupTabs(pane)
  const index = group.indexOf(tab)
  if (index >= 0) group.splice(index, 1)
}

export function getTabPane(tab, pane = state.focusedPane) {
  if (!tab) return null
  if (hasTabInPane(tab, pane)) return pane
  if (hasTabInPane(tab, 'primary')) return 'primary'
  if (hasTabInPane(tab, 'secondary')) return 'secondary'
  return null
}

export function isTabOpenAnywhere(tab) {
  return PANE_KEYS.some(pane => hasTabInPane(tab, pane))
}

export function cleanSplitSnapshot() {
  const validTabs = new Set(state.tabs)
  state.splitSnapshot.primary = state.splitSnapshot.primary.filter(tab => validTabs.has(tab))
  state.splitSnapshot.secondary = state.splitSnapshot.secondary.filter(tab => validTabs.has(tab))
  if (state.splitSnapshot.activePrimary && !validTabs.has(state.splitSnapshot.activePrimary)) state.splitSnapshot.activePrimary = null
  if (state.splitSnapshot.activeSecondary && !validTabs.has(state.splitSnapshot.activeSecondary)) state.splitSnapshot.activeSecondary = null
}

export function storeSplitSnapshot() {
  state.splitSnapshot = {
    primary: [...state.tabGroups.primary],
    secondary: [...state.tabGroups.secondary],
    activePrimary: state.activeTab,
    activeSecondary: state.secondaryTab,
    focusedPane: state.focusedPane,
  }
  cleanSplitSnapshot()
}

// ── Path helpers ───────────────────────────────────────────────────
export function relativeFilePath(path, folderPath) {
  const normalizedPath = String(path || '').replace(/\\/g, '/')
  const normalizedFolder = String(folderPath || '').replace(/\\/g, '/').replace(/\/+$/, '')
  if (normalizedFolder && normalizedPath.startsWith(normalizedFolder)) {
    return normalizedPath.slice(normalizedFolder.length).replace(/^\/+/, '')
  }
  return normalizedPath
}
