'use strict';
// Electron smoke test: launches the real app against a generated repository, drives the UI,
// fails on any renderer error and saves screenshots. Usage: npm run smoke [-- <outDir>]
const { app, BrowserWindow } = require('electron');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');

const outDir = path.resolve(process.argv.find((a, i) => i > 1 && !a.startsWith('-') && !a.endsWith('smoke.js')) || path.join(os.tmpdir(), 'ds-smoke-shots'));
fs.mkdirSync(outDir, { recursive: true });
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ds-smoke-'));
app.setPath('userData', path.join(root, 'userdata'));

const sh = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8' });
const write = (repo, rel, text) => {
  fs.mkdirSync(path.dirname(path.join(repo, rel)), { recursive: true });
  fs.writeFileSync(path.join(repo, rel), text);
};

function buildRepo() {
  const repo = path.join(root, 'demo-project');
  const bare = path.join(root, 'origin.git');
  sh(root, 'init', '--bare', bare);
  fs.mkdirSync(repo);
  sh(repo, 'init', '-b', 'main');
  for (const [k, v] of [['user.name', 'Ada Lovelace'], ['user.email', 'ada@example.com'], ['core.autocrlf', 'false'], ['commit.gpgsign', 'false']]) sh(repo, 'config', k, v);
  const commit = (msg) => {
    sh(repo, 'add', '-A');
    sh(repo, 'commit', '-q', '-m', msg);
  };
  write(repo, 'README.md', '# Demo\n\nProyecto de ejemplo.\n');
  write(repo, 'src/app.js', Array.from({ length: 40 }, (_, i) => `const v${i} = ${i};`).join('\n') + '\n');
  commit('Commit inicial');
  sh(repo, 'remote', 'add', 'origin', bare);
  sh(repo, 'push', '-q', '-u', 'origin', 'main');
  sh(repo, 'checkout', '-q', '-b', 'feature/login');
  write(repo, 'src/login.js', 'export function login() {\n  return true;\n}\n');
  commit('Añadir pantalla de login');
  write(repo, 'src/login.js', 'export function login(user) {\n  return !!user;\n}\n');
  commit('Validar usuario en login');
  sh(repo, 'checkout', '-q', 'main');
  write(repo, 'README.md', '# Demo\n\nProyecto de ejemplo con documentación.\n');
  commit('Mejorar README');
  sh(repo, 'merge', '-q', '--no-ff', '--no-edit', 'feature/login');
  sh(repo, 'tag', '-a', 'v1.0.0', '-m', 'Primera versión');
  sh(repo, 'checkout', '-q', '-b', 'experimento');
  write(repo, 'notes.txt', 'idea\n');
  commit('Experimento sin fusionar');
  sh(repo, 'checkout', '-q', 'main');
  write(repo, 'stash.txt', 'stashed\n');
  sh(repo, 'stash', 'push', '-u', '-q', '-m', 'trabajo a medias');
  // working tree changes
  const lines = fs.readFileSync(path.join(repo, 'src/app.js'), 'utf8').split('\n');
  lines[2] = 'const v2 = 200; // cambiado';
  lines.splice(30, 0, 'const nuevo = true;');
  write(repo, 'src/app.js', lines.join('\n'));
  write(repo, 'docs/guia.md', '# Guía\n\nTexto nuevo.\n');
  write(repo, 'README.md', '# Demo\n\nProyecto de ejemplo con documentación.\n\nLínea preparada.\n');
  sh(repo, 'add', 'README.md');
  return repo;
}

const repo = buildRepo();
fs.mkdirSync(app.getPath('userData'), { recursive: true });
fs.writeFileSync(path.join(app.getPath('userData'), 'state.json'), JSON.stringify({
  settings: { theme: process.env.DS_THEME || 'dark' },
  repos: [{ path: repo, name: 'demo-project' }],
  tabs: [repo],
  active: repo,
  window: { width: 1440, height: 900 },
}));

const errors = [];
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
process.on('uncaughtException', (e) => {
  console.error('MAIN PROCESS ERROR', e);
  app.exit(3);
});

app.on('browser-window-created', (_e, win) => {
  win.webContents.on('console-message', (event, legacyLevel, legacyMessage) => {
    const level = event.level !== undefined ? event.level : legacyLevel;
    const message = event.message !== undefined ? event.message : legacyMessage;
    if (level === 'error' || level === 3) errors.push(`console: ${message}`);
  });
  win.webContents.on('render-process-gone', (_ev, d) => errors.push(`renderer gone: ${d.reason}`));
  win.webContents.on('preload-error', (_ev, p, err) => errors.push(`preload: ${err.message}`));
  win.webContents.once('did-finish-load', () => drive(win).catch((e) => {
    errors.push(`driver: ${e.stack || e}`);
    finish(win);
  }));
});

