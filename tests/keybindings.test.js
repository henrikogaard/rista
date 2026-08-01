const assert = require('node:assert/strict')
const fs = require('node:fs')
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

async function importModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/keybindings.js'), 'utf8')
  const modulePath = path.join(root, 'src/renderer', `.tmp-keybindings-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePath, 'globalThis.localStorage = globalThis.__TEST_STORAGE__;\n' + source)
  try {
    return await import(`file://${modulePath}`)
  } finally {
    fs.rmSync(modulePath, { force: true })
  }
}

test('default bindings are defined for all shortcut IDs', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { getAllBindings, initKeybindings } = await importModule()
  initKeybindings()

  const all = getAllBindings()
  assert.ok(all.save, 'save binding exists')
  assert.equal(all.save.default, 'Mod+S')
  assert.equal(all.save.current, 'Mod+S')
  assert.equal(all.save.isOverridden, false)

  assert.ok(all['quick-open'], 'quick-open binding exists')
  assert.equal(all['quick-open'].default, 'Mod+P')

  assert.ok(all['zen-mode'], 'zen-mode binding exists')
  assert.equal(all['zen-mode'].default, 'Mod+Shift+Enter')

  assert.equal(Object.keys(all).length, 13, '13 default bindings')
})

test('setBinding persists override and getAllBindings reflects it', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setBinding, getAllBindings, initKeybindings } = await importModule()
  initKeybindings()

  setBinding('save', 'Mod+Shift+S')

  const all = getAllBindings()
  assert.equal(all.save.current, 'Mod+Shift+S')
  assert.equal(all.save.isOverridden, true)

  // localStorage was written
  const stored = JSON.parse(globalThis.__TEST_STORAGE__.getItem('rista-keybindings'))
  assert.deepEqual(stored, { save: 'Mod+Shift+S' })
})

test('resetBinding removes override and restores default', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setBinding, resetBinding, getAllBindings, initKeybindings } = await importModule()
  initKeybindings()

  setBinding('save', 'Mod+Shift+S')
  resetBinding('save')

  const all = getAllBindings()
  assert.equal(all.save.current, 'Mod+S')
  assert.equal(all.save.isOverridden, false)
  assert.deepEqual(globalThis.__TEST_STORAGE__.getItem('rista-keybindings'), '{}')
})

test('getBinding returns default when no override exists', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { getBinding, initKeybindings } = await importModule()
  initKeybindings()

  assert.equal(getBinding('save'), 'Mod+S')
  assert.equal(getBinding('unknown-id'), null)
})

test('findConflict detects conflicting keybindings', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { findConflict, setBinding, initKeybindings } = await importModule()
  initKeybindings()

  setBinding('new-file', 'Mod+S')
  const conflict = findConflict('new-file', 'Mod+S')
  assert.equal(conflict, 'save', 'should find save as conflicting with new-file')
})

test('findConflict returns null for unique combo', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { findConflict, initKeybindings } = await importModule()
  initKeybindings()

  const conflict = findConflict('save', 'Alt+Shift+X')
  assert.strictEqual(conflict, null)
})

test('formatKeyEvent formats macOS and windows modifiers correctly', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { formatKeyEvent, initKeybindings } = await importModule()
  initKeybindings()

  const e1 = { key: 's', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }
  assert.equal(formatKeyEvent(e1), 'Mod+S')

  const e2 = { key: 'Space', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }
  assert.equal(formatKeyEvent(e2), 'Mod+Space')

  const e3 = { key: 'f', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true }
  assert.equal(formatKeyEvent(e3), 'Mod+Shift+F')

  // Modifier keys return null
  const e4 = { key: 'Control', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }
  assert.strictEqual(formatKeyEvent(e4), null)
})

test('matchesBinding matches key event against registered bindings', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { matchesBinding, initKeybindings } = await importModule()
  initKeybindings()

  // Mod+S = save
  const e1 = { key: 's', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, preventDefault() {} }
  assert.equal(matchesBinding(e1, 'save'), true)
  assert.equal(matchesBinding(e1, 'new-file'), false)

  // Mod+N = new-file
  const e2 = { key: 'n', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, preventDefault() {} }
  assert.equal(matchesBinding(e2, 'new-file'), true)

  // Non-registered key does not match
  const e3 = { key: 'x', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, preventDefault() {} }
  assert.equal(matchesBinding(e3, 'save'), false)
})

test('matchesBinding respects overridden bindings', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setBinding, matchesBinding, initKeybindings } = await importModule()
  initKeybindings()

  setBinding('save', 'Mod+Shift+S')

  // Old binding no longer matches
  const e1 = { key: 's', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, preventDefault() {} }
  assert.equal(matchesBinding(e1, 'save'), false)

  // New binding matches
  const e2 = { key: 's', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true, preventDefault() {} }
  assert.equal(matchesBinding(e2, 'save'), true)
})

test('getBinding returns overridden value when set', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { getBinding, setBinding, initKeybindings } = await importModule()
  initKeybindings()

  assert.equal(getBinding('save'), 'Mod+S', 'default before override')
  setBinding('save', 'Mod+Shift+S')
  assert.equal(getBinding('save'), 'Mod+Shift+S', 'overridden value')
})

test('initKeybindings recovers from corrupt localStorage', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage({
    'rista-keybindings': ':::not-json:::',
  })
  const { getAllBindings, initKeybindings } = await importModule()
  initKeybindings()

  const all = getAllBindings()
  assert.equal(all.save.current, 'Mod+S', 'falls back to defaults on corrupt data')
})
