const PREFIX = 'rista-session-'

function hashPath(folderPath) {
  let hash = 0
  for (let i = 0; i < folderPath.length; i++) {
    const char = folderPath.charCodeAt(i)
    hash = ((hash << 5) - hash) + char
    hash |= 0
  }
  return Math.abs(hash).toString(36)
}

export function saveSession(folderPath, sessionData) {
  if (!folderPath) return
  const key = PREFIX + hashPath(folderPath)
  try {
    localStorage.setItem(key, JSON.stringify({
      ...sessionData,
      savedAt: new Date().toISOString(),
    }))
  } catch {}
}

export function loadSession(folderPath) {
  if (!folderPath) return null
  const key = PREFIX + hashPath(folderPath)
  try {
    const raw = localStorage.getItem(key)
    return raw ? JSON.parse(raw) : null
  } catch {
    return null
  }
}

export function clearSession(folderPath) {
  if (!folderPath) return
  const key = PREFIX + hashPath(folderPath)
  localStorage.removeItem(key)
}
