import ToastEditor from '@toast-ui/editor'
import { createEditor, updateEditorDoc } from './editor.js'
import { state, $, el, PANE_KEYS, editorViews, richEditors, richEditorMountTarget, saveTimers, syncingRichEditor, getPaneView, getSplitView, paneUsesWysiwyg, paneUsesMarkdown, getWysiwygMountSlot, getSplitEditableView, getTabForPane, cleanSplitSnapshot, storeSplitSnapshot, getFocusedTab } from './state.js'
import { chevronIcon } from './icons.js'
import { getTheme } from './theme.js'
import { updateSetting, getSettings } from './settings.js'
import { refreshPreview, updateActiveMetrics, onEditorSelectionChange, exportToPdf, handleImagePaste } from './preview.js'
import { htmlToMarkdown } from './markdown.js'
import { toggleCommandPalette } from './command-palette.js'
import { toggleFindReplace, updateFind, handleFindKeydown, findNext, findPrev, replaceOne, replaceAll } from './find-replace.js'
import { editorCmd, wrapInline, wrapSelection, insertHeading, insertList, insertLink, insertImage, insertTable, insertCallout, insertCodeBlock, insertHorizontalRule, syncToWysiwyg } from './commands.js'
import { openDiagramBuilder } from './diagram-builder.js'
import { buildInspector, setInspectorTab, handleInspectorClick as handleInspectorClickInner } from './inspector.js'
import { buildRightPanelContainer, toggleRightPanel, closeRightPanel, refreshRightPanel } from './right-panel.js'
import { buildSearchPanel, toggleSearchPanel, handleSearchInput, openSearchPanel, closeSearchPanel } from './search-panel.js'
import { renderAttachmentPreview, clearAttachmentPreview } from './attachment-preview.js'
import { buildTerminalDrawer, toggleTerminalDrawer, handleTerminalInput, openTerminalDrawer, closeTerminalDrawer } from './terminal-drawer.js'
import { buildGraphModal, openGraphModal, closeGraphModal } from './graph-modal.js'

// ── Callback registration ────────────────────────────────────────
let _callbacks = {}
export function registerWorkspaceCallbacks(cbs) { Object.assign(_callbacks, cbs) }

// ── Toolbar UI sync ──────────────────────────────────────────────
export function syncWorkspaceSplitToggle() {
  document.querySelectorAll('.workspace-toolbar [data-action="toggle-workspace-split"]').forEach(node => {
    node.classList.toggle('active', state.workspaceMode === 'dual')
  })
  const globalToggle = $('workspace-split-toggle')
  if (globalToggle) {
    globalToggle.classList.toggle('active', state.workspaceMode === 'dual')
    globalToggle.title = state.workspaceMode === 'dual' ? 'Workspace: dual' : 'Workspace: single'
    globalToggle.setAttribute('aria-pressed', state.workspaceMode === 'dual' ? 'true' : 'false')
  }
}

export function syncPaneSplitToggle() {
  const node = $('pane-split-toggle')
  if (!node) return
  const split = getPaneView(state.focusedPane) === 'split'
  node.classList.toggle('active', split)
  node.title = split ? 'Pane split: on' : 'Pane split: off'
  node.setAttribute('aria-pressed', split ? 'true' : 'false')
}

export function syncSplitToggles() {
  syncWorkspaceSplitToggle()
  syncPaneSplitToggle()
}

export function syncToolbarToggle() {
  const node = $('toolbar-toggle')
  if (!node) return
  node.classList.toggle('active', state.toolbarVisible)
  node.title = state.toolbarVisible ? 'Hide toolbars' : 'Show toolbars'
  node.setAttribute('aria-pressed', state.toolbarVisible ? 'true' : 'false')
}

// ── Split slot selector HTML ─────────────────────────────────────
export function renderSplitSlotSelector(pane, slot) {
  const splitView = getSplitView(pane)
  const currentView = splitView[slot] || 'preview'
  const currentLabel = currentView === 'markdown' ? 'MD' : currentView === 'wysiwyg' ? 'Rich Text' : 'Preview'
  return `
    <div class="split-slot-control" data-pane="${pane}">
      <div class="split-slot-control__label">${slot === 'left' ? 'left' : 'right'}</div>
      <div class="dd split-slot-dd" id="dd-sv-${pane}-${slot}">
        <div class="split-slot-trigger" data-action="toggle-dropdown" data-dropdown="dd-sv-${pane}-${slot}">
          <span>${currentLabel}</span>
          <svg class="arr" viewBox="0 0 8 6"><path d="M1 1.5l3 3 3-3"/></svg>
        </div>
        <div class="dd-menu dd-menu--compact" id="ddm-sv-${pane}-${slot}">
          <div class="dd-item${currentView === 'markdown' ? ' active' : ''}" data-action="set-split-view" data-slot="${slot}" data-slot-view="markdown">Markdown</div>
          <div class="dd-item${currentView === 'wysiwyg' ? ' active' : ''}" data-action="set-split-view" data-slot="${slot}" data-slot-view="wysiwyg">Rich Text</div>
          <div class="dd-item${currentView === 'preview' ? ' active' : ''}" data-action="set-split-view" data-slot="${slot}" data-slot-view="preview">Preview</div>
        </div>
      </div>
    </div>
  `
}

