import { state, getFocusedEditor, getFocusedTab } from './state.js'
import { getSettings } from './settings.js'
import { showContextMenu } from './context-menu.js'
import { searchFiles } from './link-index.js'
import { PROVIDERS } from './ai-providers.js'

const AI_ACTIONS = [
  { id: 'summarize', label: 'AI: Summarize', instruction: 'Summarize the following text concisely:' },
  { id: 'expand', label: 'AI: Expand', instruction: 'Expand on the following text with more detail and examples:' },
  { id: 'rewrite', label: 'AI: Rewrite', instruction: 'Rewrite the following text to be clearer and more polished:' },
  { id: 'fix-grammar', label: 'AI: Fix Grammar', instruction: 'Fix any grammar, spelling, or punctuation errors in the following text. Return only the corrected text:' },
  { id: 'explain', label: 'AI: Explain', instruction: 'Explain the following text in simple terms:' },
]

function getProviderConfig(settings) {
  const providerKey = settings.aiProvider || 'openai'
  const provider = PROVIDERS[providerKey]
  if (!provider) return null
  return {
    providerKey,
    provider,
    model: settings.aiModel || provider.defaultModel || '',
    baseUrl: settings.aiBaseUrl || provider.defaultBaseUrl || '',
  }
}

function cliActionMessage(provider) {
  return `${provider.label} discovery is available in Settings. Action transport will be wired in a later pass.`
}

export function showAiContextMenu(x, y, view) {
  const selection = view.state.sliceDoc(
    view.state.selection.main.from,
    view.state.selection.main.to
  )
  if (!selection.trim()) return

  const items = AI_ACTIONS.map(action => ({
    label: action.label,
    action: () => runAiAction(action, selection, view),
  }))

  showContextMenu(x, y, items)
}

async function runAiAction(action, selectedText, view) {
  const settings = getSettings()
  const config = getProviderConfig(settings)
  if (!config) return
  if (config.provider.transport === 'cli') {
    alert(cliActionMessage(config.provider))
    return
  }
  if (!settings.aiApiKey && !config.provider.noApiKey) {
    alert('Please set an API key in Settings → AI')
    return
  }

  const messages = [
    { role: 'user', content: `${action.instruction}\n\n${selectedText}` },
  ]

  try {
    const result = await window.fjord.aiChat({
      provider: config.providerKey,
      apiKey: settings.aiApiKey,
      model: config.model,
      baseUrl: config.baseUrl,
      messages,
    })
    if (result?.error) {
      alert('AI error: ' + result.error)
      return
    }
    const text = result?.text
    if (!text) return

    const { from, to } = view.state.selection.main
    if (action.id === 'explain') {
      view.dispatch({ changes: { from: to, to, insert: '\n\n> ' + text.replace(/\n/g, '\n> ') } })
    } else {
      view.dispatch({
        changes: { from, to, insert: text },
        selection: { anchor: from + text.length },
      })
    }
  } catch (err) {
    alert('AI error: ' + (err.message || 'Request failed'))
  }
}

export async function askNotesRag(question) {
  const settings = getSettings()
  const config = getProviderConfig(settings)
  if (!config) return { error: 'Unknown AI provider' }
  if (config.provider.transport === 'cli') {
    return { error: cliActionMessage(config.provider) }
  }
  if (!settings.aiApiKey && !config.provider.noApiKey) {
    return { error: 'Please set an API key in Settings → AI' }
  }

  const results = searchFiles(question, { limit: 5 })
  const context = results
    .filter(r => r.contentMatch || r.nameMatch)
    .map(r => `--- ${r.name} ---\n${r.preview || ''}`)
    .join('\n\n')

  const messages = [
    { role: 'system', content: `You are a knowledge assistant. Answer the user's question based on these notes from their wiki:\n\n${context}\n\nIf the notes don't contain relevant information, say so.` },
    { role: 'user', content: question },
  ]

  try {
    return await window.fjord.aiChat({
      provider: config.providerKey,
      apiKey: settings.aiApiKey,
      model: config.model,
      baseUrl: config.baseUrl,
      messages,
    })
  } catch (err) {
    return { error: err.message || 'Request failed' }
  }
}
