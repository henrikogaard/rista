import { $, state } from './state.js'
import { insertMarkdownAtSelection } from './commands.js'
import mermaid from 'mermaid'

let builderEl = null
let overlayEl = null
let previewTimer = null
let mermaidPreviewId = 0

const DIAGRAM_TYPES = [
  { value: 'flowchart', label: 'Flowchart' },
  { value: 'sequence', label: 'Sequence' },
  { value: 'classDiagram', label: 'Class' },
  { value: 'stateDiagram-v2', label: 'State' },
  { value: 'erDiagram', label: 'ER' },
  { value: 'gantt', label: 'Gantt' },
  { value: 'pie', label: 'Pie' },
]

const DIRECTIONS = [
  { value: 'TD', label: 'Top → Down' },
  { value: 'LR', label: 'Left → Right' },
  { value: 'RL', label: 'Right → Left' },
  { value: 'BT', label: 'Bottom → Top' },
]

const EDGE_STYLES = [
  { value: '-->', label: 'Arrow →' },
  { value: '---', label: 'Line —' },
  { value: '-.->',label: 'Dashed ⇢' },
  { value: '==>', label: 'Thick ⇒' },
]

function defaultState() {
  return {
    type: 'flowchart',
    direction: 'TD',
    nodes: [
      { id: 'A', label: 'Start' },
      { id: 'B', label: 'End' },
    ],
    edges: [
      { from: 'A', to: 'B', label: '', style: '-->' },
    ],
  }
}

let builderState = defaultState()

function generateMermaid() {
  const { type, direction, nodes, edges } = builderState

  if (type === 'sequence') {
    const lines = ['sequenceDiagram']
    for (const e of edges) {
      const fromNode = nodes.find(n => n.id === e.from)
      const toNode = nodes.find(n => n.id === e.to)
      const fromLabel = fromNode?.label || e.from
      const toLabel = toNode?.label || e.to
      const msg = e.label || 'message'
      lines.push(`    ${fromLabel}->>+${toLabel}: ${msg}`)
    }
    return lines.join('\n')
  }

  if (type === 'pie') {
    const lines = ['pie title Chart']
    for (const n of nodes) {
      lines.push(`    "${n.label}" : ${n.id}`)
    }
    return lines.join('\n')
  }

  if (type === 'erDiagram') {
    const lines = ['erDiagram']
    for (const e of edges) {
      lines.push(`    ${e.from} ||--o{ ${e.to} : "${e.label || 'relates'}"`)
    }
    return lines.join('\n')
  }

  if (type === 'gantt') {
    const lines = ['gantt', '    dateFormat YYYY-MM-DD', '    title Timeline']
    for (const n of nodes) {
      lines.push(`    ${n.label} :${n.id}, 2024-01-01, 7d`)
    }
    return lines.join('\n')
  }

  if (type === 'classDiagram') {
    const lines = ['classDiagram']
    for (const n of nodes) {
      lines.push(`    class ${n.id} { ${n.label} }`)
    }
    for (const e of edges) {
      lines.push(`    ${e.from} ${e.style === '==>' ? '<|--' : '-->'} ${e.to}${e.label ? ' : ' + e.label : ''}`)
    }
    return lines.join('\n')
  }

  if (type === 'stateDiagram-v2') {
    const lines = ['stateDiagram-v2']
    for (const e of edges) {
      const fromLabel = e.from === '[*]' ? '[*]' : e.from
      const toLabel = e.to === '[*]' ? '[*]' : e.to
      lines.push(`    ${fromLabel} --> ${toLabel}${e.label ? ' : ' + e.label : ''}`)
    }
    return lines.join('\n')
  }

  const lines = [`flowchart ${direction}`]
  for (const n of nodes) {
    lines.push(`    ${n.id}[${n.label}]`)
  }
  for (const e of edges) {
    const arrow = e.style || '-->'
    const labelPart = e.label ? `|${e.label}|` : ''
    lines.push(`    ${e.from} ${arrow}${labelPart} ${e.to}`)
  }
  return lines.join('\n')
}

