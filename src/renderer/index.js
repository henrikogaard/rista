import { initTheme, toggleTheme, getTheme } from './theme.js'
import { createEditor, updateEditorDoc } from './editor.js'
import { renderMarkdown, extractHeadings, getStats } from './markdown.js'

// ── Init theme before any paint ──────────────────────────────────
initTheme()

// ── App state ────────────────────────────────────────────────────
const state = {
  folderPath: null,
  tree: [],
  tabs: [],          // [{ path, name, content, dirty }]
  activeTab: null,
  view: 'split',     // 'edit' | 'split' | 'preview'
  toolbarVisible: true,
  sidebarVisible: true,
  activePopover: null, // 'stats' | 'toc' | null
}

let editorView = null
let saveTimer = null

// ── DOM refs (built below) ────────────────────────────────────────
const $ = id => document.getElementById(id)
const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html) e.innerHTML = html; return e }

// ── Build app shell ───────────────────────────────────────────────
function buildShell() {
  document.getElementById('root').innerHTML = `
    <div class="app" id="app">

      <!-- Titlebar -->
      <div class="titlebar" id="titlebar">
        <div class="titlebar__spacer"></div>
        <span class="titlebar__name">fjordmark</span>
        <span class="titlebar__path" id="folder-path"></span>
        <div class="titlebar__right">
          <div class="theme-btn" id="theme-btn" title="Toggle theme">
            ${sunIcon()}
          </div>
          <div class="ic" id="sidebar-toggle" title="Toggle sidebar">
            <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1"/><line x1="6" y1="3" x2="6" y2="13"/></svg>
          </div>
        </div>
      </div>

      <!-- Layout -->
      <div class="layout">

        <!-- Sidebar -->
        <div class="sidebar" id="sidebar">
          <div class="sidebar__header">
            <span class="sidebar__label">Explorer</span>
          </div>
          <div class="sidebar__open-btn" id="open-folder-btn">
            <svg viewBox="0 0 16 16"><path d="M2 5h4l2-2h6a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/></svg>
            Open folder…
          </div>
          <div class="file-tree" id="file-tree"></div>
        </div>

        <!-- Editor area -->
        <div class="editor-area">

          <!-- Tab bar -->
          <div class="tab-bar" id="tab-bar">
            <div style="flex:1;display:flex;align-items:flex-end;overflow-x:auto" id="tabs"></div>
          </div>

          <!-- Welcome / editor wrapper -->
          <div id="editor-wrapper" style="flex:1;display:flex;flex-direction:column;overflow:hidden">
            ${buildWelcome()}
          </div>

        </div>
      </div>

      <!-- Statusbar -->
      <div class="statusbar">
        <div class="st"><div class="st-dot"></div><span id="st-mode">Markdown</span></div>
        <div class="st" id="st-words">—</div>
        <div class="st" id="st-lines">—</div>
        <div class="st" style="margin-left:auto;color:var(--text3)">fjordmark</div>
      </div>

    </div>
  `

  // Wire up controls
  $('theme-btn').addEventListener('click', () => {
    const t = toggleTheme()
    $('theme-btn').innerHTML = t === 'dark' ? sunIcon() : moonIcon()
  })
  $('theme-btn').innerHTML = getTheme() === 'dark' ? sunIcon() : moonIcon()

  $('sidebar-toggle').addEventListener('click', toggleSidebar)
  $('open-folder-btn').addEventListener('click', openFolder)

  // Close popovers on outside click
  document.addEventListener('click', e => {
    if (!e.target.closest('.popover') && !e.target.closest('[data-pop]')) closePopovers()
  })

  // Watch for file changes from main process
  if (window.fjord) {
    window.fjord.onFileChange(({ event, path: p }) => {
      const tab = state.tabs.find(t => t.path === p)
      if (tab && !tab.dirty) loadFileIntoTab(tab)
      refreshTree()
    })
  }
}

function buildWelcome() {
  return `
    <div class="welcome" id="welcome">
      <div class="welcome__logo">fjord<span>mark</span></div>
      <div class="welcome__sub">Open a folder to start writing</div>
      <div class="welcome__btn" id="welcome-open-btn">
        <svg viewBox="0 0 16 16"><path d="M2 5h4l2-2h6a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/></svg>
        Open folder…
      </div>
    </div>
  `
}

