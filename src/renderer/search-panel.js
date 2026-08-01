import { state, $ } from './state.js'
import { getSettings } from './settings.js'
import { getLinkIndex, searchFiles } from './link-index.js'
import { buildSemanticIndex, searchSemanticIndex } from './semantic-index.js'

// ── Project Search Panel ─────────────────────────────────────────
// A floating modal for project-wide search across markdown files.

let searchQuery = ''
let searchResults = []
let selectedIndex = -1
let searchDebounceTimer = null
let searchIndexStatus = null
let searchIndexError = ''

export function buildSearchPanel() {
  return `
    <div class="search-panel-overlay" id="search-panel-overlay"></div>
    <div class="search-panel" id="search-panel">
      <div class="search-panel__input-wrap">
        <svg class="search-panel__icon" viewBox="0 0 16 16" width="14" height="14"><circle cx="7" cy="7" r="5.5" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M11 11l3 3" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/></svg>
        <input type="text" class="search-panel__input" id="search-input" placeholder="Search project files…" autocomplete="off" spellcheck="false">
        <div class="search-panel__shortcut">Esc</div>
      </div>
      <div class="search-panel__results" id="search-results"></div>
      <div class="search-panel__footer" id="search-footer"></div>
    </div>
  `
}

export function toggleSearchPanel() {
  const panel = $('search-panel')
  if (!panel) return
  if (panel.classList.contains('open')) {
    closeSearchPanel()
  } else {
    openSearchPanel()
  }
}

export function openSearchPanel() {
  const panel = $('search-panel')
  const overlay = $('search-panel-overlay')
  if (!panel || !overlay) return
  panel.classList.add('open')
  overlay.classList.add('open')
  document.body.classList.add('search-open')

  const input = $('search-input')
  if (input) {
    input.value = ''
    input.focus()
    searchQuery = ''
    searchResults = []
    selectedIndex = -1
    searchIndexStatus = null
    searchIndexError = ''
    renderSearchResults()
  }
}

export function closeSearchPanel() {
  const panel = $('search-panel')
  const overlay = $('search-panel-overlay')
  if (!panel || !overlay) return
  panel.classList.remove('open')
  overlay.classList.remove('open')
  document.body.classList.remove('search-open')
}

export function handleSearchInput(callback) {
  const input = $('search-input')
  if (!input) return

  input.addEventListener('input', (e) => {
    searchQuery = e.target.value.trim()
    clearTimeout(searchDebounceTimer)
    searchDebounceTimer = setTimeout(() => {
      runSearch()
    }, 120)
  })

  input.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      closeSearchPanel()
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      selectedIndex = Math.min(selectedIndex + 1, searchResults.length - 1)
      renderSearchResults()
      scrollSelectedIntoView()
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      selectedIndex = Math.max(selectedIndex - 1, 0)
      renderSearchResults()
      scrollSelectedIntoView()
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      const result = searchResults[selectedIndex]
      if (result) {
        callback?.(buildOpenTarget(result))
        closeSearchPanel()
      }
      return
    }
  })

  $('search-panel-overlay')?.addEventListener('click', closeSearchPanel)

  $('search-results')?.addEventListener('click', (e) => {
    const row = e.target.closest('.search-result')
    if (row && row.dataset.path) {
      const result = searchResults[Number(row.dataset.index)]
      callback?.(buildOpenTarget(result || row.dataset))
      closeSearchPanel()
    }
  })
}

