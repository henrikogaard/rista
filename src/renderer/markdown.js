import { escapeHtml, fileName } from './state.js'
import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import remarkEmoji from 'remark-emoji'
import remarkRehype from 'remark-rehype'
import rehypeKatex from 'rehype-katex'
import rehypeHighlight from 'rehype-highlight'
import rehypeStringify from 'rehype-stringify'

// ── Wikilink remark plugin ───────────────────────────────────────
function remarkWikilinks() {
  return (tree) => {
    const visit = (node) => {
      if (node.type === 'text' && node.value) {
        const parts = []
        const regex = /\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g
        let lastIndex = 0
        let match
        while ((match = regex.exec(node.value)) !== null) {
          if (match.index > lastIndex) {
            parts.push({ type: 'text', value: node.value.slice(lastIndex, match.index) })
          }
          const linkText = (match[2] || match[1]).trim()
          const target = match[1].trim()
          parts.push({
            type: 'link',
            url: '#',
            data: {
              hProperties: {
                class: 'wikilink',
                'data-wikilink': target,
              },
            },
            children: [{ type: 'text', value: linkText }],
          })
          lastIndex = match.index + match[0].length
        }
        if (lastIndex < node.value.length) {
          parts.push({ type: 'text', value: node.value.slice(lastIndex) })
        }
        if (parts.length > 0) {
          node.type = 'paragraph'
          node.children = parts
          delete node.value
        }
      }
      if (node.children) {
        for (let i = 0; i < node.children.length; i++) {
          const child = node.children[i]
          if (child.type === 'text') {
            const parts = []
            const regex = /\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g
            let lastIndex = 0
            let match
            while ((match = regex.exec(child.value)) !== null) {
              if (match.index > lastIndex) {
                parts.push({ type: 'text', value: child.value.slice(lastIndex, match.index) })
              }
              const linkText = (match[2] || match[1]).trim()
              const target = match[1].trim()
              parts.push({
                type: 'link',
                url: '#',
                data: {
                  hProperties: {
                    class: 'wikilink',
                    'data-wikilink': target,
                  },
                },
                children: [{ type: 'text', value: linkText }],
              })
              lastIndex = match.index + match[0].length
            }
            if (lastIndex < child.value.length) {
              parts.push({ type: 'text', value: child.value.slice(lastIndex) })
            }
            if (parts.length > 0) {
              node.children.splice(i, 1, ...parts)
              i += parts.length - 1
            }
          } else {
            visit(child)
          }
        }
      }
    }
    visit(tree)
  }
}

const processor = unified()
  .use(remarkParse)
  .use(remarkWikilinks)
  .use(remarkGfm)
  .use(remarkMath)
  .use(remarkEmoji)
  .use(remarkRehype, { allowDangerousHtml: true })
  .use(rehypeKatex)
  .use(rehypeHighlight, { detect: false, ignoreMissing: true })
  .use(rehypeStringify, { allowDangerousHtml: true })

let _transclusionResolver = null
const SPATIAL_DRAWING_EXT = '.fdraw.json'
const SPATIAL_CANVAS_EXT = '.fcanvas.json'
const OBSIDIAN_IMAGE_EXT_RE = /\.(avif|gif|jpe?g|png|svg|webp)$/i
const OBSIDIAN_ATTACHMENT_EXT_RE = /\.(csv|docx?|pdf|pptx?|txt|xlsx?|zip)$/i

export function setTransclusionResolver(resolver) {
  _transclusionResolver = resolver
}

function resolveTransclusions(markdown, depth = 0) {
  if (depth >= 3 || !_transclusionResolver) return markdown
  return markdown.replace(/^!\[\[([^\]|]+)(?:\|[^\]]+)?\]\]$/gm, (match, noteName) => {
    const content = _transclusionResolver(noteName.trim())
    if (content == null) return match
    const resolved = resolveTransclusions(content, depth + 1)
    const safeName = escapeHtml(noteName.trim())
    return `<div class="transclusion"><div class="transclusion__source">↗ ${safeName}</div>\n\n${resolved}\n\n</div>`
  })
}

