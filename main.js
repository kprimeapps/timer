const { app, BrowserWindow, ipcMain, screen } = require('electron');
const path = require('path');

let mainWindow = null;
let castWindow = null;

function createMainWindow() {
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 600,
    minHeight: 400,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
    autoHideMenuBar: true,
    title: 'Program Timer',
    backgroundColor: '#0a0c10',
  });

  mainWindow.loadFile('timer.html');

  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

app.whenReady().then(createMainWindow);

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) createMainWindow();
});

ipcMain.handle('get-displays', () => {
  return screen.getAllDisplays().map(d => ({
    id: d.id,
    bounds: { x: d.bounds.x, y: d.bounds.y, width: d.bounds.width, height: d.bounds.height },
    primary: d.id === screen.getPrimaryDisplay().id,
    internal: d.internal,
    scaleFactor: d.scaleFactor,
  }));
});

ipcMain.handle('open-cast', (_event, displayId) => {
  if (castWindow && !castWindow.isDestroyed()) {
    castWindow.close();
    castWindow = null;
  }

  const displays = screen.getAllDisplays();
  let display = null;

  if (displayId != null) {
    display = displays.find(d => d.id === displayId);
  }

  if (!display) {
    display = displays.find(d => !d.primary) || displays[0];
  }

  castWindow = new BrowserWindow({
    x: display.bounds.x,
    y: display.bounds.y,
    width: display.bounds.width,
    height: display.bounds.height,
    fullscreen: true,
    frame: false,
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
    backgroundColor: '#0a0c10',
  });

  castWindow.loadFile('receiver.html');

  castWindow.on('closed', () => {
    castWindow = null;
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('cast-closed');
    }
  });

  return true;
});

ipcMain.handle('close-cast', () => {
  if (castWindow && !castWindow.isDestroyed()) {
    castWindow.close();
  }
  castWindow = null;
  return true;
});

ipcMain.on('cast-state', (_event, state) => {
  if (castWindow && !castWindow.isDestroyed()) {
    castWindow.webContents.send('cast-state', state);
  }
});
