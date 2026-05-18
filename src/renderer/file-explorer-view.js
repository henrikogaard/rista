import { state, $ } from './state.js'

let _callbacks = {}

export function registerFileExplorerCallbacks(cbs) {
  Object.assign(_callbacks, cbs)
}

export function fileExplorerHeaderActions() {
  // Rendered into the widget header's right-side actions slot.
  return `
    <button type="button" class="widget__action" id="file-explorer-collapse-all" title="Collapse all" aria-label="Collapse all" ${state.folderPath ? '' : 'hidden'}>
      <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true"><path d="M3 6l3-3 3 3M3 10l3 3 3-3" stroke="currentColor" fill="none" stroke-linecap="round"/></svg>
    </button>
  `
}

export function buildFileExplorerPanel() {
  const hasFolder = Boolean(state.folderPath)
  return `
    <div class="file-explorer">
      <div class="file-explorer__open-btn" id="open-folder-btn" role="button" tabindex="0" ${hasFolder ? 'hidden' : ''}>
        <svg viewBox="0 0 16 16"><path d="M2 5h4l2-2h6a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/></svg>
        Open folder…
      </div>
      <div class="file-tree" id="file-tree"></div>
    </div>
  `
}

let _collapseAllWired = false
export function mountFileExplorerPanel() {
  refreshFileExplorerState()
  const openBtn = $('open-folder-btn')
  openBtn?.addEventListener('click', () => _callbacks.openFolder?.())
  openBtn?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); _callbacks.openFolder?.() }
  })
  // Collapse-all button lives in the widget header (rendered by right-panel)
  // so wire it via document-level delegation, once.
  if (!_collapseAllWired) {
    document.addEventListener('click', onCollapseAllClick)
    _collapseAllWired = true
  }
  _callbacks.renderTree?.()
}

function onCollapseAllClick(event) {
  const btn = event.target.closest('#file-explorer-collapse-all')
  if (!btn) return
  event.preventDefault()
  event.stopPropagation()
  _callbacks.collapseAllFolders?.()
}

export function refreshFileExplorerState() {
  const openBtn = $('open-folder-btn')
  if (openBtn) openBtn.hidden = Boolean(state.folderPath)
  const collapseBtn = $('file-explorer-collapse-all')
  if (collapseBtn) collapseBtn.hidden = !state.folderPath
}
