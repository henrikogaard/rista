// Smoke tests for app boot and module structure (Task 3.3)
// Tests which modules are available and export expected symbols.
// CodeMirror-dependent modules are tested via file-content inspection.

const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

function mockDocument() {
  return {
    getElementById: () => null,
    createElement: (tag) => ({
      className: '', innerHTML: '', style: {}, dataset: {},
      addEventListener: () => {}, removeEventListener: () => {},
      classList: { add: () => {}, remove: () => {}, toggle: () => {}, contains: () => false },
      appendChild: () => {}, replaceChildren: () => {}, closest: () => null,
      querySelectorAll: () => [], querySelector: () => null,
    }),
    querySelectorAll: () => [],
    querySelector: () => null,
    documentElement: { dataset: {}, style: { setProperty: () => {} } },
    body: {},
  }
}

function mockLocalStorage(initial = {}) {
  const store = new Map(Object.entries(initial))
  return {
    getItem(key) { return store.get(key) ?? null },
    setItem(key, value) { store.set(key, String(value)) },
    removeItem(key) { store.delete(key) },
    clear() { store.clear() },
  }
}

async function importModule(moduleName, extraProlog = '') {
  const source = fs.readFileSync(path.join(root, 'src/renderer', moduleName), 'utf8')
  const modulePath = path.join(root, 'src/renderer', `.tmp-smoke-${moduleName.replace('.js', '')}-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  const wrapped = `${extraProlog}\n${source}`
  fs.writeFileSync(modulePath, wrapped)
  try {
    return await import(`file://${modulePath}`)
  } finally {
    fs.rmSync(modulePath, { force: true })
  }
}

function fileContains(modName, ...patterns) {
  const content = fs.readFileSync(path.join(root, 'src/renderer', modName), 'utf8')
  return patterns.every(p => {
    if (p instanceof RegExp) return p.test(content)
    return content.includes(p)
  })
}

test('state module loads without error and exports expected API', async () => {
  globalThis.document = mockDocument()
  const mod = await importModule('state.js', `globalThis.document = globalThis.__TEST_DOC__ || document;`)

  assert.ok(typeof mod.$ === 'function', 'exports $() DOM helper')
  assert.ok(typeof mod.el === 'function', 'exports el()')
  assert.ok(mod.state !== undefined, 'exports state object')
  assert.ok(typeof mod.getFocusedTab === 'function', 'exports getFocusedTab')
  assert.ok(typeof mod.getTabForPane === 'function', 'exports getTabForPane')
  assert.ok(typeof mod.getPaneView === 'function', 'exports getPaneView')
  assert.ok(Array.isArray(mod.PANE_KEYS), 'PANE_KEYS is an array')
  assert.ok(mod.PANE_KEYS.includes('primary'), 'PANE_KEYS includes primary')
  assert.ok(mod.PANE_KEYS.includes('secondary'), 'PANE_KEYS includes secondary')
})

test('theme module loads and exports key functions', async () => {
  globalThis.document = mockDocument()
  globalThis.localStorage = mockLocalStorage()
  const mod = await importModule('theme.js', `globalThis.document = globalThis.__TEST_DOC__ || document;`)

  assert.ok(typeof mod.initTheme === 'function', 'exports initTheme')
  assert.ok(typeof mod.toggleTheme === 'function', 'exports toggleTheme')
  assert.ok(typeof mod.getTheme === 'function', 'exports getTheme')
})

test('perf-budget module loads and exports budget constants', async () => {
  const mod = await importModule('perf-budget.js')
  assert.equal(typeof mod.LAUNCH_BUDGET_MS, 'number', 'exports LAUNCH_BUDGET_MS')
  assert.equal(typeof mod.FILE_SWITCH_BUDGET_MS, 'number', 'exports FILE_SWITCH_BUDGET_MS')
  assert.equal(typeof mod.markLaunchStart, 'function', 'exports markLaunchStart')
  assert.equal(typeof mod.markLaunchDone, 'function', 'exports markLaunchDone')
})

test('commands module file exports expected function signatures', async () => {
  assert.ok(fileContains('commands.js', 'export function insertHeading', 'export function insertList',
    'export function insertLink', 'export function insertTable', 'export function insertCodeBlock',
    'export function insertHorizontalRule', 'export function insertCallout'))
  assert.ok(fileContains('commands.js', 'export function insertMarkdownAtSelection'))
  assert.ok(fileContains('commands.js', 'export function insertMarkdownTable'))
  assert.ok(fileContains('commands.js', 'export function wrapInline'))
  assert.ok(fileContains('commands.js', 'export function wrapSelection'))
})

test('tabs module file exports expected function signatures', async () => {
  assert.ok(fileContains('tabs.js', /export (async )?function openFolder/))
  assert.ok(fileContains('tabs.js', /export function closeTab/))
  assert.ok(fileContains('tabs.js', /export function activateTab/))
  assert.ok(fileContains('tabs.js', /export async function saveTab/))
  assert.ok(fileContains('tabs.js', /export async function saveActive/))
  assert.ok(fileContains('tabs.js', /export function showWelcomeScreen/))
  assert.ok(fileContains('tabs.js', /export function renderTabs/))
})

