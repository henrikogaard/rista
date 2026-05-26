import { $, el, state } from './state.js'
import { getAllTagNames, getFilesForTag, getLinkIndex } from './link-index.js'
import { extractHeadings } from './markdown.js'

// ── Callbacks ────────────────────────────────────────────────────
let _callbacks = {}
export function registerCommandPaletteCallbacks(cbs) { Object.assign(_callbacks, cbs) }

// ── Command registry ─────────────────────────────────────────────
const commands = []

export function registerCommands(cmds) {
  cmds.forEach(c => registerCommand(c.id, c.label, c.description, c.shortcut, c.action))
}

export function registerCommand(id, label, description, shortcut, action) {
  const existing = commands.findIndex(c => c.id === id)
  if (existing >= 0) commands[existing] = { id, label, description, shortcut, action }
  else commands.push({ id, label, description, shortcut, action })
}

// ── DOM refs ─────────────────────────────────────────────────────
let overlayEl = null
let paletteEl = null
let inputEl = null
let resultsEl = null
let activeIndex = 0
let _tagFiles = null  // When set, we're browsing files for a specific tag

// ── Tree helpers ─────────────────────────────────────────────────
function flattenTree(nodes, result = []) {
  for (const node of nodes) {
    if (node.type === 'file') result.push(node)
    if (node.children) flattenTree(node.children, result)
  }
  return result
}

// ── Fuzzy match ──────────────────────────────────────────────────
function fuzzyMatch(query, text) {
  const q = query.toLowerCase()
  const t = text.toLowerCase()
  if (!q) return { match: true, score: 0 }

  let qi = 0
  let score = 0
  let consecutive = 0
  let lastMatchIndex = -2

  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] === q[qi]) {
      qi++
      // Consecutive match bonus
      if (ti === lastMatchIndex + 1) {
        consecutive++
        score += consecutive * 2
      } else {
        consecutive = 0
        score += 1
      }
      // Word-boundary bonus
      if (ti === 0 || t[ti - 1] === '/' || t[ti - 1] === '-' || t[ti - 1] === '_' || t[ti - 1] === ' ' || t[ti - 1] === '.') {
        score += 3
      }
      lastMatchIndex = ti
    }
  }

  if (qi < q.length) return { match: false, score: 0 }

  // Exact-match bonus
  if (t === q) score += 10

  return { match: true, score }
}

// ── Search logic ─────────────────────────────────────────────────
function getResults(query) {
  const trimmed = query.trim()

  // Tag-files sub-mode: browsing files for a selected tag
  if (_tagFiles) {
    return _tagFiles
      .map(path => {
        const name = path.split(/[/\\]/).pop()
        const { match, score } = fuzzyMatch(trimmed, name)
        return { type: 'file', item: { path, name }, score, match }
      })
      .filter(r => r.match)
      .sort((a, b) => b.score - a.score)
      .slice(0, 10)
  }

  // Command mode: query starts with ">"
  if (trimmed.startsWith('>')) {
    const cmdQuery = trimmed.slice(1).trim()
    return commands
      .map(c => {
        const { match, score } = fuzzyMatch(cmdQuery, c.label)
        return { type: 'command', item: c, score, match }
      })
      .filter(r => r.match)
      .sort((a, b) => b.score - a.score)
      .slice(0, 10)
  }

  // Tag mode: query starts with "#"
  if (trimmed.startsWith('#')) {
    const tagQuery = trimmed.slice(1).trim()
    const allTags = getAllTagNames()
    return allTags
      .map(tag => {
        const { match, score } = fuzzyMatch(tagQuery, tag)
        const files = getFilesForTag(tag)
        return {
          type: 'tag',
          item: { name: tag, files, count: files.length },
          score,
          match,
        }
      })
      .filter(r => r.match)
      .sort((a, b) => b.score - a.score)
      .slice(0, 10)
  }

  // File mode: search files
  const files = flattenTree(state.tree)
  const fileResults = files
    .map(f => {
      const { match, score } = fuzzyMatch(trimmed, f.name)
      return { type: 'file', item: f, score, match }
    })
    .filter(r => r.match)

  const headingResults = trimmed
    ? getHeadingResults(trimmed, files)
    : []

  return [...fileResults, ...headingResults]
    .sort((a, b) => b.score - a.score)
    .slice(0, 10)
}

