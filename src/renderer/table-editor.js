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
