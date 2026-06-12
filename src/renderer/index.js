import './tauri-api.js'
import { initTheme } from './theme.js'
import { applySettings, getSettings } from './settings.js'
import { initKeybindings, matchesBinding } from './keybindings.js'
import { initDiagrams } from './diagrams.js'
import { getTheme, toggleTheme } from './theme.js'
import { state } from './state.js'
import { getFocusedTab } from './state.js'
import { setDocumentGoal, setSessionGoal } from './word-goals.js'
import { getStats, setTransclusionResolver } from './markdown.js'
import { registerEnsureRichEditorMounted, registerFocusPane, closeCommandDialog } from './commands.js'
import { toggleFindReplace } from './find-replace.js'
import { registerCommandPaletteCallbacks, registerCommands, toggleCommandPalette, closeCommandPalette as closePalette, openCommandPaletteFiles, openCommandPaletteCommands } from './command-palette.js'
import { toggleZenMode, exitZenMode, buildZenExitHint } from './zen-mode.js'
import { exportToHtml } from './preview.js'
import { exportToDocx } from './export-docx.js'
import { toggleSearchPanel, openSearchPanel } from './search-panel.js'
import { registerWikilinkCallback } from './preview.js'
import { renderAttachmentPreview, registerAttachmentPreviewCallbacks } from './attachment-preview.js'
import { toggleTerminalDrawer, handleTerminalInput } from './terminal-drawer.js'
import { openGraphModal } from './graph-modal.js'
import { buildGraphView, renderGraph, destroyGraph, setGraphLocalMode, getGraphLocalMode } from './graph-view.js'
import { buildCalendarPanel, refreshCalendarPanel } from './calendar-view.js'
import { getLinkIndex, resolveWikilink, onLinkIndexChange } from './link-index.js'
import { registerRightPanel, initRightSidebarWidth, restoreRightPanel } from './right-panel.js'
import { graphIcon, calendarIcon, outlineIcon, bookmarkIcon, propertiesIcon, folderIcon, agentsIcon, tagIcon, wikiQualityIcon, relatedNotesIcon } from './icons.js'
import { buildFileExplorerPanel, mountFileExplorerPanel, refreshFileExplorerState, registerFileExplorerCallbacks, fileExplorerHeaderActions } from './file-explorer-view.js'
import { buildAgentsPanel, mountAgentsPanel, refreshAgentsPanel, registerAgentsViewCallbacks, agentsViewHeaderActions } from './agents-view.js'
import { buildOutlinePanel, mountOutlinePanel, renderOutline } from './outline-view.js'
import { buildTagsPanel, mountTagsPanel, renderTagsPanel, handleTagsPanelEvent } from './tags-view.js'
import { buildWikiQualityPanel, mountWikiQualityPanel, renderWikiQualityPanel, handleWikiQualityPanelEvent } from './wiki-quality-view.js'
import { buildRelatedNotesPanel, mountRelatedNotesPanel, renderRelatedNotesPanel, handleRelatedNotesPanelEvent } from './related-notes-view.js'
import { buildBookmarksPanel, mountBookmarksPanel, unmountBookmarksPanel, renderBookmarks, setBookmarksOpenFile } from './bookmarks-view.js'
import { buildPropertiesPanel, mountPropertiesPanel, renderProperties } from './properties-view.js'
import { openDiagramBuilder, closeDiagramBuilder } from './diagram-builder.js'
import { createSession } from './agents-sidebar.js'
import { toggleRightPanel, closeRightPanel, toggleRightSidebar } from './right-panel.js'
import { initInspectorPanel } from './inspector.js'
import { initAiChatPanel, openAiSession } from './ai-chat.js'
import { mountAssistantRail, openAssistantWidgetForDock } from './assistant-rail.js'
import { executeToolByName } from './ai-tools.js'
import { registerAiReviewCallbacks } from './ai-review.js'

