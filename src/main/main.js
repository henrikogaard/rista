const { app, BrowserWindow, ipcMain, dialog, Menu, shell } = require('electron')
const path = require('path')
const fs = require('fs')
const chokidar = require('chokidar')

let mainWindow
let watcher = null

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 600,
    minHeight: 400,
    titleBarStyle: 'hiddenInset',  // macOS: traffic lights inset
    backgroundColor: '#0d0e10',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  })

  // In dev, load from dist; in prod same
  mainWindow.loadFile(path.join(__dirname, '../../public/index.html'))

  // Remove default menu on non-mac
  if (process.platform !== 'darwin') Menu.setApplicationMenu(null)
}

app.whenReady().then(() => {
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

// ── IPC: Open folder ──────────────────────────────────────────────
ipcMain.handle('dialog:openFolder', async () => {
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openDirectory'],
  })
  if (result.canceled || !result.filePaths.length) return null
  return result.filePaths[0]
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
