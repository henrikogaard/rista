import { getLinkIndex, resolveWikilink } from './link-index.js'
import { state } from './state.js'
import { analyzeWikiQuality } from './wiki-quality.js'

let _openFile = null

export function buildWikiQualityPanel() {
  return `<div class="wiki-quality" id="wiki-quality-body"></div>`
}

export function mountWikiQualityPanel(openFile) {
  _openFile = openFile || _openFile
  renderWikiQualityPanel()
}

export function renderWikiQualityPanel() {
  const body = document.getElementById('wiki-quality-body')
  if (!body) return

  if (!state.folderPath) {
    body.innerHTML = `<div class="wiki-quality-empty">No folder open</div>`
    return
  }

  const report = analyzeWikiQuality(getLinkIndex(), {
    folderPath: state.folderPath,
    resolveLink: resolveWikilink,
  })
  const totalFindings = report.summary.unresolvedLinks +
    report.summary.orphanNotes +
    report.summary.duplicateTitles +
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
    ${renderGlossaryCandidates(report.glossaryCandidates, report.summary.glossaryCandidates)}
  `
}

export function handleWikiQualityPanelEvent(event) {
  const row = event.target.closest?.('[data-path], [data-source-path]')
  if (!row) return
  if (event.type === 'keydown' && event.key !== 'Enter' && event.key !== ' ') return
  event.preventDefault()
  const path = row.dataset.path || row.dataset.sourcePath
  if (!path) return
  _openFile?.({ path, name: path.split(/[/\\]/).pop() })
}

function renderSummary(summary) {
  return `
    <div class="wiki-quality__summary">
      ${renderStat('Missing', summary.unresolvedLinks)}
      ${renderStat('Orphans', summary.orphanNotes)}
      ${renderStat('Titles', summary.duplicateTitles)}
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
    <div class="wiki-quality-row" data-source-path="${escapeAttribute(item.sourcePath)}" role="button" tabindex="0" title="${escapeAttribute(item.sourceRelativePath)}">
      <span class="wiki-quality-row__main">[[${escapeHtml(item.linkText)}]]</span>
      <span class="wiki-quality-row__meta">${escapeHtml(item.sourceName)}</span>
    </div>
  `)
}

function renderOrphans(items, total) {
  return renderSection('Orphan notes', items, total, item => `
    <div class="wiki-quality-row" data-path="${escapeAttribute(item.path)}" role="button" tabindex="0" title="${escapeAttribute(item.relativePath)}">
      <span class="wiki-quality-row__main">${escapeHtml(item.title)}</span>
      <span class="wiki-quality-row__meta">${escapeHtml(item.relativePath)}</span>
    </div>
  `)
}

function renderDuplicates(items, total) {
  return renderSection('Duplicate titles', items, total, item => `
    <div class="wiki-quality-row" data-path="${escapeAttribute(item.paths[0])}" role="button" tabindex="0" title="${escapeAttribute(item.relativePaths.join('\n'))}">
      <span class="wiki-quality-row__main">${escapeHtml(item.title)}</span>
      <span class="wiki-quality-row__meta">${item.paths.length} notes</span>
    </div>
  `)
}

function renderGlossaryCandidates(items, total) {
  return renderSection('Glossary candidates', items, total, item => `
    <div class="wiki-quality-row" data-path="${escapeAttribute(item.files[0])}" role="button" tabindex="0" title="${escapeAttribute(item.term)}">
      <span class="wiki-quality-row__main">${escapeHtml(item.term)}</span>
      <span class="wiki-quality-row__meta">${item.fileCount} files</span>
    </div>
  `)
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

function escapeHtml(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function escapeAttribute(value) {
  return escapeHtml(value).replace(/'/g, '&#39;')
}
