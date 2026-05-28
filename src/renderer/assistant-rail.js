import { getSettings } from './settings.js'
import { buildAiChatPanel, mountAiChatPanel } from './ai-chat.js'

function isRailDock(dock) {
  return dock === 'right-rail' || dock === 'left-rail'
}

export function buildAssistantRail() {
  return `
    <aside class="assistant-rail" id="assistant-rail" aria-label="AI assistant">
      <div class="assistant-rail__header">
        <span>Assistant</span>
        <span class="assistant-rail__mode" id="assistant-rail-mode">AI</span>
      </div>
      <div class="assistant-rail__body" id="assistant-rail-body"></div>
    </aside>
  `
}

export function syncAssistantRail() {
  const rail = document.getElementById('assistant-rail')
  if (!rail) return
  const dock = getSettings().assistantDock
  rail.classList.toggle('open', isRailDock(dock))
  rail.classList.toggle('assistant-rail--left', dock === 'left-rail')
  rail.classList.toggle('assistant-rail--right', dock !== 'left-rail')
}

export function mountAssistantRail() {
  const body = document.getElementById('assistant-rail-body')
  if (!body) {
    syncAssistantRail()
    return
  }

  const dock = getSettings().assistantDock
  if (!isRailDock(dock)) {
    body.replaceChildren()
    syncAssistantRail()
    return
  }

  if (body.firstChild) {
    syncAssistantRail()
    return
  }

  body.innerHTML = buildAiChatPanel()
  mountAiChatPanel()
  syncAssistantRail()
}
