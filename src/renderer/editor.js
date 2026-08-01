import { EditorState, Compartment, Annotation, RangeSetBuilder } from '@codemirror/state'
import { EditorView, keymap, drawSelection, dropCursor, Decoration, ViewPlugin, WidgetType } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { languages } from '@codemirror/language-data'
import { syntaxHighlighting, HighlightStyle, syntaxTree } from '@codemirror/language'
import { tags } from '@lezer/highlight'
import { vim } from '@replit/codemirror-vim'
import { selectNextOccurrence } from '@codemirror/search'
import { autocompletion } from '@codemirror/autocomplete'
import { getAllMdFileNames } from './link-index.js'
import { checkTableAtCursor, tableTabForward, tableTabBackward } from './table-editor.js'

// ── Minimal highlight style matching Rísta palette ──
const fjordHighlight = HighlightStyle.define([
  { tag: tags.heading1,         color: 'var(--text1)', fontWeight: '500' },
  { tag: tags.heading2,         color: 'var(--text1)', fontWeight: '500' },
  { tag: tags.heading3,         color: 'var(--text2)', fontWeight: '500' },
  { tag: tags.strong,           color: 'var(--text1)', fontWeight: '500' },
  { tag: tags.emphasis,         color: 'var(--text1)', fontStyle: 'italic' },
  { tag: tags.link,             color: 'var(--accent)' },
  { tag: tags.url,              color: 'var(--accent)', textDecoration: 'underline' },
  { tag: tags.monospace,        color: 'var(--accent-hi)', fontFamily: 'var(--mono)' },
  { tag: tags.strikethrough,    color: 'var(--text3)', textDecoration: 'line-through' },
  { tag: tags.comment,          color: 'var(--text3)' },
  { tag: tags.meta,             color: 'var(--text3)' },
  { tag: tags.processingInstruction, color: 'var(--text3)' },
])

const fjordThemeDark = EditorView.theme({
  '&': {
    height: '100%',
    backgroundColor: 'var(--surface-bg-0)',
    color: 'var(--editor-text-color)',
  },
  '.cm-scroller': {
    fontFamily: 'var(--editor-font)',
    fontSize: 'var(--editor-font-size)',
    lineHeight: 'var(--editor-line-height)',
    padding: '24px 28px 28px',
    overflowX: 'auto',
  },
  '.cm-content': { caretColor: 'var(--editor-text-color)' },
  '.cm-line': { padding: '0 2px', color: 'var(--editor-text-color)' },
  '.cm-cursor': { borderLeftColor: 'var(--editor-text-color)' },
  '.cm-activeLine': { backgroundColor: 'rgba(255,255,255,0.015)' },
  '.cm-selectionBackground, ::selection': { backgroundColor: 'rgba(91,127,166,0.25) !important' },
  '.cm-gutters': { display: 'none' },
  '.cm-focused': { outline: 'none' },
}, { dark: true })

const fjordThemeLight = EditorView.theme({
  '&': {
    height: '100%',
    backgroundColor: 'var(--surface-bg-0)',
    color: 'var(--editor-text-color)',
  },
  '.cm-scroller': {
    fontFamily: 'var(--editor-font)',
    fontSize: 'var(--editor-font-size)',
    lineHeight: 'var(--editor-line-height)',
    padding: '24px 28px 28px',
    overflowX: 'auto',
  },
  '.cm-content': { caretColor: 'var(--editor-text-color)' },
  '.cm-line': { padding: '0 2px', color: 'var(--editor-text-color)' },
  '.cm-cursor': { borderLeftColor: 'var(--editor-text-color)' },
  '.cm-activeLine': { backgroundColor: 'rgba(53,42,30,0.035)' },
  '.cm-selectionBackground, ::selection': { backgroundColor: 'rgba(58,106,154,0.15) !important' },
  '.cm-gutters': { display: 'none' },
  '.cm-focused': { outline: 'none' },
}, { dark: false })

export const themeCompartment = new Compartment()

export const typewriterCompartment = new Compartment()

export const spellcheckCompartment = new Compartment()

const spellcheckOn = EditorView.contentAttributes.of({ spellcheck: 'true' })
const spellcheckOff = EditorView.contentAttributes.of({ spellcheck: 'false' })

export const vimCompartment = new Compartment()
const vimOff = []

const programmaticDocUpdate = Annotation.define()

