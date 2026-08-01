import { $, state, getTabForPane, getFocusedTab, getFocusedEditor } from './state.js'
import { renderMarkdown, extractHeadings, getStats } from './markdown.js'
import { showStatusNotice } from './tabs.js'
import { processDiagrams } from './diagrams.js'
import { getTheme } from './theme.js'
import { getSettings } from './settings.js'
import { formatGoalStatus, isGoalReached } from './word-goals.js'
import { resolveWikilink, getLinkIndex } from './link-index.js'

// ── Preview content cache (Task 2.2) ──────────────────────────
// Avoid re-rendering when markdown hasn't changed.
const _previewCache = { primary: '', secondary: '' }
let _wikilinkCallback = null
export function registerWikilinkCallback(fn) {
  _wikilinkCallback = fn
}

export function clearPreviewCache(pane) {
  if (pane) _previewCache[pane] = ''
  else { _previewCache.primary = ''; _previewCache.secondary = '' }
}

export async function refreshPreview(pane, markdown) {
  // Skip re-render when markdown content hasn't changed for this pane
  if (_previewCache[pane] === markdown) return
  _previewCache[pane] = markdown

  const theme = getTheme()
  const activeTab = getTabForPane(pane)
  const settings = getSettings()
  const html = await renderMarkdown(markdown, {
    hideFrontmatter: settings.hideFrontmatterInRenderedModes,
    showDocumentBanners: settings.showDocumentBanners,
    currentFilePath: activeTab?.path,
    folderPath: state.folderPath,
    attachmentPaths: getWorkspaceAttachmentPaths(),
  })
  ;['single', 'left', 'right'].forEach(slot => {
    const p = $(`preview-${slot}-${pane}`)
    if (p) {
      p.innerHTML = html
      // Wire up wikilink clicks
      p.querySelectorAll('a.wikilink').forEach(link => {
        link.addEventListener('click', (e) => {
          e.preventDefault()
          const targetName = link.dataset.wikilink
          if (!targetName) return
          const allPaths = getLinkIndex().allPaths
          const target = resolveWikilink(targetName, allPaths, state.folderPath)
          if (target) {
            _wikilinkCallback?.(target)
          }
        })
      })
      p.querySelectorAll('a:not(.wikilink)').forEach(link => {
        link.addEventListener('click', async (e) => {
          const href = link.getAttribute('href') || ''
          const targetPath = resolveLocalPathFromHref(href, activeTab?.path)
          if (!targetPath) return
          e.preventDefault()
          const stat = await window.fjord?.stat?.(targetPath)
          if (stat) _wikilinkCallback?.(targetPath)
        })
      })
    }
  })
  // Render D2 and Mermaid diagram blocks as SVGs
  const diagramPromises = ['single', 'left', 'right'].map(slot => {
    const p = $(`preview-${slot}-${pane}`)
    return p ? processDiagrams(p, theme) : null
  }).filter(Boolean)
  await Promise.all(diagramPromises)
}

function resolveLocalPathFromHref(href, currentFilePath) {
  const raw = String(href || '').trim()
  if (!raw || raw.startsWith('#')) return null
  if (/^(https?:|mailto:|tel:)/i.test(raw)) return null
  const pathOnly = raw.split('#')[0].split('?')[0]
  if (!pathOnly) return null
  if (pathOnly.startsWith('/')) return pathOnly
  if (!currentFilePath) return null
  const lastSlash = Math.max(currentFilePath.lastIndexOf('/'), currentFilePath.lastIndexOf('\\'))
  const base = lastSlash >= 0 ? currentFilePath.slice(0, lastSlash) : ''
  return normalizePath(`${base}/${pathOnly}`)
}

function normalizePath(path) {
  const sep = path.includes('\\') && !path.includes('/') ? '\\' : '/'
  const parts = path.split(/[\\/]/)
  const out = []
  for (const part of parts) {
    if (!part || part === '.') continue
    if (part === '..') out.pop()
    else out.push(part)
  }
  const prefix = path.startsWith('/') ? '/' : ''
  return prefix + out.join(sep)
}

