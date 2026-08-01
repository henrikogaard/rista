const assert = require('node:assert/strict')
const test = require('node:test')

function mockLocalStorage(initial = {}) {
  const store = new Map(Object.entries(initial))
  return {
    getItem(key) { return store.get(key) ?? null },
    setItem(key, value) { store.set(key, String(value)) },
    removeItem(key) { store.delete(key) },
    _dump() { return Object.fromEntries(store) },
  }
}

function mockTabs(dirtyPaths) {
  return dirtyPaths.map(p => ({ path: p, name: p.split('/').pop(), content: '# Test\nHello', dirty: true }))
}

const fs = require('node:fs')
const path = require('node:path')

let _moduleCounter = 0

/**
 * Import crash-recovery.js wrapped in a localStorage shim.
 * Captures the mock storage reference synchronously before the
 * dynamic import() yields to the event loop, preventing concurrent
 * sibling tests from overwriting globalThis.__TEST_STORAGE__ before
 * the module evaluates.
 */
async function importModule() {
  const source = fs.readFileSync(path.join(__dirname, '..', 'src/renderer/crash-recovery.js'), 'utf8')
  const id = ++_moduleCounter

  // Capture storage state synchronously before yielding to import().
  const storage = globalThis.__TEST_STORAGE__
  const initialData = storage && typeof storage._dump === 'function'
    ? storage._dump()
    : {}
  const initCode = `const __store = new Map(${JSON.stringify(Object.entries(initialData))});
globalThis.localStorage = {
  getItem(k) { return __store.get(k) ?? null },
  setItem(k, v) { __store.set(k, String(v)) },
  removeItem(k) { __store.delete(k) },
};`

  const tmp = path.join(__dirname, '..', `.tmp-crash-${process.pid}-${id}.mjs`)
  fs.writeFileSync(tmp, initCode + '\n' + source)
  try {
    return await import(`file://${tmp}`)
  } finally {
    fs.rmSync(tmp, { force: true })
  }
}

test('saveRecoveryBuffer stores dirty tabs', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveRecoveryBuffer, loadRecoveryBuffer } = await importModule()

  saveRecoveryBuffer(mockTabs(['/docs/a.md', '/docs/b.md']))
  const loaded = loadRecoveryBuffer()
  assert.ok(loaded, 'should return recovery data')
  assert.equal(loaded.length, 2)
  assert.equal(loaded[0].path, '/docs/a.md')
  assert.ok(loaded[0].savedAt, 'should have timestamp')
})

test('saveRecoveryBuffer skips clean tabs', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveRecoveryBuffer, hasRecoveryBuffer } = await importModule()

  saveRecoveryBuffer([{ path: '/docs/a.md', content: 'test', dirty: false }])
  assert.equal(hasRecoveryBuffer(), false)
})

test('saveRecoveryBuffer handles empty tabs array', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveRecoveryBuffer, hasRecoveryBuffer } = await importModule()

  saveRecoveryBuffer([])
  assert.equal(hasRecoveryBuffer(), false)

  saveRecoveryBuffer(null)
  assert.equal(hasRecoveryBuffer(), false)
})

test('hasRecoveryBuffer returns false when no buffer exists', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { hasRecoveryBuffer } = await importModule()
  assert.equal(hasRecoveryBuffer(), false)
})

test('loadRecoveryBuffer returns null when no buffer exists', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { loadRecoveryBuffer } = await importModule()
  assert.strictEqual(loadRecoveryBuffer(), null)
})

test('hasRecoveryBuffer returns true after save', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveRecoveryBuffer, hasRecoveryBuffer } = await importModule()

  saveRecoveryBuffer(mockTabs(['/docs/a.md']))
  assert.equal(hasRecoveryBuffer(), true)
})

test('loadRecoveryBuffer clears the buffer', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveRecoveryBuffer, loadRecoveryBuffer, hasRecoveryBuffer } = await importModule()

  saveRecoveryBuffer(mockTabs(['/docs/a.md']))
  loadRecoveryBuffer()
  assert.equal(hasRecoveryBuffer(), false)
})

test('handles corrupt localStorage gracefully', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage({
    'rista-crash-recovery': '{bad json',
  })
  const { hasRecoveryBuffer, loadRecoveryBuffer } = await importModule()
  assert.equal(hasRecoveryBuffer(), true)
  const loaded = loadRecoveryBuffer()
  assert.strictEqual(loaded, null)
})

test('loadRecoveryBuffer returns null for incomplete data', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage({
    'rista-crash-recovery': JSON.stringify([{ path: '/a.md' }]),
  })
  const { loadRecoveryBuffer } = await importModule()
  const loaded = loadRecoveryBuffer()
  assert.ok(loaded)
  assert.equal(loaded.length, 1)
})
