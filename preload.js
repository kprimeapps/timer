const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
  getDisplays: () => ipcRenderer.invoke('get-displays'),
  openCast: (displayId) => ipcRenderer.invoke('open-cast', displayId),
  closeCast: () => ipcRenderer.invoke('close-cast'),
  sendCastState: (state) => ipcRenderer.send('cast-state', state),
  onCastState: (callback) => {
    ipcRenderer.on('cast-state', (_event, state) => callback(state));
  },
  onCastClosed: (callback) => {
    ipcRenderer.on('cast-closed', () => callback());
  },
});