export function getWorkspaceAttachmentPaths(items = state.tree, result = []) {
  for (const item of items || []) {
    if (item.type === 'folder') {
      getWorkspaceAttachmentPaths(item.children || [], result)
    } else if (item.type === 'file' && !/\.(md|markdown)$/i.test(item.path || item.name || '')) {
      result.push(item.path)
    }
  }
  return result
}

// ── Stats ─────────────────────────────────────────────────────────
export function updateActiveMetrics() {
  const tab = getFocusedTab()
  const markdown = tab?.content || ''
  const words = $('st-words')
  if (!tab) {
    if (words) words.textContent = '—'
    const fname = $('st-filename')
    if (fname) fname.textContent = '—'
    renderStatsPopover(getStats('', getSettings().readingSpeed))
    renderTocPopover([])
    return
  }
  const stats = getStats(markdown, getSettings().readingSpeed)
  if (words) words.textContent = `${stats.words} words`
  const goalText = formatGoalStatus(stats.words, tab?.path)
  if (goalText && words) {
    words.textContent = goalText
    if (isGoalReached(stats.words, tab?.path)) {
      words.classList.add('goal-reached')
      setTimeout(() => words.classList.remove('goal-reached'), 3000)
    }
  }
  const filename = $('st-filename')
  if (filename) {
    const name = tab?.name || ''
    filename.innerHTML = name
      ? `<span class="st-filename__name">${name}</span>${tab.dirty ? '<span class="st-dirty" aria-label="unsaved"> ·</span>' : ''}`
      : '—'
  }
  const readTime = $('st-readtime')
  if (readTime) {
    readTime.textContent = stats.readMin < 1 ? '< 1 min read' : `~${stats.readMin} min read`
  }
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
    <div class="stat-card"><div class="num">${s.sentences || 0}</div><div class="row"><span class="lbl">Sentences</span></div></div>
    <div class="stat-card"><div class="num">${s.paragraphs}</div><div class="row"><span class="lbl">Paragraphs</span></div></div>
    <div class="stat-card"><div class="num" style="font-size:15px">${s.readMin < 1 ? '< 1' : s.readMin} min</div><div class="row"><span class="lbl">Read time</span></div></div>
    <div class="stat-card"><div class="num">${s.avgSentenceLen || 0}</div><div class="row"><span class="lbl">Avg words/sentence</span></div></div>
    <div class="stat-card"><div class="num">${s.avgWordLen || 0}</div><div class="row"><span class="lbl">Avg word length</span></div></div>
    <div class="stat-card"><div class="num">${s.fkGrade || 0}</div><div class="row"><span class="lbl">FK Grade Level</span></div></div>
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

// ── PDF Export ──────────────────────────────────────────────────────
export async function exportToPdf() {
  const tab = getFocusedTab()
  if (!tab) {
    showStatusNotice('No file open', 'error')
    return
  }
  try {
    const settings = getSettings()
    const html = await renderMarkdown(tab.content || '', {
      hideFrontmatter: settings.hideFrontmatterInRenderedModes,
      showDocumentBanners: settings.showDocumentBanners,
      currentFilePath: tab.path,
      folderPath: state.folderPath,
      attachmentPaths: getWorkspaceAttachmentPaths(),
    })
    const success = await window.fjord.exportPdf({
      fileName: tab.name,
      html,
      theme: getTheme(),
      settings,
    })
    if (success) {
      // Success notification could be added here
    } else {
      showStatusNotice('Failed to export PDF', 'error')
    }
  } catch (err) {
    showStatusNotice('Error exporting PDF: ' + err.message, 'error')
  }
}

// ── HTML Export ─────────────────────────────────────────────────────
export async function exportToHtml() {
  const tab = getFocusedTab()
  if (!tab) return
  try {
    const settings = getSettings()
    const html = await renderMarkdown(tab.content || '', {
      hideFrontmatter: settings.hideFrontmatterInRenderedModes,
      showDocumentBanners: settings.showDocumentBanners,
      currentFilePath: tab.path,
      folderPath: state.folderPath,
      attachmentPaths: getWorkspaceAttachmentPaths(),
    })
    await window.fjord.exportHtml({
      fileName: tab.name,
      html,
      theme: getTheme(),
      settings,
    })
  } catch (err) {
    showStatusNotice('Error exporting HTML: ' + err.message, 'error')
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