function resolveSpatialEmbeds(markdown) {
  return String(markdown || '').replace(/^!\[\[([^\]|]+?\.(?:fdraw|fcanvas)\.json)(?:\|([^\]]+))?\]\]$/gmi, (match, target, label) => {
    const cleanTarget = target.trim()
    const type = cleanTarget.toLowerCase().endsWith(SPATIAL_DRAWING_EXT) ? 'Drawing' : 'Canvas'
    const title = (label || fileName(cleanTarget)).trim()
    return `
<a class="spatial-embed" href="${escapeHtml(cleanTarget)}">
  <span class="spatial-embed__type">${type}</span>
  <span class="spatial-embed__title">${escapeHtml(title)}</span>
  <span class="spatial-embed__hint">Open ${type.toLowerCase()}</span>
</a>`
  })
}

function resolveObsidianEmbeds(markdown, options = {}) {
  return String(markdown || '').replace(/!\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (match, target, meta) => {
    const cleanTarget = target.trim()
    if (OBSIDIAN_IMAGE_EXT_RE.test(cleanTarget)) {
      return renderObsidianImageEmbed(cleanTarget, meta, options)
    }
    if (OBSIDIAN_ATTACHMENT_EXT_RE.test(cleanTarget)) {
      return renderObsidianAttachmentEmbed(cleanTarget, meta, options)
    }
    return match
  })
}

function renderObsidianImageEmbed(target, meta, options) {
  const embed = parseObsidianEmbedMeta(target, meta)
  const src = resolveBannerSrc(target, options.currentFilePath, options)
  return `<img class="obsidian-embed obsidian-embed--image" src="${escapeHtml(src)}" alt="${escapeHtml(embed.label)}">`
}

function renderObsidianAttachmentEmbed(target, meta, options) {
  const embed = parseObsidianEmbedMeta(target, meta)
  const href = resolveBannerSrc(target, options.currentFilePath, options)
  return `<a class="obsidian-embed obsidian-embed--attachment" href="${escapeHtml(href)}">${escapeHtml(embed.label)}</a>`
}

function parseObsidianEmbedMeta(target, meta) {
  const fallback = fileName(target)
  const value = String(meta || '').trim()
  if (!value || /^\d+(?:x\d+)?$/i.test(value)) {
    return { label: fallback }
  }
  return { label: value }
}

function resolveBlockReferences(markdown) {
  return String(markdown || '')
    .split('\n')
    .map(line => line.replace(/(^|[^\S\r\n])\^([A-Za-z0-9][A-Za-z0-9-]*)\s*$/, (match, prefix, id) => {
      return `${prefix}<span class="block-ref-anchor" id="${escapeHtml(`^${id}`)}" data-block-ref="${escapeHtml(id)}"></span>`
    }))
    .join('\n')
}

export async function renderMarkdown(markdown, options = {}) {
  const renderable = getRenderableMarkdown(markdown, options)
  const compatible = resolveBlockReferences(resolveObsidianEmbeds(resolveSpatialEmbeds(renderable.body), options))
  const expanded = resolveTransclusions(compatible)
  const chunks = splitMarkdownIntoRenderChunks(expanded)
  const rendered = await Promise.all(chunks.map(renderChunk))
  const banner = options.showDocumentBanners === false ? '' : renderDocumentBanner(renderable.frontmatter, options)
  return `${banner}${rendered.join('')}`
}

export function getRenderableMarkdown(markdown, options = {}) {
  const parsed = parseFrontmatterBlock(markdown)
  if (!options.hideFrontmatter) return { ...parsed, body: markdown || '' }
  return parsed
}

export function mergeFrontmatterWithBody(originalMarkdown, bodyMarkdown) {
  const parsed = parseFrontmatterBlock(originalMarkdown)
  if (!parsed.raw) return bodyMarkdown || ''
  const body = String(bodyMarkdown || '').replace(/^\n+/, '')
  return `${parsed.raw}${body}`
}

