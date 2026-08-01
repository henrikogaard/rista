import {
  getBookmarks,
  removeBookmark,
  moveBookmark,
  onBookmarksChange,
  addBookmarkGroup,
  renameBookmarkGroup,
  removeBookmarkGroup,
  toggleGroupCollapsed,
  moveBookmarkToGroup,
  listGroups,
} from './bookmarks.js'
import { showContextMenu } from './context-menu.js'

import { $, escapeHtml, fileName } from './state.js'
let _openFile = null
let _unsubscribe = null



export function setBookmarksOpenFile(fn) {
  _openFile = fn
}

export function buildBookmarksPanel() {
  return `
    <div class="bookmarks-view" id="bookmarks-view-body"></div>
    <div class="bookmarks-view__footer">
      <button type="button" class="bookmarks-add-group" id="bookmarks-add-group-btn">+ New group</button>
    </div>
  `
}

function renderItems(items, depth = 0) {
  if (!items.length) return ''
  return items.map(item => {
    if (item.type === 'group') {
      const collapsed = !!item.collapsed
      const childrenHtml = collapsed ? '' : renderItems(item.children || [], depth + 1)
      return `
        <div class="bookmark-group${collapsed ? ' collapsed' : ''}" data-group-id="${escapeHtml(item.id)}">
          <div class="bookmark-group__header" data-action="toggle-group" style="padding-left:${depth * 12 + 6}px" role="button" tabindex="0" title="${escapeHtml(item.name)}">
            <span class="bookmark-group__caret">▾</span>
            <span class="bookmark-group__name">${escapeHtml(item.name)}</span>
            <span class="bookmark-group__count">${(item.children || []).length}</span>
          </div>
          <div class="bookmark-group__children">${childrenHtml}</div>
        </div>
      `
    }
    // file
    return `
      <div class="bookmark-item" data-path="${escapeHtml(item.path)}" data-bookmark-id="${escapeHtml(item.id || '')}" style="padding-left:${depth * 12 + 10}px" role="button" tabindex="0" title="${escapeHtml(item.path)}">
        <span class="bookmark-item__icon">★</span>
        <span class="bookmark-item__name">${escapeHtml(item.name)}</span>
        <span class="bookmark-item__remove" data-action="remove" title="Remove bookmark">×</span>
      </div>
    `
  }).join('')
}

export function renderBookmarks() {
  const body = $('bookmarks-view-body')
  if (!body) return
  const list = getBookmarks()
  if (list.length === 0) {
    body.innerHTML = `<div class="bookmarks-view__empty">No bookmarks yet.<div class="bookmarks-view__hint">Right-click a file in the tab strip or explorer and choose Bookmark.</div></div>`
    return
  }
  body.innerHTML = renderItems(list)
}

function onItemClick(event) {
  // Group header toggle
  const groupHeader = event.target.closest('[data-action="toggle-group"]')
  if (groupHeader) {
    const group = groupHeader.closest('.bookmark-group')
    const id = group?.dataset.groupId
    if (id) toggleGroupCollapsed(id)
    return
  }

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
  _openFile?.({ path, name: fileName(path) })
}

function onContextMenu(event) {
  const group = event.target.closest('.bookmark-group')
  const item = event.target.closest('.bookmark-item')

  if (item) {
    event.preventDefault()
    event.stopPropagation()
    const path = item.dataset.path
    const groups = listGroups()
    const moveSubmenu = [
      { label: '(Root level)', action: () => moveBookmarkToGroup(path, null) },
      ...(groups.length ? [{ separator: true }] : []),
      ...groups.map(g => ({ label: g.name, action: () => moveBookmarkToGroup(path, g.id) })),
    ]
    showContextMenu(event.clientX, event.clientY, [
      { label: 'Open', action: () => _openFile?.({ path, name: fileName(path) }) },
      { label: 'Move Up', action: () => moveBookmark(path, 'up') },
      { label: 'Move Down', action: () => moveBookmark(path, 'down') },
      { label: 'Move to Group…', submenu: moveSubmenu },
      { separator: true },
      { label: 'Remove Bookmark', action: () => removeBookmark(path) },
    ])
    return
  }

  if (group) {
    event.preventDefault()
    event.stopPropagation()
    const id = group.dataset.groupId
    showContextMenu(event.clientX, event.clientY, [
      { label: 'Rename Group', action: () => {
        const current = group.querySelector('.bookmark-group__name')?.textContent || ''
        const next = prompt('Rename group:', current)
        if (next != null) renameBookmarkGroup(id, next)
      } },
      { separator: true },
      { label: 'Remove Group (keep bookmarks)', action: () => removeBookmarkGroup(id) },
    ])
  }
}

function onFooterClick(event) {
  if (event.target.id !== 'bookmarks-add-group-btn') return
  const name = prompt('Group name:')
  if (!name) return
  addBookmarkGroup(name)
}

export function mountBookmarksPanel() {
  renderBookmarks()
  const body = $('bookmarks-view-body')
  if (!body) return
  body.addEventListener('click', onItemClick)
  body.addEventListener('contextmenu', onContextMenu)
  body.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return
    const groupHeader = event.target.closest('[data-action="toggle-group"]')
    if (groupHeader) {
      event.preventDefault()
      const id = groupHeader.closest('.bookmark-group')?.dataset.groupId
      if (id) toggleGroupCollapsed(id)
      return
    }
    const item = event.target.closest('.bookmark-item')
    if (!item) return
    event.preventDefault()
    _openFile?.({ path: item.dataset.path, name: item.dataset.fileName(path) })
  })

  const footer = document.querySelector('.bookmarks-view__footer')
  footer?.addEventListener('click', onFooterClick)

  _unsubscribe?.()
  _unsubscribe = onBookmarksChange(renderBookmarks)
}

export function unmountBookmarksPanel() {
  _unsubscribe?.()
  _unsubscribe = null
}
