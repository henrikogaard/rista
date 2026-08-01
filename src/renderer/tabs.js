import { collectFolderPaths, renderFileTree, highlightTreeFiles } from './tree-view.js'
import { updateEditorDoc } from './editor.js'
import { markFileSwitchStart, markFileSwitchDone } from './perf-budget.js'
import { state, $, el, PANE_KEYS, editorViews, richEditors, syncingRichEditor, saveTimers, draggedTab, setDraggedTab, getTabForPane, setTabForPane, getFocusedTab, getGroupTabs, addTabToPane, removeTabFromPane, getTabPane, isTabOpenAnywhere, cleanSplitSnapshot, storeSplitSnapshot, fileName, stripMarkdownExtension, startDragResize } from './state.js'
import { refreshPreview, updateActiveMetrics, exportToPdf } from './preview.js'
import { getSettings, updateSetting } from './settings.js'
import { attachWelcomeProjectHandlers, buildWelcome, syncWorkspaceChrome } from './shell.js'
import { addRecentProject } from './recent-projects.js'
import { showContextMenu } from './context-menu.js'
import { saveSession, loadSession } from './session-restore.js'
import { rebuildLinkIndex, updateLinkIndexForFile, getFilesForTag } from './link-index.js'
import { refreshRightPanel } from './right-panel.js'
import { saveSnapshot } from './history.js'
import { exportAsWebsite } from './publish.js'
import { toggleBookmark, isBookmarked, resetBookmarksCache } from './bookmarks.js'
import { refreshFileExplorerState } from './file-explorer-view.js'
import { jumpToLine } from './outline-view.js'
import { mergeFrontmatterWithBody } from './markdown.js'
import { isAttachmentFile, isSpatialFile, isLikelyBinaryFile } from './attachment-preview.js'
import { schedulePeriodicSave, loadRecoveryBuffer, hasRecoveryBuffer } from './crash-recovery.js'

let _sessionTimer = null
let _treeRefreshTimer = null
let _activeTreePrompt = null
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

// ── Debounced metrics pipeline (stats + headings) ─────────────
// Separated from the synchronous keystroke path so expensive
// getStats() / extractHeadings() calls do not block editing.
let _metricsTimer = null
const METRICS_DEBOUNCE_MS = 150

export function scheduleMetricsUpdate() {
  clearTimeout(_metricsTimer)
  _metricsTimer = setTimeout(() => {
    updateActiveMetrics()
  }, METRICS_DEBOUNCE_MS)
}



function clearExternalConflict(tab) {
  if (!tab) return
  tab.externalConflict = false
  tab.externalContent = null
  tab.externalEvent = null
  if (!state.tabs.some(t => t.externalConflict)) clearStatusNotice()
}

function tryRebuildLinkIndex() {
  if (getSettings().featureWikilinks || getSettings().showExperimental) {
    rebuildLinkIndex().catch(() => {})
  }
}

async function tryAwaitRebuildLinkIndex() {
  if (getSettings().featureWikilinks || getSettings().showExperimental) {
    try { await rebuildLinkIndex() } catch {}
  }
}

