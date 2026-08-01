const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

async function importModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/history.js'), 'utf8')
  const modulePath = path.join(root, `.tmp-history-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  // Mock window.fjord for listDir/createDir/readFile (needed by some module-level deps)
  const impDir = path.join(root, 'src', 'renderer')
  const modulePathInDir = path.join(impDir, `.tmp-history-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePathInDir,
    'globalThis.window = { fjord: { listDir: async () => [], createDir: async () => true, readFile: async () => null } };\n' +
    source)
  try {
    return await import(`file://${modulePathInDir}`)
  } finally {
    fs.rmSync(modulePathInDir, { force: true })

  }
}

test('relativeTime returns "just now" for recent dates', async () => {
  const { relativeTime } = await importModule()
  const now = new Date()
  assert.equal(relativeTime(now), 'just now')
  assert.equal(relativeTime(new Date(now.getTime() - 30000)), 'just now')
})

test('relativeTime returns "1m ago" for one minute old', async () => {
  const { relativeTime } = await importModule()
  const past = new Date(Date.now() - 60000)
  assert.equal(relativeTime(past), '1m ago')
})

test('relativeTime returns "5m ago" for five minutes old', async () => {
  const { relativeTime } = await importModule()
  const past = new Date(Date.now() - 300000)
  assert.equal(relativeTime(past), '5m ago')
})

test('relativeTime returns "1h ago" for one hour old', async () => {
  const { relativeTime } = await importModule()
  const past = new Date(Date.now() - 3600000)
  assert.equal(relativeTime(past), '1h ago')
})

test('relativeTime returns "3h ago" for three hours old', async () => {
  const { relativeTime } = await importModule()
  const past = new Date(Date.now() - 10800000)
  assert.equal(relativeTime(past), '3h ago')
})

test('relativeTime returns "1d ago" for one day old', async () => {
  const { relativeTime } = await importModule()
  const past = new Date(Date.now() - 86400000)
  assert.equal(relativeTime(past), '1d ago')
})

test('relativeTime returns locale date string for 7+ days', async () => {
  const { relativeTime } = await importModule()
  const past = new Date(Date.now() - 30 * 86400000)
  const result = relativeTime(past)
  assert.ok(result, 'should return a non-empty string')
  assert.ok(!result.includes('ago'), 'should not use relative format')
})

test('relativeTime returns locale date string for very old dates', async () => {
  const { relativeTime } = await importModule()
  const past = new Date(Date.now() - 2 * 365 * 86400000 - 86400000)
  const result = relativeTime(past)
  assert.ok(result, 'should return a non-empty string')
  assert.ok(!result.includes('ago'), 'should not use relative format')
})

test('relativeTime returns locale date string for two years', async () => {
  const { relativeTime } = await importModule()
  const past = new Date(Date.now() - 2 * 365 * 86400000)
  const result = relativeTime(past)
  assert.ok(result, 'should return a non-empty string')
  assert.ok(!result.includes('ago'), 'should not use relative format')
})

test('formatSize formats bytes correctly', async () => {
  const { formatSize } = await importModule()
  assert.equal(formatSize(0), '0 B')
  assert.equal(formatSize(500), '500 B')
  assert.equal(formatSize(1024), '1.0 KB')
  assert.equal(formatSize(1536), '1.5 KB')
  assert.equal(formatSize(1048576), '1.0 MB')
  assert.equal(formatSize(1610612736), '1536.0 MB')
})
