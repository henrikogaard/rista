// ── Unsaved Buffer Recovery (Task 3.6) ─────────────────────────
// Periodically saves dirty editor buffers to localStorage so they
// can be restored after a crash or unexpected relaunch.
//
// - saveRecoveryBuffer() saves the dirtiest tab's content
// - hasRecoveryBuffer() checks if recovery data exists
// - loadRecoveryBuffer() returns the recovery data then clears it
// - schedulePeriodicSave() starts the periodic save timer

const STORAGE_KEY = 'rista-crash-recovery'

let _periodicTimer = null

export function saveRecoveryBuffer(tabs) {
  if (!tabs || !tabs.length) return
  const dirtyTabs = tabs.filter(t => t.dirty && t.path && t.content)
  if (!dirtyTabs.length) return

  try {
    const data = dirtyTabs.map(t => ({
      path: t.path,
      name: t.name,
      content: t.content,
      savedAt: Date.now(),
    }))
    localStorage.setItem(STORAGE_KEY, JSON.stringify(data))
  } catch {
    // Storage full or unavailable — silently skip
  }
}



export function hasRecoveryBuffer() {
  try {
    return localStorage.getItem(STORAGE_KEY) !== null
  } catch {
    return false
  }
}

export function loadRecoveryBuffer() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    const data = JSON.parse(raw)
    localStorage.removeItem(STORAGE_KEY)
    return data
  } catch {
    localStorage.removeItem(STORAGE_KEY)
    return null
  }
}

export function schedulePeriodicSave(getTabsFn) {
  clearPeriodicSave()
  _periodicTimer = setInterval(() => {
    const tabs = getTabsFn()
    saveRecoveryBuffer(tabs)
  }, 30000) // every 30 seconds
}

export function clearPeriodicSave() {
  if (_periodicTimer) {
    clearInterval(_periodicTimer)
    _periodicTimer = null
  }
}
