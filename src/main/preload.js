const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('fjord', {
  // App metadata
  appMeta: () => ipcRenderer.invoke('app:meta'),

  // Folder picker
  openFolder: () => ipcRenderer.invoke('dialog:openFolder'),
  newMarkdownFile: (folderPath) => ipcRenderer.invoke('dialog:newMarkdownFile', folderPath),

  // File system
  readFolder: (p) => ipcRenderer.invoke('fs:readFolder', p),
  readFile: (p) => ipcRenderer.invoke('fs:readFile', p),
  writeFile: (p, content) => ipcRenderer.invoke('fs:writeFile', p, content),
  saveFileAs: (currentPath, content, folderPath) => ipcRenderer.invoke('fs:saveFileAs', currentPath, content, folderPath),
  watchFolder: (p) => ipcRenderer.invoke('fs:watchFolder', p),
  stat: (p) => ipcRenderer.invoke('fs:stat', p),

  // Export
  exportPdf: (fileName) => ipcRenderer.invoke('export:pdf', fileName),

  // File change events
  onFileChange: (cb) => {
    ipcRenderer.on('fs:change', (_, data) => cb(data))
    return () => ipcRenderer.removeAllListeners('fs:change')
  },
  onCommand: (cb) => {
    ipcRenderer.on('app:command', (_, data) => cb(data))
    return () => ipcRenderer.removeAllListeners('app:command')
  },
})
