'use strict';
// Renders the app logo to build/icon.png (512x512, transparent). Usage: npx electron build/make-icon.js
const { app, BrowserWindow } = require('electron');
const fs = require('fs');
const path = require('path');

const SIZE = 512;
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${SIZE}" height="${SIZE}" viewBox="0 0 24 24" fill="none">
  <defs><linearGradient id="g" x1="4" y1="3" x2="20" y2="21" gradientUnits="userSpaceOnUse"><stop stop-color="#6aa1ff"/><stop offset="1" stop-color="#8b5cf6"/></linearGradient></defs>
  <path d="M12 1.6 21.2 6.5v11L12 22.4 2.8 17.5v-11z" fill="url(#g)" stroke="#ffffff" stroke-opacity=".18" stroke-width=".25" stroke-linejoin="round"/>
  <circle cx="9" cy="8.6" r="1.9" fill="#fff"/><circle cx="9" cy="15.4" r="1.9" fill="#fff"/><circle cx="15.2" cy="12" r="1.9" fill="#fff"/>
  <path d="M9 10.5v3M10.7 9.5l2.8 1.5" stroke="#fff" stroke-width="1.5" stroke-linecap="round"/>
</svg>`;

app.disableHardwareAcceleration();
app.whenReady().then(async () => {
  const win = new BrowserWindow({ width: SIZE, height: SIZE, show: false, transparent: true, frame: false, webPreferences: { offscreen: true } });
  await win.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(`<html><body style="margin:0;background:transparent">${svg}</body></html>`)}`);
  await new Promise((r) => setTimeout(r, 400));
  const img = await win.webContents.capturePage({ x: 0, y: 0, width: SIZE, height: SIZE });
  const out = path.join(__dirname, 'icon.png');
  fs.writeFileSync(out, img.resize({ width: SIZE, height: SIZE }).toPNG());
  console.log(`icon written: ${out}`);
  app.exit(0);
});