// ── Shell (HTML + settings panel) ────────────────────────────────
import { buildShell, registerShellCallbacks, toggleSettingsPanel, closeSettingsPanel, toggleAppTheme, applySelectedAppIcon } from './shell.js'

function getInitialFolderPath() {
  const injected = window.__RISTA_INITIAL_FOLDER__
  return typeof injected === 'string' && injected ? injected : null
}

function getInitialFilePath() {
  const injected = window.__RISTA_INITIAL_FILE__
  return typeof injected === 'string' && injected ? injected : null
}

// ── Workspace (pane layout, editor mounting, toggles) ────────────
import {
  registerWorkspaceCallbacks,
  ensureRichEditorMounted,
  destroyRichEditor,
  focusPane,
  buildEditorUI,
  destroyEditors,
  ensureEditorForPane,
  syncWorkspaceUi,
  syncSplitLayout,
  syncFocusedPaneUi,
  refreshAllPreviews,
  maybeRefreshWysiwygPane,
  mountEditor,
  syncToolbarToggle,
  toggleToolbar,
  togglePaneSplitView,
  toggleSidebar,
  toggleWorkspaceSplit,
  toggleInspector,
  refreshRightPanel,
  applyEditorSettings,
} from './workspace.js'

// ── Tabs (file/tab operations, editor changes, saves) ────────────
import {
  registerTabCallbacks,
  openFolder,
  openFolderPath,
  openSingleFilePath,
  createNewFile,
  openFile,
  loadFileIntoTab,
  activateTab,
  renderTabs,
  closeTab,
  showWelcomeScreen,
  onEditorChange,
  onRichEditorChange,
  highlightActiveFile,
  refreshTree,
  syncFolderUi,
  collapseAllFolders,
  startSidebarResize,
  handleTabDragOver,
  handleTabDragLeave,
  handleTabDrop,
  saveActive,
  saveActiveAs,
  handleAppCommand,
  handleExternalFileChange,
  createDailyNote,
} from './tabs.js'

function openAiChatSurface() {
  const dock = getSettings().assistantDock
  if (dock === 'right-rail' || dock === 'left-rail') {
    mountAssistantRail()
    return
  }
  openAssistantWidgetForDock(dock)
}

// ── Init theme before any paint ──────────────────────────────────
initTheme()
applySettings()
applySelectedAppIcon()
initRightSidebarWidth()
initKeybindings()
initDiagrams(getTheme())

// ── Cross-module callback registration ───────────────────────────
registerEnsureRichEditorMounted(ensureRichEditorMounted)
registerFocusPane(focusPane)

// ── Initialize widget system (right + left sidebars) ─────────────
// File explorer and Agents default to the left sidebar.
registerFileExplorerCallbacks({
  openFolder,
  collapseAllFolders,
  renderTree: () => { refreshTree() },
})
registerRightPanel('files', {
  title: 'Files',
  icon: folderIcon(),
  flex: 3,
  defaultSide: 'left',
  defaultActive: true,
  build: buildFileExplorerPanel,
  headerActions: fileExplorerHeaderActions,
  onMount: mountFileExplorerPanel,
  onUnmount: () => {},
  onRefresh: refreshFileExplorerState,
})

registerAgentsViewCallbacks({
  createSession: async () => {
    const session = await createSession()
    if (session) refreshAgentsPanel()
  },
  openSession: (sessionPath) => {
    openAiChatSurface()
    openAiSession(sessionPath)
  },
})
registerRightPanel('agents', {
  title: 'Agents',
  icon: agentsIcon(),
  flex: 1,
  defaultSide: 'left',
  build: buildAgentsPanel,
  headerActions: agentsViewHeaderActions,
  onMount: mountAgentsPanel,
  onUnmount: () => {},
  onRefresh: refreshAgentsPanel,
})

initInspectorPanel(openFile, closeRightPanel)