function buildEditorUI() {
  $('editor-wrapper').innerHTML = `
    <!-- Toolbar -->
    <div class="toolbar" id="toolbar">
      <div class="ic" title="Undo" onclick="editorCmd('undo')">
        <svg viewBox="0 0 16 16"><path d="M3 7h6a4 4 0 1 1 0 8H5"/><path d="M3 4L1 7l2 3"/></svg>
      </div>
      <div class="ic" title="Redo" onclick="editorCmd('redo')">
        <svg viewBox="0 0 16 16"><path d="M13 7H7a4 4 0 1 0 0 8h4"/><path d="M13 4l2 3-2 3"/></svg>
      </div>
      <div class="tb-sep"></div>

      <div class="dd" id="dd-h">
        <div class="ic hd" onclick="toggleDd('dd-h')">
          <span class="t">H</span>
          <svg class="arr" viewBox="0 0 8 6"><path d="M1 1.5l3 3 3-3"/></svg>
        </div>
        <div class="dd-menu" id="ddm-h">
          <div class="dd-item" onclick="insertHeading(1)">Heading 1</div>
          <div class="dd-item" onclick="insertHeading(2)">Heading 2</div>
          <div class="dd-item" onclick="insertHeading(3)">Heading 3</div>
          <div class="dd-item divider" onclick="insertHeading(0)">Paragraph</div>
        </div>
      </div>

      <div class="dd" id="dd-l">
        <div class="ic" onclick="toggleDd('dd-l')">
          <svg viewBox="0 0 16 16"><circle cx="3" cy="5" r="1.1" fill="currentColor" stroke="none"/><circle cx="3" cy="8.5" r="1.1" fill="currentColor" stroke="none"/><circle cx="3" cy="12" r="1.1" fill="currentColor" stroke="none"/><line x1="6.5" y1="5" x2="14" y2="5"/><line x1="6.5" y1="8.5" x2="14" y2="8.5"/><line x1="6.5" y1="12" x2="11" y2="12"/></svg>
        </div>
        <div class="dd-menu" id="ddm-l">
          <div class="dd-item" onclick="insertList('bullet')">Bullet list</div>
          <div class="dd-item" onclick="insertList('ordered')">Numbered list</div>
          <div class="dd-item" onclick="insertList('task')">Task list</div>
        </div>
      </div>

      <div class="ic" title="Blockquote" onclick="wrapSelection('> ')">
        <svg viewBox="0 0 16 16"><path d="M3 5h10M3 8h7M3 11h5"/></svg>
      </div>
      <div class="tb-sep"></div>

      <div class="ic" id="ic-bold"  title="Bold"          onclick="wrapInline('**','**')"><span class="t">B</span></div>
      <div class="ic" id="ic-italic" title="Italic"       onclick="wrapInline('*','*')"><span class="t t-i">I</span></div>
      <div class="ic" title="Strikethrough"               onclick="wrapInline('~~','~~')">
        <svg viewBox="0 0 16 16"><line x1="3" y1="8" x2="13" y2="8"/><path d="M5.5 5.5c0-1.1 1-2 2.5-2s2.5.9 2.5 2M5.5 10.5c0 1.1 1 2 2.5 2s2.5-.9 2.5-2"/></svg>
      </div>
      <div class="ic" title="Inline code"                 onclick="wrapInline('\`','\`')">
        <svg viewBox="0 0 16 16"><path d="M5.5 5L2 8l3.5 3M10.5 5L14 8l-3.5 3"/></svg>
      </div>
      <div class="ic" title="Link"                        onclick="insertLink()">
        <svg viewBox="0 0 16 16"><path d="M6.5 9.5a3.5 3.5 0 0 0 5 0l2-2a3.5 3.5 0 0 0-5-5l-1 1"/><path d="M9.5 6.5a3.5 3.5 0 0 0-5 0l-2 2a3.5 3.5 0 0 0 5 5l1-1"/></svg>
      </div>
      <div class="tb-sep"></div>

      <div class="ic" title="Table" onclick="insertTable()">
        <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1"/><line x1="2" y1="7" x2="14" y2="7"/><line x1="7" y1="3" x2="7" y2="13"/></svg>
      </div>
      <div class="ic img-lbl" title="Image" onclick="insertImage()">
        <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M2 10l3.5-3.5 2.5 2.5 2-2 4 4"/><circle cx="11.5" cy="5.5" r="1" fill="currentColor" stroke="none"/></svg>
        <span class="lbl">Add</span>
      </div>

      <!-- Right side -->
      <div style="margin-left:auto;display:flex;align-items:center;gap:4px;position:relative">
        <div class="vseg">
          <div class="vb" id="vb-edit"    onclick="setView('edit')">Edit</div>
          <div class="vb active" id="vb-split"  onclick="setView('split')">Split</div>
          <div class="vb" id="vb-preview" onclick="setView('preview')">Preview</div>
        </div>

        <div class="ic" data-pop="stats" title="Statistics" onclick="togglePopover('stats')">
          <svg viewBox="0 0 16 16"><rect x="2" y="9" width="3" height="5" rx="0.5" fill="currentColor" stroke="none"/><rect x="6.5" y="5" width="3" height="9" rx="0.5" fill="currentColor" stroke="none"/><rect x="11" y="2" width="3" height="12" rx="0.5" fill="currentColor" stroke="none"/></svg>
        </div>
        <div class="ic" data-pop="toc" title="Table of contents" onclick="togglePopover('toc')">
          <svg viewBox="0 0 16 16"><line x1="2" y1="4" x2="14" y2="4"/><line x1="5" y1="8" x2="14" y2="8"/><line x1="5" y1="12" x2="14" y2="12"/><circle cx="2.8" cy="8" r="0.9" fill="currentColor" stroke="none"/><circle cx="2.8" cy="12" r="0.9" fill="currentColor" stroke="none"/></svg>
        </div>

        <!-- Popovers -->
        <div class="popover" id="pop-stats" style="right:0;top:calc(100% + 8px)">
          <div class="pop-tabs">
            <div class="pop-tab active" onclick="switchPopTab('stats')">
              <svg viewBox="0 0 16 16"><rect x="2" y="9" width="3" height="5" rx="0.5" fill="currentColor" stroke="none"/><rect x="6.5" y="5" width="3" height="9" rx="0.5" fill="currentColor" stroke="none"/><rect x="11" y="2" width="3" height="12" rx="0.5" fill="currentColor" stroke="none"/></svg>
            </div>
            <div class="pop-tab" onclick="switchPopTab('toc')">
              <svg viewBox="0 0 16 16"><line x1="2" y1="4" x2="14" y2="4"/><line x1="5" y1="8" x2="14" y2="8"/><line x1="5" y1="12" x2="14" y2="12"/></svg>
            </div>
          </div>
          <div class="pop-title">Statistics</div>
          <div class="stats-grid" id="stats-content"></div>
        </div>

        <div class="popover" id="pop-toc" style="right:0;top:calc(100% + 8px)">
          <div class="pop-tabs">
            <div class="pop-tab" onclick="switchPopTab('stats')">
              <svg viewBox="0 0 16 16"><rect x="2" y="9" width="3" height="5" rx="0.5" fill="currentColor" stroke="none"/><rect x="6.5" y="5" width="3" height="9" rx="0.5" fill="currentColor" stroke="none"/><rect x="11" y="2" width="3" height="12" rx="0.5" fill="currentColor" stroke="none"/></svg>
            </div>
            <div class="pop-tab active" onclick="switchPopTab('toc')">
              <svg viewBox="0 0 16 16"><line x1="2" y1="4" x2="14" y2="4"/><line x1="5" y1="8" x2="14" y2="8"/><line x1="5" y1="12" x2="14" y2="12"/></svg>
            </div>
          </div>
          <div class="pop-title">Table of Contents</div>
          <div id="toc-content"></div>
        </div>
      </div>
    </div>

    <!-- Pane labels -->
    <div class="pane-row">
      <div class="pane-label" id="pl-source">source</div>
      <div class="pane-label" id="pl-preview">preview</div>
      <div class="hide-toolbar-btn" id="hide-toolbar-btn" onclick="toggleToolbar()">hide toolbar</div>
    </div>

    <!-- Panes -->
    <div class="panes" id="panes">
      <div class="pane" id="pane-edit">
        <div class="cm-host" id="cm-host"></div>
      </div>
      <div class="pane" id="pane-preview">
        <div class="preview-pane" id="preview"></div>
      </div>
    </div>
  `

  // Mount CodeMirror
  editorView = createEditor({
    parent: $('cm-host'),
    doc: '',
    onChange: onEditorChange,
  })

  // Wire up welcome open button (may still be in DOM briefly)
  document.querySelectorAll('#welcome-open-btn').forEach(b => b.addEventListener('click', openFolder))
}