export function parseFrontmatterBlock(markdown = '') {
  const source = String(markdown || '')
  const normalized = source.replace(/^\uFEFF/, '')
  const firstLine = normalized.match(/^([^\n\r]*)(?:\r?\n|$)/)?.[1] || ''
  if (!/^(---|\*\*\*)\s*$/.test(firstLine)) {
    return { frontmatter: null, raw: '', body: source }
  }

  const lines = normalized.split(/\r?\n/)
  const bodyLines = []
  let closingIndex = -1
  for (let index = 1; index < lines.length; index += 1) {
    if (/^(---|\*\*\*)\s*$/.test(lines[index])) {
      closingIndex = index
      break
    }
    bodyLines.push(lines[index])
  }

  if (closingIndex < 0 || !bodyLines.some(line => /^\s*[\w.-]+\s*:/.test(line))) {
    return { frontmatter: null, raw: '', body: source }
  }

  const raw = `${lines.slice(0, closingIndex + 1).join('\n')}\n`
  return {
    frontmatter: parseFrontmatterProperties(bodyLines),
    raw,
    body: lines.slice(closingIndex + 1).join('\n').replace(/^\n+/, ''),
  }
}

function parseFrontmatterProperties(lines) {
  const frontmatter = {}
  for (const line of lines) {
    const match = line.match(/^\s*([\w.-]+)\s*:\s*(.*?)\s*$/)
    if (!match) continue
    frontmatter[match[1]] = parseFrontmatterValue(match[2])
  }
  return frontmatter
}

function parseFrontmatterValue(value) {
  const trimmed = String(value || '').trim()
  if (
    (trimmed.startsWith('"') && trimmed.endsWith('"')) ||
    (trimmed.startsWith("'") && trimmed.endsWith("'"))
  ) {
    return trimmed.slice(1, -1)
  }
  if (/^-?\d+(?:\.\d+)?$/.test(trimmed)) return Number(trimmed)
  if (trimmed === 'true') return true
  if (trimmed === 'false') return false
  return trimmed
}

export function renderDocumentBanner(frontmatter, options = {}) {
  const banner = getFrontmatterValue(frontmatter, 'banner')
  const src = resolveBannerSrc(banner, options.currentFilePath, options)
  if (!src) return ''

  const x = normalizeBannerAxis(getFrontmatterValue(frontmatter, 'banner_x') ?? getFrontmatterValue(frontmatter, 'banner-x'), 0.5)
  const y = normalizeBannerAxis(getFrontmatterValue(frontmatter, 'banner_y') ?? getFrontmatterValue(frontmatter, 'banner-y'), 0.5)
  const position = `${Math.round(x * 100)}% ${Math.round(y * 100)}%`
  return `<figure class="document-banner"><img class="document-banner__image" src="${escapeHtml(src)}" alt="" style="object-position:${position};"></figure>`
}

function getFrontmatterValue(frontmatter, key) {
  if (!frontmatter || typeof frontmatter !== 'object') return null
  return Object.prototype.hasOwnProperty.call(frontmatter, key) ? frontmatter[key] : null
}

function normalizeBannerAxis(value, fallback) {
  const next = Number(value)
  if (!Number.isFinite(next)) return fallback
  return Math.min(1, Math.max(0, next))
}

function resolveBannerSrc(value, currentFilePath, options = {}) {
  const raw = String(value || '').trim()
  if (!raw) return ''
  const wikilink = raw.match(/^!?\[\[([^\]|]+)(?:\|[^\]]+)?\]\]$/)
  const markdownImage = raw.match(/^!\[[^\]]*\]\(([^)]+)\)$/)
  const target = (wikilink?.[1] || markdownImage?.[1] || raw).split('#')[0].trim()
  if (!target) return ''
  if (/^(https?:|data:|blob:|file:)/i.test(target)) return target
  if (target.startsWith('/')) return toFileUrl(target)

  const resolvedAttachment = resolveWorkspaceAttachment(target, options)
  if (resolvedAttachment) return toFileUrl(resolvedAttachment)

  if (!currentFilePath) return target
  const base = currentFilePath.slice(0, Math.max(currentFilePath.lastIndexOf('/'), currentFilePath.lastIndexOf('\\')))
  return toFileUrl(normalizeLocalPath(`${base}/${target}`))
}