let _graphUnsubscribe = null
function rerenderGraphFromIndex() {
  setGraphLocalMode(getGraphLocalMode(), getFocusedTab()?.path || null)
  renderGraph(getLinkIndex(), (path) => openFile({ path, name: path.split('/').pop() }))
}
registerRightPanel('graph', {
  title: 'Graph',
  icon: graphIcon(),
  flex: 2,
  build: () => `<div id="graph-panel-body" class="widget-fill">${buildGraphView()}</div>`,
  onMount: () => {
    rerenderGraphFromIndex()
    // Re-draw whenever the link index rebuilds (folder open, file edited,
    // file moved/created/deleted by the AI agent, etc.)
    _graphUnsubscribe?.()
    _graphUnsubscribe = onLinkIndexChange(() => rerenderGraphFromIndex())
  },
  onUnmount: () => {
    _graphUnsubscribe?.()
    _graphUnsubscribe = null
    destroyGraph()
  },
  onRefresh: () => {
    // When the focused tab changes, push the new path so local mode follows it
    if (getGraphLocalMode()) {
      setGraphLocalMode(true, getFocusedTab()?.path || null)
    }
  },
})

initAiChatPanel(openFile, closeRightPanel)

registerRightPanel('properties', {
  title: 'Properties',
  icon: propertiesIcon(),
  flex: 1,
  defaultSide: 'left',
  defaultActive: true,
  build: buildPropertiesPanel,
  onMount: mountPropertiesPanel,
  onUnmount: () => {},
  onRefresh: renderProperties,
})

registerRightPanel('outline', {
  title: 'Outline',
  icon: outlineIcon(),
  flex: 1,
  build: buildOutlinePanel,
  onMount: mountOutlinePanel,
  onUnmount: () => {},
  onRefresh: renderOutline,
})

let _tagsUnsubscribe = null
registerRightPanel('tags', {
  title: 'Tags',
  icon: tagIcon(),
  flex: 1,
  defaultSide: 'left',
  build: buildTagsPanel,
  onMount: () => {
    mountTagsPanel(openFile, (tag) => {
      state.tagFilter = tag
      refreshTree()
    }, () => {
      state.tagFilter = null
      refreshTree()
    })
    const body = document.getElementById('tags-view-body')
    body?.addEventListener('click', handleTagsPanelEvent)
    body?.addEventListener('keydown', handleTagsPanelEvent)
    _tagsUnsubscribe?.()
    _tagsUnsubscribe = onLinkIndexChange(() => renderTagsPanel())
  },
  onUnmount: () => {
    const body = document.getElementById('tags-view-body')
    body?.removeEventListener('click', handleTagsPanelEvent)
    body?.removeEventListener('keydown', handleTagsPanelEvent)
    _tagsUnsubscribe?.()
    _tagsUnsubscribe = null
  },
  onRefresh: renderTagsPanel,
})

let _relatedNotesUnsubscribe = null
registerRightPanel('related-notes', {
  title: 'Related',
  icon: relatedNotesIcon(),
  flex: 1,
  build: buildRelatedNotesPanel,
  onMount: () => {
    mountRelatedNotesPanel(openFile)
    const body = document.getElementById('related-notes-body')
    body?.addEventListener('click', handleRelatedNotesPanelEvent)
    body?.addEventListener('keydown', handleRelatedNotesPanelEvent)
    _relatedNotesUnsubscribe?.()
    _relatedNotesUnsubscribe = onLinkIndexChange(() => renderRelatedNotesPanel())
  },
  onUnmount: () => {
    const body = document.getElementById('related-notes-body')
    body?.removeEventListener('click', handleRelatedNotesPanelEvent)
    body?.removeEventListener('keydown', handleRelatedNotesPanelEvent)
    _relatedNotesUnsubscribe?.()
    _relatedNotesUnsubscribe = null
  },
  onRefresh: renderRelatedNotesPanel,
})

