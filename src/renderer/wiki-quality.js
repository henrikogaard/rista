const DEFAULT_LIMIT = 8
const CAPITALIZED_PHRASE_RE = /\b[A-Z][A-Za-z0-9]+(?:[ -]+[A-Z][A-Za-z0-9]+){1,3}\b/g

const TERM_STOPWORDS = new Set([
  'Daily Note',
  'Missing Note',
  'New File',
  'Open Folder',
  'Table Of',
])

export function analyzeWikiQuality(index, options = {}) {
  const folderPath = options.folderPath || ''
  const limit = Number.isFinite(options.limit) ? options.limit : DEFAULT_LIMIT
  const resolveLink = typeof options.resolveLink === 'function' ? options.resolveLink : null
  const files = index?.files instanceof Map ? index.files : new Map()
  const allPaths = index?.allPaths instanceof Set ? index.allPaths : new Set(files.keys())
  const backlinks = index?.backlinks instanceof Map ? index.backlinks : new Map()

  const unresolvedLinks = collectUnresolvedLinks(files, allPaths, folderPath, resolveLink)
  const orphanNotes = collectOrphanNotes(files, backlinks, folderPath)
  const duplicateTitles = collectDuplicateTitles(allPaths, folderPath)
  const glossaryCandidates = collectGlossaryCandidates(files, folderPath)

  return {
    summary: {
      unresolvedLinks: unresolvedLinks.length,
      orphanNotes: orphanNotes.length,
      duplicateTitles: duplicateTitles.length,
      glossaryCandidates: glossaryCandidates.length,
      stale: Boolean(index?.dirty),
    },
    unresolvedLinks: unresolvedLinks.slice(0, limit),
    orphanNotes: orphanNotes.slice(0, limit),
    duplicateTitles: duplicateTitles.slice(0, limit),
    glossaryCandidates: glossaryCandidates.slice(0, limit),
  }
}

function collectUnresolvedLinks(files, allPaths, folderPath, resolveLink) {
  const results = []
  for (const [sourcePath, entry] of files) {
    const links = entry?.links instanceof Set ? Array.from(entry.links) : []
    for (const linkText of links) {
      const targetPath = safelyResolve(resolveLink, linkText, allPaths, folderPath)
      if (targetPath) continue
      results.push({
        linkText,
        sourcePath,
        sourceName: noteTitle(sourcePath),
        sourceRelativePath: relativePath(sourcePath, folderPath),
      })
    }
  }

  return results.sort((a, b) =>
    a.linkText.localeCompare(b.linkText, undefined, { sensitivity: 'base' }) ||
    a.sourceRelativePath.localeCompare(b.sourceRelativePath, undefined, { sensitivity: 'base' })
  )
}

function collectOrphanNotes(files, backlinks, folderPath) {
  const results = []
  for (const [path] of files) {
    const inbound = backlinks.get(path)
    if (inbound && inbound.size > 0) continue
    results.push({
      path,
      title: noteTitle(path),
      relativePath: relativePath(path, folderPath),
    })
  }

  return results.sort((a, b) =>
    a.relativePath.localeCompare(b.relativePath, undefined, { sensitivity: 'base' })
  )
}

function collectDuplicateTitles(allPaths, folderPath) {
  const groups = new Map()
  for (const path of allPaths) {
    const title = noteTitle(path)
    const key = title.toLocaleLowerCase()
    if (!groups.has(key)) groups.set(key, { title, paths: [] })
    groups.get(key).paths.push(path)
  }

  return Array.from(groups.values())
    .filter(group => group.paths.length > 1)
    .map(group => ({
      title: group.title,
      paths: group.paths.sort((a, b) => compareByDepthThenRelative(a, b, folderPath)),
      relativePaths: group.paths.map(path => relativePath(path, folderPath)),
    }))
    .sort((a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: 'base' }))
}

function collectGlossaryCandidates(files, folderPath) {
  const terms = new Map()
  for (const [path, entry] of files) {
    const content = stripMarkdownNoise(entry?.content || '')
    CAPITALIZED_PHRASE_RE.lastIndex = 0
    let match
    while ((match = CAPITALIZED_PHRASE_RE.exec(content)) !== null) {
      const term = normalizeTerm(match[0])
      if (!term || TERM_STOPWORDS.has(term)) continue
      if (!terms.has(term)) terms.set(term, { term, count: 0, files: new Set() })
      const candidate = terms.get(term)
      candidate.count += 1
      candidate.files.add(path)
    }
  }

  return Array.from(terms.values())
    .filter(candidate => candidate.files.size >= 2 || candidate.count >= 3)
    .map(candidate => ({
      term: candidate.term,
      count: candidate.count,
      fileCount: candidate.files.size,
      files: Array.from(candidate.files).sort((a, b) =>
        relativePath(a, folderPath).localeCompare(relativePath(b, folderPath), undefined, { sensitivity: 'base' })
      ),
    }))
    .sort((a, b) =>
      b.fileCount - a.fileCount ||
      b.count - a.count ||
      a.term.localeCompare(b.term, undefined, { sensitivity: 'base' })
    )
}

function safelyResolve(resolveLink, linkText, allPaths, folderPath) {
  if (!resolveLink) return null
  try {
    return resolveLink(linkText, allPaths, folderPath)
  } catch {
    return null
  }
}

function stripMarkdownNoise(content) {
  return String(content)
    .replace(/^---[\s\S]*?---\s*/m, '')
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/`[^`]*`/g, ' ')
    .replace(/\[\[[^\]]+\]\]/g, ' ')
    .replace(/!\[[^\]]*\]\([^)]+\)/g, ' ')
    .replace(/\[[^\]]+\]\([^)]+\)/g, ' ')
    .replace(/^#+\s+/gm, '')
}

function normalizeTerm(term) {
  const value = String(term || '').replace(/\s+/g, ' ').trim()
  if (!value) return ''
  if (/^(The|This|That|These|Those|And|But|For|With)\b/.test(value)) return ''
  return value
}

function noteTitle(path) {
  return String(path || '')
    .split(/[/\\]/)
    .pop()
    .replace(/\.md$/i, '')
}

function relativePath(path, folderPath) {
  const normalizedPath = String(path || '').replace(/\\/g, '/')
  const normalizedFolder = String(folderPath || '').replace(/\\/g, '/').replace(/\/+$/, '')
  if (normalizedFolder && normalizedPath.startsWith(normalizedFolder)) {
    return normalizedPath.slice(normalizedFolder.length).replace(/^\/+/, '')
  }
  return normalizedPath
}

function compareByDepthThenRelative(a, b, folderPath) {
  const aRelative = relativePath(a, folderPath)
  const bRelative = relativePath(b, folderPath)
  const aDepth = aRelative.split('/').length
  const bDepth = bRelative.split('/').length
  return aDepth - bDepth || aRelative.localeCompare(bRelative, undefined, { sensitivity: 'base' })
}