function resolveWorkspaceAttachment(target, options = {}) {
  const attachmentPaths = Array.isArray(options.attachmentPaths) ? options.attachmentPaths : []
  if (!attachmentPaths.length) return ''
  const cleanTarget = normalizeLocalPath(String(target || '').replace(/\\/g, '/')).replace(/^\/+/, '')
  const targetLower = cleanTarget.toLowerCase()
  const basenameLower = fileName(cleanTarget).toLowerCase()
  const folderPath = normalizeLocalPath(String(options.folderPath || '')).replace(/\/+$/, '')

  const exact = attachmentPaths.find(path => {
    const normalized = normalizeLocalPath(String(path || '').replace(/\\/g, '/'))
    const relative = folderPath && normalized.startsWith(`${folderPath}/`)
      ? normalized.slice(folderPath.length + 1)
      : normalized.replace(/^\/+/, '')
    return relative.toLowerCase() === targetLower
  })
  if (exact) return normalizeLocalPath(exact)

  const byName = attachmentPaths.find(path => {
    const name = fileName(path).toLowerCase()
    return name === basenameLower
  })
  return byName ? normalizeLocalPath(byName) : ''
}

function normalizeLocalPath(path) {
  const parts = path.split(/[\\/]/)
  const out = []
  for (const part of parts) {
    if (!part || part === '.') continue
    if (part === '..') out.pop()
    else out.push(part)
  }
  return `${path.startsWith('/') ? '/' : ''}${out.join('/')}`
}

function toFileUrl(path) {
  return `file://${encodeURI(path)}`
}

export function htmlToMarkdown(html) {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, 'text/html')
  return serializeBlockChildren(doc.body).replace(/\n{3,}/g, '\n\n').trim()
}

function serializeBlockChildren(parent) {
  const parts = []
  for (const node of parent.childNodes) {
    const value = serializeNode(node).trim()
    if (value) parts.push(value)
  }
  return parts.join('\n\n')
}

function serializeInlineChildren(parent) {
  return Array.from(parent.childNodes).map(node => serializeInlineNode(node)).join('')
}

function serializeNode(node) {
  if (node.nodeType === Node.TEXT_NODE) return normalizeText(node.textContent)
  if (node.nodeType !== Node.ELEMENT_NODE) return ''

  const tag = node.tagName.toLowerCase()
  switch (tag) {
    case 'h1': return `# ${serializeInlineChildren(node).trim()}`
    case 'h2': return `## ${serializeInlineChildren(node).trim()}`
    case 'h3': return `### ${serializeInlineChildren(node).trim()}`
    case 'h4': return `#### ${serializeInlineChildren(node).trim()}`
    case 'h5': return `##### ${serializeInlineChildren(node).trim()}`
    case 'h6': return `###### ${serializeInlineChildren(node).trim()}`
    case 'p': return serializeInlineChildren(node).trim()
    case 'div': {
      const hasBlockChildren = Array.from(node.children).some(child => isBlockTag(child.tagName.toLowerCase()))
      return hasBlockChildren ? serializeBlockChildren(node) : serializeInlineChildren(node).trim()
    }
    case 'ul':
      return serializeList(node, false)
    case 'ol':
      return serializeList(node, true)
    case 'pre': {
      const codeEl = node.querySelector('code')
      const language = codeEl?.getAttribute('class')?.match(/language-(\S+)/)
      const langAttr = language ? language[1] : ''
      const code = (codeEl?.textContent || node.textContent || '').replace(/\n+$/, '') || ''
      return '```' + langAttr + '\n' + code + '\n' + '```'
    }
    case 'blockquote': {
      const content = serializeBlockChildren(node) || serializeInlineChildren(node)
      return content
        .split('\n')
        .map(line => line.trim() ? `> ${line}` : '>')
        .join('\n')
    }
    case 'table':
      return serializeTable(node)
    case 'hr':
      return '---'
    case 'img':
      return `![${node.getAttribute('alt') || ''}](${node.getAttribute('src') || ''})`
    default:
      return serializeInlineChildren(node).trim()
  }
}

