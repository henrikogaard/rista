import ToastEditor from '@toast-ui/editor'
import { createEditor, updateSmartTypography, updateFocusMode, updateLivePreview, updatePosHighlight } from './editor.js'
import { state, $, el, PANE_KEYS, editorViews, richEditors, richEditorMountTarget, saveTimers, getPaneView, getSplitView, paneUsesWysiwyg, paneUsesMarkdown, getWysiwygMountSlot, getSplitEditableView, getTabForPane, cleanSplitSnapshot, storeSplitSnapshot, getFocusedTab, fileName, escapeHtml } from './state.js'
import { getTheme } from './theme.js'
import { updateSetting, getSettings } from './settings.js'
import { refreshPreview, updateActiveMetrics, exportToPdf, handleImagePaste, getWorkspaceAttachmentPaths } from './preview.js'
import { getRenderableMarkdown, htmlToMarkdown, renderDocumentBanner } from './markdown.js'
import { toggleCommandPalette } from './command-palette.js'
import { toggleFindReplace, updateFind, handleFindKeydown, findNext, findPrev, replaceOne, replaceAll } from './find-replace.js'
import { editorCmd, wrapInline, wrapSelection, insertHeading, insertList, insertLink, insertImage, insertTable, insertCallout, insertCodeBlock, insertHorizontalRule, syncToWysiwyg } from './commands.js'
import { openDiagramBuilder } from './diagram-builder.js'
import { toggleRightPanel, refreshRightPanel, restoreRightPanel } from './right-panel.js'
import { buildSearchPanel, handleSearchInput } from './search-panel.js'
import { buildGraphModal } from './graph-modal.js'
import { showAiContextMenu } from './ai-actions.js'
import { chevronIcon, editorSplitIcon } from './icons.js'

// ── Callback registration ────────────────────────────────────────
let _callbacks = {}
export function registerWorkspaceCallbacks(cbs) { Object.assign(_callbacks, cbs) }

// ── Toolbar UI sync ──────────────────────────────────────────────
export function syncWorkspaceSplitToggle() {
  const globalToggle = $('workspace-split-toggle')
  if (globalToggle) {
    globalToggle.classList.toggle('active', state.workspaceMode === 'dual')
    const label = state.workspaceMode === 'dual' ? 'Close side-by-side view' : 'Open files side by side'
    globalToggle.title = label
    globalToggle.setAttribute('aria-label', label)
    globalToggle.setAttribute('aria-pressed', state.workspaceMode === 'dual' ? 'true' : 'false')
  }
}

export function syncSplitToggles() {
  syncWorkspaceSplitToggle()
}

export function syncToolbarToggle() {
  document.querySelectorAll('[data-action="toggle-toolbar"]').forEach(node => {
    node.classList.toggle('active', state.toolbarVisible)
    node.title = state.toolbarVisible ? 'Hide editor controls (⌘\\)' : 'Show editor controls (⌘\\)'
    node.setAttribute('aria-pressed', state.toolbarVisible ? 'true' : 'false')
  })
}

// Formatting and view controls are one on-demand shelf. The quiet control in
// each pane header remains available when the shelf is collapsed.
function applyToolbarVisibility(pane) {
  const tab = getTabForPane(pane)
  const showModeBar = Boolean(tab && !tab.isAttachment && !tab.isSpatial && !tab.isBinary)
  const toolbar = document.querySelector(`.workspace-toolbar[data-pane="${pane}"]`)
  toolbar?.classList.toggle('hidden', !showModeBar)
  toolbar?.classList.toggle('collapsed', !state.toolbarVisible)
  const showPaneRow = showModeBar && getPaneView(pane) === 'split'
  document.querySelector(`.workspace-pane-row[data-pane="${pane}"]`)?.classList.toggle('hidden', !showPaneRow)
  return showModeBar
}

