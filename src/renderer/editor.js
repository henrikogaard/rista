import { EditorState, Compartment, Annotation } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, drawSelection, dropCursor, highlightActiveLine } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { languages } from '@codemirror/language-data'
import { syntaxHighlighting, HighlightStyle } from '@codemirror/language'
import { tags } from '@lezer/highlight'
import { vim } from '@replit/codemirror-vim'
import { selectNextOccurrence } from '@codemirror/search'
import { autocompletion, CompletionContext } from '@codemirror/autocomplete'
import { getAllMdFileNames } from './link-index.js'
import { checkTableAtCursor, hideTableToolbar } from './table-editor.js'

// ── Minimal highlight style matching Fjordmark palette ──
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

export function createEditor({ parent, doc = '', onChange, onSelectionChange, onPaste, onRichPaste, isDark = true, typewriterEnabled = false, spellcheckEnabled = false, vimEnabled = false }) {
  const state = EditorState.create({
    doc,
    extensions: [
      history(),
      drawSelection(),
      dropCursor(),
      EditorView.lineWrapping,
      keymap.of([{ key: 'Mod-d', run: selectNextOccurrence }, ...defaultKeymap, ...historyKeymap, indentWithTab]),
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      syntaxHighlighting(fjordHighlight),
      wikilinkCompletion,
      themeCompartment.of(isDark ? fjordThemeDark : fjordThemeLight),
      typewriterCompartment.of(typewriterEnabled ? typewriterExtension : typewriterOff),
      spellcheckCompartment.of(spellcheckEnabled ? spellcheckOn : spellcheckOff),
      vimCompartment.of(vimEnabled ? vim() : vimOff),
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
