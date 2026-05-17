import { state } from './state.js'
import { getSettings } from './settings.js'

// ── Version History ─────────────────────────────────────────────
// Saves snapshots of files to .fjordmark/history/{basename}/{timestamp}.md
// for lightweight local versioning.

const HISTORY_DIR = '.fjordmark/history'

function getHistoryDir(filePath) {
  if (!state.folderPath) return null
  // Use the path relative to folderPath, with separators replaced by __,
  // so files with the same basename in different folders don't collide.
  let rel = filePath
  if (filePath.startsWith(state.folderPath)) {
    rel = filePath.slice(state.folderPath.length).replace(/^[/\\]+/, '')
  }
  const key = rel.replace(/\.md$/i, '').replace(/[/\\]/g, '__')
  return `${state.folderPath}/${HISTORY_DIR}/${key}`
}

function formatTimestamp() {
  const now = new Date()
  const pad = (n, len = 2) => String(n).padStart(len, '0')
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}_${pad(now.getHours())}-${pad(now.getMinutes())}-${pad(now.getSeconds())}`
}

export async function saveSnapshot(filePath, content) {
  if (!state.folderPath || !filePath || !content) return false
  const dir = getHistoryDir(filePath)
  if (!dir) return false

  try {
    await window.fjord.createDir(dir)
    const timestamp = formatTimestamp()
    const snapshotPath = `${dir}/${timestamp}.md`
    const ok = await window.fjord.writeFile(snapshotPath, content)
    if (ok) {
      const settings = getSettings()
      const maxSnapshots = settings.maxHistorySnapshots || 50
      pruneSnapshots(filePath, maxSnapshots).catch(() => {})
    }
    return ok
  } catch {
    return false
  }
}

export async function getSnapshots(filePath) {
  if (!state.folderPath || !filePath) return []
  const dir = getHistoryDir(filePath)
  if (!dir) return []

  try {
    const entries = await window.fjord.listDir(dir)
    if (!entries || !entries.length) return []

    const snapshots = entries
      .filter(name => name.endsWith('.md'))
      .map(name => {
        const path = `${dir}/${name}`
        const timestampStr = name.replace(/\.md$/, '')
        const parsed = parseTimestamp(timestampStr)
        return {
          timestamp: parsed,
          timestampStr,
          path,
          name,
        }
      })
      .filter(s => s.timestamp)
      .sort((a, b) => b.timestamp - a.timestamp)

    // Fetch sizes in parallel
    const withStats = await Promise.all(
      snapshots.map(async s => {
        const stat = await window.fjord.stat(s.path)
        return { ...s, size: stat?.size || 0 }
      })
    )

    return withStats
  } catch {
    return []
  }
}

function parseTimestamp(str) {
  // Format: 2026-05-17_14-30-25
  const match = str.match(/^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})-(\d{2})$/)
  if (!match) return null
  return new Date(
    Number(match[1]),
    Number(match[2]) - 1,
    Number(match[3]),
    Number(match[4]),
    Number(match[5]),
    Number(match[6])
  )
}

export async function loadSnapshot(snapshotPath) {
  if (!snapshotPath) return null
  try {
    return await window.fjord.readFile(snapshotPath)
  } catch {
    return null
  }
}

export async function pruneSnapshots(filePath, maxCount = 50) {
  if (!state.folderPath || !filePath) return
  const dir = getHistoryDir(filePath)
  if (!dir) return

  try {
    const entries = await window.fjord.listDir(dir)
    if (!entries || entries.length <= maxCount) return

    const sorted = entries
      .filter(name => name.endsWith('.md'))
      .sort()

    // sorted is ascending (oldest first); delete from the beginning
    const toDelete = sorted.slice(0, sorted.length - maxCount)
    for (const name of toDelete) {
      await window.fjord.deleteFile(`${dir}/${name}`)
    }
  } catch {
    // Silently fail — pruning is best-effort
  }
}

// ── Relative time formatting ────────────────────────────────────
export function relativeTime(date) {
  if (!date) return ''
  const now = Date.now()
  const diff = now - date.getTime()
  const seconds = Math.floor(diff / 1000)
  const minutes = Math.floor(seconds / 60)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)

  if (seconds < 60) return 'just now'
  if (minutes < 60) return `${minutes}m ago`
  if (hours < 24) return `${hours}h ago`
  if (days < 7) return `${days}d ago`
  return date.toLocaleDateString()
}

export function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
