import { $, state, editorViews, richEditors, syncingRichEditor, paneUsesWysiwyg, getTabForPane, getWysiwygMountSlot, getFocusedEditor } from './state.js'
import { undo, redo } from '@codemirror/commands'

// ── Registration hooks for functions that live in index.js ───────
let _ensureRichEditorMounted = null
let _focusPane = null

export function registerEnsureRichEditorMounted(fn) { _ensureRichEditorMounted = fn }
export function registerFocusPane(fn) { _focusPane = fn }

// ── Path utilities ───────────────────────────────────────────────
export function normalizePathSeparators(value = '') {
  return String(value).replace(/\\/g, '/')
}

export function lastPathSegment(value = '') {
  const normalized = normalizePathSeparators(value)
  const parts = normalized.split('/').filter(Boolean)
  return parts[parts.length - 1] || ''
}

export function stripFileExtension(value = '') {
  return String(value).replace(/\.[^.]+$/, '')
}

export function directoryPath(filePath = '') {
  const normalized = normalizePathSeparators(filePath)
  const index = normalized.lastIndexOf('/')
  return index >= 0 ? normalized.slice(0, index) : ''
}

export function toRelativePath(fromFilePath, toFilePath) {
  const fromDir = directoryPath(fromFilePath)
  const fromParts = normalizePathSeparators(fromDir).split('/').filter(Boolean)
  const toParts = normalizePathSeparators(toFilePath).split('/').filter(Boolean)

  if (!fromParts.length) return normalizePathSeparators(toFilePath)

  let shared = 0
  while (
    shared < fromParts.length &&
    shared < toParts.length &&
    fromParts[shared] === toParts[shared]
  ) {
    shared += 1
  }

  const upward = Array.from({ length: fromParts.length - shared }, () => '..')
  const downward = toParts.slice(shared)
  const joined = [...upward, ...downward].join('/')
  return joined || `./${lastPathSegment(toFilePath)}`
}

// ── Command dialog ───────────────────────────────────────────────
export function openCommandDialog(type, options = {}) {
  state.commandDialog = { type, pane: state.focusedPane, ...options }
  const app = $('app')
  const body = $('command-dialog-body')
  const title = $('command-dialog-title')
  const eyebrow = $('command-dialog-eyebrow')
  const submit = $('command-dialog-submit')
  if (!app || !body || !title || !eyebrow || !submit) return

  if (type === 'link') {
    eyebrow.textContent = 'Insert'
    title.textContent = 'Link'
    submit.textContent = 'Insert link'
    body.innerHTML = `
      <label class="command-field">
        <span class="command-field__label">URL</span>
        <input class="command-field__input" id="command-link-url" name="url" type="text" placeholder="https://example.com" value="${options.url || ''}" />
      </label>
      <label class="command-field">
        <span class="command-field__label">Text</span>
        <input class="command-field__input" id="command-link-text" name="text" type="text" placeholder="Link text" value="${options.text || ''}" />
      </label>
    `
  } else if (type === 'image') {
    eyebrow.textContent = 'Insert'
    title.textContent = 'Image'
    submit.textContent = 'Insert image'
    body.innerHTML = `
      <label class="command-field">
        <span class="command-field__label">Image URL</span>
        <input class="command-field__input" id="command-image-url" name="url" type="text" placeholder="https://example.com/image.png" value="${options.url || ''}" />
      </label>
      <label class="command-field">
        <span class="command-field__label">Alt text</span>
        <input class="command-field__input" id="command-image-alt" name="alt" type="text" placeholder="Describe the image" value="${options.alt || ''}" />
      </label>
    `
  } else if (type === 'table') {
    eyebrow.textContent = 'Insert'
    title.textContent = 'Table'
    submit.textContent = 'Insert table'
    body.innerHTML = `
      <div class="command-grid">
        <label class="command-field">
          <span class="command-field__label">Columns</span>
          <input class="command-field__input" id="command-table-columns" name="columns" type="number" min="2" max="8" step="1" value="${options.columns || 3}" />
        </label>
        <label class="command-field">
          <span class="command-field__label">Rows</span>
          <input class="command-field__input" id="command-table-rows" name="rows" type="number" min="2" max="20" step="1" value="${options.rows || 2}" />
        </label>
      </div>
    `
  }

  app.classList.add('command-dialog-open')
  requestAnimationFrame(() => body.querySelector('input')?.focus())
}

export function closeCommandDialog() {
  state.commandDialog = null
  $('app')?.classList.remove('command-dialog-open')
}

