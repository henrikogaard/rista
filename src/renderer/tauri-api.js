import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { Menu } from '@tauri-apps/api/menu'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open, save } from '@tauri-apps/plugin-dialog'
import { revealItemInDir } from '@tauri-apps/plugin-opener'

const commandListeners = new Set()

function emitCommand(command, extra = {}) {
  const payload = { command, ...extra }
  for (const listener of commandListeners) listener(payload)
}

function ensureMdPath(path) {
  if (!path) return path
  return /\.(md|markdown)$/i.test(path) ? path : `${path}.md`
}

function fileName(path) {
  return String(path || '').split(/[\\/]/).pop()
}

async function pickSavePath(defaultPath, filters) {
  return save({ defaultPath, filters })
}

async function installAppMenu() {
  try {
    const item = (id, text, accelerator) => ({
      id,
      text,
      accelerator,
      action: () => emitCommand(id),
    })

    const menu = await Menu.new({
      items: [
        {
          text: 'Rísta',
          items: [
            item('view:settings', 'Settings...', 'CmdOrCtrl+,'),
            { item: 'Separator' },
            { item: 'Services' },
            { item: 'Separator' },
            { item: 'Hide' },
            { item: 'HideOthers' },
            { item: 'ShowAll' },
            { item: 'Separator' },
            { item: 'Quit' },
          ],
        },
        {
          text: 'File',
          items: [
            item('file:new', 'New File', 'CmdOrCtrl+N'),
            item('file:open-folder', 'Open Folder...', 'CmdOrCtrl+O'),
            { item: 'Separator' },
            item('file:save', 'Save', 'CmdOrCtrl+S'),
            item('file:save-as', 'Save As...', 'CmdOrCtrl+Shift+S'),
            item('file:daily-note', 'Daily Note', 'CmdOrCtrl+Shift+D'),
            { item: 'Separator' },
            item('file:export-pdf', 'Export to PDF...', 'CmdOrCtrl+E'),
            item('file:export-website', 'Export as Website...'),
            { item: 'Separator' },
            item('file:close-tab', 'Close Tab', 'CmdOrCtrl+W'),
          ],
        },
        {
          text: 'Edit',
          items: [
            { item: 'Undo' },
            { item: 'Redo' },
            { item: 'Separator' },
            { item: 'Cut' },
            { item: 'Copy' },
            { item: 'Paste' },
            { item: 'SelectAll' },
          ],
        },
        {
          text: 'View',
          items: [
            item('view:toggle-sidebar', 'Toggle Sidebar', 'CmdOrCtrl+B'),
            item('view:toggle-toolbar', 'Toggle Toolbar', 'CmdOrCtrl+\\'),
            item('view:toggle-inspector', 'Toggle Inspector'),
            item('view:toggle-graph', 'Toggle Graph'),
            item('view:toggle-calendar', 'Toggle Calendar'),
            item('view:toggle-terminal', 'Toggle Terminal', 'CmdOrCtrl+`'),
            { item: 'Separator' },
            item('view:toggle-theme', 'Toggle Theme'),
            { item: 'Separator' },
            item('view:quick-open', 'Quick Open...', 'CmdOrCtrl+P'),
            item('view:toggle-zen', 'Zen Mode', 'CmdOrCtrl+Shift+Enter'),
          ],
        },
        {
          text: 'Window',
          items: [
            { item: 'Minimize' },
            { item: 'Zoom' },
            { item: 'Separator' },
            { item: 'ToggleFullscreen' },
          ],
        },
      ],
    })
    await menu.setAsAppMenu()
  } catch (err) {
    console.warn('[rista] Failed to install app menu:', err)
  }
}

async function exportFileName(fileNameValue, extension) {
  const base = String(fileNameValue || 'export').replace(/\.(md|markdown)$/i, '')
  return `${base}.${extension}`
}