// ── Editor toolbar HTML ──────────────────────────────────────────
export function renderEditorToolbar(pane) {
  const paneView = getPaneView(pane)
  return `
    <div class="workspace-toolbar${state.toolbarVisible ? '' : ' hidden'}" data-pane="${pane}">
      <div class="ic" title="Undo" data-action="editor-cmd" data-cmd="undo">
        <svg viewBox="0 0 16 16"><path d="M3 7h6a4 4 0 1 1 0 8H5"/><path d="M3 4L1 7l2 3"/></svg>
      </div>
      <div class="ic" title="Redo" data-action="editor-cmd" data-cmd="redo">
        <svg viewBox="0 0 16 16"><path d="M13 7H7a4 4 0 1 0 0 8h4"/><path d="M13 4l2 3-2 3"/></svg>
      </div>
      <div class="tb-sep"></div>

      <div class="dd" id="dd-h-${pane}">
        <div class="ic hd" data-action="toggle-dropdown" data-dropdown="dd-h-${pane}">
          <span class="t">H</span>
          <svg class="arr" viewBox="0 0 8 6"><path d="M1 1.5l3 3 3-3"/></svg>
        </div>
        <div class="dd-menu" id="ddm-h-${pane}">
          <div class="dd-item" data-action="insert-heading" data-level="1">Heading 1</div>
          <div class="dd-item" data-action="insert-heading" data-level="2">Heading 2</div>
          <div class="dd-item" data-action="insert-heading" data-level="3">Heading 3</div>
          <div class="dd-item divider" data-action="insert-heading" data-level="0">Paragraph</div>
        </div>
      </div>

      <div class="dd" id="dd-l-${pane}">
        <div class="ic" data-action="toggle-dropdown" data-dropdown="dd-l-${pane}">
          <svg viewBox="0 0 16 16"><circle cx="3" cy="5" r="1.1" fill="currentColor" stroke="none"/><circle cx="3" cy="8.5" r="1.1" fill="currentColor" stroke="none"/><circle cx="3" cy="12" r="1.1" fill="currentColor" stroke="none"/><line x1="6.5" y1="5" x2="14" y2="5"/><line x1="6.5" y1="8.5" x2="14" y2="8.5"/><line x1="6.5" y1="12" x2="11" y2="12"/></svg>
        </div>
        <div class="dd-menu" id="ddm-l-${pane}">
          <div class="dd-item" data-action="insert-list" data-list-type="bullet">Bullet list</div>
          <div class="dd-item" data-action="insert-list" data-list-type="ordered">Numbered list</div>
          <div class="dd-item" data-action="insert-list" data-list-type="task">Task list</div>
        </div>
      </div>

      <div class="ic" title="Blockquote" data-action="wrap-selection" data-prefix="> ">
        <svg viewBox="0 0 16 16"><path d="M3 5h10M3 8h7M3 11h5"/></svg>
      </div>
      <div class="dd" id="dd-c-${pane}">
        <div class="ic" title="Callout" data-action="toggle-dropdown" data-dropdown="dd-c-${pane}">
          <svg viewBox="0 0 16 16"><path d="M3 3.5h10v7H7l-3.5 2.5V3.5Z"/><line x1="5.5" y1="6" x2="10.5" y2="6"/><line x1="5.5" y1="8.5" x2="9" y2="8.5"/></svg>
        </div>
        <div class="dd-menu" id="ddm-c-${pane}">
          <div class="dd-item" data-action="insert-callout" data-callout-type="note">Note</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="info">Info</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="tip">Tip</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="warning">Warning</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="danger">Danger</div>
        </div>
      </div>
      <div class="tb-sep"></div>

      <div class="ic" title="Bold" data-action="wrap-inline" data-before="**" data-after="**"><span class="t">B</span></div>
      <div class="ic" title="Italic" data-action="wrap-inline" data-before="*" data-after="*"><span class="t t-i">I</span></div>
      <div class="ic" title="Strikethrough" data-action="wrap-inline" data-before="~~" data-after="~~">
        <svg viewBox="0 0 16 16"><line x1="3" y1="8" x2="13" y2="8"/><path d="M5.5 5.5c0-1.1 1-2 2.5-2s2.5.9 2.5 2M5.5 10.5c0 1.1 1 2 2.5 2s2.5-.9 2.5-2"/></svg>
      </div>
      <div class="ic" title="Inline code" data-action="wrap-inline" data-before="\`" data-after="\`">
        <svg viewBox="0 0 16 16"><path d="M5.5 5L2 8l3.5 3M10.5 5L14 8l-3.5 3"/></svg>
      </div>
      <div class="ic" title="Code block" data-action="insert-code-block">
        <svg viewBox="0 0 16 16"><rect x="2.5" y="2.5" width="11" height="11" rx="1.5" fill="none"/><path d="M5.5 6L4 8l1.5 2M10.5 6L12 8l-2 2"/></svg>
      </div>
      <div class="ic" title="Link" data-action="insert-link">
        <svg viewBox="0 0 16 16"><path d="M6.5 9.5a3.5 3.5 0 0 0 5 0l2-2a3.5 3.5 0 0 0-5-5l-1 1"/><path d="M9.5 6.5a3.5 3.5 0 0 0-5 0l-2 2a3.5 3.5 0 0 0 5 5l1-1"/></svg>
      </div>
      <div class="ic" title="Horizontal rule" data-action="insert-horizontal-rule">
        <svg viewBox="0 0 16 16"><line x1="2" y1="8" x2="14" y2="8" stroke-width="2"/></svg>
      </div>
      <div class="tb-sep"></div>

      <div class="ic" title="Table" data-action="insert-table">
        <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1"/><line x1="2" y1="7" x2="14" y2="7"/><line x1="7" y1="3" x2="7" y2="13"/></svg>
      </div>
      <div class="ic" title="Image" data-action="insert-image">
        <svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="10" rx="1.5"/><path d="M2 10l3.5-3.5 2.5 2.5 2-2 4 4"/><circle cx="11.5" cy="5.5" r="1" fill="currentColor" stroke="none"/></svg>
      </div>
      <div class="ic" title="Diagram" data-action="insert-diagram">
        <svg viewBox="0 0 16 16"><rect x="1.5" y="2" width="5" height="3.5" rx="0.8"/><rect x="9.5" y="5" width="5" height="3.5" rx="0.8"/><rect x="5" y="10.5" width="5" height="3.5" rx="0.8"/><path d="M4 5.5V8.5L7.5 10.5M12 8.5V9.5L9.5 10.5" fill="none"/></svg>
      </div>
      <div class="ic" title="Find & Replace" data-action="toggle-find-replace">
        <svg viewBox="0 0 16 16"><circle cx="6" cy="6" r="3.5" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M9.5 9.5l3 3" stroke="currentColor" stroke-width="1.5" fill="none" stroke-linecap="round"/></svg>
      </div>

      <div class="workspace-toolbar__right">
        <div class="vseg">
          <div class="vb${paneView === 'markdown' ? ' active' : ''}" data-action="set-view" data-view="markdown">MD</div>
          <div class="vb${paneView === 'split' ? ' active' : ''}" data-action="set-view" data-view="split">Split</div>
          <div class="vb${paneView === 'wysiwyg' ? ' active' : ''}" data-action="set-view" data-view="wysiwyg">Rich Text</div>
          <div class="vb${paneView === 'preview' ? ' active' : ''}" data-action="set-view" data-view="preview">Preview</div>
        </div>
      </div>
    </div>
    <div class="workspace-pane-row${state.toolbarVisible ? '' : ' hidden'}" data-pane="${pane}">
      <div class="pane-label" id="pl-source-${pane}">${paneView === 'split' ? renderSplitSlotSelector(pane, 'left') : 'source'}</div>
      <div class="pane-label" id="pl-preview-${pane}">${paneView === 'split' ? renderSplitSlotSelector(pane, 'right') : 'preview'}</div>
    </div>
  `
}