let _wikiQualityUnsubscribe = null
registerRightPanel('wiki-quality', {
  title: 'Wiki',
  icon: wikiQualityIcon(),
  flex: 1,
  build: buildWikiQualityPanel,
  onMount: () => {
    mountWikiQualityPanel(openFile, { refreshTree })
    const body = document.getElementById('wiki-quality-body')
    body?.addEventListener('click', handleWikiQualityPanelEvent)
    body?.addEventListener('keydown', handleWikiQualityPanelEvent)
    _wikiQualityUnsubscribe?.()
    _wikiQualityUnsubscribe = onLinkIndexChange(() => renderWikiQualityPanel())
  },
  onUnmount: () => {
    const body = document.getElementById('wiki-quality-body')
    body?.removeEventListener('click', handleWikiQualityPanelEvent)
    body?.removeEventListener('keydown', handleWikiQualityPanelEvent)
    _wikiQualityUnsubscribe?.()
    _wikiQualityUnsubscribe = null
  },
  onRefresh: renderWikiQualityPanel,
})

setBookmarksOpenFile((item) => openFile(item))
registerRightPanel('bookmarks', {
  title: 'Bookmarks',
  icon: bookmarkIcon(),
  flex: 1,
  build: buildBookmarksPanel,
  onMount: mountBookmarksPanel,
  onUnmount: unmountBookmarksPanel,
  onRefresh: renderBookmarks,
})

registerRightPanel('calendar', {
  title: 'Calendar',
  icon: calendarIcon(),
  flex: 0,
  build: () => `<div id="calendar-panel-body" class="widget-fill"></div>`,
  onMount: () => {
    const body = document.getElementById('calendar-panel-body')
    if (body && !body.firstChild) {
      const panel = buildCalendarPanel({ onDateClick: (dateStr) => createDailyNote(dateStr), folderPath: state.folderPath })
      body.appendChild(panel)
    }
  },
  onUnmount: () => { document.getElementById('calendar-panel-body')?.replaceChildren() },
})

// ── Transclusion resolver ───────────────────────────────────────
setTransclusionResolver((noteName) => {
  const index = getLinkIndex()
  const targetPath = resolveWikilink(noteName, index.allPaths, state.folderPath)
  if (!targetPath) return null
  return index.files.get(targetPath)?.content ?? null
})

registerShellCallbacks({
  toggleToolbar,
  togglePaneSplitView,
  toggleWorkspaceSplit,
  toggleSidebar,
  toggleTerminal: toggleTerminalDrawer,
  toggleInspector,
  toggleRightPanel,
  toggleRightSidebar,
  openFolder,
  createNewFile,
  openRecentProject: (folderPath) => openFolderPath(folderPath),
  openWorkspaceInNewWindow: (folderPath) => window.fjord?.newWindow?.(folderPath),
  collapseAllFolders,
  startSidebarResize,
  handleAppCommand,
  loadFileIntoTab,
  refreshTree,
  refreshAllPreviews,
  handleExternalFileChange,
  destroyRichEditor,
  ensureRichEditorMounted,
  syncToolbarToggle,
  syncAssistantRail: mountAssistantRail,
  applyEditorSettings,
  openAgentSession: (sessionPath) => {
    openAiChatSurface()
    openAiSession(sessionPath)
  },
})

registerAiReviewCallbacks({
  executeTool: executeToolByName,
  openFile,
})

registerAttachmentPreviewCallbacks({
  openFilePath: (path) => openFile({ path, name: path.split(/[\\/]/).pop() }),
})

registerWorkspaceCallbacks({
  openFolder,
  renderTabs,
  activateTab,
  closeTab,
  showWelcomeScreen,
  onEditorChange,
  onRichEditorChange,
  highlightActiveFile,
  openFile,
  handleTabDragOver,
  handleTabDragLeave,
  handleTabDrop,
  renderAttachmentPreview,
})

