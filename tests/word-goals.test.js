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
  const source = fs.readFileSync(path.join(root, 'src/renderer/word-goals.js'), 'utf8')
  const modulePath = path.join(root, `.tmp-word-goals-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePath, 'globalThis.localStorage = globalThis.__TEST_STORAGE__;\n' + source)
  try {
    return await import(`file://${modulePath}`)
  } finally {
    fs.rmSync(modulePath, { force: true })
  }
}

test('getDocumentGoal returns null for missing file path', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { getDocumentGoal } = await importModule()
  assert.strictEqual(getDocumentGoal(null), null)
  assert.strictEqual(getDocumentGoal(undefined), null)
  assert.strictEqual(getDocumentGoal(''), null)
})

test('getDocumentGoal returns null when no goal is set', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { getDocumentGoal } = await importModule()
  assert.strictEqual(getDocumentGoal('/some/file.md'), null)
})

test('setDocumentGoal and getDocumentGoal round-trip', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setDocumentGoal, getDocumentGoal } = await importModule()

  setDocumentGoal('/path/to/note.md', 500)
  assert.equal(getDocumentGoal('/path/to/note.md'), 500)
  assert.strictEqual(getDocumentGoal('/other/note.md'), null)
})

test('setDocumentGoal with null/zero removes the goal', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setDocumentGoal, getDocumentGoal } = await importModule()

  setDocumentGoal('/path/to/note.md', 500)
  setDocumentGoal('/path/to/note.md', null)
  assert.strictEqual(getDocumentGoal('/path/to/note.md'), null)

  setDocumentGoal('/path/to/note.md', 500)
  setDocumentGoal('/path/to/note.md', 0)
  assert.strictEqual(getDocumentGoal('/path/to/note.md'), null)
})

test('setDocumentGoal ignores non-positive targets', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setDocumentGoal, getDocumentGoal } = await importModule()

  setDocumentGoal('/path/to/note.md', -5)
  assert.strictEqual(getDocumentGoal('/path/to/note.md'), null)
})

test('setSessionGoal starts a session target', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setSessionGoal, getSessionGoal, getSessionProgress } = await importModule()

  setSessionGoal(1000, 50)
  assert.equal(getSessionGoal(), 1000)

  const progress = getSessionProgress(150)
  assert.equal(progress.written, 100)
  assert.equal(progress.target, 1000)
  assert.equal(progress.complete, false)
})

test('getSessionProgress returns null when no session goal set', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { getSessionProgress } = await importModule()
  assert.strictEqual(getSessionProgress(500), null)
})

test('getSessionProgress marks complete when goal reached', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setSessionGoal, getSessionProgress } = await importModule()

  setSessionGoal(100, 0)
  const progress = getSessionProgress(200)
  assert.equal(progress.complete, true)
  assert.equal(progress.written, 200)
})

test('isGoalReached returns true only when document goal is met', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setDocumentGoal, isGoalReached } = await importModule()

  setDocumentGoal('/file.md', 100)
  assert.equal(isGoalReached(50, '/file.md'), false)
  assert.equal(isGoalReached(100, '/file.md'), true)
  assert.equal(isGoalReached(200, '/file.md'), true)
  assert.equal(isGoalReached(100, '/other.md'), false)
})

test('formatGoalStatus formats combined doc and session goals', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setDocumentGoal, setSessionGoal, formatGoalStatus } = await importModule()

  setDocumentGoal('/file.md', 1000)
  setSessionGoal(500, 0)

  const status = formatGoalStatus(300, '/file.md')
  assert.ok(status.includes('300 / 1000'), 'doc goal shown')
  assert.ok(status.includes('session: 300 / 500'), 'session goal shown')
})

test('formatGoalStatus returns empty string for no goals', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { formatGoalStatus } = await importModule()

  assert.equal(formatGoalStatus(100, '/file.md'), '')
})

test('setSessionGoal with zero/null clears session goal', async () => {
  globalThis.__TEST_STORAGE__ = mockLocalStorage()
  const { setSessionGoal, getSessionGoal } = await importModule()

  setSessionGoal(500, 0)
  assert.equal(getSessionGoal(), 500)

  setSessionGoal(0, 0)
  assert.strictEqual(getSessionGoal(), null)

  setSessionGoal(500, 0)
  setSessionGoal(-1, 0)
  assert.strictEqual(getSessionGoal(), null)
})
