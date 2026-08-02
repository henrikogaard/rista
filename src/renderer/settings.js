const STORAGE_KEY = 'rista-settings'
const AI_PROVIDER_KEYS = [
  'openai',
  'anthropic',
  'openrouter',
  'ollama',
  'custom-openai-compatible',
  'opencode-go',
  'opencode-zen',
]

export const APP_ICON_VARIANTS = [
  { value: 'nordic-steel', label: 'Nordic Steel' },
  { value: 'aurora-gradient', label: 'Aurora Gradient' },
  { value: 'black-stone', label: 'Black Stone' },
  { value: 'paper-ink', label: 'Paper Ink' },
  { value: 'future-rune', label: 'Future Rune' },
]

export const ASSISTANT_DOCK_OPTIONS = [
  { value: 'hidden', label: 'Off (hidden by default)' },
  { value: 'right-sidebar', label: 'Right sidebar' },
  { value: 'left-sidebar', label: 'Left sidebar' },
  { value: 'right-rail', label: 'Dedicated right rail' },
  { value: 'left-rail', label: 'Dedicated left rail' },
]

const PROPORTIONAL_FONT_OPTIONS = [
  { value: "'DM Sans', system-ui, sans-serif", label: 'DM Sans' },
  { value: "system-ui, sans-serif", label: 'System Sans' },
  { value: "Georgia, 'Times New Roman', serif", label: 'System Serif' },
  { value: "'Inter', system-ui, sans-serif", label: 'Inter' },
  { value: "'Space Grotesk', system-ui, sans-serif", label: 'Space Grotesk' },
  { value: "'B612', system-ui, sans-serif", label: 'B612' },
  { value: "'Nunito', system-ui, sans-serif", label: 'Nunito' },
  { value: "'Lora', Georgia, serif", label: 'Lora' },
  { value: "'Merriweather', Georgia, serif", label: 'Merriweather' },
  { value: "'Source Serif 4', Georgia, serif", label: 'Source Serif 4' },
  { value: "'IBM Plex Sans', 'Helvetica Neue', sans-serif", label: 'IBM Plex Sans' },
  { value: "'Work Sans', system-ui, sans-serif", label: 'Work Sans' },
  { value: "'Figtree', system-ui, sans-serif", label: 'Figtree' },
]

const MONO_FONT_OPTIONS = [
  { value: "'DM Mono', 'Fira Mono', monospace", label: 'DM Mono' },
  { value: "'SF Mono', 'Monaco', 'Cascadia Mono', monospace", label: 'System Mono' },
  { value: "'JetBrains Mono', 'Fira Code', monospace", label: 'JetBrains Mono' },
  { value: "'IBM Plex Mono', 'Menlo', monospace", label: 'IBM Plex Mono' },
  { value: "'Fira Code', 'SF Mono', monospace", label: 'Fira Code' },
  { value: "'Source Code Pro', 'Menlo', monospace", label: 'Source Code Pro' },
]

export const FONT_OPTIONS = {
  ui: PROPORTIONAL_FONT_OPTIONS,
  explorer: PROPORTIONAL_FONT_OPTIONS,
  preview: PROPORTIONAL_FONT_OPTIONS,
  editor: [PROPORTIONAL_FONT_OPTIONS[0], ...MONO_FONT_OPTIONS],
}

