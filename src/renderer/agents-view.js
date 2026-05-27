import { renderAgentsList } from './agents-sidebar.js'

let _callbacks = {}

export function registerAgentsViewCallbacks(cbs) {
  Object.assign(_callbacks, cbs)
}

export function agentsViewHeaderActions() {
  return `<button type="button" class="widget__action" id="agents-new-btn" title="New session" aria-label="New session">+</button>`
}

export function buildAgentsPanel() {
  return `
    <div class="agents-view">
      <div class="agents-list" id="agents-list"></div>
      <div class="agents-empty" id="agents-empty">
        <span>No sessions yet</span>
        <p class="agents-empty__sub">Sessions are stored locally in your project.</p>
      </div>
    </div>
  `
}

let _newBtnWired = false
export function mountAgentsPanel() {
  renderAgentsList()
  if (!_newBtnWired) {
    document.addEventListener('click', onNewBtnClick)
    _newBtnWired = true
  }
}

function onNewBtnClick(event) {
  const btn = event.target.closest('#agents-new-btn')
  if (!btn) return
  event.preventDefault()
  event.stopPropagation()
  _callbacks.createSession?.()
}

document.addEventListener('click', (event) => {
  const card = event.target.closest('.agent-card[data-session-path]')
  if (!card) return
  _callbacks.openSession?.(card.dataset.sessionPath)
})

document.addEventListener('keydown', (event) => {
  if (event.key !== 'Enter' && event.key !== ' ') return
  const card = event.target.closest('.agent-card[data-session-path]')
  if (!card) return
  event.preventDefault()
  _callbacks.openSession?.(card.dataset.sessionPath)
})

export function refreshAgentsPanel() {
  renderAgentsList()
}