test('markdown module loads and exports render pipeline', async () => {
  const mod = await importModule('markdown.js')

  assert.equal(typeof mod.renderMarkdown, 'function', 'exports renderMarkdown')
  assert.equal(typeof mod.getStats, 'function', 'exports getStats')
  assert.equal(typeof mod.extractHeadings, 'function', 'exports extractHeadings')
  assert.equal(typeof mod.parseFrontmatterBlock, 'function', 'exports parseFrontmatterBlock')
  assert.equal(typeof mod.getRenderableMarkdown, 'function', 'exports getRenderableMarkdown')
  assert.equal(typeof mod.htmlToMarkdown, 'function', 'exports htmlToMarkdown')
})

test('settings module loads and exports expected API', async () => {
  globalThis.document = mockDocument()
  globalThis.localStorage = mockLocalStorage()
  const mod = await importModule('settings.js', `globalThis.document = globalThis.__TEST_DOC__ || document;`)

  assert.equal(typeof mod.getSettings, 'function', 'exports getSettings')
  assert.equal(typeof mod.setSettings, 'function', 'exports setSettings')
  assert.equal(typeof mod.updateSetting, 'function', 'exports updateSetting')
  assert.equal(typeof mod.resetSettings, 'function', 'exports resetSettings')
})

test('word-goals module loads and exports goal functions', async () => {
  globalThis.document = mockDocument()
  globalThis.localStorage = mockLocalStorage()
  const mod = await importModule('word-goals.js', `globalThis.document = globalThis.__TEST_DOC__ || document;`)

  assert.equal(typeof mod.setDocumentGoal, 'function', 'exports setDocumentGoal')
  assert.equal(typeof mod.setSessionGoal, 'function', 'exports setSessionGoal')
  assert.equal(typeof mod.getSessionProgress, 'function', 'exports getSessionProgress')
  assert.equal(typeof mod.isGoalReached, 'function', 'exports isGoalReached')
})

test('shell module file exports known functions', async () => {
  assert.ok(fileContains('shell.js', 'export function buildShell', 'export function buildWelcome',
    'export function toggleSettingsPanel', 'export function applySelectedAppIcon'))
  assert.ok(fileContains('shell.js', /export function syncWorkspaceChrome/))
})

test('keybindings module loads and exports API', async () => {
  globalThis.localStorage = mockLocalStorage()
  const mod = await importModule('keybindings.js', `globalThis.localStorage = globalThis.__TEST_STORAGE__;`)

  assert.equal(typeof mod.getAllBindings, 'function', 'exports getAllBindings')
  assert.equal(typeof mod.formatKeyEvent, 'function', 'exports formatKeyEvent')
  assert.equal(typeof mod.findConflict, 'function', 'exports findConflict')
})

test('crash-recovery module loads and exports API', async () => {
  globalThis.localStorage = mockLocalStorage()
  const mod = await importModule('crash-recovery.js', `globalThis.localStorage = globalThis.__TEST_STORAGE__;`)

  assert.equal(typeof mod.saveRecoveryBuffer, 'function', 'exports saveRecoveryBuffer')
  assert.equal(typeof mod.loadRecoveryBuffer, 'function', 'exports loadRecoveryBuffer')
  assert.equal(typeof mod.hasRecoveryBuffer, 'function', 'exports hasRecoveryBuffer')
  assert.equal(typeof mod.clearRecoveryBuffer, 'function', 'exports clearRecoveryBuffer')
  assert.equal(typeof mod.schedulePeriodicSave, 'function', 'exports schedulePeriodicSave')
})

test('tree-view module loads and exports API', async () => {
  globalThis.document = mockDocument()
  const mod = await importModule('tree-view.js', `globalThis.document = globalThis.__TEST_DOC__ || document;`)

  assert.equal(typeof mod.collectFolderPaths, 'function', 'exports collectFolderPaths')
  assert.equal(typeof mod.renderFileTree, 'function', 'exports renderFileTree')
  assert.equal(typeof mod.highlightTreeFiles, 'function', 'exports highlightTreeFiles')
})

test('source files exist for all renderer modules', async () => {
  const expected = [
    'index.js', 'state.js', 'editor.js', 'markdown.js', 'preview.js',
    'shell.js', 'workspace.js', 'tabs.js', 'settings.js', 'commands.js',
    'theme.js', 'keybindings.js', 'command-palette.js', 'find-replace.js',
    'file-explorer-view.js', 'right-panel.js', 'panels.js', 'tree-view.js',
    'crash-recovery.js', 'word-goals.js', 'perf-budget.js', 'zen-mode.js',
    'icons.js', 'publish.js', 'calendar-view.js',
  ]
  for (const name of expected) {
    const fullPath = path.join(root, 'src/renderer', name)
    assert.ok(fs.existsSync(fullPath), `source file ${name} should exist`)
    const stat = fs.statSync(fullPath)
    assert.ok(stat.size > 50, `${name} should be non-trivial (${stat.size} bytes)`)
  }
})