// ── Editor UI ────────────────────────────────────────────────────
export function buildEditorUI() {
  destroyEditors()

  $('editor-wrapper').innerHTML = `
    <!-- Find & Replace -->
    <div class="find-replace-panel hidden" id="find-replace">
      <input type="text" class="find-replace-input" id="find-input" placeholder="Find…">
      <input type="text" class="find-replace-input" id="replace-input" placeholder="Replace with…">
      <div class="find-replace-buttons">
        <button class="find-btn" id="find-next-btn">Next</button>
        <button class="find-btn" id="find-prev-btn">Prev</button>
        <button class="replace-btn" id="replace-one-btn">Replace</button>
        <button class="replace-all-btn" id="replace-all-btn">Replace All</button>
      </div>
      <div class="find-count" id="find-count"></div>
      <button class="find-btn" id="find-close-btn" style="margin-left:auto">Close</button>
    </div>
    <!-- Search Panel -->
    ${buildSearchPanel()}
    <!-- Panes -->
    <div class="panes" id="panes">
      <div class="panes-workspace" id="panes-workspace">
        <section class="workspace-pane workspace-pane--primary" id="workspace-primary" data-pane="primary">
          <div class="workspace-pane__header">
            <div class="workspace-pane__label">Editor A</div>
            <div class="workspace-tabs" id="tabs-primary"></div>
          </div>
          ${renderEditorToolbar('primary')}
          <div class="panes-main" id="panes-main-primary">
            <div class="single-surface" id="single-surface-primary">
              <div class="preview-pane preview-pane--single" id="preview-single-primary"></div>
              <div class="cm-host" id="cm-host-primary"></div>
              <div class="minimap" id="minimap-primary" style="display:none"></div>
            </div>
            <div class="split-layout" id="split-layout-primary">
              <div class="pane" id="pane-left-primary">
                <div class="view-slot" id="view-slot-left-primary">
                  <div class="preview-pane preview-pane--slot" id="preview-left-primary"></div>
                </div>
              </div>
              <div class="split-resizer" id="split-resizer-primary" data-pane="primary" title="Resize split view"></div>
              <div class="pane" id="pane-right-primary">
                <div class="view-slot" id="view-slot-right-primary">
                  <div class="preview-pane preview-pane--slot" id="preview-right-primary"></div>
                </div>
              </div>
            </div>
          </div>
        </section>
        <div class="workspace-resizer hidden" id="workspace-resizer" title="Resize document split"></div>
        <section class="workspace-pane workspace-pane--secondary hidden" id="workspace-secondary" data-pane="secondary">
          <div class="workspace-pane__header">
            <div class="workspace-pane__label">Editor B</div>
            <div class="workspace-tabs" id="tabs-secondary"></div>
          </div>
          ${renderEditorToolbar('secondary')}
          <div class="panes-main" id="panes-main-secondary">
            <div class="single-surface" id="single-surface-secondary">
              <div class="preview-pane preview-pane--single" id="preview-single-secondary"></div>
              <div class="cm-host" id="cm-host-secondary"></div>
              <div class="minimap" id="minimap-secondary" style="display:none"></div>
            </div>
            <div class="split-layout" id="split-layout-secondary">
              <div class="pane" id="pane-left-secondary">
                <div class="view-slot" id="view-slot-left-secondary">
                  <div class="preview-pane preview-pane--slot" id="preview-left-secondary"></div>
                </div>
              </div>
              <div class="split-resizer" id="split-resizer-secondary" data-pane="secondary" title="Resize split view"></div>
              <div class="pane" id="pane-right-secondary">
                <div class="view-slot" id="view-slot-right-secondary">
                  <div class="preview-pane preview-pane--slot" id="preview-right-secondary"></div>
                </div>
              </div>
            </div>
          </div>
          <div class="workspace-pane__empty" id="workspace-secondary-empty">
            <div class="workspace-pane__empty-title">Split workspace</div>
            <div class="workspace-pane__empty-copy">Open another markdown file to compare, reference, or edit beside the current note.</div>
            <div class="workspace-pane__empty-action" data-action="open-secondary-file" role="button" tabindex="0">Quick open</div>
          </div>
        </section>
      </div>
      ${buildRightPanelContainer()}
    </div>
    ${buildTerminalDrawer()}
    ${buildGraphModal()}
  `

  mountEditor('primary')
  if (state.workspaceMode === 'dual') mountEditor('secondary')

  // Wire up welcome open button (may still be in DOM briefly)
  document.querySelectorAll('#welcome-open-btn').forEach(b => b.addEventListener('click', () => _callbacks.openFolder?.()))
  wireEditorUiEvents()
  syncWorkspaceUi()
  syncSplitLayout()
  syncFocusedPaneUi()
  refreshAllPreviews()
  updateActiveMetrics()
}

