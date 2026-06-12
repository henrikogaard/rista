import { state } from './state.js'

let hintEl = null
let fadeTimer = null

// ── Build the exit hint (call once after buildShell) ─────────────
export function buildZenExitHint() {
  hintEl = document.createElement('div')
  hintEl.className = 'zen-exit-hint'
  hintEl.textContent = 'Esc to exit Zen Mode'
  hintEl.addEventListener('click', exitZenMode)
  const root = document.getElementById('root')
  if (root) root.appendChild(hintEl)
}

// ── Toggle ───────────────────────────────────────────────────────
export function toggleZenMode() {
  if (state.zenMode) exitZenMode()
  else enterZenMode()
}

// ── Enter ────────────────────────────────────────────────────────
export function enterZenMode() {
  state.zenMode = true
  document.documentElement.setAttribute('data-zen', 'true')
  window.fjord?.setFullscreen?.(true).catch(() => {})
  showHint()

  state._zenMouseHandler = () => showHint()
  document.addEventListener('mousemove', state._zenMouseHandler)
}

// ── Exit ─────────────────────────────────────────────────────────
export function exitZenMode() {
  if (!state.zenMode) return
  state.zenMode = false
  document.documentElement.removeAttribute('data-zen')
  window.fjord?.setFullscreen?.(false).catch(() => {})
  hideHint()

  if (state._zenMouseHandler) {
    document.removeEventListener('mousemove', state._zenMouseHandler)
    state._zenMouseHandler = null
  }
}

// ── Query ────────────────────────────────────────────────────────
export function isZenMode() {
  return state.zenMode
}

// ── Hint helpers ─────────────────────────────────────────────────
function showHint() {
  if (!hintEl) return
  if (fadeTimer) clearTimeout(fadeTimer)
  hintEl.style.opacity = '1'
  fadeTimer = setTimeout(() => {
    if (hintEl) hintEl.style.opacity = '0'
  }, 2000)
}

function hideHint() {
  if (!hintEl) return
  if (fadeTimer) clearTimeout(fadeTimer)
  hintEl.style.opacity = '0'
}