// ── File tree ─────────────────────────────────────────────────────
function renderTree(items, container) {
  container.innerHTML = ''
  items.forEach(item => {
    if (item.type === 'folder') {
      const f = el('div', 'tree-folder open')
      f.innerHTML = `<svg viewBox="0 0 6 10"><path d="M1 1l4 4-4 4" stroke-width="1.5" stroke="currentColor" fill="none" stroke-linecap="round"/></svg>${item.name}`
      const children = el('div')
      children.style.display = 'block'
      if (item.children) renderTree(item.children, children)
      f.addEventListener('click', () => {
        f.classList.toggle('open')
        children.style.display = f.classList.contains('open') ? 'block' : 'none'
      })
      container.appendChild(f)
      container.appendChild(children)
    } else {
      const fi = el('div', 'tree-file')
      fi.innerHTML = `<div class="tree-file__dot"></div>${item.name}`
      fi.addEventListener('click', () => openFile(item))
      container.appendChild(fi)
    }
  })
}

async function refreshTree() {
  if (!state.folderPath || !window.fjord) return
  state.tree = await window.fjord.readFolder(state.folderPath)
  renderTree(state.tree, $('file-tree'))
  highlightActiveFile()
}

function highlightActiveFile() {
  document.querySelectorAll('.tree-file').forEach(f => f.classList.remove('active'))
  if (!state.activeTab) return
  document.querySelectorAll('.tree-file').forEach(f => {
    if (f.textContent.trim() === state.activeTab.name) f.classList.add('active')
  })
}

