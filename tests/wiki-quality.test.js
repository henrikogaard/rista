const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

async function importWikiQualityModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/wiki-quality.js'), 'utf8')
  const modulePath = path.join(os.tmpdir(), `rista-wiki-quality-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  fs.writeFileSync(modulePath, source)
  const mod = await import(`file://${modulePath}`)
  fs.rmSync(modulePath, { force: true })
  return mod
}

test('wiki quality analysis finds missing links, orphans, duplicate titles, and glossary candidates', async () => {
  const { analyzeWikiQuality } = await importWikiQualityModule()
  const folderPath = '/vault'
  const paths = [
    '/vault/Home.md',
    '/vault/Project.md',
    '/vault/archive/Project.md',
    '/vault/Orphan.md',
  ]
  const index = {
    allPaths: new Set(paths),
    dirty: true,
    files: new Map([
      ['/vault/Home.md', {
        content: '# Home\n\nRista Graph ties notes together. [[Project]] references [[Missing Note]].',
        links: new Set(['Project', 'Missing Note']),
      }],
      ['/vault/Project.md', {
        content: '# Project\n\nRista Graph helps keep wiki maps useful. [[Home]]',
        links: new Set(['Home']),
      }],
      ['/vault/archive/Project.md', {
        content: '# Project\n\nOld project page.',
        links: new Set(),
      }],
      ['/vault/Orphan.md', {
        content: '# Orphan\n\nPrivate scratch note.',
        links: new Set(),
      }],
    ]),
    backlinks: new Map([
      ['/vault/Home.md', new Set(['/vault/Project.md'])],
      ['/vault/Project.md', new Set(['/vault/Home.md'])],
    ]),
  }

  const report = analyzeWikiQuality(index, {
    folderPath,
    resolveLink: (linkText) => {
      if (linkText === 'Home') return '/vault/Home.md'
      if (linkText === 'Project') return '/vault/Project.md'
      return null
    },
  })

  assert.equal(report.summary.stale, true)
  assert.equal(report.summary.unresolvedLinks, 1)
  assert.equal(report.unresolvedLinks[0].linkText, 'Missing Note')
  assert.equal(report.unresolvedLinks[0].sourcePath, '/vault/Home.md')
  assert.deepEqual(report.orphanNotes.map(note => note.path), [
    '/vault/archive/Project.md',
    '/vault/Orphan.md',
  ])
  assert.equal(report.duplicateTitles[0].title, 'Project')
  assert.deepEqual(report.duplicateTitles[0].paths, [
    '/vault/Project.md',
    '/vault/archive/Project.md',
  ])
  assert.equal(report.glossaryCandidates[0].term, 'Rista Graph')
  assert.equal(report.glossaryCandidates[0].fileCount, 2)
})
