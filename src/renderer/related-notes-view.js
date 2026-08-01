import { getFocusedTab } from './state.js'
import { getLinkIndex } from './link-index.js'
import { buildSemanticIndex, findRelatedNotes } from './semantic-index.js'
import { state } from './state.js'
import { getSettings } from './settings.js'

import { $ } from './state.js'
let _openFile = null

export function buildRelatedNotesPanel() {
  return `<div class="related-notes" id="related-notes-body"></div>`
}

export function mountRelatedNotesPanel(openFile) {
  _openFile = openFile || _openFile
  renderRelatedNotesPanel()
}

export function renderRelatedNotesPanel() {
  const body = $('related-notes-body')
  if (!body) return

  const tab = getFocusedTab()
  if (!state.folderPath) {
    body.innerHTML = `<div class="related-notes__empty">No folder open</div>`
    return
  }
  if (!tab?.path) {
    body.innerHTML = `<div class="related-notes__empty">No active note</div>`
    return
  }

  let semanticIndex
  let related
  try {
    const settings = getSettings()
    const semanticEnabled = settings.featureSemanticIndex || settings.showExperimental
    if (semanticEnabled) {
      semanticIndex = buildSemanticIndex(getLinkIndex(), { folderPath: state.folderPath })
      related = findRelatedNotes(semanticIndex, tab.path, { limit: 8 })
    } else {
      related = []
    }
  } catch (err) {
    body.innerHTML = renderRelatedError(err)
    return
  }

  if (!related.length) {
    body.innerHTML = `
      ${renderStatus(semanticIndex)}
      <div class="related-notes__empty">No related notes</div>
    `
    return
  }

  body.innerHTML = `
    ${renderStatus(semanticIndex)}
    <div class="related-notes__list">
      ${related.map(renderRelatedNote).join('')}
    </div>
  `
}

export function handleRelatedNotesPanelEvent(event) {
  const row = event.target.closest?.('[data-path]')
  if (!row) return
  if (event.type === 'keydown' && event.key !== 'Enter' && event.key !== ' ') return
  event.preventDefault()
  const path = row.dataset.path
  if (!path) return
  _openFile?.({ path, name: path.split(/[/\\]/).pop() })
}

function renderStatus(index) {
  const stale = index.dirty ? '<span class="related-notes__stale">Index updating</span>' : ''
  return `
    <div class="related-notes__status">
      <span>${index.documentCount} notes indexed</span>
      ${stale}
    </div>
  `
}

function renderRelatedError(err) {
  const message = err?.message || 'Unable to build related notes'
  return `<div class="related-notes__error">Index error: ${escapeHtml(message)}</div>`
}

function renderRelatedNote(item) {
  return `
    <div class="related-note" data-path="${escapeAttribute(item.path)}" role="button" tabindex="0" title="${escapeAttribute(item.relativePath)}">
      <div class="related-note__top">
        <span class="related-note__title">${escapeHtml(item.title)}</span>
        <span class="related-note__score">${Math.round(item.relevance * 100)}%</span>
      </div>
      <div class="related-note__meta">${escapeHtml(item.heading)} · ${escapeHtml(item.reason)}</div>
      <div class="related-note__snippet">${escapeHtml(item.snippet)}</div>
    </div>
  `
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
