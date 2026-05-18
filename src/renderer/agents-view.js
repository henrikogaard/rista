import { renderAgentsList } from './agents-sidebar.js'

let _callbacks = {}

export function registerAgentsViewCallbacks(cbs) {
  Object.assign(_callbacks, cbs)
}

export function buildAgentsPanel() {
  return `
    <div class="agents-view">
      <div class="agents-view__toolbar">
        <span class="agents-view__label">Sessions</span>
        <div class="agents-view__new" id="agents-new-btn" title="New session" role="button" tabindex="0">+</div>
      </div>
      <div class="agents-list" id="agents-list"></div>
      <div class="agents-empty" id="agents-empty">
        <span>No sessions yet</span>
        <p class="agents-empty__sub">Sessions are stored locally in your project.</p>
      </div>
    </div>
  `
}

export function mountAgentsPanel() {
  renderAgentsList()
  const newBtn = document.getElementById('agents-new-btn')
  newBtn?.addEventListener('click', () => _callbacks.createSession?.())
  newBtn?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); _callbacks.createSession?.() }
  })
}

export function refreshAgentsPanel() {
  renderAgentsList()
}
