import { getAllTagNames, getFilesForTag } from './link-index.js'
import { state } from './state.js'

import { $ } from './state.js'
let _openFile = null
let _onSelectTag = null
let _onClearTag = null

export function buildTagsPanel() {
  return `<div class="tags-view" id="tags-view-body"></div>`
}

export function mountTagsPanel(openFile, onSelectTag, onClearTag) {
  _openFile = openFile || _openFile
  _onSelectTag = onSelectTag || _onSelectTag
  _onClearTag = onClearTag || _onClearTag
  renderTagsPanel()
}

export function renderTagsPanel() {
  const body = $('tags-view-body')
  if (!body) return

  const tags = getAllTagNames()
    .map(name => ({ name, files: getFilesForTag(name) }))
    .sort((a, b) => b.files.length - a.files.length || a.name.localeCompare(b.name))

  const activeTag = state.tagFilter
  const activeHtml = activeTag ? `
    <div class="tags-selection">
      <span class="tags-selection__label">Selected</span>
      <span class="tags-selection__value">#${escapeHtml(activeTag)}</span>
      <button type="button" class="tags-selection__clear" data-action="clear-tag-filter">Clear</button>
    </div>
  ` : ''

  if (tags.length === 0) {
    body.innerHTML = `${activeHtml}<div class="tags-view__empty">No tags found</div>`
    return
  }

  body.innerHTML = `${activeHtml}${tags.map(tag => `
    <div class="tag-row${tag.name === activeTag ? ' active' : ''}" data-tag="${escapeAttribute(tag.name)}" role="button" tabindex="0" title="#${escapeAttribute(tag.name)}">
      <span class="tag-row__name">#${escapeHtml(tag.name)}</span>
      <span class="tag-row__count">${tag.files.length}</span>
    </div>
  `).join('')}`
}

export function handleTagsPanelEvent(event) {
  const clear = event.target.closest?.('[data-action="clear-tag-filter"]')
  if (clear) {
    if (event.type === 'keydown' && event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    _onClearTag?.()
    renderTagsPanel()
    return
  }

  const row = event.target.closest?.('[data-tag]')
  if (!row) return
  if (event.type === 'keydown' && event.key !== 'Enter' && event.key !== ' ') return
  event.preventDefault()
  const files = getFilesForTag(row.dataset.tag)
  _onSelectTag?.(row.dataset.tag)
  renderTagsPanel()
  if (files.length === 1 && _openFile) {
    const path = files[0]
    _openFile({ path, name: path.split(/[/\\]/).pop() })
  }
}

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function escapeAttribute(value) {
  return escapeHtml(value).replace(/'/g, '&#39;')
}
