import { state, $ } from './state.js'
import { getOutgoingLinks, getBacklinks, getAllMdFileNames } from './link-index.js'
import { getStats, extractHeadings } from './markdown.js'
import { getSettings } from './settings.js'

// ── Inspector Panel ──────────────────────────────────────────────
// Replaces the insights panel with a proper right inspector with tabs.

const TABS = [
  { id: 'outline', label: 'Outline', icon: outlineIcon },
  { id: 'links', label: 'Links', icon: linksIcon },
  { id: 'stats', label: 'Stats', icon: statsIcon },
  { id: 'ai', label: 'AI', icon: aiIcon },
]

let activeTab = 'outline'

export function buildInspector() {
  return `
    <aside class="inspector" id="inspector">
      <div class="inspector__header">
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
        <div class="inspector__close" id="inspector-close-btn" title="Close inspector" role="button" tabindex="0">
          <svg viewBox="0 0 16 16"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/></svg>
        </div>
      </div>
      <div class="inspector__body" id="inspector-body"></div>
    </aside>
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
  const markdown = state.activeTab?.content || ''

  switch (activeTab) {
    case 'outline':
      body.innerHTML = renderOutlineContent(markdown)
      break
    case 'links':
      body.innerHTML = renderLinksContent()
      break
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
  const tab = state.activeTab
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

  const link = event.target.closest('.inspector-link[data-link-path]')
  if (link && link.dataset.linkPath) {
    const linkPath = link.dataset.linkPath
    openFileFn?.({ path: linkPath, name: linkPath.split('/').pop() })
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

function statsIcon() {
  return `<svg viewBox="0 0 16 16"><rect x="2" y="9" width="3" height="5" rx="0.5" fill="currentColor" stroke="none"/><rect x="6.5" y="5" width="3" height="9" rx="0.5" fill="currentColor" stroke="none"/><rect x="11" y="2" width="3" height="12" rx="0.5" fill="currentColor" stroke="none"/></svg>`
}

function aiIcon() {
  return `<svg viewBox="0 0 16 16"><path d="M8 1.5l2 4.5 4.5 2-4.5 2-2 4.5-2-4.5L1.5 8l4.5-2z" fill="currentColor"/></svg>`
}