registerWikilinkCallback((path) => openFile({ path, name: path.split(/[\\/]/).pop() }))

registerTabCallbacks({
  buildEditorUI,
  destroyEditors,
  ensureEditorForPane,
  syncWorkspaceUi,
  syncSplitLayout,
  syncFocusedPaneUi,
  refreshAllPreviews,
  maybeRefreshWysiwygPane,
  mountEditor,
  destroyRichEditor,
  focusPane,
  toggleSidebar,
  toggleToolbar,
  toggleInspector,
  toggleTerminal: toggleTerminalDrawer,
  toggleTheme: toggleAppTheme,
  toggleZen: toggleZenMode,
  toggleSettings: toggleSettingsPanel,
  toggleRightPanel,
  openQuickOpen: openCommandPaletteFiles,
})

// ── Templates ────────────────────────────────────────────────────
const BUILT_IN_TEMPLATES = [
  { name: 'Blog Post', content: '# {{title}}\n\n*{{date}}*\n\n' },
  { name: 'Meeting Notes', content: '# Meeting Notes — {{date}}\n\n## Attendees\n\n- \n\n## Agenda\n\n1. \n\n## Notes\n\n\n\n## Action Items\n\n- [ ] ' },
  { name: 'Daily Note', content: '# {{date}}\n\n## Tasks\n\n- [ ] \n\n## Notes\n\n' },
  { name: 'README', content: '# Project Name\n\n## Description\n\n\n\n## Installation\n\n```bash\nnpm install\n```\n\n## Usage\n\n## License\n\nMIT' },
  { name: 'Changelog', content: '# Changelog\n\n## [Unreleased]\n\n### Added\n\n- \n\n### Changed\n\n### Fixed\n' },
]

async function getAvailableTemplates() {
  const builtIn = [...BUILT_IN_TEMPLATES]
  if (state.folderPath && window.fjord.readTemplates) {
    try {
      const userTemplates = await window.fjord.readTemplates(state.folderPath)
      return [...builtIn, ...userTemplates]
    } catch { /* ignore */ }
  }
  return builtIn
}

function resolveTemplateVars(content) {
  const now = new Date()
  const date = now.toISOString().split('T')[0]
  let resolved = content.replace(/\{\{date\}\}/g, date)
  if (resolved.includes('{{title}}')) {
    const title = prompt('Title:') || 'Untitled'
    resolved = resolved.replace(/\{\{title\}\}/g, title)
  }
  return resolved.replace(/\{\{cursor\}\}/g, '')
}

// ── First-run sample document ────────────────────────────────────
const FIRST_RUN_SAMPLE = `# Welcome to Rista ✦

A local-first Markdown editor. Your files, your folder — no cloud, no accounts.

## Getting started

Open a folder with **⌘O** to see all your notes in the sidebar.
Press **⌘K** to jump to any file or run a command.

## Writing shortcuts

| Action | Shortcut |
|---|---|
| Bold | ⌘B |
| Italic | ⌘I |
| Inline code | ⌘\` |
| Find & replace | ⌘F |
| Project search | ⇧⌘F |
| Zen mode | ⇧⌘↵ |

## Views

Toggle between **Edit**, **Split**, and **Preview** using the buttons in the toolbar.

## Tips

- Type \`---\` on its own line for a horizontal rule
- Type \`"quotes"\` and they become "smart quotes" automatically
- Use \`#tag\` anywhere in a note to build a tag index

---

_This file lives at \`~/Documents/Rista/welcome.md\`. Feel free to edit or delete it._
`