export const THEME_PRESETS = {
  dark: [
  {
      value: 'nordic-night',
      label: 'Nordic Night',
      bg: ['#0d0e10', '#111214', '#161719', '#1c1d20', '#252729', '#2e3033'],
      text: ['#dddfe6', '#8e91a0', '#5a5e6e'],
      accent: '#5b7fa6',
      green: '#4a9966',
      red: '#c0504d',
      amber: '#c8903a',
      atmosphere: { ambientIntensity: 42, surfaceOpacity: 82, surfaceBlur: 18 },
    },
    {
      value: 'deep-fjord',
      label: 'Deep Fjord',
      bg: ['#071013', '#0b171b', '#102126', '#162b31', '#203940', '#2b4850'],
      text: ['#d9e7e8', '#7e969e', '#415962'],
      accent: '#4d8fa3',
      green: '#4e9a72',
      red: '#bc5a55',
      amber: '#c58d3e',
      atmosphere: { ambientIntensity: 50, surfaceOpacity: 84, surfaceBlur: 20 },
    },
    {
      value: 'graphite',
      label: 'Graphite',
      bg: ['#0c0c0d', '#121214', '#19191b', '#202124', '#2a2b2f', '#34363b'],
      text: ['#e2e2e5', '#8c8d94', '#50515a'],
      accent: '#7a8798',
      green: '#65936f',
      red: '#bd5d58',
      amber: '#b88b45',
      atmosphere: { ambientIntensity: 24, surfaceOpacity: 88, surfaceBlur: 14 },
    },
    {
      value: 'gruvbox-dark',
      label: 'Gruvbox Dark',
      bg: ['#1d2021', '#282828', '#32302f', '#3c3836', '#504945', '#665c54'],
      text: ['#ebdbb2', '#b09c8a', '#7c6f64'],
      accent: '#d79921',
      green: '#98971a',
      red: '#cc241d',
      amber: '#d65d0e',
      atmosphere: { ambientIntensity: 24, surfaceOpacity: 88, surfaceBlur: 12 },
    },
    {
      value: 'everforest-dark',
      label: 'Everforest Dark',
      bg: ['#1e2326', '#272e33', '#2e383c', '#374145', '#414b50', '#4f5b58'],
      text: ['#d3c6aa', '#a5b1a8', '#7a8478'],
      accent: '#7fbbb3',
      green: '#a7c080',
      red: '#e67e80',
      amber: '#dbbc7f',
      atmosphere: { ambientIntensity: 32, surfaceOpacity: 86, surfaceBlur: 16 },
    },
    {
      value: 'ayu-dark',
      label: 'Ayu Dark',
      bg: ['#0b0e14', '#11151c', '#151a23', '#1b212c', '#242b38', '#303746'],
      text: ['#bfbdb6', '#7e8590', '#4d5560'],
      accent: '#ffb454',
      green: '#aad94c',
      red: '#f07178',
      amber: '#ff8f40',
      atmosphere: { ambientIntensity: 30, surfaceOpacity: 86, surfaceBlur: 14 },
    },
    {
      value: 'ayu-mirage',
      label: 'Ayu Mirage',
      bg: ['#171b24', '#1f2430', '#242936', '#2a3040', '#343d4f', '#414b60'],
      text: ['#cccac2', '#9299a1', '#5c6570'],
      accent: '#ffcc66',
      green: '#bbe67e',
      red: '#f28779',
      amber: '#ffd580',
      atmosphere: { ambientIntensity: 36, surfaceOpacity: 85, surfaceBlur: 18 },
    },
    {
      value: 'pine',
      label: 'Pine',
      bg: ['#0b100d', '#101813', '#172119', '#1e2a21', '#29382d', '#35483b'],
      text: ['#dce6dd', '#849585', '#4b5b50'],
      accent: '#6f9479',
      green: '#5ba06b',
      red: '#bc5e55',
      amber: '#c59a45',
      atmosphere: { ambientIntensity: 38, surfaceOpacity: 84, surfaceBlur: 18 },
    },
    {
      value: 'aubergine',
      label: 'Aubergine',
      bg: ['#110d13', '#18121b', '#211828', '#2a2033', '#382b43', '#463754'],
      text: ['#e6dde9', '#9789a2', '#5b4b66'],
      accent: '#8c78ad',
      green: '#60966f',
      red: '#c16068',
      amber: '#c49250',
      atmosphere: { ambientIntensity: 46, surfaceOpacity: 83, surfaceBlur: 20 },
    },
    {
      value: 'ember',
      label: 'Ember',
      bg: ['#171512', '#1b1916', '#201d19', '#29241f', '#342d26', '#43382e'],
      text: ['#e8e0d8', '#a39a91', '#6f6861'],
      accent: '#d9824f',
      green: '#789b72',
      red: '#c56b5b',
      amber: '#c99558',
      atmosphere: { ambientIntensity: 0, surfaceOpacity: 100, surfaceBlur: 0 },
    },
  ],
  light: [
    {
      value: 'nordic-paper',
      label: 'Nordic Paper',
      bg: ['#e9e6df', '#e2ddd4', '#d8d1c7', '#ccc3b7', '#b8aea0', '#a69b8c'],
      text: ['#221d18', '#454038', '#938879'],
      accent: '#5c7695',
      green: '#527a5b',
      red: '#a14e4a',
      amber: '#9a7032',
      atmosphere: { ambientIntensity: 30, surfaceOpacity: 88, surfaceBlur: 16 },
    },
    {
      value: 'snow',
      label: 'Snow',
      bg: ['#f4f6f5', '#edf1f0', '#e4e9e8', '#d7dfdd', '#c6d0ce', '#b6c2bf'],
      text: ['#17201f', '#3b4745', '#879592'],
      accent: '#587d8d',
      green: '#4f7f62',
      red: '#a45750',
      amber: '#99753a',
      atmosphere: { ambientIntensity: 22, surfaceOpacity: 92, surfaceBlur: 12 },
    },
    {
      value: 'gruvbox-light',
      label: 'Gruvbox Light',
      bg: ['#fbf1c7', '#f2e5bc', '#ebdbb2', '#d5c4a1', '#bdae93', '#a89984'],
      text: ['#3c3836', '#524a42', '#928374'],
      accent: '#b57614',
      green: '#79740e',
      red: '#9d0006',
      amber: '#af3a03',
      atmosphere: { ambientIntensity: 22, surfaceOpacity: 92, surfaceBlur: 12 },
    },
    {
      value: 'everforest-light',
      label: 'Everforest Light',
      bg: ['#f3ead3', '#efdfc0', '#e6d5b8', '#d8caac', '#c8b99a', '#b9aa8d'],
      text: ['#5c6a72', '#5a6870', '#939f91'],
      accent: '#3a94c5',
      green: '#8da101',
      red: '#f85552',
      amber: '#dfa000',
      atmosphere: { ambientIntensity: 28, surfaceOpacity: 90, surfaceBlur: 14 },
    },
    {
      value: 'ayu-light',
      label: 'Ayu Light',
      bg: ['#fafafa', '#f3f4f5', '#e7e8ea', '#d8d9dc', '#c7c9cc', '#b8bac0'],
      text: ['#5c6166', '#6a727d', '#abb0b6'],
      accent: '#ff9940',
      green: '#86b300',
      red: '#f07171',
      amber: '#f2ae49',
      atmosphere: { ambientIntensity: 18, surfaceOpacity: 92, surfaceBlur: 10 },
    },
    {
      value: 'warm-linen',
      label: 'Warm Linen',
      bg: ['#efe7dc', '#e7dccd', '#ddd0be', '#d1c0aa', '#bfac93', '#ae9a81'],
      text: ['#251d15', '#574c40', '#9b8977'],
      accent: '#8a6f4f',
      green: '#617c55',
      red: '#a85a4d',
      amber: '#a27638',
      atmosphere: { ambientIntensity: 28, surfaceOpacity: 90, surfaceBlur: 16 },
    },
    {
      value: 'mist',
      label: 'Mist',
      bg: ['#e6e9ea', '#dde2e4', '#d2d9dc', '#c4cdd1', '#b1bdc2', '#a1afb5'],
      text: ['#1a2023', '#434e52', '#839096'],
      accent: '#5d748f',
      green: '#577d67',
      red: '#9d5857',
      amber: '#96743d',
      atmosphere: { ambientIntensity: 34, surfaceOpacity: 88, surfaceBlur: 18 },
    },
    {
      value: 'sage',
      label: 'Sage',
      bg: ['#e4e7dc', '#dce1d3', '#d2d8c6', '#c3ccb5', '#b1bda1', '#9fac90'],
      text: ['#1d2318', '#4d5744', '#8b967d'],
      accent: '#667f5d',
      green: '#5f835e',
      red: '#9f5b50',
      amber: '#92773a',
      atmosphere: { ambientIntensity: 26, surfaceOpacity: 90, surfaceBlur: 14 },
    },
    {
      value: 'ember-paper',
      label: 'Ember Paper',
      bg: ['#f0e9df', '#e9e0d2', '#ddd0bd', '#cdbda4', '#b6a285', '#a28c6d'],
      text: ['#241d15', '#574a38', '#97876f'],
      accent: '#c07a2d',
      green: '#6f8f52',
      red: '#a85a48',
      amber: '#b5742f',
      atmosphere: { ambientIntensity: 30, surfaceOpacity: 90, surfaceBlur: 16 },
    },
  ],
}

