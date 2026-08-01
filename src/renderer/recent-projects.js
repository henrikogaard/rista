const STORAGE_KEY = 'rista-recent-projects'
const PINNED_STORAGE_KEY = 'rista-pinned-projects'
const MAX_RECENT = 10

function escapeHtml(value) {
  return String(value || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function projectName(folderPath) {
  return String(folderPath || '').split(/[\\/]/).filter(Boolean).pop() || folderPath
}

function projectFromPath(folderPath, extra = {}) {
  return {
    path: folderPath,
    name: projectName(folderPath),
    ...extra,
  }
}

export function getRecentProjects() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : []
  } catch {
    return []
  }
}

export function addRecentProject(folderPath) {
  const projects = getRecentProjects().filter(p => p.path !== folderPath)
  projects.unshift(projectFromPath(folderPath, { lastOpened: new Date().toISOString() }))
  if (projects.length > MAX_RECENT) projects.length = MAX_RECENT
  localStorage.setItem(STORAGE_KEY, JSON.stringify(projects))
}

export function removeRecentProject(folderPath) {
  const projects = getRecentProjects().filter(p => p.path !== folderPath)
  localStorage.setItem(STORAGE_KEY, JSON.stringify(projects))
}

export function getPinnedProjects() {
  try {
    const raw = localStorage.getItem(PINNED_STORAGE_KEY)
    const pinned = raw ? JSON.parse(raw) : []
    return Array.isArray(pinned) ? pinned.filter(p => p?.path) : []
  } catch {
    return []
  }
}

export function isPinnedProject(folderPath) {
  return getPinnedProjects().some(p => p.path === folderPath)
}

export function pinProject(folderPath) {
  if (!folderPath) return
  const recent = getRecentProjects().find(p => p.path === folderPath)
  const pinned = getPinnedProjects().filter(p => p.path !== folderPath)
  pinned.unshift(projectFromPath(folderPath, {
    name: recent?.name || projectName(folderPath),
    pinnedAt: new Date().toISOString(),
  }))
  localStorage.setItem(PINNED_STORAGE_KEY, JSON.stringify(pinned))
}

export function unpinProject(folderPath) {
  const pinned = getPinnedProjects().filter(p => p.path !== folderPath)
  localStorage.setItem(PINNED_STORAGE_KEY, JSON.stringify(pinned))
}

export function togglePinnedProject(folderPath) {
  if (isPinnedProject(folderPath)) {
    unpinProject(folderPath)
    return false
  }
  pinProject(folderPath)
  return true
}

export function formatRelativeTime(isoString) {
  const diff = Date.now() - new Date(isoString).getTime()
  const minutes = Math.floor(diff / 60000)
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days}d ago`
  return `${Math.floor(days / 30)}mo ago`
}

export function renderRecentProjectsHtml() {
  const projects = getRecentProjects()
  if (!projects.length) return ''
  const pinnedPaths = new Set(getPinnedProjects().map(p => p.path))

  const items = projects.map(p => `
    <div class="recent-item" data-path="${escapeHtml(p.path)}" title="${escapeHtml(p.path)}">
      <span class="recent-name">${escapeHtml(p.name)}</span>
      <span class="recent-path">${escapeHtml(p.path)}</span>
      <span class="recent-time">${formatRelativeTime(p.lastOpened)}</span>
      <span class="recent-pin${pinnedPaths.has(p.path) ? ' active' : ''}" data-pin-path="${escapeHtml(p.path)}" title="${pinnedPaths.has(p.path) ? 'Unpin workspace' : 'Pin workspace'}" role="button" tabindex="0">Pin</span>
      <span class="recent-remove" data-remove-path="${escapeHtml(p.path)}" title="Remove from recent" role="button" tabindex="0">&times;</span>
    </div>
  `).join('')

  return `
    <div class="recent-projects">
      <div class="recent-header">Recent</div>
      ${items}
    </div>
  `
}

export function renderPinnedProjectsHtml({ empty = false } = {}) {
  const projects = getPinnedProjects()
  if (!projects.length) {
    return empty ? '<div class="pinned-empty">Pinned workspaces will appear here after you pin them from the welcome screen.</div>' : ''
  }

  const items = projects.map(p => `
    <div class="pinned-item" data-path="${escapeHtml(p.path)}" title="${escapeHtml(p.path)}">
      <div class="pinned-item__copy">
        <div class="pinned-item__name">${escapeHtml(p.name)}</div>
        <div class="pinned-item__path">${escapeHtml(p.path)}</div>
      </div>
      <div class="pinned-item__actions">
        <div class="settings-btn settings-btn--muted pinned-open" data-open-path="${escapeHtml(p.path)}" role="button" tabindex="0">Open</div>
        <div class="settings-btn settings-btn--muted pinned-open-new" data-open-new-path="${escapeHtml(p.path)}" role="button" tabindex="0">New window</div>
        <div class="settings-btn settings-btn--muted pinned-unpin" data-unpin-path="${escapeHtml(p.path)}" role="button" tabindex="0">Unpin</div>
      </div>
    </div>
  `).join('')

  return `<div class="pinned-list">${items}</div>`
}
