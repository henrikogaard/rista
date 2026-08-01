import { showStatusNotice } from './tabs.js'
import { state, getFocusedEditor, getFocusedTab } from './state.js'
import { getSettings } from './settings.js'
import { showContextMenu } from './context-menu.js'
import { searchFiles } from './link-index.js'
import { PROVIDERS } from './ai-providers.js'
import { queueAiReviewItem } from './ai-review.js'
import { saveActive } from './tabs.js'

const SOURCE_AWARE_AI_ACTIONS = [
  { id: 'summarize', label: 'AI: Summarize', mode: 'replace', instruction: 'Summarize the source excerpt concisely. Return only Markdown that should replace the selection.' },
  { id: 'extract-tasks', label: 'AI: Extract Tasks', mode: 'replace', instruction: 'Extract concrete tasks from the source excerpt. Return only a Markdown task list.' },
  { id: 'rewrite', label: 'AI: Rewrite', mode: 'replace', instruction: 'Rewrite the source excerpt to be clearer and more polished. Return only the replacement Markdown.' },
  { id: 'fix-grammar', label: 'AI: Fix Grammar', mode: 'replace', instruction: 'Fix grammar, spelling, and punctuation in the source excerpt. Return only the corrected Markdown.' },
  { id: 'explain', label: 'AI: Explain', mode: 'append-quote', instruction: 'Explain the source excerpt in simple terms. Return only the explanation Markdown.' },
  { id: 'suggest-tags', label: 'AI: Suggest Tags', mode: 'append', instruction: 'Suggest concise tags for the source excerpt. Return only Markdown with a short heading and tag list.' },
  { id: 'suggest-aliases', label: 'AI: Suggest Aliases', mode: 'append', instruction: 'Suggest useful aliases for the source excerpt. Return only Markdown with a short heading and aliases.' },
  { id: 'suggest-properties', label: 'AI: Suggest Properties', mode: 'append', instruction: 'Suggest YAML properties for the source excerpt. Return only a fenced yaml block and a short note.' },
]
const AI_ACTIONS = SOURCE_AWARE_AI_ACTIONS

function getProviderConfig(settings) {
  const providerKey = settings.aiProvider || 'openai'
  const provider = PROVIDERS[providerKey]
  if (!provider) return null
  return {
    providerKey,
    provider,
    apiKey: provider.apiKey ? settings.aiApiKey : '',
    model: settings.aiModel || provider.defaultModel || '',
    baseUrl: settings.aiBaseUrl || provider.defaultBaseUrl || '',
  }
}

function cliActionMessage(provider) {
  return `${provider.label} discovery is available in Settings. Action transport will be wired in a later pass.`
}

export function showAiContextMenu(x, y, view) {
  const range = {
    from: view.state.selection.main.from,
    to: view.state.selection.main.to,
  }
  const selection = view.state.sliceDoc(range.from, range.to)
  if (!selection.trim()) return

  const items = AI_ACTIONS.map(action => ({
    label: action.label,
    action: () => runAiAction(action, selection, view, range),
  }))

  showContextMenu(x, y, items)
}

async function runAiAction(action, selectedText, view, range) {
  const settings = getSettings()
  const config = getProviderConfig(settings)
  if (!config) return
  if (config.provider.transport === 'cli') {
    showStatusNotice(cliActionMessage(config.provider), 'info')
    return
  }
  if (!settings.aiApiKey && !config.provider.noApiKey) {
    showStatusNotice('Please set an API key in Settings → AI', 'error')
    return
  }

  const tab = getFocusedTab()
  const relativePath = relativeProjectPath(tab?.path)
  if (!tab?.path || !relativePath) {
    showStatusNotice('Open a project note before running source-aware AI actions.', 'error')
    return
  }

  const sourceContext = buildSourceContext(selectedText, view, range)
  const messages = [
    {
      role: 'system',
      content: 'You are editing a local Markdown note. Use only the supplied source excerpt. Do not invent citations or hidden context.',
    },
    { role: 'user', content: `${action.instruction}\n\n${sourceContext.prompt}` },
  ]

  try {
    const result = await window.fjord.aiChat({
      provider: config.providerKey,
      apiKey: config.apiKey,
      model: config.model,
      baseUrl: config.baseUrl,
      messages,
    })
    if (result?.error) {
      showStatusNotice('AI error: ' + result.error, 'error')
      return
    }
    const text = result?.text
    if (!text) return

    const currentMarkdown = view.state.doc.toString()
    const nextMarkdown = buildReviewedMarkdown(action, currentMarkdown, text, range)
    await saveActive()
    const reviewItem = await queueAiReviewItem({
      toolName: 'write_file',
      toolInput: { path: relativePath, content: nextMarkdown },
      source: action.label,
      sourceNotes: sourceContext.sourceNotes,
    })
    showStatusNotice(`Queued for review: ${reviewItem.summary}`, 'success')
  } catch (err) {
    showStatusNotice('AI error: ' + (err.message || 'Request failed'), 'error')
  }
}

function buildSourceContext(selectedText, view, range) {
  const tab = getFocusedTab()
  const fromLine = view.state.doc.lineAt(range.from)
  const toLine = view.state.doc.lineAt(range.to)
  const rel = relativeProjectPath(tab?.path) || tab?.name || 'Untitled'
  return {
    prompt: [
      `Source note: ${rel}`,
      `Source lines: ${fromLine.number}-${toLine.number}`,
      '',
      selectedText,
    ].join('\n'),
    sourceNotes: [{
      path: tab?.path || '',
      name: tab?.name || rel,
      relativePath: rel,
      range: `L${fromLine.number}-L${toLine.number}`,
    }],
  }
}

function buildReviewedMarkdown(action, markdown, generatedText, range) {
  const text = String(generatedText || '').trim()
  const before = markdown.slice(0, range.from)
  const selected = markdown.slice(range.from, range.to)
  const after = markdown.slice(range.to)
  if (action.mode === 'append-quote') {
    return `${before}${selected}\n\n> ${text.replace(/\n/g, '\n> ')}${after}`
  }
  if (action.mode === 'append') {
    return `${before}${selected}\n\n${text}${after}`
  }
  return `${before}${text}${after}`
}

function relativeProjectPath(path) {
  if (!path || !state.folderPath) return ''
  const normalizedPath = String(path).replace(/\\/g, '/')
  const normalizedRoot = String(state.folderPath).replace(/\\/g, '/').replace(/\/+$/, '')
  if (!normalizedPath.startsWith(normalizedRoot)) return ''
  return normalizedPath.slice(normalizedRoot.length).replace(/^\/+/, '')
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
      apiKey: config.apiKey,
      model: config.model,
      baseUrl: config.baseUrl,
      messages,
    })
  } catch (err) {
    return { error: err.message || 'Request failed' }
  }
}
