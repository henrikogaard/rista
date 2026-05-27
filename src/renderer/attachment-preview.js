import { state, $ } from './state.js'

// ── Attachment Preview ───────────────────────────────────────────
// Handles preview of images and PDFs in the workspace.

const IMAGE_EXTS = ['.png', '.jpg', '.jpeg', '.gif', '.svg', '.webp', '.bmp']
const PDF_EXTS = ['.pdf']
const DRAWING_EXTS = ['.fdraw.json']
const CANVAS_EXTS = ['.fcanvas.json']
let _openFilePath = null

export function registerAttachmentPreviewCallbacks({ openFilePath }) {
  _openFilePath = openFilePath
}

export function isImageFile(path) {
  return IMAGE_EXTS.some(ext => path.toLowerCase().endsWith(ext))
}

export function isPdfFile(path) {
  return PDF_EXTS.some(ext => path.toLowerCase().endsWith(ext))
}

export function isAttachmentFile(path) {
  return isImageFile(path) || isPdfFile(path)
}

export function isDrawingFile(path = '') {
  return DRAWING_EXTS.some(ext => String(path).toLowerCase().endsWith(ext))
}

export function isCanvasFile(path = '') {
  return CANVAS_EXTS.some(ext => String(path).toLowerCase().endsWith(ext))
}

export function isSpatialFile(path = '') {
  return isDrawingFile(path) || isCanvasFile(path)
}

export function isLikelyBinaryFile(path = '') {
  const name = String(path).toLowerCase()
  if (!name) return false
  // Keep markdown-first behavior, but avoid trying to load obvious binaries as text.
  if (name.endsWith('.md') || isAttachmentFile(name) || isSpatialFile(name) || name.endsWith('.txt') || name.endsWith('.json')) {
    return false
  }
  const binExts = ['.zip', '.gz', '.tar', '.7z', '.dmg', '.pkg', '.exe', '.app', '.woff', '.woff2', '.ttf', '.otf', '.mp3', '.wav', '.mp4', '.mov']
  return binExts.some(ext => name.endsWith(ext))
}

export async function renderAttachmentPreview(pane, tab) {
  const singleSurface = $(`single-surface-${pane}`)
  const splitLeft = $(`view-slot-left-${pane}`)
  const splitRight = $(`view-slot-right-${pane}`)

  const container = singleSurface || splitLeft || splitRight
  if (!container) return

  // Remove any existing attachment preview
  container.querySelectorAll('.attachment-preview').forEach(el => el.remove())

  const previewHost = document.createElement('div')
  previewHost.className = 'attachment-preview'

  if (isSpatialFile(tab.path)) {
    if (isDrawingFile(tab.path)) {
      await renderDrawingEditor(previewHost, tab)
    } else {
      await renderCanvasWorkspace(previewHost, tab)
    }
  } else if (isImageFile(tab.path)) {
    try {
      const result = await window.fjord.readFileBase64(tab.path)
      if (result.error) throw new Error(result.error)
      const base64 = result.data
      const ext = tab.path.split('.').pop().toLowerCase()
      const mime = ext === 'svg' ? 'image/svg+xml' : ext === 'webp' ? 'image/webp' : ext === 'gif' ? 'image/gif' : ext === 'png' ? 'image/png' : 'image/jpeg'
      previewHost.innerHTML = `<img src="data:${mime};base64,${base64}" alt="${tab.name}" style="max-width:100%;max-height:100%;object-fit:contain;">`
    } catch {
      previewHost.innerHTML = `<div class="attachment-error">Failed to load image</div>`
    }
  } else if (isPdfFile(tab.path)) {
    previewHost.innerHTML = `<embed src="file://${tab.path}" type="application/pdf" width="100%" height="100%" style="border:none;">`
  } else {
    previewHost.innerHTML = `<div class="attachment-error">Preview is not available for this file type.</div>`
  }

  container.appendChild(previewHost)
}

export function clearAttachmentPreview(pane) {
  const singleSurface = $(`single-surface-${pane}`)
  const splitLeft = $(`view-slot-left-${pane}`)
  const splitRight = $(`view-slot-right-${pane}`)

  const container = singleSurface || splitLeft || splitRight
  container?.querySelectorAll('.attachment-preview').forEach(el => el.remove())
}

