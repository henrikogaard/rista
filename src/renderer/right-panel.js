import { state, $ } from './state.js'
import { showContextMenu } from './context-menu.js'

// ── Sidebar Widget System ────────────────────────────────────────
// Stackable widgets that can live on either the left or right sidebar.
// Multiple widgets can be active at once; each has its own
// header (title + collapse + close + drag handle).

const _widgets = new Map()
const SIDES = ['right', 'left']
const STACK_ID = { right: 'right-sidebar-stack', left: 'left-widget-stack' }

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
    <div class="right-sidebar__tabs" id="right-sidebar-tabs"></div>
    <div class="right-sidebar__stack widget-stack widget-stack--right" id="right-sidebar-stack"></div>
  </div>`
}

const WIDGETS_KEY = 'fjordmark-right-widgets'
const COLLAPSED_KEY = 'fjordmark-right-widgets-collapsed'
const ORDER_KEY = 'fjordmark-right-widgets-order'
const FLEX_KEY = 'fjordmark-right-widgets-flex'
const SIDE_KEY = 'fjordmark-widget-side'
const SEEN_KEY = 'fjordmark-widget-seen'

function loadWidgetState() {
  try {
    const active = JSON.parse(localStorage.getItem(WIDGETS_KEY) || '[]')
    const collapsed = JSON.parse(localStorage.getItem(COLLAPSED_KEY) || '[]')
    const order = JSON.parse(localStorage.getItem(ORDER_KEY) || '[]')
    const flex = JSON.parse(localStorage.getItem(FLEX_KEY) || '{}')
    const side = JSON.parse(localStorage.getItem(SIDE_KEY) || '{}')
    return { active, collapsed, order, flex, side }
  } catch {
    return { active: [], collapsed: [], order: [], flex: {}, side: {} }
  }
}

function persistWidgetState() {
  try {
    localStorage.setItem(WIDGETS_KEY, JSON.stringify(Array.from(state.rightWidgets)))
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(Array.from(state.collapsedWidgets)))
    localStorage.setItem(ORDER_KEY, JSON.stringify(state.rightWidgetsOrder || []))
    localStorage.setItem(FLEX_KEY, JSON.stringify(state.rightWidgetsFlex || {}))
    localStorage.setItem(SIDE_KEY, JSON.stringify(state.widgetSide || {}))
  } catch {}
}

function ensureStateShape() {
  if (!(state.rightWidgets instanceof Set)) {
    const stored = loadWidgetState()
    state.rightWidgets = new Set(stored.active)
    state.collapsedWidgets = new Set(stored.collapsed)
    state.rightWidgetsOrder = stored.order
    state.rightWidgetsFlex = stored.flex
    state.widgetSide = stored.side
  }
  if (!(state.collapsedWidgets instanceof Set)) {
    state.collapsedWidgets = new Set()
  }
  if (!Array.isArray(state.rightWidgetsOrder)) {
    state.rightWidgetsOrder = []
  }
  if (!state.rightWidgetsFlex || typeof state.rightWidgetsFlex !== 'object') {
    state.rightWidgetsFlex = {}
  }
  if (!state.widgetSide || typeof state.widgetSide !== 'object') {
    state.widgetSide = {}
  }
}

function getSeenWidgets() {
  try { return new Set(JSON.parse(localStorage.getItem(SEEN_KEY) || '[]')) }
  catch { return new Set() }
}

function markSeen(seen) {
  try { localStorage.setItem(SEEN_KEY, JSON.stringify(Array.from(seen))) } catch {}
}

function applyFirstLaunchDefaults() {
  // Activate defaultActive widgets the first time we encounter them (per widget).
  // This way, newly-introduced widgets light up for existing users too.
  const seen = getSeenWidgets()
  let changed = false
  for (const [id, hooks] of _widgets) {
    if (seen.has(id)) continue
    seen.add(id)
    if (hooks.defaultActive) {
      state.rightWidgets.add(id)
      changed = true
    }
  }
  markSeen(seen)
  if (changed) persistWidgetState()
}

function getWidgetSide(id) {
  ensureStateShape()
  if (state.widgetSide[id] === 'left' || state.widgetSide[id] === 'right') {
    return state.widgetSide[id]
  }
  const hooks = _widgets.get(id)
  return hooks?.defaultSide === 'left' ? 'left' : 'right'
}

function setWidgetSide(id, side) {
  ensureStateShape()
  state.widgetSide[id] = side === 'left' ? 'left' : 'right'
  persistWidgetState()
}

function getWidgetFlex(id) {
  ensureStateShape()
  const override = state.rightWidgetsFlex[id]
  if (typeof override === 'number' && override > 0) return override
  const hooks = _widgets.get(id)
  return Number(hooks?.flex || 1)
}

function getActiveWidgetIds() {
  ensureStateShape()
  // Preserve user-defined order, fall back to registration order
  const orderMap = new Map()
  state.rightWidgetsOrder.forEach((id, i) => orderMap.set(id, i))
  let nextOrder = state.rightWidgetsOrder.length
  for (const id of _widgets.keys()) {
    if (!orderMap.has(id)) orderMap.set(id, nextOrder++)
  }
  return Array.from(state.rightWidgets)
    .filter(id => _widgets.has(id))
    .sort((a, b) => orderMap.get(a) - orderMap.get(b))
}

function renderTabs() {
  const tabsEl = $('right-sidebar-tabs')
  if (!tabsEl) return
  ensureStateShape()
  const allIds = Array.from(_widgets.keys())
  tabsEl.innerHTML = allIds.map(id => {
    const hooks = _widgets.get(id)
    const active = state.rightWidgets.has(id) ? ' active' : ''
    const title = hooks?.title || id
    return `<div class="right-sidebar__tab${active}" data-action="widget-tab" data-widget="${id}" title="${escapeHtml(title)}" role="button" tabindex="0">${hooks?.icon || ''}</div>`
  }).join('')
}

function getActiveWidgetIdsForSide(side) {
  return getActiveWidgetIds().filter(id => getWidgetSide(id) === side)
}

function renderStack(side) {
  const stack = $(STACK_ID[side])
  if (!stack) return  // host not yet built (e.g. before editor UI mount)

  // Unmount widgets that have moved away or been closed
  stack.querySelectorAll('.widget').forEach(node => {
    const id = node.dataset.widget
    const stillHere = state.rightWidgets.has(id) && getWidgetSide(id) === side
    if (!stillHere) {
      const hooks = _widgets.get(id)
      hooks?.onUnmount?.()
      node.remove()
    }
  })

  const activeIds = getActiveWidgetIdsForSide(side)
  if (activeIds.length === 0) {
    stack.replaceChildren()
    stack.classList.remove('has-widgets')
    return
  }
  stack.classList.add('has-widgets')

  const existing = new Map()
  stack.querySelectorAll('.widget').forEach(node => existing.set(node.dataset.widget, node))

  const fragment = document.createDocumentFragment()
  activeIds.forEach((id, i) => {
    let node = existing.get(id)
    if (!node) {
      const hooks = _widgets.get(id)
      if (!hooks) return
      const collapsed = state.collapsedWidgets.has(id)
      node = document.createElement('section')
      node.className = `widget${collapsed ? ' widget--collapsed' : ''}`
      node.dataset.widget = id
      node.dataset.side = side
      node.style.flex = collapsed ? '0 0 auto' : `${getWidgetFlex(id)} 1 0`
      const actionsHtml = typeof hooks.headerActions === 'function' ? hooks.headerActions() : ''
      node.innerHTML = `
        <header class="widget__header" draggable="true" data-action="widget-toggle-collapse" data-widget="${id}">
          <span class="widget__drag-handle" aria-hidden="true">⋮⋮</span>
          ${hooks.icon ? `<span class="widget__icon">${hooks.icon}</span>` : ''}
          <span class="widget__title">${escapeHtml(hooks.title || id)}</span>
          <span class="widget__spacer"></span>
          ${actionsHtml ? `<div class="widget__actions">${actionsHtml}</div>` : ''}
          <span class="widget__caret" aria-hidden="true">▾</span>
          <span class="widget__close" data-action="widget-close" data-widget="${id}" role="button" tabindex="0" title="Close">×</span>
        </header>
        <div class="widget__body">${hooks.build()}</div>
      `
      fragment.appendChild(node)
      if (!collapsed) requestAnimationFrame(() => hooks.onMount?.())
    } else {
      node.dataset.side = side  // stay in sync if moved
      fragment.appendChild(node)
    }
    if (i < activeIds.length - 1) {
      const resizer = document.createElement('div')
      resizer.className = 'widget-resizer'
      resizer.dataset.action = 'widget-resize'
      resizer.dataset.aboveWidget = id
      resizer.dataset.belowWidget = activeIds[i + 1]
      resizer.dataset.side = side
      fragment.appendChild(resizer)
    }
  })
  stack.replaceChildren(fragment)

  stack.querySelectorAll('.widget').forEach(node => {
    const id = node.dataset.widget
    const collapsed = state.collapsedWidgets.has(id)
    node.classList.toggle('widget--collapsed', collapsed)
    node.style.flex = collapsed ? '0 0 auto' : `${getWidgetFlex(id)} 1 0`
  })
}

function renderSidebar() {
  ensureStateShape()
  // Either or both stack hosts may exist; renderStack guards individually.
  for (const side of SIDES) renderStack(side)

  // Right container is part of the editor UI which may not yet be built.
  const container = $('right-panel-container')
  if (container) {
    const rightActive = getActiveWidgetIdsForSide('right').length > 0
    container.classList.toggle('open', rightActive)
  }

  renderTabs()
  syncRightPanelToggles()
}

function setWidgetOrder(orderedIds) {
  ensureStateShape()
  // Keep ordering for all registered widgets so unknown ones still sort correctly
  const known = new Set(_widgets.keys())
  const seen = new Set()
  const next = []
  for (const id of orderedIds) {
    if (known.has(id) && !seen.has(id)) { next.push(id); seen.add(id) }
  }
  for (const id of _widgets.keys()) {
    if (!seen.has(id)) next.push(id)
  }
  state.rightWidgetsOrder = next
  persistWidgetState()
}

export function openRightPanel(id) {
  ensureStateShape()
  if (!_widgets.has(id)) return
  // The widget's assigned side's stack must exist
  const side = getWidgetSide(id)
  if (!$(STACK_ID[side])) return
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

/**
 * Toggle the whole sidebar: if any widget is open, close all;
 * otherwise re-open the previously active widgets (or inspector as fallback).
 */
let _lastActive = null
export function toggleRightSidebar() {
  ensureStateShape()
  const rightIds = getActiveWidgetIdsForSide('right')
  if (rightIds.length > 0) {
    _lastActive = rightIds.slice()
    rightIds.forEach(id => state.rightWidgets.delete(id))
    renderSidebar()
    persistWidgetState()
    return
  }
  if (!$('right-panel-container')) return
  const toRestore = (_lastActive && _lastActive.length ? _lastActive : ['inspector'])
    .filter(id => _widgets.has(id) && getWidgetSide(id) === 'right')
  if (toRestore.length === 0 && _widgets.has('inspector') && getWidgetSide('inspector') === 'right') {
    toRestore.push('inspector')
  }
  toRestore.forEach(id => state.rightWidgets.add(id))
  renderSidebar()
  persistWidgetState()
}

export function toggleWidgetCollapse(id) {
  ensureStateShape()
  if (state.collapsedWidgets.has(id)) {
    state.collapsedWidgets.delete(id)
    // Body stays in the DOM with its listeners attached — no remount needed.
    // Refresh content in case state changed while hidden.
    const hooks = _widgets.get(id)
    hooks?.onRefresh?.()
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
  // Default: rebuild body — widget may be on either side
  const widget = document.querySelector(`.widget[data-widget="${id}"]`)
  if (!widget) return
  hooks.onUnmount?.()
  const body = widget.querySelector('.widget__body')
  if (body) body.innerHTML = hooks.build()
  hooks.onMount?.()
}

function syncRightPanelToggles() {
  ensureStateShape()
  const toggle = $('right-sidebar-toggle')
  if (toggle) {
    const anyRight = getActiveWidgetIdsForSide('right').length > 0
    toggle.classList.toggle('active', anyRight)
  }
}

function escapeHtml(value = '') {
  return String(value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

// ── Event delegation ────────────────────────────────────────────
// Wire once when this module is imported.
document.addEventListener('click', (event) => {
  // Tab strip click → toggle widget
  const tab = event.target.closest('[data-action="widget-tab"]')
  if (tab) {
    toggleRightPanel(tab.dataset.widget)
    return
  }
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
  const sidebarResizer = event.target.closest('[data-action="right-sidebar-resize"]')
  if (sidebarResizer) {
    startRightSidebarResize(event)
    return
  }
  const widgetResizer = event.target.closest('[data-action="widget-resize"]')
  if (widgetResizer) startWidgetResize(event, widgetResizer)
})

document.addEventListener('contextmenu', (event) => {
  const header = event.target.closest('.widget__header')
  if (!header) return
  const id = header.dataset.widget
  if (!id) return
  event.preventDefault()
  const currentSide = getWidgetSide(id)
  const otherSide = currentSide === 'right' ? 'left' : 'right'
  const otherLabel = otherSide === 'right' ? 'right sidebar' : 'left sidebar'
  showContextMenu(event.clientX, event.clientY, [
    { label: `Move to ${otherLabel}`, action: () => moveWidgetToSide(id, otherSide) },
    { separator: true },
    { label: state.collapsedWidgets.has(id) ? 'Expand' : 'Collapse', action: () => toggleWidgetCollapse(id) },
    { label: 'Close', action: () => closeRightPanel(id) },
  ])
})

export function moveWidgetToSide(id, side) {
  if (!_widgets.has(id)) return
  setWidgetSide(id, side)
  // Ensure the widget is active so the destination renders it
  state.rightWidgets.add(id)
  renderSidebar()
  persistWidgetState()
}

function startWidgetResize(event, resizer) {
  ensureStateShape()
  const aboveId = resizer.dataset.aboveWidget
  const belowId = resizer.dataset.belowWidget
  if (!aboveId || !belowId) return
  if (state.collapsedWidgets.has(aboveId) || state.collapsedWidgets.has(belowId)) return

  const side = resizer.dataset.side || 'right'
  const stack = $(STACK_ID[side])
  const aboveEl = stack?.querySelector(`.widget[data-widget="${aboveId}"]`)
  const belowEl = stack?.querySelector(`.widget[data-widget="${belowId}"]`)
  if (!aboveEl || !belowEl) return

  event.preventDefault()
  document.body.classList.add('is-resizing-widget')

  const startY = event.clientY
  const aboveStart = aboveEl.getBoundingClientRect().height
  const belowStart = belowEl.getBoundingClientRect().height
  const total = aboveStart + belowStart
  const flexAbove = getWidgetFlex(aboveId)
  const flexBelow = getWidgetFlex(belowId)
  const totalFlex = flexAbove + flexBelow
  const minPx = 60

  const onMove = (e) => {
    const delta = e.clientY - startY
    let nextAbove = Math.max(minPx, Math.min(total - minPx, aboveStart + delta))
    const nextBelow = total - nextAbove
    const newFlexAbove = (nextAbove / total) * totalFlex
    const newFlexBelow = (nextBelow / total) * totalFlex
    state.rightWidgetsFlex[aboveId] = newFlexAbove
    state.rightWidgetsFlex[belowId] = newFlexBelow
    aboveEl.style.flex = `${newFlexAbove} 1 0`
    belowEl.style.flex = `${newFlexBelow} 1 0`
  }
  const onUp = () => {
    document.body.classList.remove('is-resizing-widget')
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
    persistWidgetState()
  }
  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

// ── Drag-to-reorder widget headers ──────────────────────────────
let _dragId = null
document.addEventListener('dragstart', (event) => {
  const header = event.target.closest('.widget__header[draggable="true"]')
  if (!header) return
  _dragId = header.dataset.widget
  // Drag the whole widget visually
  const widget = header.closest('.widget')
  if (widget && event.dataTransfer) {
    event.dataTransfer.effectAllowed = 'move'
    event.dataTransfer.setData('text/plain', _dragId)
    event.dataTransfer.setDragImage(widget, 10, 10)
    widget.classList.add('widget--dragging')
  }
  document.body.classList.add('is-dragging-widget')
})
document.addEventListener('dragend', (event) => {
  const header = event.target.closest('.widget__header')
  if (header) header.closest('.widget')?.classList.remove('widget--dragging')
  _dragId = null
  document.body.classList.remove('is-dragging-widget')
  document.querySelectorAll('.widget--drop-above, .widget--drop-below, .widget-stack--drop-end').forEach(n => {
    n.classList.remove('widget--drop-above', 'widget--drop-below', 'widget-stack--drop-end')
  })
})
function findStackFromEvent(event) {
  for (const side of SIDES) {
    const stack = $(STACK_ID[side])
    if (stack && event.target.closest(`#${STACK_ID[side]}`)) return { stack, side }
  }
  return null
}