function syncWorkspaceBreadcrumb(pane) {
  const node = $(`workspace-breadcrumb-${pane}`)
  if (!node) return
  const tab = getTabForPane(pane)
  if (!tab?.path) {
    node.innerHTML = ''
    node.classList.add('hidden')
    return
  }
  const workspacePath = String(state.folderPath || '').replace(/\\/g, '/')
  const relativePath = String(tab.path).replace(/\\/g, '/').replace(`${workspacePath}/`, '')
  const parts = relativePath.split('/').filter(Boolean)
  node.innerHTML = parts.map((part, index) => `
    <span class="workspace-breadcrumb__part${index === parts.length - 1 ? ' current' : ''}">${escapeHtml(part.replace(/\.md$/i, ''))}</span>
  `).join('<span class="workspace-breadcrumb__separator" aria-hidden="true">/</span>')
  node.classList.remove('hidden')
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
    <div class="workspace-toolbar" data-pane="${pane}">
      <div class="dd" id="dd-h-${pane}">
        <div class="ic hd" data-action="toggle-dropdown" data-dropdown="dd-h-${pane}" title="Text style">
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

      <div class="ic" title="Bold" data-action="wrap-inline" data-before="**" data-after="**"><span class="t">B</span></div>
      <div class="ic" title="Italic" data-action="wrap-inline" data-before="*" data-after="*"><span class="t t-i">I</span></div>
      <div class="ic" title="Link" data-action="insert-link">
        <svg viewBox="0 0 16 16"><path d="M6.5 9.5a3.5 3.5 0 0 0 5 0l2-2a3.5 3.5 0 0 0-5-5l-1 1"/><path d="M9.5 6.5a3.5 3.5 0 0 0-5 0l-2 2a3.5 3.5 0 0 0 5 5l1-1"/></svg>
      </div>
      <div class="dd" id="dd-l-${pane}">
        <div class="ic" data-action="toggle-dropdown" data-dropdown="dd-l-${pane}" title="Lists">
          <svg viewBox="0 0 16 16"><circle cx="3" cy="5" r="1.1" fill="currentColor" stroke="none"/><circle cx="3" cy="8.5" r="1.1" fill="currentColor" stroke="none"/><circle cx="3" cy="12" r="1.1" fill="currentColor" stroke="none"/><line x1="6.5" y1="5" x2="14" y2="5"/><line x1="6.5" y1="8.5" x2="14" y2="8.5"/><line x1="6.5" y1="12" x2="11" y2="12"/></svg>
        </div>
        <div class="dd-menu" id="ddm-l-${pane}">
          <div class="dd-item" data-action="insert-list" data-list-type="bullet">Bullet list</div>
          <div class="dd-item" data-action="insert-list" data-list-type="ordered">Numbered list</div>
          <div class="dd-item" data-action="insert-list" data-list-type="task">Task list</div>
        </div>
      </div>

      <div class="dd editor-more" id="dd-more-${pane}">
        <div class="editor-more-trigger" data-action="toggle-dropdown" data-dropdown="dd-more-${pane}" role="button" tabindex="0">
          <span>More</span>
          <svg class="arr" viewBox="0 0 8 6"><path d="M1 1.5l3 3 3-3"/></svg>
        </div>
        <div class="dd-menu editor-more-menu" id="ddm-more-${pane}">
          <div class="dd-item" data-action="editor-cmd" data-cmd="undo">Undo</div>
          <div class="dd-item" data-action="editor-cmd" data-cmd="redo">Redo</div>
          <div class="dd-item divider" data-action="wrap-inline" data-before="~~" data-after="~~">Strikethrough</div>
          <div class="dd-item" data-action="wrap-inline" data-before="\`" data-after="\`">Inline code</div>
          <div class="dd-item" data-action="insert-code-block">Code block</div>
          <div class="dd-item" data-action="wrap-selection" data-prefix="> ">Blockquote</div>
          <div class="dd-item" data-action="insert-callout" data-callout-type="note">Callout</div>
          <div class="dd-item" data-action="insert-horizontal-rule">Horizontal rule</div>
          <div class="dd-item divider" data-action="insert-table">Table</div>
          <div class="dd-item" data-action="insert-image">Image</div>
          <div class="dd-item" data-action="insert-diagram">Diagram</div>
          <div class="dd-item" data-action="toggle-find-replace">Find & Replace</div>
          <div class="dd-item divider" data-action="set-view" data-view="preview" title="Rendered preview">Rendered preview</div>
          ${pane === 'primary' ? '<div class="dd-item" id="workspace-split-toggle" data-action="toggle-workspace-split">Open files side by side</div>' : ''}
        </div>
      </div>

      <div class="workspace-toolbar__right">
        <div class="vseg">
          <div class="vb${paneView === 'wysiwyg' ? ' active' : ''}" data-action="set-view" data-view="wysiwyg" title="Rich text editor" aria-label="Rich text editor" role="button" tabindex="0">Rich Text</div>
          <div class="vb${paneView === 'markdown' ? ' active' : ''}" data-action="set-view" data-view="markdown" title="Markdown source" aria-label="Markdown source" role="button" tabindex="0">Markdown</div>
          <div class="vb${paneView === 'split' ? ' active' : ''}" data-action="set-view" data-view="split" title="Pane split preview" aria-label="Pane split preview" role="button" tabindex="0">Split</div>
        </div>
        <div class="workspace-toolbar__mode-separator" aria-hidden="true"></div>
        <div class="editor-shelf-hide" data-action="toggle-toolbar" aria-label="Hide editor controls" role="button" tabindex="0">
          ${chevronIcon()}
        </div>
      </div>
    </div>
    <div class="workspace-pane-row" data-pane="${pane}">
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
            <div class="workspace-tabs-wrap">
              <div class="workspace-tabs" id="tabs-primary"></div>
              <div class="workspace-tabs__overflow" id="tabs-overflow-primary" data-action="tab-overflow" data-pane="primary" role="button" tabindex="0" title="All open tabs" aria-label="All open tabs" hidden>
                <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
              </div>
            </div>
            <div class="editor-controls-toggle" data-action="toggle-toolbar" aria-label="Toggle editor controls" aria-pressed="${state.toolbarVisible ? 'true' : 'false'}" role="button" tabindex="0">
              ${editorSplitIcon()}
            </div>
          </div>
          <div class="workspace-breadcrumb hidden" id="workspace-breadcrumb-primary"></div>
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
            <div class="workspace-tabs-wrap">
              <div class="workspace-tabs" id="tabs-secondary"></div>
              <div class="workspace-tabs__overflow" id="tabs-overflow-secondary" data-action="tab-overflow" data-pane="secondary" role="button" tabindex="0" title="All open tabs" aria-label="All open tabs" hidden>
                <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
              </div>
            </div>
            <div class="editor-controls-toggle" data-action="toggle-toolbar" aria-label="Toggle editor controls" aria-pressed="${state.toolbarVisible ? 'true' : 'false'}" role="button" tabindex="0">
              ${editorSplitIcon()}
            </div>
          </div>
          <div class="workspace-breadcrumb hidden" id="workspace-breadcrumb-secondary"></div>
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
            <div class="workspace-pane__empty-kicker">Editor B is ready</div>
            <div class="workspace-pane__empty-title">Split workspace</div>
            <div class="workspace-pane__empty-copy">Open another markdown file to compare, reference, or edit beside the current note.</div>
            <div class="workspace-pane__empty-action" data-action="open-secondary-file" role="button" tabindex="0">Quick open</div>
          </div>
        </section>
      </div>
    </div>
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
  restoreRightPanel()
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
    smartTypographyEnabled: getSettings().smartTypography !== false,
    focusModeEnabled: Boolean(getSettings().focusMode),
    livePreviewEnabled: Boolean(getSettings().livePreview),
    posHighlightEnabled: Boolean(getSettings().posHighlight),
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
      console.error(`[rista] Error destroying WYSIWYG editor for pane "${pane}":`, err)
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
  const tab = getTabForPane(pane)
  const markdown = getRenderableMarkdown(tab?.content ?? '', {
    hideFrontmatter: getSettings().hideFrontmatterInRenderedModes,
  }).body
  destroyRichEditor(pane)

  const host = document.createElement('div')
  host.className = 'wysiwyg-editor'
  host.id = `wysiwyg-editor-${pane}`
  const bannerHost = document.createElement('div')
  bannerHost.className = 'wysiwyg-banner'
  bannerHost.id = `wysiwyg-banner-${pane}`
  const editorHost = document.createElement('div')
  editorHost.className = 'wysiwyg-editor__body'
  host.appendChild(bannerHost)
  host.appendChild(editorHost)
  slotHost.appendChild(host)

  try {
    richEditors[pane] = new ToastEditor({
      el: editorHost,
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
        blur: () => _callbacks.onRichEditorChange?.(pane),
      },
    })
    richEditors[pane].setHeight('100%')
    richEditorMountTarget[pane] = mountTarget
    renderWysiwygBanner(pane)
    return richEditors[pane]
  } catch (err) {
    console.error(`[rista] Failed to mount WYSIWYG editor for pane "${pane}":`, err)
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
  renderWysiwygBanner(pane)
  syncToWysiwyg(pane)
}