async function js(win, code) {
  return win.webContents.executeJavaScript(`(async () => { ${code} })()`, true);
}

async function shot(win, name) {
  await wait(350);
  const stray = await win.webContents.executeJavaScript(`[...document.querySelectorAll('body *')].filter(e => [...e.childNodes].some(n => n.nodeType === 3 && /^(null|undefined|NaN)$/.test(n.textContent.trim()))).map(e => e.className).join(', ')`);
  if (stray) errors.push(`stray text node in ${name}: ${stray}`);
  const img = await win.webContents.capturePage();
  const file = path.join(outDir, `${name}.png`);
  fs.writeFileSync(file, img.toPNG());
  console.log(`screenshot ${file}`);
}

async function until(win, cond, label, timeout = 8000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await js(win, `return !!(${cond});`)) return;
    await wait(100);
  }
  throw new Error(`timeout waiting for ${label}`);
}

async function drive(win) {
  await js(win, `window.addEventListener('error', (e) => console.error('uncaught', e.message, e.filename, e.lineno)); window.addEventListener('unhandledrejection', (e) => console.error('unhandled', e.reason && (e.reason.stack || e.reason)));`);
  await until(win, `document.querySelectorAll('.hrow').length > 5`, 'history rows');
  await until(win, `document.querySelector('#detail-left .meta')`, 'commit detail');
  await until(win, `document.querySelector('#detail-diff .dl')`, 'commit diff');
  const info = await js(win, `return {
    rows: document.querySelectorAll('.hrow').length,
    pseudo: !!document.querySelector('.hrow.pseudo'),
    refs: [...document.querySelectorAll('.hrow .ref')].map(r => r.textContent),
    sidebar: document.querySelector('#sidebar').innerText,
  };`);
  console.log('history:', JSON.stringify(info.refs), 'pseudo row:', info.pseudo);
  if (!info.pseudo) throw new Error('missing uncommitted-changes row');
  for (const want of ['main', 'feature/login', 'experimento', 'v1.0.0', 'origin/main']) {
    if (!info.refs.includes(want)) throw new Error(`missing ref badge ${want}`);
  }
  if (!/trabajo a medias/.test(info.sidebar)) throw new Error('stash missing in sidebar');
  await shot(win, '01-history');

  // select the merge commit
  await js(win, `const rows=[...document.querySelectorAll('.hrow')]; rows.find(r=>r.textContent.includes("Merge branch")).click();`);
  await until(win, `document.querySelector('#detail-left .subject') && document.querySelector('#detail-left .subject').textContent.startsWith('Merge')`, 'merge detail');
  await until(win, `document.querySelector('#detail-diff .dl')`, 'merge diff');
  await shot(win, '02-merge-commit');

  // changes view
  await js(win, `setView('changes');`);
  await until(win, `document.querySelectorAll('#unstaged-panel .frow').length === 2 && document.querySelectorAll('#staged-panel .frow').length === 1`, 'changes lists');
  await js(win, `[...document.querySelectorAll('#unstaged-panel .frow')].find(r => r.textContent.includes('app.js')).click();`);
  await until(win, `document.querySelectorAll('#changes-diff .hunk').length === 2`, 'two hunks for app.js');
  // pick one added line and stage just that line
  await js(win, `const add=[...document.querySelectorAll('#changes-diff .dl.add')].find(r=>r.textContent.includes('nuevo')); add.click();`);
  await until(win, `[...document.querySelectorAll('#changes-diff .h-actions button')].some(b => b.textContent.includes('Preparar líneas (1)'))`, 'line selection');
  await shot(win, '03-line-selection');
  await js(win, `[...document.querySelectorAll('#changes-diff .h-actions button')].find(b => b.textContent.includes('Preparar líneas')).click();`);
  await until(win, `[...document.querySelectorAll('#staged-panel .frow')].some(r => r.textContent.includes('app.js'))`, 'app.js partially staged');
  const cached = sh(repo, 'diff', '--cached', '--', 'src/app.js');
  if (!cached.includes('+const nuevo = true;') || cached.includes('v2 = 200')) throw new Error(`unexpected staged diff:\n${cached}`);
  console.log('partial line staging verified against git');

  // stage untracked via checkbox, write message, commit
  await js(win, `[...document.querySelectorAll('#unstaged-panel .frow')].find(r => r.textContent.includes('guia.md')).querySelector('input').click();`);
  await until(win, `[...document.querySelectorAll('#staged-panel .frow')].some(r => r.textContent.includes('guia.md'))`, 'guia staged');
  await js(win, `const ta=document.querySelector('#commit-message'); ta.value='Añadir guía y ajustes'; ta.dispatchEvent(new Event('input'));`);
  await shot(win, '04-changes-ready');
  await js(win, `document.querySelector('#commit-btn').click();`);
  await until(win, `document.querySelectorAll('#staged-panel .frow').length === 0`, 'commit done');
  const last = sh(repo, 'log', '-1', '--format=%s');
  if (last.trim() !== 'Añadir guía y ajustes') throw new Error(`commit not created: ${last}`);
  console.log('commit verified:', last.trim());

  // context menu on a commit + a dialog
  await js(win, `setView('history');`);
  await until(win, `document.querySelector('.hrow')`, 'history again');
  await js(win, `const r=[...document.querySelectorAll('.hrow')].find(r=>r.textContent.includes('Mejorar README')); const b=r.getBoundingClientRect(); r.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:b.left+300,clientY:b.top+10}));`);
  await until(win, `document.querySelector('.ctx')`, 'context menu');
  await shot(win, '05-context-menu');
  await js(win, `closeMenu();`);
  if (await js(win, `return document.querySelectorAll('.ctx').length;`)) throw new Error('context menu did not close');
  await js(win, `openPushDialog();`);
  await until(win, `document.querySelector('.modal')`, 'push dialog');
  await shot(win, '06-push-dialog');
  await js(win, `[...document.querySelectorAll('.modal .btn')].find(b=>b.textContent==='Push').click();`);
  await until(win, `!document.querySelector('.modal') && !S.busy`, 'push finished', 15000);
  const remoteHead = sh(repo, 'ls-remote', 'origin', 'refs/heads/main').split('\t')[0];
  const localHead = sh(repo, 'rev-parse', 'main').trim();
  if (remoteHead !== localHead) throw new Error('push did not update origin/main');
  console.log('push verified');

  // merge conflict: banner + conflicted panel
  sh(repo, 'checkout', '-q', '-b', 'conflicto');
  write(repo, 'README.md', '# Demo\n\nVersión de la rama conflicto.\n');
  sh(repo, 'commit', '-q', '-m', 'Cambio en conflicto', '--', 'README.md');
  sh(repo, 'checkout', '-q', 'main');
  write(repo, 'README.md', '# Demo\n\nVersión de main.\n');
  sh(repo, 'commit', '-q', '-m', 'Cambio en main', '--', 'README.md');
  await js(win, `await op('Merge', () => g('merge', S.active, 'conflicto'));`);
  await until(win, `!document.querySelector('#banner').classList.contains('hidden')`, 'conflict banner');
  await js(win, `setView('changes');`);
  await until(win, `document.querySelectorAll('#conflicted-panel .frow').length === 1`, 'conflicted file');
  await until(win, `document.querySelector('#changes-diff .dl.conflict-marker')`, 'conflict markers');
  await shot(win, '07-conflict');
  await js(win, `await op('Abortar', () => g('abortOperation', S.active, 'merge'));`);
  await until(win, `document.querySelector('#banner').classList.contains('hidden')`, 'merge aborted');
  await until(win, `document.querySelector('#commit-message').value === ''`, 'merge message cleared after abort');

  // home screen and light theme
  await js(win, `activate(null);`);
  await until(win, `document.querySelector('.repo-row')`, 'home');
  await shot(win, '08-home');
  await js(win, `applyTheme('light'); activate(S.repos[0].path);`);
  await until(win, `document.documentElement.dataset.theme === 'light' && document.querySelectorAll('.hrow').length > 3`, 'light theme');
  await until(win, `document.querySelector('#detail-diff .dl')`, 'light diff');
  await shot(win, '09-light');
  finish(win);
}

function finish(win) {
  setTimeout(() => {
    if (errors.length) {
      console.error(`\nSMOKE FAILED\n${errors.join('\n')}`);
      process.exitCode = 1;
    } else {
      console.log('\nSMOKE OK');
    }
    try {
      win.destroy();
    } catch { /* already gone */ }
    app.exit(process.exitCode || 0);
  }, 300);
}

setTimeout(() => {
  console.error('SMOKE TIMEOUT', errors.join('\n'));
  app.exit(2);
}, 90000);

require('../main.js');