// ── Open folder ───────────────────────────────────────────────────
async function openFolder() {
  if (!window.fjord) return
  const p = await window.fjord.openFolder()
  if (!p) return
  state.folderPath = p
  $('folder-path').textContent = p.split('/').pop()
  await window.fjord.watchFolder(p)
  await refreshTree()

  if (!$('toolbar')) buildEditorUI()
}

// ── Open file ─────────────────────────────────────────────────────
async function openFile(item) {
  if (!window.fjord) return
  const existing = state.tabs.find(t => t.path === item.path)
  if (existing) { activateTab(existing); return }

  const content = await window.fjord.readFile(item.path)
  const tab = { path: item.path, name: item.name, content, dirty: false }
  state.tabs.push(tab)
  activateTab(tab)
}

async function loadFileIntoTab(tab) {
  if (!window.fjord) return
  tab.content = await window.fjord.readFile(tab.path)
  tab.dirty = false
  if (state.activeTab === tab) {
    updateEditorDoc(editorView, tab.content)
    refreshPreview(tab.content)
  }
  renderTabs()
}

// ── Tabs ──────────────────────────────────────────────────────────
function activateTab(tab) {
  state.activeTab = tab
  if (editorView) updateEditorDoc(editorView, tab.content)
  refreshPreview(tab.content)
  renderTabs()
  highlightActiveFile()
  updateStats(tab.content)
}

function renderTabs() {
  const container = $('tabs')
  if (!container) return
  container.innerHTML = ''
  state.tabs.forEach(tab => {
    const t = el('div', `tab${tab === state.activeTab ? ' active' : ''}`)
    t.innerHTML = `<div class="tab__dot"></div>${tab.name}${tab.dirty ? ' ·' : ''}<div class="tab__close">✕</div>`
    t.addEventListener('click', () => activateTab(tab))
    t.querySelector('.tab__close').addEventListener('click', e => { e.stopPropagation(); closeTab(tab) })
    container.appendChild(t)
  })
}

