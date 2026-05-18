import { state } from './state.js'

const PREFIX = 'fjordmark-bookmarks-'

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
  return _cached
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
  return load().some(b => b.path === path)
}

export function addBookmark(path, name) {
  if (!path) return
  const list = load()
  if (list.some(b => b.path === path)) return
  list.push({ path, name: name || path.split('/').pop(), addedAt: Date.now() })
  _cached = list
  save()
}

export function removeBookmark(path) {
  if (!path) return
  const list = load()
  const idx = list.findIndex(b => b.path === path)
  if (idx === -1) return
  list.splice(idx, 1)
  _cached = list
  save()
}

export function toggleBookmark(path, name) {
  if (isBookmarked(path)) removeBookmark(path)
  else addBookmark(path, name)
}

export function moveBookmark(path, direction) {
  const list = load()
  const idx = list.findIndex(b => b.path === path)
  if (idx === -1) return
  const target = direction === 'up' ? idx - 1 : idx + 1
  if (target < 0 || target >= list.length) return
  ;[list[idx], list[target]] = [list[target], list[idx]]
  _cached = list
  save()
}
