import { state, $ } from './state.js'

// ── Right Panel System ──────────────────────────────────────────
// Shared container for inspector, graph, calendar, ai-chat panels.
// Only one right panel can be open at a time.

const _panels = new Map()

/**
 * Register a panel type.
 * @param {string} id    - unique panel id (e.g. 'inspector')
 * @param {object} hooks
 * @param {function} hooks.build    - returns HTML string for the panel content
 * @param {function} [hooks.onOpen]  - called after panel content is injected
 * @param {function} [hooks.onClose] - called when panel is closed
 * @param {function} [hooks.onRefresh] - called to refresh panel content
 */
export function registerRightPanel(id, hooks) {
  _panels.set(id, hooks)
}

/**
 * Returns HTML for the right panel container.
 * Should be injected once into the DOM (inside .panes, after panes-workspace).
 */
export function buildRightPanelContainer() {
  return `<div class="right-panel-container" id="right-panel-container"></div>`
}

/**
 * Toggle a right panel: if it's open, close it; if another is open, switch; if none, open it.
 */
export function toggleRightPanel(id) {
  if (state.rightPanel === id) {
    closeRightPanel()
  } else {
    openRightPanel(id)
  }
}

/**
 * Open a specific right panel by id.
 */
export function openRightPanel(id) {
  const hooks = _panels.get(id)
  if (!hooks) return

  const container = $('right-panel-container')
  if (!container) return  // Editor UI not built yet — bail without mutating state

  // Close current panel if different
  if (state.rightPanel && state.rightPanel !== id) {
    const prevHooks = _panels.get(state.rightPanel)
    prevHooks?.onClose?.()
  }

  state.rightPanel = id
  container.innerHTML = hooks.build()
  container.classList.add('open')
  hooks.onOpen?.()
  syncRightPanelToggles()
}

/**
 * Close the currently open right panel.
 */
export function closeRightPanel() {
  if (!state.rightPanel) return

  const hooks = _panels.get(state.rightPanel)
  hooks?.onClose?.()

  state.rightPanel = null
  const container = $('right-panel-container')
  if (container) {
    container.classList.remove('open')
    container.innerHTML = ''
  }
  syncRightPanelToggles()
}

/**
 * Get the currently active right panel id, or null.
 */
export function getActiveRightPanel() {
  return state.rightPanel
}

/**
 * Refresh the currently open right panel (re-render its content).
 */
export function refreshRightPanel() {
  if (!state.rightPanel) return
  const hooks = _panels.get(state.rightPanel)
  if (!hooks) return

  if (hooks.onRefresh) {
    hooks.onRefresh()
  } else {
    // Default: rebuild content
    const container = $('right-panel-container')
    if (container) {
      container.innerHTML = hooks.build()
      hooks.onOpen?.()
    }
  }
}

/**
 * Sync toggle button active states for all right panel toggles.
 */
function syncRightPanelToggles() {
  const panelIds = ['inspector', 'graph', 'calendar', 'ai-chat']
  for (const pid of panelIds) {
    const toggle = $(`${pid}-toggle`) || $(`${pid}-panel-toggle`)
    if (toggle) {
      toggle.classList.toggle('active', state.rightPanel === pid)
    }
  }
}
