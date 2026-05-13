const { app, BrowserWindow, ipcMain, dialog, Menu, shell, nativeImage } = require('electron')
const path = require('path')
const fs = require('fs')
const { execFile } = require('child_process')
const chokidar = require('chokidar')

let mainWindow
let watcher = null
const appIconPath = path.join(__dirname, '../../public/icon.svg')
const aboutIconPath = path.join(__dirname, '../../public/icon.png')

function escapeHtml(value = '') {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function buildExportHtml({ title, html, theme = 'dark', settings = {} }) {
  const isLight = theme === 'light'
  const bg = isLight ? '#f7f2e8' : '#0d0e10'
  const surface = isLight ? '#fffaf0' : '#121417'
  const text = isLight ? '#221d18' : '#dddfe6'
  const muted = isLight ? '#5f564b' : '#7a7d8a'
  const border = isLight ? 'rgba(34,29,24,0.12)' : 'rgba(255,255,255,0.08)'
  const accent = isLight ? '#5c7695' : '#5b7fa6'
  const previewFont = settings.previewFontCustom || settings.previewFont || "'DM Sans', system-ui, sans-serif"
  const previewFontSize = settings.previewFontSize || 13
  const previewLineHeight = settings.previewLineHeight || 1.75

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>${escapeHtml(title)}</title>
  <style>
    :root {
      --bg: ${bg};
      --surface: ${surface};
      --text: ${text};
      --muted: ${muted};
      --border: ${border};
      --accent: ${accent};
      --font: ${previewFont};
      --font-size: ${previewFontSize}px;
      --line-height: ${previewLineHeight};
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; padding: 0; background: var(--bg); color: var(--text); }
    body { font-family: var(--font); font-size: var(--font-size); line-height: var(--line-height); }
    .page {
      max-width: 860px;
      margin: 0 auto;
      padding: 48px 56px 72px;
      background: var(--surface);
    }
    .preview-pane { color: var(--text); }
    .preview-pane h1 { font-size: 1.7em; font-weight: 600; border-bottom: 1px solid var(--border); padding-bottom: 10px; margin: 0 0 14px; }
    .preview-pane h2 { font-size: 1.3em; font-weight: 600; margin: 24px 0 8px; }
    .preview-pane h3 { font-size: 1.08em; font-weight: 600; color: var(--muted); margin: 18px 0 6px; }
    .preview-pane p { margin: 0 0 12px; white-space: pre-wrap; }
    .preview-pane ul, .preview-pane ol { padding-left: 18px; margin: 0 0 12px; }
    .preview-pane li { margin: 4px 0; }
    .preview-pane code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.92em; background: rgba(127,127,127,0.14); padding: 1px 5px; border-radius: 4px; color: var(--accent); }
    .preview-pane pre { background: rgba(127,127,127,0.1); border: 1px solid var(--border); border-radius: 8px; padding: 14px 16px; margin: 12px 0; overflow-x: auto; }
    .preview-pane pre code { background: transparent; padding: 0; color: var(--muted); }
    .preview-pane blockquote { border-left: 2px solid var(--border); margin: 14px 0; padding: 3px 0 3px 14px; color: var(--muted); }
    .preview-pane a { color: var(--accent); text-decoration: none; }
    .preview-pane img { max-width: 100%; border-radius: 4px; margin: 8px 0; }
    .preview-pane hr { border: none; border-top: 1px solid var(--border); margin: 20px 0; }
    .preview-pane table { border-collapse: collapse; width: 100%; margin: 12px 0; }
    .preview-pane th { padding: 6px 10px; border-bottom: 1px solid var(--border); color: var(--muted); text-align: left; font-weight: 600; }
    .preview-pane td { padding: 5px 10px; border-bottom: 1px solid var(--border); }
    .preview-pane .callout { margin: 16px 0; padding: 12px 14px 12px 16px; border-left: 2px solid var(--accent); background: rgba(127,127,127,0.08); }
    .preview-pane .callout__title { margin: 0 0 8px; font-size: 0.86em; font-weight: 700; letter-spacing: 0.08em; text-transform: uppercase; color: var(--muted); }
    .preview-pane .callout__body > :last-child { margin-bottom: 0; }
    @page { margin: 18mm 16mm; }
  </style>
</head>
<body>
  <main class="page">
    <article class="preview-pane">${html}</article>
  </main>
</body>
</html>`
}

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

ipcMain.handle('dialog:pickImageFile', async () => {
  try {
    const result = await dialog.showOpenDialog(mainWindow, {
      properties: ['openFile'],
      filters: [
        { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'avif'] },
      ],
    })
    if (result.canceled || !result.filePaths.length) return null
    const filePath = result.filePaths[0]
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

// ── IPC: Create directory ─────────────────────────────────────────
ipcMain.handle('fs:createDir', async (_, dirPath) => {
  try {
    fs.mkdirSync(dirPath, { recursive: true })
    return true
  } catch {
    return false
  }
})

// ── IPC: Write image file ─────────────────────────────────────────
ipcMain.handle('fs:writeImageFile', async (_, dirPath, base64Data, fileName) => {
  try {
    fs.mkdirSync(dirPath, { recursive: true })
    const filePath = path.join(dirPath, fileName)
    const buffer = Buffer.from(base64Data, 'base64')
    fs.writeFileSync(filePath, buffer)
    return { path: filePath, name: fileName }
  } catch (err) {
    console.error('Image write error:', err)
    return null
  }
})

// ── IPC: Export to PDF ────────────────────────────────────────────
ipcMain.handle('export:pdf', async (_, payload) => {
  try {
    const fileName = typeof payload === 'string' ? payload : payload?.fileName
    const savePath = await dialog.showSaveDialog(mainWindow, {
      defaultPath: `${fileName.replace(/\.md$/, '')}.pdf`,
      filters: [{ name: 'PDF files', extensions: ['pdf'] }],
    })
    if (savePath.canceled || !savePath.filePath) return false

    const printWindow = new BrowserWindow({
      show: false,
      webPreferences: {
        sandbox: true,
      },
    })
    const html = buildExportHtml({
      title: fileName,
      html: payload?.html || '',
      theme: payload?.theme || 'dark',
      settings: payload?.settings || {},
    })

    await printWindow.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(html)}`)

    const pdfData = await printWindow.webContents.printToPDF({
      marginsType: 1,
      pageSize: 'A4',
      printBackground: true,
    })
    printWindow.destroy()
    fs.writeFileSync(savePath.filePath, pdfData)
    return true
  } catch (err) {
    console.error('PDF export error:', err)
    return false
  }
})

// ── IPC: Render D2 diagram ────────────────────────────────────────
ipcMain.handle('render:d2', async (_, source, themeId = 0) => {
  return new Promise((resolve) => {
    const args = ['-', '-', '--theme', String(themeId)]
    try {
      const child = execFile('d2', args, {
        timeout: 5000,
        maxBuffer: 1024 * 1024 * 4,
        env: { ...process.env },
      }, (error, stdout, stderr) => {
        if (error) {
          if (error.code === 'ENOENT') {
            return resolve({ error: 'D2 is not installed. Install from https://d2lang.com' })
          }
          if (error.killed) {
            return resolve({ error: 'D2 rendering timed out' })
          }
          return resolve({ error: stderr || error.message })
        }
        resolve({ svg: stdout })
      })
      child.stdin.write(source)
      child.stdin.end()
    } catch (err) {
      resolve({ error: err.message })
    }
  })
})
