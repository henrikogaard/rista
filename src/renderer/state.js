import { getSettings } from './settings.js'

// ── DOM helpers ───────────────────────────────────────────────────
export const $ = id => document.getElementById(id)
export const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html) e.innerHTML = html; return e }
export const settingsValue = key => getSettings()[key]
export const featureEnabled = key => getSettings()[key] || getSettings().showExperimental

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
