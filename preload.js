'use strict';
const { contextBridge, ipcRenderer, webUtils } = require('electron');

const invoke = (channel, ...args) => ipcRenderer.invoke(channel, ...args);

contextBridge.exposeInMainWorld('api', {
  git: (method, ...args) => invoke('git', method, ...args),
  cancel: (opId) => invoke('git:cancel', opId),
  info: () => invoke('app:info'),
  readStore: () => invoke('store:read'),
  writeStore: (data) => invoke('store:write', data),
  pickFolder: (title) => invoke('dialog:folder', title),
  openPath: (repo, rel) => invoke('shell:open', repo, rel),
  reveal: (repo, rel) => invoke('shell:reveal', repo, rel),
  openTerminal: (repo) => invoke('shell:terminal', repo),
  openExternal: (url) => invoke('shell:external', url),
  copy: (text) => invoke('clipboard:write', text),
  setTheme: (pref) => invoke('theme:set', pref),
  watch: (repo) => invoke('watch', repo),
  pathForFile: (file) => {
    try {
      return webUtils.getPathForFile(file);
    } catch {
      return null;
    }
  },
  onProgress: (cb) => ipcRenderer.on('git:progress', (_e, data) => cb(data)),
  onRepoChanged: (cb) => ipcRenderer.on('repo:changed', (_e, repo) => cb(repo)),
  onSystemTheme: (cb) => ipcRenderer.on('theme:system', (_e, theme) => cb(theme)),
});
