// Theme manager — persists to localStorage
const STORAGE_KEY = 'fjordmark-theme'

export function getTheme() {
  return localStorage.getItem(STORAGE_KEY) || 'dark'
}

export function setTheme(theme) {
  localStorage.setItem(STORAGE_KEY, theme)
  document.documentElement.setAttribute('data-theme', theme)
}

export function toggleTheme() {
  const next = getTheme() === 'dark' ? 'light' : 'dark'
  setTheme(next)
  return next
}

export function initTheme() {
  setTheme(getTheme())
}