function runSearch() {
  if (searchQuery.length < 2) {
    searchResults = []
    selectedIndex = -1
    searchIndexStatus = null
    searchIndexError = ''
    renderSearchResults()
    return
  }

  try {
    const settings = getSettings()
    const semanticEnabled = settings.featureSemanticIndex || settings.showExperimental
    const semanticIndex = semanticEnabled
      ? buildSemanticIndex(getLinkIndex(), { folderPath: state.folderPath }) : null
    searchIndexStatus = semanticIndex
    searchIndexError = ''
    const fileResults = searchFiles(searchQuery, { limit: 50 })
    const semanticResults = searchSemanticIndex(semanticIndex, searchQuery, { limit: 50 })
    searchResults = mergeSearchResults(fileResults, semanticResults).slice(0, 50)
  } catch (err) {
    searchIndexError = err?.message || 'Semantic index failed'
    searchIndexStatus = searchIndexStatus || { documentCount: 0, dirty: false }
    try {
      searchResults = searchFiles(searchQuery, { limit: 50 })
    } catch {
      searchResults = []
    }
  }

  selectedIndex = searchResults.length > 0 ? 0 : -1
  renderSearchResults()
}

function mergeSearchResults(fileResults, semanticResults) {
  const byPath = new Map()

  for (const result of fileResults) {
    byPath.set(result.path, {
      ...result,
      rank: result.nameMatch ? 100 : 60,
    })
  }

  for (const result of semanticResults) {
    const existing = byPath.get(result.path)
    if (existing) {
      existing.semanticMatch = true
      existing.heading = result.heading
      existing.line = result.line
      existing.relevance = result.relevance
      existing.preview = existing.preview || result.snippet
      existing.rank += result.score * 50
    } else {
      byPath.set(result.path, {
        path: result.path,
        name: result.name,
        preview: result.snippet,
        heading: result.heading,
        line: result.line,
        relevance: result.relevance,
        semanticMatch: true,
        nameMatch: false,
        contentMatch: true,
        rank: result.score * 50,
      })
    }
  }

  return Array.from(byPath.values())
    .sort((a, b) => b.rank - a.rank || a.name.localeCompare(b.name))
}

function renderSearchResults() {
  const container = $('search-results')
  const footer = $('search-footer')
  if (!container) return

  if (!searchResults.length) {
    if (searchQuery.length < 2) {
      container.innerHTML = `<div class="search-empty">Type to search project files</div>`
    } else {
      container.innerHTML = `<div class="search-empty">No results for "${escapeHtml(searchQuery)}"</div>`
    }
    if (footer) footer.textContent = searchIndexStatus || searchIndexError ? searchIndexFooter() : ''
    return
  }

  container.innerHTML = searchResults.map((r, i) => `
    <div class="search-result${i === selectedIndex ? ' active' : ''}" data-path="${escapeAttr(r.path)}" data-index="${i}" data-heading="${escapeAttr(r.heading || '')}" data-line="${escapeAttr(r.line || '')}">
      <div class="search-result__name">${escapeHtml(r.name)}</div>
      <div class="search-result__path">${escapeHtml(r.path)}</div>
      ${r.heading ? `<div class="search-result__cite">${escapeHtml(r.heading)}${r.relevance ? ` · ${Math.round(r.relevance * 100)}%` : ''}</div>` : ''}
      ${r.preview ? `<div class="search-result__preview">${escapeHtml(r.preview)}</div>` : ''}
    </div>
  `).join('')

  if (footer) {
    footer.textContent = `${searchResults.length} result${searchResults.length === 1 ? '' : 's'} · ${searchIndexFooter()}`
  }
}

function searchIndexFooter() {
  if (searchIndexError) return `Index error: ${searchIndexError}`
  const count = searchIndexStatus?.documentCount || 0
  const stale = searchIndexStatus?.dirty ? ' · index updating' : ''
  return `${count} notes indexed${stale}`
}

function buildOpenTarget(result) {
  if (!result) return null
  const path = result.path
  const name = result.name || path?.split(/[/\\]/).pop()
  if (result.heading && Number.isFinite(Number(result.line)) && Number(result.line) > 0) {
    return {
      path,
      name,
      heading: { text: result.heading, line: Number(result.line) },
    }
  }
  return { path, name }
}

function scrollSelectedIntoView() {
  const selected = document.querySelector('.search-result.active')
  if (selected) selected.scrollIntoView({ block: 'nearest' })
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function escapeAttr(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}
