import { state, $ } from './state.js'
import { searchFiles } from './link-index.js'

// ── Project Search Panel ─────────────────────────────────────────
// A floating modal for project-wide search across markdown files.

let searchQuery = ''
let searchResults = []
let selectedIndex = -1
let searchDebounceTimer = null

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
        callback?.(result.path)
        closeSearchPanel()
      }
      return
    }
  })

  $('search-panel-overlay')?.addEventListener('click', closeSearchPanel)

  $('search-results')?.addEventListener('click', (e) => {
    const row = e.target.closest('.search-result')
    if (row && row.dataset.path) {
      callback?.(row.dataset.path)
      closeSearchPanel()
    }
  })
}

function runSearch() {
  if (searchQuery.length < 2) {
    searchResults = []
    selectedIndex = -1
    renderSearchResults()
    return
  }
  searchResults = searchFiles(searchQuery, { limit: 50 })
  selectedIndex = searchResults.length > 0 ? 0 : -1
  renderSearchResults()
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
    if (footer) footer.textContent = ''
    return
  }

  container.innerHTML = searchResults.map((r, i) => `
    <div class="search-result${i === selectedIndex ? ' active' : ''}" data-path="${escapeAttr(r.path)}" data-index="${i}">
      <div class="search-result__name">${escapeHtml(r.name)}</div>
      <div class="search-result__path">${escapeHtml(r.path)}</div>
      ${r.preview ? `<div class="search-result__preview">${escapeHtml(r.preview)}</div>` : ''}
    </div>
  `).join('')

  if (footer) {
    footer.textContent = `${searchResults.length} result${searchResults.length === 1 ? '' : 's'}`
  }
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
