const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

function mockLocalStorage(initial = {}) {
  const store = new Map(Object.entries(initial))
  return {
    getItem(key) {
      return store.has(key) ? store.get(key) : null
    },
    setItem(key, value) {
      store.set(key, String(value))
    },
    removeItem(key) {
      store.delete(key)
    },
    clear() {
      store.clear()
    },
  }
}

async function importSettingsModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/settings.js'), 'utf8')
  const modulePath = path.join(os.tmpdir(), `rista-settings-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePath, source)
  const mod = await import(`file://${modulePath}`)
  fs.rmSync(modulePath, { force: true })
  return mod
}

test('settings sanitizes invalid assistant dock values', async () => {
  global.localStorage = mockLocalStorage({
    'fjordmark-settings': JSON.stringify({ assistantDock: 'floating-panel' }),
  })

  const { getSettings } = await importSettingsModule()

  assert.equal(getSettings().assistantDock, 'right-sidebar')
})
