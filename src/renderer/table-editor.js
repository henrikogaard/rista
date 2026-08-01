// ── Table Editor ─────────────────────────────────────────────────
// Floating toolbar for editing markdown tables in CodeMirror.

let _toolbarEl = null
let _activeView = null
let _tableRange = null

export function isInsideTable(state, pos) {
  const line = state.doc.lineAt(pos)
  return /^\s*\|/.test(line.text)
}

export function getTableRange(state, pos) {
  const doc = state.doc
  const startLine = doc.lineAt(pos)
  if (!/^\s*\|/.test(startLine.text)) return null

  let from = startLine.number
  while (from > 1 && /^\s*\|/.test(doc.line(from - 1).text)) from--

  let to = startLine.number
  while (to < doc.lines && /^\s*\|/.test(doc.line(to + 1).text)) to++

  return { from: doc.line(from).from, to: doc.line(to).to, fromLine: from, toLine: to }
}

export function parseMarkdownTable(text) {
  const lines = text.split('\n').filter(l => l.trim())
  if (lines.length < 2) return null
  const parseRow = line => line.replace(/^\s*\|/, '').replace(/\|\s*$/, '').split('|').map(c => c.trim())
  const headers = parseRow(lines[0])
  const dividerLine = lines[1]
  const alignments = parseRow(dividerLine).map(cell => {
    if (/^:-+:$/.test(cell)) return 'center'
    if (/^-+:$/.test(cell)) return 'right'
    return 'left'
  })
  const rows = lines.slice(2).map(parseRow)
  return { headers, alignments, rows }
}

export function serializeTable({ headers, alignments, rows }) {
  const divider = alignments.map((a, i) => {
    const w = Math.max(3, (headers[i] || '').length)
    const d = '-'.repeat(w)
    if (a === 'center') return ':' + d.slice(1, -1) + ':'
    if (a === 'right') return d.slice(0, -1) + ':'
    return d
  })
  const formatRow = cells => '| ' + cells.map((c, i) => (c || '').padEnd(Math.max(3, (headers[i] || '').length))).join(' | ') + ' |'
  return [formatRow(headers), '| ' + divider.join(' | ') + ' |', ...rows.map(formatRow)].join('\n')
}

function applyTableEdit(view, range, editFn) {
  const text = view.state.doc.sliceString(range.from, range.to)
  const table = parseMarkdownTable(text)
  if (!table) return
  editFn(table)
  const newText = serializeTable(table)
  view.dispatch({ changes: { from: range.from, to: range.to, insert: newText } })
}

export function addRow(view, range) {
  applyTableEdit(view, range, table => {
    table.rows.push(table.headers.map(() => ''))
  })
}

export function addColumn(view, range) {
  applyTableEdit(view, range, table => {
    table.headers.push('col')
    table.alignments.push('left')
    table.rows.forEach(r => r.push(''))
  })
}

export function deleteRow(view, range) {
  const line = view.state.doc.lineAt(view.state.selection.main.head)
  const text = view.state.doc.sliceString(range.from, range.to)
  const lines = text.split('\n')
  const lineIdx = line.number - view.state.doc.lineAt(range.from).number
  applyTableEdit(view, range, table => {
    const dataIdx = lineIdx - 2
    if (dataIdx >= 0 && dataIdx < table.rows.length) table.rows.splice(dataIdx, 1)
  })
}

export function deleteColumn(view, range) {
  const head = view.state.selection.main.head
  const line = view.state.doc.lineAt(head)
  const beforeCursor = line.text.slice(0, head - line.from)
  const colIdx = (beforeCursor.match(/\|/g) || []).length - 1
  if (colIdx < 0) return
  applyTableEdit(view, range, table => {
    if (table.headers.length <= 1) return
    table.headers.splice(colIdx, 1)
    table.alignments.splice(colIdx, 1)
    table.rows.forEach(r => r.splice(colIdx, 1))
  })
}

export function showTableToolbar(view, range) {
  hideTableToolbar()
  _activeView = view
  _tableRange = range

  const toolbar = document.createElement('div')
  toolbar.className = 'table-toolbar'
  toolbar.innerHTML = `
    <div class="table-toolbar__btn" data-action="add-row">+ Row</div>
    <div class="table-toolbar__btn" data-action="add-col">+ Col</div>
    <div class="table-toolbar__sep"></div>
    <div class="table-toolbar__btn" data-action="del-row">- Row</div>
    <div class="table-toolbar__btn" data-action="del-col">- Col</div>
    <div class="table-toolbar__sep"></div>
    <div class="table-toolbar__btn" data-action="align-left">L</div>
    <div class="table-toolbar__btn" data-action="align-center">C</div>
    <div class="table-toolbar__btn" data-action="align-right">R</div>
  `
  toolbar.addEventListener('click', e => {
    const btn = e.target.closest('[data-action]')
    if (!btn || !_activeView || !_tableRange) return
    const r = getTableRange(_activeView.state, _activeView.state.selection.main.head)
    if (!r) return
    const action = btn.dataset.action
    if (action === 'add-row') addRow(_activeView, r)
    if (action === 'add-col') addColumn(_activeView, r)
    if (action === 'del-row') deleteRow(_activeView, r)
    if (action === 'del-col') deleteColumn(_activeView, r)
    if (action === 'align-left') toggleColumnAlignment(_activeView, r, 'left')
    if (action === 'align-center') toggleColumnAlignment(_activeView, r, 'center')
    if (action === 'align-right') toggleColumnAlignment(_activeView, r, 'right')
    _tableRange = getTableRange(_activeView.state, _activeView.state.selection.main.head)
  })

  const coords = view.coordsAtPos(range.from)
  if (coords) {
    const editorRect = view.dom.getBoundingClientRect()
    toolbar.style.left = `${coords.left - editorRect.left}px`
    toolbar.style.top = `${coords.top - editorRect.top - 32}px`
  }

  view.dom.style.position = 'relative'
  view.dom.appendChild(toolbar)
  _toolbarEl = toolbar
}

