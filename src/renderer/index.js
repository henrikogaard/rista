import { initTheme } from './theme.js'
import { applySettings } from './settings.js'
import { initDiagrams } from './diagrams.js'
import { getTheme, toggleTheme } from './theme.js'
import { state } from './state.js'
import { getFocusedTab } from './state.js'
import { registerEnsureRichEditorMounted, registerFocusPane, closeCommandDialog } from './commands.js'
import { toggleFindReplace } from './find-replace.js'
import { registerCommandPaletteCallbacks, registerCommands, toggleCommandPalette, closeCommandPalette as closePalette } from './command-palette.js'
import { toggleZenMode, exitZenMode, buildZenExitHint } from './zen-mode.js'

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
  { id: 'save',            label: 'Save',                 description: 'Save the active file',           shortcut: '\u2318S',   action: () => saveActive() },
  { id: 'settings',        label: 'Settings',             description: 'Open settings panel',            shortcut: '\u2318,',   action: () => toggleSettingsPanel() },
  { id: 'toggle-zen',      label: 'Toggle Zen Mode',      description: 'Distraction-free writing',       shortcut: '\u21e7\u2318\u23ce', action: () => toggleZenMode() },
])

// ── Keyboard shortcuts ───────────────────────────────────────────
document.addEventListener('keydown', e => {
  const mod = e.metaKey || e.ctrlKey
  if (mod && e.shiftKey && e.key === 'Enter') { e.preventDefault(); toggleZenMode(); return }
  if (e.key === 'Escape' && state.zenMode) { e.preventDefault(); exitZenMode(); return }
  if (mod && e.key === 'k') { e.preventDefault(); toggleCommandPalette(); return }
  if (e.key === 'Escape' && state.commandPaletteOpen) { e.preventDefault(); closePalette(); return }
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
