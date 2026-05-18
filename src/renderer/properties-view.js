import { parseFrontmatter, applyFrontmatter } from './tags.js'
import { state, getFocusedTab, editorViews, getTabForPane, PANE_KEYS } from './state.js'
import { updateEditorDoc } from './editor.js'

const RESERVED_NAMES = new Set(['aliases', 'tags', 'cssclasses'])

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function detectType(value) {
  if (Array.isArray(value)) return 'list'
  if (typeof value === 'boolean') return 'boolean'
  if (typeof value === 'number') return 'number'
  if (typeof value === 'string') {
    if (/^\d{4}-\d{2}-\d{2}(T\d{2}:\d{2})?/.test(value)) return 'date'
    if (/^(true|false)$/i.test(value)) return 'boolean'
    if (/^-?\d+(\.\d+)?$/.test(value)) return 'number'
  }
  return 'text'
}

function getActiveTab() {
  return getFocusedTab()
}

function readFrontmatter(tab) {
  if (!tab) return {}
  const { frontmatter } = parseFrontmatter(tab.content || '')
  return frontmatter || {}
}

function writeFrontmatter(tab, next) {
  if (!tab) return
  const newContent = applyFrontmatter(tab.content || '', next)
  if (newContent === tab.content) return
  tab.content = newContent
  tab.dirty = true
  // Push into every pane that has this tab mounted so dual-pane stays in sync
  for (const pane of PANE_KEYS) {
    if (getTabForPane(pane) === tab) {
      const view = editorViews[pane]
      if (view) updateEditorDoc(view, newContent)
    }
  }
}

export function buildPropertiesPanel() {
  return `<div class="properties-view" id="properties-view-body"></div>`
}

export function renderProperties() {
  const body = document.getElementById('properties-view-body')
  if (!body) return
  const tab = getActiveTab()
  if (!tab) {
    body.innerHTML = `<div class="properties-view__empty">No note open</div>`
    return
  }
  const fm = readFrontmatter(tab)
  const keys = Object.keys(fm)

  const rows = keys.map(key => {
    const value = fm[key]
    const type = detectType(value)
    const inputHtml = renderInput(key, value, type)
    return `
      <div class="prop-row" data-key="${escapeHtml(key)}" data-type="${type}">
        <div class="prop-row__key" title="${escapeHtml(key)}">${escapeHtml(key)}</div>
        <div class="prop-row__value">${inputHtml}</div>
        <div class="prop-row__remove" data-action="remove-prop" title="Remove">×</div>
      </div>
    `
  }).join('')

  const emptyHint = keys.length === 0
    ? `<div class="properties-view__empty">No properties yet. Click <em>+ Add property</em> to create one.</div>`
    : ''

  body.innerHTML = `
    ${emptyHint}
    <div class="prop-list">${rows}</div>
    <div class="prop-add">
      <input type="text" class="prop-add__key" id="prop-add-key" placeholder="Property name" />
      <button type="button" class="prop-add__btn" id="prop-add-btn">+ Add</button>
    </div>
  `
}

function renderInput(key, value, type) {
  const lc = key.toLowerCase()
  if (type === 'list' || RESERVED_NAMES.has(lc)) {
    const items = Array.isArray(value) ? value : (value ? [value] : [])
    return `<input type="text" class="prop-input prop-input--list" data-input="${escapeHtml(key)}" value="${escapeHtml(items.join(', '))}" placeholder="comma, separated, values" />`
  }
  if (type === 'boolean') {
    const checked = String(value) === 'true'
    return `<input type="checkbox" class="prop-input prop-input--bool" data-input="${escapeHtml(key)}" ${checked ? 'checked' : ''} />`
  }
  const safeValue = value == null ? '' : value
  if (type === 'number') {
    return `<input type="number" class="prop-input" data-input="${escapeHtml(key)}" value="${escapeHtml(safeValue)}" />`
  }
  if (type === 'date') {
    return `<input type="text" class="prop-input" data-input="${escapeHtml(key)}" value="${escapeHtml(safeValue)}" placeholder="YYYY-MM-DD" />`
  }
  return `<input type="text" class="prop-input" data-input="${escapeHtml(key)}" value="${escapeHtml(safeValue)}" />`
}

function commitChange(key, rawValue, type) {
  const tab = getActiveTab()
  if (!tab) return
  const fm = readFrontmatter(tab)
  const lc = key.toLowerCase()
  if (type === 'list' || RESERVED_NAMES.has(lc)) {
    const items = String(rawValue)
      .split(',')
      .map(s => s.trim())
      .filter(Boolean)
    fm[key] = items
  } else if (type === 'boolean') {
    fm[key] = rawValue === true || rawValue === 'true'
  } else if (type === 'number') {
    const num = Number(rawValue)
    fm[key] = Number.isFinite(num) ? num : rawValue
  } else {
    fm[key] = rawValue
  }
  writeFrontmatter(tab, fm)
}

function removeProperty(key) {
  const tab = getActiveTab()
  if (!tab) return
  const fm = readFrontmatter(tab)
  if (!(key in fm)) return
  delete fm[key]
  writeFrontmatter(tab, fm)
  renderProperties()
}

function addProperty() {
  const input = document.getElementById('prop-add-key')
  if (!input) return
  const key = (input.value || '').trim()
  if (!key) return
  if (!/^[a-zA-Z0-9_-]+$/.test(key)) return
  const tab = getActiveTab()
  if (!tab) return
  const fm = readFrontmatter(tab)
  if (key in fm) return
  fm[key] = RESERVED_NAMES.has(key.toLowerCase()) ? [] : ''
  writeFrontmatter(tab, fm)
  input.value = ''
  renderProperties()
  // Focus the new value input
  requestAnimationFrame(() => {
    const newInput = document.querySelector(`[data-input="${CSS.escape(key)}"]`)
    newInput?.focus()
  })
}

export function mountPropertiesPanel() {
  renderProperties()
  const body = document.getElementById('properties-view-body')
  if (!body) return

  body.addEventListener('change', (event) => {
    const input = event.target.closest('[data-input]')
    if (!input) return
    const row = input.closest('.prop-row')
    const key = row?.dataset.key
    const type = row?.dataset.type || 'text'
    if (!key) return
    const value = input.type === 'checkbox' ? input.checked : input.value
    commitChange(key, value, type)
  })

  body.addEventListener('keydown', (event) => {
    // Commit text inputs on Enter so users don't have to blur first
    if (event.key !== 'Enter') return
    if (event.target.matches('.prop-input') && event.target.type !== 'checkbox') {
      event.target.blur()
    }
    if (event.target.id === 'prop-add-key') {
      event.preventDefault()
      addProperty()
    }
  })

  body.addEventListener('click', (event) => {
    const remove = event.target.closest('[data-action="remove-prop"]')
    if (remove) {
      const row = remove.closest('.prop-row')
      if (row?.dataset.key) removeProperty(row.dataset.key)
      return
    }
    if (event.target.id === 'prop-add-btn') addProperty()
  })
}
