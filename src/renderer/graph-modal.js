import { state } from './state.js'
import { getLinkIndex } from './link-index.js'
import { buildGraphView, renderGraph, destroyGraph } from './graph-view.js'

import { $ } from './state.js'
// ── Graph Modal ──────────────────────────────────────────────────
let _graphOpen = false

export function buildGraphModal() {
  return `
    <div class="graph-modal-overlay" id="graph-modal-overlay"></div>
    <div class="graph-modal" id="graph-modal">
      <div class="graph-modal__header">
        <div class="graph-modal__title">Knowledge Graph</div>
        <div class="graph-modal__close" id="graph-modal-close" title="Close" role="button" tabindex="0">
          <svg viewBox="0 0 16 16" width="14" height="14"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/></svg>
        </div>
      </div>
      <div class="graph-modal__body" id="graph-modal-body"></div>
    </div>
  `
}

export function openGraphModal(openFileFn) {
  if (_graphOpen) return
  _graphOpen = true

  const overlay = $('graph-modal-overlay')
  const modal = $('graph-modal')
  const body = $('graph-modal-body')
  if (!overlay || !modal || !body) return

  overlay.classList.add('open')
  modal.classList.add('open')

  body.innerHTML = buildGraphView()
  const linkIndex = getLinkIndex()
  renderGraph(linkIndex, (path) => {
    openFileFn?.(path)
    closeGraphModal()
  })

  overlay.addEventListener('click', closeGraphModal, { once: true })
  $('graph-modal-close')?.addEventListener('click', closeGraphModal, { once: true })
}

export function closeGraphModal() {
  _graphOpen = false
  const overlay = $('graph-modal-overlay')
  const modal = $('graph-modal')
  if (overlay) overlay.classList.remove('open')
  if (modal) modal.classList.remove('open')
  destroyGraph()
}