async function tryRestoreSession() {
  if (!state.folderPath) return
  // Check for unsaved crash recovery first
  if (hasRecoveryBuffer()) {
    const unsaved = loadRecoveryBuffer()
    if (unsaved && unsaved.length) {
      for (const u of unsaved) {
        if (u.path && u.content) {
          const tab = state.tabs.find(t => t.path === u.path)
          if (tab) {
            tab.content = u.content
            tab.dirty = true
          }
        }
      }
    }
  }
  const session = loadSession(state.folderPath)
  if (!session || !session.tabs?.length) return
  for (const saved of session.tabs) {
    try {
      const content = await window.fjord.readFile(saved.path)
      if (content !== null && content !== undefined) {
        const tab = { path: saved.path, name: fileName(saved.path), content, dirty: false, pinned: saved.pinned || false }
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

function confirmExternalOverwrite(tab, confirmOverwrite = false) {
  if (!tab?.externalConflict) return true
  if (tab.externalContent === tab.content) {
    clearExternalConflict(tab)
    return true
  }
  if (!confirmOverwrite) return false
  return confirm(`External changes detected for "${tab.name}". Saving now will overwrite the version on disk. Continue?`)
}

export function showStatusNotice(message, tone = 'info', { sticky = false } = {}) {
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

function resetOpenDocuments() {
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
}

function parentPath(filePath) {
  return String(filePath || '').replace(/[/\\][^/\\]+$/, '')
}

// ── Callback registration ────────────────────────────────────────
let _callbacks = {}
export function registerTabCallbacks(cbs) { Object.assign(_callbacks, cbs) }

// ── File tree ────────────────────────────────────────────────────
export function renderTree(items, container, depth = 0) {
  const visibleItems = state.tagFilter
    ? filterTreeByPaths(items, new Set(getFilesForTag(state.tagFilter)))
    : items

  renderFileTree({
    items: visibleItems,
    container,
    expandedPaths: state.expandedFolders,
    activePaths: new Set([state.activeTab?.path, state.secondaryTab?.path].filter(Boolean)),
    onToggleFolder: (folderPath, nextOpen) => {
      if (nextOpen) state.expandedFolders.add(folderPath)
      else state.expandedFolders.delete(folderPath)
    },
    onOpenFile: openFile,
    onOpenFilePreview: (item) => openFile(item, { preview: true }),
    onContextMenu: showTreeContextMenu,
    depth,
  })
}

function filterTreeByPaths(items, allowedPaths) {
  return items.flatMap(item => {
    if (item.type === 'file') {
      return allowedPaths.has(item.path) ? [item] : []
    }
    const children = item.children ? filterTreeByPaths(item.children, allowedPaths) : []
    return children.length ? [{ ...item, children }] : []
  })
}

function showTreeContextMenu(item, event) {
  if (item.type === 'folder') {
    showContextMenu(event.clientX, event.clientY, [
      { label: 'New File', action: () => createFileInFolder(item.path) },
      { label: 'New Folder', action: () => createFolderInFolder(item.path) },
      { separator: true },
      { label: 'Rename Folder', action: () => renameTreeItem(item) },
      { label: 'Delete Folder', action: () => deleteTreeItem(item) },
      { separator: true },
      { label: 'Reveal in Finder', action: () => window.fjord.showInFolder?.(item.path) },
      { label: 'Copy Path', action: () => navigator.clipboard.writeText(item.path) },
    ])
    return
  }

  const bookmarked = isBookmarked(item.path)
  showContextMenu(event.clientX, event.clientY, [
    { label: 'Open', action: () => openFile(item) },
    ...(state.workspaceMode === 'dual'
      ? [{ label: 'Open in Other Pane', action: () => {
          const previous = state.focusedPane
          state.focusedPane = previous === 'primary' ? 'secondary' : 'primary'
          openFile(item)
        } }]
      : []),
    { separator: true },
    { label: 'Rename', action: () => renameTreeItem(item) },
    { label: 'Duplicate', action: async () => {
      const result = await window.fjord.duplicateFile?.(item.path)
      if (result) {
        await refreshTree()
        await tryAwaitRebuildLinkIndex()
      }
    }},
    { label: 'Delete', action: () => deleteTreeItem(item) },
    { separator: true },
    { label: bookmarked ? 'Remove Bookmark' : 'Bookmark', action: () => toggleBookmark(item.path, item.name) },
    { label: 'Copy Path', action: () => navigator.clipboard.writeText(item.path) },
    { label: 'Reveal in Finder', action: () => window.fjord.showInFolder?.(item.path) },
  ])
}

async function createFileInFolder(folderPath) {
  const name = await promptTreeText({
    title: 'New File',
    label: 'File name',
    submitLabel: 'Create',
    placeholder: 'untitled',
  })
  if (!name) return
  const sanitized = stripMarkdownExtension(sanitizeTreeName(name))
  if (!sanitized) return
  const filePath = `${folderPath}/${sanitized}.md`
  try {
    const ok = await window.fjord.writeFile(filePath, '')
    if (!ok) {
      showStatusNotice('Could not create file. Check the folder permissions.', 'error')
      return
    }
    state.expandedFolders.add(folderPath)
    await refreshTree()
    await tryAwaitRebuildLinkIndex()
    await openFile({ path: filePath, name: `${sanitized}.md`, type: 'file' })
  } catch {
    showStatusNotice('Could not create file. Check the folder permissions.', 'error')
  }
}

async function createFolderInFolder(folderPath) {
  const name = await promptTreeText({
    title: 'New Folder',
    label: 'Folder name',
    submitLabel: 'Create',
    placeholder: 'untitled',
  })
  if (!name) return
  const sanitized = sanitizeTreeName(name)
  if (!sanitized) return
  const dirPath = `${folderPath}/${sanitized}`
  try {
    const ok = await window.fjord.createDir?.(dirPath)
    if (!ok) {
      showStatusNotice('Could not create folder. Check the folder permissions.', 'error')
      return
    }
    state.expandedFolders.add(folderPath)
    await refreshTree()
  } catch {
    showStatusNotice('Could not create folder. Check the folder permissions.', 'error')
  }
}

async function renameTreeItem(item) {
  const currentName = item.name
  const next = await promptTreeText({
    title: item.type === 'folder' ? 'Rename Folder' : 'Rename File',
    label: 'Name',
    submitLabel: 'Rename',
    value: currentName,
  })
  if (!next || next === currentName) return
  const sanitized = sanitizeTreeName(next)
  if (!sanitized || sanitized === currentName) return
  const parent = item.path.replace(/[/\\][^/\\]+$/, '')
  const newPath = `${parent}/${sanitized}`
  try {
    const ok = await window.fjord.renameFile?.(item.path, newPath)
    if (!ok) {
      showStatusNotice(`Could not rename ${item.type}. Check the folder permissions.`, 'error')
      return
    }
    // Update any open tabs
    for (const tab of state.tabs) {
      if (tab.path === item.path) {
        tab.path = newPath
        tab.name = sanitized
      }
    }
    if (window.fjord.setRepresentedFile && state.activeTab) {
      window.fjord.setRepresentedFile(state.activeTab.path)
    }
    await refreshTree()
    renderTabs()
    await tryAwaitRebuildLinkIndex()
  } catch {
    showStatusNotice(`Could not rename ${item.type}. Check the folder permissions.`, 'error')
  }
}

function sanitizeTreeName(name) {
  return String(name || '').trim().replace(/[\\/]+/g, '-')
}

function promptTreeText({ title, label, submitLabel, value = '', placeholder = '' }) {
  return new Promise(resolve => {
    _activeTreePrompt?.(null)

    const overlay = el('div', 'tree-prompt')
    overlay.innerHTML = `
      <div class="tree-prompt__overlay"></div>
      <form class="tree-prompt__panel">
        <div class="command-dialog__header">
          <div>
            <div class="command-dialog__eyebrow">Files</div>
            <div class="command-dialog__title"></div>
          </div>
        </div>
        <div class="command-dialog__body">
          <label class="command-field">
            <span class="command-field__label"></span>
            <input class="command-field__input" name="value" type="text" autocomplete="off" />
          </label>
        </div>
        <div class="command-dialog__footer">
          <button type="button" class="settings-btn settings-btn--muted" data-action="cancel">Cancel</button>
          <button type="submit" class="settings-btn"></button>
        </div>
      </form>
    `

    const panel = overlay.querySelector('.tree-prompt__panel')
    const input = overlay.querySelector('input')
    overlay.querySelector('.command-dialog__title').textContent = title
    overlay.querySelector('.command-field__label').textContent = label
    overlay.querySelector('button[type="submit"]').textContent = submitLabel
    input.value = value
    input.placeholder = placeholder

    const cleanup = (result) => {
      document.removeEventListener('keydown', onKeyDown)
      overlay.remove()
      if (_activeTreePrompt === cleanup) _activeTreePrompt = null
      resolve(result)
    }
    const onKeyDown = (event) => {
      if (event.key === 'Escape') cleanup(null)
    }

    overlay.querySelector('.tree-prompt__overlay').addEventListener('click', () => cleanup(null))
    overlay.querySelector('[data-action="cancel"]').addEventListener('click', () => cleanup(null))
    panel.addEventListener('submit', (event) => {
      event.preventDefault()
      cleanup(input.value.trim())
    })
    document.addEventListener('keydown', onKeyDown)
    document.body.appendChild(overlay)
    _activeTreePrompt = cleanup
    requestAnimationFrame(() => {
      input.focus()
      input.select()
    })
  })
}

async function deleteTreeItem(item) {
  const label = item.type === 'folder' ? 'folder' : 'file'
  if (!confirm(`Move ${label} "${item.name}" to trash?`)) return
  try {
    const ok = await window.fjord.trashFile?.(item.path)
    if (!ok) return
    // Close any tabs pointing at the deleted file or any descendants
    const prefixRe = new RegExp(`^${item.path.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}[/\\\\]`)
    for (const tab of [...state.tabs]) {
      if (tab.path === item.path || prefixRe.test(tab.path)) {
        closeTab(tab, getTabPane(tab) || 'primary')
      }
    }
    await refreshTree()
    await tryAwaitRebuildLinkIndex()
  } catch {}
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
  // The file explorer is now a widget; ask it to refresh its CTA/label/etc.
  refreshFileExplorerState()
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
  startDragResize(event, {
    min: 180,
    max: 420,
    initial: getSettings().sidebarWidth,
    bodyClass: 'is-resizing-sidebar',
    captureTarget: $('sidebar-resizer'),
    computeWidth: moveEvent => moveEvent.clientX,
    setWidth: px => document.documentElement.style.setProperty('--sidebar-width', `${px}px`),
    onCommit: px => updateSetting('sidebarWidth', px),
  })
}

// ── Open folder ──────────────────────────────────────────────────
export async function openFolder() {
  if (!window.fjord) return
  const p = await window.fjord.openFolder()
  if (!p) return
  state.folderPath = p
  state.singleFilePath = null
  resetBookmarksCache()
  addRecentProject(p)
  resetOpenDocuments()
  state.expandedFolders.clear()
  syncFolderUi()
  syncWorkspaceChrome()
  await window.fjord.watchFolder(p)
  await refreshTree()
  tryRebuildLinkIndex()
  showWelcomeScreen()

  schedulePeriodicSave(() => state.tabs)
  await tryRestoreSession()}

export async function openFolderPath(folderPath) {
  if (!window.fjord || !folderPath) return
  state.folderPath = folderPath
  state.singleFilePath = null
  resetBookmarksCache()
  addRecentProject(folderPath)
  resetOpenDocuments()
  state.expandedFolders.clear()
  syncFolderUi()
  syncWorkspaceChrome()
  await window.fjord.watchFolder(folderPath)
  await refreshTree()
  tryRebuildLinkIndex()
  showWelcomeScreen()

  await tryRestoreSession()}

export async function openSingleFilePath(filePath) {
  if (!window.fjord || !filePath) return
  state.folderPath = null
  state.singleFilePath = filePath
  state.tree = []
  state.tagFilter = null
  state.sidebarVisible = false
  state.rightPanel = null
  state.inspectorOpen = false
  resetBookmarksCache()
  resetOpenDocuments()
  state.expandedFolders.clear()
  syncFolderUi()
  syncWorkspaceChrome()
  await window.fjord.watchFolder?.(parentPath(filePath))
  const name = fileName(filePath)
  await openFile({ path: filePath, name })
  const sidebar = $('sidebar')
  if (sidebar) sidebar.classList.add('collapsed')
  $('sidebar-toggle')?.setAttribute('aria-pressed', 'false')
  _callbacks.syncWorkspaceUi?.()
  _callbacks.syncSplitLayout?.()
  renderTabs()
}

export async function createDailyNote(dateOverride) {
  if (!window.fjord || !state.folderPath) {
    showStatusNotice('Open a folder first', 'error')
    return
  }
  const settings = getSettings()
  const dailyFolder = settings.dailyNotesFolder || 'daily'
  const template = settings.dailyNoteTemplate || '# {{date}}\n\n'
  const date = dateOverride || new Date().toISOString().split('T')[0]
  const filename = `${date}.md`
  const dirPath = `${state.folderPath}/${dailyFolder}`
  const fullPath = `${dirPath}/${filename}`

  // Check if file already exists using stat
  const fileStat = await window.fjord.stat(fullPath)
  if (fileStat) {
    // File exists — just open it
    await openFile({ path: fullPath, name: filename })
    return
  }

  // Ensure directory exists, then create the file
  await window.fjord.createDir(dirPath)
  const content = template.replace(/\{\{date\}\}/g, date)
  await window.fjord.writeFile(fullPath, content)
  await refreshTree()
  await openFile({ path: fullPath, name: filename })
}

export async function createNewFile() {
  if (!window.fjord || !state.folderPath) {
    showStatusNotice('Open a folder first', 'error')
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
  const _t0 = performance.now()
  if (!$('workspace-primary')) _callbacks.buildEditorUI?.()
  const targetPane = state.workspaceMode === 'dual' ? state.focusedPane : 'primary'

  const existing = state.tabs.find(t => t.path === item.path)
  if (existing) {
    if (!preview && existing.preview) existing.preview = false
    activateTab(existing, targetPane)
    if (item.heading?.line) requestAnimationFrame(() => jumpToLine(item.heading.line))
    return
  }

  if (preview) {
    const group = getGroupTabs(targetPane)
    const existingPreview = group.find(t => t.preview)
    if (existingPreview) closeTab(existingPreview, targetPane)
  }

  const isAttachment = isAttachmentFile(item.path || '')
  const isSpatial = isSpatialFile(item.path || '')
  const isBinary = isLikelyBinaryFile(item.path || '')
  let content = ''
  if (!isAttachment && !isBinary) {
    content = await window.fjord.readFile(item.path)
  }
  const tab = { path: item.path, name: item.name, content, dirty: false, pinned: false, isAttachment, isSpatial, isBinary, preview }
  state.tabs.push(tab)
  activateTab(tab, targetPane)
  if (!isAttachment && !isBinary && item.heading?.line) requestAnimationFrame(() => jumpToLine(item.heading.line))
  // Perf budget (#61): file-switch latency ≤ 80 ms
  const elapsed = performance.now() - _t0
  if (elapsed > 80) console.warn(`[rista/perf] file-switch latency: ${Math.round(elapsed)}ms (budget 80ms)`)
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
  markFileSwitchStart()
  if (!$('workspace-primary')) _callbacks.buildEditorUI?.()
  if (pane === 'secondary' && state.workspaceMode !== 'dual') {
    state.workspaceMode = 'dual'
  }

  addTabToPane(tab, pane)
  setTabForPane(pane, tab)
  _callbacks.focusPane?.(pane)
  _callbacks.ensureEditorForPane?.(pane)

  if (tab.isAttachment || tab.isSpatial || tab.isBinary) {
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
  refreshRightPanel()
  if (window.fjord.setRepresentedFile) window.fjord.setRepresentedFile(tab.path)
  persistSession()
  markFileSwitchDone()
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
    // Defer overflow check until after layout
    requestAnimationFrame(() => updateTabOverflowIndicator(pane))
  })
}

function updateTabOverflowIndicator(pane) {
  const container = $(`tabs-${pane}`)
  const btn = $(`tabs-overflow-${pane}`)
  if (!container || !btn) return
  const group = getGroupTabs(pane)
  if (group.length === 0) { btn.hidden = true; return }
  const overflowing = container.scrollWidth > container.clientWidth + 1
  btn.hidden = !overflowing && group.length <= 1
}

export function showTabOverflowMenu(x, y, pane) {
  const group = getGroupTabs(pane)
  if (group.length === 0) return
  const sorted = [...group].sort((a, b) => (b.pinned ? 1 : 0) - (a.pinned ? 1 : 0))
  const activeTab = getTabForPane(pane)
  const items = sorted.map(tab => ({
    label: `${tab === activeTab ? '● ' : '   '}${tab.pinned ? '📌 ' : ''}${tab.name}${tab.dirty ? ' ·' : ''}`,
    action: () => activateTab(tab, pane),
  }))
  showContextMenu(x, y, items)
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
    { label: isBookmarked(tab.path) ? 'Remove Bookmark' : 'Bookmark', action: () => toggleBookmark(tab.path, tab.name) },
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
  scheduleMetricsUpdate()
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
  $('welcome-new-file-btn')?.addEventListener('click', createNewFile)
  attachWelcomeProjectHandlers($('welcome'), {
    openProject: (folderPath) => openFolderPath(folderPath),
    openProjectNewWindow: (folderPath) => window.fjord?.newWindow?.(folderPath),
    refreshWelcome: showWelcomeScreen,
  })
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
    markdown = getSettings().hideFrontmatterInRenderedModes
      ? mergeFrontmatterWithBody(tab.content, editor.getMarkdown())
      : editor.getMarkdown()
  } catch (err) {
    console.error(`[rista] Failed to read WYSIWYG content for pane "${pane}":`, err)
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
  scheduleMetricsUpdate()
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
    refreshRightPanel()
    saveSnapshot(tab.path, tab.content).catch(() => {})
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
  if (command === 'file:new-window') window.fjord?.newWindow?.()
  if (command === 'file:open-folder-new-window') window.fjord?.openFolderInNewWindow?.()
  if (command === 'file:daily-note') createDailyNote()
  if (command === 'file:new') createNewFile()
  if (command === 'file:open-folder') openFolder()
  if (command === 'file:save') saveActive()
  if (command === 'file:save-as') saveActiveAs()
  if (command === 'file:export-pdf') exportToPdf()
  if (command === 'file:export-website' && (getSettings().featurePublish || getSettings().showExperimental)) exportAsWebsite()
  if (command === 'file:close-window') window.fjord?.windowAction?.('close')
  if (command === 'file:close-tab' && getFocusedTab()) closeTab(getFocusedTab())
  if (command === 'view:toggle-sidebar') _callbacks.toggleSidebar?.()
  if (command === 'view:toggle-toolbar') _callbacks.toggleToolbar?.()
  if (command === 'view:toggle-inspector') _callbacks.toggleInspector?.()
  if (command === 'view:toggle-terminal') _callbacks.toggleTerminal?.()
  if (command === 'view:toggle-theme') _callbacks.toggleTheme?.()
  if (command === 'view:toggle-zen') _callbacks.toggleZen?.()
  if (command === 'view:toggle-graph') _callbacks.toggleRightPanel?.('graph')
  if (command === 'view:toggle-calendar') _callbacks.toggleRightPanel?.('calendar')
  if (command === 'view:quick-open') _callbacks.openQuickOpen?.()
  if (command === 'view:settings') _callbacks.toggleSettings?.()
  if (command === 'file:open' && data?.path) {
    const name = fileName(data.path)
    if (state.folderPath) openFile({ path: data.path, name })
    else openSingleFilePath(data.path)
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
  if (state.singleFilePath && changedPath !== state.singleFilePath) return
  if (state.folderPath && !changedPath.startsWith(state.folderPath)) return

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

  // Task 2.4: only rebuild the tree for structural events (add/delete/rename),
  // not for content-only changes which don't affect the file listing.
  if (state.folderPath && event !== 'change') {
    scheduleTreeRefresh()
  }
}

// ── Tab strip overflow handling ─────────────────────────────────
// Wheel: convert vertical scroll → horizontal so trackpads/mice can navigate
// the tab strip. Overflow chevron opens a dropdown of all open tabs.
document.addEventListener('click', (event) => {
  const overflowBtn = event.target.closest('[data-action="tab-overflow"]')
  if (overflowBtn) {
    const rect = overflowBtn.getBoundingClientRect()
    showTabOverflowMenu(rect.right, rect.bottom + 2, overflowBtn.dataset.pane || 'primary')
  }
})

document.addEventListener('wheel', (event) => {
  const strip = event.target.closest('.workspace-tabs')
  if (!strip) return
  if (event.deltaY === 0) return
  // Only translate when there's actually horizontal overflow
  if (strip.scrollWidth <= strip.clientWidth + 1) return
  strip.scrollLeft += event.deltaY
  event.preventDefault()
}, { passive: false })

// Re-check overflow when the window or panes resize
if (typeof ResizeObserver !== 'undefined') {
  const ro = new ResizeObserver(() => {
    for (const pane of PANE_KEYS) {
      const container = $(`tabs-${pane}`)
      const btn = $(`tabs-overflow-${pane}`)
      if (!container || !btn) continue
      const overflowing = container.scrollWidth > container.clientWidth + 1
      const empty = container.classList.contains('is-empty')
      btn.hidden = empty || (!overflowing && getGroupTabs(pane).length <= 1)
    }
  })
  // Observe the window once DOM is ready
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => ro.observe(document.body))
  } else {
    ro.observe(document.body)
  }
}
