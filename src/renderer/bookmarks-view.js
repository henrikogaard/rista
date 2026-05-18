import { getBookmarks, removeBookmark, moveBookmark, onBookmarksChange } from './bookmarks.js'
import { showContextMenu } from './context-menu.js'

let _openFile = null
let _unsubscribe = null

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

export function setBookmarksOpenFile(fn) {
  _openFile = fn
}

export function buildBookmarksPanel() {
  return `<div class="bookmarks-view" id="bookmarks-view-body"></div>`
}

export function renderBookmarks() {
  const body = document.getElementById('bookmarks-view-body')
  if (!body) return
  const list = getBookmarks()
  if (list.length === 0) {
    body.innerHTML = `<div class="bookmarks-view__empty">No bookmarks yet.<div class="bookmarks-view__hint">Right-click a file in the tab strip or explorer and choose Bookmark.</div></div>`
    return
  }
  body.innerHTML = list.map(b => `
    <div class="bookmark-item" data-path="${escapeHtml(b.path)}" role="button" tabindex="0" title="${escapeHtml(b.path)}">
      <span class="bookmark-item__icon">★</span>
      <span class="bookmark-item__name">${escapeHtml(b.name)}</span>
      <span class="bookmark-item__remove" data-action="remove" title="Remove bookmark">×</span>
    </div>
  `).join('')
}

function onItemClick(event) {
  const removeBtn = event.target.closest('[data-action="remove"]')
  const item = event.target.closest('.bookmark-item')
  if (!item) return
  const path = item.dataset.path
  if (!path) return
  if (removeBtn) {
    event.stopPropagation()
    removeBookmark(path)
    return
  }
  _openFile?.({ path, name: path.split('/').pop() })
}

function onContextMenu(event) {
  const item = event.target.closest('.bookmark-item')
  if (!item) return
  event.preventDefault()
  const path = item.dataset.path
  showContextMenu(event.clientX, event.clientY, [
    { label: 'Open', action: () => _openFile?.({ path, name: path.split('/').pop() }) },
    { label: 'Move Up', action: () => moveBookmark(path, 'up') },
    { label: 'Move Down', action: () => moveBookmark(path, 'down') },
    { separator: true },
    { label: 'Remove Bookmark', action: () => removeBookmark(path) },
  ])
}

export function mountBookmarksPanel() {
  renderBookmarks()
  const body = document.getElementById('bookmarks-view-body')
  if (!body) return
  body.addEventListener('click', onItemClick)
  body.addEventListener('contextmenu', onContextMenu)
  body.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return
    const item = event.target.closest('.bookmark-item')
    if (!item) return
    event.preventDefault()
    _openFile?.({ path: item.dataset.path, name: item.dataset.path.split('/').pop() })
  })
  _unsubscribe?.()
  _unsubscribe = onBookmarksChange(renderBookmarks)
}

export function unmountBookmarksPanel() {
  _unsubscribe?.()
  _unsubscribe = null
}
