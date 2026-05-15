import { state } from './state.js'

// ── Graph View ───────────────────────────────────────────────────
// A canvas-based 2D graph visualization of wikilinks.

export function buildGraphView() {
  return `
    <div class="graph-view" id="graph-view">
      <canvas class="graph-canvas" id="graph-canvas"></canvas>
      <div class="graph-empty" id="graph-empty" style="display:none">
        <span>No links to visualize</span>
        <p class="graph-empty__sub">Create [[wikilinks]] between your notes to see them here.</p>
      </div>
      <div class="graph-controls">
        <div class="graph-btn" id="graph-reset-btn" title="Reset view">Reset</div>
        <div class="graph-btn" id="graph-close-btn" title="Close graph">Close</div>
      </div>
    </div>
  `
}

let _animationId = null
let _listeners = []
let _nodes = []
let _edges = []
let _camera = { x: 0, y: 0, zoom: 1 }
let _dragging = null
let _hovered = null
let _onNodeClick = null

export function renderGraph(linkIndex, onNodeClick) {
  _onNodeClick = onNodeClick
  const canvas = document.getElementById('graph-canvas')
  const empty = document.getElementById('graph-empty')
  if (!canvas) return

  const ctx = canvas.getContext('2d')
  const dpr = window.devicePixelRatio || 1

  const resize = () => {
    const rect = canvas.parentElement.getBoundingClientRect()
    canvas.width = rect.width * dpr
    canvas.height = rect.height * dpr
    canvas.style.width = rect.width + 'px'
    canvas.style.height = rect.height + 'px'
    ctx.scale(dpr, dpr)
  }
  resize()
  window.addEventListener('resize', resize)
  _listeners.push({ target: window, type: 'resize', fn: resize })

  // Build graph from link index
  const files = linkIndex.files || new Map()
  const backlinks = linkIndex.backlinks || new Map()
  const allPaths = linkIndex.allPaths || new Set()

  const pathToIndex = new Map()
  _nodes = []
  _edges = []

  for (const path of allPaths) {
    const name = path.split(/[/\\]/).pop().replace(/\.md$/i, '')
    pathToIndex.set(path, _nodes.length)
    _nodes.push({
      id: path,
      name,
      x: (Math.random() - 0.5) * 400,
      y: (Math.random() - 0.5) * 400,
      vx: 0,
      vy: 0,
      radius: 6 + name.length * 0.8,
    })
  }

  for (const [sourcePath, { links }] of files) {
    const sourceIdx = pathToIndex.get(sourcePath)
    if (sourceIdx === undefined) continue
    for (const link of links) {
      // Find target by name
      const targetPath = Array.from(allPaths).find(p => {
        const n = p.split(/[/\\]/).pop().replace(/\.md$/i, '')
        return n === link
      })
      if (targetPath) {
        const targetIdx = pathToIndex.get(targetPath)
        if (targetIdx !== undefined && targetIdx !== sourceIdx) {
          _edges.push({ source: sourceIdx, target: targetIdx })
        }
      }
    }
  }

  if (_nodes.length === 0 || _edges.length === 0) {
    canvas.style.display = 'none'
    if (empty) empty.style.display = 'flex'
    return
  }

  canvas.style.display = 'block'
  if (empty) empty.style.display = 'none'

  // Force simulation (simple)
  const simulate = () => {
    // Repulsion
    for (let i = 0; i < _nodes.length; i++) {
      for (let j = i + 1; j < _nodes.length; j++) {
        const dx = _nodes[j].x - _nodes[i].x
        const dy = _nodes[j].y - _nodes[i].y
        const dist = Math.sqrt(dx * dx + dy * dy) || 1
        const force = 2000 / (dist * dist)
        const fx = (dx / dist) * force
        const fy = (dy / dist) * force
        _nodes[i].vx -= fx
        _nodes[i].vy -= fy
        _nodes[j].vx += fx
        _nodes[j].vy += fy
      }
    }
    // Attraction along edges
    for (const edge of _edges) {
      const a = _nodes[edge.source]
      const b = _nodes[edge.target]
      const dx = b.x - a.x
      const dy = b.y - a.y
      const dist = Math.sqrt(dx * dx + dy * dy) || 1
      const force = dist * 0.001
      const fx = (dx / dist) * force
      const fy = (dy / dist) * force
      a.vx += fx
      a.vy += fy
      b.vx -= fx
      b.vy -= fy
    }
    // Center gravity
    for (const node of _nodes) {
      node.vx -= node.x * 0.0005
      node.vy -= node.y * 0.0005
      // Damping
      node.vx *= 0.9
      node.vy *= 0.9
      // Update
      if (node !== _dragging) {
        node.x += node.vx
        node.y += node.vy
      }
    }
  }

  const draw = () => {
    const rect = canvas.parentElement.getBoundingClientRect()
    ctx.clearRect(0, 0, rect.width, rect.height)
    ctx.save()
    ctx.translate(rect.width / 2 + _camera.x, rect.height / 2 + _camera.y)
    ctx.scale(_camera.zoom, _camera.zoom)

    // Draw edges
    ctx.strokeStyle = 'rgba(91,127,166,0.15)'
    ctx.lineWidth = 1
    for (const edge of _edges) {
      const a = _nodes[edge.source]
      const b = _nodes[edge.target]
      ctx.beginPath()
      ctx.moveTo(a.x, a.y)
      ctx.lineTo(b.x, b.y)
      ctx.stroke()
    }

    // Draw nodes
    for (const node of _nodes) {
      const isHovered = node === _hovered
      ctx.beginPath()
      ctx.arc(node.x, node.y, node.radius, 0, Math.PI * 2)
      ctx.fillStyle = isHovered ? 'var(--accent-hi)' : 'var(--accent)'
      ctx.fill()
      if (isHovered) {
        ctx.strokeStyle = 'rgba(255,255,255,0.2)'
        ctx.lineWidth = 2
        ctx.stroke()
      }

      // Label
      ctx.fillStyle = 'var(--text2)'
      ctx.font = '11px var(--font)'
      ctx.textAlign = 'center'
      ctx.fillText(node.name, node.x, node.y + node.radius + 14)
    }

    ctx.restore()
  }

  const loop = () => {
    simulate()
    draw()
    _animationId = requestAnimationFrame(loop)
  }
  loop()

  // Mouse events
  let isDragging = false
  let dragStart = { x: 0, y: 0 }

  const onPointerDown = (e) => {
    const rect = canvas.getBoundingClientRect()
    const mx = (e.clientX - rect.left - rect.width / 2 - _camera.x) / _camera.zoom
    const my = (e.clientY - rect.top - rect.height / 2 - _camera.y) / _camera.zoom

    for (const node of _nodes) {
      const dx = mx - node.x
      const dy = my - node.y
      if (Math.sqrt(dx * dx + dy * dy) < node.radius + 4) {
        _dragging = node
        isDragging = true
        dragStart = { x: node.x, y: node.y }
        canvas.setPointerCapture(e.pointerId)
        return
      }
    }

    isDragging = true
    dragStart = { x: e.clientX, y: e.clientY, camX: _camera.x, camY: _camera.y }
    canvas.setPointerCapture(e.pointerId)
  }

  const onPointerMove = (e) => {
    const rect = canvas.getBoundingClientRect()
    const mx = (e.clientX - rect.left - rect.width / 2 - _camera.x) / _camera.zoom
    const my = (e.clientY - rect.top - rect.height / 2 - _camera.y) / _camera.zoom

    _hovered = null
    for (const node of _nodes) {
      const dx = mx - node.x
      const dy = my - node.y
      if (Math.sqrt(dx * dx + dy * dy) < node.radius + 4) {
        _hovered = node
        break
      }
    }
    canvas.style.cursor = _hovered ? 'pointer' : isDragging && !_dragging ? 'grabbing' : 'default'

    if (!isDragging) return

    if (_dragging) {
      _dragging.x = mx
      _dragging.y = my
      _dragging.vx = 0
      _dragging.vy = 0
    } else {
      _camera.x = dragStart.camX + (e.clientX - dragStart.x)
      _camera.y = dragStart.camY + (e.clientY - dragStart.y)
    }
  }

  const onPointerUp = (e) => {
    if (_dragging && isDragging) {
      const dx = _dragging.x - dragStart.x
      const dy = _dragging.y - dragStart.y
      if (Math.sqrt(dx * dx + dy * dy) < 5) {
        _onNodeClick?.(_dragging.id)
      }
    }
    _dragging = null
    isDragging = false
    canvas.releasePointerCapture(e.pointerId)
  }

  const onWheel = (e) => {
    e.preventDefault()
    const factor = e.deltaY > 0 ? 0.9 : 1.1
    _camera.zoom *= factor
    _camera.zoom = Math.max(0.2, Math.min(3, _camera.zoom))
  }

  const onResetClick = () => {
    _camera = { x: 0, y: 0, zoom: 1 }
  }

  canvas.addEventListener('pointerdown', onPointerDown)
  canvas.addEventListener('pointermove', onPointerMove)
  canvas.addEventListener('pointerup', onPointerUp)
  canvas.addEventListener('wheel', onWheel)
  _listeners.push(
    { target: canvas, type: 'pointerdown', fn: onPointerDown },
    { target: canvas, type: 'pointermove', fn: onPointerMove },
    { target: canvas, type: 'pointerup', fn: onPointerUp },
    { target: canvas, type: 'wheel', fn: onWheel },
  )

  const resetBtn = document.getElementById('graph-reset-btn')
  if (resetBtn) {
    resetBtn.addEventListener('click', onResetClick)
    _listeners.push({ target: resetBtn, type: 'click', fn: onResetClick })
  }
}

export function destroyGraph() {
  if (_animationId) {
    cancelAnimationFrame(_animationId)
    _animationId = null
  }
  for (const { target, type, fn } of _listeners) {
    target.removeEventListener(type, fn)
  }
  _listeners = []
}