export function destroyEditors() {
  PANE_KEYS.forEach(pane => {
    clearTimeout(saveTimers[pane])
    saveTimers[pane] = null
    if (editorViews[pane]) {
      editorViews[pane].destroy()
      editorViews[pane] = null
    }
    destroyRichEditor(pane)
  })
}

export function mountEditor(pane) {
  const host = $(`cm-host-${pane}`)
  if (!host || editorViews[pane]) return
  editorViews[pane] = createEditor({
    parent: host,
    doc: getTabForPane(pane)?.content || '',
    onChange: content => _callbacks.onEditorChange?.(pane, content),
    onSelectionChange: editorState => onEditorSelectionChange(pane, editorState),
    onPaste: file => handleImagePaste(file, editorViews[pane], pane, (p, c) => _callbacks.onEditorChange?.(p, c)),
    onRichPaste: (html, editorView) => {
      const md = htmlToMarkdown(html)
      const pos = editorView.state.selection.main.head
      editorView.dispatch({
        changes: { from: pos, to: pos, insert: md },
        selection: { anchor: pos + md.length },
      })
      _callbacks.onEditorChange?.(pane, editorView.state.doc.toString())
    },
    isDark: getTheme() === 'dark',
    typewriterEnabled: getSettings().typewriterScrolling,
    spellcheckEnabled: getSettings().spellcheck,
    vimEnabled: getSettings().vimMode,
  })

  // Wire minimap updates
  if (editorViews[pane]) {
    editorViews[pane].scrollDOM.addEventListener('scroll', () => updateMinimap(pane))
    updateMinimap(pane)
  }
}

export function destroyRichEditor(pane) {
  if (richEditors[pane]) {
    try {
      richEditors[pane].destroy()
    } catch (err) {
      console.error(`[fjordmark] Error destroying WYSIWYG editor for pane "${pane}":`, err)
    }
    richEditors[pane] = null
  }
  // Clean up any orphaned WYSIWYG host elements
  document.querySelectorAll(`#single-surface-${pane} .wysiwyg-editor, #view-slot-left-${pane} .wysiwyg-editor, #view-slot-right-${pane} .wysiwyg-editor`).forEach(node => {
    node.remove()
  })
  richEditorMountTarget[pane] = null
}