async function renderPreview() {
  const previewEl = builderEl?.querySelector('.diagram-builder__preview')
  if (!previewEl) return

  const code = generateMermaid()
  const id = `fjord-db-preview-${mermaidPreviewId++}`

  try {
    const { svg } = await mermaid.render(id, code)
    previewEl.innerHTML = svg
    previewEl.classList.remove('diagram-builder__preview--error')
  } catch (err) {
    document.getElementById(id)?.remove()
    previewEl.textContent = err?.message || 'Invalid diagram'
    previewEl.classList.add('diagram-builder__preview--error')
  }
}

function schedulePreview() {
  clearTimeout(previewTimer)
  previewTimer = setTimeout(renderPreview, 300)
}

function usesDirection(type) {
  return type === 'flowchart'
}

function usesNodes(type) {
  return ['flowchart', 'sequence', 'pie', 'gantt', 'classDiagram'].includes(type)
}

function usesEdges(type) {
  return ['flowchart', 'sequence', 'erDiagram', 'classDiagram', 'stateDiagram-v2'].includes(type)
}

function renderBuilder() {
  const { type, direction, nodes, edges } = builderState
  const showDirection = usesDirection(type)
  const showNodes = usesNodes(type)
  const showEdges = usesEdges(type)

  const isPie = type === 'pie'
  const nodeIdLabel = isPie ? 'Value' : 'ID'
  const nodeLabelLabel = isPie ? 'Slice' : 'Label'

  builderEl.querySelector('.diagram-builder__body').innerHTML = `
    <div class="diagram-builder__controls">
      <div class="diagram-builder__row">
        <label class="command-field">
          <span class="command-field__label">Type</span>
          <select class="command-field__input" id="db-type">
            ${DIAGRAM_TYPES.map(t => `<option value="${t.value}"${t.value === type ? ' selected' : ''}>${t.label}</option>`).join('')}
          </select>
        </label>
        ${showDirection ? `
          <label class="command-field">
            <span class="command-field__label">Direction</span>
            <select class="command-field__input" id="db-direction">
              ${DIRECTIONS.map(d => `<option value="${d.value}"${d.value === direction ? ' selected' : ''}>${d.label}</option>`).join('')}
            </select>
          </label>
        ` : ''}
      </div>

      ${showNodes ? `
        <div class="diagram-builder__section">
          <div class="diagram-builder__section-header">
            <span class="command-field__label">Nodes</span>
            <div class="diagram-builder__add-btn" id="db-add-node">+ Add</div>
          </div>
          ${nodes.map((n, i) => `
            <div class="diagram-builder__item" data-node-index="${i}">
              <input class="command-field__input diagram-builder__input--sm" placeholder="${nodeIdLabel}" value="${n.id}" data-field="node-id" data-index="${i}" />
              <input class="command-field__input diagram-builder__input--sm" placeholder="${nodeLabelLabel}" value="${n.label}" data-field="node-label" data-index="${i}" />
              <div class="diagram-builder__remove-btn" data-remove-node="${i}">×</div>
            </div>
          `).join('')}
        </div>
      ` : ''}

      ${showEdges ? `
        <div class="diagram-builder__section">
          <div class="diagram-builder__section-header">
            <span class="command-field__label">Edges</span>
            <div class="diagram-builder__add-btn" id="db-add-edge">+ Add</div>
          </div>
          ${edges.map((e, i) => `
            <div class="diagram-builder__item" data-edge-index="${i}">
              <input class="command-field__input diagram-builder__input--sm" placeholder="From" value="${e.from}" data-field="edge-from" data-index="${i}" />
              <select class="command-field__input diagram-builder__input--sm" data-field="edge-style" data-index="${i}">
                ${EDGE_STYLES.map(s => `<option value="${s.value}"${s.value === e.style ? ' selected' : ''}>${s.label}</option>`).join('')}
              </select>
              <input class="command-field__input diagram-builder__input--sm" placeholder="To" value="${e.to}" data-field="edge-to" data-index="${i}" />
              <input class="command-field__input diagram-builder__input--sm" placeholder="Label" value="${e.label}" data-field="edge-label" data-index="${i}" />
              <div class="diagram-builder__remove-btn" data-remove-edge="${i}">×</div>
            </div>
          `).join('')}
        </div>
      ` : ''}
    </div>
    <div class="diagram-builder__preview"></div>
  `

  wireBuilderEvents()
  schedulePreview()
}

