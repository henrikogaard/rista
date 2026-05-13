const STORAGE_KEY = 'fjordmark-settings'

export const FONT_OPTIONS = {
  ui: [
    { value: "'DM Sans', system-ui, sans-serif", label: 'DM Sans' },
    { value: "system-ui, sans-serif", label: 'System Sans' },
    { value: "'Inter', system-ui, sans-serif", label: 'Inter' },
    { value: "'Space Grotesk', system-ui, sans-serif", label: 'Space Grotesk' },
    { value: "'B612', system-ui, sans-serif", label: 'B612' },
    { value: "'Nunito', system-ui, sans-serif", label: 'Nunito' },
    { value: "'Lora', Georgia, serif", label: 'Lora' },
    { value: "'Merriweather', Georgia, serif", label: 'Merriweather' },
    { value: "'Source Serif 4', Georgia, serif", label: 'Source Serif 4' },
    { value: "'Avenir Next', 'Segoe UI', sans-serif", label: 'Avenir / Segoe' },
    { value: "'IBM Plex Sans', 'Helvetica Neue', sans-serif", label: 'IBM Plex Sans' },
    { value: "'Work Sans', system-ui, sans-serif", label: 'Work Sans' },
    { value: "'Figtree', system-ui, sans-serif", label: 'Figtree' },
    { value: "Georgia, 'Times New Roman', serif", label: 'Georgia' },
  ],
  editor: [
    { value: "'DM Mono', 'Fira Mono', monospace", label: 'DM Mono' },
    { value: "'SF Mono', 'Monaco', 'Cascadia Mono', monospace", label: 'SF Mono' },
    { value: "'JetBrains Mono', 'Fira Code', monospace", label: 'JetBrains Mono' },
    { value: "'IBM Plex Mono', 'Menlo', monospace", label: 'IBM Plex Mono' },
    { value: "'Fira Code', 'SF Mono', monospace", label: 'Fira Code' },
    { value: "'Source Code Pro', 'Menlo', monospace", label: 'Source Code Pro' },
    { value: "'Cascadia Mono', 'Consolas', monospace", label: 'Cascadia Mono' },
  ],
  preview: [
    { value: "'DM Sans', system-ui, sans-serif", label: 'DM Sans' },
    { value: "system-ui, sans-serif", label: 'System Sans' },
    { value: "'Inter', system-ui, sans-serif", label: 'Inter' },
    { value: "'Space Grotesk', system-ui, sans-serif", label: 'Space Grotesk' },
    { value: "'B612', system-ui, sans-serif", label: 'B612' },
    { value: "'Nunito', system-ui, sans-serif", label: 'Nunito' },
    { value: "Georgia, 'Times New Roman', serif", label: 'Georgia' },
    { value: "'Lora', Georgia, serif", label: 'Lora' },
    { value: "'Merriweather', Georgia, serif", label: 'Merriweather' },
    { value: "'Source Serif 4', Georgia, serif", label: 'Source Serif 4' },
    { value: "'Iowan Old Style', 'Palatino Linotype', serif", label: 'Iowan / Palatino' },
  ],
  explorer: [
    { value: "'DM Sans', system-ui, sans-serif", label: 'DM Sans' },
    { value: "system-ui, sans-serif", label: 'System Sans' },
    { value: "'Inter', system-ui, sans-serif", label: 'Inter' },
    { value: "'Space Grotesk', system-ui, sans-serif", label: 'Space Grotesk' },
    { value: "'B612', system-ui, sans-serif", label: 'B612' },
    { value: "'Nunito', system-ui, sans-serif", label: 'Nunito' },
    { value: "'Lora', Georgia, serif", label: 'Lora' },
    { value: "'Merriweather', Georgia, serif", label: 'Merriweather' },
    { value: "'Source Serif 4', Georgia, serif", label: 'Source Serif 4' },
    { value: "'Avenir Next', 'Segoe UI', sans-serif", label: 'Avenir / Segoe' },
    { value: "'IBM Plex Sans', 'Helvetica Neue', sans-serif", label: 'IBM Plex Sans' },
    { value: "'Work Sans', system-ui, sans-serif", label: 'Work Sans' },
    { value: "'Figtree', system-ui, sans-serif", label: 'Figtree' },
  ],
}

