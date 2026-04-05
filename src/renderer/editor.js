import { EditorState } from '@codemirror/state'
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

const fjordTheme = EditorView.theme({
  '&': {
    height: '100%',
    backgroundColor: 'var(--bg0)',
    color: 'var(--text2)',
  },
  '.cm-scroller': {
    fontFamily: 'var(--mono)',
    fontSize: '13px',
    lineHeight: '1.75',
    padding: '20px 24px',
    overflowX: 'auto',
  },
  '.cm-content': { caretColor: 'var(--text1)' },
  '.cm-cursor': { borderLeftColor: 'var(--text1)' },
  '.cm-activeLine': { backgroundColor: 'transparent' },
  '.cm-selectionBackground, ::selection': { backgroundColor: 'rgba(91,127,166,0.25) !important' },
  '.cm-gutters': { display: 'none' },
  '.cm-focused': { outline: 'none' },
}, { dark: true })

export function createEditor({ parent, doc = '', onChange }) {
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
      fjordTheme,
      EditorView.updateListener.of(update => {
        if (update.docChanged && onChange) {
          onChange(update.state.doc.toString())
        }
      }),
    ],
  })

  const view = new EditorView({ state, parent })
  return view
}

export function updateEditorDoc(view, doc) {
  const current = view.state.doc.toString()
  if (current === doc) return
  view.dispatch({
    changes: { from: 0, to: current.length, insert: doc },
  })
}