function getHeadingResults(query, files) {
  const index = getLinkIndex()
  return files.flatMap(f => {
    const content = index.files.get(f.path)?.content || ''
    return extractHeadings(content).map(heading => {
      const { match, score } = fuzzyMatch(query, `${f.name} ${heading.text}`)
      return {
        type: 'heading',
        item: {
          path: f.path,
          name: f.name,
          heading: {
            text: heading.text,
            line: heading.line,
            level: heading.level,
          },
        },
        score: score + 4,
        match,
      }
    })
  }).filter(result => result.match)
}

// ── Render results ───────────────────────────────────────────────
function renderResults(results) {
  if (!resultsEl) return
  resultsEl.innerHTML = ''

  if (results.length === 0) {
    const empty = el('div', 'cmd-palette__empty', 'No results')
    resultsEl.appendChild(empty)
    return
  }

  results.forEach((r, i) => {
    const row = el('div', `cmd-palette__item${i === activeIndex ? ' active' : ''}`)
    row.dataset.index = i

    const icon = el('div', 'cmd-palette__icon')
    icon.textContent = r.type === 'file' ? '#' : r.type === 'tag' ? '•' : r.type === 'heading' ? 'H' : '>'

    const info = el('div', 'cmd-palette__info')
    const label = el('div', 'cmd-palette__label')
    if (r.type === 'file') label.textContent = r.item.name
    else if (r.type === 'tag') label.textContent = '#' + r.item.name
    else if (r.type === 'heading') label.textContent = r.item.heading.text
    else label.textContent = r.item.label

    info.appendChild(label)

    const desc = r.type === 'command'
      ? r.item.description
      : r.type === 'tag'
        ? `${r.item.count} file${r.item.count === 1 ? '' : 's'}`
        : r.type === 'heading'
          ? `${relativePath(r.item.path)}:${r.item.heading.line}`
          : relativePath(r.item.path)
    if (desc) {
      const descEl = el('div', 'cmd-palette__desc')
      descEl.textContent = desc
      info.appendChild(descEl)
    }

    row.appendChild(icon)
    row.appendChild(info)

    if (r.type === 'command' && r.item.shortcut) {
      const kbd = el('div', 'cmd-palette__shortcut')
      kbd.textContent = r.item.shortcut
      row.appendChild(kbd)
    }

    row.addEventListener('mouseenter', () => {
      activeIndex = i
      highlightActive()
    })

    row.addEventListener('click', e => {
      e.stopPropagation()
      selectResult(results[i])
    })

    resultsEl.appendChild(row)
  })
}

function highlightActive() {
  if (!resultsEl) return
  const items = resultsEl.querySelectorAll('.cmd-palette__item')
  items.forEach((item, i) => {
    if (i === activeIndex) item.classList.add('active')
    else item.classList.remove('active')
  })
  // Scroll active into view
  const activeEl = resultsEl.querySelector('.cmd-palette__item.active')
  if (activeEl) activeEl.scrollIntoView({ block: 'nearest' })
}

function relativePath(filePath) {
  if (!state.folderPath || !filePath) return filePath || ''
  if (filePath.startsWith(state.folderPath)) {
    return filePath.slice(state.folderPath.length + 1)
  }
  return filePath
}