export function renderWysiwygBanner(pane) {
  const bannerHost = document.getElementById(`wysiwyg-banner-${pane}`)
  if (!bannerHost) return
  const tab = getTabForPane(pane)
  const settings = getSettings()
  const renderable = getRenderableMarkdown(tab?.content || '', { hideFrontmatter: true })
  const html = settings.showDocumentBanners === false ? '' : renderDocumentBanner(renderable.frontmatter, {
    currentFilePath: tab?.path,
    folderPath: state.folderPath,
    attachmentPaths: getWorkspaceAttachmentPaths(),
  })
  bannerHost.innerHTML = html
  bannerHost.classList.toggle('hidden', !html)
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
    if (pane === 'secondary' && !dual) return
    if (panesMain) panesMain.style.display = tab ? 'flex' : 'none'
    if (empty && pane === 'secondary') empty.style.display = tab ? 'none' : 'flex'
    syncWorkspaceBreadcrumb(pane)
    applyToolbarVisibility(pane)

    // Handle non-markdown previews: hide editor/preview, show only preview host
    if (tab?.isAttachment || tab?.isSpatial || tab?.isBinary) {
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
}

export function applyEditorSettings() {
  const s = getSettings()
  PANE_KEYS.forEach(pane => {
    const view = editorViews[pane]
    if (!view) return
    updateSmartTypography(view, s.smartTypography !== false)
    updateFocusMode(view, Boolean(s.focusMode))
    updateLivePreview(view, Boolean(s.livePreview))
    updatePosHighlight(view, Boolean(s.posHighlight))
  })
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
  document.querySelectorAll('.workspace-pane__header').forEach(node => {
    node.addEventListener('click', handleToolbarClick)
    node.addEventListener('keydown', event => {
      if (event.key !== 'Enter' && event.key !== ' ') return
      const control = event.target.closest('[data-action]')
      if (!control) return
      event.preventDefault()
      control.click()
    })
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
  document.querySelectorAll('.cm-host').forEach(host => {
    host.addEventListener('contextmenu', (e) => {
      const pane = host.id.replace('cm-host-', '')
      const view = editorViews[pane]
      if (!view) return
      const sel = view.state.selection.main
      if (sel.from === sel.to) return
      e.preventDefault()
      showAiContextMenu(e.clientX, e.clientY, view)
    })
  })
  document.querySelectorAll('.workspace-tabs').forEach(node => {
    node.addEventListener('dragover', e => _callbacks.handleTabDragOver?.(e))
    node.addEventListener('dragleave', e => _callbacks.handleTabDragLeave?.(e))
    node.addEventListener('drop', e => _callbacks.handleTabDrop?.(e))
  })
  handleSearchInput(target => {
    if (!target) return
    if (typeof target === 'string') {
      _callbacks.openFile?.({ path: target, name: fileName(target) })
      return
    }
    _callbacks.openFile?.(target)
  })
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
  if (action === 'toggle-workspace-split') toggleWorkspaceSplit()
  if (action === 'toggle-toolbar') toggleToolbar()
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
  applyToolbarVisibility(pane)
  const toolbar = document.querySelector(`.workspace-toolbar[data-pane="${pane}"]`)
  ;['markdown', 'split', 'wysiwyg', 'preview'].forEach(viewName => {
    toolbar?.querySelectorAll(`.vb[data-view="${viewName}"]`).forEach(node => {
      node.classList.toggle('active', viewName === view)
    })
  })
  // syncSplitLayout handles mounting + syncing WYSIWYG via maybeRefreshWysiwygPane.
  // Calling syncToWysiwyg before layout is ready can cause mount into hidden containers.
  syncSplitLayout()
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
  PANE_KEYS.forEach(pane => applyToolbarVisibility(pane))
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
  if (control) {
    control.classList.toggle('active', state.sidebarVisible)
    control.setAttribute('aria-pressed', state.sidebarVisible ? 'true' : 'false')
  }
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
