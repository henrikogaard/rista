import { fileName, stripMarkdownExtension, relativeFilePath } from './state.js'
const STOPWORDS = new Set([
  'about', 'after', 'again', 'also', 'and', 'are', 'because', 'been', 'before', 'being',
  'between', 'but', 'can', 'could', 'did', 'does', 'doing', 'for', 'from', 'had',
  'has', 'have', 'her', 'here', 'him', 'his', 'how', 'into', 'its', 'just',
  'more', 'most', 'not', 'now', 'our', 'out', 'over', 'she', 'should', 'that',
  'the', 'their', 'them', 'then', 'there', 'these', 'they', 'this', 'those', 'through',
  'too', 'under', 'was', 'were', 'what', 'when', 'where', 'which', 'while', 'who',
  'why', 'with', 'would', 'you', 'your',
])

export function buildSemanticIndex(linkIndex, options = {}) {
  const folderPath = options.folderPath || ''
  const files = linkIndex?.files instanceof Map ? linkIndex.files : new Map()
  const documents = []
  const docFrequency = new Map()

  for (const [path, entry] of files) {
    const content = entry?.content || ''
    const title = noteTitle(path)
    const sections = extractSections(content)
    const counts = new Map()

    addTokens(counts, tokenize(title), 3)
    for (const section of sections) {
      addTokens(counts, tokenize(section.heading), 2)
      addTokens(counts, tokenize(section.text), 1)
    }

    for (const term of counts.keys()) {
      docFrequency.set(term, (docFrequency.get(term) || 0) + 1)
    }

    documents.push({
      path,
      name: fileName(path),
      title,
      relativePath: relativeFilePath(path, folderPath),
      sections,
      counts,
      weights: new Map(),
      norm: 0,
    })
  }

  const idf = buildIdf(docFrequency, documents.length)
  for (const document of documents) {
    document.weights = weightTerms(document.counts, idf)
    document.norm = vectorNorm(document.weights)
  }

  return {
    status: documents.length ? 'ready' : 'empty',
    dirty: Boolean(linkIndex?.dirty),
    documentCount: documents.length,
    documents,
    idf,
    folderPath,
    updatedAt: Date.now(),
  }
}

export function searchSemanticIndex(semanticIndex, query, options = {}) {
  const limit = Number.isFinite(options.limit) ? options.limit : 8
  const excludePath = options.excludePath || null
  const queryWeights = weightTerms(countTokens(tokenize(query)), semanticIndex?.idf || new Map())
  const queryNorm = vectorNorm(queryWeights)
  if (!queryNorm) return []

  return (semanticIndex?.documents || [])
    .filter(document => document.path !== excludePath)
    .map(document => {
      const baseScore = cosineSimilarity(document.weights, queryWeights, document.norm, queryNorm)
      const snippet = bestSnippetForTerms(document, Array.from(queryWeights.keys()))
      const score = baseScore + snippet.rankBoost
      return {
        path: document.path,
        name: document.name,
        title: document.title,
        relativePath: document.relativePath,
        heading: snippet.heading,
        line: snippet.line,
        snippet: snippet.text,
        score,
        relevance: Math.round(score * 1000) / 1000,
      }
    })
    .filter(result => result.score > 0)
    .sort((a, b) => b.score - a.score || a.relativePath.localeCompare(b.relativePath))
    .slice(0, limit)
}

export function findRelatedNotes(semanticIndex, currentPath, options = {}) {
  const limit = Number.isFinite(options.limit) ? options.limit : 6
  const documents = semanticIndex?.documents || []
  const current = documents.find(document => document.path === currentPath)
  if (!current || !current.norm) return []

  return documents
    .filter(document => document.path !== currentPath)
    .map(document => {
      const score = cosineSimilarity(document.weights, current.weights, document.norm, current.norm)
      const sharedTerms = topSharedTerms(current, document)
      const snippet = bestSnippetForTerms(document, sharedTerms)
      return {
        path: document.path,
        name: document.name,
        title: document.title,
        relativePath: document.relativePath,
        heading: snippet.heading,
        line: snippet.line,
        snippet: snippet.text,
        score,
        relevance: Math.round(score * 1000) / 1000,
        reason: sharedTerms.length ? `Shared terms: ${sharedTerms.join(', ')}` : 'Similar language',
      }
    })
    .filter(result => result.score > 0.02)
    .sort((a, b) => b.score - a.score || a.relativePath.localeCompare(b.relativePath))
    .slice(0, limit)
}

function buildIdf(docFrequency, documentCount) {
  const idf = new Map()
  for (const [term, count] of docFrequency) {
    idf.set(term, 1 + Math.log((1 + documentCount) / (1 + count)))
  }
  return idf
}

