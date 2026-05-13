import { $, getFocusedEditor, state } from './state.js'

// ── Find & Replace ──────────────────────────────────────────────────
let findMatches = []
let currentMatchIndex = -1

export function toggleFindReplace() {
  const panel = $('find-replace')
  panel.classList.toggle('hidden')
  if (!panel.classList.contains('hidden')) {
    $('find-input').focus()
  }
}

export function updateFind() {
  const view = getFocusedEditor()
  if (!view) return
  const query = $('find-input').value
  if (!query) {
    findMatches = []
    currentMatchIndex = -1
    updateFindCount()
    return
  }

  const text = view.state.doc.toString()
  findMatches = []
  const regex = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi')
  let match
  while ((match = regex.exec(text)) !== null) {
    findMatches.push({ from: match.index, to: match.index + match[0].length })
  }
  currentMatchIndex = findMatches.length > 0 ? 0 : -1
  updateFindCount()
  highlightMatch()
}

function updateFindCount() {
  const count = $('find-count')
  if (findMatches.length === 0) {
    count.textContent = 'No results'
  } else {
    count.textContent = `${currentMatchIndex + 1}/${findMatches.length}`
  }
}

function highlightMatch() {
  const view = getFocusedEditor()
  if (!view) return
  if (currentMatchIndex < 0 || !findMatches[currentMatchIndex]) return
  const match = findMatches[currentMatchIndex]
  view.dispatch({
    selection: { anchor: match.from, head: match.to },
    scrollIntoView: true,
  })
}

export function findNext() {
  if (findMatches.length === 0) return
  currentMatchIndex = (currentMatchIndex + 1) % findMatches.length
  updateFindCount()
  highlightMatch()
}

export function findPrev() {
  if (findMatches.length === 0) return
  currentMatchIndex = (currentMatchIndex - 1 + findMatches.length) % findMatches.length
  updateFindCount()
  highlightMatch()
}

export function replaceOne(onEditorChange) {
  const view = getFocusedEditor()
  if (!view || currentMatchIndex < 0) return
  const match = findMatches[currentMatchIndex]
  const replacement = $('replace-input').value
  view.dispatch({
    changes: { from: match.from, to: match.to, insert: replacement },
  })
  onEditorChange(state.focusedPane, view.state.doc.toString())
  updateFind()
}

export function replaceAll(onEditorChange) {
  const view = getFocusedEditor()
  if (!view || findMatches.length === 0) return
  const replacement = $('replace-input').value
  const changes = findMatches.map(match => ({
    from: match.from,
    to: match.to,
    insert: replacement,
  })).reverse()

  view.dispatch({
    changes,
  })
  onEditorChange(state.focusedPane, view.state.doc.toString())
  updateFind()
}

export function handleFindKeydown(e) {
  if (e.key === 'Enter') findNext()
  if (e.key === 'Escape') toggleFindReplace()
}