function closeTab(tab) {
  const idx = state.tabs.indexOf(tab)
  state.tabs.splice(idx, 1)
  if (state.activeTab === tab) {
    const next = state.tabs[idx] || state.tabs[idx - 1] || null
    state.activeTab = next
    if (next) activateTab(next)
    else if (editorView) updateEditorDoc(editorView, '')
  }
  renderTabs()
}

// ── Editor changes ────────────────────────────────────────────────
function onEditorChange(content) {
  if (!state.activeTab) return
  state.activeTab.content = content
  state.activeTab.dirty = true
  renderTabs()
  refreshPreview(content)
  updateStats(content)

  // Auto-save after 800ms idle
  clearTimeout(saveTimer)
  saveTimer = setTimeout(() => saveActive(), 800)
}

async function saveActive() {
  if (!state.activeTab || !window.fjord) return
  const ok = await window.fjord.writeFile(state.activeTab.path, state.activeTab.content)
  if (ok) { state.activeTab.dirty = false; renderTabs() }
}

// ── Preview ───────────────────────────────────────────────────────
async function refreshPreview(markdown) {
  const p = $('preview')
  if (!p) return
  p.innerHTML = await renderMarkdown(markdown)
}

// ── Stats ─────────────────────────────────────────────────────────
function updateStats(markdown) {
  const s = getStats(markdown)
  const el = $('st-words')
  if (el) el.textContent = `${s.words} words`
  const ll = $('st-lines')
  if (ll) ll.textContent = `${markdown.split('\n').length} lines`
  renderStatsPopover(s)
  renderTocPopover(extractHeadings(markdown))
}

function renderStatsPopover(s) {
  const c = $('stats-content')
  if (!c) return
  c.innerHTML = `
    <div class="stat-card"><div class="num">${s.words}</div><div class="row"><span class="lbl">Words</span></div></div>
    <div class="stat-card"><div class="num">${s.chars}</div><div class="row"><span class="lbl">Characters</span></div></div>
    <div class="stat-card"><div class="num">${s.paragraphs}</div><div class="row"><span class="lbl">Paragraphs</span></div></div>
    <div class="stat-card"><div class="num" style="font-size:15px">${s.readMin < 1 ? '< 1' : s.readMin} min</div><div class="row"><span class="lbl">Read time</span></div></div>
  `
}

function renderTocPopover(headings) {
  const c = $('toc-content')
  if (!c) return
  if (!headings.length) {
    c.innerHTML = `<div class="toc-empty"><div class="icon">🏔️</div><div class="msg">No headers yet</div></div>`
    return
  }
  c.innerHTML = headings.map(h =>
    `<div class="toc-item h${h.level}">${h.text}</div>`
  ).join('')
}

// ── View (edit / split / preview) ────────────────────────────────
function setView(v) {
  state.view = v
  ;['edit', 'split', 'preview'].forEach(x => {
    const b = $(`vb-${x}`)
    if (b) b.classList.toggle('active', x === v)
  })
  const pe = $('pane-edit')
  const pp = $('pane-preview')
  const pls = $('pl-source')
  const plp = $('pl-preview')
  if (!pe) return
  pe.classList.toggle('hidden', v === 'preview')
  pp.classList.toggle('hidden', v === 'edit')
  if (pls) pls.style.display = v === 'preview' ? 'none' : ''
  if (plp) plp.style.display = v === 'edit' ? 'none' : ''
}

// ── Toolbar toggle ────────────────────────────────────────────────
function toggleToolbar() {
  state.toolbarVisible = !state.toolbarVisible
  const tb = $('toolbar')
  const btn = $('hide-toolbar-btn')
  if (tb) tb.classList.toggle('hidden', !state.toolbarVisible)
  if (btn) btn.textContent = state.toolbarVisible ? 'hide toolbar' : 'show toolbar'
}

// ── Sidebar toggle ────────────────────────────────────────────────
function toggleSidebar() {
  state.sidebarVisible = !state.sidebarVisible
  const sb = $('sidebar')
  if (sb) sb.classList.toggle('collapsed', !state.sidebarVisible)
}

