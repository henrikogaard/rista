import { state } from './state.js'

const PREFIX = 'rista-bookmarks-'

function hashPath(folderPath) {
  let hash = 0
  for (let i = 0; i < folderPath.length; i++) {
    const char = folderPath.charCodeAt(i)
    hash = ((hash << 5) - hash) + char
    hash |= 0
  }
  return Math.abs(hash).toString(36)
}

function storageKey() {
  if (!state.folderPath) return null
  return PREFIX + hashPath(state.folderPath)
}

let _listeners = new Set()
let _cached = null

function load() {
  const key = storageKey()
  if (!key) return []
  if (_cached) return _cached
  try {
    const raw = localStorage.getItem(key)
    _cached = raw ? JSON.parse(raw) : []
    if (!Array.isArray(_cached)) _cached = []
  } catch {
    _cached = []
  }
  // Migrate legacy entries (no `type` field) to `type: 'file'`.
  for (const item of _cached) {
    if (item && !item.type) item.type = 'file'
  }
  return _cached
}

// ── Group helpers ───────────────────────────────────────────────
function* walk(list) {
  for (const item of list) {
    yield item
    if (item?.type === 'group' && Array.isArray(item.children)) {
      yield* walk(item.children)
    }
  }
}

function findItemAndParent(list, predicate, parent = null) {
  for (const item of list) {
    if (predicate(item)) return { item, parent: parent || list, list }
    if (item?.type === 'group' && Array.isArray(item.children)) {
      const found = findItemAndParent(item.children, predicate, item.children)
      if (found) return found
    }
  }
  return null
}

function save() {
  const key = storageKey()
  if (!key) return
  try {
    localStorage.setItem(key, JSON.stringify(_cached || []))
  } catch {}
  _listeners.forEach(fn => fn())
}

export function resetBookmarksCache() {
  _cached = null
  _listeners.forEach(fn => fn())
}

export function onBookmarksChange(fn) {
  _listeners.add(fn)
  return () => _listeners.delete(fn)
}

export function getBookmarks() {
  return [...load()]
}

export function isBookmarked(path) {
  for (const item of walk(load())) {
    if (item?.type === 'file' && item.path === path) return true
  }
  return false
}

export function addBookmark(path, name) {
  if (!path) return
  const list = load()
  // Don't duplicate
  for (const item of walk(list)) if (item?.type === 'file' && item.path === path) return
  list.push({ id: makeId(), type: 'file', path, name: name || path.split('/').pop(), addedAt: Date.now() })
  _cached = list
  save()
}

export function removeBookmark(path) {
  if (!path) return
  const list = load()
  const found = findItemAndParent(list, (it) => it?.type === 'file' && it.path === path)
  if (!found) return
  const idx = found.list.indexOf(found.item)
  if (idx === -1) return
  found.list.splice(idx, 1)
  _cached = list
  save()
}

export function toggleBookmark(path, name) {
  if (isBookmarked(path)) removeBookmark(path)
  else addBookmark(path, name)
}

function makeId() {
  return 'b' + Math.random().toString(36).slice(2, 10)
}

export function addBookmarkGroup(name) {
  const trimmed = (name || '').trim() || 'New group'
  const list = load()
  const id = makeId()
  list.push({ id, type: 'group', name: trimmed, children: [], collapsed: false })
  _cached = list
  save()
  return id
}

export function renameBookmarkGroup(id, name) {
  const list = load()
  const found = findItemAndParent(list, (it) => it?.id === id && it?.type === 'group')
  if (!found) return
  found.item.name = (name || '').trim() || found.item.name
  save()
}

export function removeBookmarkGroup(id) {
  const list = load()
  const found = findItemAndParent(list, (it) => it?.id === id && it?.type === 'group')
  if (!found) return
  // Move children up to the parent list at the group's position
  const idx = found.list.indexOf(found.item)
  if (idx === -1) return
  const children = found.item.children || []
  found.list.splice(idx, 1, ...children)
  save()
}

export function toggleGroupCollapsed(id) {
  const list = load()
  const found = findItemAndParent(list, (it) => it?.id === id && it?.type === 'group')
  if (!found) return
  found.item.collapsed = !found.item.collapsed
  save()
}

export function moveBookmarkToGroup(bookmarkPath, groupId) {
  const list = load()
  const src = findItemAndParent(list, (it) => it?.type === 'file' && it.path === bookmarkPath)
  if (!src) return
  // Remove from current location
  const idx = src.list.indexOf(src.item)
  if (idx !== -1) src.list.splice(idx, 1)

  if (groupId == null) {
    // Move to root
    list.push(src.item)
  } else {
    const target = findItemAndParent(list, (it) => it?.id === groupId && it?.type === 'group')
    if (!target) {
      // Group disappeared — put back at top level
      list.push(src.item)
    } else {
      target.item.children = target.item.children || []
      target.item.children.push(src.item)
    }
  }
  save()
}

export function moveBookmark(path, direction) {
  const list = load()
  const found = findItemAndParent(list, (it) => it?.type === 'file' && it.path === path)
  if (!found) return
  const idx = found.list.indexOf(found.item)
  const target = direction === 'up' ? idx - 1 : idx + 1
  if (target < 0 || target >= found.list.length) return
  ;[found.list[idx], found.list[target]] = [found.list[target], found.list[idx]]
  save()
}

export function listGroups() {
  const list = load()
  const out = []
  for (const item of walk(list)) {
    if (item?.type === 'group') out.push({ id: item.id, name: item.name })
  }
  return out
}