// ── Boot ─────────────────────────────────────────────────────────
// Perf budget (#61): cold launch ≤ 400 ms to first interactive frame.
// File-switch latency ≤ 80 ms p95. Log a warning if exceeded.
const _bootStart = performance.now()
buildShell()
mountAssistantRail()
buildZenExitHint()
// The terminal drawer lives in the persistent shell now, not the editor UI —
// wire its input listener once.
handleTerminalInput()
// Activate default left-sidebar widgets (Files) before the editor UI exists.
restoreRightPanel()
syncFolderUi()
const initialFolderPath = getInitialFolderPath()
const initialFilePath = getInitialFilePath()
if (initialFilePath) {
  openSingleFilePath(initialFilePath)
} else if (initialFolderPath) {
  openFolderPath(initialFolderPath)
} else {
  window.fjord?.launchFile?.()
    .then(async filePath => {
      if (filePath) {
        openSingleFilePath(filePath)
        return
      }
      // First-run: create a sample welcome document so new users aren't dropped
      // into a blank screen. Only runs once (guarded by localStorage flag).
      if (window.fjord && !localStorage.getItem('rista-onboarded')) {
        localStorage.setItem('rista-onboarded', '1')
        try {
          const home = await window.fjord.getHomeDir?.()
          if (home) {
            const dir = `${home}/Documents/Rista`
            const samplePath = `${dir}/welcome.md`
            await window.fjord.createDir(dir).catch(() => {})
            const existing = await window.fjord.stat(samplePath).catch(() => null)
            if (!existing) {
              await window.fjord.writeFile(samplePath, FIRST_RUN_SAMPLE)
            }
            await openSingleFilePath(samplePath)
          }
        } catch (_) { /* non-fatal — user just sees empty state */ }
      }
    })
    .catch(() => {})
}

// Perf budget check: warn in console if shell construction exceeded budget.
requestAnimationFrame(() => {
  const elapsed = performance.now() - _bootStart
  const BUDGET_MS = 400
  if (elapsed > BUDGET_MS) {
    console.warn(`[rista/perf] Cold launch exceeded budget: ${Math.round(elapsed)}ms > ${BUDGET_MS}ms`)
  }
})

// ── Command palette ──────────────────────────────────────────────
registerCommandPaletteCallbacks({ openFile })

