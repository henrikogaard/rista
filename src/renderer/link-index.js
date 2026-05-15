import { state } from './state.js'

// ── Link Indexer ─────────────────────────────────────────────────
// Scans all markdown files in the open folder for wikilinks [[...]]
// and builds an index for backlinks, outgoing links, and search.

let _index = {
  files: new Map(),      // path => { content, links: Set() }
  backlinks: new Map(),  // targetPath => Set(sourcePaths)
  allPaths: new Set(),   // All .md file paths
  dirty: false,
}

const WIKILINK_RE = /\[\[([^\]|]+)(?:\|[^\]]+)?\]\]/g

export function clearLinkIndex() {
  _index = { files: new Map(), backlinks: new Map(), allPaths: new Set(), dirty: false }
}

export function getLinkIndex() {
  return _index
}

export async function rebuildLinkIndex() {
  clearLinkIndex()
  if (!state.folderPath) return _index

  // Read all markdown files using main process
  const files = await window.fjord.readFolder(state.folderPath)
  const mdPaths = collectMdPaths(files)

  for (const path of mdPaths) {
    _index.allPaths.add(path)
    try {
      const content = await window.fjord.readFile(path)
      const links = extractWikilinks(content)
      _index.files.set(path, { content, links })
    } catch {
      _index.files.set(path, { content: '', links: new Set() })
    }
  }

  // Build backlinks
  for (const [sourcePath, { links }] of _index.files) {
    for (const link of links) {
      const target = resolveWikilink(link, _index.allPaths, state.folderPath)
      if (target) {
        if (!_index.backlinks.has(target)) {
          _index.backlinks.set(target, new Set())
        }
        _index.backlinks.get(target).add(sourcePath)
      }
    }
  }

  _index.dirty = false
  return _index
}

export function updateLinkIndexForFile(path, content) {
  if (!path || !_index.allPaths.has(path)) {
    if (path && path.endsWith('.md')) _index.allPaths.add(path)
    else return
  }
  const links = extractWikilinks(content)
  _index.files.set(path, { content, links })
  _index.dirty = true

  // Defer full backlink rebuild
  if (!_index._rebuildTimeout) {
    _index._rebuildTimeout = setTimeout(() => {
      _index._rebuildTimeout = null
      rebuildBacklinks()
    }, 500)
  }
}

export function rebuildBacklinks() {
  _index.backlinks.clear()
  for (const [sourcePath, { links }] of _index.files) {
    for (const link of links) {
      const target = resolveWikilink(link, _index.allPaths, state.folderPath)
      if (target) {
        if (!_index.backlinks.has(target)) {
          _index.backlinks.set(target, new Set())
        }
        _index.backlinks.get(target).add(sourcePath)
      }
    }
  }
  _index.dirty = false
}

export function removeFromLinkIndex(path) {
  _index.files.delete(path)
  _index.allPaths.delete(path)
  for (const set of _index.backlinks.values()) {
    set.delete(path)
  }
  _index.dirty = true
}

export function extractWikilinks(content) {
  const links = new Set()
  let m
  while ((m = WIKILINK_RE.exec(content)) !== null) {
    links.add(m[1].trim())
  }
  return links
}

export function resolveWikilink(linkText, allPaths, folderPath) {
  // Try exact match
  const exact = Array.from(allPaths).find(p => {
    const name = p.split(/[/\\]/).pop()
    const base = name.replace(/\.md$/i, '')
    return base === linkText
  })
  if (exact) return exact

  // Try with .md suffix
  const withExt = Array.from(allPaths).find(p => {
    const name = p.split(/[/\\]/).pop()
    return name === linkText + '.md' || name === linkText
  })
  if (withExt) return withExt

  return null
}

export function getOutgoingLinks(path) {
  const entry = _index.files.get(path)
  if (!entry) return []
  return Array.from(entry.links).map(link => {
    const target = resolveWikilink(link, _index.allPaths, state.folderPath)
    return { linkText: link, targetPath: target, exists: Boolean(target) }
  })
}

export function getBacklinks(path) {
  const backlinks = _index.backlinks.get(path)
  if (!backlinks) return []
  return Array.from(backlinks).map(sourcePath => {
    const entry = _index.files.get(sourcePath)
    const sourceLinks = entry ? Array.from(entry.links) : []
    const matchingLinks = sourceLinks.filter(link => {
      const target = resolveWikilink(link, _index.allPaths, state.folderPath)
      return target === path
    })
    return {
      sourcePath,
      sourceName: sourcePath.split(/[/\\]/).pop().replace(/\.md$/i, ''),
      linkTexts: matchingLinks,
    }
  })
}

export function getAllMdFileNames() {
  return Array.from(_index.allPaths)
    .map(p => p.split(/[/\\]/).pop().replace(/\.md$/i, ''))
    .sort((a, b) => a.localeCompare(b))
}

export function searchFiles(query, options = {}) {
  const { includeContent = true, includePaths = true, limit = 50 } = options
  if (!query || query.length < 2) return []
  const q = query.toLowerCase()
  const results = []

  for (const [path, { content }] of _index.files) {
    const name = path.split(/[/\\]/).pop()
    const nameMatch = name.toLowerCase().includes(q)
    let contentMatch = false
    let preview = ''

    if (includeContent && content) {
      const idx = content.toLowerCase().indexOf(q)
      if (idx !== -1) {
        contentMatch = true
        const start = Math.max(0, idx - 40)
        const end = Math.min(content.length, idx + q.length + 60)
        preview = content.slice(start, end).replace(/\n/g, ' ')
        if (start > 0) preview = '...' + preview
        if (end < content.length) preview = preview + '...'
      }
    }

    if (nameMatch || contentMatch) {
      results.push({
        path,
        name,
        nameMatch,
        contentMatch,
        preview,
      })
    }

    if (results.length >= limit) break
  }

  // Sort: name matches first, then content matches
  return results.sort((a, b) => {
    if (a.nameMatch && !b.nameMatch) return -1
    if (!a.nameMatch && b.nameMatch) return 1
    return a.name.localeCompare(b.name)
  })
}

function collectMdPaths(nodes) {
  const paths = []
  for (const node of nodes) {
    if (node.type === 'file' && node.name.endsWith('.md')) {
      paths.push(node.path)
    } else if (node.type === 'folder' && node.children) {
      paths.push(...collectMdPaths(node.children))
    }
  }
  return paths
}
