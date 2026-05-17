import { state, $, getFocusedTab } from './state.js'
import { getOutgoingLinks, getBacklinks, getAllMdFileNames, getTagsForFile, getAllTagNames, getFilesForTag } from './link-index.js'
import { getStats, extractHeadings } from './markdown.js'
import { getSettings } from './settings.js'
import { registerRightPanel } from './right-panel.js'
import { getSnapshots, loadSnapshot, relativeTime, formatSize } from './history.js'

// ── Inspector Panel ──────────────────────────────────────────────
// Registered as a right panel via the shared right-panel system.

const TABS = [
  { id: 'outline', label: 'Outline', icon: outlineIcon },
  { id: 'links', label: 'Links', icon: linksIcon },
  { id: 'tags', label: 'Tags', icon: tagsIcon },
  { id: 'history', label: 'History', icon: historyIcon },
  { id: 'stats', label: 'Stats', icon: statsIcon },
  { id: 'ai', label: 'AI', icon: aiIcon },
]

let activeTab = 'outline'

export function buildInspector() {
  return `
    <div class="inspector">
      <div class="inspector__tabs">
        ${TABS.map(tab => `
          <div
            class="inspector__tab${tab.id === activeTab ? ' active' : ''}"
            data-inspector-tab="${tab.id}"
            role="button"
            tabindex="0"
            title="${tab.label}"
          >
            ${tab.icon()}
          </div>
        `).join('')}
      </div>
      <div class="inspector__body" id="inspector-body"></div>
    </div>
  `
}

export function setInspectorTab(tabId) {
  activeTab = TABS.some(t => t.id === tabId) ? tabId : 'outline'
  syncInspectorTabs()
  renderInspectorContent()
}

export function syncInspectorTabs() {
  document.querySelectorAll('[data-inspector-tab]').forEach(node => {
    node.classList.toggle('active', node.dataset.inspectorTab === activeTab)
  })
}

export function renderInspectorContent() {
  const body = $('inspector-body')
  if (!body) return
  const markdown = getFocusedTab()?.content || ''

  switch (activeTab) {
    case 'outline':
      body.innerHTML = renderOutlineContent(markdown)
      break
    case 'links':
      body.innerHTML = renderLinksContent()
      break
    case 'tags':
      body.innerHTML = renderTagsContent()
      break
    case 'history':
      renderHistoryContent(body)
      return
    case 'stats':
      body.innerHTML = renderStatsContent(markdown)
      break
    case 'ai':
      body.innerHTML = renderAiContent()
      break
  }
}

function renderOutlineContent(markdown) {
  const headings = extractHeadings(markdown)
  if (!headings.length) {
    return `<div class="inspector-empty"><span>No headers yet</span></div>`
  }
  return `
    <div class="inspector-outline">
      ${headings.map(h => `
        <div class="inspector-outline__item inspector-outline__item--h${h.level}">${escapeHtml(h.text)}</div>
      `).join('')}
    </div>
  `
}

function renderLinksContent() {
  const tab = getFocusedTab()
  if (!tab?.path) {
    return `<div class="inspector-empty"><span>No file open</span></div>`
  }

  const outgoing = getOutgoingLinks(tab.path)
  const backlinks = getBacklinks(tab.path)

  let html = ''

  if (outgoing.length) {
    html += `<div class="inspector-section__title">Outgoing</div>`
    html += `<div class="inspector-link-list">`
    for (const link of outgoing) {
      const statusClass = link.exists ? 'resolved' : 'unresolved'
      html += `
        <div class="inspector-link ${statusClass}" data-link-path="${escapeAttr(link.targetPath || '')}">
          <span class="inspector-link__name">${escapeHtml(link.linkText)}</span>
          ${link.exists ? '' : '<span class="inspector-link__badge">missing</span>'}
        </div>
      `
    }
    html += `</div>`
  }

  if (backlinks.length) {
    html += `<div class="inspector-section__title inspector-section__title--backlinks">Backlinks</div>`
    html += `<div class="inspector-link-list">`
    for (const backlink of backlinks) {
      html += `
        <div class="inspector-link resolved" data-link-path="${escapeAttr(backlink.sourcePath)}">
          <span class="inspector-link__name">${escapeHtml(backlink.sourceName)}</span>
          ${backlink.context ? `<span class="backlink-context">${backlink.context}</span>` : ''}
        </div>
      `
    }
    html += `</div>`
  }

  if (!outgoing.length && !backlinks.length) {
    html = `<div class="inspector-empty"><span>No links in this note</span></div>`
  }

  return html
}