export function submitCommandDialog(event) {
  event.preventDefault()
  const dialog = state.commandDialog
  if (!dialog) return

  const pane = dialog.pane || state.focusedPane
  const form = event.currentTarget
  const data = new FormData(form)

  if (dialog.type === 'link') {
    const url = String(data.get('url') || '').trim()
    const text = String(data.get('text') || '').trim()
    if (!url) return
    if (paneUsesWysiwyg(pane) && richEditors[pane]) {
      _focusPane?.(pane)
      richEditors[pane].focus()
      richEditors[pane].exec('addLink', { linkUrl: url, linkText: text || richEditors[pane].getSelectedText() || url })
    } else {
      insertMarkdownAtSelection(pane, `[${text || 'link'}](${url})`, text ? url.length + 3 : 4)
    }
  }

  if (dialog.type === 'image') {
    const url = String(data.get('url') || '').trim()
    const alt = String(data.get('alt') || '').trim()
    if (!url) return
    if (paneUsesWysiwyg(pane) && richEditors[pane]) {
      _focusPane?.(pane)
      richEditors[pane].focus()
      richEditors[pane].exec('addImage', { imageUrl: url, altText: alt || 'Image' })
    } else {
      insertMarkdownAtSelection(pane, `![${alt || 'alt'}](${url})`, url.length + 1)
    }
  }

  if (dialog.type === 'table') {
    const columns = Number(data.get('columns') || 3)
    const rows = Number(data.get('rows') || 2)
    if (paneUsesWysiwyg(pane) && richEditors[pane]) {
      _focusPane?.(pane)
      richEditors[pane].focus()
      richEditors[pane].exec('addTable', { columnCount: columns, rowCount: rows })
    } else {
      insertMarkdownTable(pane, columns, rows)
    }
  }

  closeCommandDialog()
}

// ── WYSIWYG command helpers ──────────────────────────────────────
export function getFocusedWysiwygEditor() {
  if (!paneUsesWysiwyg(state.focusedPane)) return null
  return richEditors[state.focusedPane] || _ensureRichEditorMounted?.(state.focusedPane)
}

export function runWysiwygCommand(action) {
  const editor = getFocusedWysiwygEditor()
  if (!editor) return false
  editor.focus()
  action(editor)
  return true
}

// ── Editor insert/manipulation functions ─────────────────────────
export function insertMarkdownAtSelection(pane, text, selectLength = 0) {
  const view = editorViews[pane]
  if (!view) return
  const { state: s, dispatch } = view
  const sel = s.selection.main
  dispatch(s.update({
    changes: { from: sel.from, to: sel.to, insert: text },
    selection: { anchor: sel.from + text.length - selectLength, head: sel.from + text.length },
  }))
  view.focus()
}

export function insertMarkdownTable(pane, columns, rows) {
  const safeColumns = Math.max(2, Math.min(8, Number(columns) || 3))
  const safeRows = Math.max(2, Math.min(20, Number(rows) || 2))
  const header = `| ${Array.from({ length: safeColumns }, (_, i) => `Column ${i + 1}`).join(' | ')} |`
  const divider = `| ${Array.from({ length: safeColumns }, () => '---').join(' | ')} |`
  const bodyRows = Array.from({ length: safeRows - 1 }, () =>
    `| ${Array.from({ length: safeColumns }, () => 'Cell').join(' | ')} |`,
  )
  insertMarkdownAtSelection(pane, `\n${[header, divider, ...bodyRows].join('\n')}\n`)
}

export function editorCmd(cmd) {
  if (runWysiwygCommand(() => {
    if (cmd === 'undo') richEditors[state.focusedPane].exec('undo')
    if (cmd === 'redo') richEditors[state.focusedPane].exec('redo')
  })) return
  const view = getFocusedEditor()
  if (!view) return
  if (cmd === 'undo') undo(view)
  if (cmd === 'redo') redo(view)
}

export function wrapInline(before, after) {
  if (runWysiwygCommand(() => {
    if (before === '**' && after === '**') richEditors[state.focusedPane].exec('bold')
    else if (before === '*' && after === '*') richEditors[state.focusedPane].exec('italic')
    else if (before === '~~' && after === '~~') richEditors[state.focusedPane].exec('strike')
    else if (before === '`' && after === '`') richEditors[state.focusedPane].exec('code')
  })) return
  const view = getFocusedEditor()
  if (!view) return
  const { state: s, dispatch } = view
  const sel = s.selection.main
  const selected = s.sliceDoc(sel.from, sel.to)
  dispatch(s.update({
    changes: { from: sel.from, to: sel.to, insert: `${before}${selected || 'text'}${after}` },
    selection: { anchor: sel.from + before.length, head: sel.from + before.length + (selected || 'text').length },
  }))
  view.focus()
}

