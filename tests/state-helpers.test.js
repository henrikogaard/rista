const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs')
const path = require('node:path')

const root = path.resolve(__dirname, '..')

async function importState(extraProlog = '') {
  const source = fs.readFileSync(path.join(root, 'src/renderer/state.js'), 'utf8')
  const modPath = path.join(root, 'src/renderer', `.tmp-sh-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  const wrapped = `${extraProlog}\n${source}`
  fs.writeFileSync(modPath, wrapped)
  try {
    return await import(`file://${modPath}`)
  } finally {
    try { fs.rmSync(modPath, { force: true }) } catch {}
  }
}

test('relativeFilePath strips folder prefix', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath('/vault/notes/a.md', '/vault'), 'notes/a.md')
})

test('relativeFilePath returns full path when no folder match', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath('/other/b.md', '/vault'), '/other/b.md')
})

test('relativeFilePath handles trailing slash in folder', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath('/vault/c.md', '/vault/'), 'c.md')
})

test('relativeFilePath handles backslash paths', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath('C:\\docs\\d.md', 'C:\\docs'), 'd.md')
})

test('relativeFilePath returns empty string for empty path', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath('', '/vault'), '')
})

test('relativeFilePath returns empty for null path', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath(null, '/vault'), '')
})

test('relativeFilePath returns path when folderPath is null', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath('/vault/a.md', null), '/vault/a.md')
})

test('relativeFilePath returns path when folderPath is empty', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath('/vault/a.md', ''), '/vault/a.md')
})

test('relativeFilePath handles nested subfolder paths', async () => {
  const m = await importState()
  assert.equal(m.relativeFilePath('/vault/projects/sub/note.md', '/vault'), 'projects/sub/note.md')
})