function weightTerms(counts, idf) {
  const weights = new Map()
  for (const [term, count] of counts) {
    weights.set(term, count * (idf.get(term) || 1))
  }
  return weights
}

function vectorNorm(weights) {
  let total = 0
  for (const value of weights.values()) total += value * value
  return Math.sqrt(total)
}

function cosineSimilarity(a, b, aNorm, bNorm) {
  if (!aNorm || !bNorm) return 0
  let dot = 0
  const [small, large] = a.size < b.size ? [a, b] : [b, a]
  for (const [term, value] of small) {
    dot += value * (large.get(term) || 0)
  }
  return dot / (aNorm * bNorm)
}

function bestSnippetForTerms(document, terms) {
  const wanted = new Set(terms)
  let best = null

  for (const section of document.sections) {
    const tokens = tokenize(section.text)
    const positions = []
    tokens.forEach((token, index) => {
      if (wanted.has(token)) positions.push(index)
    })
    if (!positions.length) continue
    const span = positions[positions.length - 1] - positions[0] + 1
    const rank = positions.length + positions.length / Math.max(1, span)
    if (!best || rank > best.rank) {
      best = {
        rank,
        rankBoost: rank * 0.01,
        heading: section.heading,
        line: section.line,
        text: snippetFromSection(section.text, Array.from(wanted)),
      }
    }
  }

  if (best) return best
  const fallback = document.sections[0] || { heading: document.title, line: 1, text: document.title }
  return {
    rank: 0,
    rankBoost: 0,
    heading: fallback.heading,
    line: fallback.line,
    text: snippetFromSection(fallback.text, []),
  }
}

function topSharedTerms(a, b) {
  const terms = []
  for (const [term, aWeight] of a.weights) {
    const bWeight = b.weights.get(term)
    if (!bWeight) continue
    terms.push({ term, score: aWeight * bWeight })
  }
  return terms
    .sort((left, right) => right.score - left.score || left.term.localeCompare(right.term))
    .slice(0, 4)
    .map(item => item.term)
}

function extractSections(content) {
  const lines = stripFrontmatter(String(content || '')).split(/\r?\n/)
  const sections = []
  let heading = 'Document'
  let line = 1
  let buffer = []

  const flush = () => {
    const text = stripMarkdown(buffer.join('\n')).trim()
    if (text) sections.push({ heading, line, text })
  }

  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(/^(#{1,6})\s+(.+?)\s*$/)
    if (match) {
      flush()
      heading = match[2].replace(/\s+#+$/, '').trim()
      line = index + 1
      buffer = []
    } else {
      buffer.push(lines[index])
    }
  }
  flush()

  if (!sections.length) {
    const text = stripMarkdown(lines.join('\n')).trim()
    if (text) sections.push({ heading: 'Document', line: 1, text })
  }
  return sections
}

function snippetFromSection(text, terms) {
  const clean = String(text || '').replace(/\s+/g, ' ').trim()
  if (clean.length <= 170) return clean
  const lower = clean.toLowerCase()
  const firstMatch = terms
    .map(term => lower.indexOf(term))
    .filter(index => index >= 0)
    .sort((a, b) => a - b)[0]
  const center = firstMatch >= 0 ? firstMatch : 0
  const start = Math.max(0, center - 55)
  const end = Math.min(clean.length, start + 170)
  return `${start > 0 ? '...' : ''}${clean.slice(start, end).trim()}${end < clean.length ? '...' : ''}`
}

function stripFrontmatter(content) {
  return content.replace(/^---[\s\S]*?---\s*/, '')
}

function stripMarkdown(content) {
  return String(content || '')
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/`[^`]*`/g, ' ')
    .replace(/!\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, '$2 $1')
    .replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, '$2 $1')
    .replace(/!\[[^\]]*\]\([^)]+\)/g, ' ')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/[#*_~>`^|\[\](){}:;,.!?\/\\-]+/g, ' ')
}

function tokenize(text) {
  const normalized = String(text || '')
    .normalize('NFKD')
    .toLowerCase()
  return normalized.match(/[a-z0-9][a-z0-9-]{2,}/g)
    ?.filter(token => !STOPWORDS.has(token) && !/^\d+$/.test(token)) || []
}

function countTokens(tokens) {
  const counts = new Map()
  addTokens(counts, tokens, 1)
  return counts
}

function addTokens(counts, tokens, weight) {
  for (const token of tokens) {
    counts.set(token, (counts.get(token) || 0) + weight)
  }
}

function noteTitle(path) {
  return stripMarkdownExtension(fileName(path))
}
