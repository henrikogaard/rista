import { getLinkIndex, rebuildLinkIndex, resolveWikilink } from './link-index.js'
import { state, $, escapeHtml, fileName, stripMarkdownExtension } from './state.js'
import { analyzeWikiQuality } from './wiki-quality.js'
let _openFile = null
let _refreshTree = null
const DISMISSED_KEY = 'rista-wiki-quality-dismissed'

export function buildWikiQualityPanel() {
  return `<div class="wiki-quality" id="wiki-quality-body"></div>`
}

export function mountWikiQualityPanel(openFile, options = {}) {
  _openFile = openFile || _openFile
  _refreshTree = options.refreshTree || _refreshTree
  renderWikiQualityPanel()
}

export function renderWikiQualityPanel() {
  const body = $('wiki-quality-body')
  if (!body) return

  if (!state.folderPath) {
    body.innerHTML = `<div class="wiki-quality-empty">No folder open</div>`
    return
  }

  const report = analyzeWikiQuality(getLinkIndex(), {
    folderPath: state.folderPath,
    resolveLink: resolveWikilink,
    dismissed: getDismissedFindings(),
  })
  const totalFindings = report.summary.unresolvedLinks +
    report.summary.orphanNotes +
    report.summary.duplicateTitles +
    report.summary.nearDuplicateNotes +
    report.summary.glossaryCandidates

  if (totalFindings === 0) {
    body.innerHTML = `
      ${renderSummary(report.summary)}
      <div class="wiki-quality-empty">No quality findings</div>
    `
    return
  }

  body.innerHTML = `
    ${renderSummary(report.summary)}
    ${report.summary.stale ? '<div class="wiki-quality__stale">Index updating</div>' : ''}
    ${renderMissingLinks(report.unresolvedLinks, report.summary.unresolvedLinks)}
    ${renderOrphans(report.orphanNotes, report.summary.orphanNotes)}
    ${renderDuplicates(report.duplicateTitles, report.summary.duplicateTitles)}
    ${renderNearDuplicates(report.nearDuplicateNotes, report.summary.nearDuplicateNotes)}
    ${renderGlossaryCandidates(report.glossaryCandidates, report.summary.glossaryCandidates)}
  `
}

