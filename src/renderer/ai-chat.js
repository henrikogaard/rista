import { state, $, getFocusedTab } from './state.js'
import { getSettings } from './settings.js'
import { PROVIDERS } from './ai-providers.js'
import { registerRightPanel } from './right-panel.js'
import { TOOLS, toolsForProvider, executeToolByName, getToolSpec } from './ai-tools.js'

// ── AI Chat Panel ──────────────────────────────────────────────
// Messages use a unified shape; the main-process IPC translates per provider.
//   { role: 'user' | 'assistant' | 'tool', content, toolCalls?, toolCallId? }
// We additionally render local-only "system" status entries (e.g. "Reading …")
// using role: '__status' which is excluded before sending.

let _closeRightPanelFn = null
let _messages = []
let _sending = false
const MAX_TOOL_TURNS = 10

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
        <textarea class="ai-chat__textarea" id="ai-chat-input" placeholder="Ask anything about this folder…" rows="1"></textarea>
        <button class="ai-chat__send" id="ai-chat-send" type="button">Send</button>
      </div>
    </div>
  `
}

function renderMessages() {
  const container = document.getElementById('ai-chat-messages')
  if (!container) return

  let html = ''
  const visible = _messages.filter(m => m.role !== 'tool' && !(m.role === 'assistant' && !m.content && !m.toolCalls?.length))
  if (visible.length === 0 && !_sending) {
    html = `<div class="ai-chat__empty">
      Ask anything about your notes.
      <div class="ai-chat__empty-hint">The assistant can read, search, write, move and delete files in this folder.</div>
      <div class="ai-chat__empty-hint">Enter to send · Shift+Enter for newline</div>
    </div>`
  } else {
    for (const msg of visible) {
      if (msg.role === '__status') {
        html += `<div class="ai-chat__status">${escapeHtml(msg.content)}</div>`
        continue
      }
      if (msg.role === 'user') {
        html += `<div class="ai-chat__msg ai-chat__msg--user">${escapeHtml(msg.content)}</div>`
        continue
      }
      // assistant: text + optional tool call markers
      if (msg.content) {
        html += `<div class="ai-chat__msg ai-chat__msg--assistant">${escapeHtml(msg.content)}</div>`
      }
      if (Array.isArray(msg.toolCalls)) {
        for (const tc of msg.toolCalls) {
          const summary = summarizeToolCall(tc)
          html += `<div class="ai-chat__tool" title="${escapeHtml(JSON.stringify(tc.input || {}, null, 2))}">
            <span class="ai-chat__tool-icon">⚒</span>
            <span class="ai-chat__tool-name">${escapeHtml(tc.name)}</span>
            <span class="ai-chat__tool-arg">${escapeHtml(summary)}</span>
          </div>`
        }
      }
    }
    if (_sending) {
      html += `<div class="ai-chat__msg ai-chat__msg--typing">Thinking</div>`
    }
  }
  container.innerHTML = html
  container.scrollTop = container.scrollHeight
}

function summarizeToolCall(tc) {
  const input = tc.input || {}
  if (tc.name === 'list_files') return input.folder || '(root)'
  if (tc.name === 'read_file' || tc.name === 'delete_file' || tc.name === 'create_folder') return input.path || ''
  if (tc.name === 'write_file') return `${input.path || ''} (${(input.content || '').length} chars)`
  if (tc.name === 'move_file') return `${input.from || ''} → ${input.to || ''}`
  if (tc.name === 'search_notes') return `"${input.query || ''}"`
  return JSON.stringify(input)
}

function pushStatus(content) {
  _messages.push({ role: '__status', content })
  renderMessages()
}

function buildSystemPrompt() {
  const tab = getFocusedTab()
  const focusedNote = tab
    ? `The user currently has "${tab.name}" open. Its full content:\n\n${tab.content || '(empty)'}`
    : 'The user does not currently have a note open.'
  const root = state.folderPath || '(no folder open)'
  return [
    'You are a writing assistant embedded in Rísta, a local-first markdown editor.',
    `The user has the folder "${root}" open as their project.`,
    'You can use the provided tools to read, search, write, move, and delete files inside this folder. Always inspect the project structure with list_files or search_notes before making destructive changes. Paths are RELATIVE to the project root.',
    'Never invent file paths. Confirm by listing or searching first when unsure.',
    'When writing markdown, preserve existing frontmatter if present.',
    '',
    focusedNote,
  ].join('\n')
}

// Returns whether the user approves the destructive call.
async function confirmDestructive(tc) {
  const summary = summarizeToolCall(tc)
  return window.confirm(`The AI wants to ${tc.name.replace('_', ' ')}:\n\n${summary}\n\nProceed?`)
}

async function runAgentLoop() {
  const config = getProviderConfig()
  if (!config) return
  const systemPrompt = buildSystemPrompt()
  const supportsTools = config.provider === 'anthropic' || config.provider === 'openai'
  const tools = supportsTools ? toolsForProvider(config.provider) : undefined

  for (let turn = 0; turn < MAX_TOOL_TURNS; turn++) {
    // Strip local-only status entries before sending
    const apiMessages = [
      { role: 'system', content: systemPrompt },
      ..._messages.filter(m => m.role !== '__status'),
    ]
    let result
    try {
      result = await window.fjord.aiChat({
        provider: config.provider,
        apiKey: config.apiKey,
        model: config.model,
        baseUrl: config.baseUrl,
        messages: apiMessages,
        tools,
      })
    } catch (err) {
      _messages.push({ role: 'assistant', content: `Error: ${err.message}` })
      return
    }
    if (result.error) {
      _messages.push({ role: 'assistant', content: `Error: ${result.error}` })
      return
    }

    const assistantMsg = { role: 'assistant', content: result.text || '' }
    if (Array.isArray(result.toolCalls) && result.toolCalls.length) {
      assistantMsg.toolCalls = result.toolCalls
    }
    _messages.push(assistantMsg)
    renderMessages()

    if (!assistantMsg.toolCalls || assistantMsg.toolCalls.length === 0) {
      // Done — no more tools requested
      if (!assistantMsg.content) {
        assistantMsg.content = '(empty response)'
        renderMessages()
      }
      return
    }

    // Execute each tool, append tool_result messages
    for (const tc of assistantMsg.toolCalls) {
      const spec = getToolSpec(tc.name)
      if (!spec) {
        _messages.push({ role: 'tool', toolCallId: tc.id, content: `Unknown tool: ${tc.name}`, isError: true })
        continue
      }
      if (spec.destructive) {
        const ok = await confirmDestructive(tc)
        if (!ok) {
          _messages.push({ role: 'tool', toolCallId: tc.id, content: 'User declined the action.', isError: true })
          pushStatus(`Declined: ${tc.name}`)
          continue
        }
      }
      pushStatus(`${tc.name}: ${summarizeToolCall(tc)}`)
      try {
        const out = await executeToolByName(tc.name, tc.input || {})
        _messages.push({ role: 'tool', toolCallId: tc.id, content: String(out ?? '') })
      } catch (err) {
        _messages.push({ role: 'tool', toolCallId: tc.id, content: `Error: ${err.message}`, isError: true })
      }
    }
    renderMessages()
  }

  _messages.push({ role: 'assistant', content: `(stopped after ${MAX_TOOL_TURNS} tool turns to prevent runaway)` })
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

  try {
    await runAgentLoop()
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