export const DEFAULT_SETTINGS = {
  ambientBackground: true,
  ambientIntensity: 42,
  surfaceOpacity: 82,
  surfaceBlur: 18,
  contrastBoost: 0,
  sidebarWidth: 220,
  splitRatio: 50,
  documentSplitRatio: 50,
  textColor: '',
  mutedTextColor: '',
  subtleTextColor: '',
  accentColor: '',
  uiFont: FONT_OPTIONS.ui[0].value,
  uiFontCustom: '',
  uiFontSize: 13,
  explorerFont: FONT_OPTIONS.explorer[0].value,
  explorerFontCustom: '',
  explorerFontSize: 12,
  editorFont: FONT_OPTIONS.editor[0].value,
  editorFontCustom: '',
  editorFontSize: 13,
  editorLineHeight: 1.75,
  editorTextColor: '',
  previewFont: FONT_OPTIONS.preview[0].value,
  previewFontCustom: '',
  previewFontSize: 13,
  previewLineHeight: 1.75,
  previewTextColor: '',
  typewriterScrolling: false,
  spellcheck: false,
  vimMode: false,
  autoSaveDelay: 800,
  tabIndentation: 'spaces',
  indentWidth: 2,
  softWrap: true,
  showLineNumbers: false,
  showStatusBar: true,
  defaultViewMode: 'split',
  readingSpeed: 200,
  zenParagraphDimming: false,
  zenColumnWidth: 700,
  showMinimap: false,
}

const NUMERIC_KEYS = new Set([
  'ambientIntensity',
  'surfaceOpacity',
  'surfaceBlur',
  'contrastBoost',
  'sidebarWidth',
  'splitRatio',
  'documentSplitRatio',
  'uiFontSize',
  'explorerFontSize',
  'editorFontSize',
  'editorLineHeight',
  'previewFontSize',
  'previewLineHeight',
  'autoSaveDelay',
  'indentWidth',
  'readingSpeed',
  'zenColumnWidth',
])

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value))
}

function sanitizeColor(value) {
  const next = String(value || '').trim()
  return /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.test(next) ? next : ''
}

function hexToRgb(hex) {
  const normalized = hex.replace('#', '')
  const expanded = normalized.length === 3
    ? normalized.split('').map(char => `${char}${char}`).join('')
    : normalized

  return {
    r: Number.parseInt(expanded.slice(0, 2), 16),
    g: Number.parseInt(expanded.slice(2, 4), 16),
    b: Number.parseInt(expanded.slice(4, 6), 16),
  }
}

function mixHex(baseHex, targetHex, amount) {
  const a = hexToRgb(baseHex)
  const b = hexToRgb(targetHex)
  const t = clamp(amount, 0, 1)
  const mix = channel => Math.round(a[channel] + (b[channel] - a[channel]) * t)
  return `#${[mix('r'), mix('g'), mix('b')].map(value => value.toString(16).padStart(2, '0')).join('')}`
}