export function handleWikiQualityPanelEvent(event) {
  const action = event.target.closest?.('[data-action]')
  if (action) {
    if (event.type === 'keydown' && event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    event.stopPropagation()
    if (action.dataset.action === 'create-missing-note') {
      createMissingNoteFromFinding(action.dataset.linkText)
      return
    }
    if (action.dataset.action === 'dismiss-finding') {
      dismissWikiQualityFinding(action.dataset.findingKey)
      return
    }
  }

  const row = event.target.closest?.('[data-path], [data-source-path]')
  if (!row) return
  if (event.type === 'keydown' && event.key !== 'Enter' && event.key !== ' ') return
  event.preventDefault()
  const path = row.dataset.path || row.dataset.sourcePath
  if (!path) return
  _openFile?.({ path, name: fileName(path) })
}

function renderSummary(summary) {
  return `
    <div class="wiki-quality__summary">
      ${renderStat('Missing', summary.unresolvedLinks)}
      ${renderStat('Orphans', summary.orphanNotes)}
      ${renderStat('Duplicates', summary.duplicateTitles + summary.nearDuplicateNotes)}
      ${renderStat('Terms', summary.glossaryCandidates)}
    </div>
  `
}

function renderStat(label, value) {
  return `
    <div class="wiki-quality-stat">
      <span>${escapeHtml(label)}</span>
      <strong>${value}</strong>
    </div>
  `
}

function renderMissingLinks(items, total) {
  return renderSection('Missing links', items, total, item => `
    <div class="wiki-quality-row" data-source-path="${escapeAttribute(item.sourcePath)}" data-finding-key="${escapeAttribute(item.key)}" role="button" tabindex="0" title="${escapeAttribute(item.sourceRelativePath)}">
      <span class="wiki-quality-row__main">[[${escapeHtml(item.linkText)}]]</span>
      <span class="wiki-quality-row__meta">${escapeHtml(item.sourceName)}</span>
      <span class="wiki-quality-row__actions">
        <button type="button" class="wiki-quality-action" data-action="create-missing-note" data-link-text="${escapeAttribute(item.linkText)}">Create</button>
        ${renderDismissButton(item.key)}
      </span>
    </div>
  `)
}

function renderOrphans(items, total) {
  return renderSection('Orphan notes', items, total, item => `
    <div class="wiki-quality-row" data-path="${escapeAttribute(item.path)}" data-finding-key="${escapeAttribute(item.key)}" role="button" tabindex="0" title="${escapeAttribute(item.relativePath)}">
      <span class="wiki-quality-row__main">${escapeHtml(item.title)}</span>
      <span class="wiki-quality-row__meta">${escapeHtml(item.relativePath)}</span>
      <span class="wiki-quality-row__actions">${renderDismissButton(item.key)}</span>
    </div>
  `)
}

function renderDuplicates(items, total) {
  return renderSection('Duplicate titles', items, total, item => `
    <div class="wiki-quality-row" data-path="${escapeAttribute(item.paths[0])}" data-finding-key="${escapeAttribute(item.key)}" role="button" tabindex="0" title="${escapeAttribute(item.relativePaths.join('\n'))}">
      <span class="wiki-quality-row__main">${escapeHtml(item.title)}</span>
      <span class="wiki-quality-row__meta">${item.paths.length} notes</span>
      <span class="wiki-quality-row__actions">${renderDismissButton(item.key)}</span>
    </div>
  `)
}

function renderNearDuplicates(items, total) {
  return renderSection('Similar notes', items, total, item => `
    <div class="wiki-quality-row" data-path="${escapeAttribute(item.paths[0])}" data-finding-key="${escapeAttribute(item.key)}" role="button" tabindex="0" title="${escapeAttribute(item.relativePaths.join('\n'))}">
      <span class="wiki-quality-row__main">${escapeHtml(item.title)}</span>
      <span class="wiki-quality-row__meta">${Math.round(item.similarity * 100)}%</span>
      <span class="wiki-quality-row__actions">${renderDismissButton(item.key)}</span>
    </div>
  `)
}

function renderGlossaryCandidates(items, total) {
  return renderSection('Glossary candidates', items, total, item => `
    <div class="wiki-quality-row" data-path="${escapeAttribute(item.files[0])}" data-finding-key="${escapeAttribute(item.key)}" role="button" tabindex="0" title="${escapeAttribute(item.term)}">
      <span class="wiki-quality-row__main">${escapeHtml(item.term)}</span>
      <span class="wiki-quality-row__meta">${item.fileCount} files</span>
      <span class="wiki-quality-row__actions">${renderDismissButton(item.key)}</span>
    </div>
  `)
}

function renderDismissButton(key) {
  return `<button type="button" class="wiki-quality-action wiki-quality-action--muted" data-action="dismiss-finding" data-finding-key="${escapeAttribute(key)}">Dismiss</button>`
}

function renderSection(title, items, total, renderItem) {
  if (total === 0) return ''
  const more = total > items.length ? `<span class="wiki-quality-section__more">+${total - items.length}</span>` : ''
  return `
    <section class="wiki-quality-section">
      <div class="wiki-quality-section__title">
        <span>${escapeHtml(title)}</span>
        ${more}
      </div>
      <div class="wiki-quality-section__items">
        ${items.map(renderItem).join('')}
      </div>
    </section>
  `
}



function escapeAttribute(value) {
  return escapeHtml(value).replace(/'/g, '&#39;')
}

function getDismissedFindings() {
  try {
    return new Set(JSON.parse(localStorage.getItem(DISMISSED_KEY) || '[]'))
  } catch {
    return new Set()
  }
}

function setDismissedFindings(dismissed) {
  try {
    localStorage.setItem(DISMISSED_KEY, JSON.stringify(Array.from(dismissed)))
  } catch {}
}

export async function createMissingNoteFromFinding(linkText) {
  if (!window.fjord || !state.folderPath || !linkText) return
  const target = cleanMissingTarget(linkText)
  if (!target) return
  const filePath = `${state.folderPath}/${target.endsWith('.md') ? target : `${target}.md`}`
  const title = stripMarkdownExtension(fileName(filePath))
  const parent = filePath.replace(/[/\\][^/\\]+$/, '')
  if (parent && parent !== state.folderPath) {
    try { await window.fjord.createDir?.(parent) } catch {}
  }
  const exists = await window.fjord.stat?.(filePath)
  if (!exists) {
    await window.fjord.writeFile(filePath, `# ${title}\n\n`)
  }
  await _refreshTree?.()
  try { await rebuildLinkIndex() } catch {}
  _openFile?.({ path: filePath, name: fileName(filePath) })
  renderWikiQualityPanel()
}

export function dismissWikiQualityFinding(key) {
  if (!key) return
  const dismissed = getDismissedFindings()
  dismissed.add(key)
  setDismissedFindings(dismissed)
  renderWikiQualityPanel()
}

function cleanMissingTarget(linkText) {
  const target = String(linkText || '')
    .split('#')[0]
    .trim()
    .replace(/\\/g, '/')
    .replace(/^\/+/, '')
    .replace(/\.md$/i, '')
  return target
    .split('/')
    .filter(segment => segment && segment !== '.' && segment !== '..')
    .join('/')
}
