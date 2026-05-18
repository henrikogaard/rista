import { extractHeadings } from './markdown.js'
import { state, editorViews, getFocusedTab } from './state.js'

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

export function buildOutlinePanel() {
  return `<div class="outline-view" id="outline-view-body"></div>`
}

export function renderOutline() {
  const body = document.getElementById('outline-view-body')
  if (!body) return
  const tab = getFocusedTab()
  if (!tab) {
    body.innerHTML = `<div class="outline-view__empty">No note open</div>`
    return
  }
  const headings = extractHeadings(tab.content || '')
  if (headings.length === 0) {
    body.innerHTML = `<div class="outline-view__empty">No headings in this note</div>`
    return
  }
  const minLevel = Math.min(...headings.map(h => h.level))
  const html = headings.map(h => {
    const indent = (h.level - minLevel)
    return `<div class="outline-item outline-item--l${h.level}" data-line="${h.line}" style="padding-left:${indent * 12 + 8}px" role="button" tabindex="0" title="${escapeHtml(h.text)}"><span class="outline-item__bullet"></span><span class="outline-item__text">${escapeHtml(h.text)}</span></div>`
  }).join('')
  body.innerHTML = html
}

function jumpToLine(lineNumber) {
  const pane = state.focusedPane || 'primary'
  const view = editorViews[pane]
  if (!view) return
  const doc = view.state.doc
  if (lineNumber < 1 || lineNumber > doc.lines) return
  const line = doc.line(lineNumber)
  view.dispatch({
    selection: { anchor: line.from, head: line.from },
    effects: [],
    scrollIntoView: true,
  })
  // Center the heading in the viewport for nicer feel
  requestAnimationFrame(() => {
    try {
      const block = view.lineBlockAt(line.from)
      const target = block.top - view.scrollDOM.clientHeight / 2 + block.height / 2
      view.scrollDOM.scrollTop = Math.max(0, target)
    } catch {}
    view.focus()
  })
}

export function mountOutlinePanel() {
  renderOutline()
  const body = document.getElementById('outline-view-body')
  if (!body) return
  body.addEventListener('click', (event) => {
    const item = event.target.closest('.outline-item')
    if (!item) return
    const line = Number(item.dataset.line || 0)
    if (line > 0) jumpToLine(line)
  })
  body.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' && event.key !== ' ') return
    const item = event.target.closest('.outline-item')
    if (!item) return
    event.preventDefault()
    const line = Number(item.dataset.line || 0)
    if (line > 0) jumpToLine(line)
  })
}
