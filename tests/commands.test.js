const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs')
const path = require('node:path')

const root = path.resolve(__dirname, '..')

async function importCommands(extraProlog = '') {
  const source = fs.readFileSync(path.join(root, 'src/renderer/commands.js'), 'utf8')
  const modPath = path.join(root, 'src/renderer', `.tmp-cmds-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  const wrapped = `${extraProlog}\n${source}`
  fs.writeFileSync(modPath, wrapped)
  try {
    return await import(`file://${modPath}`)
  } finally {
    try { fs.rmSync(modPath, { force: true }) } catch {}
  }
}

test('normalizePathSeparators replaces backslashes', async () => {
  const m = await importCommands()
  assert.equal(m.normalizePathSeparators('a\\b\\c.md'), 'a/b/c.md')
  assert.equal(m.normalizePathSeparators('a/b/c.md'), 'a/b/c.md')
  assert.equal(m.normalizePathSeparators(), '')
})

test('lastPathSegment returns filename from path', async () => {
  const m = await importCommands()
  assert.equal(m.lastPathSegment('/a/b/c.md'), 'c.md')
  assert.equal(m.lastPathSegment('c.md'), 'c.md')
  assert.equal(m.lastPathSegment(), '')
})

test('stripFileExtension removes extension', async () => {
  const m = await importCommands()
  assert.equal(m.stripFileExtension('file.md'), 'file')
  assert.equal(m.stripFileExtension('file.min.js'), 'file.min')
  assert.equal(m.stripFileExtension('file'), 'file')
  assert.equal(m.stripFileExtension(), '')
})

test('directoryPath returns parent directory', async () => {
  const m = await importCommands()
  assert.equal(m.directoryPath('/a/b/c.md'), '/a/b')
  assert.equal(m.directoryPath('/a/b/'), '/a/b')
  assert.equal(m.directoryPath('c.md'), '')
  assert.equal(m.directoryPath(), '')
})

test('toRelativePath computes relative path between files', async () => {
  const m = await importCommands()
  assert.equal(m.toRelativePath('/docs/a.md', '/docs/b.md'), 'b.md')
  assert.equal(m.toRelativePath('/docs/sub/a.md', '/docs/b.md'), '../b.md')
  assert.equal(m.toRelativePath('/docs/a.md', '/docs/sub/b.md'), 'sub/b.md')
  // Root-level files with no shared parent fall back to absolute path
  assert.equal(m.toRelativePath('/a.md', '/b.md'), '/b.md')
})

test('formatCalloutLabel formats type string', async () => {
  const m = await importCommands()
  assert.equal(m.formatCalloutLabel('note'), 'Note')
  assert.equal(m.formatCalloutLabel('warning'), 'Warning')
  assert.equal(m.formatCalloutLabel('abstract'), 'Abstract')
  assert.equal(m.formatCalloutLabel('question'), 'Question')
  assert.equal(m.formatCalloutLabel('tip'), 'Tip')
  assert.equal(m.formatCalloutLabel('multi-word'), 'Multi Word')
  assert.equal(m.formatCalloutLabel('already_capitalized'), 'Already Capitalized')
  assert.equal(m.formatCalloutLabel(), 'Note')
})
