import { state, fileName } from './state.js'
import { searchFiles, rebuildLinkIndex } from './link-index.js'
import { refreshTree } from './tabs.js'

// ── Tool definitions ────────────────────────────────────────────
// Each tool has:
//   name: identifier used by the LLM
//   description: tells the model what the tool does
//   schema: JSON schema for the input parameters
//   destructive: true → user must confirm before execution
//   execute(input): returns a string result (success message + relevant data)

export const TOOLS = [
  {
    name: 'list_files',
    description: "List markdown files and folders inside the user's project, optionally scoped to a subfolder. Use this first to see what notes exist before opening or modifying anything.",
    schema: {
      type: 'object',
      properties: {
        folder: { type: 'string', description: "Optional subfolder path relative to the project root, e.g. 'daily' or 'Personal Projects/Writing'. Omit to list the project root." },
        recursive: { type: 'boolean', description: 'Include files in nested folders. Default false.' },
      },
    },
    execute: async ({ folder, recursive } = {}) => {
      const root = state.folderPath
      if (!root) throw new Error('No project folder open')
      const target = folder ? joinPath(root, folder) : root
      if (!isInsideProject(target)) throw new Error('Path outside the project root')
      const tree = await window.fjord.readFolder(target)
      const out = []
      const walk = (items, depth = 0) => {
        for (const item of items) {
          const rel = stripRoot(item.path, root)
          out.push(`${'  '.repeat(depth)}${item.type === 'folder' ? '📁' : '📄'} ${rel}`)
          if (recursive && item.type === 'folder' && item.children) walk(item.children, depth + 1)
        }
      }
      walk(tree)
      if (out.length === 0) return '(empty)'
      return out.join('\n')
    },
  },
  {
    name: 'read_file',
    description: 'Read the content of a markdown file in the project. Use this to inspect a note before editing it.',
    schema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: "Path relative to the project root, e.g. 'daily/2026-05-15.md'." },
      },
      required: ['path'],
    },
    execute: async ({ path: rel }) => {
      const root = state.folderPath
      if (!root) throw new Error('No project folder open')
      const abs = joinPath(root, rel)
      if (!isInsideProject(abs)) throw new Error('Path outside the project root')
      const content = await window.fjord.readFile(abs)
      if (content == null) throw new Error(`File not found: ${rel}`)
      return content
    },
  },
  {
    name: 'search_notes',
    description: 'Full-text search across the project for a keyword or phrase. Returns matching files with brief snippets.',
    schema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'Search query.' },
        limit: { type: 'number', description: 'Max results to return (default 20).' },
      },
      required: ['query'],
    },
    execute: async ({ query, limit }) => {
      const results = searchFiles(query, { limit: limit || 20 })
      if (!results?.length) return `No results for "${query}".`
      const root = state.folderPath
      return results.map(r => {
        const rel = stripRoot(r.path, root)
        const snippet = r.snippet ? `\n    "${r.snippet.replace(/\s+/g, ' ').slice(0, 160)}"` : ''
        return `📄 ${rel}${snippet}`
      }).join('\n')
    },
  },
  {
    name: 'write_file',
    description: 'Create a new markdown file or completely overwrite an existing one. The path is relative to the project root. Parent folders are created automatically.',
    destructive: true,
    schema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: "Path relative to the project root, e.g. 'Family/Texas.md'." },
        content: { type: 'string', description: 'Full markdown content.' },
      },
      required: ['path', 'content'],
    },
    execute: async ({ path: rel, content }) => {
      const root = state.folderPath
      if (!root) throw new Error('No project folder open')
      const abs = joinPath(root, rel)
      if (!isInsideProject(abs)) throw new Error('Path outside the project root')
      // Ensure parent directory exists
      const parent = abs.replace(/[/\\][^/\\]+$/, '')
      if (parent && parent !== root) {
        try { await window.fjord.createDir(parent) } catch {}
      }
      const ok = await window.fjord.writeFile(abs, content)
      if (!ok) throw new Error(`Failed to write ${rel}`)
      await refreshTree()
      try { await rebuildLinkIndex() } catch {}
      return `Wrote ${rel} (${content.length} chars).`
    },
  },
  {
    name: 'move_file',
    description: 'Move or rename a file. Use this to reorganize notes — move a file into a different folder, or change its name.',
    destructive: true,
    schema: {
      type: 'object',
      properties: {
        from: { type: 'string', description: 'Existing path relative to project root.' },
        to: { type: 'string', description: 'New path relative to project root.' },
      },
      required: ['from', 'to'],
    },
    execute: async ({ from, to }) => {
      const root = state.folderPath
      if (!root) throw new Error('No project folder open')
      const src = joinPath(root, from)
      const dst = joinPath(root, to)
      if (!isInsideProject(src) || !isInsideProject(dst)) throw new Error('Path outside the project root')
      // Ensure destination parent exists
      const parent = dst.replace(/[/\\][^/\\]+$/, '')
      if (parent && parent !== root) {
        try { await window.fjord.createDir(parent) } catch {}
      }
      const ok = await window.fjord.renameFile(src, dst)
      if (!ok) throw new Error(`Failed to move ${from} → ${to}`)
      // Update any open tabs pointing at the moved file
      for (const tab of state.tabs) {
        if (tab.path === src) {
          tab.path = dst
          tab.name = fileName(dst)
        }
      }
      await refreshTree()
      try { await rebuildLinkIndex() } catch {}
      return `Moved ${from} → ${to}.`
    },
  },
  {
    name: 'delete_file',
    description: 'Move a file to the system trash. The user can still recover it from the OS trash if needed.',
    destructive: true,
    schema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Path relative to project root.' },
      },
      required: ['path'],
    },
    execute: async ({ path: rel }) => {
      const root = state.folderPath
      if (!root) throw new Error('No project folder open')
      const abs = joinPath(root, rel)
      if (!isInsideProject(abs)) throw new Error('Path outside the project root')
      const ok = await window.fjord.trashFile(abs)
      if (!ok) throw new Error(`Failed to delete ${rel}`)
      await refreshTree()
      try { await rebuildLinkIndex() } catch {}
      return `Moved ${rel} to trash.`
    },
  },
  {
    name: 'create_folder',
    description: 'Create a new folder inside the project. Existing folders are not affected.',
    schema: {
      type: 'object',
      properties: {
        path: { type: 'string', description: 'Folder path relative to project root.' },
      },
      required: ['path'],
    },
    execute: async ({ path: rel }) => {
      const root = state.folderPath
      if (!root) throw new Error('No project folder open')
      const abs = joinPath(root, rel)
      if (!isInsideProject(abs)) throw new Error('Path outside the project root')
      const ok = await window.fjord.createDir(abs)
      if (!ok) throw new Error(`Failed to create folder ${rel}`)
      await refreshTree()
      return `Created folder ${rel}.`
    },
  },
]

