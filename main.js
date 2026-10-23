'use strict';
const { app, BrowserWindow, ipcMain, dialog, shell, Menu, clipboard, nativeTheme, screen } = require('electron');
const path = require('path');
const fs = require('fs');
const { spawn } = require('child_process');
const gitService = require('./src/git');

const IS_MAC = process.platform === 'darwin';
const TITLEBAR_HEIGHT = 40;
const THEMES = {
  dark: { color: '#14161b', symbolColor: '#c9cfd9', bg: '#14161b' },
  light: { color: '#eef0f4', symbolColor: '#2a2f38', bg: '#f7f8fa' },
};

let win = null;

// ---------------------------------------------------------------- persisted state

const storeFile = () => path.join(app.getPath('userData'), 'state.json');

function readStore() {
  try {
    return JSON.parse(fs.readFileSync(storeFile(), 'utf8')) || {};
  } catch {
    return {};
  }
}

function writeStore(data) {
  const file = storeFile();
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = file + '.tmp';
  fs.writeFileSync(tmp, JSON.stringify(data, null, 2));
  fs.renameSync(tmp, file);
}

function resolveTheme(pref) {
  if (pref === 'light' || pref === 'dark') return pref;
  return nativeTheme.shouldUseDarkColors ? 'dark' : 'light';
}

function applyTheme(pref) {
  nativeTheme.themeSource = pref === 'light' || pref === 'dark' ? pref : 'system';
  const theme = resolveTheme(pref);
  if (win && !win.isDestroyed()) {
    win.setBackgroundColor(THEMES[theme].bg);
    if (!IS_MAC && typeof win.setTitleBarOverlay === 'function') {
      win.setTitleBarOverlay({ color: THEMES[theme].color, symbolColor: THEMES[theme].symbolColor, height: TITLEBAR_HEIGHT });
    }
  }
  return theme;
}

function visibleBounds(b) {
  if (!b || typeof b.x !== 'number' || typeof b.y !== 'number') return {};
  const ok = screen.getAllDisplays().some(({ workArea: a }) =>
    b.x < a.x + a.width - 100 && b.x + b.width > a.x + 100 && b.y >= a.y - 10 && b.y < a.y + a.height - 100);
  return ok ? { x: b.x, y: b.y } : {};
}

// ---------------------------------------------------------------- window

function createWindow() {
  const store = readStore();
  const saved = store.window || {};
  const pref = (store.settings && store.settings.theme) || 'dark';
  nativeTheme.themeSource = pref === 'system' ? 'system' : pref;
  const theme = resolveTheme(pref);

  win = new BrowserWindow({
    width: saved.width || 1440,
    height: saved.height || 900,
    ...visibleBounds(saved),
    minWidth: 960,
    minHeight: 600,
    show: false,
    title: 'Destiny Stone',
    icon: path.join(__dirname, 'build', 'icon.png'),
    backgroundColor: THEMES[theme].bg,
    titleBarStyle: 'hidden',
    ...(IS_MAC
      ? { trafficLightPosition: { x: 14, y: 13 } }
      : { titleBarOverlay: { color: THEMES[theme].color, symbolColor: THEMES[theme].symbolColor, height: TITLEBAR_HEIGHT } }),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      spellcheck: false,
    },
  });

  win.once('ready-to-show', () => {
    if (saved.maximized) win.maximize();
    win.show();
  });

  win.on('close', () => {
    const data = readStore();
    data.window = { ...win.getNormalBounds(), maximized: win.isMaximized() };
    try {
      writeStore(data);
    } catch { /* not fatal */ }
  });

  win.on('closed', () => {
    win = null;
    stopWatching();
  });

  win.webContents.setWindowOpenHandler(({ url }) => {
    if (/^https?:\/\//i.test(url)) shell.openExternal(url);
    return { action: 'deny' };
  });
  win.webContents.on('will-navigate', (e) => e.preventDefault());

  win.loadFile(path.join(__dirname, 'renderer', 'index.html'));
}