if (window.__TAURI_INTERNALS__) {
  const currentWindow = getCurrentWindow()

  window.fjord = {
    appMeta: () => invoke('app_meta'),
    windowAction: (action) => {
      if (action === 'close') return currentWindow.close()
      if (action === 'minimize') return currentWindow.minimize()
      if (action === 'toggle-maximize') return currentWindow.toggleMaximize()
      return Promise.resolve(false)
    },
    startWindowDrag: () => currentWindow.startDragging(),

    openFolder: async () => open({ multiple: false, directory: true }),
    newMarkdownFile: async (folderPath) => {
      const picked = await save({
        defaultPath: folderPath ? `${folderPath}/untitled.md` : 'untitled.md',
        filters: [{ name: 'Markdown files', extensions: ['md'] }],
      })
      if (!picked) return null
      const path = ensureMdPath(picked)
      return invoke('create_file', { path })
    },
    pickImageFile: async () => {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'avif'] }],
      })
      return picked ? { path: picked, name: fileName(picked) } : null
    },

    readFolder: (path) => invoke('read_folder', { path }),
    readFile: (path) => invoke('read_file', { path }),
    readFileBase64: (path) => invoke('read_file_base64', { path }),
    writeFile: (path, content) => invoke('write_file', { path, content }),
    saveFileAs: async (currentPath, content, folderPath) => {
      const defaultPath = currentPath || (folderPath ? `${folderPath}/untitled.md` : 'untitled.md')
      const picked = await save({
        defaultPath,
        filters: [{ name: 'Markdown files', extensions: ['md'] }],
      })
      if (!picked) return null
      const path = ensureMdPath(picked)
      const ok = await invoke('write_file', { path, content })
      return ok ? { path, name: fileName(path) } : null
    },
    watchFolder: (path) => invoke('watch_folder', { path }),
    stat: (path) => invoke('stat_file', { path }),
    createDir: (path) => invoke('create_dir', { path }),
    writeImageFile: (dirPath, base64Data, fileNameValue) => invoke('write_image_file', { dirPath, base64Data, fileName: fileNameValue }),
    renameFile: (oldPath, newPath) => invoke('rename_file', { oldPath, newPath }),
    trashFile: (path) => invoke('trash_file', { path }),
    duplicateFile: (path) => invoke('duplicate_file', { path }),
    createFile: (path) => invoke('create_file', { path }),
    showInFolder: async (path) => {
      await revealItemInDir(path)
      return true
    },
    readTemplates: (folderPath) => invoke('read_templates', { folderPath }),
    listDir: (path) => invoke('list_dir', { path }),
    deleteFile: (path) => invoke('delete_file', { path }),

    runTerminalCommand: (command, cwd) => invoke('run_terminal_command', { command, cwd }),
    importContent: ({ folderPath, title, body, sourceUrl }) => invoke('import_content', { folderPath, title, body, sourceUrl }),
    setRepresentedFile: (path) => invoke('set_represented_file', { path }),
    setAppIcon: (variant, theme) => invoke('set_app_icon', { variant, theme }),

    exportPdf: () => invoke('export_pdf'),
    exportHtml: async (payload) => {
      const path = await pickSavePath(await exportFileName(payload?.fileName, 'html'), [{ name: 'HTML files', extensions: ['html'] }])
      return path ? invoke('export_html', { path, payload }) : false
    },
    saveDocx: async (base64Data, fileNameValue) => {
      const path = await pickSavePath(`${fileNameValue}.docx`, [{ name: 'Word documents', extensions: ['docx'] }])
      return path ? invoke('save_binary_file', { path, base64Data }) : false
    },
    exportSite: ({ outputDir, files }) => invoke('export_site', { outputDir, files }),
    pickExportFolder: () => open({ multiple: false, directory: true }),

    exportSettings: async (jsonString) => {
      const path = await pickSavePath('rista-settings.json', [{ name: 'JSON files', extensions: ['json'] }])
      return path ? invoke('write_file', { path, content: jsonString }) : false
    },
    importSettings: async () => {
      const path = await open({ multiple: false, directory: false, filters: [{ name: 'JSON files', extensions: ['json'] }] })
      return path ? invoke('read_file', { path }) : null
    },

    aiChat: (params) => invoke('ai_chat', { params }),
    renderD2: (source, themeId) => invoke('render_d2', { source, themeId }),

    onFileChange: (cb) => {
      let unlisten = null
      listen('fs-change', (event) => cb(event.payload)).then((fn) => { unlisten = fn })
      return () => unlisten?.()
    },
    onCommand: (cb) => {
      commandListeners.add(cb)
      return () => commandListeners.delete(cb)
    },
  }

  installAppMenu()
}
