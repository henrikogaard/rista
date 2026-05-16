import { collectFolderPaths, renderFileTree, highlightTreeFiles } from './tree-view.js'
import { updateEditorDoc } from './editor.js'
import { state, $, el, PANE_KEYS, editorViews, richEditors, syncingRichEditor, saveTimers, draggedTab, setDraggedTab, getTabForPane, setTabForPane, getFocusedTab, getGroupTabs, addTabToPane, removeTabFromPane, getTabPane, isTabOpenAnywhere, cleanSplitSnapshot, storeSplitSnapshot } from './state.js'
import { refreshPreview, updateActiveMetrics, exportToPdf } from './preview.js'
import { getSettings, updateSetting } from './settings.js'
import { buildWelcome } from './shell.js'
import { addRecentProject, removeRecentProject } from './recent-projects.js'
import { showContextMenu } from './context-menu.js'
import { saveSession, loadSession } from './session-restore.js'
import { rebuildLinkIndex, updateLinkIndexForFile, removeFromLinkIndex } from './link-index.js'
import { renderInspectorContent } from './inspector.js'

let _sessionTimer = null
let _treeRefreshTimer = null
function persistSession() {
  clearTimeout(_sessionTimer)
  _sessionTimer = setTimeout(() => {
    if (!state.folderPath) return
    saveSession(state.folderPath, {
      tabs: state.tabGroups.primary.filter(t => !t.preview).map(t => ({ path: t.path, pinned: t.pinned || false })),
      secondaryTabs: state.tabGroups.secondary.filter(t => !t.preview).map(t => ({ path: t.path, pinned: t.pinned || false })),
      activeTabPath: state.activeTab?.path || null,
      secondaryTabPath: state.secondaryTab?.path || null,
      workspaceMode: state.workspaceMode,
      sidebarVisible: state.sidebarVisible,
      toolbarVisible: state.toolbarVisible,
    })
  }, 500)
}

function scheduleAutoSave(pane, tab) {
  clearTimeout(saveTimers[pane])
  saveTimers[pane] = setTimeout(() => saveTab(tab), getSettings().autoSaveDelay)
}

export function scheduleTreeRefresh() {
  clearTimeout(_treeRefreshTimer)
  _treeRefreshTimer = setTimeout(() => refreshTree(), 120)
}

function clearExternalConflict(tab) {
  if (!tab) return
  tab.externalConflict = false
  tab.externalContent = null
  tab.externalEvent = null
  if (!state.tabs.some(t => t.externalConflict)) clearStatusNotice()
}

function confirmExternalOverwrite(tab, confirmOverwrite = false) {
  if (!tab?.externalConflict) return true
  if (tab.externalContent === tab.content) {
    clearExternalConflict(tab)
    return true
  }
  if (!confirmOverwrite) return false
  return confirm(`External changes detected for "${tab.name}". Saving now will overwrite the version on disk. Continue?`)
}

function showStatusNotice(message, tone = 'info', { sticky = false } = {}) {
  const node = $('st-update')
  if (!node) return
  node.textContent = message
  node.dataset.tone = tone
  node.hidden = false
  if (!sticky) {
    window.setTimeout(() => {
      if (node.textContent === message && !state.tabs.some(tab => tab.externalConflict)) clearStatusNotice()
    }, 4200)
  }
}

function clearStatusNotice() {
  const node = $('st-update')
  if (!node) return
  node.textContent = ''
  node.removeAttribute('data-tone')
  node.hidden = true
}

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
    onOpenFilePreview: (item) => openFile(item, { preview: true }),
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
  addRecentProject(p)
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
  rebuildLinkIndex().catch(() => {})
  showWelcomeScreen()

  // Try restoring session
  const session = loadSession(state.folderPath)
  if (session && session.tabs?.length) {
    for (const saved of session.tabs) {
      try {
        const content = await window.fjord.readFile(saved.path)
        if (content !== null && content !== undefined) {
          const tab = { path: saved.path, name: saved.path.split('/').pop(), content, dirty: false, pinned: saved.pinned || false }
          state.tabs.push(tab)
          addTabToPane(tab, 'primary')
        }
      } catch {}
    }
    if (session.activeTabPath) {
      const activeTab = state.tabs.find(t => t.path === session.activeTabPath)
      if (activeTab) activateTab(activeTab)
    }
  }
}