export function wrapSelection(prefix) {
  if (runWysiwygCommand(() => {
    if (prefix === '> ') richEditors[state.focusedPane].exec('blockQuote')
  })) return
  const view = getFocusedEditor()
  if (!view) return
  const { state: s, dispatch } = view
  const line = s.doc.lineAt(s.selection.main.from)
  dispatch(s.update({ changes: { from: line.from, insert: prefix } }))
  view.focus()
}

export function insertHeading(level) {
  if (runWysiwygCommand(() => {
    if (level === 0) richEditors[state.focusedPane].exec('paragraph')
    else richEditors[state.focusedPane].exec('heading', { level })
  })) {
    document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
    return
  }
  const view = getFocusedEditor()
  if (!view) return
  const prefix = level === 0 ? '' : '#'.repeat(level) + ' '
  const { state: s, dispatch } = view
  const line = s.doc.lineAt(s.selection.main.from)
  const cleaned = line.text.replace(/^#{1,6}\s*/, '')
  dispatch(s.update({ changes: { from: line.from, to: line.to, insert: prefix + cleaned } }))
  view.focus()
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
}

export function insertList(type) {
  if (runWysiwygCommand(() => {
    if (type === 'ordered') richEditors[state.focusedPane].exec('orderedList')
    else if (type === 'bullet') richEditors[state.focusedPane].exec('bulletList')
    else if (type === 'task') richEditors[state.focusedPane].exec('taskList')
  })) {
    document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
    return
  }
  if (!getFocusedEditor()) return
  const prefixes = { bullet: '- ', ordered: '1. ', task: '- [ ] ' }
  wrapSelection(prefixes[type])
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
}

export function insertLink() {
  const pane = state.focusedPane
  const selected = paneUsesWysiwyg(pane) && richEditors[pane]
    ? richEditors[pane].getSelectedText()
    : (() => {
        const view = editorViews[pane]
        if (!view) return ''
        const sel = view.state.selection.main
        return view.state.sliceDoc(sel.from, sel.to)
      })()
  openCommandDialog('link', { text: selected })
}

export async function insertImage() {
  const picked = await window.fjord?.pickImageFile?.()
  if (picked?.path) {
    insertImageReference(picked.path, picked.name)
    return
  }
  openCommandDialog('image')
}

export function insertTable() {
  openCommandDialog('table', { columns: 3, rows: 2 })
}

export function insertCallout(type) {
  const pane = state.focusedPane
  const label = formatCalloutLabel(type)
  const snippet = `> [!${String(type || 'note').toUpperCase()}] ${label}\n> `

  if (paneUsesWysiwyg(pane) && richEditors[pane]) {
    _focusPane?.(pane)
    richEditors[pane].focus()
    const selectedText = richEditors[pane].getSelectedText().trim()
    const content = selectedText
      ? `> [!${String(type || 'note').toUpperCase()}] ${label}\n${selectedText.split('\n').map(line => `> ${line}`).join('\n')}\n`
      : snippet
    richEditors[pane].replaceSelection(content)
    document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
    return
  }

  insertMarkdownAtSelection(pane, snippet)
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
}

export function formatCalloutLabel(type) {
  return String(type || 'note')
    .replace(/[-_]+/g, ' ')
    .replace(/\b\w/g, letter => letter.toUpperCase()) || 'Note'
}

export function insertImageReference(filePath, fileName = '') {
  const pane = state.focusedPane
  const tab = getTabForPane(pane)
  const alt = stripFileExtension(fileName || lastPathSegment(filePath) || 'Image')
  const imagePath = tab?.path ? toRelativePath(tab.path, filePath) : normalizePathSeparators(filePath)

  if (paneUsesWysiwyg(pane) && richEditors[pane]) {
    _focusPane?.(pane)
    richEditors[pane].focus()
    richEditors[pane].exec('addImage', { imageUrl: imagePath, altText: alt || 'Image' })
    return
  }

  insertMarkdownAtSelection(pane, `![${alt || 'Image'}](${imagePath})`, imagePath.length + 1)
}

// ── Sync to WYSIWYG ─────────────────────────────────────────────
export function syncToWysiwyg(pane = state.focusedPane) {
  const editor = richEditors[pane] || _ensureRichEditorMounted?.(pane)
  const tab = getTabForPane(pane)
  if (!editor || !tab) return

  // Toast UI normalizes markdown (e.g., trailing newlines, whitespace).
  // Trim both sides for comparison to avoid unnecessary setMarkdown calls
  // that reset cursor position and selection state.
  const editorContent = (editor.getMarkdown() || '').trim()
  const tabContent = (tab.content || '').trim()
  if (editorContent === tabContent) return

  syncingRichEditor[pane] = true
  try {
    editor.setMarkdown(tab.content || '')
  } catch (err) {
    console.error(`[fjordmark] Failed to sync content to WYSIWYG for pane "${pane}":`, err)
  }
  syncingRichEditor[pane] = false
}