function hexToRgba(hex, alpha) {
  const { r, g, b } = hexToRgb(hex)
  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

function getThemePalette() {
  const theme = document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark'
  if (theme === 'light') {
    return {
      theme,
      text1: '#221d18',
      text2: '#5f564b',
      text3: '#938879',
      accent: '#5c7695',
    }
  }

  return {
    theme,
    text1: '#dddfe6',
    text2: '#7a7d8a',
    text3: '#484b57',
    accent: '#5b7fa6',
  }
}

function sanitize(settings) {
  const next = { ...DEFAULT_SETTINGS, ...settings }

  if (!FONT_OPTIONS.ui.some(option => option.value === next.uiFont)) next.uiFont = DEFAULT_SETTINGS.uiFont
  if (!FONT_OPTIONS.explorer.some(option => option.value === next.explorerFont)) next.explorerFont = DEFAULT_SETTINGS.explorerFont
  if (!FONT_OPTIONS.editor.some(option => option.value === next.editorFont)) next.editorFont = DEFAULT_SETTINGS.editorFont
  if (!FONT_OPTIONS.preview.some(option => option.value === next.previewFont)) next.previewFont = DEFAULT_SETTINGS.previewFont

  next.uiFontSize = clamp(Number(next.uiFontSize) || DEFAULT_SETTINGS.uiFontSize, 11, 16)
  next.ambientBackground = Boolean(next.ambientBackground)
  next.ambientIntensity = clamp(Number(next.ambientIntensity) || DEFAULT_SETTINGS.ambientIntensity, 0, 100)
  next.surfaceOpacity = clamp(Number(next.surfaceOpacity) || DEFAULT_SETTINGS.surfaceOpacity, 45, 100)
  next.surfaceBlur = clamp(Number(next.surfaceBlur) || DEFAULT_SETTINGS.surfaceBlur, 0, 32)
  next.contrastBoost = clamp(Number(next.contrastBoost) || DEFAULT_SETTINGS.contrastBoost, 0, 40)
  next.sidebarWidth = clamp(Number(next.sidebarWidth) || DEFAULT_SETTINGS.sidebarWidth, 180, 420)
  next.splitRatio = clamp(Number(next.splitRatio) || DEFAULT_SETTINGS.splitRatio, 20, 80)
  next.documentSplitRatio = clamp(Number(next.documentSplitRatio) || DEFAULT_SETTINGS.documentSplitRatio, 25, 75)
  next.explorerFontSize = clamp(Number(next.explorerFontSize) || DEFAULT_SETTINGS.explorerFontSize, 11, 16)
  next.editorFontSize = clamp(Number(next.editorFontSize) || DEFAULT_SETTINGS.editorFontSize, 12, 18)
  next.editorLineHeight = clamp(Number(next.editorLineHeight) || DEFAULT_SETTINGS.editorLineHeight, 1.4, 2.1)
  next.previewFontSize = clamp(Number(next.previewFontSize) || DEFAULT_SETTINGS.previewFontSize, 12, 18)
  next.previewLineHeight = clamp(Number(next.previewLineHeight) || DEFAULT_SETTINGS.previewLineHeight, 1.4, 2.1)
  next.textColor = sanitizeColor(next.textColor)
  next.mutedTextColor = sanitizeColor(next.mutedTextColor)
  next.subtleTextColor = sanitizeColor(next.subtleTextColor)
  next.accentColor = sanitizeColor(next.accentColor)
  next.uiFontCustom = String(next.uiFontCustom || '').trim()
  next.explorerFontCustom = String(next.explorerFontCustom || '').trim()
  next.editorFontCustom = String(next.editorFontCustom || '').trim()
  next.previewFontCustom = String(next.previewFontCustom || '').trim()
  next.editorTextColor = sanitizeColor(next.editorTextColor)
  next.previewTextColor = sanitizeColor(next.previewTextColor)
  next.typewriterScrolling = Boolean(next.typewriterScrolling)
  next.spellcheck = Boolean(next.spellcheck)
  next.vimMode = Boolean(next.vimMode)
  next.autoSaveDelay = clamp(Number(next.autoSaveDelay) || 800, 200, 5000)
  next.tabIndentation = ['spaces', 'tabs'].includes(next.tabIndentation) ? next.tabIndentation : 'spaces'
  next.indentWidth = [2, 4, 8].includes(Number(next.indentWidth)) ? Number(next.indentWidth) : 2
  next.softWrap = next.softWrap !== false
  next.showLineNumbers = Boolean(next.showLineNumbers)
  next.showStatusBar = next.showStatusBar !== false
  next.defaultViewMode = ['markdown', 'split', 'preview'].includes(next.defaultViewMode) ? next.defaultViewMode : 'split'
  next.readingSpeed = clamp(Number(next.readingSpeed) || 200, 100, 500)
  next.zenParagraphDimming = Boolean(next.zenParagraphDimming)
  next.zenColumnWidth = clamp(Number(next.zenColumnWidth) || 700, 500, 900)
  next.showMinimap = Boolean(next.showMinimap)

  return next
}

export function getSettings() {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return { ...DEFAULT_SETTINGS }
    return sanitize(JSON.parse(raw))
  } catch {
    return { ...DEFAULT_SETTINGS }
  }
}