export function ensureRichEditorMounted(pane) {
  const slot = getWysiwygMountSlot(pane)
  if (!slot) {
    destroyRichEditor(pane)
    return null
  }

  const slotHost = slot === 'single' ? $(`single-surface-${pane}`) : $(`view-slot-${slot}-${pane}`)
  if (!slotHost) return null

  const mountTarget = `${pane}:${slot}`

  // If editor exists at the correct target, verify its DOM element is still attached
  if (richEditors[pane] && richEditorMountTarget[pane] === mountTarget) {
    const existingHost = document.getElementById(`wysiwyg-editor-${pane}`)
    if (existingHost && existingHost.isConnected) {
      return richEditors[pane]
    }
    // DOM element is stale/detached — fall through to re-mount
  }

  // Use tab content as canonical source of truth (always synced by change handlers)
  const markdown = getTabForPane(pane)?.content ?? ''
  destroyRichEditor(pane)

  const host = document.createElement('div')
  host.className = 'wysiwyg-editor'
  host.id = `wysiwyg-editor-${pane}`
  slotHost.appendChild(host)

  try {
    richEditors[pane] = new ToastEditor({
      el: host,
      initialValue: markdown,
      initialEditType: 'wysiwyg',
      hideModeSwitch: true,
      previewStyle: 'tab',
      usageStatistics: false,
      toolbarItems: [],
      autofocus: false,
      theme: getTheme() === 'dark' ? 'dark' : undefined,
      events: {
        focus: () => focusPane(pane),
        change: () => _callbacks.onRichEditorChange?.(pane),
      },
    })
    richEditors[pane].setHeight('100%')
    richEditorMountTarget[pane] = mountTarget
    return richEditors[pane]
  } catch (err) {
    console.error(`[fjordmark] Failed to mount WYSIWYG editor for pane "${pane}":`, err)
    host.remove()
    richEditors[pane] = null
    richEditorMountTarget[pane] = null
    return null
  }
}

export function ensureEditorForPane(pane) {
  if (!$(`workspace-primary`)) buildEditorUI()
  if (!editorViews[pane]) mountEditor(pane)
  if (paneUsesWysiwyg(pane)) ensureRichEditorMounted(pane)
}

export function refreshAllPreviews() {
  PANE_KEYS.forEach(pane => {
    const tab = getTabForPane(pane)
    refreshPreview(pane, tab?.content || '')
    maybeRefreshWysiwygPane(pane)
  })
}

export function maybeRefreshWysiwygPane(pane) {
  if (!paneUsesWysiwyg(pane)) return
  ensureRichEditorMounted(pane)
  syncToWysiwyg(pane)
}

// ── Minimap ──────────────────────────────────────────────────────
export function updateMinimap(pane) {
  const minimap = $(`minimap-${pane}`)
  const editor = editorViews[pane]
  if (!minimap || !editor || !getSettings().showMinimap) {
    if (minimap) minimap.style.display = 'none'
    return
  }
  minimap.style.display = 'block'

  const doc = editor.state.doc.toString()
  const lines = doc.split('\n')
  const linesHtml = lines.map(line => {
    const len = Math.min(line.length, 80)
    return `<div class="minimap-line" style="width:${len * 0.6}px"></div>`
  }).join('')

  const scrollTop = editor.scrollDOM.scrollTop
  const scrollHeight = editor.scrollDOM.scrollHeight
  const clientHeight = editor.scrollDOM.clientHeight
  const ratio = scrollTop / (scrollHeight || 1)
  const viewRatio = clientHeight / (scrollHeight || 1)

  minimap.innerHTML = linesHtml + `<div class="minimap-viewport" style="top:${ratio * 100}%;height:${viewRatio * 100}%"></div>`
}

// ── Workspace layout sync ────────────────────────────────────────
export function syncWorkspaceUi() {
  const dual = state.workspaceMode === 'dual'
  const app = $('app')
  const secondaryPane = $('workspace-secondary')
  const secondaryEmpty = $('workspace-secondary-empty')
  const workspaceResizer = $('workspace-resizer')
  const primaryPane = $('workspace-primary')

  if (app) app.dataset.workspaceMode = state.workspaceMode
  if (primaryPane) primaryPane.style.flexBasis = dual ? 'var(--document-split-ratio)' : '100%'
  if (secondaryPane) secondaryPane.classList.toggle('hidden', !dual)
  if (workspaceResizer) workspaceResizer.classList.toggle('hidden', !dual)
  syncSplitToggles()
  if (secondaryEmpty) secondaryEmpty.style.display = dual && !state.secondaryTab ? 'flex' : 'none'

  PANE_KEYS.forEach(pane => {
    const tab = getTabForPane(pane)
    const empty = $(`workspace-${pane}-empty`)
    const panesMain = $(`panes-main-${pane}`)
    const toolbar = document.querySelector(`.workspace-toolbar[data-pane="${pane}"]`)
    const paneRow = document.querySelector(`.workspace-pane-row[data-pane="${pane}"]`)
    if (pane === 'secondary' && !dual) return
    if (panesMain) panesMain.style.display = tab ? 'flex' : 'none'
    if (empty && pane === 'secondary') empty.style.display = tab ? 'none' : 'flex'
    if (toolbar) toolbar.style.display = tab && !tab.isAttachment ? 'flex' : 'none'
    if (paneRow) paneRow.style.display = tab && state.toolbarVisible && !tab.isAttachment ? 'flex' : 'none'

    // Handle attachment tabs: hide editor/preview, show only attachment preview
    if (tab?.isAttachment) {
      const singleSurface = $(`single-surface-${pane}`)
      const splitLayout = $(`split-layout-${pane}`)
      const cmHost = $(`cm-host-${pane}`)
      if (singleSurface) {
        singleSurface.style.display = 'flex'
        // Hide children except attachment preview
        Array.from(singleSurface.children).forEach(child => {
          if (!child.classList.contains('attachment-preview')) {
            child.style.display = 'none'
          }
        })
      }
      if (splitLayout) splitLayout.style.display = 'none'
      if (cmHost) cmHost.style.display = 'none'
    }
  })
}

