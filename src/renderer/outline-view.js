import { extractHeadings } from './markdown.js'
import { state, $, editorViews, getFocusedTab, escapeHtml } from './state.js'
import { outlineIcon } from './icons.js'


let _cachedHeadings = []
let _activeLine = null

export function buildOutlinePanel() {
  return `<div class="outline-view" id="outline-view-body"></div>`
}

export function renderOutline() {
  const body = $('outline-view-body')
  if (!body) return
  // Re-attach scroll listeners — after editor rebuilds (folder switch, session
  // restore), the old scrollDOMs are gone and our WeakSet has no record of
  // the new ones. attachScrollSync is idempotent thanks to the WeakSet.
  attachScrollSync()
  const tab = getFocusedTab()
  if (!tab) {
    _cachedHeadings = []
    body.innerHTML = `<div class="outline-view__empty"><span class="outline-view__empty-icon">${outlineIcon()}</span><span>No note open</span></div>`
    return
  }
  _cachedHeadings = extractHeadings(tab.content || '')
  if (_cachedHeadings.length === 0) {
    body.innerHTML = `<div class="outline-view__empty"><span class="outline-view__empty-icon">${outlineIcon()}</span><span>No headings in this note</span></div>`
    return
  }
  const minLevel = Math.min(..._cachedHeadings.map(h => h.level))
  const html = _cachedHeadings.map(h => {
    const indent = (h.level - minLevel)
    return `<div class="outline-item outline-item--l${h.level}" data-line="${h.line}" style="padding-left:${indent * 12 + 8}px" role="button" tabindex="0" title="${escapeHtml(h.text)}"><span class="outline-item__bullet"></span><span class="outline-item__text">${escapeHtml(h.text)}</span></div>`
  }).join('')
  body.innerHTML = html
  updateActiveOutlineItem()
}

function findActiveHeadingLine() {
  if (_cachedHeadings.length === 0) return null
  const pane = state.focusedPane || 'primary'
  const view = editorViews[pane]
  if (!view) return null
  // Use whichever line is closer to the top of the viewport (1-indexed)
  let topLine = 1
  try {
    const blockTop = view.lineBlockAtHeight(view.scrollDOM.scrollTop + 8)
    if (blockTop) topLine = view.state.doc.lineAt(blockTop.from).number
  } catch { return null }
  // The active heading is the last one at or before topLine
  let active = _cachedHeadings[0]?.line || null
  for (const h of _cachedHeadings) {
    if (h.line <= topLine) active = h.line
    else break
  }
  return active
}

function updateActiveOutlineItem() {
  const body = $('outline-view-body')
  if (!body) return
  const nextLine = findActiveHeadingLine()
  if (nextLine === _activeLine) return
  _activeLine = nextLine
  body.querySelectorAll('.outline-item.active').forEach(n => n.classList.remove('active'))
  if (_activeLine == null) return
  const el = body.querySelector(`.outline-item[data-line="${_activeLine}"]`)
  if (el) {
    el.classList.add('active')
    // Keep the active heading in view without jerking the page
    const rect = el.getBoundingClientRect()
    const parentRect = body.getBoundingClientRect()
    if (rect.top < parentRect.top || rect.bottom > parentRect.bottom) {
      el.scrollIntoView({ block: 'nearest' })
    }
  }
}

export function jumpToLine(lineNumber) {
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

let _scrollListenersAttached = new WeakSet()
let _scrollTickPending = false

function attachScrollSync() {
  for (const pane of ['primary', 'secondary']) {
    const view = editorViews[pane]
    if (!view || _scrollListenersAttached.has(view.scrollDOM)) continue
    const onScroll = () => {
      if (_scrollTickPending) return
      _scrollTickPending = true
      requestAnimationFrame(() => {
        _scrollTickPending = false
        updateActiveOutlineItem()
      })
    }
    view.scrollDOM.addEventListener('scroll', onScroll, { passive: true })
    _scrollListenersAttached.add(view.scrollDOM)
  }
}

export function mountOutlinePanel() {
  renderOutline()
  attachScrollSync()
  const body = $('outline-view-body')
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
