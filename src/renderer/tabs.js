import { collectFolderPaths, renderFileTree, highlightTreeFiles } from './tree-view.js'
import { updateEditorDoc } from './editor.js'
import { state, $, el, PANE_KEYS, editorViews, richEditors, syncingRichEditor, saveTimers, draggedTab, setDraggedTab, getTabForPane, setTabForPane, getFocusedTab, getGroupTabs, addTabToPane, removeTabFromPane, getTabPane, isTabOpenAnywhere, cleanSplitSnapshot, storeSplitSnapshot } from './state.js'
import { refreshPreview, updateActiveMetrics, exportToPdf } from './preview.js'
import { updateSetting } from './settings.js'
import { buildWelcome } from './shell.js'

// ── Callback registration ────────────────────────────────────────
let _callbacks = {}
export function registerTabCallbacks(cbs) { Object.assign(_callbacks, cbs) }

// ── File tree ────────────────────────────────────────────────────
export function renderTree(items, container, depth = 0) {
  renderFileTree({
    items,
    container,
    expandedPaths: state.expandedFolders,
    activePaths: new Set([state.activeTab?.path, state.secondaryTab?.path].filter(Boolean)),
    onToggleFolder: (folderPath, nextOpen) => {
      if (nextOpen) state.expandedFolders.add(folderPath)
      else state.expandedFolders.delete(folderPath)
    },
    onOpenFile: openFile,
    depth,
  })
}

