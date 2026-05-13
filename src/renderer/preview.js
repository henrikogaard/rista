import { $, state, getTabForPane, getFocusedTab, getFocusedEditor } from './state.js'
import { renderMarkdown, extractHeadings, getStats } from './markdown.js'
import { processDiagrams } from './diagrams.js'
import { getTheme } from './theme.js'
import { getSettings } from './settings.js'

// ── Preview ───────────────────────────────────────────────────────
export async function refreshPreview(pane, markdown) {
  const html = await renderMarkdown(markdown)
  const theme = getTheme()
  ;['single', 'left', 'right'].forEach(slot => {
    const p = $(`preview-${slot}-${pane}`)
    if (p) p.innerHTML = html
  })
  // Render D2 and Mermaid diagram blocks as SVGs
  const diagramPromises = ['single', 'left', 'right'].map(slot => {
    const p = $(`preview-${slot}-${pane}`)
    return p ? processDiagrams(p, theme) : null
  }).filter(Boolean)
  await Promise.all(diagramPromises)
}

// ── Stats ─────────────────────────────────────────────────────────
export function updateActiveMetrics() {
  const tab = getFocusedTab()
  const markdown = tab?.content || ''
  const words = $('st-words')
  const lines = $('st-lines')
  updateCursorStatus()
  if (!tab) {
    if (words) words.textContent = '—'
    if (lines) lines.textContent = '—'
    renderStatsPopover(getStats(''))
    renderTocPopover([])
    return
  }
  const stats = getStats(markdown)
  if (words) words.textContent = `${stats.words} words`
  if (lines) lines.textContent = `${markdown.split('\n').length} lines`
  renderStatsPopover(stats)
  renderTocPopover(extractHeadings(markdown))
}

export function updateCursorStatus(editorState = getFocusedEditor()?.state) {
  const cursor = $('st-cursor')
  if (!cursor) return
  if (!editorState) {
    cursor.textContent = 'Ln 1, Col 1'
    return
  }
  const head = editorState.selection.main.head
  const line = editorState.doc.lineAt(head)
  const lineNumber = line.number
  const column = head - line.from + 1
  cursor.textContent = `Ln ${lineNumber}, Col ${column}`
}

export function onEditorSelectionChange(pane, editorState) {
  if (pane !== state.focusedPane) return
  updateCursorStatus(editorState)
}

export function renderStatsPopover(s) {
  const c = $('stats-content')
  if (!c) return
  c.innerHTML = `
    <div class="stat-card"><div class="num">${s.words}</div><div class="row"><span class="lbl">Words</span></div></div>
    <div class="stat-card"><div class="num">${s.chars}</div><div class="row"><span class="lbl">Characters</span></div></div>
    <div class="stat-card"><div class="num">${s.paragraphs}</div><div class="row"><span class="lbl">Paragraphs</span></div></div>
    <div class="stat-card"><div class="num" style="font-size:15px">${s.readMin < 1 ? '< 1' : s.readMin} min</div><div class="row"><span class="lbl">Read time</span></div></div>
  `
}

export function renderTocPopover(headings) {
  const c = $('toc-content')
  if (!c) return
  if (!headings.length) {
    c.innerHTML = `<div class="toc-empty"><div class="icon">🏔️</div><div class="msg">No headers yet</div></div>`
    return
  }
  c.innerHTML = headings.map(h =>
    `<div class="toc-item h${h.level}">${h.text}</div>`
  ).join('')
}

export function updateStats(markdown) {
  void markdown
  updateActiveMetrics()
}

// ── PDF Export ──────────────────────────────────────────────────────
export async function exportToPdf() {
  const tab = getFocusedTab()
  if (!tab) {
    alert('No file open')
    return
  }
  try {
    const html = await renderMarkdown(tab.content || '')
    const success = await window.fjord.exportPdf({
      fileName: tab.name,
      html,
      theme: getTheme(),
      settings: getSettings(),
    })
    if (success) {
      // Success notification could be added here
    } else {
      alert('Failed to export PDF')
    }
  } catch (err) {
    alert('Error exporting PDF: ' + err.message)
  }
}

// ── Image Paste Handler ─────────────────────────────────────────────
export async function handleImagePaste(file, view, pane = state.focusedPane, onEditorChange) {
  const tab = getTabForPane(pane)
  if (!tab) return

  const reader = new FileReader()
  reader.onload = async (e) => {
    const base64Full = e.target.result
    const base64Data = base64Full.split(',')[1]
    const ext = file.type.split('/')[1] || 'png'
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
    const fileName = `image-${timestamp}.${ext}`

    // Determine _assets/ directory next to the current .md file
    const filePath = tab.path
    if (!filePath) return

    const lastSlash = filePath.lastIndexOf('/') !== -1 ? filePath.lastIndexOf('/') : filePath.lastIndexOf('\\')
    const dirPath = filePath.substring(0, lastSlash)
    const assetsDir = dirPath + '/_assets'

    const result = await window.fjord.writeImageFile(assetsDir, base64Data, fileName)
    if (!result) return

    const relativePath = '_assets/' + fileName
    const markdown = `![${fileName}](${relativePath})`
    const pos = view.state.selection.main.head

    view.dispatch({
      changes: { from: pos, to: pos, insert: markdown },
      selection: { anchor: pos + markdown.length },
    })

    onEditorChange(pane, view.state.doc.toString())
  }
  reader.readAsDataURL(file)
}