// ── Dropdown ─────────────────────────────────────────────────────
function toggleDd(id) {
  const map = { 'dd-h': 'ddm-h', 'dd-l': 'ddm-l' }
  const menuId = map[id]
  const menu = $(menuId)
  if (!menu) return
  const was = menu.classList.contains('open')
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
  if (!was) menu.classList.add('open')
}

// ── Popovers ──────────────────────────────────────────────────────
function togglePopover(name) {
  const id = `pop-${name}`
  const el = $(id)
  if (!el) return
  const was = el.classList.contains('open')
  closePopovers()
  if (!was) { el.classList.add('open'); state.activePopover = name }
}

function closePopovers() {
  document.querySelectorAll('.popover').forEach(p => p.classList.remove('open'))
  state.activePopover = null
}

function switchPopTab(to) {
  closePopovers()
  togglePopover(to)
}

// ── Editor commands ───────────────────────────────────────────────
function editorCmd(cmd) {
  if (!editorView) return
  if (cmd === 'undo') { import('@codemirror/commands').then(m => m.undo(editorView)) }
  if (cmd === 'redo') { import('@codemirror/commands').then(m => m.redo(editorView)) }
}

function wrapInline(before, after) {
  if (!editorView) return
  const { state: s, dispatch } = editorView
  const sel = s.selection.main
  const selected = s.sliceDoc(sel.from, sel.to)
  dispatch(s.update({
    changes: { from: sel.from, to: sel.to, insert: `${before}${selected || 'text'}${after}` },
    selection: { anchor: sel.from + before.length, head: sel.from + before.length + (selected || 'text').length },
  }))
  editorView.focus()
}

function wrapSelection(prefix) {
  if (!editorView) return
  const { state: s, dispatch } = editorView
  const line = s.doc.lineAt(s.selection.main.from)
  dispatch(s.update({ changes: { from: line.from, insert: prefix } }))
  editorView.focus()
}

function insertHeading(level) {
  if (!editorView) return
  const prefix = level === 0 ? '' : '#'.repeat(level) + ' '
  const { state: s, dispatch } = editorView
  const line = s.doc.lineAt(s.selection.main.from)
  const cleaned = line.text.replace(/^#{1,6}\s*/, '')
  dispatch(s.update({ changes: { from: line.from, to: line.to, insert: prefix + cleaned } }))
  editorView.focus()
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
}

function insertList(type) {
  if (!editorView) return
  const prefixes = { bullet: '- ', ordered: '1. ', task: '- [ ] ' }
  wrapSelection(prefixes[type])
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
}

function insertLink() { wrapInline('[', '](url)') }
function insertImage() { wrapInline('![alt](', ')') }

function insertTable() {
  if (!editorView) return
  const table = '\n| Column 1 | Column 2 | Column 3 |\n|----------|----------|----------|\n| Cell     | Cell     | Cell     |\n'
  const { state: s, dispatch } = editorView
  const sel = s.selection.main
  dispatch(s.update({ changes: { from: sel.from, insert: table } }))
  editorView.focus()
}

// ── Icons ─────────────────────────────────────────────────────────
function sunIcon() {
  return `<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="3"/><path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.2 3.2l1.4 1.4M11.4 11.4l1.4 1.4M3.2 12.8l1.4-1.4M11.4 4.6l1.4-1.4"/></svg>`
}
function moonIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M12.5 10A6 6 0 0 1 6 3.5c0-.5.1-1 .2-1.5A6 6 0 1 0 13.5 9.8c-.3.1-.7.2-1 .2z"/></svg>`
}

// Expose globals for inline onclick handlers
Object.assign(window, {
  editorCmd, wrapInline, wrapSelection, insertHeading, insertList,
  insertLink, insertImage, insertTable,
  setView, toggleToolbar, toggleDd, togglePopover, switchPopTab,
})

// ── Boot ─────────────────────────────────────────────────────────
buildShell()

// Keyboard shortcuts
document.addEventListener('keydown', e => {
  const mod = e.metaKey || e.ctrlKey
  if (mod && e.key === 's') { e.preventDefault(); saveActive() }
  if (mod && e.key === 'b') { e.preventDefault(); toggleSidebar() }
  if (mod && e.key === '\\') { e.preventDefault(); toggleToolbar() }
})
