import { parseFrontmatter, applyFrontmatter } from './tags.js'
import { $, getFocusedTab, editorViews, getTabForPane, PANE_KEYS, escapeHtml } from './state.js'
import { updateEditorDoc } from './editor.js'
import { refreshPreview } from './preview.js'
const RESERVED_NAMES = new Set(['aliases', 'tags', 'cssclasses'])



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
      refreshPreview(pane, newContent)
    }
  }
}

function numberValue(value, fallback = 0.5) {
  const next = Number(value)
  if (!Number.isFinite(next)) return fallback
  return Math.min(1, Math.max(0, next))
}

export function buildPropertiesPanel() {
  return `<div class="properties-view" id="properties-view-body"></div>`
}

export function renderProperties() {
  const body = $('properties-view-body')
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
    ${renderBannerSettings(fm)}
    ${emptyHint}
    <div class="prop-list">${rows}</div>
    <div class="prop-add">
      <input type="text" class="prop-add__key" id="prop-add-key" placeholder="Property name" />
      <button type="button" class="prop-add__btn" id="prop-add-btn">+ Add</button>
    </div>
  `
}

function renderBannerSettings(fm) {
  const banner = fm.banner || ''
  const x = numberValue(fm.banner_x, 0.5)
  const y = numberValue(fm.banner_y, 0.5)
  const hasBanner = Boolean(String(banner).trim())
  const previewLabel = hasBanner ? escapeHtml(banner) : 'No banner set'
  return `
    <section class="banner-settings">
      <div class="banner-settings__head">
        <div>
          <div class="banner-settings__title">Banner</div>
          <div class="banner-settings__hint">Use an Obsidian link or local image path.</div>
        </div>
        <button type="button" class="banner-settings__clear" data-action="clear-banner" ${hasBanner ? '' : 'disabled'}>Clear</button>
      </div>
      <div class="banner-settings__preview${hasBanner ? ' has-banner' : ''}" title="${previewLabel}">
        <span>${previewLabel}</span>
      </div>
      <input
        type="text"
        class="prop-input banner-settings__input"
        id="banner-image-input"
        value="${escapeHtml(banner)}"
        placeholder="![[cover.jpg]] or _assets/cover.jpg"
      />
      <label class="banner-settings__axis">
        <span>Horizontal</span>
        <input class="banner-settings__range" type="range" min="0" max="1" step="0.01" value="${x}" data-banner-axis="banner_x">
        <output>${Math.round(x * 100)}%</output>
      </label>
      <label class="banner-settings__axis">
        <span>Vertical</span>
        <input class="banner-settings__range" type="range" min="0" max="1" step="0.01" value="${y}" data-banner-axis="banner_y">
        <output>${Math.round(y * 100)}%</output>
      </label>
      <button type="button" class="banner-settings__apply" data-action="apply-banner">Apply banner</button>
    </section>
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
  const input = $('prop-add-key')
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

function updateBannerSettings(partial = {}) {
  const tab = getActiveTab()
  if (!tab) return
  const fm = readFrontmatter(tab)
  const currentBanner = String(fm.banner || '').trim()
  const nextBanner = Object.prototype.hasOwnProperty.call(partial, 'banner')
    ? String(partial.banner || '').trim()
    : currentBanner

  if (!nextBanner) {
    delete fm.banner
    delete fm.banner_x
    delete fm.banner_y
  } else {
    fm.banner = nextBanner
    fm.banner_x = numberValue(partial.banner_x ?? fm.banner_x, 0.5)
    fm.banner_y = numberValue(partial.banner_y ?? fm.banner_y, 0.5)
  }

  writeFrontmatter(tab, fm)
  renderProperties()
}

function readBannerForm() {
  return {
    banner: $('banner-image-input')?.value || '',
    banner_x: document.querySelector('[data-banner-axis="banner_x"]')?.value,
    banner_y: document.querySelector('[data-banner-axis="banner_y"]')?.value,
  }
}

export function mountPropertiesPanel() {
  renderProperties()
  const body = $('properties-view-body')
  if (!body) return

  body.addEventListener('change', (event) => {
    const axis = event.target.closest('[data-banner-axis]')
    if (axis) {
      updateBannerSettings(readBannerForm())
      return
    }

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
    if (event.target.id === 'banner-image-input') {
      event.preventDefault()
      updateBannerSettings(readBannerForm())
    }
  })

  body.addEventListener('click', (event) => {
    const applyBanner = event.target.closest('[data-action="apply-banner"]')
    if (applyBanner) {
      updateBannerSettings(readBannerForm())
      return
    }

    const clearBanner = event.target.closest('[data-action="clear-banner"]')
    if (clearBanner) {
      updateBannerSettings({ banner: '' })
      return
    }

    const remove = event.target.closest('[data-action="remove-prop"]')
    if (remove) {
      const row = remove.closest('.prop-row')
      if (row?.dataset.key) removeProperty(row.dataset.key)
      return
    }
    if (event.target.id === 'prop-add-btn') addProperty()
  })
}
