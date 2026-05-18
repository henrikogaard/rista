// ── Graph View ───────────────────────────────────────────────────
// Canvas-based 2D graph of wikilinks. Force-directed layout, pre-warmed
// before first paint so it doesn't open mid-explosion.

export function buildGraphView() {
  return `
    <div class="graph-view" id="graph-view">
      <canvas class="graph-canvas" id="graph-canvas"></canvas>
      <div class="graph-empty" id="graph-empty" style="display:none">
        <span>No links to visualize</span>
        <p class="graph-empty__sub">Create [[wikilinks]] between notes to see them here.</p>
      </div>
      <div class="graph-controls">
        <div class="graph-btn" id="graph-local-btn" title="Toggle local graph (current note's neighborhood)">Local</div>
        <div class="graph-btn" id="graph-reset-btn" title="Reset view">Reset</div>
      </div>
      <div class="graph-tooltip" id="graph-tooltip" style="display:none"></div>
    </div>
  `
}

let _animationId = null
let _resizeObserver = null
let _resizeListener = null
let _listeners = []
let _nodes = []
let _edges = []
let _adjacency = new Map()  // nodeIdx -> Set(otherIdx) for hover-highlight
let _camera = { x: 0, y: 0, zoom: 1 }
let _dragging = null
let _hovered = null
let _onNodeClick = null
let _simulationCooled = false
let _ctx = null
let _canvas = null

// Resolve CSS variables to concrete colors (Canvas can't read `var(--x)`).
function readCssVar(name, fallback) {
  if (typeof document === 'undefined') return fallback
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return v || fallback
}