export function syncFocusedPaneUi() {
  const app = $('app')
  if (app) app.dataset.focusedPane = state.focusedPane
  PANE_KEYS.forEach(pane => {
    const workspace = $(`workspace-${pane}`)
    workspace?.classList.toggle('focused', state.focusedPane === pane)
  })
  syncPaneSplitToggle()
}

export function syncSplitLayout() {
  const dual = state.workspaceMode === 'dual'
  PANE_KEYS.forEach(pane => {
    const active = pane === 'primary' || dual
    const tab = getTabForPane(pane)
    const paneView = getPaneView(pane)
    const splitView = getSplitView(pane)
    const singleSurface = $(`single-surface-${pane}`)
    const splitLayout = $(`split-layout-${pane}`)
    const leftPane = $(`pane-left-${pane}`)
    const rightPane = $(`pane-right-${pane}`)
    const splitResizer = $(`split-resizer-${pane}`)
    const sourceLabel = $(`pl-source-${pane}`)
    const previewLabel = $(`pl-preview-${pane}`)
    if (!active || !tab) return
    if (singleSurface) singleSurface.style.display = paneView === 'split' ? 'none' : 'flex'
    if (splitLayout) splitLayout.style.display = paneView === 'split' ? 'flex' : 'none'
    if (paneView === 'split') {
      if (leftPane) leftPane.classList.toggle('hidden', false)
      if (rightPane) rightPane.classList.toggle('hidden', false)
      if (leftPane) leftPane.style.display = 'flex'
      if (rightPane) rightPane.style.display = 'flex'
      if (splitResizer) splitResizer.style.display = ''
      if (leftPane) leftPane.style.flex = '0 0 var(--split-ratio)'
      if (rightPane) rightPane.style.flex = '1 1 calc(100% - var(--split-ratio))'
      placePaneView(pane, 'left', splitView.left)
      placePaneView(pane, 'right', splitView.right)
    } else {
      if (splitResizer) splitResizer.style.display = 'none'
      placePaneView(pane, 'left', null)
      placePaneView(pane, 'right', null)
      placeStandaloneView(pane, paneView)
    }
    if (sourceLabel) {
      sourceLabel.style.display = ''
      sourceLabel.innerHTML = paneView === 'split' ? renderSplitSlotSelector(pane, 'left') : paneView
    }
    if (previewLabel) {
      previewLabel.style.display = paneView === 'split' ? '' : 'none'
      previewLabel.innerHTML = paneView === 'split' ? renderSplitSlotSelector(pane, 'right') : 'preview'
    }
    if (paneUsesWysiwyg(pane)) maybeRefreshWysiwygPane(pane)
    else destroyRichEditor(pane)
    if (paneUsesMarkdown(pane)) queueMicrotask(() => editorViews[pane]?.requestMeasure?.())
  })
}

export function placeStandaloneView(pane, viewType) {
  const surface = $(`single-surface-${pane}`)
  if (!surface) return

  const preview = $(`preview-single-${pane}`)
  const cmHost = $(`cm-host-${pane}`)

  if (preview) preview.style.display = viewType === 'preview' ? 'block' : 'none'

  if (viewType === 'markdown' && cmHost) {
    surface.appendChild(cmHost)
    cmHost.style.display = 'block'
    queueMicrotask(() => editorViews[pane]?.requestMeasure?.())
  } else if (cmHost) {
    surface.appendChild(cmHost)
    cmHost.style.display = 'none'
  }
}

export function placePaneView(pane, slot, viewType) {
  const slotHost = $(`view-slot-${slot}-${pane}`)
  if (!slotHost) return

  const preview = $(`preview-${slot}-${pane}`)
  const cmHost = $(`cm-host-${pane}`)

  if (preview) preview.style.display = viewType === 'preview' ? 'block' : 'none'

  if (viewType === 'markdown' && cmHost) {
    slotHost.appendChild(cmHost)
    cmHost.style.display = 'block'
    queueMicrotask(() => editorViews[pane]?.requestMeasure?.())
  }
}

// ── Event wiring ─────────────────────────────────────────────────
export function wireEditorUiEvents() {
  $('find-input')?.addEventListener('keyup', updateFind)
  $('find-input')?.addEventListener('keydown', handleFindKeydown)
  $('find-next-btn')?.addEventListener('click', findNext)
  $('find-prev-btn')?.addEventListener('click', findPrev)
  $('replace-one-btn')?.addEventListener('click', () => replaceOne((p, c) => _callbacks.onEditorChange?.(p, c)))
  $('replace-all-btn')?.addEventListener('click', () => replaceAll((p, c) => _callbacks.onEditorChange?.(p, c)))
  $('find-close-btn')?.addEventListener('click', toggleFindReplace)
  $('split-resizer-primary')?.addEventListener('pointerdown', startSplitResize)
  $('split-resizer-secondary')?.addEventListener('pointerdown', startSplitResize)
  $('workspace-resizer')?.addEventListener('pointerdown', startWorkspaceSplitResize)
  document.querySelectorAll('.workspace-toolbar').forEach(node => {
    node.addEventListener('click', handleToolbarClick)
  })
  document.querySelectorAll('.workspace-pane-row').forEach(node => {
    node.addEventListener('click', handleToolbarClick)
  })
  document.querySelectorAll('.workspace-pane__empty').forEach(node => {
    node.addEventListener('click', handleToolbarClick)
    node.addEventListener('keydown', event => {
      if (event.key !== 'Enter' && event.key !== ' ') return
      const control = event.target.closest('[data-action]')
      if (!control) return
      event.preventDefault()
      control.click()
    })
  })
  document.querySelectorAll('.workspace-pane').forEach(node => {
    node.addEventListener('pointerdown', () => focusPane(node.dataset.pane))
  })
  document.querySelectorAll('.workspace-tabs').forEach(node => {
    node.addEventListener('dragover', e => _callbacks.handleTabDragOver?.(e))
    node.addEventListener('dragleave', e => _callbacks.handleTabDragLeave?.(e))
    node.addEventListener('drop', e => _callbacks.handleTabDrop?.(e))
  })
  handleSearchInput(path => _callbacks.openFile?.({ path, name: path.split('/').pop() }))
  handleTerminalInput()
}

