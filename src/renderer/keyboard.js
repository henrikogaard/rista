import { state } from './state.js'
import { matchesBinding } from './keybindings.js'
import { toggleZenMode, exitZenMode } from './zen-mode.js'
import { closeCommandPalette as closePalette, openCommandPaletteFiles, openCommandPaletteCommands } from './command-palette.js'
import { closeCommandDialog } from './commands.js'
import { closeDiagramBuilder } from './diagram-builder.js'
import { toggleSettingsPanel, closeSettingsPanel } from './shell.js'
import { toggleSidebar, toggleToolbar } from './workspace.js'
import { toggleTerminalDrawer } from './terminal-drawer.js'
import { toggleFindReplace } from './find-replace.js'
import { openSearchPanel } from './search-panel.js'
import { saveActive, saveActiveAs, createNewFile, createDailyNote } from './tabs.js'

// ── Keyboard shortcuts ───────────────────────────────────────────
export function initKeyboardShortcuts() {
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
}

// ── Auto-hide chrome while typing (#47) ─────────────────────────
// When the user is actively typing in the editor, fade out tabs and brandrail
// so the writing surface can breathe. Chrome reappears on mouse move or pause.
let _typingTimer = null
function _setTyping(active) {
  document.documentElement.dataset.typing = active ? 'true' : 'false'
}

export function initAutoHideChrome() {
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
}