document.addEventListener('dragover', (event) => {
  if (!_dragId) return
  const target = findStackFromEvent(event)
  if (!target) return
  const { stack, side } = target
  const targetWidget = event.target.closest('.widget')
  event.preventDefault()
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'

  document.querySelectorAll('.widget--drop-above, .widget--drop-below').forEach(n => {
    n.classList.remove('widget--drop-above', 'widget--drop-below')
  })
  document.querySelectorAll('.widget-stack--drop-end').forEach(n => n.classList.remove('widget-stack--drop-end'))

  if (targetWidget && targetWidget.dataset.widget !== _dragId && stack.contains(targetWidget)) {
    const rect = targetWidget.getBoundingClientRect()
    const above = event.clientY < rect.top + rect.height / 2
    targetWidget.classList.add(above ? 'widget--drop-above' : 'widget--drop-below')
  } else {
    // Dropping into an empty area of a stack → append at the end of that side
    stack.classList.add('widget-stack--drop-end')
  }
})

document.addEventListener('drop', (event) => {
  if (!_dragId) return
  const target = findStackFromEvent(event)
  if (!target) return
  const { stack, side } = target
  event.preventDefault()

  // Apply the side change first so subsequent getWidgetSide() reads are correct
  if (getWidgetSide(_dragId) !== side) setWidgetSide(_dragId, side)

  // Compute target index *within the destination side* (0-based)
  const sideItemsBefore = Array.from(stack.querySelectorAll('.widget'))
    .map(n => n.dataset.widget)
    .filter(id => id !== _dragId)
  let insertAtSide = sideItemsBefore.length
  const targetWidget = event.target.closest('.widget')
  if (targetWidget && targetWidget.dataset.widget !== _dragId && stack.contains(targetWidget)) {
    const rect = targetWidget.getBoundingClientRect()
    const above = event.clientY < rect.top + rect.height / 2
    insertAtSide = sideItemsBefore.indexOf(targetWidget.dataset.widget)
    if (!above) insertAtSide += 1
  }

  // Translate the per-side insertion to a global position in rightWidgetsOrder.
  // Strip _dragId out, then insert it before the (insertAtSide)-th destination-
  // side item (or at the end if there are fewer destination items).
  const without = state.rightWidgetsOrder.filter(id => id !== _dragId)
  const destPositions = []
  without.forEach((id, i) => {
    if (_widgets.has(id) && getWidgetSide(id) === side) destPositions.push(i)
  })
  let globalInsertPos
  if (destPositions.length === 0) {
    globalInsertPos = without.length
  } else if (insertAtSide >= destPositions.length) {
    globalInsertPos = destPositions[destPositions.length - 1] + 1
  } else {
    globalInsertPos = destPositions[insertAtSide]
  }
  without.splice(globalInsertPos, 0, _dragId)
  setWidgetOrder(without)

  document.querySelectorAll('.widget-stack--drop-end').forEach(n => n.classList.remove('widget-stack--drop-end'))
  renderSidebar()
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
  applyFirstLaunchDefaults()
  renderSidebar()
}

export function initRightSidebarWidth() {
  try {
    const saved = localStorage.getItem('fjordmark-right-sidebar-width')
    if (saved) document.documentElement.style.setProperty('--right-sidebar-width', saved)
  } catch {}
}