export function handleToolbarClick(event) {
  const control = event.target.closest('[data-action]')
  if (!control) return
  const pane = control.closest('[data-pane]')?.dataset.pane
  if (pane) focusPane(pane)

  const { action } = control.dataset

  if (action === 'editor-cmd') editorCmd(control.dataset.cmd)
  if (action === 'toggle-dropdown') toggleDd(control.dataset.dropdown)
  if (action === 'insert-heading') insertHeading(Number(control.dataset.level))
  if (action === 'insert-list') insertList(control.dataset.listType)
  if (action === 'wrap-selection') wrapSelection(control.dataset.prefix || '')
  if (action === 'wrap-inline') wrapInline(control.dataset.before || '', control.dataset.after || '')
  if (action === 'insert-link') insertLink()
  if (action === 'insert-image') insertImage()
  if (action === 'insert-table') insertTable()
  if (action === 'insert-callout') insertCallout(control.dataset.calloutType || 'note')
  if (action === 'insert-code-block') insertCodeBlock()
  if (action === 'insert-horizontal-rule') insertHorizontalRule()
  if (action === 'insert-diagram') openDiagramBuilder()
  if (action === 'toggle-find-replace') toggleFindReplace()
  if (action === 'export-pdf') exportToPdf()
  if (action === 'set-view') setPaneView(pane || state.focusedPane, control.dataset.view)
  if (action === 'set-split-view') setSplitPaneView(pane || state.focusedPane, control.dataset.slot, control.dataset.slotView)
  if (action === 'toggle-toolbar') toggleToolbar()
  if (action === 'toggle-workspace-split') toggleWorkspaceSplit()
  if (action === 'open-secondary-file') { focusPane('secondary'); toggleCommandPalette() }
  if (action === 'toggle-inspector') toggleInspector()
}

