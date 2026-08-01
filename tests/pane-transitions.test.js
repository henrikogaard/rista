const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

function mockLocalStorage(initial = {}) {
  const store = new Map(Object.entries(initial))
  return {
    getItem(key) { return store.get(key) ?? null },
    setItem(key, value) { store.set(key, String(value)) },
    removeItem(key) { store.delete(key) },
  }
}

function mockGetSettings(overrides = {}) {
  const defaults = {
    autoSaveDelay: 800,
    assistantDock: 'hidden',
    showExperimental: false,
    featureDiagramBuilder: false,
    featureAgents: false,
    featureGraphView: false,
    featureTags: false,
    featureProperties: false,
    featureRelatedNotes: false,
    featureCalendar: false,
    featureWikilinks: false,
    featureBookmarks: false,
    featureWikiQuality: false,
    featureSemanticSearch: false,
    featureInspector: false,
    ...overrides,
  }
  return defaults
}

async function importStateModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/state.js'), 'utf8')
  // Write temp file inside src/renderer/ so relative imports (./settings.js) resolve correctly
  const modulePath = path.join(root, 'src/renderer', `.tmp-state-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  const wrapped = `
globalThis.document = { getElementById: () => null, createElement: () => ({ className: '', innerHTML: '' }) };
${source}
`
  fs.writeFileSync(modulePath, wrapped)
  try {
    return await import(`file://${modulePath}`)
  } finally {
    fs.rmSync(modulePath, { force: true })
  }
}

test('getPaneView returns default split view for unknown panes', async () => {
  const { getPaneView } = await importStateModule()
  assert.equal(getPaneView('unknown'), 'split')
  assert.equal(getPaneView('tertiary'), 'split')
})

test('makeSplitView returns correct left/right slot assignment', async () => {
  const { makeSplitView } = await importStateModule()

  const defaultView = makeSplitView()
  assert.equal(defaultView.left, 'markdown')
  assert.equal(defaultView.right, 'preview')

  const leftPreview = makeSplitView('markdown', 'left')
  assert.equal(leftPreview.left, 'preview')
  assert.equal(leftPreview.right, 'markdown')

  const wysiwygSplit = makeSplitView('wysiwyg', 'right')
  assert.equal(wysiwygSplit.left, 'wysiwyg')
  assert.equal(wysiwygSplit.right, 'preview')

  const wysiwygLeftPreview = makeSplitView('wysiwyg', 'left')
  assert.equal(wysiwygLeftPreview.left, 'preview')
  assert.equal(wysiwygLeftPreview.right, 'wysiwyg')
})

test('getSplitEditableView returns markdown as default', async () => {
  const { state, getSplitEditableView } = await importStateModule()
  // Default is 'markdown'
  assert.equal(getSplitEditableView('primary'), 'markdown')
  assert.equal(getSplitEditableView('secondary'), 'markdown')
})

test('getSplitPreviewSide returns right as default', async () => {
  const { state, getSplitPreviewSide } = await importStateModule()
  assert.equal(getSplitPreviewSide('primary'), 'right')
  assert.equal(getSplitPreviewSide('secondary'), 'right')
})

test('paneUsesWysiwyg returns true only when pane view is wysiwyg or split with wysiwyg editable', async () => {
  const { state, paneUsesWysiwyg, paneUsesMarkdown } = await importStateModule()

  // Default: primary is 'split', editable is 'markdown'
  assert.equal(paneUsesWysiwyg('primary'), false)
  assert.equal(paneUsesMarkdown('primary'), true)

  // Standalone markdown
  state.paneView.primary = 'markdown'
  assert.equal(paneUsesWysiwyg('primary'), false)
  assert.equal(paneUsesMarkdown('primary'), true)

  // Standalone wysiwyg
  state.paneView.primary = 'wysiwyg'
  assert.equal(paneUsesWysiwyg('primary'), true)
  assert.equal(paneUsesMarkdown('primary'), false)

  // Standalone preview
  state.paneView.primary = 'preview'
  assert.equal(paneUsesWysiwyg('primary'), false)
  assert.equal(paneUsesMarkdown('primary'), false)

  // Split with wysiwyg editable
  state.paneView.primary = 'split'
  state.splitEditableMode.primary = 'wysiwyg'
  assert.equal(paneUsesWysiwyg('primary'), true)
  assert.equal(paneUsesMarkdown('primary'), false)
})

test('getWysiwygMountSlot returns correct mount target', async () => {
  const { state, getWysiwygMountSlot } = await importStateModule()

  // Wysiwyg standalone
  state.paneView.primary = 'wysiwyg'
  assert.equal(getWysiwygMountSlot('primary'), 'single')

  // Not wysiwyg mode
  state.paneView.primary = 'markdown'
  assert.equal(getWysiwygMountSlot('primary'), null)

  // Split with wysiwyg editable, preview on right -> wysiwyg on left
  state.paneView.primary = 'split'
  state.splitEditableMode.primary = 'wysiwyg'
  state.splitPreviewSide.primary = 'right'
  assert.equal(getWysiwygMountSlot('primary'), 'left')

  // Split with wysiwyg editable, preview on left -> wysiwyg on right
  state.splitPreviewSide.primary = 'left'
  assert.equal(getWysiwygMountSlot('primary'), 'right')
})

test('state defaults are set correctly', async () => {
  const { state, PANE_KEYS } = await importStateModule()

  assert.equal(state.workspaceMode, 'single')
  assert.equal(state.focusedPane, 'primary')
  assert.deepEqual(state.paneView, { primary: 'split', secondary: 'split' })
  assert.deepEqual(state.splitEditableMode, { primary: 'markdown', secondary: 'markdown' })
  assert.deepEqual(state.splitPreviewSide, { primary: 'right', secondary: 'right' })
  assert.equal(state.sidebarVisible, true)
  assert.equal(state.toolbarVisible, false)
  assert.equal(state.inspectorOpen, false)
  assert.equal(state.rightPanel, null)
  assert.equal(state.settingsOpen, false)
  assert.equal(state.zenMode, false)
  assert.deepEqual(PANE_KEYS, ['primary', 'secondary'])
  assert.equal(state.commandPaletteOpen, false)
  assert.strictEqual(state.folderPath, null)
  assert.strictEqual(state.tagFilter, null)
})