async function renderDrawingEditor(host, tab) {
  const raw = await window.fjord.readFile(tab.path)
  let doc = null
  try { doc = JSON.parse(raw || '{}') } catch { doc = null }
  if (!doc || !Array.isArray(doc.strokes)) {
    doc = { type: 'fjord-drawing', version: 1, strokes: [] }
  }
  host.innerHTML = `
    <div class="spatial-toolbar">
      <div class="spatial-toolbar__title">Drawing</div>
      <div class="spatial-toolbar__actions">
        <div class="spatial-btn" data-action="clear">Clear</div>
        <div class="spatial-btn" data-action="save">Save</div>
      </div>
    </div>
    <canvas class="drawing-canvas" width="1400" height="900"></canvas>
  `
  const canvas = host.querySelector('.drawing-canvas')
  const ctx = canvas.getContext('2d')
  let drawing = false
  let current = []

  function drawStroke(points) {
    if (!points.length) return
    ctx.beginPath()
    ctx.moveTo(points[0].x, points[0].y)
    for (const point of points.slice(1)) ctx.lineTo(point.x, point.y)
    ctx.strokeStyle = '#5b7fa6'
    ctx.lineWidth = 2
    ctx.lineCap = 'round'
    ctx.lineJoin = 'round'
    ctx.stroke()
  }

  function renderAll() {
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    for (const stroke of doc.strokes) drawStroke(stroke)
    if (current.length) drawStroke(current)
  }

  function pointerToPoint(e) {
    const rect = canvas.getBoundingClientRect()
    const x = ((e.clientX - rect.left) / rect.width) * canvas.width
    const y = ((e.clientY - rect.top) / rect.height) * canvas.height
    return { x, y }
  }

  canvas.addEventListener('pointerdown', (e) => {
    drawing = true
    current = [pointerToPoint(e)]
    renderAll()
  })
  canvas.addEventListener('pointermove', (e) => {
    if (!drawing) return
    current.push(pointerToPoint(e))
    renderAll()
  })
  const endStroke = () => {
    if (!drawing) return
    drawing = false
    if (current.length > 1) doc.strokes.push(current)
    current = []
    renderAll()
  }
  canvas.addEventListener('pointerup', endStroke)
  canvas.addEventListener('pointerleave', endStroke)

  host.querySelector('[data-action="clear"]')?.addEventListener('click', () => {
    doc.strokes = []
    current = []
    renderAll()
  })
  host.querySelector('[data-action="save"]')?.addEventListener('click', async () => {
    const ok = await window.fjord.writeFile(tab.path, JSON.stringify(doc, null, 2))
    if (!ok) alert('Failed to save drawing')
  })

  renderAll()
}

async function renderCanvasWorkspace(host, tab) {
  const raw = await window.fjord.readFile(tab.path)
  let doc = null
  try { doc = JSON.parse(raw || '{}') } catch { doc = null }
  if (!doc || !Array.isArray(doc.cards)) {
    doc = { type: 'fjord-canvas', version: 1, cards: [], links: [] }
  }
  host.innerHTML = `
    <div class="spatial-toolbar">
      <div class="spatial-toolbar__title">Canvas</div>
      <div class="spatial-toolbar__actions">
        <div class="spatial-btn" data-action="add-card">Add card</div>
        <div class="spatial-btn" data-action="save">Save</div>
      </div>
    </div>
    <div class="canvas-board"></div>
  `
  const board = host.querySelector('.canvas-board')

  function renderCards() {
    board.innerHTML = ''
    for (const card of doc.cards) {
      const node = document.createElement('div')
      node.className = 'canvas-card'
      node.style.left = `${Number(card.x || 40)}px`
      node.style.top = `${Number(card.y || 40)}px`
      node.dataset.path = card.path
      node.innerHTML = `
        <div class="canvas-card__title">${escapeHtml(card.title || card.path.split(/[\\/]/).pop())}</div>
        <div class="canvas-card__path">${escapeHtml(card.path || '')}</div>
      `
      node.addEventListener('dblclick', () => {
        if (_openFilePath && card.path) _openFilePath(card.path)
      })
      enableCardDrag(node, card, renderCards)
      board.appendChild(node)
    }
  }

  host.querySelector('[data-action="add-card"]')?.addEventListener('click', () => {
    const path = prompt('Note path to place on canvas:')
    if (!path) return
    doc.cards.push({
      path,
      title: path.split(/[\\/]/).pop().replace(/\.md$/i, ''),
      x: 60 + doc.cards.length * 18,
      y: 60 + doc.cards.length * 12,
    })
    renderCards()
  })

  host.querySelector('[data-action="save"]')?.addEventListener('click', async () => {
    const ok = await window.fjord.writeFile(tab.path, JSON.stringify(doc, null, 2))
    if (!ok) alert('Failed to save canvas')
  })

  renderCards()
}

function enableCardDrag(node, card, rerender) {
  let start = null
  node.addEventListener('pointerdown', (e) => {
    start = {
      x: e.clientX,
      y: e.clientY,
      cx: Number(card.x || 40),
      cy: Number(card.y || 40),
    }
    node.setPointerCapture(e.pointerId)
  })
  node.addEventListener('pointermove', (e) => {
    if (!start) return
    card.x = start.cx + (e.clientX - start.x)
    card.y = start.cy + (e.clientY - start.y)
    node.style.left = `${card.x}px`
    node.style.top = `${card.y}px`
  })
  node.addEventListener('pointerup', () => {
    if (!start) return
    start = null
    rerender()
  })
}

function escapeHtml(text) {
  return String(text || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}
