const assert = require('node:assert/strict')
const fs = require('node:fs')
const path = require('node:path')
const test = require('node:test')

const root = path.resolve(__dirname, '..')
const read = file => fs.readFileSync(path.join(root, file), 'utf8')

test('status bar owns a single responsive global control cluster with clear labels', () => {
  const shell = read('src/renderer/shell.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(shell, /statusbar__metrics/)
  assert.match(shell, /statusbar__controls/)
  // Frequently-toggled workspace state lives in the status bar.
  assert.match(shell, /aria-label="Toggle file explorer"/)
  assert.match(shell, /aria-label="Toggle terminal"/)
  assert.match(shell, /aria-label="Toggle widgets panel"/)
  assert.match(shell, /aria-label="Open settings"/)
  // Theme and toolbar toggles moved out of the bar — they belong in the
  // View menu and Settings respectively.
  assert.doesNotMatch(shell, /id="theme-btn"/)
  assert.doesNotMatch(shell, /id="toolbar-toggle"/)
  assert.match(css, /\.statusbar\s*\{[\s\S]*cursor: default/)
  assert.match(css, /\.app-controls\s*\{[\s\S]*cursor: default/)
  assert.match(css, /\.app-controls \.theme-btn\s*\{[\s\S]*width: 22px/)
  assert.match(css, /\.app-controls \.theme-btn\s*\{[\s\S]*border: 1px solid transparent/)
  assert.match(css, /\.app-controls__sep\s*\{[\s\S]*cursor: default/)
})

test('settings panel uses compact tabs and visual preset swatches', () => {
  const shell = read('src/renderer/shell.js')
  const settings = read('src/renderer/settings.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(shell, /settings-tabs/)
  assert.match(shell, /settings-preset-grid/)
  assert.match(shell, /settings-swatch/)
  assert.match(shell, /settings-advanced/)
  assert.match(shell, /Installed app font or stack/)
  assert.match(shell, /Installed editor font or stack/)
  assert.match(settings, /const PROPORTIONAL_FONT_OPTIONS/)
  assert.match(settings, /const MONO_FONT_OPTIONS/)
  assert.match(settings, /ui: PROPORTIONAL_FONT_OPTIONS/)
  assert.match(settings, /explorer: PROPORTIONAL_FONT_OPTIONS/)
  assert.match(settings, /preview: PROPORTIONAL_FONT_OPTIONS/)
  assert.match(settings, /editor: MONO_FONT_OPTIONS/)
  assert.doesNotMatch(settings, /Avenir \/ Segoe/)
  assert.doesNotMatch(settings, /Iowan \/ Palatino/)
  assert.match(css, /\.settings-page\.active/)
  assert.match(css, /\.settings-swatch/)
})

test('settings exposes selectable Split Rune app icon variants', () => {
  const settings = read('src/renderer/settings.js')
  const shell = read('src/renderer/shell.js')
  const css = read('src/renderer/styles/main.css')
  const adapter = read('src/renderer/tauri-api.js')
  const tauriMain = read('src-tauri/src/main.rs')

  assert.match(settings, /APP_ICON_VARIANTS/)
  assert.match(settings, /appIconVariant: 'aurora-gradient'/)
  assert.match(settings, /appIconTheme: 'auto'/)
  assert.match(shell, /renderAppIconPicker/)
  assert.match(shell, /data-app-icon-variant/)
  assert.match(shell, /applySelectedAppIcon/)
  assert.match(css, /\.settings-icon-grid/)
  assert.match(adapter, /setAppIcon: \(variant, theme\) => invoke\('set_app_icon'/)
  assert.match(tauriMain, /fn set_app_icon/)
  assert.ok(fs.existsSync(path.join(root, 'public/logos/rista-split-rune-aurora-gradient-dark.svg')))
  assert.ok(fs.existsSync(path.join(root, 'src-tauri/icons/rista-split-rune-aurora-gradient-dark.png')))
})

test('macOS install copy verifies the bundled app icon', () => {
  const pkg = JSON.parse(read('package.json'))
  const installScript = read('scripts/install-macos-app.js')
  const fixScript = read('scripts/fix-macos-bundle-icon.js')

  assert.equal(pkg.scripts.package, 'tauri build --bundles app && node scripts/fix-macos-bundle-icon.js')
  assert.equal(pkg.scripts['install:mac'], 'node scripts/install-macos-app.js')
  assert.match(fixScript, /Rista\.icns/)
  assert.match(fixScript, /Rísta\.icns/)
  assert.match(fixScript, /Set :CFBundleIconFile/)
  assert.match(installScript, /Contents\/Resources\/Rista\.icns/)
  assert.match(installScript, /CFBundleIconFile/)
  assert.match(installScript, /assertBundleIcon\(sourceApp\)/)
  assert.match(installScript, /fs\.cpSync\(sourceApp, targetApp/)
  assert.match(installScript, /assertBundleIcon\(targetApp\)/)
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

test('sidebar resize updates CSS during drag and persists once on release', () => {
  const tabs = read('src/renderer/tabs.js')
  const rightPanel = read('src/renderer/right-panel.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(tabs, /function startSidebarResize/)
  assert.match(tabs, /requestAnimationFrame\(applyWidth\)/)
  assert.match(tabs, /style\.setProperty\('--sidebar-width'/)
  assert.match(tabs, /cancelAnimationFrame\(frame\)/)
  assert.match(tabs, /onUp[\s\S]*updateSetting\('sidebarWidth', pendingWidth\)/)
  assert.doesNotMatch(tabs, /onMove[\s\S]{0,220}updateSetting\('sidebarWidth'/)
  assert.match(rightPanel, /requestAnimationFrame\(applyWidth\)/)
  assert.match(rightPanel, /style\.setProperty\('--right-sidebar-width'/)
  assert.match(rightPanel, /cancelAnimationFrame\(frame\)/)
  assert.match(rightPanel, /onUp[\s\S]*localStorage\.setItem\('fjordmark-right-sidebar-width'/)
  assert.match(css, /\.sidebar-resizer\s*\{[\s\S]*background: transparent/)
  assert.match(css, /\.sidebar-resizer\s*\{[\s\S]*margin-left: -5px/)
  assert.match(css, /\.sidebar-resizer::before\s*\{[\s\S]*width: 1px/)
})

test('tag pane is a movable widget backed by the link index tag data', () => {
  const index = read('src/renderer/index.js')
  const tagsView = read('src/renderer/tags-view.js')
  const explorer = read('src/renderer/file-explorer-view.js')
  const tabs = read('src/renderer/tabs.js')
  const state = read('src/renderer/state.js')

  assert.match(index, /registerRightPanel\('tags'/)
  assert.match(index, /tagIcon\(\)/)
  assert.match(index, /onLinkIndexChange\(.*renderTagsPanel/s)
  assert.match(index, /mountTagsPanel\(openFile,[\s\S]*state\.tagFilter = null/)
  assert.match(tagsView, /getAllTagNames/)
  assert.match(tagsView, /getFilesForTag/)
  assert.match(tagsView, /buildTagsPanel/)
  assert.match(tagsView, /mountTagsPanel/)
  assert.match(tagsView, /tags-selection/)
  assert.match(tagsView, /data-action="clear-tag-filter"/)
  assert.match(state, /tagFilter: null/)
  assert.doesNotMatch(explorer, /file-explorer-filter/)
  assert.doesNotMatch(explorer, /clearTagFilter/)
  assert.match(tabs, /getFilesForTag\(state\.tagFilter\)/)
})

test('right sidebar widget tabs expose only honest visible hit targets', () => {
  const rightPanel = read('src/renderer/right-panel.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(rightPanel, /class="right-sidebar__tab/)
  assert.match(css, /\.right-sidebar__tabs\s*\{[\s\S]*cursor: default/)
  assert.match(css, /\.right-sidebar__tab\s*\{[\s\S]*width: 22px/)
  assert.match(css, /\.right-sidebar__tab\s*\{[\s\S]*height: 22px/)
  assert.match(css, /\.right-sidebar__tab\s*\{[\s\S]*border: 1px solid var\(--surface-edge\)/)
  assert.match(css, /\.right-sidebar__tab svg\s*\{[\s\S]*pointer-events: none/)
})

test('sidebars own the full window edges around the center chrome', () => {
  const shell = read('src/renderer/shell.js')
  const rightPanel = read('src/renderer/right-panel.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(shell, /<div class="layout">[\s\S]*<div class="sidebar" id="sidebar">[\s\S]*sidebar-drag-region[\s\S]*<div class="editor-area">[\s\S]*<div class="brandrail"/)
  assert.match(rightPanel, /right-sidebar__drag-region/)
  assert.match(shell, /<div class="editor-area">[\s\S]*<div class="statusbar">[\s\S]*\$\{buildRightPanelContainer\(\)\}/)
  assert.doesNotMatch(shell, /<div class="app" id="app">\s*<!-- Top window rail -->/)
  assert.match(css, /\.layout\s*\{[\s\S]*min-height: 0/)
  assert.match(css, /\.editor-area\s*\{[\s\S]*min-height: 0/)
  assert.match(css, /\.sidebar-drag-region\s*\{[\s\S]*height: var\(--brandrail-height\)/)
  assert.match(css, /\.right-sidebar__drag-region\s*\{[\s\S]*height: var\(--brandrail-height\)/)
  assert.match(css, /\.right-sidebar\s*\{[\s\S]*backdrop-filter: blur\(calc\(var\(--surface-blur\) \* 0\.72\)\)/)
})

test('right-panel mounts graph widgets after their DOM is committed', () => {
  const rightPanel = read('src/renderer/right-panel.js')
  const graphView = read('src/renderer/graph-view.js')
  const linkIndex = read('src/renderer/link-index.js')

  assert.match(rightPanel, /const pendingMounts = \[\]/)
  assert.match(rightPanel, /pendingMounts\.push\(hooks\)/)
  assert.match(rightPanel, /stack\.replaceChildren\(fragment\)[\s\S]*pendingMounts\.forEach/)
  assert.doesNotMatch(rightPanel, /requestAnimationFrame\(\(\) => hooks\.onMount/)
  assert.match(graphView, /resolveWikilink/)
  assert.match(graphView, /targetPath = resolveWikilink/)
  assert.match(linkIndex, /normalizeWikilinkTarget/)
  assert.match(linkIndex, /\.split\('#'\)/)
  assert.match(linkIndex, /normalizedPath/)
})

test('settings exposes editable hotkeys with conflict-aware capture and reset', () => {
  const shell = read('src/renderer/shell.js')
  const keybindings = read('src/renderer/keybindings.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(shell, /\{ id: 'hotkeys', label: 'Hotkeys' \}/)
  assert.match(shell, /getAllBindings\(\)/)
  assert.match(shell, /data-keybinding-capture/)
  assert.match(shell, /data-keybinding-reset/)
  assert.match(shell, /findConflict\(/)
  assert.match(keybindings, /export function formatKeyEvent/)
  assert.match(css, /\.keybinding-list/)
})

test('quick open includes heading matches and opens selected headings by line', () => {
  const palette = read('src/renderer/command-palette.js')
  const tabs = read('src/renderer/tabs.js')

  assert.match(palette, /extractHeadings/)
  assert.match(palette, /type: 'heading'/)
  assert.match(palette, /heading:\s*\{/)
  assert.match(palette, /openFile\(\{ path: result\.item\.path, name: result\.item\.name, heading: result\.item\.heading \}\)/)
  assert.match(tabs, /jumpToLine/)
  assert.match(tabs, /item\.heading\.line/)
})

test('desktop shell is a Tauri app rather than Electron', () => {
  const pkg = JSON.parse(read('package.json'))
  const tauriConfig = read('src-tauri/tauri.conf.json')
  const cargo = read('src-tauri/Cargo.toml')

  assert.equal(pkg.name, 'rista')
  assert.equal(pkg.scripts.dev, 'tauri dev')
  assert.equal(pkg.scripts.package, 'tauri build --bundles app && node scripts/fix-macos-bundle-icon.js')
  assert.ok(pkg.devDependencies['@tauri-apps/cli'])
  assert.ok(pkg.dependencies['@tauri-apps/api'])
  assert.ok(!pkg.devDependencies.electron)
  assert.ok(!pkg.devDependencies['electron-builder'])
  assert.ok(!pkg.dependencies['electron-updater'])
  assert.match(tauriConfig, /"productName": "Rísta"/)
  assert.match(tauriConfig, /"identifier": "app\.rista\.desktop"/)
  assert.match(tauriConfig, /"frontendDist": "\.\.\/dist\/app"/)
  assert.match(cargo, /tauri =/)
  assert.match(cargo, /notify =/)
})

test('renderer installs the Tauri-backed window.fjord compatibility API', () => {
  const index = read('src/renderer/index.js')
  const adapter = read('src/renderer/tauri-api.js')

  assert.match(index, /import '\.\/tauri-api\.js'/)
  assert.match(adapter, /from '@tauri-apps\/api\/core'/)
  assert.match(adapter, /from '@tauri-apps\/api\/event'/)
  assert.match(adapter, /window\.fjord =/)
  assert.match(adapter, /openFolder/)
  assert.match(adapter, /readFolder/)
  assert.match(adapter, /watchFolder/)
  assert.match(adapter, /onFileChange/)
  assert.match(adapter, /onCommand/)
})

test('visible app identity is rebranded to Rista', () => {
  const html = read('public/index.html')
  const shell = read('src/renderer/shell.js')
  const state = read('src/renderer/state.js')
  const readme = read('README.md')

  assert.match(html, /<title>Rísta<\/title>/)
  assert.match(shell, /Rísta/)
  assert.doesNotMatch(shell, /fjord<span>mark<\/span>/)
  assert.match(state, /appMeta: \{ name: 'Rísta'/)
  assert.match(readme, /^# Rísta/m)
  assert.match(readme, /Tauri/)
  assert.doesNotMatch(readme, /Electron 29/)
})

test('Tauri uses native macOS overlay chrome with app-owned branding', () => {
  const tauriConfig = read('src-tauri/tauri.conf.json')
  const shell = read('src/renderer/shell.js')
  const css = read('src/renderer/styles/main.css')
  const adapter = read('src/renderer/tauri-api.js')
  const capability = read('src-tauri/capabilities/default.json')

  assert.match(tauriConfig, /"decorations": true/)
  assert.match(tauriConfig, /"shadow": true/)
  assert.match(tauriConfig, /"transparent": false/)
  assert.match(tauriConfig, /"titleBarStyle": "Overlay"/)
  assert.match(tauriConfig, /"hiddenTitle": true/)
  assert.match(tauriConfig, /"trafficLightPosition": \{\s*"x": 15,\s*"y": 17\s*\}/)
  assert.doesNotMatch(shell, /titlebar__spacer/)
  assert.doesNotMatch(shell, /window-control/)
  assert.doesNotMatch(shell, /data-window-action/)
  assert.match(shell, /handleWindowDragRegionPointerDown/)
  assert.match(shell, /sidebar-drag-region/)
  assert.match(shell, /right-sidebar-drag-region/)
  assert.match(shell, /brandrail__mark/)
  assert.match(shell, /data-tauri-drag-region/)
  assert.match(css, /\.brandrail/)
  assert.match(css, /html, body, #root\s*\{[\s\S]*margin: 0/)
  assert.match(css, /\.brandrail__identity/)
  assert.match(css, /left: 50%/)
  assert.match(css, /transform: translateX\(-50%\)/)
  assert.doesNotMatch(css, /\.window-control/)
  assert.match(adapter, /getCurrentWindow/)
  assert.match(adapter, /windowAction/)
  assert.match(adapter, /startWindowDrag/)
  assert.match(capability, /core:window:allow-close/)
  assert.match(capability, /core:window:allow-minimize/)
  assert.match(capability, /core:window:allow-toggle-maximize/)
  assert.match(capability, /core:window:allow-start-dragging/)
})

test('Tauri chrome positions Rista branding in the top overlay rail', () => {
  const shell = read('src/renderer/shell.js')
  const css = read('src/renderer/styles/main.css')

  assert.match(shell, /brandrail__mark/)
  assert.match(shell, /brandrail__wordmark/)
  assert.match(shell, /brandrail__identity/)
  assert.match(shell, /Rísta/)
  assert.doesNotMatch(shell, /local markdown, carved cleanly/)
  assert.match(css, /\.brandrail__mark/)
  assert.match(css, /\.brandrail__path\s*\{[\s\S]*display: none/)
})

test('widgets use pointer-driven repositioning instead of native HTML drag events', () => {
  const rightPanel = read('src/renderer/right-panel.js')

  assert.match(rightPanel, /data-action="widget-drag"/)
  assert.match(rightPanel, /function startWidgetDrag/)
  assert.match(rightPanel, /closest\('\.widget__header'\)/)
  assert.match(rightPanel, /toggleWidgetCollapse\(dragId\)/)
  assert.match(rightPanel, /pointermove/)
  assert.match(rightPanel, /onCancel/)
  assert.match(rightPanel, /finishDrag\(upEvent, false\)/)
  assert.match(rightPanel, /finishDrag\(cancelEvent, true\)/)
  assert.match(rightPanel, /elementFromPoint/)
  assert.doesNotMatch(rightPanel, /draggable="true"/)
  assert.doesNotMatch(rightPanel, /addEventListener\('dragstart'/)
})

test('app chrome is non-selectable while editing surfaces keep text selection', () => {
  const css = read('src/renderer/styles/main.css')

  assert.match(css, /body\s*\{[\s\S]*user-select: none/)
  assert.match(css, /\.cm-content,[\s\S]*user-select: text/)
  assert.match(css, /input,[\s\S]*textarea,[\s\S]*\[contenteditable="true"\][\s\S]*user-select: text/)
})
