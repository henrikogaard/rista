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
  'quick-open': 'Mod+P',
  'command-palette': 'Mod+K',
  'zen-mode': 'Mod+Shift+Enter',
  'daily-note': 'Mod+Shift+D',
}

const bindingLabels = {
  'save': 'Save',
  'save-as': 'Save as',
  'new-file': 'New file',
  'toggle-sidebar': 'Toggle file explorer',
  'toggle-toolbar': 'Toggle toolbar',
  'find-replace': 'Find and replace',
  'project-search': 'Project search',
  'terminal': 'Toggle terminal',
  'settings': 'Open settings',
  'quick-open': 'Quick open',
  'command-palette': 'Command palette',
  'zen-mode': 'Zen mode',
  'daily-note': 'Daily note',
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
      label: bindingLabels[id] || id,
      default: defaultBindings[id],
      current: overrides[id] || defaultBindings[id],
      isOverridden: !!overrides[id],
    }
  }
  return all
}

export function formatKeyEvent(event) {
  const key = normalizeKey(event.key)
  if (!key) return null

  const parts = []
  if (event.metaKey || event.ctrlKey) parts.push('Mod')
  if (event.altKey) parts.push('Alt')
  if (event.shiftKey) parts.push('Shift')
  parts.push(key)
  return parts.join('+')
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

function normalizeKey(key) {
  if (!key) return null
  const value = String(key)
  const lower = value.toLowerCase()
  if (['meta', 'control', 'ctrl', 'alt', 'shift'].includes(lower)) return null
  if (lower === ' ') return 'Space'
  if (lower === 'escape') return 'Escape'
  if (lower === 'enter') return 'Enter'
  if (lower === 'tab') return 'Tab'
  if (lower === 'backspace') return 'Backspace'
  if (lower === 'delete') return 'Delete'
  if (lower.startsWith('arrow')) return value[0].toUpperCase() + value.slice(1)
  return value.length === 1 ? value.toUpperCase() : value[0].toUpperCase() + value.slice(1)
}
