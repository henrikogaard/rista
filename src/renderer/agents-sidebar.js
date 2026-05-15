import { state, $ } from './state.js'

// ── Agents Sidebar ───────────────────────────────────────────────
// Local session history stored in .fjordmark/sessions/

const SESSIONS_DIR = '.fjordmark/sessions'

export function buildAgentsSidebar() {
  return `
    <div class="agents-sidebar" id="agents-sidebar" style="display:none">
      <div class="agents-sidebar__header">
        <span class="agents-sidebar__label">Agents</span>
        <div class="agents-sidebar__new" id="agents-new-btn" title="New session" role="button" tabindex="0">+</div>
      </div>
      <div class="agents-list" id="agents-list"></div>
      <div class="agents-empty" id="agents-empty">
        <span>No sessions yet</span>
        <p class="agents-empty__sub">Sessions are stored locally in your project.</p>
      </div>
    </div>
  `
}

export async function loadSessions() {
  if (!state.folderPath) return []
  const sessionsPath = state.folderPath + '/' + SESSIONS_DIR
  try {
    const files = await window.fjord.readFolder(sessionsPath)
    const sessions = []
    for (const file of files) {
      if (file.type === 'file' && file.name.endsWith('.json')) {
        try {
          const content = await window.fjord.readFile(file.path)
          const data = JSON.parse(content)
          sessions.push({ ...data, path: file.path })
        } catch { /* ignore invalid */ }
      }
    }
    return sessions.sort((a, b) => (b.updatedAt || 0) - (a.updatedAt || 0))
  } catch {
    return []
  }
}

export async function createSession(title = 'New Session') {
  if (!state.folderPath) return null
  const sessionsDir = state.folderPath + '/' + SESSIONS_DIR
  await window.fjord.createDir(sessionsDir)
  const id = 'session-' + Date.now()
  const filePath = sessionsDir + '/' + id + '.json'
  const session = {
    id,
    title,
    model: 'local',
    createdAt: Date.now(),
    updatedAt: Date.now(),
    messages: [],
  }
  await window.fjord.writeFile(filePath, JSON.stringify(session, null, 2))
  return session
}

export async function renderAgentsList() {
  const list = $('agents-list')
  const empty = $('agents-empty')
  if (!list || !empty) return
  const sessions = await loadSessions()
  if (!sessions.length) {
    list.style.display = 'none'
    empty.style.display = 'flex'
    return
  }
  list.style.display = 'flex'
  empty.style.display = 'none'
  list.innerHTML = sessions.map(s => `
    <div class="agent-card" data-session-path="${escapeAttr(s.path)}">
      <div class="agent-card__title">${escapeHtml(s.title)}</div>
      <div class="agent-card__meta">
        <span class="agent-card__model">${escapeHtml(s.model || 'local')}</span>
        <span class="agent-card__time">${formatTime(s.updatedAt)}</span>
      </div>
    </div>
  `).join('')
}

export function toggleSidebarMode() {
  const current = state.sidebarMode || 'explorer'
  state.sidebarMode = current === 'explorer' ? 'agents' : 'explorer'
  const fileTree = $('file-tree')
  const agentsSidebar = $('agents-sidebar')
  const sidebarLabel = $('sidebar-label')
  const toggleBtn = $('sidebar-mode-toggle')
  if (fileTree) fileTree.style.display = state.sidebarMode === 'explorer' ? 'block' : 'none'
  if (agentsSidebar) agentsSidebar.style.display = state.sidebarMode === 'agents' ? 'flex' : 'none'
  if (sidebarLabel) sidebarLabel.textContent = state.sidebarMode === 'explorer' ? 'Explorer' : 'Agents'
  if (toggleBtn) toggleBtn.classList.toggle('active', state.sidebarMode === 'agents')
  if (state.sidebarMode === 'agents') renderAgentsList()
}

function formatTime(ts) {
  if (!ts) return ''
  const d = new Date(ts)
  const now = new Date()
  const diff = now - d
  if (diff < 60000) return 'just now'
  if (diff < 3600000) return Math.floor(diff / 60000) + 'm ago'
  if (diff < 86400000) return Math.floor(diff / 3600000) + 'h ago'
  return d.toLocaleDateString()
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function escapeAttr(text) {
  return String(text)
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}