function serializeInlineNode(node) {
  if (node.nodeType === Node.TEXT_NODE) return normalizeText(node.textContent)
  if (node.nodeType !== Node.ELEMENT_NODE) return ''

  const tag = node.tagName.toLowerCase()
  switch (tag) {
    case 'br':
      return '\n'
    case 'strong':
    case 'b':
      return `**${serializeInlineChildren(node)}**`
    case 'em':
    case 'i':
      return `*${serializeInlineChildren(node)}*`
    case 'code':
      return `\`${serializeInlineChildren(node)}\``
    case 'a': {
      const text = serializeInlineChildren(node).trim()
      const href = node.getAttribute('href') || ''
      return href ? `[${text || href}](${href})` : text
    }
    case 'img':
      return `![${node.getAttribute('alt') || ''}](${node.getAttribute('src') || ''})`
    case 'span':
    case 'mark':
    case 'u':
    case 's':
    case 'del':
      return serializeInlineChildren(node)
    case 'div':
      return `${serializeInlineChildren(node).trim()}\n`
    default:
      return serializeInlineChildren(node)
  }
}

function serializeList(node, ordered) {
  const items = Array.from(node.children)
    .filter(child => child.tagName?.toLowerCase() === 'li')
    .map((item, index) => {
      const marker = ordered ? `${index + 1}. ` : '- '
      const checkbox = item.querySelector(':scope > input[type="checkbox"]')
      const clonedItem = item.cloneNode(true)
      clonedItem.querySelectorAll('input[type="checkbox"]').forEach(input => input.remove())
      const content = serializeInlineChildren(clonedItem).trim() || serializeBlockChildren(clonedItem)
      if (checkbox) {
        const taskMarker = checkbox.checked ? '- [x] ' : '- [ ] '
        return `${taskMarker}${content.trim()}`
      }
      return `${marker}${content.trim()}`
    })
  return items.join('\n')
}

function normalizeText(text = '') {
  return text.replace(/\u00a0/g, ' ')
}

function serializeTable(node) {
  const rows = Array.from(node.querySelectorAll('tr'))
  if (!rows.length) return ''

  const serializedRows = rows.map(row =>
    Array.from(row.children)
      .filter(cell => ['th', 'td'].includes(cell.tagName.toLowerCase()))
      .map(cell => serializeInlineChildren(cell).replace(/\n+/g, ' ').trim() || ' ')
  ).filter(row => row.length)

  if (!serializedRows.length) return ''

  const header = serializedRows[0]
  const divider = header.map(() => '---')
  const body = serializedRows.slice(1)

  return [
    `| ${header.join(' | ')} |`,
    `| ${divider.join(' | ')} |`,
    ...body.map(row => `| ${row.join(' | ')} |`),
  ].join('\n')
}

function isBlockTag(tag) {
  return ['p', 'div', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'ul', 'ol', 'pre', 'blockquote', 'hr', 'table'].includes(tag)
}

function splitMarkdownIntoRenderChunks(markdown) {
  const lines = markdown.split('\n')
  const chunks = []
  let buffer = []

  const flushBuffer = () => {
    if (!buffer.length) return
    chunks.push({ type: 'markdown', value: buffer.join('\n') })
    buffer = []
  }

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index]
    const match = line.match(/^>\s*\[!([A-Za-z0-9_-]+)\]([+-])?(?:\s+(.*))?$/)
    if (!match) {
      buffer.push(line)
      continue
    }

    flushBuffer()

    const bodyLines = []
    index += 1
    while (index < lines.length && /^> ?/.test(lines[index])) {
      bodyLines.push(lines[index].replace(/^> ?/, ''))
      index += 1
    }
    index -= 1

    chunks.push({
      type: 'callout',
      calloutType: match[1].toLowerCase(),
      fold: match[2] === '-' ? 'closed' : match[2] === '+' ? 'open' : '',
      title: (match[3] || '').trim(),
      body: bodyLines.join('\n').trim(),
    })
  }

  flushBuffer()
  return chunks
}