const wikilinkCompletion = autocompletion({
  override: [
    (context) => {
      const word = context.matchBefore(/\[\[[^\]]*/)
      if (!word || (word.from === word.to && !context.explicit)) return null
      const query = word.text.slice(2).toLowerCase()
      const names = getAllMdFileNames()
      const options = names
        .filter(name => name.toLowerCase().includes(query))
        .map(name => ({
          label: name,
          apply: (view, _completion, from, to) => {
            const insert = `[[${name}]]`
            view.dispatch({
              changes: { from, to, insert },
              selection: { anchor: from + insert.length },
            })
          },
        }))
      return {
        from: word.from,
        options,
        filter: false,
      }
    },
  ],
  defaultKeymap: true,
  icons: false,
})

const typewriterExtension = EditorView.updateListener.of(update => {
  if (!update.docChanged && !update.selectionSet) return
  const view = update.view
  const head = update.state.selection.main.head
  const coords = view.coordsAtPos(head)
  if (!coords) return
  const editorRect = view.dom.getBoundingClientRect()
  const targetY = editorRect.top + editorRect.height / 2
  const diff = coords.top - targetY
  if (Math.abs(diff) > 10) {
    view.scrollDOM.scrollBy({ top: diff, behavior: 'smooth' })
  }
})

const typewriterOff = []

// ── Smart typography (#43) ───────────────────────────────────────
// Transforms "straight" ASCII punctuation into typographic equivalents
// as the user types. Runs only when the setting is enabled.
export const smartTypographyCompartment = new Compartment()

function isOpenQuoteContext(view, pos) {
  if (pos === 0) return true
  const before = view.state.doc.sliceString(Math.max(0, pos - 1), pos)
  return /[\s\u00a0(\[{—–\-]/.test(before) || before === ''
}

const smartTypographyExtension = EditorView.inputHandler.of((view, from, to, text) => {
  // Double dash → em dash
  if (text === '-') {
    const before = view.state.doc.sliceString(Math.max(0, from - 1), from)
    if (before === '-') {
      view.dispatch({
        changes: { from: from - 1, to, insert: '\u2014' },
        selection: { anchor: from },
      })
      return true
    }
  }
  // Ellipsis
  if (text === '.') {
    const before = view.state.doc.sliceString(Math.max(0, from - 2), from)
    if (before === '..') {
      view.dispatch({
        changes: { from: from - 2, to, insert: '\u2026' },
        selection: { anchor: from - 1 },
      })
      return true
    }
  }
  // Smart double quotes
  if (text === '"') {
    const quote = isOpenQuoteContext(view, from) ? '\u201c' : '\u201d'
    view.dispatch({ changes: { from, to, insert: quote }, selection: { anchor: from + 1 } })
    return true
  }
  // Smart single quotes (skip apostrophes in contractions)
  if (text === "'") {
    const charBefore = from > 0 ? view.state.doc.sliceString(from - 1, from) : ''
    // Apostrophe in contraction (letter before): use closing/apostrophe form
    if (/[a-zA-Z]/.test(charBefore)) {
      view.dispatch({ changes: { from, to, insert: '\u2019' }, selection: { anchor: from + 1 } })
      return true
    }
    const quote = isOpenQuoteContext(view, from) ? '\u2018' : '\u2019'
    view.dispatch({ changes: { from, to, insert: quote }, selection: { anchor: from + 1 } })
    return true
  }
  return false
})

const smartTypographyOff = []

export function createEditor({ parent, doc = '', onChange, onSelectionChange, onPaste, onRichPaste, isDark = true, typewriterEnabled = false, spellcheckEnabled = false, vimEnabled = false, smartTypographyEnabled = true, focusModeEnabled = false, livePreviewEnabled = false, posHighlightEnabled = false }) {
  const state = EditorState.create({
    doc,
    extensions: [
      history(),
      drawSelection(),
      dropCursor(),
      EditorView.lineWrapping,
      keymap.of([
        { key: 'Mod-d', run: selectNextOccurrence },
        // ── Markdown formatting shortcuts (#50) ──────────────────
        { key: 'Mod-b', run: view => { mdWrap(view, '**', '**'); return true } },
        { key: 'Mod-i', run: view => { mdWrap(view, '*', '*');   return true } },
        { key: 'Mod-`', run: view => { mdWrap(view, '`', '`');   return true } },
        { key: 'Mod-Shift-s', run: view => { mdWrap(view, '~~', '~~'); return true } },
        // ── Table cell navigation (#52) ──────────────────────────
        { key: 'Tab',       run: view => tableTabForward(view) },
        { key: 'Shift-Tab', run: view => tableTabBackward(view) },
        ...defaultKeymap, ...historyKeymap, indentWithTab,
      ]),
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      syntaxHighlighting(fjordHighlight),
      wikilinkCompletion,
      themeCompartment.of(isDark ? fjordThemeDark : fjordThemeLight),
      typewriterCompartment.of(typewriterEnabled ? typewriterExtension : typewriterOff),
      spellcheckCompartment.of(spellcheckEnabled ? spellcheckOn : spellcheckOff),
      vimCompartment.of(vimEnabled ? vim() : vimOff),
      smartTypographyCompartment.of(smartTypographyEnabled ? smartTypographyExtension : smartTypographyOff),
      focusModeCompartment.of(focusModeEnabled ? focusModePlugin : focusModeOff),
      livePreviewCompartment.of(livePreviewEnabled ? livePreviewPlugin : livePreviewOff),
      posHighlightCompartment.of(posHighlightEnabled ? posHighlightPlugin : posHighlightOff),
      EditorView.updateListener.of(update => {
        const isProgrammatic = update.transactions.some(transaction => transaction.annotation(programmaticDocUpdate))
        if (update.docChanged && onChange && !isProgrammatic) {
          onChange(update.state.doc.toString())
        }
        if ((update.docChanged || update.selectionSet || update.focusChanged) && onSelectionChange) {
          onSelectionChange(update.state)
        }
        if (update.selectionSet || update.docChanged) {
          checkTableAtCursor(update.view)
        }
      }),
    ],
  })

  const view = new EditorView({ state, parent })

  // Handle paste events for images and rich text
  if (onPaste || onRichPaste) {
    parent.addEventListener('paste', async (e) => {
      const items = e.clipboardData?.items
      if (!items) return

      // Check for images first
      for (let item of items) {
        if (item.kind === 'file' && item.type.startsWith('image/')) {
          e.preventDefault()
          const file = item.getAsFile()
          if (file && onPaste) await onPaste(file, view)
          return
        }
      }

      // Check for rich text (HTML) - convert to Markdown
      const htmlData = e.clipboardData?.getData('text/html')
      if (htmlData && onRichPaste) {
        e.preventDefault()
        onRichPaste(htmlData, view)
      }
    })
  }

  // Handle drag-and-drop images
  if (onPaste) {
    parent.addEventListener('dragover', (e) => {
      if (e.dataTransfer?.types?.includes('Files')) {
        e.preventDefault()
        parent.classList.add('drag-over')
      }
    })

    parent.addEventListener('dragleave', (e) => {
      parent.classList.remove('drag-over')
    })

    parent.addEventListener('drop', async (e) => {
      parent.classList.remove('drag-over')
      const files = e.dataTransfer?.files
      if (!files) return

      for (const file of files) {
        if (file.type.startsWith('image/')) {
          e.preventDefault()
          await onPaste(file, view)
        }
      }
    })
  }

  return view
}

export function updateEditorDoc(view, doc) {
  const current = view.state.doc.toString()
  if (current === doc) return
  view.dispatch({
    changes: { from: 0, to: current.length, insert: doc },
    annotations: programmaticDocUpdate.of(true),
  })
}

export function updateEditorTheme(view, isDark) {
  const newTheme = isDark ? fjordThemeDark : fjordThemeLight
  view.dispatch({
    effects: themeCompartment.reconfigure(newTheme),
  })
}

export function updateTypewriterMode(view, enabled) {
  view.dispatch({
    effects: typewriterCompartment.reconfigure(enabled ? typewriterExtension : typewriterOff),
  })
}

export function updateSpellcheck(view, enabled) {
  view.dispatch({
    effects: spellcheckCompartment.reconfigure(enabled ? spellcheckOn : spellcheckOff),
  })
}

export function updateVimMode(view, enabled) {
  view.dispatch({
    effects: vimCompartment.reconfigure(enabled ? vim() : vimOff),
  })
}

export function updateSmartTypography(view, enabled) {
  view.dispatch({
    effects: smartTypographyCompartment.reconfigure(enabled ? smartTypographyExtension : smartTypographyOff),
  })
}

// ── Focus mode (#45) ─────────────────────────────────────────────
// Dims all lines outside the active paragraph. A "paragraph" is the
// contiguous block of non-blank lines containing the cursor.
export const focusModeCompartment = new Compartment()

const dimDecoration = Decoration.line({ class: 'cm-focus-dim' })

function buildFocusDecorations(view) {
  const builder = new RangeSetBuilder()
  const { head } = view.state.selection.main
  const cursorLine = view.state.doc.lineAt(head)

  // Find the paragraph block boundaries (blank lines delimit paragraphs)
  let blockStart = cursorLine.number
  let blockEnd = cursorLine.number
  const lineCount = view.state.doc.lines

  while (blockStart > 1) {
    const prev = view.state.doc.line(blockStart - 1)
    if (prev.text.trim() === '') break
    blockStart--
  }
  while (blockEnd < lineCount) {
    const next = view.state.doc.line(blockEnd + 1)
    if (next.text.trim() === '') break
    blockEnd++
  }

  for (let i = 1; i <= lineCount; i++) {
    if (i < blockStart || i > blockEnd) {
      const line = view.state.doc.line(i)
      builder.add(line.from, line.from, dimDecoration)
    }
  }
  return builder.finish()
}

const focusModePlugin = ViewPlugin.fromClass(
  class {
    decorations
    constructor(view) {
      this.decorations = buildFocusDecorations(view)
    }
    update(update) {
      if (update.docChanged || update.selectionSet || update.viewportChanged) {
        this.decorations = buildFocusDecorations(update.view)
      }
    }
  },
  { decorations: v => v.decorations }
)

const focusModeOff = []

export function updateFocusMode(view, enabled) {
  view.dispatch({
    effects: focusModeCompartment.reconfigure(enabled ? focusModePlugin : focusModeOff),
  })
}

// ── Live preview / syntax hiding (#49) ──────────────────────────
// Hides Markdown syntax marks on lines NOT containing the cursor,
// giving a Typora-style "live preview" feel in the source editor.
export const livePreviewCompartment = new Compartment()

// Map heading node names → CSS class for visual sizing
const HEADING_CLASS = {
  ATXHeading1: 'cm-live-h1',
  ATXHeading2: 'cm-live-h2',
  ATXHeading3: 'cm-live-h3',
  ATXHeading4: 'cm-live-h4',
  ATXHeading5: 'cm-live-h5',
  ATXHeading6: 'cm-live-h6',
}

// Node types whose subtrees we never decorate (raw display always)
const SKIP_SUBTREE = new Set(['FencedCode', 'CodeBlock', 'HTMLBlock', 'CommentBlock'])

class HRWidget extends WidgetType {
  toDOM() {
    const el = document.createElement('div')
    el.className = 'cm-live-hr'
    el.setAttribute('aria-hidden', 'true')
    return el
  }
  eq() { return true }
  ignoreEvent() { return true }
}

class ImageWidget extends WidgetType {
  constructor(src, alt) { super(); this.src = src; this.alt = alt }
  toDOM() {
    const wrap = document.createElement('span')
    wrap.className = 'cm-live-img'
    const img = document.createElement('img')
    img.src = this.src
    img.alt = this.alt
    img.className = 'cm-live-img__el'
    img.loading = 'lazy'
    img.onerror = () => { wrap.classList.add('cm-live-img--broken') }
    wrap.appendChild(img)
    return wrap
  }
  eq(other) { return other.src === this.src && other.alt === this.alt }
  ignoreEvent() { return false }
}

function buildLivePreviewDecorations(view) {
  const { state } = view
  const builder = new RangeSetBuilder()

  // Collect line numbers that contain any cursor / selection endpoint
  const activeLines = new Set()
  for (const range of state.selection.ranges) {
    const fromLine = state.doc.lineAt(range.from).number
    const toLine = state.doc.lineAt(range.to).number
    for (let ln = fromLine; ln <= toLine; ln++) activeLines.add(ln)
  }

  const { from: vpFrom, to: vpTo } = view.viewport
  const pending = []

  syntaxTree(state).iterate({
    from: vpFrom,
    to: vpTo,
    enter(node) {
      // Never decorate inside code/HTML blocks
      if (SKIP_SUBTREE.has(node.name)) return false

      const lineNum = state.doc.lineAt(node.from).number

      // Heading containers: add line class, then let children (HeaderMark) run
      const hClass = HEADING_CLASS[node.name]
      if (hClass) {
        if (activeLines.has(lineNum)) return false // skip whole subtree
        const line = state.doc.lineAt(node.from)
        pending.push({ from: line.from, to: line.from, deco: Decoration.line({ class: hClass }) })
        return // continue into children
      }

      // For all other nodes: skip if cursor is on this line
      if (activeLines.has(lineNum)) return false

      switch (node.name) {
        case 'HeaderMark': {
          // Hide "# " including the trailing space
          let end = node.to
          if (state.doc.sliceString(node.to, node.to + 1) === ' ') end++
          pending.push({ from: node.from, to: end, deco: Decoration.replace({}) })
          break
        }
        case 'EmphasisMark':
          pending.push({ from: node.from, to: node.to, deco: Decoration.replace({}) })
          break
        case 'CodeMark':
          // Only hide backticks for inline code, not fenced (already excluded above)
          if (node.node.parent?.name === 'InlineCode') {
            pending.push({ from: node.from, to: node.to, deco: Decoration.replace({}) })
          }
          break
        case 'HorizontalRule': {
          const line = state.doc.lineAt(node.from)
          pending.push({ from: line.from, to: line.to, deco: Decoration.replace({ widget: new HRWidget() }) })
          break
        }
        case 'LinkMark':
          // Hide [ ] ( ) markers; keep link text visible
          if (node.node.parent?.name === 'Link') {
            pending.push({ from: node.from, to: node.to, deco: Decoration.replace({}) })
          }
          // For Image nodes, we handle the whole replacement below — skip individual marks
          break
        case 'URL':
          // Hide the URL inside a link — link text stays visible
          if (node.node.parent?.name === 'Link') {
            pending.push({ from: node.from, to: node.to, deco: Decoration.replace({}) })
          }
          break
        case 'Image': {
          // Replace ![alt](src) with an actual <img> widget (#51)
          // Walk children to extract alt text and URL
          let imgSrc = ''
          let imgAlt = ''
          let child = node.node.firstChild
          while (child) {
            if (child.name === 'URL') {
              imgSrc = state.doc.sliceString(child.from, child.to)
            } else if (child.name === 'LinkLabel') {
              // LinkLabel spans from "[" to "]", get inner text
              imgAlt = state.doc.sliceString(child.from + 1, child.to - 1)
            }
            child = child.nextSibling
          }
          if (imgSrc) {
            pending.push({ from: node.from, to: node.to, deco: Decoration.replace({ widget: new ImageWidget(imgSrc, imgAlt) }) })
          }
          return false // don't process children (LinkMark, URL already covered)
        }
        case 'QuoteMark':
          // Hide "> " blockquote prefix
          pending.push({ from: node.from, to: node.to + (state.doc.sliceString(node.to, node.to + 1) === ' ' ? 1 : 0), deco: Decoration.replace({}) })
          break
      }
    },
  })

  // Sort: same `from` → zero-width (line decos) before span decos
  pending.sort((a, b) => {
    if (a.from !== b.from) return a.from - b.from
    return (a.to - a.from) - (b.to - b.from)
  })

  let prevFrom = -1
  let prevTo = -1
  for (const { from, to, deco } of pending) {
    // Skip exact duplicates
    if (from === prevFrom && to === prevTo) continue
    builder.add(from, to, deco)
    prevFrom = from
    prevTo = to
  }
  return builder.finish()
}

const livePreviewPlugin = ViewPlugin.fromClass(
  class {
    decorations
    constructor(view) {
      this.decorations = buildLivePreviewDecorations(view)
    }
    update(update) {
      if (update.docChanged || update.selectionSet || update.viewportChanged) {
        this.decorations = buildLivePreviewDecorations(update.view)
      }
    }
  },
  { decorations: v => v.decorations }
)

const livePreviewOff = []

export function updateLivePreview(view, enabled) {
  view.dispatch({
    effects: livePreviewCompartment.reconfigure(enabled ? livePreviewPlugin : livePreviewOff),
  })
}

// ── Markdown formatting helper (used by keymap shortcuts) ────────
function mdWrap(view, before, after) {
  const { state: s } = view
  const sel = s.selection.main
  const selected = s.sliceDoc(sel.from, sel.to)
  // Toggle: if selection is already wrapped, unwrap it
  if (selected.startsWith(before) && selected.endsWith(after) && selected.length > before.length + after.length) {
    const inner = selected.slice(before.length, selected.length - after.length)
    view.dispatch(s.update({
      changes: { from: sel.from, to: sel.to, insert: inner },
      selection: { anchor: sel.from, head: sel.from + inner.length },
    }))
  } else {
    const insert = `${before}${selected || 'text'}${after}`
    view.dispatch(s.update({
      changes: { from: sel.from, to: sel.to, insert },
      selection: { anchor: sel.from + before.length, head: sel.from + before.length + (selected || 'text').length },
    }))
  }
  view.focus()
}

// ── Parts-of-speech highlighting (#57) ──────────────────────────
// Client-side regex marks three categories:
//   cm-pos-adverb    — words ending in -ly (often weakeners)
//   cm-pos-passive   — passive voice constructions (was/were/been + past participle)
//   cm-pos-complex   — sentences longer than 30 words (hard to parse)
export const posHighlightCompartment = new Compartment()

const ADVERB_RE = /\b\w+ly\b/g
// Passive: (was|were|is|are|been|be)\s+\w+ed
const PASSIVE_RE = /\b(?:was|were|is|are|has been|have been|had been|be|being)\s+\w+(?:ed|en)\b/gi
// Sentence boundary
const SENTENCE_RE = /[^.!?\n]+[.!?]?/g

function buildPosDecorations(view) {
  const builder = new RangeSetBuilder()
  const pending = []
  const { from: vpFrom, to: vpTo } = view.viewport
  const text = view.state.doc.sliceString(vpFrom, vpTo)
  const base = vpFrom

  // Skip inside fenced code blocks
  const fenced = new Set()
  let inFence = false
  view.state.doc.iterLines(
    view.state.doc.lineAt(vpFrom).number,
    view.state.doc.lineAt(vpTo).number + 1,
    (lineText) => {
      if (/^```/.test(lineText)) inFence = !inFence
    }
  )
  // Simple approach: mark entire code blocks to skip — just check per-match later

  // Adverbs
  let m
  ADVERB_RE.lastIndex = 0
  while ((m = ADVERB_RE.exec(text)) !== null) {
    const from = base + m.index
    const to = from + m[0].length
    pending.push({ from, to, cls: 'cm-pos-adverb' })
  }

  // Passive voice
  PASSIVE_RE.lastIndex = 0
  while ((m = PASSIVE_RE.exec(text)) !== null) {
    const from = base + m.index
    const to = from + m[0].length
    pending.push({ from, to, cls: 'cm-pos-passive' })
  }

  // Long sentences (>30 words) — mark the full sentence span
  SENTENCE_RE.lastIndex = 0
  while ((m = SENTENCE_RE.exec(text)) !== null) {
    const words = m[0].trim().split(/\s+/).filter(Boolean)
    if (words.length > 30) {
      const from = base + m.index
      const to = from + m[0].length
      pending.push({ from, to, cls: 'cm-pos-complex' })
    }
  }

  // Filter out ranges inside code/frontmatter lines and sort
  pending.sort((a, b) => a.from - b.from || a.to - b.to)

  let lastTo = -1
  for (const { from, to, cls } of pending) {
    if (from < lastTo) continue // skip overlapping marks
    // Check if this range is inside a code span (backtick-delimited)
    const lineText = view.state.doc.lineAt(from).text
    // skip if the line starts with 4 spaces (indented code), ``` or is frontmatter
    if (/^(?:\s{4}|```|~~~|\s*---)/.test(lineText)) continue
    builder.add(from, to, Decoration.mark({ class: cls }))
    lastTo = to
  }
  return builder.finish()
}

const posHighlightPlugin = ViewPlugin.fromClass(
  class {
    decorations
    constructor(view) { this.decorations = buildPosDecorations(view) }
    update(update) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildPosDecorations(update.view)
      }
    }
  },
  { decorations: v => v.decorations }
)

const posHighlightOff = []

export function updatePosHighlight(view, enabled) {
  view.dispatch({
    effects: posHighlightCompartment.reconfigure(enabled ? posHighlightPlugin : posHighlightOff),
  })
}
