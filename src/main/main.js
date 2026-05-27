const { app, BrowserWindow, ipcMain, dialog, Menu, shell, nativeImage } = require('electron')
const path = require('path')
const fs = require('fs')
const http = require('http')
const https = require('https')
const { execFile } = require('child_process')
const chokidar = require('chokidar')
const { autoUpdater } = require('electron-updater')

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
    {
      label: 'Daily Note',
      accelerator: 'CmdOrCtrl+Shift+D',
      click: () => sendRendererCommand('file:daily-note'),
    },
    { type: 'separator' },
    {
      label: 'Export to PDF…',
      accelerator: 'CmdOrCtrl+E',
      click: () => sendRendererCommand('file:export-pdf'),
    },
    {
      label: 'Export as Website…',
      click: () => sendRendererCommand('file:export-website'),
    },
    { type: 'separator' },
    {
      label: 'Close Tab',
      accelerator: 'CmdOrCtrl+W',
      click: () => sendRendererCommand('file:close-tab'),
    },
  ]

  const viewSubmenu = [
    {
      label: 'Toggle Sidebar',
      accelerator: 'CmdOrCtrl+B',
      click: () => sendRendererCommand('view:toggle-sidebar'),
    },
    {
      label: 'Toggle Toolbar',
      accelerator: 'CmdOrCtrl+\\',
      click: () => sendRendererCommand('view:toggle-toolbar'),
    },
    {
      label: 'Toggle Inspector',
      click: () => sendRendererCommand('view:toggle-inspector'),
    },
    {
      label: 'Toggle Graph',
      click: () => sendRendererCommand('view:toggle-graph'),
    },
    {
      label: 'Toggle Calendar',
      click: () => sendRendererCommand('view:toggle-calendar'),
    },
    {
      label: 'Toggle Terminal',
      accelerator: 'CmdOrCtrl+`',
      click: () => sendRendererCommand('view:toggle-terminal'),
    },
    { type: 'separator' },
    {
      label: 'Toggle Theme',
      click: () => sendRendererCommand('view:toggle-theme'),
    },
    { type: 'separator' },
    {
      label: 'Quick Open…',
      accelerator: 'CmdOrCtrl+P',
      click: () => sendRendererCommand('view:quick-open'),
    },
    {
      label: 'Zen Mode',
      accelerator: 'CmdOrCtrl+Shift+Enter',
      click: () => sendRendererCommand('view:toggle-zen'),
    },
    { type: 'separator' },
    { role: 'resetZoom' },
    { role: 'zoomIn' },
    { role: 'zoomOut' },
    { type: 'separator' },
    { role: 'togglefullscreen' },
    { type: 'separator' },
    { role: 'toggleDevTools' },
  ]

  const template = []

  if (process.platform === 'darwin') {
    template.push({
      role: 'appMenu',
      submenu: [
        { role: 'about' },
        { type: 'separator' },
        {
          label: 'Settings…',
          accelerator: 'CmdOrCtrl+,',
          click: () => sendRendererCommand('view:settings'),
        },
        { type: 'separator' },
        { role: 'services' },
        { type: 'separator' },
        { role: 'hide' },
        { role: 'hideOthers' },
        { role: 'unhide' },
        { type: 'separator' },
        { role: 'quit' },
      ],
    })
  }

  template.push(
    { label: 'File', submenu: fileSubmenu },
    { role: 'editMenu' },
    { label: 'View', submenu: viewSubmenu },
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
    titleBarStyle: 'hidden',
    trafficLightPosition: { x: 14, y: 12 },
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
    applicationName: 'Rísta',
    applicationVersion: app.getVersion(),
    version: app.getVersion(),
    copyright: 'Author: Henrik Øgård',
    iconPath: aboutIconPath,
  })

  if (process.platform === 'darwin') {
    const appIcon = nativeImage.createFromPath(appIconPath)
    if (!appIcon.isEmpty()) app.dock.setIcon(appIcon)
  }
  createWindow()

  // Auto-update
  setTimeout(() => {
    try { autoUpdater.checkForUpdatesAndNotify() } catch {}
  }, 5000)
  setInterval(() => {
    try { autoUpdater.checkForUpdatesAndNotify() } catch {}
  }, 4 * 60 * 60 * 1000)

  autoUpdater.on('update-available', () => {
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('app:command', { command: 'update:available' })
    }
  })
  autoUpdater.on('update-downloaded', () => {
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('app:command', { command: 'update:ready' })
    }
  })

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