function hexWithAlpha(hex, alpha) {
  const h = hex.replace('#', '')
  const expanded = h.length === 3 ? h.split('').map(c => c + c).join('') : h
  const r = parseInt(expanded.slice(0, 2), 16)
  const g = parseInt(expanded.slice(2, 4), 16)
  const b = parseInt(expanded.slice(4, 6), 16)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

function getPalette() {
  const accent = readCssVar('--accent', '#5b7fa6')
  const accentHi = readCssVar('--accent-hi', '#7ba3cc')
  const text1 = readCssVar('--text1', '#dddfe6')
  const text2 = readCssVar('--text2', '#8e91a0')
  const text3 = readCssVar('--text3', '#5a5e6e')
  return {
    accent,
    accentHi,
    text1,
    text2,
    text3,
    nodeFill: hexWithAlpha(accent, 0.85),
    nodeFillHover: accentHi,
    nodeFillDim: hexWithAlpha(accent, 0.25),
    nodeStrokeHover: hexWithAlpha(text1, 0.6),
    edgeStroke: hexWithAlpha(accent, 0.18),
    edgeStrokeHi: hexWithAlpha(accentHi, 0.65),
    labelText: text2,
    labelTextHi: text1,
  }
}

let _palette = getPalette()
let _lastLinkIndex = null
let _localMode = false
let _localPath = null
const LOCAL_HOPS = 2

export function setGraphLocalMode(enabled, path) {
  _localMode = !!enabled
  if (path !== undefined) _localPath = path
  if (_lastLinkIndex && _onNodeClick) {
    // Tear down before re-render so listeners don't accumulate
    teardownInteractive()
    renderGraph(_lastLinkIndex, _onNodeClick)
  }
}

export function getGraphLocalMode() {
  return _localMode
}

function teardownInteractive() {
  if (_animationId) { cancelAnimationFrame(_animationId); _animationId = null }
  if (_resizeObserver) { _resizeObserver.disconnect(); _resizeObserver = null }
  if (_resizeListener) { window.removeEventListener('resize', _resizeListener); _resizeListener = null }
  for (const { target, type, fn } of _listeners) {
    try { target.removeEventListener(type, fn) } catch {}
  }
  _listeners = []
}

export function renderGraph(linkIndex, onNodeClick) {
  _onNodeClick = onNodeClick
  _lastLinkIndex = linkIndex
  _canvas = document.getElementById('graph-canvas')
  const empty = document.getElementById('graph-empty')
  if (!_canvas) return
  _ctx = _canvas.getContext('2d')
  _palette = getPalette()

  // Build node list. Filter out orphans (nodes with no links) to keep the
  // graph clean — they otherwise pile up around the edges as background noise.
  const files = linkIndex.files || new Map()
  const allPaths = linkIndex.allPaths || new Set()

  const nameToPath = new Map()
  for (const path of allPaths) {
    const name = basename(path)
    nameToPath.set(name, path)
  }

  const edgesByPath = new Map()  // path -> Set(neighborPath)
  for (const [sourcePath, entry] of files) {
    if (!entry?.links) continue
    for (const link of entry.links) {
      const targetPath = nameToPath.get(link)
      if (!targetPath || targetPath === sourcePath) continue
      if (!edgesByPath.has(sourcePath)) edgesByPath.set(sourcePath, new Set())
      if (!edgesByPath.has(targetPath)) edgesByPath.set(targetPath, new Set())
      edgesByPath.get(sourcePath).add(targetPath)
      edgesByPath.get(targetPath).add(sourcePath)  // undirected for layout
    }
  }

  // Local mode: keep only the subgraph within LOCAL_HOPS of _localPath
  if (_localMode && _localPath) {
    const keep = new Set([_localPath])
    let frontier = new Set([_localPath])
    for (let hop = 0; hop < LOCAL_HOPS; hop++) {
      const next = new Set()
      for (const p of frontier) {
        const neighbors = edgesByPath.get(p)
        if (!neighbors) continue
        for (const n of neighbors) {
          if (!keep.has(n)) {
            keep.add(n)
            next.add(n)
          }
        }
      }
      frontier = next
      if (frontier.size === 0) break
    }
    const filtered = new Map()
    for (const [src, neighbors] of edgesByPath) {
      if (!keep.has(src)) continue
      const kept = new Set()
      for (const n of neighbors) if (keep.has(n)) kept.add(n)
      filtered.set(src, kept)
    }
    edgesByPath.clear()
    for (const [k, v] of filtered) edgesByPath.set(k, v)
  }

  const pathToIdx = new Map()
  _nodes = []
  _edges = []
  _adjacency = new Map()

  for (const [path, neighbors] of edgesByPath) {
    pathToIdx.set(path, _nodes.length)
    _nodes.push({
      id: path,
      name: basename(path),
      x: (Math.random() - 0.5) * 200,
      y: (Math.random() - 0.5) * 200,
      vx: 0,
      vy: 0,
      degree: neighbors.size,
    })
  }

  for (const [sourcePath, neighbors] of edgesByPath) {
    const si = pathToIdx.get(sourcePath)
    for (const target of neighbors) {
      const ti = pathToIdx.get(target)
      if (ti === undefined || ti <= si) continue  // dedupe (undirected)
      _edges.push({ source: si, target: ti })
    }
  }

  for (let i = 0; i < _nodes.length; i++) _adjacency.set(i, new Set())
  for (const e of _edges) {
    _adjacency.get(e.source).add(e.target)
    _adjacency.get(e.target).add(e.source)
  }

  if (_nodes.length === 0 || _edges.length === 0) {
    _canvas.style.display = 'none'
    if (empty) empty.style.display = 'flex'
    return
  }
  _canvas.style.display = 'block'
  if (empty) empty.style.display = 'none'

  // Pre-warm: run the simulation a few hundred steps before first paint so
  // the user sees a settled layout instead of an exploding hairball.
  _simulationCooled = false
  for (let step = 0; step < 250; step++) {
    simulate(step / 250)  // gradually cool
  }
  _simulationCooled = true

  // Set up canvas sizing
  const setupSize = () => {
    if (!_canvas) return
    const rect = _canvas.parentElement.getBoundingClientRect()
    const dpr = window.devicePixelRatio || 1
    _canvas.width = Math.max(1, Math.floor(rect.width * dpr))
    _canvas.height = Math.max(1, Math.floor(rect.height * dpr))
    _canvas.style.width = rect.width + 'px'
    _canvas.style.height = rect.height + 'px'
    // Reset transform before scaling so we don't compound across resizes
    _ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  }
  setupSize()

  // Resize observer for the container (sidebar resize, widget reorder, etc.)
  if (typeof ResizeObserver !== 'undefined') {
    _resizeObserver = new ResizeObserver(setupSize)
    _resizeObserver.observe(_canvas.parentElement)
  }
  _resizeListener = setupSize
  window.addEventListener('resize', _resizeListener)

  // Mouse events
  let isDragging = false
  let dragStart = { x: 0, y: 0 }

  const screenToWorld = (clientX, clientY) => {
    const rect = _canvas.getBoundingClientRect()
    return {
      x: (clientX - rect.left - rect.width / 2 - _camera.x) / _camera.zoom,
      y: (clientY - rect.top - rect.height / 2 - _camera.y) / _camera.zoom,
    }
  }

  const findNodeAt = (worldX, worldY) => {
    for (const node of _nodes) {
      const r = nodeRadius(node)
      const dx = worldX - node.x
      const dy = worldY - node.y
      if (dx * dx + dy * dy < (r + 3) * (r + 3)) return node
    }
    return null
  }

  const onPointerDown = (e) => {
    const { x, y } = screenToWorld(e.clientX, e.clientY)
    const hit = findNodeAt(x, y)
    if (hit) {
      _dragging = hit
      isDragging = true
      dragStart = { x: hit.x, y: hit.y, clientX: e.clientX, clientY: e.clientY }
    } else {
      isDragging = true
      _dragging = null
      dragStart = { clientX: e.clientX, clientY: e.clientY, camX: _camera.x, camY: _camera.y }
    }
    _canvas.setPointerCapture(e.pointerId)
  }

  const onPointerMove = (e) => {
    const { x, y } = screenToWorld(e.clientX, e.clientY)
    _hovered = findNodeAt(x, y)
    _canvas.style.cursor = _hovered ? 'pointer' : isDragging ? 'grabbing' : 'grab'

    const tooltip = document.getElementById('graph-tooltip')
    if (tooltip) {
      if (_hovered && !isDragging) {
        tooltip.style.display = 'block'
        tooltip.style.left = (e.clientX - _canvas.getBoundingClientRect().left + 10) + 'px'
        tooltip.style.top = (e.clientY - _canvas.getBoundingClientRect().top + 10) + 'px'
        tooltip.textContent = _hovered.name
      } else {
        tooltip.style.display = 'none'
      }
    }

    if (!isDragging) return
    if (_dragging) {
      _dragging.x = x
      _dragging.y = y
      _dragging.vx = 0
      _dragging.vy = 0
    } else {
      _camera.x = dragStart.camX + (e.clientX - dragStart.clientX)
      _camera.y = dragStart.camY + (e.clientY - dragStart.clientY)
    }
  }

  const onPointerUp = (e) => {
    if (_dragging && isDragging) {
      const moved = Math.hypot(e.clientX - dragStart.clientX, e.clientY - dragStart.clientY)
      if (moved < 5) _onNodeClick?.(_dragging.id)
    }
    _dragging = null
    isDragging = false
    try { _canvas.releasePointerCapture(e.pointerId) } catch {}
  }

  const onWheel = (e) => {
    e.preventDefault()
    const factor = e.deltaY > 0 ? 0.9 : 1.111
    const next = Math.max(0.3, Math.min(3, _camera.zoom * factor))
    _camera.zoom = next
  }

  const onResetClick = () => { _camera = { x: 0, y: 0, zoom: 1 } }

  _canvas.addEventListener('pointerdown', onPointerDown)
  _canvas.addEventListener('pointermove', onPointerMove)
  _canvas.addEventListener('pointerup', onPointerUp)
  _canvas.addEventListener('pointercancel', onPointerUp)
  _canvas.addEventListener('wheel', onWheel, { passive: false })
  _listeners.push(
    { target: _canvas, type: 'pointerdown', fn: onPointerDown },
    { target: _canvas, type: 'pointermove', fn: onPointerMove },
    { target: _canvas, type: 'pointerup', fn: onPointerUp },
    { target: _canvas, type: 'pointercancel', fn: onPointerUp },
    { target: _canvas, type: 'wheel', fn: onWheel },
  )

  const resetBtn = document.getElementById('graph-reset-btn')
  if (resetBtn) {
    resetBtn.addEventListener('click', onResetClick)
    _listeners.push({ target: resetBtn, type: 'click', fn: onResetClick })
  }

  const localBtn = document.getElementById('graph-local-btn')
  if (localBtn) {
    localBtn.classList.toggle('graph-btn--active', _localMode)
    const onLocalClick = () => setGraphLocalMode(!_localMode)
    localBtn.addEventListener('click', onLocalClick)
    _listeners.push({ target: localBtn, type: 'click', fn: onLocalClick })
  }

  // Animation loop
  const loop = () => {
    simulate(1)  // fully cooled — small perturbations only
    draw()
    _animationId = requestAnimationFrame(loop)
  }
  loop()
}

function nodeRadius(node) {
  // 3..8 px, scaling by sqrt(degree) so high-degree nodes are slightly bigger
  return 3 + Math.min(5, Math.sqrt(node.degree))
}

function basename(p) {
  return p.split(/[/\\]/).pop().replace(/\.md$/i, '')
}

function simulate(coolingFactor) {
  // coolingFactor: 0..1 — used during pre-warm to scale damping/repulsion
  // Repulsion (all pairs)
  const REPULSION = 1200
  for (let i = 0; i < _nodes.length; i++) {
    for (let j = i + 1; j < _nodes.length; j++) {
      const a = _nodes[i]
      const b = _nodes[j]
      const dx = b.x - a.x
      const dy = b.y - a.y
      let dist2 = dx * dx + dy * dy
      if (dist2 < 1) dist2 = 1
      const dist = Math.sqrt(dist2)
      const force = REPULSION / dist2
      const fx = (dx / dist) * force
      const fy = (dy / dist) * force
      a.vx -= fx
      a.vy -= fy
      b.vx += fx
      b.vy += fy
    }
  }
  // Spring attraction along edges
  const SPRING_LENGTH = 60
  const SPRING_K = 0.04
  for (const edge of _edges) {
    const a = _nodes[edge.source]
    const b = _nodes[edge.target]
    const dx = b.x - a.x
    const dy = b.y - a.y
    const dist = Math.sqrt(dx * dx + dy * dy) || 1
    const displacement = dist - SPRING_LENGTH
    const force = displacement * SPRING_K
    const fx = (dx / dist) * force
    const fy = (dy / dist) * force
    a.vx += fx
    a.vy += fy
    b.vx -= fx
    b.vy -= fy
  }
  // Center gravity (weak)
  const GRAVITY = 0.005
  for (const node of _nodes) {
    node.vx -= node.x * GRAVITY
    node.vy -= node.y * GRAVITY
    // Damping (stronger when cooled)
    const damping = 0.55 + 0.3 * coolingFactor
    node.vx *= damping
    node.vy *= damping
    if (node !== _dragging) {
      // Cap velocity to prevent explosion during pre-warm
      const v = Math.hypot(node.vx, node.vy)
      const maxV = _simulationCooled ? 4 : 16
      if (v > maxV) {
        node.vx = (node.vx / v) * maxV
        node.vy = (node.vy / v) * maxV
      }
      node.x += node.vx
      node.y += node.vy
    }
  }
}

function draw() {
  if (!_ctx || !_canvas) return
  const rect = _canvas.parentElement.getBoundingClientRect()
  _ctx.clearRect(0, 0, rect.width, rect.height)
  _ctx.save()
  _ctx.translate(rect.width / 2 + _camera.x, rect.height / 2 + _camera.y)
  _ctx.scale(_camera.zoom, _camera.zoom)

  // Determine highlighted set (hovered node + its neighbors)
  const highlighted = new Set()
  let hoveredIdx = -1
  if (_hovered) {
    hoveredIdx = _nodes.indexOf(_hovered)
    if (hoveredIdx !== -1) {
      highlighted.add(hoveredIdx)
      for (const n of _adjacency.get(hoveredIdx) || []) highlighted.add(n)
    }
  }
  const hasHighlight = highlighted.size > 0

  // Edges
  _ctx.lineWidth = 1
  for (const edge of _edges) {
    const isHi = hasHighlight && (edge.source === hoveredIdx || edge.target === hoveredIdx)
    _ctx.strokeStyle = isHi ? _palette.edgeStrokeHi : _palette.edgeStroke
    _ctx.beginPath()
    _ctx.moveTo(_nodes[edge.source].x, _nodes[edge.source].y)
    _ctx.lineTo(_nodes[edge.target].x, _nodes[edge.target].y)
    _ctx.stroke()
  }

  // Nodes
  for (let i = 0; i < _nodes.length; i++) {
    const node = _nodes[i]
    const r = nodeRadius(node)
    const isHovered = node === _hovered
    const isDim = hasHighlight && !highlighted.has(i)

    _ctx.beginPath()
    _ctx.arc(node.x, node.y, r, 0, Math.PI * 2)
    _ctx.fillStyle = isHovered
      ? _palette.nodeFillHover
      : isDim
        ? _palette.nodeFillDim
        : _palette.nodeFill
    _ctx.fill()
    if (isHovered) {
      _ctx.lineWidth = 1.5
      _ctx.strokeStyle = _palette.nodeStrokeHover
      _ctx.stroke()
    }
  }

  // Labels: only for highlighted nodes when hovering, or for high-degree
  // nodes when not (keeps the graph clean).
  _ctx.font = `11px ${readCssVar('--ui-font', "'DM Sans', system-ui, sans-serif")}`
  _ctx.textAlign = 'center'
  _ctx.textBaseline = 'top'
  for (let i = 0; i < _nodes.length; i++) {
    const node = _nodes[i]
    const r = nodeRadius(node)
    const isHovered = node === _hovered
    const inHi = highlighted.has(i)
    const showLabel = hasHighlight ? inHi : node.degree >= 3
    if (!showLabel) continue
    _ctx.fillStyle = isHovered || inHi ? _palette.labelTextHi : _palette.labelText
    _ctx.fillText(node.name, node.x, node.y + r + 3)
  }

  _ctx.restore()
}

export function destroyGraph() {
  if (_animationId) {
    cancelAnimationFrame(_animationId)
    _animationId = null
  }
  if (_resizeObserver) {
    _resizeObserver.disconnect()
    _resizeObserver = null
  }
  if (_resizeListener) {
    window.removeEventListener('resize', _resizeListener)
    _resizeListener = null
  }
  for (const { target, type, fn } of _listeners) {
    try { target.removeEventListener(type, fn) } catch {}
  }
  _listeners = []
  _nodes = []
  _edges = []
  _adjacency = new Map()
  _hovered = null
  _dragging = null
  _ctx = null
  _canvas = null
}