// ── Select ───────────────────────────────────────────────────────
function selectResult(result) {
  if (result.type === 'tag') {
    // Show files for this tag as a sub-search
    if (inputEl) {
      const files = result.item.files
      if (files.length === 1) {
        // Single file: open it directly
        closeCommandPalette()
        if (_callbacks.openFile) {
          const name = files[0].split(/[/\\]/).pop()
          _callbacks.openFile({ path: files[0], name })
        }
      } else {
        // Show files with this tag: clear input and show filtered list
        _tagFiles = files
        inputEl.value = ''
        inputEl.placeholder = `Files tagged #${result.item.name}...`
        activeIndex = 0
        renderResults(getResults(''))
      }
    }
    return
  }

  closeCommandPalette()
  _tagFiles = null
  if (result.type === 'file' && _callbacks.openFile) {
    _callbacks.openFile(result.item)
  } else if (result.type === 'heading' && _callbacks.openFile) {
    _callbacks.openFile({ path: result.item.path, name: result.item.name, heading: result.item.heading })
  } else if (result.type === 'command' && result.item.action) {
    result.item.action()
  }
}

// ── Build DOM ────────────────────────────────────────────────────
function buildPalette() {
  if (overlayEl) return

  // Overlay backdrop
  overlayEl = el('div', 'cmd-palette__overlay')
  overlayEl.addEventListener('click', () => closeCommandPalette())

  // Palette container
  paletteEl = el('div', 'cmd-palette')

  // Input
  inputEl = document.createElement('input')
  inputEl.type = 'text'
  inputEl.className = 'cmd-palette__input'
  inputEl.placeholder = 'Search files, # for tags, > for commands...'
  inputEl.spellcheck = false
  inputEl.autocomplete = 'off'

  inputEl.addEventListener('input', () => {
    activeIndex = 0
    const results = getResults(inputEl.value)
    renderResults(results)
  })

  inputEl.addEventListener('keydown', e => {
    const results = getResults(inputEl.value)

    if (e.key === 'ArrowDown') {
      e.preventDefault()
      activeIndex = Math.min(activeIndex + 1, results.length - 1)
      highlightActive()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      activeIndex = Math.max(activeIndex - 1, 0)
      highlightActive()
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (results[activeIndex]) selectResult(results[activeIndex])
    } else if (e.key === 'Escape') {
      e.preventDefault()
      closeCommandPalette()
    }
  })

  // Results container
  resultsEl = el('div', 'cmd-palette__results')

  paletteEl.appendChild(inputEl)
  paletteEl.appendChild(resultsEl)

  const root = $('root')
  if (root) {
    root.appendChild(overlayEl)
    root.appendChild(paletteEl)
  }
}

function destroyPalette() {
  if (overlayEl) { overlayEl.remove(); overlayEl = null }
  if (paletteEl) { paletteEl.remove(); paletteEl = null }
  inputEl = null
  resultsEl = null
}

// ── Lifecycle ────────────────────────────────────────────────────
export function openCommandPalette(initialValue = '') {
  if (state.commandPaletteOpen) return
  state.commandPaletteOpen = true

  buildPalette()
  overlayEl.classList.add('open')
  paletteEl.classList.add('open')

  activeIndex = 0
  if (inputEl) inputEl.value = initialValue
  const results = getResults(initialValue)
  renderResults(results)

  requestAnimationFrame(() => inputEl?.focus())
}

export function openCommandPaletteFiles() {
  openCommandPalette('')
}

export function openCommandPaletteCommands() {
  openCommandPalette('>')
}

export function closeCommandPalette() {
  if (!state.commandPaletteOpen) return
  state.commandPaletteOpen = false
  _tagFiles = null

  if (overlayEl) overlayEl.classList.remove('open')
  if (paletteEl) paletteEl.classList.remove('open')

  // Destroy after transition
  setTimeout(() => {
    if (!state.commandPaletteOpen) destroyPalette()
  }, 200)
}

export function toggleCommandPalette() {
  if (state.commandPaletteOpen) closeCommandPalette()
  else openCommandPalette()
}