export function applySettings(settings = getSettings()) {
  const next = sanitize(settings)
  const root = document.documentElement
  const palette = getThemePalette()
  const contrast = next.contrastBoost / 100
  const contrastTarget = palette.theme === 'light' ? '#000000' : '#ffffff'
  const baseText1 = mixHex(palette.text1, contrastTarget, contrast * 0.58)
  const baseText2 = mixHex(palette.text2, contrastTarget, contrast * 0.46)
  const baseText3 = mixHex(palette.text3, contrastTarget, contrast * 0.34)
  const accent = next.accentColor || palette.accent
  const accentHi = palette.theme === 'light'
    ? mixHex(accent, '#000000', 0.2)
    : mixHex(accent, '#ffffff', 0.22)
  const accentDim = hexToRgba(accent, palette.theme === 'light' ? 0.14 : 0.16)
  const text1 = next.textColor || baseText1
  const text2 = next.mutedTextColor || baseText2
  const text3 = next.subtleTextColor || baseText3

  root.style.setProperty('--ambient-opacity', next.ambientBackground ? String(next.ambientIntensity / 100) : '0')
  root.style.setProperty('--surface-opacity', String(next.surfaceOpacity / 100))
  root.style.setProperty('--surface-blur', `${next.surfaceBlur}px`)
  root.style.setProperty('--surface-bg-0', `rgba(var(--bg0-rgb), ${Math.max(0.24, next.surfaceOpacity / 100 - 0.18).toFixed(2)})`)
  root.style.setProperty('--surface-bg-1', `rgba(var(--bg1-rgb), ${Math.max(0.28, next.surfaceOpacity / 100 - 0.12).toFixed(2)})`)
  root.style.setProperty('--surface-bg-2', `rgba(var(--bg2-rgb), ${Math.max(0.32, next.surfaceOpacity / 100 - 0.08).toFixed(2)})`)
  root.style.setProperty('--surface-bg-3', `rgba(var(--bg3-rgb), ${Math.max(0.38, next.surfaceOpacity / 100 - 0.02).toFixed(2)})`)
  root.style.setProperty('--surface-edge', `rgba(255,255,255, ${Math.max(0.03, next.surfaceOpacity / 100 * 0.09).toFixed(2)})`)
  root.style.setProperty('--text1', text1)
  root.style.setProperty('--text2', text2)
  root.style.setProperty('--text3', text3)
  root.style.setProperty('--accent', accent)
  root.style.setProperty('--accent-hi', accentHi)
  root.style.setProperty('--accent-dim', accentDim)
  root.style.setProperty('--editor-text-color', next.editorTextColor || text1)
  root.style.setProperty('--preview-text-color', next.previewTextColor || text1)
  root.style.setProperty('--preview-muted-color', next.previewTextColor || text2)
  root.style.setProperty('--preview-heading-color', next.previewTextColor || text1)
  root.style.setProperty('--sidebar-width', `${next.sidebarWidth}px`)
  root.style.setProperty('--split-ratio', `${next.splitRatio}%`)
  root.style.setProperty('--document-split-ratio', `${next.documentSplitRatio}%`)
  root.style.setProperty('--ui-font', next.uiFontCustom || next.uiFont)
  root.style.setProperty('--font', next.uiFontCustom || next.uiFont)
  root.style.setProperty('--ui-font-size', `${next.uiFontSize}px`)
  root.style.setProperty('--explorer-font', next.explorerFontCustom || next.explorerFont)
  root.style.setProperty('--explorer-font-size', `${next.explorerFontSize}px`)
  root.style.setProperty('--editor-font', next.editorFontCustom || next.editorFont)
  root.style.setProperty('--editor-font-size', `${next.editorFontSize}px`)
  root.style.setProperty('--editor-line-height', String(next.editorLineHeight))
  root.style.setProperty('--preview-font', next.previewFontCustom || next.previewFont)
  root.style.setProperty('--preview-font-size', `${next.previewFontSize}px`)
  root.style.setProperty('--preview-line-height', String(next.previewLineHeight))
  return next
}

export function setSettings(partial) {
  const next = sanitize({ ...getSettings(), ...partial })
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  applySettings(next)
  return next
}

export function updateSetting(key, value) {
  const nextValue = NUMERIC_KEYS.has(key) ? Number(value) : value
  return setSettings({ [key]: nextValue })
}

export function resetSettings() {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(DEFAULT_SETTINGS))
  return applySettings(DEFAULT_SETTINGS)
}