function getPreset(theme, value) {
  return THEME_PRESETS[theme].find(preset => preset.value === value) || THEME_PRESETS[theme][0]
}

export const DEFAULT_SETTINGS = {
  ambientBackground: false,
  ambientIntensity: 0,
  surfaceOpacity: 100,
  surfaceBlur: 0,
  contrastBoost: 22,
  sidebarWidth: 236,
  splitRatio: 50,
  documentSplitRatio: 50,
  textColor: '',
  mutedTextColor: '',
  subtleTextColor: '',
  accentColor: '',
  uiFont: FONT_OPTIONS.ui[0].value,
  uiFontCustom: '',
  uiFontSize: 12,
  explorerFont: FONT_OPTIONS.explorer[0].value,
  explorerFontCustom: '',
  explorerFontSize: 12,
  editorFont: FONT_OPTIONS.editor[0].value,
  editorFontCustom: '',
  editorFontSize: 15,
  editorLineHeight: 1.75,
  editorTextColor: '',
  previewFont: FONT_OPTIONS.preview[0].value,
  previewFontCustom: '',
  previewFontSize: 16,
  previewLineHeight: 1.8,
  previewMirrorEditor: false,
  previewTextColor: '',
  hideFrontmatterInRenderedModes: true,
  showDocumentBanners: true,
  typewriterScrolling: false,
  spellcheck: false,
  vimMode: false,
  darkThemePreset: 'ember',
  lightThemePreset: 'ember-paper',
  autoSaveDelay: 800,
  tabIndentation: 'spaces',
  indentWidth: 2,
  softWrap: true,
  showLineNumbers: false,
  showStatusBar: true,
  // ── Feature flags (#64) ──────────────────────────────────────────
  // Non-core modules: all off by default. Core (editor, preview, tree,
  // command palette, find/replace, export, themes, settings) stays on.
  showExperimental: false,
  featureGraphView: false,
  featureCalendar: false,
  featureBookmarks: false,
  featureTags: false,
  featureAgents: false,
  featureProperties: false,
  featureInspector: false,
  featureWikilinks: false,
  featureSemanticIndex: false,
  featureWikiQuality: false,
  featureRelatedNotes: false,
  featureDiagramBuilder: false,
  featureTerminal: false,
  featurePublish: false,
  zenParagraphDimming: false,
  defaultViewMode: 'markdown',
  readingSpeed: 200,
  zenColumnWidth: 700,
  showMinimap: false,
  smartTypography: true,
  focusMode: false,
  livePreview: true,
  posHighlight: false,
  docxExportEnabled: false,
  dailyNotesFolder: 'daily',
  dailyNoteTemplate: '# {{date}}\n\n',
  maxHistorySnapshots: 50,
  appIconVariant: 'aurora-gradient',
  appIconTheme: 'auto',
  aiProvider: 'openai',
  assistantDock: 'hidden',
  aiApiKey: '',
  aiModel: '',
  aiBaseUrl: '',
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
  'maxHistorySnapshots',
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

function getThemePalette(settings) {
  const theme = document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark'
  const preset = getPreset(theme, theme === 'light' ? settings.lightThemePreset : settings.darkThemePreset)
  return {
    theme,
    preset,
    text1: preset.text[0],
    text2: preset.text[1],
    text3: preset.text[2],
    accent: preset.accent,
  }
}

function setHexRgb(root, name, hex) {
  const { r, g, b } = hexToRgb(hex)
  root.style.setProperty(`--${name}`, hex)
  root.style.setProperty(`--${name}-rgb`, `${r},${g},${b}`)
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
  next.hideFrontmatterInRenderedModes = next.hideFrontmatterInRenderedModes !== false
  next.showDocumentBanners = next.showDocumentBanners !== false
  next.typewriterScrolling = Boolean(next.typewriterScrolling)
  next.spellcheck = Boolean(next.spellcheck)
  next.vimMode = Boolean(next.vimMode)
  next.darkThemePreset = THEME_PRESETS.dark.some(preset => preset.value === next.darkThemePreset) ? next.darkThemePreset : DEFAULT_SETTINGS.darkThemePreset
  next.lightThemePreset = THEME_PRESETS.light.some(preset => preset.value === next.lightThemePreset) ? next.lightThemePreset : DEFAULT_SETTINGS.lightThemePreset
  next.autoSaveDelay = clamp(Number(next.autoSaveDelay) || 800, 200, 5000)
  next.tabIndentation = ['spaces', 'tabs'].includes(next.tabIndentation) ? next.tabIndentation : 'spaces'
  next.indentWidth = [2, 4, 8].includes(Number(next.indentWidth)) ? Number(next.indentWidth) : 2
  next.softWrap = next.softWrap !== false
  next.showLineNumbers = Boolean(next.showLineNumbers)
  next.showStatusBar = next.showStatusBar !== false
  next.showExperimental = Boolean(next.showExperimental)
  next.featureGraphView = Boolean(next.featureGraphView)
  next.featureCalendar = Boolean(next.featureCalendar)
  next.featureBookmarks = Boolean(next.featureBookmarks)
  next.featureTags = Boolean(next.featureTags)
  next.featureAgents = Boolean(next.featureAgents)
  next.featureProperties = Boolean(next.featureProperties)
  next.featureInspector = Boolean(next.featureInspector)
  next.featureWikilinks = Boolean(next.featureWikilinks)
  next.featureSemanticIndex = Boolean(next.featureSemanticIndex)
  next.featureWikiQuality = Boolean(next.featureWikiQuality)
  next.featureRelatedNotes = Boolean(next.featureRelatedNotes)
  next.featureDiagramBuilder = Boolean(next.featureDiagramBuilder)
  next.featureTerminal = Boolean(next.featureTerminal)
  next.featurePublish = Boolean(next.featurePublish)
  next.defaultViewMode = ['markdown', 'split', 'preview'].includes(next.defaultViewMode) ? next.defaultViewMode : DEFAULT_SETTINGS.defaultViewMode
  next.readingSpeed = clamp(Number(next.readingSpeed) || 200, 100, 500)
  next.zenParagraphDimming = Boolean(next.zenParagraphDimming)
  next.zenColumnWidth = clamp(Number(next.zenColumnWidth) || 700, 500, 900)
  next.showMinimap = Boolean(next.showMinimap)
  next.dailyNotesFolder = String(next.dailyNotesFolder || 'daily').trim().replace(/^\/+|\/+$/g, '') || 'daily'
  next.dailyNoteTemplate = String(next.dailyNoteTemplate ?? DEFAULT_SETTINGS.dailyNoteTemplate)
  next.maxHistorySnapshots = clamp(Number(next.maxHistorySnapshots) || 50, 5, 500)
  next.appIconVariant = APP_ICON_VARIANTS.some(icon => icon.value === next.appIconVariant) ? next.appIconVariant : DEFAULT_SETTINGS.appIconVariant
  next.appIconTheme = ['auto', 'dark', 'light'].includes(next.appIconTheme) ? next.appIconTheme : DEFAULT_SETTINGS.appIconTheme
  next.aiProvider = AI_PROVIDER_KEYS.includes(next.aiProvider) ? next.aiProvider : 'openai'
  if (!ASSISTANT_DOCK_OPTIONS.some(option => option.value === next.assistantDock)) {
    next.assistantDock = DEFAULT_SETTINGS.assistantDock
  }
  next.aiApiKey = String(next.aiApiKey || '')
  next.aiModel = String(next.aiModel || '')
  next.aiBaseUrl = String(next.aiBaseUrl || '')

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
  const palette = getThemePalette(next)
  const contrast = next.contrastBoost / 100
  const contrastTarget = palette.theme === 'light' ? '#000000' : '#ffffff'
  const baseText1 = mixHex(palette.text1, contrastTarget, contrast * 0.58)
  const baseText2 = mixHex(palette.text2, contrastTarget, contrast * 0.50)
  const baseText3 = mixHex(palette.text3, contrastTarget, contrast * 0.44)
  const accent = next.accentColor || palette.accent
  const accentHi = palette.theme === 'light'
    ? mixHex(accent, '#000000', 0.2)
    : mixHex(accent, '#ffffff', 0.22)
  const accentDim = hexToRgba(accent, palette.theme === 'light' ? 0.14 : 0.16)
  const text1 = next.textColor || baseText1
  const text2 = next.mutedTextColor || baseText2
  const text3 = next.subtleTextColor || baseText3

  palette.preset.bg.forEach((hex, index) => setHexRgb(root, `bg${index}`, hex))
  root.style.setProperty('--green', palette.preset.green)
  root.style.setProperty('--red', palette.preset.red)
  root.style.setProperty('--amber', palette.preset.amber)

  root.style.setProperty('--ambient-opacity', next.ambientBackground ? String(next.ambientIntensity / 100) : '0')
  root.style.setProperty('--surface-opacity', String(next.surfaceOpacity / 100))
  root.style.setProperty('--surface-blur', `${next.surfaceBlur}px`)
  root.style.setProperty('--surface-bg-0', `rgba(var(--bg0-rgb), ${Math.max(0.24, next.surfaceOpacity / 100 - 0.18).toFixed(2)})`)
  root.style.setProperty('--surface-bg-1', `rgba(var(--bg1-rgb), ${Math.max(0.28, next.surfaceOpacity / 100 - 0.12).toFixed(2)})`)
  root.style.setProperty('--surface-bg-2', `rgba(var(--bg2-rgb), ${Math.max(0.32, next.surfaceOpacity / 100 - 0.08).toFixed(2)})`)
  root.style.setProperty('--surface-bg-3', `rgba(var(--bg3-rgb), ${Math.max(0.38, next.surfaceOpacity / 100 - 0.02).toFixed(2)})`)
  const edgeRgb = palette.theme === 'light' ? '53,42,30' : '255,255,255'
  root.style.setProperty('--surface-edge', `rgba(${edgeRgb}, ${Math.max(0.03, next.surfaceOpacity / 100 * 0.09).toFixed(2)})`)
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
  const resolvedPreviewFont = next.previewMirrorEditor
    ? (next.editorFontCustom || next.editorFont)
    : (next.previewFontCustom || next.previewFont)
  const resolvedPreviewSize = next.previewMirrorEditor ? next.editorFontSize : next.previewFontSize
  const resolvedPreviewLH = next.previewMirrorEditor ? next.editorLineHeight : next.previewLineHeight
  root.style.setProperty('--preview-font', resolvedPreviewFont)
  root.style.setProperty('--preview-font-size', `${resolvedPreviewSize}px`)
  root.style.setProperty('--preview-line-height', String(resolvedPreviewLH))
  root.dataset.statusbar = next.showStatusBar ? 'true' : 'false'
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
  if (key === 'darkThemePreset' || key === 'lightThemePreset') {
    const theme = key === 'darkThemePreset' ? 'dark' : 'light'
    const preset = getPreset(theme, nextValue)
    return setSettings({ [key]: nextValue, ...preset.atmosphere })
  }
  return setSettings({ [key]: nextValue })
}

export function resetSettings() {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(DEFAULT_SETTINGS))
  return applySettings(DEFAULT_SETTINGS)
}
