const STORAGE_KEY = 'fjordmark-recent-projects'
const MAX_RECENT = 10

export function getRecentProjects() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : []
  } catch {
    return []
  }
}

export function addRecentProject(folderPath) {
  const name = folderPath.split('/').pop() || folderPath.split('\\').pop() || folderPath
  const projects = getRecentProjects().filter(p => p.path !== folderPath)
  projects.unshift({ path: folderPath, name, lastOpened: new Date().toISOString() })
  if (projects.length > MAX_RECENT) projects.length = MAX_RECENT
  localStorage.setItem(STORAGE_KEY, JSON.stringify(projects))
}

export function removeRecentProject(folderPath) {
  const projects = getRecentProjects().filter(p => p.path !== folderPath)
  localStorage.setItem(STORAGE_KEY, JSON.stringify(projects))
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

  const items = projects.map(p => `
    <div class="recent-item" data-path="${p.path}" title="${p.path}">
      <span class="recent-name">${p.name}</span>
      <span class="recent-path">${p.path}</span>
      <span class="recent-time">${formatRelativeTime(p.lastOpened)}</span>
      <span class="recent-remove" data-remove-path="${p.path}">&times;</span>
    </div>
  `).join('')

  return `
    <div class="recent-projects">
      <div class="recent-header">Recent</div>
      ${items}
    </div>
  `
}
