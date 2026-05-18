import { state, $ } from './state.js'

let _callbacks = {}

export function registerFileExplorerCallbacks(cbs) {
  Object.assign(_callbacks, cbs)
}

export function buildFileExplorerPanel() {
  const hasFolder = Boolean(state.folderPath)
  return `
    <div class="file-explorer">
      <div class="file-explorer__toolbar">
        <span class="file-explorer__location" id="file-explorer-location">${hasFolder ? escapeHtml(folderName(state.folderPath)) : 'No folder open'}</span>
        <div class="file-explorer__actions">
          <div class="file-explorer__action" id="file-explorer-collapse-all" title="Collapse all" role="button" tabindex="0" ${hasFolder ? '' : 'hidden'}>
            <svg viewBox="0 0 16 16" width="11" height="11"><path d="M3 6l3-3 3 3M3 10l3 3 3-3" stroke="currentColor" fill="none" stroke-linecap="round"/></svg>
          </div>
        </div>
      </div>
      <div class="file-explorer__open-btn" id="open-folder-btn" role="button" tabindex="0" ${hasFolder ? 'hidden' : ''}>
        <svg viewBox="0 0 16 16"><path d="M2 5h4l2-2h6a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/></svg>
        Open folder…
      </div>
      <div class="file-tree" id="file-tree"></div>
    </div>
  `
}

export function mountFileExplorerPanel() {
  refreshFileExplorerState()
  const openBtn = $('open-folder-btn')
  openBtn?.addEventListener('click', () => _callbacks.openFolder?.())
  openBtn?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); _callbacks.openFolder?.() }
  })
  const collapseBtn = $('file-explorer-collapse-all')
  collapseBtn?.addEventListener('click', () => _callbacks.collapseAllFolders?.())
  collapseBtn?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); _callbacks.collapseAllFolders?.() }
  })
  _callbacks.renderTree?.()
}

export function refreshFileExplorerState() {
  const location = $('file-explorer-location')
  if (location) {
    location.textContent = state.folderPath ? folderName(state.folderPath) : 'No folder open'
  }
  const openBtn = $('open-folder-btn')
  if (openBtn) openBtn.hidden = Boolean(state.folderPath)
  const collapseBtn = $('file-explorer-collapse-all')
  if (collapseBtn) collapseBtn.hidden = !state.folderPath
}

function folderName(p) {
  if (!p) return ''
  return p.split(/[/\\]/).filter(Boolean).pop() || p
}

function escapeHtml(value = '') {
  return String(value).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}
