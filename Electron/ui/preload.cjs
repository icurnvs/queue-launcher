const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('launcher', {
  action: (name, payload) => ipcRenderer.invoke('launcher:action', name, payload),
  bounds: (rect) => ipcRenderer.send('launcher:bounds', rect),
  onState: (callback) => {
    const listener = (_event, state) => callback(state);
    ipcRenderer.on('launcher:state', listener);
  },
});
