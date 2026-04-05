const { app, BrowserWindow, ipcMain, dialog, Menu, shell, nativeImage } = require('electron')
const path = require('path')
const fs = require('fs')
const chokidar = require('chokidar')

let mainWindow
let watcher = null
const appIconPath = path.join(__dirname, '../../public/icon.svg')
const aboutIconPath = path.join(__dirname, '../../public/icon.png')

function sendRendererCommand(command) {
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('app:command', { command })
  }
}

function buildAppMenu() {
  const fileSubmenu = [
    {
      label: 'New File',
      accelerator: 'CmdOrCtrl+N',
      click: () => sendRendererCommand('file:new'),
    },
    {
      label: 'Open Folder…',
      accelerator: 'CmdOrCtrl+O',
      click: () => sendRendererCommand('file:open-folder'),
    },
    { type: 'separator' },
    {
      label: 'Save',
      accelerator: 'CmdOrCtrl+S',
      click: () => sendRendererCommand('file:save'),
    },
    {
      label: 'Save As…',
      accelerator: 'CmdOrCtrl+Shift+S',
      click: () => sendRendererCommand('file:save-as'),
    },
    { type: 'separator' },
    {
      label: 'Export to PDF…',
      accelerator: 'CmdOrCtrl+E',
      click: () => sendRendererCommand('file:export-pdf'),
    },
    { type: 'separator' },
    {
      label: 'Close Tab',
      accelerator: 'CmdOrCtrl+W',
      click: () => sendRendererCommand('file:close-tab'),
    },
  ]

  const template = []

  if (process.platform === 'darwin') {
    template.push({ role: 'appMenu' })
  }

  template.push(
    { label: 'File', submenu: fileSubmenu },
    { role: 'editMenu' },
    { role: 'viewMenu' },
    { role: 'windowMenu' }
  )

  if (process.platform !== 'darwin') {
    template.push({ role: 'help', submenu: [{ role: 'about' }] })
  }

  Menu.setApplicationMenu(Menu.buildFromTemplate(template))
}

function createWindow() {
  const appIcon = nativeImage.createFromPath(appIconPath)
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 600,
    minHeight: 400,
    titleBarStyle: 'hiddenInset',  // macOS: traffic lights inset
    backgroundColor: '#0d0e10',
    icon: appIcon.isEmpty() ? undefined : appIcon,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  })

  // In dev, load from dist; in prod same
  mainWindow.loadFile(path.join(__dirname, '../../public/index.html'))

  buildAppMenu()
}

app.whenReady().then(() => {
  app.setAboutPanelOptions({
    applicationName: 'Fjordmark',
    applicationVersion: app.getVersion(),
    version: app.getVersion(),
    copyright: 'Copyright Henrik Øgård',
    iconPath: aboutIconPath,
  })

  if (process.platform === 'darwin') {
    const appIcon = nativeImage.createFromPath(appIconPath)
    if (!appIcon.isEmpty()) app.dock.setIcon(appIcon)
  }
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

ipcMain.handle('app:meta', async () => ({
  name: app.getName(),
  version: app.getVersion(),
}))

// ── IPC: Open folder ──────────────────────────────────────────────
ipcMain.handle('dialog:openFolder', async () => {
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openDirectory'],
  })
  if (result.canceled || !result.filePaths.length) return null
  return result.filePaths[0]
})

ipcMain.handle('dialog:newMarkdownFile', async (_, folderPath) => {
  try {
    if (!folderPath) return null
    const result = await dialog.showSaveDialog(mainWindow, {
      defaultPath: path.join(folderPath, 'untitled.md'),
      filters: [{ name: 'Markdown files', extensions: ['md'] }],
    })
    if (result.canceled || !result.filePath) return null
    const filePath = result.filePath.endsWith('.md') ? result.filePath : `${result.filePath}.md`
    if (!fs.existsSync(filePath)) fs.writeFileSync(filePath, '', 'utf-8')
    return { path: filePath, name: path.basename(filePath) }
  } catch {
    return null
  }
})

