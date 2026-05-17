import { state, $, getFocusedTab } from './state.js'
import { getSettings } from './settings.js'
import { PROVIDERS } from './ai-providers.js'
import { registerRightPanel } from './right-panel.js'

// ── AI Chat Panel ──────────────────────────────────────────────
let _closeRightPanelFn = null
let _messages = []  // { role: 'user'|'assistant', content: string }
let _sending = false

function getProviderConfig() {
  const s = getSettings()
  const providerKey = s.aiProvider || 'openai'
  const provider = PROVIDERS[providerKey]
  if (!provider) return null
  return {
    provider: providerKey,
    apiKey: s.aiApiKey || '',
    model: s.aiModel || provider.defaultModel,
    baseUrl: s.aiBaseUrl || provider.defaultBaseUrl,
    label: provider.label,
    noApiKey: provider.noApiKey || false,
  }
}

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function buildPanel() {
  const config = getProviderConfig()
  const modelLabel = config ? config.model : 'Not configured'

  return `
    <div class="ai-chat">
      <div class="ai-chat__model-bar">
        <span class="ai-chat__model">${escapeHtml(modelLabel)}</span>
        <span class="ai-chat__clear" id="ai-chat-clear" role="button" tabindex="0">Clear</span>
      </div>
      <div class="ai-chat__messages" id="ai-chat-messages"></div>
      <div class="ai-chat__input-area">
        <textarea class="ai-chat__textarea" id="ai-chat-input" placeholder="Ask about your note..." rows="1"></textarea>
        <button class="ai-chat__send" id="ai-chat-send" type="button">Send</button>
      </div>
    </div>
  `
}

function renderMessages() {
  const container = document.getElementById('ai-chat-messages')
  if (!container) return

  let html = ''
  for (const msg of _messages) {
    const cls = msg.role === 'user' ? 'ai-chat__msg--user' : 'ai-chat__msg--assistant'
    html += `<div class="ai-chat__msg ${cls}">${escapeHtml(msg.content)}</div>`
  }
  if (_sending) {
    html += `<div class="ai-chat__msg ai-chat__msg--typing">Thinking...</div>`
  }
  container.innerHTML = html
  container.scrollTop = container.scrollHeight
}

function getNoteContext() {
  const tab = getFocusedTab()
  if (!tab) return ''
  return tab.content || ''
}

async function sendMessage() {
  const input = document.getElementById('ai-chat-input')
  if (!input) return
  const text = input.value.trim()
  if (!text || _sending) return

  const config = getProviderConfig()
  if (!config) return
  if (!config.noApiKey && !config.apiKey) {
    _messages.push({ role: 'assistant', content: 'Please set your API key in Settings > AI.' })
    renderMessages()
    return
  }

  _messages.push({ role: 'user', content: text })
  input.value = ''
  input.style.height = 'auto'
  _sending = true
  renderMessages()
  updateSendButton()

  const noteContent = getNoteContext()
  const systemPrompt = `You are a writing assistant. The user is working on a markdown note. Here is the current note content:\n\n${noteContent}\n\nHelp them with their writing.`

  const apiMessages = [
    { role: 'system', content: systemPrompt },
    ..._messages,
  ]

  try {
    const result = await window.fjord.aiChat({
      provider: config.provider,
      apiKey: config.apiKey,
      model: config.model,
      baseUrl: config.baseUrl,
      messages: apiMessages,
    })
    if (result.error) {
      _messages.push({ role: 'assistant', content: `Error: ${result.error}` })
    } else {
      _messages.push({ role: 'assistant', content: result.text || '(empty response)' })
    }
  } catch (err) {
    _messages.push({ role: 'assistant', content: `Error: ${err.message}` })
  }

  _sending = false
  renderMessages()
  updateSendButton()
}

function updateSendButton() {
  const btn = document.getElementById('ai-chat-send')
  if (btn) btn.disabled = _sending
}

function onPanelOpen() {
  renderMessages()

  const clearBtn = document.getElementById('ai-chat-clear')
  clearBtn?.addEventListener('click', () => {
    _messages = []
    renderMessages()
  })

  const sendBtn = document.getElementById('ai-chat-send')
  sendBtn?.addEventListener('click', sendMessage)

  const input = document.getElementById('ai-chat-input')
  input?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      sendMessage()
    }
  })

  // Auto-resize textarea
  input?.addEventListener('input', () => {
    input.style.height = 'auto'
    input.style.height = Math.min(input.scrollHeight, 120) + 'px'
  })

  input?.focus()
}

export function initAiChatPanel(openFileFn, closeRightPanelFn) {
  _closeRightPanelFn = closeRightPanelFn
  registerRightPanel('ai-chat', {
    title: 'AI Chat',
    icon: aiChatWidgetIcon(),
    flex: 2,
    build: buildPanel,
    onMount: onPanelOpen,
    onUnmount: () => {},
    onRefresh: () => renderMessages(),
  })
}

function aiChatWidgetIcon() {
  return `<svg viewBox="0 0 16 16" width="11" height="11"><circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor"/><path d="M5 7l1.5 1.5L11 5" fill="none" stroke="currentColor" stroke-linecap="round"/></svg>`
}