function renderStatsContent(markdown) {
  const stats = getStats(markdown, getSettings().readingSpeed)
  return `
    <div class="inspector-stats">
      <div class="inspector-stat"><span class="inspector-stat__num">${stats.words}</span><span class="inspector-stat__lbl">Words</span></div>
      <div class="inspector-stat"><span class="inspector-stat__num">${stats.chars}</span><span class="inspector-stat__lbl">Characters</span></div>
      <div class="inspector-stat"><span class="inspector-stat__num">${stats.paragraphs}</span><span class="inspector-stat__lbl">Paragraphs</span></div>
      <div class="inspector-stat"><span class="inspector-stat__num">${stats.sentences || 0}</span><span class="inspector-stat__lbl">Sentences</span></div>
      <div class="inspector-stat"><span class="inspector-stat__num">${stats.readMin < 1 ? '< 1' : stats.readMin}</span><span class="inspector-stat__lbl">Min read</span></div>
      <div class="inspector-stat"><span class="inspector-stat__num">${stats.fkGrade || 0}</span><span class="inspector-stat__lbl">FK Grade</span></div>
      <div class="inspector-stat inspector-stat--wide"><span class="inspector-stat__num" style="font-size:14px">${stats.avgWordLen || 0}</span><span class="inspector-stat__lbl">Avg word length</span></div>
      <div class="inspector-stat inspector-stat--wide"><span class="inspector-stat__num" style="font-size:14px">${stats.avgSentenceLen || 0}</span><span class="inspector-stat__lbl">Avg sentence length</span></div>
    </div>
  `
}

function renderTagsContent() {
  const tab = getFocusedTab()
  if (!tab?.path) {
    return `<div class="inspector-empty"><span>No file open</span></div>`
  }

  const fileTags = getTagsForFile(tab.path)
  const allTags = getAllTagNames()

  let html = ''

  // Current file tags
  html += `<div class="inspector-section__title">This file</div>`
  if (fileTags.length) {
    html += `<div class="inspector-tags">`
    for (const tag of fileTags) {
      html += `<span class="tag-pill" data-tag="${escapeAttr(tag)}">#${escapeHtml(tag)}</span>`
    }
    html += `</div>`
  } else {
    html += `<div class="inspector-empty"><span>No tags</span></div>`
  }

  // All project tags
  if (allTags.length) {
    html += `<div class="inspector-section__title" style="margin-top:14px">All tags</div>`
    html += `<div class="tag-section">`
    for (const tag of allTags) {
      const count = getFilesForTag(tag).length
      html += `
        <div class="inspector-link" data-tag="${escapeAttr(tag)}">
          <span class="inspector-link__name">#${escapeHtml(tag)}</span>
          <span class="inspector-link__badge" style="background:var(--border2);color:var(--text3)">${count}</span>
        </div>
      `
    }
    html += `</div>`
  }

  return html
}

async function renderHistoryContent(body) {
  const tab = getFocusedTab()
  if (!tab?.path) {
    body.innerHTML = `<div class="inspector-empty"><span>No file open</span></div>`
    return
  }

  body.innerHTML = `<div class="inspector-empty"><span>Loading history...</span></div>`

  const snapshots = await getSnapshots(tab.path)
  if (!snapshots.length) {
    body.innerHTML = `<div class="inspector-empty"><span>No history yet</span><p class="inspector-empty__sub">Snapshots are saved automatically each time you save.</p></div>`
    return
  }

  let html = `<div class="inspector-section__title">Snapshots (${snapshots.length})</div>`
  html += `<div class="history-list">`
  for (const snapshot of snapshots) {
    html += `
      <div class="history-item" data-snapshot-path="${escapeAttr(snapshot.path)}" role="button" tabindex="0">
        <div class="history-item__time">${escapeHtml(relativeTime(snapshot.timestamp))}</div>
        <div class="history-item__meta">${escapeHtml(snapshot.timestampStr)} &middot; ${escapeHtml(formatSize(snapshot.size))}</div>
      </div>
    `
  }
  html += `</div>`
  body.innerHTML = html
}

function renderAiContent() {
  return `
    <div class="inspector-empty inspector-empty--ai">
      <span>AI Review</span>
      <p class="inspector-empty__sub">AI-powered review will appear here when suggestions are available.</p>
    </div>
  `
}

