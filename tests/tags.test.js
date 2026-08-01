const assert = require('node:assert/strict')
const test = require('node:test')

// ── Inline the exact pure tag/frontmatter parser logic from tags.js ─
const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---/

function parseScalar(value) {
  const trimmed = String(value || '').trim()
  if (
    (trimmed.startsWith('"') && trimmed.endsWith('"')) ||
    (trimmed.startsWith("'") && trimmed.endsWith("'"))
  ) {
    return trimmed.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, '\\')
  }
  if (/^-?\d+(\.\d+)?$/.test(trimmed)) return Number(trimmed)
  if (/^(true|false)$/i.test(trimmed)) return trimmed.toLowerCase() === 'true'
  return trimmed
}

function parseFrontmatter(markdown) {
  if (!markdown || typeof markdown !== 'string') {
    return { frontmatter: null, body: markdown || '' }
  }
  const match = markdown.match(FRONTMATTER_RE)
  if (!match) return { frontmatter: null, body: markdown }

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
    const listItemMatch = trimmed.match(/^-\s+(.+)/)
    if (listItemMatch && inList) {
      listItems.push(parseScalar(listItemMatch[1].trim()))
      continue
    }
    const kvMatch = trimmed.match(/^([a-zA-Z0-9_-]+)\s*:\s*(.*)/)
    if (kvMatch) {
      flushList()
      const key = kvMatch[1]
      const value = kvMatch[2].trim()
      if (!value) {
        currentKey = key
        inList = true
        listItems = []
      } else if (value.startsWith('[') && value.endsWith(']')) {
        const inner = value.slice(1, -1)
        frontmatter[key] = inner.split(',').map(s => parseScalar(s.trim())).filter(Boolean)
      } else {
        frontmatter[key] = parseScalar(value)
      }
    } else if (inList && listItemMatch) {
      listItems.push(listItemMatch[1].trim())
    }
  }
  flushList()

  if (frontmatter.tags && typeof frontmatter.tags === 'string') {
    frontmatter.tags = frontmatter.tags.split(',').map(s => s.trim()).filter(Boolean)
  }

  return { frontmatter, body }
}

const INLINE_TAG_RE = /(?:^|\s)#([a-zA-Z0-9_/-]+)/g

function extractTags(markdown) {
  if (!markdown || typeof markdown !== 'string') return []
  const tags = new Set()
  const { frontmatter } = parseFrontmatter(markdown)
  if (frontmatter?.tags) {
    for (const tag of frontmatter.tags) {
      const normalized = String(tag).trim().toLowerCase()
      if (normalized) tags.add(normalized)
    }
  }
  const body = parseFrontmatter(markdown).body
  let m
  while ((m = INLINE_TAG_RE.exec(body)) !== null) {
    tags.add(m[1].toLowerCase())
  }
  return [...tags].sort()
}

// ── Tests ────────────────────────────────────────────────────────
test('parseFrontmatter returns null for no frontmatter', () => {
  const result = parseFrontmatter('Just text\n\n# Heading')
  assert.equal(result.frontmatter, null)
  assert.equal(result.body, 'Just text\n\n# Heading')
})

test('parseFrontmatter returns null for empty input', () => {
  const result = parseFrontmatter('')
  assert.equal(result.frontmatter, null)
  assert.equal(result.body, '')
})

test('parseFrontmatter returns null for null input', () => {
  const result = parseFrontmatter(null)
  assert.equal(result.frontmatter, null)
  assert.equal(result.body, '')
})

test('parseFrontmatter parses basic frontmatter', () => {
  const result = parseFrontmatter('---\ntitle: Hello\ntags: test\n---\n\n# Content')
  assert.ok(result.frontmatter, 'frontmatter parsed')
  assert.equal(result.frontmatter.title, 'Hello')
  // tags as string gets split into array by comma
  assert.deepEqual(result.frontmatter.tags, ['test'])
})

test('parseFrontmatter parses multi-value tags (list syntax)', () => {
  const result = parseFrontmatter('---\ntags:\n  - one\n  - two\n---\n\nBody')
  assert.ok(result.frontmatter)
  assert.deepEqual(result.frontmatter.tags, ['one', 'two'])
})

test('parseFrontmatter handles inline array tags', () => {
  const result = parseFrontmatter('---\ntags: [a, b, c]\n---\n\nBody')
  assert.ok(result.frontmatter)
  assert.deepEqual(result.frontmatter.tags, ['a', 'b', 'c'])
})

test('parseFrontmatter strips body of frontmatter', () => {
  const result = parseFrontmatter('---\nkey: val\n---\n\n# Real content\n\nParagraph.')
  assert.equal(result.body.trim(), '# Real content\n\nParagraph.')
})

test('parseFrontmatter parses numeric and boolean values', () => {
  const result = parseFrontmatter('---\ncount: 42\nactive: true\n---\n\nBody')
  assert.equal(result.frontmatter.count, 42)
  assert.equal(result.frontmatter.active, true)
})

test('extractTags extracts inline tags', () => {
  const tags = extractTags('# Hello\n\nThis is a #test and also #another-tag')
  assert.ok(tags.includes('test'))
  assert.ok(tags.includes('another-tag'))
})

test('extractTags extracts frontmatter tags', () => {
  const tags = extractTags('---\ntags:\n  - project-x\n  - urgent\n---\n\n# Heading')
  assert.ok(tags.includes('project-x'))
  assert.ok(tags.includes('urgent'))
})

test('extractTags returns empty for no tags', () => {
  const tags = extractTags('# Just a heading\n\nPlain paragraph.')
  assert.deepEqual(tags, [])
})

test('extractTags returns empty for empty string', () => {
  const tags = extractTags('')
  assert.deepEqual(tags, [])
})

test('extractTags deduplicates tags', () => {
  const tags = extractTags('#test and #test again with #test')
  const count = tags.filter(t => t === 'test').length
  assert.equal(count, 1, 'deduplicated')
})

test('parseFrontmatter supports quoted values', () => {
  const result = parseFrontmatter('---\ntitle: "Hello World"\n---\n\nBody')
  assert.equal(result.frontmatter.title, 'Hello World')
})