export async function refreshTree() {
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

export function syncFolderUi() {
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

export function collapseAllFolders() {
  state.expandedFolders.clear()
  renderTree(state.tree, $('file-tree'))
  highlightActiveFile()
}

export function highlightActiveFile() {
  highlightTreeFiles(new Set([state.activeTab?.path, state.secondaryTab?.path].filter(Boolean)))
}

export function startSidebarResize(event) {
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

// ── Open folder ──────────────────────────────────────────────────
export async function openFolder() {
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

export async function createNewFile() {
  if (!window.fjord || !state.folderPath) {
    alert('Open a folder first')
    return
  }
  const created = await window.fjord.newMarkdownFile(state.folderPath)
  if (!created) return
  await refreshTree()
  await openFile(created)
}

// ── Open file ────────────────────────────────────────────────────
export async function openFile(item) {
  if (!window.fjord) return
  if (!$('workspace-primary')) _callbacks.buildEditorUI?.()
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

export async function loadFileIntoTab(tab) {
  if (!window.fjord) return
  tab.content = await window.fjord.readFile(tab.path)
  tab.dirty = false
  PANE_KEYS.forEach(pane => {
    if (getTabForPane(pane) === tab) {
      if (editorViews[pane]) updateEditorDoc(editorViews[pane], tab.content)
      refreshPreview(pane, tab.content)
      _callbacks.maybeRefreshWysiwygPane?.(pane)
    }
  })
  renderTabs()
  updateActiveMetrics()
}

// ── Tabs ─────────────────────────────────────────────────────────
export function activateTab(tab, pane = 'primary') {
  if (!$('workspace-primary')) _callbacks.buildEditorUI?.()
  if (pane === 'secondary' && state.workspaceMode !== 'dual') {
    state.workspaceMode = 'dual'
  }

  addTabToPane(tab, pane)
  setTabForPane(pane, tab)
  _callbacks.focusPane?.(pane)
  _callbacks.ensureEditorForPane?.(pane)
  if (editorViews[pane]) updateEditorDoc(editorViews[pane], tab.content)
  refreshPreview(pane, tab.content)
  _callbacks.maybeRefreshWysiwygPane?.(pane)
  _callbacks.syncWorkspaceUi?.()
  _callbacks.syncSplitLayout?.()
  renderTabs()
  highlightActiveFile()
  updateActiveMetrics()
}

export function renderTabs() {
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

export function closeTab(tab, pane = getTabPane(tab)) {
  if (!pane) return
  const group = getGroupTabs(pane)
  const idx = group.indexOf(tab)
  if (idx < 0) return

  removeTabFromPane(tab, pane)
  const next = group[idx] || group[idx - 1] || null
  setTabForPane(pane, next)
  if (editorViews[pane]) updateEditorDoc(editorViews[pane], next?.content || '')
  refreshPreview(pane, next?.content || '')
  _callbacks.maybeRefreshWysiwygPane?.(pane)

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
    _callbacks.syncWorkspaceUi?.()
    _callbacks.syncSplitLayout?.()
    _callbacks.syncFocusedPaneUi?.()
    renderTabs()
    highlightActiveFile()
    updateActiveMetrics()
  }
}

export function moveTabToPane(tab, fromPane, toPane) {
  if (!tab || !fromPane || !toPane || fromPane === toPane) return
  removeTabFromPane(tab, fromPane)
  addTabToPane(tab, toPane)

  if (getTabForPane(fromPane) === tab) {
    const sourceTabs = getGroupTabs(fromPane)
    setTabForPane(fromPane, sourceTabs[sourceTabs.length - 1] || null)
    if (editorViews[fromPane]) updateEditorDoc(editorViews[fromPane], getTabForPane(fromPane)?.content || '')
    refreshPreview(fromPane, getTabForPane(fromPane)?.content || '')
    _callbacks.maybeRefreshWysiwygPane?.(fromPane)
  }

  setTabForPane(toPane, tab)
  _callbacks.ensureEditorForPane?.(toPane)
  if (editorViews[toPane]) updateEditorDoc(editorViews[toPane], tab.content)
  refreshPreview(toPane, tab.content)
  _callbacks.maybeRefreshWysiwygPane?.(toPane)
  _callbacks.focusPane?.(toPane)
  storeSplitSnapshot()
  _callbacks.syncWorkspaceUi?.()
  _callbacks.syncSplitLayout?.()
  _callbacks.syncFocusedPaneUi?.()
  renderTabs()
  highlightActiveFile()
  updateActiveMetrics()
}

// ── Tab drag handlers ────────────────────────────────────────────
export function handleTabDragStart(event, tab, pane) {
  if (state.workspaceMode !== 'dual') return
  setDraggedTab({ tab, pane })
  event.dataTransfer.effectAllowed = 'move'
  event.dataTransfer.setData('text/plain', tab.path)
  event.currentTarget.classList.add('dragging')
}

export function handleTabDragEnd(event) {
  event.currentTarget.classList.remove('dragging')
  document.querySelectorAll('.workspace-tabs').forEach(node => node.classList.remove('is-drop-target'))
  setDraggedTab(null)
}

export function handleTabDragOver(event) {
  if (!draggedTab || state.workspaceMode !== 'dual') return
  event.preventDefault()
  event.dataTransfer.dropEffect = 'move'
  event.currentTarget.classList.add('is-drop-target')
}

export function handleTabDragLeave(event) {
  event.currentTarget.classList.remove('is-drop-target')
}

export function handleTabDrop(event) {
  if (!draggedTab || state.workspaceMode !== 'dual') return
  event.preventDefault()
  const targetPane = event.currentTarget.dataset.pane
  event.currentTarget.classList.remove('is-drop-target')
  moveTabToPane(draggedTab.tab, draggedTab.pane, targetPane)
}

// ── Welcome screen ───────────────────────────────────────────────
export function showWelcomeScreen() {
  _callbacks.destroyEditors?.()
  const wrapper = $('editor-wrapper')
  if (!wrapper) return
  wrapper.innerHTML = buildWelcome()
  $('welcome-open-btn')?.addEventListener('click', openFolder)
  renderTabs()
  updateActiveMetrics()
}

// ── Editor changes ───────────────────────────────────────────────
export function onEditorChange(pane, content) {
  const tab = getTabForPane(pane)
  if (!tab) return
  tab.content = content
  tab.dirty = true
  syncTabRepresentations(tab, pane, { source: 'markdown' })

  // Auto-save after 800ms idle
  clearTimeout(saveTimers[pane])
  saveTimers[pane] = setTimeout(() => saveTab(tab), 800)
}

export function onRichEditorChange(pane) {
  if (syncingRichEditor[pane]) return
  const tab = getTabForPane(pane)
  const editor = richEditors[pane]
  if (!tab || !editor) return

  let markdown
  try {
    markdown = editor.getMarkdown()
  } catch (err) {
    console.error(`[fjordmark] Failed to read WYSIWYG content for pane "${pane}":`, err)
    return
  }
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

export function syncTabRepresentations(tab, sourcePane, { source } = {}) {
  renderTabs()
  PANE_KEYS.forEach(pane => {
    if (getTabForPane(pane) !== tab) return
    // Update CM — skip if this change originated from markdown (CM already has it)
    if (source !== 'markdown' && editorViews[pane]) updateEditorDoc(editorViews[pane], tab.content)
    refreshPreview(pane, tab.content)
    // Update WYSIWYG for panes where the change did NOT originate from WYSIWYG
    if (source !== 'wysiwyg') _callbacks.maybeRefreshWysiwygPane?.(pane)
  })
  highlightActiveFile()
  updateActiveMetrics()
}

export async function saveTab(tab) {
  if (!tab || !window.fjord) return
  const ok = await window.fjord.writeFile(tab.path, tab.content)
  if (ok) { tab.dirty = false; renderTabs() }
}

export async function saveActive() {
  const tab = getFocusedTab()
  if (!tab) return
  const ok = await window.fjord.writeFile(tab.path, tab.content)
  if (ok) { tab.dirty = false; renderTabs() }
}

export async function saveActiveAs() {
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

// ── App commands ─────────────────────────────────────────────────
export function handleAppCommand(command) {
  if (command === 'file:new') createNewFile()
  if (command === 'file:open-folder') openFolder()
  if (command === 'file:save') saveActive()
  if (command === 'file:save-as') saveActiveAs()
  if (command === 'file:export-pdf') exportToPdf()
  if (command === 'file:close-tab' && getFocusedTab()) closeTab(getFocusedTab())
}
