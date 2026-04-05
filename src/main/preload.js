const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('fjord', {
  // Folder picker
  openFolder: () => ipcRenderer.invoke('dialog:openFolder'),

  // File system
  readFolder: (p) => ipcRenderer.invoke('fs:readFolder', p),
  readFile: (p) => ipcRenderer.invoke('fs:readFile', p),
  writeFile: (p, content) => ipcRenderer.invoke('fs:writeFile', p, content),
  watchFolder: (p) => ipcRenderer.invoke('fs:watchFolder', p),
  stat: (p) => ipcRenderer.invoke('fs:stat', p),

  // File change events
  onFileChange: (cb) => {
    ipcRenderer.on('fs:change', (_, data) => cb(data))
    return () => ipcRenderer.removeAllListeners('fs:change')
  },
})
