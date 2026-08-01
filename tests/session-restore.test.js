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
  const source = fs.readFileSync(path.join(root, 'src/renderer/session-restore.js'), 'utf8')
  const modulePath = path.join(root, `.tmp-session-restore-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePath, 'globalThis.localStorage = globalThis.__TEST_STORAGE__;\n' + source)
  try {
    return await import(`file://${modulePath}`)
  } finally {
    fs.rmSync(modulePath, { force: true })
  }
}

test('saveSession stores serialized data under a hashed key', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveSession, loadSession } = await importModule()

  saveSession('/some/folder', { tabs: ['a.md', 'b.md'], activeTab: 'a.md' })
  const loaded = loadSession('/some/folder')
  assert.ok(loaded, 'session should be loadable')
  assert.deepEqual(loaded.tabs, ['a.md', 'b.md'])
  assert.equal(loaded.activeTab, 'a.md')
  assert.ok(loaded.savedAt, 'savedAt timestamp should exist')
})

test('loadSession returns null for unknown folder', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { loadSession } = await importModule()
  assert.strictEqual(loadSession('/unknown/path'), null)
})

test('saveSession with null folderPath is a no-op', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveSession, loadSession } = await importModule()

  saveSession(null, { tabs: ['a.md'] })
  // Should not have thrown; no key should be set
  assert.strictEqual(loadSession('/some/folder'), null)
})

test('loadSession returns null for null/undefined folderPath', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { loadSession } = await importModule()
  assert.strictEqual(loadSession(null), null)
  assert.strictEqual(loadSession(undefined), null)
})

test('clearSession removes stored session', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveSession, loadSession, clearSession } = await importModule()

  saveSession('/test/folder', { tabs: ['test.md'] })
  assert.ok(loadSession('/test/folder'), 'session exists after save')

  clearSession('/test/folder')
  assert.strictEqual(loadSession('/test/folder'), null, 'session gone after clear')
})

test('clearSession with null folderPath is a no-op', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveSession, clearSession } = await importModule()

  // Should not throw
  clearSession(null)
  clearSession(undefined)
})

test('saveSession stores minimal data gracefully', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveSession, loadSession } = await importModule()

  saveSession('/empty', {})
  const loaded = loadSession('/empty')
  assert.ok(loaded, 'should load even with empty data')
  assert.ok(loaded.savedAt, 'savedAt should still be set')
})

test('loadSession recovers gracefully from corrupt localStorage', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage({
    'rista-session-abc123': '{not valid json',
  })
  const { loadSession } = await importModule()
  assert.strictEqual(loadSession('/corrupt'), null)
})

test('different folder paths produce different storage keys', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { saveSession, loadSession } = await importModule()

  saveSession('/folder/a', { tabs: ['a.md'] })
  saveSession('/folder/b', { tabs: ['b.md'] })

  // Check isolation
  assert.deepEqual(loadSession('/folder/a').tabs, ['a.md'])
  assert.deepEqual(loadSession('/folder/b').tabs, ['b.md'])
})