export function handleInspectorClick(event, openFileFn) {
  const tab = event.target.closest('[data-inspector-tab]')
  if (tab) {
    setInspectorTab(tab.dataset.inspectorTab)
    return
  }

  const historyItem = event.target.closest('.history-item[data-snapshot-path]')
  if (historyItem) {
    const snapshotPath = historyItem.dataset.snapshotPath
    loadSnapshot(snapshotPath).then(content => {
      if (content != null) {
        const name = snapshotPath.split(/[/\\]/).pop()
        openFileFn?.({ path: snapshotPath, name: `[snapshot] ${name}` })
      }
    })
    return
  }

  const link = event.target.closest('.inspector-link[data-link-path]')
  if (link && link.dataset.linkPath) {
    const linkPath = link.dataset.linkPath
    openFileFn?.({ path: linkPath, name: linkPath.split('/').pop() })
    return
  }

  // Tag click (pill or list item) — open the first file with this tag,
  // or no-op if only the current file has it.
  const tagEl = event.target.closest('[data-tag]')
  if (tagEl) {
    const tag = tagEl.dataset.tag
    const files = getFilesForTag(tag)
    const current = getFocusedTab()?.path
    const others = files.filter(p => p !== current)
    if (others.length > 0) {
      const path = others[0]
      openFileFn?.({ path, name: path.split(/[/\\]/).pop() })
    }
    return
  }
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function escapeAttr(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

// ── Icons ────────────────────────────────────────────────────────
function outlineIcon() {
  return `<svg viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="1.5" rx="0.5" fill="currentColor"/><rect x="2" y="6.5" width="9" height="1.5" rx="0.5" fill="currentColor"/><rect x="2" y="10" width="10" height="1.5" rx="0.5" fill="currentColor"/></svg>`
}

function linksIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M6.5 9.5a3.5 3.5 0 0 0 5 0l2-2a3.5 3.5 0 0 0-5-5l-1 1"/><path d="M9.5 6.5a3.5 3.5 0 0 0-5 0l-2 2a3.5 3.5 0 0 0 5 5l1-1"/></svg>`
}

function tagsIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M2 7.5V3a1 1 0 0 1 1-1h4.5L14 8.5 8.5 14 2 7.5z" fill="none" stroke="currentColor" stroke-width="1.3"/><circle cx="5.5" cy="5.5" r="1" fill="currentColor"/></svg>`
}

function historyIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M8 3.5v5l3.5 2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/><circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.3"/></svg>`
}

function statsIcon() {
  return `<svg viewBox="0 0 16 16"><rect x="2" y="9" width="3" height="5" rx="0.5" fill="currentColor" stroke="none"/><rect x="6.5" y="5" width="3" height="9" rx="0.5" fill="currentColor" stroke="none"/><rect x="11" y="2" width="3" height="12" rx="0.5" fill="currentColor" stroke="none"/></svg>`
}

function aiIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M8 1.5l2 4.5 4.5 2-4.5 2-2 4.5-2-4.5L1.5 8l4.5-2z" fill="currentColor"/></svg>`
}

// ── Register as right panel ─────────────────────────────────────
let _openFileFn = null
let _closeRightPanelFn = null

export function initInspectorPanel(openFileFn, closeRightPanelFn) {
  _openFileFn = openFileFn
  _closeRightPanelFn = closeRightPanelFn
  registerRightPanel('inspector', {
    title: 'Inspector',
    icon: inspectorWidgetIcon(),
    flex: 2,
    build: buildInspector,
    onMount: () => {
      state.inspectorOpen = true
      renderInspectorContent()
      const container = $('right-panel-container')
      if (container && !container.dataset.inspectorClickWired) {
        container.addEventListener('click', _onContainerClick)
        container.dataset.inspectorClickWired = 'true'
      }
    },
    onUnmount: () => {
      state.inspectorOpen = false
    },
    onRefresh: renderInspectorContent,
  })
}

function inspectorWidgetIcon() {
  return `<svg viewBox="0 0 16 16" width="11" height="11"><rect x="1" y="1.5" width="9" height="13" rx="1.5" fill="none" stroke="currentColor"/><line x1="12" y1="3" x2="15" y2="3" stroke="currentColor" stroke-linecap="round"/><line x1="12" y1="6" x2="15" y2="6" stroke="currentColor" stroke-linecap="round"/><line x1="12" y1="9" x2="15" y2="9" stroke="currentColor" stroke-linecap="round"/></svg>`
}

function _onContainerClick(event) {
  // Inspector close button no longer exists in widget mode — widget header has its own close
  handleInspectorClick(event, _openFileFn)
}
