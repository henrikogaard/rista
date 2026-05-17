// ── Tags & Frontmatter Parser ───────────────────────────────────
// Parses YAML frontmatter tags and inline #tag patterns from markdown.
// No external YAML library — manual parsing only.

const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---/

// Inline tag: must be preceded by start-of-line or whitespace, must not
// be a heading (## ) or hex color (#fff).  Captures the tag name.
const INLINE_TAG_RE = /(?:^|\s)#([a-zA-Z0-9_/-]+)/g

/**
 * Parse YAML frontmatter from a markdown string.
 * Returns { frontmatter: object|null, body: string }.
 * Only the `tags` field is parsed into an array; other keys are stored as strings.
 */
export function parseFrontmatter(markdown) {
  if (!markdown || typeof markdown !== 'string') {
    return { frontmatter: null, body: markdown || '' }
  }

  const match = markdown.match(FRONTMATTER_RE)
  if (!match) {
    return { frontmatter: null, body: markdown }
  }

  const raw = match[1]
  const body = markdown.slice(match[0].length).replace(/^\r?\n/, '')
  const frontmatter = {}

  const lines = raw.split('\n')
  let currentKey = null
  let listItems = []
  let inList = false

  const flushList = () => {
    if (currentKey && inList) {
      frontmatter[currentKey] = listItems
      listItems = []
      inList = false
      currentKey = null
    }
  }

  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed) continue

    // List item under a key (  - value)
    const listItemMatch = trimmed.match(/^-\s+(.+)/)
    if (listItemMatch && inList) {
      listItems.push(listItemMatch[1].trim())
      continue
    }

    // New key: value pair
    const kvMatch = trimmed.match(/^([a-zA-Z0-9_-]+)\s*:\s*(.*)/)
    if (kvMatch) {
      flushList()
      const key = kvMatch[1]
      const value = kvMatch[2].trim()

      if (!value) {
        // Value might be a YAML list on subsequent lines
        currentKey = key
        inList = true
        listItems = []
      } else if (value.startsWith('[') && value.endsWith(']')) {
        // Inline array: tags: [a, b, c]
        const inner = value.slice(1, -1)
        frontmatter[key] = inner
          .split(',')
          .map(s => s.trim())
          .filter(Boolean)
      } else {
        frontmatter[key] = value
      }
    } else if (inList && listItemMatch) {
      listItems.push(listItemMatch[1].trim())
    }
  }

  flushList()

  // Normalize tags to always be an array
  if (frontmatter.tags && typeof frontmatter.tags === 'string') {
    frontmatter.tags = frontmatter.tags
      .split(',')
      .map(s => s.trim())
      .filter(Boolean)
  }

  return { frontmatter, body }
}

/**
 * Extract inline #tag patterns from markdown body text.
 * Avoids matching headings (## Heading) and hex colors (#fff, #a1b2c3).
 */
export function parseInlineTags(markdown) {
  if (!markdown || typeof markdown !== 'string') return []

  const tags = new Set()
  const lines = markdown.split('\n')

  for (const line of lines) {
    // Skip heading lines
    if (/^#{1,6}\s/.test(line)) continue
    // Skip code fences
    if (/^```/.test(line.trim())) continue

    let m
    INLINE_TAG_RE.lastIndex = 0
    while ((m = INLINE_TAG_RE.exec(line)) !== null) {
      const tag = m[1]
      // Skip hex color patterns (1-8 hex chars only)
      if (/^[0-9a-fA-F]{1,8}$/.test(tag)) continue
      tags.add(tag)
    }
  }

  return Array.from(tags)
}

/**
 * Combine frontmatter tags and inline tags into a deduplicated, sorted array.
 */
export function getAllTags(markdown) {
  const { frontmatter, body } = parseFrontmatter(markdown)
  const fmTags = (frontmatter?.tags && Array.isArray(frontmatter.tags))
    ? frontmatter.tags
    : []
  const inlineTags = parseInlineTags(body)

  const all = new Set()
  for (const t of fmTags) all.add(t.toLowerCase())
  for (const t of inlineTags) all.add(t.toLowerCase())

  return Array.from(all).sort()
}