registerCommands([
  { id: 'toggle-sidebar',  label: 'Toggle Sidebar',       description: 'Show or hide the sidebar',       shortcut: '\u2318B',   action: () => toggleSidebar() },
  { id: 'toggle-toolbar',  label: 'Toggle Toolbar',       description: 'Show or hide the toolbar',       shortcut: '\u2318\\',  action: () => toggleToolbar() },
  { id: 'toggle-theme',    label: 'Toggle Theme',         description: 'Switch between dark and light',  shortcut: '',          action: () => toggleAppTheme() },
  { id: 'new-file',        label: 'New File',             description: 'Create a new markdown file',     shortcut: '\u2318N',   action: () => createNewFile() },
  { id: 'open-folder',     label: 'Open Folder',          description: 'Open a project folder',          shortcut: '',          action: () => openFolder() },
  { id: 'find-replace',    label: 'Find & Replace',       description: 'Search within the editor',       shortcut: '\u2318F',   action: () => toggleFindReplace() },
  { id: 'project-search',  label: 'Project Search',       description: 'Search across all project files', shortcut: '\u21e7\u2318F', action: () => openSearchPanel() },
  { id: 'save',            label: 'Save',                 description: 'Save the active file',           shortcut: '\u2318S',   action: () => saveActive() },
  { id: 'settings',        label: 'Settings',             description: 'Open settings panel',            shortcut: '\u2318,',   action: () => toggleSettingsPanel() },
  { id: 'toggle-zen',      label: 'Toggle Zen Mode',      description: 'Distraction-free writing',       shortcut: '\u21e7\u2318\u23ce', action: () => toggleZenMode() },
  { id: 'set-doc-goal', label: 'Set Document Word Goal', description: 'Set a word count target for this document', shortcut: '', action: () => {
    const target = prompt('Target word count:')
    if (target) setDocumentGoal(getFocusedTab()?.path, parseInt(target, 10))
  }},
  { id: 'set-session-goal', label: 'Set Session Word Goal', description: 'Set a word target for this session', shortcut: '', action: () => {
    const target = prompt('Session word target:')
    if (target) setSessionGoal(parseInt(target, 10), getStats(getFocusedTab()?.content || '').words)
  }},
  { id: 'clear-doc-goal', label: 'Clear Document Word Goal', description: 'Remove the word count target', shortcut: '', action: () => {
    setDocumentGoal(getFocusedTab()?.path, null)
  }},
  { id: 'export-html', label: 'Export to HTML', description: 'Save as standalone HTML file', shortcut: '', action: () => exportToHtml() },
  { id: 'export-docx', label: 'Export to DOCX', description: 'Save as Word document (experimental)', shortcut: '', when: () => getSettings().docxExportEnabled, action: () => exportToDocx() },
  { id: 'print', label: 'Print', description: 'Print the preview', shortcut: '', action: () => window.print() },
  { id: 'new-from-template', label: 'New File from Template', description: 'Create a file from a template', shortcut: '', action: async () => {
    const templates = await getAvailableTemplates()
    const names = templates.map(t => t.name)
    const choice = prompt('Choose template:\n' + names.map((n, i) => `${i + 1}. ${n}`).join('\n'))
    if (!choice) return
    const idx = parseInt(choice, 10) - 1
    const template = Number.isFinite(idx) && templates[idx] ? templates[idx] : templates.find(t => t.name.toLowerCase() === choice.toLowerCase())
    if (!template) { alert('Template not found'); return }
    if (!state.folderPath) { alert('Open a folder first'); return }
    const fileName = prompt('File name:', `${template.name.toLowerCase().replace(/\s+/g, '-')}.md`)
    if (!fileName) return
    const content = resolveTemplateVars(template.content)
    const fullPath = state.folderPath + '/' + fileName
    await window.fjord.createFile(fullPath)
    await window.fjord.writeFile(fullPath, content)
    await refreshTree()
    await openFile({ path: fullPath, name: fileName })
  }},
  { id: 'import-content', label: 'Import Content', description: 'Create a note from external content', shortcut: '', action: async () => {
    if (!state.folderPath) { alert('Open a folder first'); return }
    const title = prompt('Title:') || 'Untitled'
    const body = prompt('Body (Markdown):') || ''
    const sourceUrl = prompt('Source URL:') || ''
    const result = await window.fjord.importContent({ folderPath: state.folderPath, title, body, sourceUrl })
    if (result.error) { alert('Import failed: ' + result.error); return }
    await refreshTree()
    await openFile({ path: result.path, name: result.name })
  }},
  { id: 'new-canvas', label: 'New Canvas', description: 'Create a spatial canvas document', shortcut: '', action: async () => {
    if (!state.folderPath) { alert('Open a folder first'); return }
    const name = prompt('Canvas file name:', 'workspace.fcanvas.json')
    if (!name) return
    const fileName = name.endsWith('.fcanvas.json') ? name : `${name}.fcanvas.json`
    const fullPath = `${state.folderPath}/${fileName}`
    const content = JSON.stringify({ type: 'fjord-canvas', version: 1, cards: [], links: [] }, null, 2)
    await window.fjord.writeFile(fullPath, content)
    await refreshTree()
    await openFile({ path: fullPath, name: fileName })
  }},
  { id: 'new-drawing', label: 'New Drawing', description: 'Create an embedded drawing document', shortcut: '', action: async () => {
    if (!state.folderPath) { alert('Open a folder first'); return }
    const name = prompt('Drawing file name:', 'drawing.fdraw.json')
    if (!name) return
    const fileName = name.endsWith('.fdraw.json') ? name : `${name}.fdraw.json`
    const fullPath = `${state.folderPath}/${fileName}`
    const content = JSON.stringify({ type: 'fjord-drawing', version: 1, strokes: [] }, null, 2)
    await window.fjord.writeFile(fullPath, content)
    await refreshTree()
    await openFile({ path: fullPath, name: fileName })
  }},
  { id: 'show-graph', label: 'Show Knowledge Graph', description: 'Visualize note connections', shortcut: '', action: () => openGraphModal(openFile) },
  { id: 'insert-diagram', label: 'Insert Diagram', description: 'Open the visual diagram builder', shortcut: '', action: () => openDiagramBuilder() },
  { id: 'daily-note', label: 'Daily Note', description: 'Open or create today\'s daily note', shortcut: '⇧⌘D', action: () => createDailyNote() },
])

