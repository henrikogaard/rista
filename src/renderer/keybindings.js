const STORAGE_KEY = 'fjordmark-keybindings'

const defaultBindings = {
  'save': 'Mod+S',
  'save-as': 'Mod+Shift+S',
  'new-file': 'Mod+N',
  'toggle-sidebar': 'Mod+B',
  'toggle-toolbar': 'Mod+\\',
  'find-replace': 'Mod+F',
  'project-search': 'Mod+Shift+F',
  'terminal': 'Mod+J',
  'settings': 'Mod+,',
  'command-palette': 'Mod+K',
  'zen-mode': 'Mod+Shift+Enter',
}

let overrides = {}

export function initKeybindings() {
  try {
    overrides = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
  } catch {
    overrides = {}
  }
}

export function getBinding(id) {
  return overrides[id] || defaultBindings[id] || null
}

export function setBinding(id, keys) {
  overrides[id] = keys
  localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides))
}

export function resetBinding(id) {
  delete overrides[id]
  localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides))
}

export function getAllBindings() {
  const all = {}
  for (const id of Object.keys(defaultBindings)) {
    all[id] = {
      id,
      default: defaultBindings[id],
      current: overrides[id] || defaultBindings[id],
      isOverridden: !!overrides[id],
    }
  }
  return all
}

export function findConflict(id, keys) {
  for (const [otherId, otherKeys] of Object.entries({ ...defaultBindings, ...overrides })) {
    if (otherId !== id && otherKeys === keys) return otherId
  }
  return null
}

export function matchesBinding(e, id) {
  const binding = getBinding(id)
  if (!binding) return false
  return matchesKeyCombo(e, binding)
}

function matchesKeyCombo(e, combo) {
  const parts = combo.split('+').map(p => p.trim().toLowerCase())
  const needsMod = parts.includes('mod') || parts.includes('cmd') || parts.includes('ctrl')
  const needsShift = parts.includes('shift')
  const needsAlt = parts.includes('alt')
  const key = parts.filter(p => !['mod', 'cmd', 'ctrl', 'shift', 'alt'].includes(p))[0]

  if (needsMod && !(e.metaKey || e.ctrlKey)) return false
  if (!needsMod && (e.metaKey || e.ctrlKey)) return false
  if (needsShift && !e.shiftKey) return false
  if (needsAlt && !e.altKey) return false

  const eventKey = e.key.toLowerCase()
  return eventKey === key || (key === 'enter' && eventKey === 'enter') || (key === '\\' && eventKey === '\\')
}
