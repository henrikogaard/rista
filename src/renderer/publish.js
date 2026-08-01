import { showStatusNotice } from './tabs.js'
import { state, escapeHtml, fileName, stripMarkdownExtension } from './state.js'
import { renderMarkdown } from './markdown.js'
import { getLinkIndex } from './link-index.js'
import { getSettings } from './settings.js'

// ── Static Site Export ──────────────────────────────────────────
// Exports all markdown files in the project as a static HTML website.




function resolveWikilinksToHtml(html, allBaseNames) {
  // Replace wikilink anchors: <a class="wikilink" data-wikilink="NoteName" href="#">...
  return html.replace(
    /<a\s+class="wikilink"\s+data-wikilink="([^"]+)"\s+href="#">([^<]*)<\/a>/g,
    (match, target, text) => {
      const normalized = target.trim()
      const found = allBaseNames.find(b => b.toLowerCase() === normalized.toLowerCase())
      if (found) {
        return `<a class="wikilink" href="${encodeURIComponent(found)}.html">${text}</a>`
      }
      return `<a class="wikilink wikilink--broken">${text}</a>`
    }
  )
}

function getThemeColors() {
  const root = document.documentElement
  const style = getComputedStyle(root)
  const theme = root.getAttribute('data-theme') === 'light' ? 'light' : 'dark'
  return {
    theme,
    bg: style.getPropertyValue('--bg0').trim(),
    surface: style.getPropertyValue('--bg1').trim(),
    text: style.getPropertyValue('--text1').trim(),
    muted: style.getPropertyValue('--text2').trim(),
    border: style.getPropertyValue('--border').trim() || (theme === 'light' ? 'rgba(34,29,24,0.12)' : 'rgba(255,255,255,0.08)'),
    accent: style.getPropertyValue('--accent').trim(),
  }
}

function buildSitePageHtml({ title, bodyHtml, nav, colors, settings }) {
  const previewFont = settings.previewFontCustom || settings.previewFont || "'DM Sans', system-ui, sans-serif"
  const previewFontSize = settings.previewFontSize || 13
  const previewLineHeight = settings.previewLineHeight || 1.75

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>${escapeHtml(title)}</title>
  <style>
    :root {
      --bg: ${colors.bg};
      --surface: ${colors.surface};
      --text: ${colors.text};
      --muted: ${colors.muted};
      --border: ${colors.border};
      --accent: ${colors.accent};
      --font: ${previewFont};
      --font-size: ${previewFontSize}px;
      --line-height: ${previewLineHeight};
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; padding: 0; background: var(--bg); color: var(--text); }
    body { font-family: var(--font); font-size: var(--font-size); line-height: var(--line-height); }
    .site-layout { display: flex; min-height: 100vh; }
    .site-nav {
      width: 220px; flex-shrink: 0; padding: 24px 16px;
      background: var(--surface); border-right: 1px solid var(--border);
      overflow-y: auto; position: sticky; top: 0; height: 100vh;
    }
    .site-nav__title { font-size: 14px; font-weight: 600; margin: 0 0 16px; color: var(--text); }
    .site-nav__link {
      display: block; padding: 4px 8px; margin: 2px 0; 
      color: var(--muted); text-decoration: none; font-size: 12px;
    }
    .site-nav__link:hover { background: rgba(127,127,127,0.1); color: var(--text); }
    .site-nav__link.active { background: rgba(127,127,127,0.14); color: var(--accent); }
    .page { max-width: 860px; margin: 0 auto; padding: 48px 56px 72px; flex: 1; }
    .preview-pane { color: var(--text); }
    .preview-pane h1 { font-size: 1.7em; font-weight: 600; border-bottom: 1px solid var(--border); padding-bottom: 10px; margin: 0 0 14px; }
    .preview-pane h2 { font-size: 1.3em; font-weight: 600; margin: 24px 0 8px; }
    .preview-pane h3 { font-size: 1.08em; font-weight: 600; color: var(--muted); margin: 18px 0 6px; }
    .preview-pane p { margin: 0 0 12px; white-space: pre-wrap; }
    .preview-pane ul, .preview-pane ol { padding-left: 18px; margin: 0 0 12px; }
    .preview-pane li { margin: 4px 0; }
    .preview-pane code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.92em; background: rgba(127,127,127,0.14); padding: 1px 5px;  color: var(--accent); }
    .preview-pane pre { background: rgba(127,127,127,0.1); border: 1px solid var(--border);  padding: 14px 16px; margin: 12px 0; overflow-x: auto; }
    .preview-pane pre code { background: transparent; padding: 0; color: var(--muted); }
    .preview-pane blockquote { border-left: 2px solid var(--border); margin: 14px 0; padding: 3px 0 3px 14px; color: var(--muted); }
    .preview-pane a { color: var(--accent); text-decoration: none; }
    .preview-pane a:hover { text-decoration: underline; }
    .preview-pane img { max-width: 100%;  margin: 8px 0; }
    .preview-pane hr { border: none; border-top: 1px solid var(--border); margin: 20px 0; }
    .preview-pane table { border-collapse: collapse; width: 100%; margin: 12px 0; }
    .preview-pane th { padding: 6px 10px; border-bottom: 1px solid var(--border); color: var(--muted); text-align: left; font-weight: 600; }
    .preview-pane td { padding: 5px 10px; border-bottom: 1px solid var(--border); }
    .preview-pane .callout { margin: 16px 0; padding: 12px 14px 12px 16px; border-left: 2px solid var(--accent); background: rgba(127,127,127,0.08); }
    .preview-pane .callout__title { margin: 0 0 8px; font-size: 0.86em; font-weight: 700; letter-spacing: 0.08em; text-transform: uppercase; color: var(--muted); }
    .preview-pane .callout__body > :last-child { margin-bottom: 0; }
    .wikilink--broken { color: var(--muted); text-decoration: line-through; cursor: default; }
    @media (max-width: 768px) {
      .site-layout { flex-direction: column; }
      .site-nav { width: 100%; height: auto; position: static; border-right: none; border-bottom: 1px solid var(--border); }
      .page { padding: 24px 20px 48px; }
    }
  </style>
