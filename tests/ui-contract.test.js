const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')
const read = file => fs.readFileSync(path.join(root, file), 'utf8')

test('status bar owns a single responsive global control cluster with clear labels', () => {
  const shell = read('src/renderer/shell.js')

  assert.match(shell, /statusbar__metrics/)
  assert.match(shell, /statusbar__controls/)
  assert.match(shell, /aria-label="Toggle sidebar"/)
  assert.match(shell, /aria-label="Toggle toolbars"/)
  assert.match(shell, /aria-label="Toggle theme"/)
  assert.match(shell, /aria-label="Open settings"/)
})

test('settings panel uses compact tabs and visual preset swatches', () => {
  const shell = read('src/renderer/shell.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(shell, /settings-tabs/)
  assert.match(shell, /settings-preset-grid/)
  assert.match(shell, /settings-swatch/)
  assert.match(shell, /settings-advanced/)
  assert.match(css, /\.settings-page\.active/)
  assert.match(css, /\.settings-swatch/)
})

test('split workspace has an actionable empty secondary pane', () => {
  const workspace = read('src/renderer/workspace.js')

  assert.match(workspace, /data-action="open-secondary-file"/)
  assert.match(workspace, /toggleCommandPalette/)
  assert.match(workspace, /state\.tabGroups\.secondary = \[\]/)
  assert.match(workspace, /state\.secondaryTab = null/)
})

test('programmatic editor document updates do not mark tabs dirty', () => {
  const editor = read('src/renderer/editor.js')

  assert.match(editor, /Annotation/)
  assert.match(editor, /programmaticDocUpdate/)
  assert.match(editor, /!isProgrammatic/)
  assert.match(editor, /annotations: programmaticDocUpdate\.of\(true\)/)
})

test('autosave delay follows settings instead of a hardcoded timer', () => {
  const tabs = read('src/renderer/tabs.js')

  assert.match(tabs, /getSettings\(\)\.autoSaveDelay/)
  assert.doesNotMatch(tabs, /setTimeout\(\(\) => saveTab\(tab\), 800\)/)
})

test('dirty external changes are tracked and require overwrite confirmation', () => {
  const tabs = read('src/renderer/tabs.js')

  assert.match(tabs, /handleExternalFileChange/)
  assert.match(tabs, /externalConflict/)
  assert.match(tabs, /externalContent/)
  assert.match(tabs, /confirm\(/)
  assert.match(tabs, /confirmOverwrite/)
  assert.match(tabs, /External changes detected/)
})

test('external conflicts expose explicit reload and overwrite actions', () => {
  const tabs = read('src/renderer/tabs.js')

  assert.match(tabs, /reloadExternalChanges/)
  assert.match(tabs, /overwriteExternalChanges/)
  assert.match(tabs, /Reload from Disk/)
  assert.match(tabs, /Overwrite Disk/)
  assert.match(tabs, /External change/)
})

test('file watcher refreshes are batched instead of immediate per-event tree rebuilds', () => {
  const tabs = read('src/renderer/tabs.js')
  const shell = read('src/renderer/shell.js')
  const index = read('src/renderer/index.js')

  assert.match(tabs, /scheduleTreeRefresh/)
  assert.match(shell, /handleExternalFileChange/)
  assert.match(index, /handleExternalFileChange/)
  assert.doesNotMatch(shell, /onFileChange\(\(\{ event, path: p \}\) => \{\s*const tab[\s\S]*?_callbacks\.refreshTree\?\.\(\)\s*\}\)/)
})
