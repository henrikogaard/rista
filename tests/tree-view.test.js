const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

async function importModule() {
  const source = fs.readFileSync(path.join(root, 'src/renderer/tree-view.js'), 'utf8')
  const modulePath = path.join(root, 'src/renderer', `.tmp-tree-view-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)
  // Polyfill document for module
  const wrapped = `
globalThis.document = { 
  createElement: (tag) => ({ className: '', innerHTML: '', style: {}, dataset: {}, addEventListener: () => {}, removeEventListener: () => {}, classList: { add: () => {}, remove: () => {}, toggle: () => {} }, appendChild: () => {}, querySelectorAll: () => [], closest: () => null }),
  querySelectorAll: () => [],
  querySelector: () => null
};
${source}
`
  fs.writeFileSync(modulePath, wrapped)
  try {
    return await import(`file://${modulePath}`)
  } finally {
    fs.rmSync(modulePath, { force: true })
  }
}

test('collectFolderPaths collects all folder paths recursively', async () => {
  const { collectFolderPaths } = await importModule()

  const tree = [
    { type: 'folder', path: '/root', name: 'root', children: [
      { type: 'file', path: '/root/a.md', name: 'a.md' },
      { type: 'folder', path: '/root/sub', name: 'sub', children: [
        { type: 'file', path: '/root/sub/b.md', name: 'b.md' },
      ]},
    ]},
    { type: 'file', path: '/rootme.md', name: 'rootme.md' },
    { type: 'folder', path: '/empty', name: 'empty', children: [] },
  ]

  const paths = collectFolderPaths(tree)
  assert.deepEqual(paths, ['/root', '/root/sub', '/empty'])
})

test('collectFolderPaths returns empty array for empty input', async () => {
  const { collectFolderPaths } = await importModule()
  assert.deepEqual(collectFolderPaths([]), [])
  assert.deepEqual(collectFolderPaths([{ type: 'file', path: '/a.md' }]), [])
})

test('highlightTreeFiles toggles active class based on paths', async () => {
  const { highlightTreeFiles } = await importModule()

  // Set up mock DOM
  const node1 = { dataset: { path: '/active.md' }, classList: { toggle: () => {} } }
  const node2 = { dataset: { path: '/inactive.md' }, classList: { toggle: () => {} } }
  const node3 = { dataset: { path: '/active2.md' }, classList: { toggle: () => {} } }

  // Override querySelectorAll for this test context
  globalThis.document.querySelectorAll = () => [node1, node2, node3]

  let toggled1 = '', toggled2 = ''
  node1.classList = { toggle: (cls, state) => { toggled1 = state; } }
  node2.classList = { toggle: (cls, state) => { toggled2 = state; } }
  node3.classList = { toggle: (cls, state) => { toggled3 = state; } }
  let toggled3 = ''

  highlightTreeFiles(new Set(['/active.md', '/active2.md']))

  assert.equal(toggled1, true, 'active file gets active class')
  assert.equal(toggled2, false, 'inactive file does not get active class')
  assert.equal(toggled3, true, 'second active file gets active class')
})