export async function openFolderPath(folderPath) {
  if (!window.fjord || !folderPath) return
  state.folderPath = folderPath
  addRecentProject(folderPath)
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
  await window.fjord.watchFolder(folderPath)
  await refreshTree()
  rebuildLinkIndex().catch(() => {})
  showWelcomeScreen()

  // Try restoring session
  const session = loadSession(state.folderPath)
  if (session && session.tabs?.length) {
    for (const saved of session.tabs) {
      try {
        const content = await window.fjord.readFile(saved.path)
        if (content !== null && content !== undefined) {
          const tab = { path: saved.path, name: saved.path.split('/').pop(), content, dirty: false, pinned: saved.pinned || false }
          state.tabs.push(tab)
          addTabToPane(tab, 'primary')
        }
      } catch {}
    }
    if (session.activeTabPath) {
      const activeTab = state.tabs.find(t => t.path === session.activeTabPath)
      if (activeTab) activateTab(activeTab)
    }
  }
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
export async function openFile(item, { preview = false } = {}) {
  if (!window.fjord) return
  if (!$('workspace-primary')) _callbacks.buildEditorUI?.()
  const targetPane = state.workspaceMode === 'dual' ? state.focusedPane : 'primary'

  const existing = state.tabs.find(t => t.path === item.path)
  if (existing) {
    if (!preview && existing.preview) existing.preview = false
    activateTab(existing, targetPane)
    return
  }

  if (preview) {
    const group = getGroupTabs(targetPane)
    const existingPreview = group.find(t => t.preview)
    if (existingPreview) closeTab(existingPreview, targetPane)
  }

  const isAttachment = item.path && (/\.(png|jpe?g|gif|svg|webp|bmp|pdf)$/i).test(item.path)
  let content = ''
  if (!isAttachment) {
    content = await window.fjord.readFile(item.path)
  }
  const tab = { path: item.path, name: item.name, content, dirty: false, pinned: false, isAttachment, preview }
  state.tabs.push(tab)
  activateTab(tab, targetPane)
}

export async function loadFileIntoTab(tab) {
  if (!window.fjord) return
  tab.content = await window.fjord.readFile(tab.path)
  tab.dirty = false
  clearExternalConflict(tab)
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

  if (tab.isAttachment) {
    _callbacks.renderAttachmentPreview?.(pane, tab)
  } else {
    if (editorViews[pane]) updateEditorDoc(editorViews[pane], tab.content)
    refreshPreview(pane, tab.content)
    _callbacks.maybeRefreshWysiwygPane?.(pane)
  }

  _callbacks.syncWorkspaceUi?.()
  _callbacks.syncSplitLayout?.()
  renderTabs()
  highlightActiveFile()
  updateActiveMetrics()
  renderInspectorContent()
  if (window.fjord.setRepresentedFile) window.fjord.setRepresentedFile(tab.path)
  persistSession()
}

export function renderTabs() {
  PANE_KEYS.forEach(pane => {
    const container = $(`tabs-${pane}`)
    if (!container) return
    container.innerHTML = ''
    container.dataset.pane = pane

    const group = getGroupTabs(pane)
    const sortedGroup = [...group].sort((a, b) => (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0))
    sortedGroup.forEach(tab => {
      const isFocused = pane === state.focusedPane && getTabForPane(pane) === tab
      const t = el('div', `tab${isFocused ? ' active' : ''}${tab.pinned ? ' pinned' : ''}${tab.preview ? ' preview' : ''}`)
      t.draggable = state.workspaceMode === 'dual'
      t.dataset.pane = pane
      t.title = tab.externalConflict
        ? `${tab.name} — external changes detected`
        : tab.name
      t.innerHTML = `
        <div class="tab__dot"></div>
        <span class="tab__name">${tab.name}${tab.externalConflict ? ' !' : tab.dirty ? ' ·' : ''}</span>
        ${tab.pinned ? '<span class="tab__pin">&#128204;</span>' : '<div class="tab__close">✕</div>'}
      `
      t.classList.toggle('conflict', Boolean(tab.externalConflict))
      t.addEventListener('click', () => activateTab(tab, pane))
      t.addEventListener('dblclick', () => {
        if (tab.preview) { tab.preview = false; renderTabs() }
      })
      t.addEventListener('dragstart', event => handleTabDragStart(event, tab, pane))
      t.addEventListener('dragend', handleTabDragEnd)
      const closeBtn = t.querySelector('.tab__close')
      if (closeBtn) {
        closeBtn.addEventListener('click', event => {
          event.stopPropagation()
          closeTab(tab, pane)
        })
      }
      t.addEventListener('contextmenu', (e) => {
        e.preventDefault()
        showTabContextMenu(e.clientX, e.clientY, tab, pane)
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
  if (tab.pinned) return  // Don't close pinned tabs
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
    persistSession()
  } else {
    if (state.focusedPane === 'secondary' && !state.secondaryTab) state.focusedPane = 'primary'
    _callbacks.syncWorkspaceUi?.()
    _callbacks.syncSplitLayout?.()
    _callbacks.syncFocusedPaneUi?.()
    renderTabs()
    highlightActiveFile()
    updateActiveMetrics()
    persistSession()
  }
}

export function togglePinTab(tab) {
  if (!tab) return
  tab.pinned = !tab.pinned
  renderTabs()
}

function showTabContextMenu(x, y, tab, pane) {
  const items = [
    ...(tab.externalConflict ? [
      { label: 'Reload from Disk', action: () => reloadExternalChanges(tab) },
      { label: 'Overwrite Disk', action: () => overwriteExternalChanges(tab) },
      { separator: true },
    ] : []),
    { label: 'Close', action: () => closeTab(tab, pane) },
    { label: 'Close Others', action: () => {
      const group = [...getGroupTabs(pane)]
      for (const t of group) {
        if (t !== tab && !t.pinned) closeTab(t, pane)
      }
    }},
    { label: 'Close All', action: () => {
      const group = [...getGroupTabs(pane)]
      for (const t of group) {
        if (!t.pinned) closeTab(t, pane)
      }
    }},
    { label: 'Close to the Right', action: () => {
      const group = getGroupTabs(pane)
      const idx = group.indexOf(tab)
      const toClose = group.slice(idx + 1).filter(t => !t.pinned)
      for (const t of toClose) closeTab(t, pane)
    }},
    { separator: true },
    { label: tab.pinned ? 'Unpin' : 'Pin', action: () => togglePinTab(tab) },
    { separator: true },
    { label: 'Copy Path', action: () => navigator.clipboard.writeText(tab.path) },
    { label: 'Reveal in Finder', action: () => window.fjord.showInFolder(tab.path) },
  ]
  showContextMenu(x, y, items)
}

export async function reloadExternalChanges(tab) {
  if (!tab || !window.fjord) return false
  await loadFileIntoTab(tab)
  showStatusNotice(`Reloaded ${tab.name}`, 'success')
  return true
}

export async function overwriteExternalChanges(tab) {
  if (!tab) return false
  const ok = await saveTab(tab, { confirmOverwrite: true })
  if (ok) showStatusNotice(`Saved ${tab.name}`, 'success')
  return ok
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
  // Wire recent projects click handlers
  const welcomeEl = $('welcome')
  if (welcomeEl) {
    welcomeEl.addEventListener('click', (e) => {
      const removeBtn = e.target.closest('[data-remove-path]')
      if (removeBtn) {
        e.stopPropagation()
        removeRecentProject(removeBtn.dataset.removePath)
        const item = removeBtn.closest('.recent-item')
        if (item) item.remove()
        const remaining = welcomeEl.querySelectorAll('.recent-item')
        if (remaining.length === 0) {
          const recentSection = welcomeEl.querySelector('.recent-projects')
          if (recentSection) recentSection.remove()
        }
        return
      }
      const recentItem = e.target.closest('.recent-item')
      if (recentItem) {
        const folderPath = recentItem.dataset.path
        if (folderPath) openFolderPath(folderPath)
      }
    })
  }
  renderTabs()
  updateActiveMetrics()
}

// ── Editor changes ───────────────────────────────────────────────
export function onEditorChange(pane, content) {
  const tab = getTabForPane(pane)
  if (!tab) return
  if (tab.preview) tab.preview = false
  tab.content = content
  tab.dirty = true
  syncTabRepresentations(tab, pane, { source: 'markdown' })

  scheduleAutoSave(pane, tab)
}

export function onRichEditorChange(pane) {
  if (syncingRichEditor[pane]) return
  const tab = getTabForPane(pane)
  const editor = richEditors[pane]
  if (!tab || !editor) return
  if (tab.preview) tab.preview = false

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

  scheduleAutoSave(pane, tab)
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

export async function saveTab(tab, options = {}) {
  if (!tab || !window.fjord) return
  if (!confirmExternalOverwrite(tab, Boolean(options.confirmOverwrite))) return false
  const ok = await window.fjord.writeFile(tab.path, tab.content)
  if (ok) {
    const hadConflict = Boolean(tab.externalConflict)
    tab.dirty = false
    clearExternalConflict(tab)
    renderTabs()
    if (hadConflict) showStatusNotice(`Saved ${tab.name}`, 'success')
    updateLinkIndexForFile(tab.path, tab.content)
    renderInspectorContent()
  }
  return ok
}

export async function saveActive() {
  const tab = getFocusedTab()
  if (!tab) return
  return saveTab(tab, { confirmOverwrite: true })
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
    clearExternalConflict(tab)
    activateTab(tab, state.focusedPane)
    renderTabs()
    return
  }

  await openFile(saved)
}

// ── App commands ─────────────────────────────────────────────────
export function handleAppCommand(command, data) {
  if (command === 'file:new') createNewFile()
  if (command === 'file:open-folder') openFolder()
  if (command === 'file:save') saveActive()
  if (command === 'file:save-as') saveActiveAs()
  if (command === 'file:export-pdf') exportToPdf()
  if (command === 'file:close-tab' && getFocusedTab()) closeTab(getFocusedTab())
  if (command === 'view:toggle-sidebar') _callbacks.toggleSidebar?.()
  if (command === 'view:toggle-toolbar') _callbacks.toggleToolbar?.()
  if (command === 'view:toggle-inspector') _callbacks.toggleInspector?.()
  if (command === 'view:toggle-terminal') _callbacks.toggleTerminal?.()
  if (command === 'view:toggle-zen') _callbacks.toggleZen?.()
  if (command === 'view:settings') _callbacks.toggleSettings?.()
  if (command === 'file:open' && data?.path) {
    const name = data.path.split('/').pop()
    openFile({ path: data.path, name })
  }
  if (command === 'update:available') {
    const updateEl = $('st-update')
    if (updateEl) {
      updateEl.textContent = 'Update available'
      updateEl.style.display = 'inline'
    }
  }
  if (command === 'update:ready') {
    const updateEl = $('st-update')
    if (updateEl) {
      updateEl.textContent = 'Update ready — restart to install'
      updateEl.style.display = 'inline'
    }
  }
}

export async function handleExternalFileChange({ event, path: changedPath } = {}) {
  if (!changedPath) return

  const tab = state.tabs.find(t => t.path === changedPath)
  if (tab) {
    if (tab.dirty) {
      tab.externalConflict = true
      tab.externalEvent = event
      tab.externalContent = await window.fjord.readFile(changedPath)
      showStatusNotice(`External change: ${tab.name}`, 'warning', { sticky: true })
      renderTabs()
    } else {
      await loadFileIntoTab(tab)
    }
  }

  scheduleTreeRefresh()
}
