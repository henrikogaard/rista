import { initTheme } from './theme.js'
import { applySettings } from './settings.js'
import { initDiagrams } from './diagrams.js'
import { getTheme } from './theme.js'
import { state } from './state.js'
import { getFocusedTab } from './state.js'
import { registerEnsureRichEditorMounted, registerFocusPane, closeCommandDialog } from './commands.js'
import { toggleFindReplace } from './find-replace.js'

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
  toggleToolbar,
  toggleSidebar,
  toggleWorkspaceSplit,
} from './workspace.js'

// ── Tabs (file/tab operations, editor changes, saves) ────────────
import {
  registerTabCallbacks,
  openFolder,
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
} from './tabs.js'

// ── Init theme before any paint ──────────────────────────────────
initTheme()
applySettings()
initDiagrams(getTheme())

// ── Cross-module callback registration ───────────────────────────
registerEnsureRichEditorMounted(ensureRichEditorMounted)
registerFocusPane(focusPane)

registerShellCallbacks({
  toggleToolbar,
  toggleWorkspaceSplit,
  toggleSidebar,
  openFolder,
  collapseAllFolders,
  startSidebarResize,
  handleAppCommand,
  loadFileIntoTab,
  refreshTree,
  destroyRichEditor,
  ensureRichEditorMounted,
  syncToolbarToggle,
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
})

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
})

// ── Boot ─────────────────────────────────────────────────────────
buildShell()
syncFolderUi()

// ── Keyboard shortcuts ───────────────────────────────────────────
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