// ── Keyboard shortcuts ───────────────────────────────────────────
document.addEventListener('keydown', e => {
  // Escape handlers (not customizable)
  if (e.key === 'Escape') {
    if (state.zenMode) { e.preventDefault(); exitZenMode(); return }
    if (state.commandPaletteOpen) { e.preventDefault(); closePalette(); return }
    if (state.commandDialog) { e.preventDefault(); closeCommandDialog(); return }
    if (document.querySelector('.diagram-builder.open')) { e.preventDefault(); closeDiagramBuilder(); return }
    if (state.settingsOpen) { e.preventDefault(); closeSettingsPanel(); return }
  }

  // Customizable shortcuts via keybindings registry
  // When the CM editor is focused, let it handle formatting shortcuts (Mod-B/I/K etc.)
  // by skipping the conflicting app-level shortcuts here.
  const cmFocused = !!document.activeElement?.closest('.cm-editor')
  if (matchesBinding(e, 'zen-mode')) { e.preventDefault(); toggleZenMode(); return }
  if (matchesBinding(e, 'quick-open')) { e.preventDefault(); openCommandPaletteFiles(); return }
  if (matchesBinding(e, 'command-palette') && !cmFocused) { e.preventDefault(); openCommandPaletteCommands(); return }
  if (matchesBinding(e, 'save-as')) { e.preventDefault(); saveActiveAs(); return }
  if (matchesBinding(e, 'save')) { e.preventDefault(); saveActive(); return }
  if (matchesBinding(e, 'new-file')) { e.preventDefault(); createNewFile(); return }
  if (matchesBinding(e, 'toggle-sidebar') && !cmFocused) { e.preventDefault(); toggleSidebar(); return }
  if (matchesBinding(e, 'toggle-toolbar')) { e.preventDefault(); toggleToolbar(); return }
  if (matchesBinding(e, 'find-replace')) { e.preventDefault(); toggleFindReplace(); return }
  if (matchesBinding(e, 'project-search')) { e.preventDefault(); openSearchPanel(); return }
  if (matchesBinding(e, 'terminal')) { e.preventDefault(); toggleTerminalDrawer(); return }
  if (matchesBinding(e, 'settings')) { e.preventDefault(); toggleSettingsPanel(); return }
  if (matchesBinding(e, 'daily-note')) { e.preventDefault(); createDailyNote(); return }
})

// ── Auto-hide chrome while typing (#47) ─────────────────────────
// When the user is actively typing in the editor, fade out tabs and brandrail
// so the writing surface can breathe. Chrome reappears on mouse move or pause.
let _typingTimer = null
function _setTyping(active) {
  document.documentElement.dataset.typing = active ? 'true' : 'false'
}
document.addEventListener('keydown', e => {
  // Only trigger when a printable key is pressed and a CM editor is focused
  if (e.ctrlKey || e.metaKey || e.altKey) return
  const focused = document.activeElement
  if (!focused?.closest('.cm-editor')) return
  _setTyping(true)
  clearTimeout(_typingTimer)
  _typingTimer = setTimeout(() => _setTyping(false), 1800)
})
document.addEventListener('mousemove', () => {
  if (document.documentElement.dataset.typing === 'true') {
    clearTimeout(_typingTimer)
    _setTyping(false)
  }
})
