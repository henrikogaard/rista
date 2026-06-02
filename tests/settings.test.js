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

function mockDocument(theme = 'dark') {
  const values = new Map()
  return {
    documentElement: {
      dataset: {},
      getAttribute(name) {
        return name === 'data-theme' ? theme : null
      },
      style: {
        values,
        setProperty(name, value) {
          values.set(name, value)
        },
      },
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

async function importAiProvidersModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/ai-providers.js'), 'utf8')
  const modulePath = path.join(os.tmpdir(), `rista-ai-providers-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
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

  assert.equal(getSettings().assistantDock, 'right-rail')
})

test('custom OpenAI-compatible provider does not require an API key by default', async () => {
  const { PROVIDERS } = await importAiProvidersModule()

  assert.equal(PROVIDERS['custom-openai-compatible'].apiKey, false)
  assert.equal(PROVIDERS['custom-openai-compatible'].noApiKey, true)
})

test('clearing a custom font override makes the selected preset effective', async () => {
  global.localStorage = mockLocalStorage()
  global.document = mockDocument()

  const { FONT_OPTIONS, getSettings, setSettings } = await importSettingsModule()
  const customStack = "'Custom Interface', system-ui, sans-serif"
  const preset = FONT_OPTIONS.ui[1].value

  setSettings({
    uiFont: FONT_OPTIONS.ui[0].value,
    uiFontCustom: customStack,
  })
  assert.equal(document.documentElement.style.values.get('--ui-font'), customStack)

  setSettings({
    uiFont: preset,
    uiFontCustom: '',
  })

  assert.equal(getSettings().uiFontCustom, '')
  assert.equal(document.documentElement.style.values.get('--ui-font'), preset)
})