// ── IPC: Read folder tree ─────────────────────────────────────────
ipcMain.handle('fs:readFolder', async (_, folderPath) => {
  return readFolderTree(folderPath)
})

function readFolderTree(folderPath) {
  const entries = []
  try {
    const items = fs.readdirSync(folderPath, { withFileTypes: true })
    for (const item of items) {
      if (item.name.startsWith('.')) continue
      const fullPath = path.join(folderPath, item.name)
      if (item.isDirectory()) {
        entries.push({
          type: 'folder',
          name: item.name,
          path: fullPath,
          children: readFolderTree(fullPath),
        })
      } else if (item.name.endsWith('.md')) {
        entries.push({
          type: 'file',
          name: item.name,
          path: fullPath,
        })
      }
    }
  } catch {}
  return entries.sort((a, b) => {
    if (a.type !== b.type) return a.type === 'folder' ? -1 : 1
    return a.name.localeCompare(b.name)
  })
}

// ── IPC: Read file ────────────────────────────────────────────────
ipcMain.handle('fs:readFile', async (_, filePath) => {
  try {
    return fs.readFileSync(filePath, 'utf-8')
  } catch {
    return ''
  }
})

// ── IPC: Write file ───────────────────────────────────────────────
ipcMain.handle('fs:writeFile', async (_, filePath, content) => {
  try {
    fs.writeFileSync(filePath, content, 'utf-8')
    return true
  } catch {
    return false
  }
})

ipcMain.handle('fs:saveFileAs', async (_, currentPath, content, folderPath) => {
  try {
    const baseDir = currentPath ? path.dirname(currentPath) : folderPath
    const defaultName = currentPath ? path.basename(currentPath) : 'untitled.md'
    const result = await dialog.showSaveDialog(mainWindow, {
      defaultPath: baseDir ? path.join(baseDir, defaultName) : defaultName,
      filters: [{ name: 'Markdown files', extensions: ['md'] }],
    })
    if (result.canceled || !result.filePath) return null
    const filePath = result.filePath.endsWith('.md') ? result.filePath : `${result.filePath}.md`
    fs.writeFileSync(filePath, content, 'utf-8')
    return { path: filePath, name: path.basename(filePath) }
  } catch {
    return null
  }
})

// ── IPC: Watch folder ─────────────────────────────────────────────
ipcMain.handle('fs:watchFolder', async (_, folderPath) => {
  if (watcher) await watcher.close()
  watcher = chokidar.watch(folderPath, {
    ignored: /(^|[/\\])\../,
    persistent: true,
    ignoreInitial: true,
  })
  watcher.on('all', (event, changedPath) => {
    if (!changedPath.endsWith('.md')) return
    mainWindow.webContents.send('fs:change', { event, path: changedPath })
  })
  return true
})

// ── IPC: Get file stats ───────────────────────────────────────────
ipcMain.handle('fs:stat', async (_, filePath) => {
  try {
    const s = fs.statSync(filePath)
    return { mtime: s.mtime.toISOString(), size: s.size }
  } catch {
    return null
  }
})

// ── IPC: Export to PDF ────────────────────────────────────────────
ipcMain.handle('export:pdf', async (_, fileName) => {
  try {
    const savePath = await dialog.showSaveDialog(mainWindow, {
      defaultPath: `${fileName.replace(/\.md$/, '')}.pdf`,
      filters: [{ name: 'PDF files', extensions: ['pdf'] }],
    })
    if (savePath.canceled || !savePath.filePath) return false

    // Render the preview pane to PDF
    const pdfData = await mainWindow.webContents.printToPDF({
      marginsType: 1,
      pageSize: 'A4',
      printBackground: false,
    })
    fs.writeFileSync(savePath.filePath, pdfData)
    return true
  } catch (err) {
    console.error('PDF export error:', err)
    return false
  }
})