async function renderChunk(chunk) {
  if (chunk.type === 'callout') return renderCallout(chunk)
  const result = await processor.process(chunk.value)
  return String(result)
}

async function renderCallout({ calloutType, title, body, fold }) {
  const renderedBody = body
    ? String(await processor.process(body))
    : '<p></p>'

  const safeType = sanitizeCalloutType(calloutType)
  const displayTitle = escapeHtml(title || titleizeCalloutType(safeType))
  const foldAttr = fold ? ` data-callout-fold="${fold}"` : ''

  return `
    <div class="callout callout--${safeType}" data-callout="${safeType}"${foldAttr}>
      <div class="callout__title">${displayTitle}</div>
      <div class="callout__body">${renderedBody}</div>
    </div>
  `
}

function sanitizeCalloutType(type = '') {
  return type.toLowerCase().replace(/[^a-z0-9_-]/g, '') || 'note'
}

function titleizeCalloutType(type = '') {
  return type
    .replace(/[-_]+/g, ' ')
    .replace(/\b\w/g, letter => letter.toUpperCase()) || 'Note'
}



// Extract headings for ToC. Skips fenced code blocks. Returns line number
// (1-indexed) so consumers can jump the editor to the heading.
export function extractHeadings(markdown) {
  const headings = []
  const lines = markdown.split('\n')
  let inFence = false
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    if (/^\s*```/.test(line)) { inFence = !inFence; continue }
    if (inFence) continue
    const m = line.match(/^(#{1,6})\s+(.+)/)
    if (m) {
      headings.push({ level: m[1].length, text: m[2].trim(), line: i + 1 })
    }
  }
  return headings
}

// Word / char / paragraph count
export function getStats(markdown, readingSpeed = 200) {
  const text = markdown.replace(/```[\s\S]*?```/g, '').replace(/`[^`]+`/g, '').replace(/[#*_~\[\]]/g, '')
  const words = text.trim() ? text.trim().split(/\s+/).length : 0
  const chars = text.replace(/\s/g, '').length
  const paragraphs = markdown.split(/\n\n+/).filter(p => p.trim()).length
  const wpm = Math.max(50, Math.min(1000, readingSpeed || 200))
  const readMin = Math.ceil(words / wpm) || 0

  // Sentence detection (basic: split on . ! ? followed by space or end)
  const sentences = text.split(/[.!?]+(?:\s|$)/).filter(s => s.trim()).length
  const avgSentenceLen = sentences > 0 ? Math.round(words / sentences * 10) / 10 : 0
  const avgWordLen = words > 0 ? Math.round(chars / words * 10) / 10 : 0

  // Flesch-Kincaid Grade Level (approximate)
  const syllables = countSyllables(text)
  const fkGrade = words > 0 && sentences > 0
    ? Math.round((0.39 * (words / sentences) + 11.8 * (syllables / words) - 15.59) * 10) / 10
    : 0

  return { words, chars, paragraphs, readMin, sentences, avgSentenceLen, avgWordLen, fkGrade }
}

function countSyllables(text) {
  const words = text.toLowerCase().split(/\s+/).filter(Boolean)
  let total = 0
  for (const word of words) {
    const cleaned = word.replace(/[^a-z]/g, '')
    if (!cleaned) continue
    let count = cleaned.replace(/(?:[^laeiouy]es|ed|[^laeiouy]e)$/, '').match(/[aeiouy]{1,2}/g)
    total += (count ? count.length : 1)
  }
  return total
}
