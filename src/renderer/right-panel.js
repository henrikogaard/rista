import { state, $ } from './state.js'

// ── Right Sidebar Widget System ─────────────────────────────────
// Stackable widgets in a shared right sidebar.
// Multiple widgets can be active at once; each has its own
// header (title + collapse + close).

const _widgets = new Map()

/**
 * Register a widget.
 * @param {string} id
 * @param {object} hooks
 * @param {string}   [hooks.title]      - shown in widget header
 * @param {string}   [hooks.icon]       - small SVG markup for the header
 * @param {function} hooks.build        - returns HTML string for body
 * @param {function} [hooks.onMount]    - called after body is injected
 * @param {function} [hooks.onUnmount]  - called before body is removed
 * @param {function} [hooks.onRefresh]  - called to refresh existing body
 * @param {number}   [hooks.flex]       - relative flex weight (default 1)
 */
export function registerRightPanel(id, hooks) {
  _widgets.set(id, hooks)
}

export function buildRightPanelContainer() {
  return `<div class="right-sidebar" id="right-panel-container">
    <div class="right-sidebar__resizer" data-action="right-sidebar-resize"></div>
  </div>`
}

const WIDGETS_KEY = 'fjordmark-right-widgets'
const COLLAPSED_KEY = 'fjordmark-right-widgets-collapsed'

function loadWidgetState() {
  try {
    const active = JSON.parse(localStorage.getItem(WIDGETS_KEY) || '[]')
    const collapsed = JSON.parse(localStorage.getItem(COLLAPSED_KEY) || '[]')
    return { active, collapsed }
  } catch {
    return { active: [], collapsed: [] }
  }
}

function persistWidgetState() {
  try {
    localStorage.setItem(WIDGETS_KEY, JSON.stringify(Array.from(state.rightWidgets)))
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(Array.from(state.collapsedWidgets)))
  } catch {}
}

function ensureStateShape() {
  if (!(state.rightWidgets instanceof Set)) {
    const stored = loadWidgetState()
    state.rightWidgets = new Set(stored.active)
    state.collapsedWidgets = new Set(stored.collapsed)
  }
  if (!(state.collapsedWidgets instanceof Set)) {
    state.collapsedWidgets = new Set()
  }
}

function getActiveWidgetIds() {
  ensureStateShape()
  // Preserve insertion order from registration but only return active ones
  return Array.from(_widgets.keys()).filter(id => state.rightWidgets.has(id))
}

function renderSidebar() {
  const container = $('right-panel-container')
  if (!container) return
  ensureStateShape()

  // Unmount widgets that are no longer active
  container.querySelectorAll('.widget').forEach(node => {
    const id = node.dataset.widget
    if (!state.rightWidgets.has(id)) {
      const hooks = _widgets.get(id)
      hooks?.onUnmount?.()
      node.remove()
    }
  })

  const activeIds = getActiveWidgetIds()
  if (activeIds.length === 0) {
    container.classList.remove('open')
    container.querySelectorAll('.widget').forEach(n => n.remove())
    syncRightPanelToggles()
    return
  }
  container.classList.add('open')

  // Append any newly-active widgets that aren't already in the DOM
  for (const id of activeIds) {
    if (container.querySelector(`.widget[data-widget="${id}"]`)) continue
    const hooks = _widgets.get(id)
    if (!hooks) continue
    const collapsed = state.collapsedWidgets.has(id)
    const wrapper = document.createElement('section')
    wrapper.className = `widget${collapsed ? ' widget--collapsed' : ''}`
    wrapper.dataset.widget = id
    wrapper.style.flex = collapsed ? '0 0 auto' : String(hooks.flex || 1)
    wrapper.innerHTML = `
      <header class="widget__header" data-action="widget-toggle-collapse" data-widget="${id}">
        ${hooks.icon ? `<span class="widget__icon">${hooks.icon}</span>` : ''}
        <span class="widget__title">${escapeHtml(hooks.title || id)}</span>
        <span class="widget__spacer"></span>
        <span class="widget__caret" aria-hidden="true">▾</span>
        <span class="widget__close" data-action="widget-close" data-widget="${id}" role="button" tabindex="0" title="Close">×</span>
      </header>
      <div class="widget__body">${hooks.build()}</div>
    `
    container.appendChild(wrapper)
    if (!collapsed) hooks.onMount?.()
  }

  // Update flex for collapse state on existing widgets too
  container.querySelectorAll('.widget').forEach(node => {
    const id = node.dataset.widget
    const collapsed = state.collapsedWidgets.has(id)
    const hooks = _widgets.get(id)
    node.classList.toggle('widget--collapsed', collapsed)
    node.style.flex = collapsed ? '0 0 auto' : String(hooks?.flex || 1)
  })

  syncRightPanelToggles()
}

