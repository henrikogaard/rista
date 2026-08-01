const assert = require('node:assert/strict')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')

const STORE = {}
const MOCK_STATE = { folderPath: '/test/project' }

async function importBookmarks() {
  // We need to inject state and localStorage before loading
  const source = fs.readFileSync(path.join(root, 'src/renderer/bookmarks.js'), 'utf8')
  const modPath = path.join(os.tmpdir(), `rista-bm-${Date.now()}-${Math.random().toString(16).slice(2)}.mjs`)

  // Patch the module to use our mocks
  // The module imports { state, fileName } from './state.js' — provide shims
  const patched = source
    .replace(/import \{[^}]*\} from '\.\/state\.js'/, '')
    .replace(/\bstate\.folderPath\b/g, 'MOCK_STATE.folderPath')
    .replace(/\bfileName\(/g, 'MOCK_FILE_NAME(')

  // define shims inside the module before its code runs
  const shim = `const MOCK_STATE = { folderPath: '/test/project' };\nconst MOCK_FILE_NAME = (p) => String(p || '').split(/[\\\\/]/).pop() || '';\n`
  const final = shim + patched

  fs.writeFileSync(modPath, final)
  const mod = await import(`file://${modPath}`)
  fs.rmSync(modPath, { force: true })
  return mod
}

test.before(() => {
  global.localStorage = {
    _data: {},
    getItem(k) { return this._data[k] ?? null },
    setItem(k, v) { this._data[k] = String(v) },
    removeItem(k) { delete this._data[k] },
  }
  global.MOCK_STATE = MOCK_STATE
})

test('isBookmarked returns false for unknown path', async () => {
  const { isBookmarked } = await importBookmarks()
  assert.equal(isBookmarked('/test/project/unknown.md'), false)
})

test('addBookmark adds a bookmark', async () => {
  const { addBookmark, isBookmarked, getBookmarks } = await importBookmarks()
  addBookmark('/test/project/note.md', 'note')
  assert.ok(isBookmarked('/test/project/note.md'))
  const list = getBookmarks()
  assert.ok(list.some(b => b.path === '/test/project/note.md'))
})

test('addBookmark does not duplicate', async () => {
  const { addBookmark, getBookmarks } = await importBookmarks()
  addBookmark('/test/project/doc.md', 'doc')
  addBookmark('/test/project/doc.md', 'doc')
  const list = getBookmarks()
  const matches = list.filter(b => b.path === '/test/project/doc.md')
  assert.equal(matches.length, 1, 'no duplicates')
})

test('removeBookmark removes a bookmark', async () => {
  const { addBookmark, removeBookmark, isBookmarked } = await importBookmarks()
  addBookmark('/test/project/remove-me.md', 'remove')
  assert.ok(isBookmarked('/test/project/remove-me.md'))
  removeBookmark('/test/project/remove-me.md')
  assert.equal(isBookmarked('/test/project/remove-me.md'), false)
})

test('toggleBookmark toggles state', async () => {
  const { toggleBookmark, isBookmarked } = await importBookmarks()
  const path = '/test/project/toggle.md'
  assert.equal(isBookmarked(path), false)
  toggleBookmark(path, 'toggle')
  assert.ok(isBookmarked(path))
  toggleBookmark(path, 'toggle')
  assert.equal(isBookmarked(path), false)
})

test('bookmark groups can be added', async () => {
  const { addBookmarkGroup, listGroups } = await importBookmarks()
  const id = addBookmarkGroup('My Group')
  assert.ok(id, 'group id returned')
  const groups = listGroups()
  assert.ok(groups.some(g => g.name === 'My Group'))
})

test('renaming a bookmark group', async () => {
  const { addBookmarkGroup, renameBookmarkGroup, listGroups } = await importBookmarks()
  const id = addBookmarkGroup('Old Name')
  renameBookmarkGroup(id, 'New Name')
  const groups = listGroups()
  assert.ok(groups.some(g => g.name === 'New Name'))
})

test('moving a bookmark to a group', async () => {
  const { addBookmark, addBookmarkGroup, moveBookmarkToGroup, getBookmarks } = await importBookmarks()
  const bmPath = '/test/project/move-to-group.md'
  addBookmark(bmPath, 'move')
  const groupId = addBookmarkGroup('Target Group')
  moveBookmarkToGroup(bmPath, groupId)
  // It should still be findable
  const { isBookmarked } = await importBookmarks()
  assert.ok(isBookmarked(bmPath))
})
