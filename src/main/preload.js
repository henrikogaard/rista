const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('fjord', {
  // App metadata
  appMeta: () => ipcRenderer.invoke('app:meta'),

  // Folder picker
  openFolder: () => ipcRenderer.invoke('dialog:openFolder'),
  newMarkdownFile: (folderPath) => ipcRenderer.invoke('dialog:newMarkdownFile', folderPath),
  pickImageFile: () => ipcRenderer.invoke('dialog:pickImageFile'),

  // File system
  readFolder: (p) => ipcRenderer.invoke('fs:readFolder', p),
  readFile: (p) => ipcRenderer.invoke('fs:readFile', p),
  writeFile: (p, content) => ipcRenderer.invoke('fs:writeFile', p, content),
  saveFileAs: (currentPath, content, folderPath) => ipcRenderer.invoke('fs:saveFileAs', currentPath, content, folderPath),
  watchFolder: (p) => ipcRenderer.invoke('fs:watchFolder', p),
  stat: (p) => ipcRenderer.invoke('fs:stat', p),
  createDir: (p) => ipcRenderer.invoke('fs:createDir', p),
  writeImageFile: (dirPath, base64Data, fileName) => ipcRenderer.invoke('fs:writeImageFile', dirPath, base64Data, fileName),

  // Export
  exportPdf: (fileName) => ipcRenderer.invoke('export:pdf', fileName),

  // Diagram rendering
  renderD2: (source, themeId) => ipcRenderer.invoke('render:d2', source, themeId),

  // File change events
  onFileChange: (cb) => {
    const listener = (_, data) => cb(data)
    ipcRenderer.on('fs:change', listener)
    return () => ipcRenderer.removeListener('fs:change', listener)
  },
  onCommand: (cb) => {
    const listener = (_, data) => cb(data)
    ipcRenderer.on('app:command', listener)
    return () => ipcRenderer.removeListener('app:command', listener)
  },
})
