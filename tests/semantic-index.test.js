const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

async function importSemanticIndexModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/semantic-index.js'), 'utf8')
  const modulePath = path.join(os.tmpdir(), `rista-semantic-index-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePath, source)
  const mod = await import(`file://${modulePath}`)
  fs.rmSync(modulePath, { force: true })
  return mod
}

function makeIndex() {
  return {
    dirty: false,
    files: new Map([
      ['/vault/Smores.md', {
        content: '# Ingredients\n\nGraham crackers, chocolate spread, and marshmallow creme.\n\n# Directions\n\nToast over the fire.',
      }],
      ['/vault/Campfire Desserts.md', {
        content: '# Notes\n\nChocolate desserts with graham crackers work well near a campfire.',
      }],
      ['/vault/Garden.md', {
        content: '# Plants\n\nTomatoes, basil, and soil notes for spring.',
      }],
    ]),
  }
}

test('local semantic index returns cited search results with headings and snippets', async () => {
  const { buildSemanticIndex, searchSemanticIndex } = await importSemanticIndexModule()

  const index = buildSemanticIndex(makeIndex(), { folderPath: '/vault' })
  const results = searchSemanticIndex(index, 'chocolate graham crackers', { limit: 2 })

  assert.equal(index.status, 'ready')
  assert.equal(index.documentCount, 3)
  assert.equal(results[0].path, '/vault/Smores.md')
  assert.equal(results[0].heading, 'Ingredients')
  assert.match(results[0].snippet, /Graham crackers/)
  assert.ok(results[0].score > 0)
  assert.ok(results[0].relevance > 0)
})

test('local semantic index finds related notes for the current document', async () => {
  const { buildSemanticIndex, findRelatedNotes } = await importSemanticIndexModule()

  const index = buildSemanticIndex(makeIndex(), { folderPath: '/vault' })
  const related = findRelatedNotes(index, '/vault/Smores.md', { limit: 2 })

  assert.equal(related[0].path, '/vault/Campfire Desserts.md')
  assert.match(related[0].reason, /chocolate/)
  assert.match(related[0].snippet, /graham crackers/)
  assert.equal(related.some(item => item.path === '/vault/Garden.md'), false)
})