export function openRightPanel(id) {
  ensureStateShape()
  if (!_widgets.has(id)) return
  if (!$('right-panel-container')) return  // editor UI not built yet
  state.rightWidgets.add(id)
  renderSidebar()
  persistWidgetState()
}

export function closeRightPanel(id) {
  ensureStateShape()
  // If called with no id, close all (back-compat with prior behavior)
  if (id === undefined) {
    state.rightWidgets.clear()
  } else {
    state.rightWidgets.delete(id)
  }
  renderSidebar()
  persistWidgetState()
}

export function toggleRightPanel(id) {
  ensureStateShape()
  if (state.rightWidgets.has(id)) {
    closeRightPanel(id)
  } else {
    openRightPanel(id)
  }
}

export function toggleWidgetCollapse(id) {
  ensureStateShape()
  if (state.collapsedWidgets.has(id)) {
    state.collapsedWidgets.delete(id)
    const hooks = _widgets.get(id)
    hooks?.onMount?.()
  } else {
    state.collapsedWidgets.add(id)
  }
  renderSidebar()
  persistWidgetState()
}

export function getActiveRightPanel() {
  ensureStateShape()
  // For backwards-compat: return the first active widget (or null)
  return getActiveWidgetIds()[0] || null
}

export function isRightPanelActive(id) {
  ensureStateShape()
  return state.rightWidgets.has(id)
}

export function refreshRightPanel(id) {
  ensureStateShape()
  if (id === undefined) {
    for (const wid of getActiveWidgetIds()) refreshRightPanel(wid)
    return
  }
  if (!state.rightWidgets.has(id)) return
  const hooks = _widgets.get(id)
  if (!hooks) return
  if (hooks.onRefresh) {
    hooks.onRefresh()
    return
  }
  // Default: rebuild body
  const container = $('right-panel-container')
  const widget = container?.querySelector(`.widget[data-widget="${id}"]`)
  if (!widget) return
  hooks.onUnmount?.()
  const body = widget.querySelector('.widget__body')
  if (body) body.innerHTML = hooks.build()
  hooks.onMount?.()
}

function syncRightPanelToggles() {
  ensureStateShape()
  const panelIds = ['inspector', 'graph', 'calendar', 'ai-chat']
  for (const pid of panelIds) {
    const toggle = $(`${pid}-toggle`) || $(`${pid}-panel-toggle`)
    if (toggle) toggle.classList.toggle('active', state.rightWidgets.has(pid))
  }
}

function escapeHtml(value = '') {
  return String(value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

// ── Event delegation ────────────────────────────────────────────
// Wire once when this module is imported.
document.addEventListener('click', (event) => {
  const closeBtn = event.target.closest('[data-action="widget-close"]')
  if (closeBtn) {
    event.stopPropagation()
    closeRightPanel(closeBtn.dataset.widget)
    return
  }
  const header = event.target.closest('[data-action="widget-toggle-collapse"]')
  if (header && !event.target.closest('[data-action="widget-close"]')) {
    toggleWidgetCollapse(header.dataset.widget)
  }
})

document.addEventListener('pointerdown', (event) => {
  const resizer = event.target.closest('[data-action="right-sidebar-resize"]')
  if (resizer) startRightSidebarResize(event)
})

// ── Sidebar resize ──────────────────────────────────────────────
export function startRightSidebarResize(event) {
  const container = $('right-panel-container')
  if (!container) return
  event.preventDefault()
  document.body.classList.add('is-resizing-right-sidebar')
  const startX = event.clientX
  const startWidth = container.getBoundingClientRect().width

  const onMove = (e) => {
    const delta = startX - e.clientX
    const next = Math.max(220, Math.min(640, startWidth + delta))
    document.documentElement.style.setProperty('--right-sidebar-width', `${next}px`)
  }
  const onUp = () => {
    document.body.classList.remove('is-resizing-right-sidebar')
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
    // Persist
    const final = document.documentElement.style.getPropertyValue('--right-sidebar-width')
    try { localStorage.setItem('fjordmark-right-sidebar-width', final) } catch {}
  }
  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

/**
 * Re-render the sidebar after the editor UI has been built.
 * Call this from `buildEditorUI()` so persisted widgets reappear.
 */
export function restoreRightPanel() {
  ensureStateShape()
  renderSidebar()
}

export function initRightSidebarWidth() {
  try {
    const saved = localStorage.getItem('fjordmark-right-sidebar-width')
    if (saved) document.documentElement.style.setProperty('--right-sidebar-width', saved)
  } catch {}
}
