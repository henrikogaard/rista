import { state, fileName } from './state.js'

const _items = []
const _listeners = new Set()
let _executeTool = null
let _openFile = null

export function registerAiReviewCallbacks({ executeTool, openFile }) {
  _executeTool = executeTool
  _openFile = openFile
}

export function onAiReviewChange(fn) {
  _listeners.add(fn)
  return () => _listeners.delete(fn)
}

function emitChange() {
  for (const fn of _listeners) {
    try { fn(_items) } catch {}
  }
}

function nowId() {
  return `review-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
}

function lineCount(text = '') {
  return String(text || '').split('\n').length
}

async function buildPreviewForTool(name, input) {
  const targetPath = input?.path || input?.from || ''
  if (name === 'write_file') {
    const existing = await window.fjord.readFile(joinProjectPath(input.path))
    const oldLines = lineCount(existing || '')
    const newLines = lineCount(input.content || '')
    const added = Math.max(0, newLines - oldLines)
    const removed = Math.max(0, oldLines - newLines)
    return {
      path: input.path,
      summary: `Write ${input.path}`,
      added,
      removed,
      preview: buildWritePreview(existing || '', input.content || ''),
    }
  }
  if (name === 'move_file') {
    return {
      path: input.from,
      summary: `Move ${input.from} -> ${input.to}`,
      added: 0,
      removed: 0,
      preview: `${input.from}\n→ ${input.to}`,
    }
  }
  if (name === 'delete_file') {
    return {
      path: input.path,
      summary: `Delete ${input.path}`,
      added: 0,
      removed: 0,
      preview: input.path,
    }
  }
  return {
    path: targetPath,
    summary: `${name} ${targetPath}`.trim(),
    added: 0,
    removed: 0,
    preview: JSON.stringify(input || {}, null, 2),
  }
}

function buildWritePreview(before, after) {
  const beforeLines = String(before || '').split('\n')
  const afterLines = String(after || '').split('\n')
  const out = []
  const max = Math.max(beforeLines.length, afterLines.length)
  for (let i = 0; i < max; i++) {
    const b = beforeLines[i]
    const a = afterLines[i]
    if (b === a) continue
    if (b !== undefined) out.push(`- ${b}`)
    if (a !== undefined) out.push(`+ ${a}`)
    if (out.length >= 18) break
  }
  return out.join('\n') || '(no visible line changes)'
}

function joinProjectPath(rel = '') {
  const root = state.folderPath || ''
  const path = String(rel || '')
  if (!path || !root) return path
  if (path.startsWith('/') || /^[a-z]:\\/i.test(path)) return path
  const sep = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  return `${root}${sep}${path.replace(/^[/\\]+/, '')}`
}

export async function queueAiReviewItem({ toolName, toolInput, source = 'ai-chat', sourceNotes = [] }) {
  const details = await buildPreviewForTool(toolName, toolInput || {})
  const item = {
    id: nowId(),
    toolName,
    toolInput: toolInput || {},
    source,
    sourceNotes,
    createdAt: Date.now(),
    status: 'pending',
    ...details,
  }
  _items.unshift(item)
  emitChange()
  return item
}

export function getAiReviewItems() {
  return _items
}

export async function acceptAiReviewItem(id) {
  const item = _items.find(entry => entry.id === id)
  if (!item || item.status !== 'pending' || !_executeTool) return false
  const conflictTab = findConflictingReviewTab(item)
  if (conflictTab) {
    item.error = `${conflictTab.name || item.path} has unsaved or external changes`
    emitChange()
    return false
  }
  item.status = 'applying'
  item.error = ''
  emitChange()
  try {
    await _executeTool(item.toolName, item.toolInput)
    item.status = 'accepted'
    emitChange()
    return true
  } catch {
    item.status = 'pending'
    emitChange()
    return false
  }
}

function findConflictingReviewTab(item) {
  const paths = [
    item.toolInput?.path,
    item.toolInput?.from,
    item.toolInput?.to,
    item.path,
  ].filter(Boolean).map(joinProjectPath)
  return state.tabs.find(tab => paths.includes(tab.path) && (tab.dirty || tab.externalConflict))
}

export function rejectAiReviewItem(id) {
  const item = _items.find(entry => entry.id === id)
  if (!item || item.status !== 'pending') return false
  item.status = 'rejected'
  emitChange()
  return true
}

export async function acceptAllAiReviewItems() {
  const pending = _items.filter(item => item.status === 'pending')
  for (const item of pending) await acceptAiReviewItem(item.id)
}

export function rejectAllAiReviewItems() {
  _items.forEach(item => {
    if (item.status === 'pending') item.status = 'rejected'
  })
  emitChange()
}

export function openAiReviewItem(itemId) {
  const item = _items.find(entry => entry.id === itemId)
  if (!item || !_openFile) return
  const targetPath = joinProjectPath(item.toolInput?.path || item.toolInput?.from)
  if (!targetPath) return
  _openFile({
    path: targetPath,
    name: fileName(targetPath),
  })
}