// ── Resize handlers ──────────────────────────────────────────────
export function startSplitResize(event) {
  const pane = event.currentTarget?.dataset?.pane || 'primary'
  if (getPaneView(pane) !== 'split') return
  event.preventDefault()
  const resizer = event.currentTarget
  const panesMain = $(`panes-main-${pane}`)
  if (!panesMain) return

  document.body.classList.add('is-resizing-split')
  resizer?.setPointerCapture?.(event.pointerId)

  const onMove = moveEvent => {
    const rect = panesMain.getBoundingClientRect()
    const ratio = ((moveEvent.clientX - rect.left) / rect.width) * 100
    updateSetting('splitRatio', Math.min(80, Math.max(20, ratio)))
  }

  const onUp = () => {
    document.body.classList.remove('is-resizing-split')
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

export function startWorkspaceSplitResize(event) {
  if (state.workspaceMode !== 'dual') return
  event.preventDefault()
  const resizer = $('workspace-resizer')
  const workspace = $('panes-workspace')
  if (!workspace) return

  document.body.classList.add('is-resizing-split')
  resizer?.setPointerCapture?.(event.pointerId)

  const onMove = moveEvent => {
    const rect = workspace.getBoundingClientRect()
    const ratio = ((moveEvent.clientX - rect.left) / rect.width) * 100
    updateSetting('documentSplitRatio', Math.min(75, Math.max(25, ratio)))
  }

  const onUp = () => {
    document.body.classList.remove('is-resizing-split')
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', onUp)
    window.removeEventListener('pointercancel', onUp)
  }

  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', onUp)
  window.addEventListener('pointercancel', onUp)
}

// ── Focus pane ───────────────────────────────────────────────────
export function focusPane(pane) {
  if (!pane) return
  if (pane === 'secondary' && state.workspaceMode !== 'dual') return
  state.focusedPane = pane
  syncFocusedPaneUi()
  updateActiveMetrics()
}

// ── View mode functions ──────────────────────────────────────────
export function setPaneView(pane, view) {
  if (!pane) return
  const previousView = getPaneView(pane)
  if (view === 'split') {
    state.splitEditableMode[pane] = previousView === 'wysiwyg'
      ? 'wysiwyg'
      : previousView === 'markdown'
        ? 'markdown'
        : getSplitEditableView(pane)
  }
  if (view === 'markdown' || view === 'wysiwyg') {
    state.splitEditableMode[pane] = view
  }
  state.paneView[pane] = view
  const toolbar = document.querySelector(`.workspace-toolbar[data-pane="${pane}"]`)
  ;['markdown', 'split', 'wysiwyg', 'preview'].forEach(viewName => {
    toolbar?.querySelectorAll(`.vb[data-view="${viewName}"]`).forEach(node => {
      node.classList.toggle('active', viewName === view)
    })
  })
  // syncSplitLayout handles mounting + syncing WYSIWYG via maybeRefreshWysiwygPane.
  // Calling syncToWysiwyg before layout is ready can cause mount into hidden containers.
  syncSplitLayout()
  syncPaneSplitToggle()
  if (view === 'wysiwyg') {
    // Focus the WYSIWYG editor after layout is settled
    queueMicrotask(() => richEditors[pane]?.focus())
  }
}

export function setSplitPaneView(pane, slot, view) {
  if (!pane || !slot) return
  if (view === 'preview') {
    state.splitPreviewSide[pane] = slot
  } else {
    state.splitEditableMode[pane] = view === 'wysiwyg' ? 'wysiwyg' : 'markdown'
    state.splitPreviewSide[pane] = slot === 'left' ? 'right' : 'left'
  }
  syncSplitLayout()
  if (paneUsesWysiwyg(pane)) maybeRefreshWysiwygPane(pane)
}

// ── Toggle functions ─────────────────────────────────────────────
export function toggleToolbar() {
  state.toolbarVisible = !state.toolbarVisible
  document.querySelectorAll('.workspace-toolbar').forEach(node => {
    node.classList.toggle('hidden', !state.toolbarVisible)
  })
  document.querySelectorAll('.workspace-pane-row').forEach(node => {
    node.classList.toggle('hidden', !state.toolbarVisible)
  })
  syncToolbarToggle()
}

export function togglePaneSplitView() {
  const pane = state.focusedPane || 'primary'
  const currentView = getPaneView(pane)
  setPaneView(pane, currentView === 'split' ? getSplitEditableView(pane) : 'split')
}

export function toggleSidebar() {
  state.sidebarVisible = !state.sidebarVisible
  const sb = $('sidebar')
  if (sb) sb.classList.toggle('collapsed', !state.sidebarVisible)
  const control = $('sidebar-toggle')
  if (control) control.setAttribute('aria-pressed', state.sidebarVisible ? 'true' : 'false')
}

export function toggleDd(id) {
  const menuId = id.replace(/^dd-/, 'ddm-')
  const menu = $(menuId)
  if (!menu) return
  const was = menu.classList.contains('open')
  document.querySelectorAll('.dd-menu').forEach(m => m.classList.remove('open'))
  if (!was) menu.classList.add('open')
}

export function toggleInspector() {
  toggleRightPanel('inspector')
}

export { refreshRightPanel }

export function toggleWorkspaceSplit() {
  state.workspaceMode = state.workspaceMode === 'dual' ? 'single' : 'dual'

  if (state.workspaceMode === 'dual') {
    cleanSplitSnapshot()
    const snapshotPrimary = state.splitSnapshot.primary.filter(tab => state.tabs.includes(tab))
    const snapshotSecondary = state.splitSnapshot.secondary.filter(tab => state.tabs.includes(tab))

    if (snapshotPrimary.length || snapshotSecondary.length) {
      const assigned = new Set([...snapshotPrimary, ...snapshotSecondary])
      const unassigned = state.tabs.filter(tab => !assigned.has(tab))
      state.tabGroups.primary = [...snapshotPrimary, ...unassigned]
      state.tabGroups.secondary = [...snapshotSecondary]
      state.activeTab = state.tabGroups.primary.includes(state.splitSnapshot.activePrimary)
        ? state.splitSnapshot.activePrimary
        : state.tabGroups.primary[0] || null
      state.secondaryTab = state.tabGroups.secondary.includes(state.splitSnapshot.activeSecondary)
        ? state.splitSnapshot.activeSecondary
        : state.tabGroups.secondary[0] || null
      state.focusedPane = state.splitSnapshot.focusedPane === 'secondary' && state.secondaryTab ? 'secondary' : 'primary'
    } else {
      if (!state.activeTab && state.tabs[0]) state.activeTab = state.tabs[0]
      state.tabGroups.primary = state.activeTab ? [state.activeTab, ...state.tabs.filter(tab => tab !== state.activeTab)] : []
      state.tabGroups.secondary = []
      state.secondaryTab = null
      state.focusedPane = 'secondary'
    }
    if (state.secondaryTab) ensureEditorForPane('secondary')
  } else {
    storeSplitSnapshot()
    const mergedActiveTab = getFocusedTab() || state.activeTab || state.secondaryTab
    state.tabGroups.primary = [...new Set([...state.tabGroups.primary, ...state.tabGroups.secondary])]
    state.activeTab = mergedActiveTab && state.tabGroups.primary.includes(mergedActiveTab)
      ? mergedActiveTab
      : state.tabGroups.primary[0] || null
    state.secondaryTab = null
    state.tabGroups.secondary = []
    if (editorViews.secondary) {
      editorViews.secondary.destroy()
      editorViews.secondary = null
    }
    destroyRichEditor('secondary')
    clearTimeout(saveTimers.secondary)
    saveTimers.secondary = null
    state.focusedPane = 'primary'
  }

  syncWorkspaceUi()
  syncSplitLayout()
  syncFocusedPaneUi()
  _callbacks.renderTabs?.()
  refreshAllPreviews()
  updateActiveMetrics()
}