// ── Helpers ─────────────────────────────────────────────────────
function joinPath(root, rel) {
  if (!rel) return root
  // Use the same separator as the root path (Windows backslash vs POSIX slash)
  const sep = root.includes('\\') && !root.includes('/') ? '\\' : '/'
  const r = String(rel).replace(/^[/\\]+/, '')  // strip leading slashes
  return `${root}${sep}${r}`
}

function stripRoot(abs, root) {
  if (!abs.startsWith(root)) return abs
  return abs.slice(root.length).replace(/^[/\\]+/, '')
}

function isInsideProject(p) {
  const root = state.folderPath
  if (!root || !p) return false
  // Reject path-traversal segments without rejecting filenames that merely
  // contain consecutive dots (e.g. "my..notes.md").
  const segments = p.split(/[\\/]/)
  if (segments.some(s => s === '..' || s === '.')) return false
  return p === root || p.startsWith(root + '/') || p.startsWith(root + '\\')
}

// ── Tool execution ──────────────────────────────────────────────
export async function executeToolByName(name, input) {
  const tool = TOOLS.find(t => t.name === name)
  if (!tool) throw new Error(`Unknown tool: ${name}`)
  return await tool.execute(input || {})
}

export function getToolSpec(name) {
  return TOOLS.find(t => t.name === name)
}

// ── Provider-specific tool schemas ──────────────────────────────
export function toolsForProvider(provider) {
  if (provider === 'anthropic') {
    return TOOLS.map(t => ({
      name: t.name,
      description: t.description,
      input_schema: t.schema,
    }))
  }
  // OpenAI (and Ollama models that support OpenAI function-calling)
  return TOOLS.map(t => ({
    type: 'function',
    function: {
      name: t.name,
      description: t.description,
      parameters: t.schema,
    },
  }))
}