export function hideTableToolbar() {
  if (_toolbarEl) {
    _toolbarEl.remove()
    _toolbarEl = null
  }
  _activeView = null
  _tableRange = null
}

export function checkTableAtCursor(view) {
  const pos = view.state.selection.main.head
  if (isInsideTable(view.state, pos)) {
    const range = getTableRange(view.state, pos)
    if (range) {
      // Avoid teardown/rebuild on every keystroke: only show if the table changed
      if (_toolbarEl && _activeView === view && _tableRange && _tableRange.from === range.from && _tableRange.to === range.to) {
        return
      }
      showTableToolbar(view, range)
      return
    }
  }
  hideTableToolbar()
}

export function toggleColumnAlignment(view, range, alignment) {
  const text = view.state.doc.sliceString(range.from, range.to)
  const table = parseMarkdownTable(text)
  if (!table) return
  const head = view.state.selection.main.head
  const line = view.state.doc.lineAt(head)
  const beforeCursor = line.text.slice(0, head - line.from)
  const colIdx = (beforeCursor.match(/\|/g) || []).length - 1
  if (colIdx < 0 || colIdx >= table.alignments.length) return
  table.alignments[colIdx] = alignment
  const newText = serializeTable(table)
  view.dispatch({ changes: { from: range.from, to: range.to, insert: newText } })
}

// ── Tab / Shift-Tab navigation between table cells (#52) ─────────
export function tableTabForward(view) {
  const { state } = view
  const pos = state.selection.main.head
  if (!isInsideTable(state, pos)) return false

  const line = state.doc.lineAt(pos)
  const rest = line.text.slice(pos - line.from)
  const nextPipe = rest.indexOf('|', 1) // skip first char (might be '|' itself)

  if (nextPipe !== -1) {
    // Move into the next cell on the same line
    const cellStart = pos + nextPipe + 1
    const cellEnd = (() => {
      const after = line.text.slice(cellStart - line.from)
      const nextP = after.indexOf('|')
      return nextP !== -1 ? cellStart + nextP : line.to
    })()
    // Skip the separator row (line 2 of the table, all dashes)
    const targetLine = state.doc.lineAt(cellStart)
    if (/^\s*\|[\s:|-]+\|\s*$/.test(targetLine.text)) {
      return tableTabForward(view)
    }
    view.dispatch({ selection: { anchor: cellStart, head: Math.min(cellEnd, line.to) } })
    return true
  }

  // Move to the first cell of the next line
  const nextLineNum = line.number + 1
  if (nextLineNum > state.doc.lines) return false
  const nextLine = state.doc.line(nextLineNum)
  if (!isInsideTable(state, nextLine.from)) return false

  const firstPipe = nextLine.text.indexOf('|')
  const start = nextLine.from + firstPipe + 1
  const after = nextLine.text.slice(firstPipe + 1)
  const endPipe = after.indexOf('|')
  const end = endPipe !== -1 ? start + endPipe : nextLine.to
  // Skip separator row
  if (/^\s*\|[\s:|-]+\|\s*$/.test(nextLine.text)) {
    const skipLine = state.doc.line(nextLineNum + 1)
    if (skipLine && isInsideTable(state, skipLine.from)) {
      const fp = skipLine.text.indexOf('|')
      const s = skipLine.from + fp + 1
      const af = skipLine.text.slice(fp + 1)
      const ep = af.indexOf('|')
      const e = ep !== -1 ? s + ep : skipLine.to
      view.dispatch({ selection: { anchor: s, head: e } })
      return true
    }
    return false
  }
  view.dispatch({ selection: { anchor: start, head: end } })
  return true
}

export function tableTabBackward(view) {
  const { state } = view
  const pos = state.selection.main.head
  if (!isInsideTable(state, pos)) return false

  const line = state.doc.lineAt(pos)
  const before = line.text.slice(0, pos - line.from)
  // Find the second-to-last pipe in `before`
  const pipes = []
  for (let i = 0; i < before.length; i++) if (before[i] === '|') pipes.push(i)

  if (pipes.length >= 2) {
    const cellStart = line.from + pipes[pipes.length - 2] + 1
    const cellEnd = line.from + pipes[pipes.length - 1]
    view.dispatch({ selection: { anchor: cellStart, head: cellEnd } })
    return true
  }

  // Move to the last cell of the previous line
  const prevLineNum = line.number - 1
  if (prevLineNum < 1) return false
  const prevLine = state.doc.line(prevLineNum)
  if (!isInsideTable(state, prevLine.from)) return false
  // Skip separator row
  const lineToUse = /^\s*\|[\s:|-]+\|\s*$/.test(prevLine.text)
    ? (prevLineNum > 1 ? state.doc.line(prevLineNum - 1) : null)
    : prevLine
  if (!lineToUse || !isInsideTable(state, lineToUse.from)) return false

  const pipes2 = []
  for (let i = 0; i < lineToUse.text.length; i++) if (lineToUse.text[i] === '|') pipes2.push(i)
  if (pipes2.length >= 2) {
    const cellStart = lineToUse.from + pipes2[pipes2.length - 2] + 1
    const cellEnd = lineToUse.from + pipes2[pipes2.length - 1]
    view.dispatch({ selection: { anchor: cellStart, head: cellEnd } })
    return true
  }
  return false
}
