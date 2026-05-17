import { initTheme } from './theme.js'
import { applySettings } from './settings.js'
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
import { renderAttachmentPreview } from './attachment-preview.js'
import { toggleTerminalDrawer } from './terminal-drawer.js'
import { openGraphModal } from './graph-modal.js'
import { buildGraphView, renderGraph, destroyGraph } from './graph-view.js'
import { buildCalendarPanel, refreshCalendarPanel } from './calendar-view.js'
import { getLinkIndex, resolveWikilink } from './link-index.js'
import { registerRightPanel, initRightSidebarWidth } from './right-panel.js'
import { graphIcon, calendarIcon } from './icons.js'
import { openDiagramBuilder, closeDiagramBuilder } from './diagram-builder.js'
import { toggleSidebarMode, createSession, renderAgentsList } from './agents-sidebar.js'
import { toggleRightPanel, closeRightPanel, toggleRightSidebar } from './right-panel.js'
import { initInspectorPanel } from './inspector.js'
import { initAiChatPanel } from './ai-chat.js'

// ── Shell (HTML + settings panel) ────────────────────────────────
import { buildShell, registerShellCallbacks, toggleSettingsPanel, closeSettingsPanel } from './shell.js'

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
  syncPaneSplitToggle,
  toggleToolbar,
  togglePaneSplitView,
  toggleSidebar,
  toggleWorkspaceSplit,
  toggleInspector,
  refreshRightPanel,
} from './workspace.js'

// ── Tabs (file/tab operations, editor changes, saves) ────────────
import {
  registerTabCallbacks,
  openFolder,
  openFolderPath,
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

// ── Init theme before any paint ──────────────────────────────────
initTheme()
applySettings()
initRightSidebarWidth()
initKeybindings()
initDiagrams(getTheme())

// ── Cross-module callback registration ───────────────────────────
registerEnsureRichEditorMounted(ensureRichEditorMounted)
registerFocusPane(focusPane)

// ── Initialize right panel system ────────────────────────────────
initInspectorPanel(openFile, closeRightPanel)

registerRightPanel('graph', {
  title: 'Graph',
  icon: graphIcon(),
  flex: 2,
  build: () => `<div id="graph-panel-body" class="widget-fill">${buildGraphView()}</div>`,
  onMount: () => {
    renderGraph(getLinkIndex(), (path) => openFile({ path, name: path.split('/').pop() }))
  },
  onUnmount: () => destroyGraph(),
})

initAiChatPanel(openFile, closeRightPanel)

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
  toggleSidebarMode,
  toggleTerminal: toggleTerminalDrawer,
  toggleInspector,
  toggleRightPanel,
  toggleRightSidebar,
  openFolder,
  openRecentProject: (folderPath) => openFolderPath(folderPath),
  collapseAllFolders,
  startSidebarResize,
  handleAppCommand,
  loadFileIntoTab,
  refreshTree,
  handleExternalFileChange,
  destroyRichEditor,
  ensureRichEditorMounted,
  syncToolbarToggle,
  syncPaneSplitToggle,
  createAgentSession: async () => {
    const session = await createSession()
    if (session) renderAgentsList()
  },
  openAgentSession: (sessionPath) => {
    // Open session JSON as a note for now (could be a special UI later)
    openFile({ path: sessionPath, name: sessionPath.split('/').pop() })
  },
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

registerWikilinkCallback(openFile)

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

// ── Boot ─────────────────────────────────────────────────────────
buildShell()
buildZenExitHint()
syncFolderUi()

// ── Command palette ──────────────────────────────────────────────
registerCommandPaletteCallbacks({ openFile })

registerCommands([
  { id: 'toggle-sidebar',  label: 'Toggle Sidebar',       description: 'Show or hide the sidebar',       shortcut: '\u2318B',   action: () => toggleSidebar() },
  { id: 'toggle-toolbar',  label: 'Toggle Toolbar',       description: 'Show or hide the toolbar',       shortcut: '\u2318\\',  action: () => toggleToolbar() },
  { id: 'toggle-theme',    label: 'Toggle Theme',         description: 'Switch between dark and light',  shortcut: '',          action: () => toggleTheme() },
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
  { id: 'export-docx', label: 'Export to DOCX', description: 'Save as Word document', shortcut: '', action: () => exportToDocx() },
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
  if (matchesBinding(e, 'zen-mode')) { e.preventDefault(); toggleZenMode(); return }
  if (matchesBinding(e, 'quick-open')) { e.preventDefault(); openCommandPaletteFiles(); return }
  if (matchesBinding(e, 'command-palette')) { e.preventDefault(); openCommandPaletteCommands(); return }
  if (matchesBinding(e, 'save-as')) { e.preventDefault(); saveActiveAs(); return }
  if (matchesBinding(e, 'save')) { e.preventDefault(); saveActive(); return }
  if (matchesBinding(e, 'new-file')) { e.preventDefault(); createNewFile(); return }
  if (matchesBinding(e, 'toggle-sidebar')) { e.preventDefault(); toggleSidebar(); return }
  if (matchesBinding(e, 'toggle-toolbar')) { e.preventDefault(); toggleToolbar(); return }
  if (matchesBinding(e, 'find-replace')) { e.preventDefault(); toggleFindReplace(); return }
  if (matchesBinding(e, 'project-search')) { e.preventDefault(); openSearchPanel(); return }
  if (matchesBinding(e, 'terminal')) { e.preventDefault(); toggleTerminalDrawer(); return }
  if (matchesBinding(e, 'settings')) { e.preventDefault(); toggleSettingsPanel(); return }
  if (matchesBinding(e, 'daily-note')) { e.preventDefault(); createDailyNote(); return }
})