</head>
<body>
  <div class="site-layout">
    <nav class="site-nav">
      <div class="site-nav__title">${escapeHtml(fileName(state.folderPath) || 'Notes')}</div>
      ${nav}
    </nav>
    <main class="page">
      <article class="preview-pane">${bodyHtml}</article>
    </main>
  </div>
</body>
</html>`
}

function buildNavLinks(allFiles, currentBaseName) {
  return allFiles
    .map(f => {
      const base = stripMarkdownExtension(fileName(f.path))
      const activeClass = base === currentBaseName ? ' active' : ''
      return `<a class="site-nav__link${activeClass}" href="${encodeURIComponent(base)}.html">${escapeHtml(base)}</a>`
    })
    .join('\n      ')
}

function buildIndexHtml(allFiles, colors, settings) {
  const previewFont = settings.previewFontCustom || settings.previewFont || "'DM Sans', system-ui, sans-serif"
  const previewFontSize = settings.previewFontSize || 13
  const projectName = fileName(state.folderPath) || 'Notes'

  const listItems = allFiles
    .map(f => {
      const base = stripMarkdownExtension(fileName(f.path))
      return `<li><a href="${encodeURIComponent(base)}.html">${escapeHtml(base)}</a></li>`
    })
    .join('\n        ')

  const nav = buildNavLinks(allFiles, '')

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>${escapeHtml(projectName)}</title>
  <style>
    :root {
      --bg: ${colors.bg};
      --surface: ${colors.surface};
      --text: ${colors.text};
      --muted: ${colors.muted};
      --border: ${colors.border};
      --accent: ${colors.accent};
      --font: ${previewFont};
      --font-size: ${previewFontSize}px;
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; padding: 0; background: var(--bg); color: var(--text); }
    body { font-family: var(--font); font-size: var(--font-size); line-height: 1.75; }
    .site-layout { display: flex; min-height: 100vh; }
    .site-nav {
      width: 220px; flex-shrink: 0; padding: 24px 16px;
      background: var(--surface); border-right: 1px solid var(--border);
      overflow-y: auto; position: sticky; top: 0; height: 100vh;
    }
    .site-nav__title { font-size: 14px; font-weight: 600; margin: 0 0 16px; color: var(--text); }
    .site-nav__link {
      display: block; padding: 4px 8px; margin: 2px 0; 
      color: var(--muted); text-decoration: none; font-size: 12px;
    }
    .site-nav__link:hover { background: rgba(127,127,127,0.1); color: var(--text); }
    .page { max-width: 860px; margin: 0 auto; padding: 48px 56px 72px; flex: 1; }
    h1 { font-size: 1.7em; font-weight: 600; margin: 0 0 24px; }
    ul { padding-left: 18px; }
    li { margin: 6px 0; }
    a { color: var(--accent); text-decoration: none; }
    a:hover { text-decoration: underline; }
    @media (max-width: 768px) {
      .site-layout { flex-direction: column; }
      .site-nav { width: 100%; height: auto; position: static; border-right: none; border-bottom: 1px solid var(--border); }
      .page { padding: 24px 20px 48px; }
    }
  </style>
</head>
<body>
  <div class="site-layout">
    <nav class="site-nav">
      <div class="site-nav__title">${escapeHtml(projectName)}</div>
      ${nav}
    </nav>
    <main class="page">
      <h1>${escapeHtml(projectName)}</h1>
      <ul>
        ${listItems}
      </ul>
    </main>
  </div>
</body>
</html>`
}

export async function exportAsWebsite() {
  if (!state.folderPath) {
    showStatusNotice('Open a folder first', 'error')
    return
  }

  const outputDir = await window.fjord.pickExportFolder()
  if (!outputDir) return

  const index = getLinkIndex()
  const settings = getSettings()
  const colors = getThemeColors()

  // Collect all md files from the link index
  const allPaths = Array.from(index.allPaths).sort((a, b) => a.localeCompare(b))
  const allBaseNames = allPaths.map(p => stripMarkdownExtension(fileName(p)))
  const allFiles = allPaths.map(p => ({
    path: p,
    baseName: stripMarkdownExtension(fileName(p)),
    content: index.files.get(p)?.content || '',
  }))

  // Render each file to HTML
  const files = []
  for (const file of allFiles) {
    let html = await renderMarkdown(file.content, {
      hideFrontmatter: settings.hideFrontmatterInRenderedModes,
      showDocumentBanners: settings.showDocumentBanners,
      currentFilePath: file.path,
    })
    html = resolveWikilinksToHtml(html, allBaseNames)
    const nav = buildNavLinks(allFiles, file.baseName)
    const pageHtml = buildSitePageHtml({
      title: file.baseName,
      bodyHtml: html,
      nav,
      colors,
      settings,
    })
    files.push({ name: `${file.baseName}.html`, html: pageHtml })
  }

  // Build index page
  const indexHtml = buildIndexHtml(allFiles, colors, settings)
  files.push({ name: 'index.html', html: indexHtml })

  // Write all files via IPC
  const ok = await window.fjord.exportSite({ outputDir, files })
  if (ok) {
    showStatusNotice(`Exported ${files.length} pages to ${outputDir}`, 'success')
  } else {
    showStatusNotice('Export failed. Check permissions and try again.', 'error')
  }
}
