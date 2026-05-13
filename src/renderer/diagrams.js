import mermaid from 'mermaid'

// ── Cache ─────────────────────────────────────────────────────────
const cache = new Map()
let mermaidIdCounter = 0

function cacheKey(lang, source, theme) {
  return `${lang}:${theme}:${source}`
}

// ── Mermaid init ──────────────────────────────────────────────────
function getMermaidThemeConfig(theme) {
  const isDark = theme === 'dark'
  return {
    startOnLoad: false,
    theme: isDark ? 'dark' : 'default',
    themeVariables: isDark
      ? {
          primaryColor: '#1c1d20',
          primaryTextColor: '#dddfe6',
          primaryBorderColor: '#2e3033',
          lineColor: '#484b57',
          secondaryColor: '#161719',
          tertiaryColor: '#111214',
          noteBkgColor: '#1c1d20',
          noteTextColor: '#7a7d8a',
          fontFamily: "'DM Sans', system-ui, sans-serif",
        }
      : {
          primaryColor: '#d8d1c7',
          primaryTextColor: '#221d18',
          primaryBorderColor: '#b8aea0',
          lineColor: '#5f564b',
          secondaryColor: '#e2ddd4',
          tertiaryColor: '#e9e6df',
          noteBkgColor: '#e2ddd4',
          noteTextColor: '#5f564b',
          fontFamily: "'DM Sans', system-ui, sans-serif",
        },
  }
}

let currentTheme = 'dark'

export function initDiagrams(theme = 'dark') {
  currentTheme = theme
  mermaid.initialize(getMermaidThemeConfig(theme))
}

export function clearDiagramCache() {
  cache.clear()
}

// ── D2 theme mapping ──────────────────────────────────────────────
function getD2ThemeId(theme) {
  return theme === 'dark' ? 200 : 0
}

// ── Render a single Mermaid block ─────────────────────────────────
async function renderMermaidBlock(source) {
  const id = `fjord-mermaid-${mermaidIdCounter++}`
  try {
    const { svg } = await mermaid.render(id, source)
    return { svg }
  } catch (err) {
    // mermaid may insert a broken element into the DOM; clean up
    document.getElementById(id)?.remove()
    const msg = err?.message || err?.str || String(err)
    return { error: msg }
  }
}

// ── Render a single D2 block ──────────────────────────────────────
async function renderD2Block(source, theme) {
  if (!window.fjord?.renderD2) {
    return { error: 'D2 rendering is not available' }
  }
  const themeId = getD2ThemeId(theme)
  return window.fjord.renderD2(source, themeId)
}

// ── Build result DOM ──────────────────────────────────────────────
function buildDiagramElement(lang, result) {
  const wrapper = document.createElement('div')
  wrapper.className = `diagram diagram--${lang}`

  if (result.error) {
    wrapper.className = 'diagram diagram--error'
    wrapper.textContent = result.error
    return wrapper
  }

  wrapper.innerHTML = result.svg
  return wrapper
}

// ── Main: process all diagram blocks in a preview element ─────────
export async function processDiagrams(previewElement, theme) {
  if (!previewElement) return

  currentTheme = theme || currentTheme
  const blocks = previewElement.querySelectorAll(
    'pre > code[class*="language-d2"], pre > code[class*="language-mermaid"]'
  )

  const promises = Array.from(blocks).map(async (codeEl) => {
    const preEl = codeEl.parentElement
    if (!preEl || preEl.tagName !== 'PRE') return

    const classes = codeEl.className
    const lang = classes.includes('language-d2') ? 'd2'
      : classes.includes('language-mermaid') ? 'mermaid'
      : null
    if (!lang) return

    const source = codeEl.textContent.trim()
    if (!source) return

    const key = cacheKey(lang, source, currentTheme)
    let result = cache.get(key)

    if (!result) {
      result = lang === 'mermaid'
        ? await renderMermaidBlock(source)
        : await renderD2Block(source, currentTheme)
      cache.set(key, result)
    }

    const diagramEl = buildDiagramElement(lang, result)
    preEl.replaceWith(diagramEl)
  })

  await Promise.all(promises)
}