// ── macOS open-file event (double-click .md in Finder) ────────────
app.on('open-file', (event, filePath) => {
  event.preventDefault()
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('app:command', { command: 'file:open', path: filePath })
  }
})

ipcMain.handle('app:meta', async () => ({
  name: app.getName(),
  version: app.getVersion(),
}))

// ── IPC: Set represented file (macOS proxy icon) ──────────────────
ipcMain.handle('window:setRepresentedFile', async (_, filePath) => {
  if (process.platform === 'darwin' && mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.setRepresentedFilename(filePath || '')
  }
})

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
      } else if (isProjectVisibleFile(item.name)) {
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

function isProjectVisibleFile(name = '') {
  const lower = String(name).toLowerCase()
  return lower.endsWith('.md')
    || lower.endsWith('.markdown')
    || lower.endsWith('.png')
    || lower.endsWith('.jpg')
    || lower.endsWith('.jpeg')
    || lower.endsWith('.gif')
    || lower.endsWith('.svg')
    || lower.endsWith('.webp')
    || lower.endsWith('.bmp')
    || lower.endsWith('.pdf')
    || lower.endsWith('.fcanvas.json')
    || lower.endsWith('.fdraw.json')
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
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('fs:change', { event, path: changedPath })
    }
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

// ── IPC: Read templates ───────────────────────────────────────────
ipcMain.handle('fs:readTemplates', async (_, folderPath) => {
  const templatesDir = path.join(folderPath, '_templates')
  try {
    if (!fs.existsSync(templatesDir)) return []
    const files = fs.readdirSync(templatesDir).filter(f => f.endsWith('.md'))
    return files.map(f => ({
      name: f.replace(/\.md$/, ''),
      path: path.join(templatesDir, f),
      content: fs.readFileSync(path.join(templatesDir, f), 'utf-8'),
    }))
  } catch {
    return []
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

// ── IPC: Export to HTML ───────────────────────────────────────────
ipcMain.handle('export:html', async (_, payload) => {
  try {
    const fileName = payload?.fileName || 'export'
    const savePath = await dialog.showSaveDialog(mainWindow, {
      defaultPath: `${fileName.replace(/\.md$/, '')}.html`,
      filters: [{ name: 'HTML files', extensions: ['html'] }],
    })
    if (savePath.canceled || !savePath.filePath) return false
    const html = buildExportHtml({
      title: fileName,
      html: payload?.html || '',
      theme: payload?.theme || 'dark',
      settings: payload?.settings || {},
    })
    fs.writeFileSync(savePath.filePath, html, 'utf-8')
    return true
  } catch (err) {
    console.error('HTML export error:', err)
    return false
  }
})

// ── IPC: Export to DOCX ───────────────────────────────────────────
ipcMain.handle('export:docx', async (_, base64Data, fileName) => {
  try {
    const savePath = await dialog.showSaveDialog(mainWindow, {
      defaultPath: `${fileName}.docx`,
      filters: [{ name: 'Word documents', extensions: ['docx'] }],
    })
    if (savePath.canceled || !savePath.filePath) return false
    const buffer = Buffer.from(base64Data, 'base64')
    fs.writeFileSync(savePath.filePath, buffer)
    return true
  } catch (err) {
    console.error('DOCX export error:', err)
    return false
  }
})

// ── IPC: Settings export ──────────────────────────────────────────
ipcMain.handle('settings:export', async (_, jsonString) => {
  const savePath = await dialog.showSaveDialog(mainWindow, {
    defaultPath: 'fjordmark-settings.json',
    filters: [{ name: 'JSON files', extensions: ['json'] }],
  })
  if (savePath.canceled || !savePath.filePath) return false
  fs.writeFileSync(savePath.filePath, jsonString, 'utf-8')
  return true
})

// ── IPC: Settings import ──────────────────────────────────────────
ipcMain.handle('settings:import', async () => {
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ['openFile'],
    filters: [{ name: 'JSON files', extensions: ['json'] }],
  })
  if (result.canceled || !result.filePaths.length) return null
  return fs.readFileSync(result.filePaths[0], 'utf-8')
})

// ── IPC: Rename file ──────────────────────────────────────────────
ipcMain.handle('fs:renameFile', async (_, oldPath, newPath) => {
  try {
    fs.renameSync(oldPath, newPath)
    return true
  } catch {
    return false
  }
})

// ── IPC: Trash file ──────────────────────────────────────────────
ipcMain.handle('fs:trashFile', async (_, filePath) => {
  try {
    await shell.trashItem(filePath)
    return true
  } catch {
    return false
  }
})

// ── IPC: Duplicate file ──────────────────────────────────────────
ipcMain.handle('fs:duplicateFile', async (_, filePath) => {
  try {
    const ext = path.extname(filePath)
    const base = path.basename(filePath, ext)
    const dir = path.dirname(filePath)
    let copyPath = path.join(dir, `${base}-copy${ext}`)
    let counter = 1
    while (fs.existsSync(copyPath)) {
      counter++
      copyPath = path.join(dir, `${base}-copy-${counter}${ext}`)
    }
    fs.copyFileSync(filePath, copyPath)
    return { path: copyPath, name: path.basename(copyPath) }
  } catch {
    return null
  }
})

// ── IPC: Create file ─────────────────────────────────────────────
ipcMain.handle('fs:createFile', async (_, filePath) => {
  try {
    if (!fs.existsSync(filePath)) {
      fs.writeFileSync(filePath, '', 'utf-8')
    }
    return { path: filePath, name: path.basename(filePath) }
  } catch {
    return null
  }
})

// ── IPC: Reveal in Finder / Explorer ─────────────────────────────
ipcMain.handle('fs:showInFolder', async (_, filePath) => {
  shell.showItemInFolder(filePath)
  return true
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

// ── IPC: Read file as base64 (for attachments) ───────────────────
ipcMain.handle('fs:readFileBase64', async (_, filePath) => {
  try {
    const buffer = fs.readFileSync(filePath)
    return { data: buffer.toString('base64'), mimeType: 'application/octet-stream' }
  } catch (err) {
    return { error: err.message }
  }
})

// ── IPC: Run terminal command ────────────────────────────────────
// Hand the WHOLE command string to the shell — splitting on whitespace
// destroys quoted args, pipes, and options like `ls -la "My Folder"`.
ipcMain.handle('terminal:run', async (_, command, cwd) => {
  return new Promise((resolve) => {
    const { spawn } = require('child_process')
    // Prefer the user's login shell so PATH from .zshrc / .bash_profile is
    // respected (Electron's default /bin/sh sees a minimal env).
    const userShell = process.env.SHELL || '/bin/sh'
    // -l = login shell (sources profile), -c = run the following command string
    const child = spawn(userShell, ['-l', '-c', command], {
      cwd: cwd || process.cwd(),
      env: process.env,
    })
    let stdout = ''
    let stderr = ''
    let bytes = 0
    const MAX_BYTES = 4 * 1024 * 1024
    const onData = (buf, target) => {
      const s = buf.toString()
      bytes += s.length
      if (bytes > MAX_BYTES) {
        try { child.kill() } catch {}
        target.value += `\n[output truncated at ${MAX_BYTES} bytes]`
        return
      }
      target.value += s
    }
    const out = { value: '' }
    const err = { value: '' }
    child.stdout.on('data', d => onData(d, out))
    child.stderr.on('data', d => onData(d, err))
    child.on('close', (code) => {
      resolve({ stdout: out.value, stderr: err.value, code: code ?? 0 })
    })
    child.on('error', (e) => {
      resolve({ stdout: out.value, stderr: err.value, code: -1, error: e.message })
    })
  })
})

// ── IPC: AI Chat ─────────────────────────────────────────────────
// Unified message format from renderer:
//   { role: 'user', content: 'text' }
//   { role: 'assistant', content: 'text' | '', toolCalls?: [{ id, name, input }] }
//   { role: 'tool', toolCallId: 'id', content: 'result text' }
//   { role: 'system', content: 'text' }
//
// Response (unified):
//   { text: '...', toolCalls: [{ id, name, input }] | undefined, stop: 'end_turn'|'tool_use' }
//   { error: '...' }
ipcMain.handle('ai:chat', async (_, { provider, apiKey, model, baseUrl, messages, tools }) => {
  return new Promise((resolve) => {
    let url, headers, body

    if (provider === 'anthropic') {
      url = new URL(`${baseUrl}/v1/messages`)
      headers = {
        'Content-Type': 'application/json',
        'x-api-key': apiKey,
        'anthropic-version': '2023-06-01',
      }
      // Anthropic Messages API: `system` is a top-level field, not a message role.
      const systemParts = messages.filter(m => m.role === 'system').map(m => m.content)
      const conversation = []
      for (const m of messages) {
        if (m.role === 'system') continue
        if (m.role === 'assistant') {
          const blocks = []
          if (m.content) blocks.push({ type: 'text', text: m.content })
          if (Array.isArray(m.toolCalls)) {
            for (const tc of m.toolCalls) {
              blocks.push({ type: 'tool_use', id: tc.id, name: tc.name, input: tc.input || {} })
            }
          }
          // Anthropic rejects empty content arrays/strings — fall back to a
          // single whitespace text block so the conversation stays valid.
          conversation.push({ role: 'assistant', content: blocks.length ? blocks : [{ type: 'text', text: ' ' }] })
        } else if (m.role === 'tool') {
          // Anthropic tool results arrive as a user message with tool_result blocks
          const last = conversation[conversation.length - 1]
          const block = { type: 'tool_result', tool_use_id: m.toolCallId, content: m.content || '' }
          if (m.isError) block.is_error = true
          if (last && last.role === 'user' && Array.isArray(last.content)) {
            last.content.push(block)
          } else {
            conversation.push({ role: 'user', content: [block] })
          }
        } else {
          conversation.push({ role: m.role, content: m.content })
        }
      }
      const payload = { model, max_tokens: 4096, messages: conversation }
      if (systemParts.length) payload.system = systemParts.join('\n\n')
      if (Array.isArray(tools) && tools.length) payload.tools = tools
      body = JSON.stringify(payload)
    } else if (provider === 'openai') {
      url = new URL(`${baseUrl}/v1/chat/completions`)
      headers = {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${apiKey}`,
      }
      const openaiMessages = messages.map(m => {
        if (m.role === 'assistant') {
          const msg = { role: 'assistant', content: m.content || '' }
          if (Array.isArray(m.toolCalls) && m.toolCalls.length) {
            msg.tool_calls = m.toolCalls.map(tc => ({
              id: tc.id,
              type: 'function',
              function: { name: tc.name, arguments: JSON.stringify(tc.input || {}) },
            }))
          }
          return msg
        }
        if (m.role === 'tool') {
          return { role: 'tool', tool_call_id: m.toolCallId, content: m.content || '' }
        }
        return { role: m.role, content: m.content }
      })
      const payload = { model, messages: openaiMessages }
      if (Array.isArray(tools) && tools.length) payload.tools = tools
      body = JSON.stringify(payload)
    } else if (provider === 'ollama') {
      url = new URL(`${baseUrl}/api/chat`)
      headers = { 'Content-Type': 'application/json' }
      // Ollama only knows {role, content} — drop tool metadata, tools unsupported here
      const plain = messages.map(m => ({ role: m.role === 'tool' ? 'user' : m.role, content: m.content || '' }))
      body = JSON.stringify({ model, messages: plain, stream: false })
    } else {
      return resolve({ error: `Unknown provider: ${provider}` })
    }

    const transport = url.protocol === 'https:' ? https : http
    const req = transport.request(url, { method: 'POST', headers }, (res) => {
      let data = ''
      res.on('data', chunk => { data += chunk })
      res.on('end', () => {
        try {
          const json = JSON.parse(data)
          if (res.statusCode < 200 || res.statusCode >= 300) {
            const errMsg = json.error?.message || json.error?.type || JSON.stringify(json.error) || `HTTP ${res.statusCode}`
            return resolve({ error: errMsg })
          }
          let text = ''
          let toolCalls
          let stop
          if (provider === 'anthropic') {
            for (const block of json.content || []) {
              if (block.type === 'text') text += block.text || ''
              else if (block.type === 'tool_use') {
                toolCalls = toolCalls || []
                toolCalls.push({ id: block.id, name: block.name, input: block.input || {} })
              }
            }
            stop = json.stop_reason
          } else if (provider === 'openai') {
            const message = json.choices?.[0]?.message || {}
            text = message.content || ''
            if (Array.isArray(message.tool_calls) && message.tool_calls.length) {
              toolCalls = message.tool_calls.map(tc => {
                let input = {}
                try { input = JSON.parse(tc.function?.arguments || '{}') } catch {}
                return { id: tc.id, name: tc.function?.name, input }
              })
            }
            stop = json.choices?.[0]?.finish_reason
          } else if (provider === 'ollama') {
            text = json.message?.content || ''
          }
          resolve({ text, toolCalls, stop })
        } catch (err) {
          resolve({ error: `Failed to parse response: ${err.message}` })
        }
      })
    })
    req.on('error', (err) => resolve({ error: err.message }))
    req.setTimeout(120000, () => {
      req.destroy()
      resolve({ error: 'Request timed out' })
    })
    req.write(body)
    req.end()
  })
})

// ── IPC: Pick export folder ──────────────────────────────────────
ipcMain.handle('dialog:pickExportFolder', async () => {
  const result = await dialog.showOpenDialog(mainWindow, {
    title: 'Choose export destination',
    properties: ['openDirectory', 'createDirectory'],
  })
  if (result.canceled || !result.filePaths.length) return null
  return result.filePaths[0]
})

// ── IPC: Export as static site ───────────────────────────────────
ipcMain.handle('export:site', async (_, { outputDir, files }) => {
  try {
    if (!outputDir || !files?.length) return false
    fs.mkdirSync(outputDir, { recursive: true })
    for (const file of files) {
      const filePath = path.join(outputDir, file.name)
      fs.writeFileSync(filePath, file.html, 'utf-8')
    }
    return true
  } catch (err) {
    console.error('Site export error:', err)
    return false
  }
})

// ── IPC: List directory entries ──────────────────────────────────
ipcMain.handle('fs:listDir', async (_, dirPath) => {
  try {
    if (!fs.existsSync(dirPath)) return []
    return fs.readdirSync(dirPath)
  } catch {
    return []
  }
})

// ── IPC: Delete file ─────────────────────────────────────────────
ipcMain.handle('fs:deleteFile', async (_, filePath) => {
  try {
    if (fs.existsSync(filePath)) fs.unlinkSync(filePath)
    return true
  } catch {
    return false
  }
})

// ── IPC: Import external content as Markdown ─────────────────────
ipcMain.handle('clipper:import', async (_, { folderPath, title, body, sourceUrl }) => {
  try {
    const safeTitle = (title || 'untitled').replace(/[^a-zA-Z0-9\-_\s]/g, '').trim() || 'untitled'
    let fileName = `${safeTitle}.md`
    let filePath = path.join(folderPath, fileName)
    let counter = 1
    while (fs.existsSync(filePath)) {
      fileName = `${safeTitle}-${counter}.md`
      filePath = path.join(folderPath, fileName)
      counter++
    }
    const frontmatter = `---\ntitle: ${title || 'Untitled'}\nsource: ${sourceUrl || ''}\nimported: ${new Date().toISOString()}\n---\n\n`
    const content = frontmatter + (body || '')
    fs.writeFileSync(filePath, content, 'utf-8')
    return { path: filePath, name: fileName }
  } catch (err) {
    return { error: err.message }
  }
})