function wireBuilderEvents() {
  const body = builderEl.querySelector('.diagram-builder__body')

  body.querySelector('#db-type')?.addEventListener('change', (e) => {
    builderState.type = e.target.value
    renderBuilder()
  })

  body.querySelector('#db-direction')?.addEventListener('change', (e) => {
    builderState.direction = e.target.value
    schedulePreview()
  })

  body.querySelector('#db-add-node')?.addEventListener('click', () => {
    const nextId = String.fromCharCode(65 + builderState.nodes.length)
    builderState.nodes.push({ id: nextId, label: `Node ${builderState.nodes.length + 1}` })
    renderBuilder()
  })

  body.querySelector('#db-add-edge')?.addEventListener('click', () => {
    const nodes = builderState.nodes
    builderState.edges.push({
      from: nodes[0]?.id || 'A',
      to: nodes[nodes.length - 1]?.id || 'B',
      label: '',
      style: '-->',
    })
    renderBuilder()
  })

  body.querySelectorAll('[data-remove-node]').forEach(btn => {
    btn.addEventListener('click', () => {
      builderState.nodes.splice(Number(btn.dataset.removeNode), 1)
      renderBuilder()
    })
  })

  body.querySelectorAll('[data-remove-edge]').forEach(btn => {
    btn.addEventListener('click', () => {
      builderState.edges.splice(Number(btn.dataset.removeEdge), 1)
      renderBuilder()
    })
  })

  body.querySelectorAll('[data-field]').forEach(input => {
    input.addEventListener('input', (e) => {
      const idx = Number(e.target.dataset.index)
      const field = e.target.dataset.field
      if (field === 'node-id') builderState.nodes[idx].id = e.target.value
      else if (field === 'node-label') builderState.nodes[idx].label = e.target.value
      else if (field === 'edge-from') builderState.edges[idx].from = e.target.value
      else if (field === 'edge-to') builderState.edges[idx].to = e.target.value
      else if (field === 'edge-label') builderState.edges[idx].label = e.target.value
      else if (field === 'edge-style') builderState.edges[idx].style = e.target.value
      schedulePreview()
    })

    if (input.tagName === 'SELECT') {
      input.addEventListener('change', (e) => {
        const idx = Number(e.target.dataset.index)
        const field = e.target.dataset.field
        if (field === 'edge-style') builderState.edges[idx].style = e.target.value
        schedulePreview()
      })
    }
  })
}

function insertDiagram() {
  const code = generateMermaid()
  const block = `\n\`\`\`mermaid\n${code}\n\`\`\`\n`
  insertMarkdownAtSelection(state.focusedPane, block)
  closeDiagramBuilder()
}

export function openDiagramBuilder() {
  builderState = defaultState()

  if (!builderEl) {
    overlayEl = document.createElement('div')
    overlayEl.className = 'diagram-builder-overlay'
    overlayEl.addEventListener('click', closeDiagramBuilder)

    builderEl = document.createElement('div')
    builderEl.className = 'diagram-builder'
    builderEl.innerHTML = `
      <div class="diagram-builder__panel">
        <div class="diagram-builder__header">
          <div>
            <div class="command-dialog__eyebrow">Insert</div>
            <div class="command-dialog__title">Diagram</div>
          </div>
          <div class="theme-btn diagram-builder__close" title="Close">
            <svg viewBox="0 0 16 16" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 4l8 8M12 4l-8 8"/></svg>
          </div>
        </div>
        <div class="diagram-builder__body"></div>
        <div class="diagram-builder__footer">
          <button type="button" class="settings-btn settings-btn--muted diagram-builder__cancel">Cancel</button>
          <button type="button" class="settings-btn diagram-builder__insert">Insert diagram</button>
        </div>
      </div>
    `

    builderEl.querySelector('.diagram-builder__close').addEventListener('click', closeDiagramBuilder)
    builderEl.querySelector('.diagram-builder__cancel').addEventListener('click', closeDiagramBuilder)
    builderEl.querySelector('.diagram-builder__insert').addEventListener('click', insertDiagram)

    document.body.appendChild(overlayEl)
    document.body.appendChild(builderEl)
  }

  builderEl.classList.add('open')
  overlayEl.classList.add('open')
  renderBuilder()
}

export function closeDiagramBuilder() {
  clearTimeout(previewTimer)
  builderEl?.classList.remove('open')
  overlayEl?.classList.remove('open')
}