function buildMenu() {
  const template = [
    ...(IS_MAC ? [{ role: 'appMenu' }] : []),
    { role: 'editMenu' },
    {
      label: 'Ver',
      submenu: [
        { role: 'reload', label: 'Recargar' },
        { role: 'toggleDevTools', label: 'Herramientas de desarrollo' },
        { type: 'separator' },
        { role: 'resetZoom', label: 'Tamaño real' },
        { role: 'zoomIn', label: 'Acercar' },
        { role: 'zoomOut', label: 'Alejar' },
        { type: 'separator' },
        { role: 'togglefullscreen', label: 'Pantalla completa' },
      ],
    },
    // No window menu: it would bind Ctrl/Cmd+W to "close window", and the app uses it to close tabs.
  ];
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

// ---------------------------------------------------------------- file watcher

let watcher = null;
let watchTimer = null;

function stopWatching() {
  clearTimeout(watchTimer);
  if (watcher) {
    try {
      watcher.close();
    } catch { /* already closed */ }
    watcher = null;
  }
}

function watchRepo(repo) {
  stopWatching();
  if (!repo || !fs.existsSync(repo)) return false;
  try {
    watcher = fs.watch(repo, { recursive: true }, (_evt, file) => {
      if (file) {
        const f = String(file).replace(/\\/g, '/');
        if (/(^|\/)\.git\/(objects|logs)(\/|$)/.test(f) || f.endsWith('.lock')) return;
      }
      clearTimeout(watchTimer);
      watchTimer = setTimeout(() => {
        if (win && !win.isDestroyed()) win.webContents.send('repo:changed', repo);
      }, 400);
    });
    watcher.on('error', stopWatching);
    return true;
  } catch {
    watcher = null;
    return false;
  }
}

// ---------------------------------------------------------------- terminal

function trySpawn(cmd, args, opts) {
  return new Promise((resolve) => {
    try {
      const child = spawn(cmd, args, { detached: true, stdio: 'ignore', ...opts });
      child.once('error', () => resolve(false));
      child.once('spawn', () => {
        child.unref();
        resolve(true);
      });
    } catch {
      resolve(false);
    }
  });
}

async function openTerminal(repo) {
  if (!repo || !fs.existsSync(repo)) throw new Error('La carpeta no existe.');
  if (process.platform === 'win32') {
    if (await trySpawn('wt.exe', ['-d', repo], { cwd: repo })) return;
    const gitBash = [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, 'Programs')]
      .filter(Boolean)
      .map((p) => path.join(p, 'Git', 'git-bash.exe'))
      .find((p) => fs.existsSync(p));
    if (gitBash && (await trySpawn(gitBash, [`--cd=${repo}`], { cwd: repo }))) return;
    if (await trySpawn('powershell.exe', ['-NoExit'], { cwd: repo })) return;
  } else if (IS_MAC) {
    if (await trySpawn('open', ['-a', 'Terminal', repo])) return;
  } else {
    for (const cmd of ['x-terminal-emulator', 'gnome-terminal', 'konsole', 'xfce4-terminal', 'xterm']) {
      if (await trySpawn(cmd, [], { cwd: repo })) return;
    }
  }
  throw new Error('No se pudo abrir un terminal.');
}

// ---------------------------------------------------------------- IPC

function handle(channel, fn) {
  ipcMain.handle(channel, async (_e, ...args) => {
    try {
      return { ok: true, data: await fn(...args) };
    } catch (err) {
      return { ok: false, error: (err && err.message) || String(err) };
    }
  });
}

function registerIpc() {
  gitService.setProgressSink((opId, line) => {
    if (win && !win.isDestroyed()) win.webContents.send('git:progress', { opId, line });
  });

  handle('git', (method, ...args) => {
    if (!Object.prototype.hasOwnProperty.call(gitService.api, method)) throw new Error(`Operación desconocida: ${method}`);
    return gitService.api[method](...args);
  });
  handle('git:cancel', (opId) => gitService.cancel(opId));

  handle('app:info', () => ({ platform: process.platform, version: app.getVersion() }));
  handle('store:read', () => {
    const { window: _w, ...rest } = readStore();
    return rest;
  });
  handle('store:write', (data) => {
    const cur = readStore();
    writeStore({ ...data, window: cur.window });
  });

  handle('dialog:folder', async (title) => {
    const res = await dialog.showOpenDialog(win, { title, properties: ['openDirectory', 'createDirectory'] });
    return res.canceled || !res.filePaths.length ? null : res.filePaths[0];
  });

  handle('shell:open', async (repo, rel) => {
    const err = await shell.openPath(gitService.safeJoin(repo, rel));
    if (err) throw new Error(err);
  });
  handle('shell:reveal', (repo, rel) => {
    if (!rel) return shell.openPath(gitService.safeJoin(repo, '.'));
    shell.showItemInFolder(gitService.safeJoin(repo, rel));
  });
  handle('shell:terminal', (repo) => openTerminal(repo));
  handle('shell:external', (url) => {
    if (!/^https?:\/\//i.test(String(url))) throw new Error('URL no válida.');
    return shell.openExternal(url);
  });
  handle('clipboard:write', (text) => clipboard.writeText(String(text)));
  handle('theme:set', (pref) => applyTheme(pref));
  handle('watch', (repo) => watchRepo(repo));

  nativeTheme.on('updated', () => {
    if (win && !win.isDestroyed()) win.webContents.send('theme:system', nativeTheme.shouldUseDarkColors ? 'dark' : 'light');
  });
}

// ---------------------------------------------------------------- lifecycle

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (win) {
      if (win.isMinimized()) win.restore();
      win.focus();
    }
  });

  app.whenReady().then(() => {
    buildMenu();
    registerIpc();
    createWindow();
    app.on('activate', () => {
      if (BrowserWindow.getAllWindows().length === 0) createWindow();
    });
  });

  app.on('window-all-closed', () => {
    if (!IS_MAC) app.quit();
  });
}
