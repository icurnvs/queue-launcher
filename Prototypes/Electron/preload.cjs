const { ipcRenderer, contextBridge } = require('electron');

contextBridge.exposeInMainWorld('launcher', {
  state: () => ipcRenderer.invoke('launcher', 'state'),
  add: () => ipcRenderer.invoke('launcher', 'add'),
  select: (id) => ipcRenderer.invoke('launcher', 'select', id),
  close: () => ipcRenderer.invoke('launcher', 'close'),
  saveLink: () => ipcRenderer.invoke('launcher', 'saveLink'),
});
