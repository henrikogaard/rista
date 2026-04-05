import { EditorState, Compartment } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, drawSelection, dropCursor, highlightActiveLine } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { markdown, markdownLanguage } from '@codemirror/lang-markdown'
import { languages } from '@codemirror/language-data'
import { syntaxHighlighting, HighlightStyle } from '@codemirror/language'
import { tags } from '@lezer/highlight'

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

export function createEditor({ parent, doc = '', onChange, onSelectionChange, onPaste, isDark = true }) {
  const state = EditorState.create({
    doc,
    extensions: [
      history(),
      drawSelection(),
      dropCursor(),
      EditorView.lineWrapping,
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
      markdown({ base: markdownLanguage, codeLanguages: languages }),
      syntaxHighlighting(fjordHighlight),
      themeCompartment.of(isDark ? fjordThemeDark : fjordThemeLight),
      EditorView.updateListener.of(update => {
        if (update.docChanged && onChange) {
          onChange(update.state.doc.toString())
        }
        if ((update.docChanged || update.selectionSet || update.focusChanged) && onSelectionChange) {
          onSelectionChange(update.state)
        }
      }),
    ],
  })

  const view = new EditorView({ state, parent })

  // Handle paste events for images
  if (onPaste) {
    parent.addEventListener('paste', async (e) => {
      const items = e.clipboardData?.items
      if (!items) return

      for (let item of items) {
        if (item.kind === 'file' && item.type.startsWith('image/')) {
          e.preventDefault()
          const file = item.getAsFile()
          if (file && onPaste) {
            await onPaste(file, view)
          }
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
  })
}

export function updateEditorTheme(view, isDark) {
  const newTheme = isDark ? fjordThemeDark : fjordThemeLight
  view.dispatch({
    effects: themeCompartment.reconfigure(newTheme),
  })
}
