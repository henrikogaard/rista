import './tauri-api.js'
import { markLaunchStart, markLaunchDone } from './perf-budget.js'
import { initTheme } from './theme.js'
import { applySettings, getSettings } from './settings.js'
import { initKeybindings } from './keybindings.js'
import { initDiagrams } from './diagrams.js'
import { getTheme, toggleTheme } from './theme.js'
import { state } from './state.js'
import { getFocusedTab } from './state.js'
import { setDocumentGoal, setSessionGoal } from './word-goals.js'
import { getStats, setTransclusionResolver } from './markdown.js'
import { registerEnsureRichEditorMounted, registerFocusPane } from './commands.js'
import { toggleFindReplace } from './find-replace.js'
import { registerCommandPaletteCallbacks, registerCommands, openCommandPaletteFiles } from './command-palette.js'
import { toggleZenMode, buildZenExitHint } from './zen-mode.js'
import { exportToHtml } from './preview.js'
import { exportToDocx } from './export-docx.js'
import { openSearchPanel } from './search-panel.js'
import { registerWikilinkCallback } from './preview.js'
import { renderAttachmentPreview, registerAttachmentPreviewCallbacks } from './attachment-preview.js'
import { toggleTerminalDrawer, handleTerminalInput } from './terminal-drawer.js'
import { openGraphModal } from './graph-modal.js'
import { getLinkIndex, resolveWikilink } from './link-index.js'
import { initRightSidebarWidth, restoreRightPanel } from './right-panel.js'
import { mountBookmarksPanel } from './bookmarks-view.js'
import { openDiagramBuilder } from './diagram-builder.js'
import { toggleRightPanel, toggleRightSidebar } from './right-panel.js'
import { ensureFirstRunSample } from './first-run.js'
import { getAvailableTemplates, resolveTemplateVars } from './templates.js'
import { openAiSession } from './ai-chat.js'
import { mountAssistantRail, openAssistantWidgetForDock } from './assistant-rail.js'
import { executeToolByName } from './ai-tools.js'
import { registerAiReviewCallbacks } from './ai-review.js'

// ── Shell (HTML + settings panel) ────────────────────────────────
import { buildShell, registerShellCallbacks, toggleSettingsPanel, toggleAppTheme, applySelectedAppIcon, syncSettingsForm } from './shell.js'

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
import { initKeyboardShortcuts, initAutoHideChrome } from './keyboard.js'
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
  showStatusNotice,
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
_featureEnabled('featureDiagramBuilder') && initDiagrams(getTheme())

// ── Cross-module callback registration ───────────────────────────
registerEnsureRichEditorMounted(ensureRichEditorMounted)
registerFocusPane(focusPane)

function _featureEnabled(key) { return getSettings()[key] || getSettings().showExperimental; }

// ── Panel registrations (extracted to panels.js) ─────────────────
import { initPanels } from './panels.js'
initPanels({
  openFile,
  refreshTree,
  openAiSession,
  openAiChatSurface,
  createDailyNote,
  collapseAllFolders,
  openFolderPath,
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
markLaunchStart()
buildShell()
syncSettingsForm()
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
      ensureFirstRunSample(openSingleFilePath)
    })
    .catch(() => {})
}

// Perf budget check: warn in console if shell construction exceeded budget.
requestAnimationFrame(() => {
  markLaunchDone()
})
initKeyboardShortcuts()
initAutoHideChrome()

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
    if (!template) { showStatusNotice('Template not found', 'error'); return }
    if (!state.folderPath) { showStatusNotice('Open a folder first', 'error'); return }
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
    if (!state.folderPath) { showStatusNotice('Open a folder first', 'error'); return }
    const title = prompt('Title:') || 'Untitled'
    const body = prompt('Body (Markdown):') || ''
    const sourceUrl = prompt('Source URL:') || ''
    const result = await window.fjord.importContent({ folderPath: state.folderPath, title, body, sourceUrl })
    if (result.error) { showStatusNotice('Import failed: ' + result.error, 'error'); return }
    await refreshTree()
    await openFile({ path: result.path, name: result.name })
  }},
  { id: 'new-canvas', label: 'New Canvas', description: 'Create a spatial canvas document', shortcut: '', action: async () => {
    if (!state.folderPath) { showStatusNotice('Open a folder first', 'error'); return }
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
    if (!state.folderPath) { showStatusNotice('Open a folder first', 'error'); return }
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

