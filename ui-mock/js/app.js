// App shell: state, tab bar, toolbar, banner, sidebar, status bar, toasts, menus and the
// simulated git actions. Mirrors crates/codesprout-app/src/app.rs, actions.rs and views/sidebar.rs.
'use strict';

const $ = (s, r = document) => r.querySelector(s);
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

// widgets.rs › rel_time / full_time / plural / hash_color / initials / split_path
function relTime(ts) {
  const d = Math.max(0, Date.now() / 1000 - ts);
  const units = [[60, null], [3600, 60, 'minuto', 'minutos'], [86400, 3600, 'hora', 'horas'], [2592000, 86400, 'día', 'días'], [31536000, 2592000, 'mes', 'meses'], [Infinity, 31536000, 'año', 'años']];
  if (d < 60) return 'ahora mismo';
  for (const [lim, div, one, many] of units.slice(1)) {
    if (d < lim) {
      const n = Math.floor(d / div);
      return `hace ${n} ${n === 1 ? one : many}`;
    }
  }
}
const pad = (n) => String(n).padStart(2, '0');
function fullTime(ts) {
  const t = new Date(ts * 1000);
  return `${pad(t.getDate())}/${pad(t.getMonth() + 1)}/${t.getFullYear()} ${pad(t.getHours())}:${pad(t.getMinutes())}`;
}
const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;
function hashColor(s) {
  let h = 0;
  for (const ch of s) h = (Math.imul(h, 31) + ch.charCodeAt(0)) >>> 0;
  return `var(--lane-${h % 10})`;
}
const initials = (n) => (n.split(/[\s\-_.]+/).filter(Boolean).slice(0, 2).map((p) => p[0]).join('') || '?').toUpperCase();
const splitPath = (p) => {
  const i = p.lastIndexOf('/');
  return i < 0 ? ['', p] : [p.slice(0, i), p.slice(i + 1)];
};
const STATUS = { M: ['var(--yellow)', 'M', 'Modificado'], A: ['var(--green)', 'A', 'Añadido'], '?': ['var(--green)', '+', 'Sin seguimiento'], D: ['var(--red)', 'D', 'Eliminado'], R: ['var(--purple)', 'R', 'Renombrado'], C: ['var(--purple)', 'C', 'Copiado'], U: ['var(--red)', '!', 'Conflicto'] };
function statusBadge(st) {
  const [color, letter, name] = STATUS[st.length > 1 ? 'U' : st] || ['var(--muted)', 'T', 'Cambio de tipo'];
  return `<span class="st" title="${name}" style="color:${color};background:color-mix(in srgb, ${color} 16%, transparent)">${letter}</span>`;
}
const pill = (text, color) => `<span class="pill" style="color:${color};background:color-mix(in srgb, ${color} 14%, transparent)">${esc(text)}</span>`;

const App = (() => {
  const clone = (x) => JSON.parse(JSON.stringify(x));
  const S = {
    settings: { theme: 'dark', zoom: 1, vertical: false, chronological: false, allBranches: true, historyMode: 'explorer', ignoreWhitespace: false, wordDiff: true, context: 3, sidebarW: 250, detailH: 330, changesW: 380, collapsed: {}, autoFetch: 0, maxCommits: 5000 },
    screen: 'repo',
    view: 'history',
    selected: null,
    compareWith: null,
    detailFile: 0,
    filter: '',
    search: { query: '', mode: 'Mensaje', results: null },
    sel: { list: null, paths: new Set(), focus: null, anchor: null },
    message: '', amend: false, pushAfter: false, sign: false,
    busy: null,
    accounts: [],
    update: null,
    diffSel: new Set(),
  };

  function loadData() {
    S.repo = clone(MOCK_REPO);
    S.commits = S.repo.commits;
    S.refs = S.repo.refs;
    S.files = S.repo.files;
    S.status = clone(MOCK_STATUS);
    S.diffs = clone(MOCK_DIFFS);
    S.untracked = clone(MOCK_UNTRACKED);
    S.accounts = clone(MOCK_ACCOUNTS);
    S.update = null;
    S.selected = S.refs.head;
    recompute();
  }

  // app.rs › apply_refresh: ref map, layout, selection
  function recompute() {
    S.index = new Map(S.commits.map((c, i) => [c.hash, i]));
    S.changeCount = S.status.staged.length + S.status.unstaged.length + S.status.conflicted.length;
    const chrono = S.settings.chronological || S.settings.vertical;
    S.layout = ExplorerLayout.layout(S.commits, S.refs, S.changeCount > 0, chrono);
    const m = new Map();
    const add = (h, r) => (m.has(h) ? m.get(h) : m.set(h, []).get(h)).push(r);
    if (!S.refs.currentBranch && S.refs.head) add(S.refs.head, { kind: 'head', name: 'HEAD' });
    for (const b of S.refs.local) add(b.hash, { kind: 'branch', name: b.name, current: b.current });
    for (const b of S.refs.remote) add(b.hash, { kind: 'remote', name: b.name });
    for (const t of S.refs.tags) add(t.hash, { kind: 'tag', name: t.name });
    const order = (r) => (r.kind === 'branch' && r.current ? 0 : r.kind === 'head' ? 1 : r.kind === 'branch' ? 2 : r.kind === 'remote' ? 3 : 4);
    for (const l of m.values()) l.sort((a, b) => order(a) - order(b));
    S.refMap = m;
    pruneSelection();
  }

  function pruneSelection() {
    const s = S.status;
    const lists = { conflicted: s.conflicted, staged: s.staged, unstaged: s.unstaged };
    if (S.sel.list) {
      const ok = new Set(lists[S.sel.list].map((f) => f.path));
      S.sel.paths = new Set([...S.sel.paths].filter((p) => ok.has(p)));
      if (!ok.has(S.sel.focus)) S.sel.focus = [...S.sel.paths][0] || null;
    }
    if (!S.sel.focus) {
      const k = ['conflicted', 'unstaged', 'staged'].find((k) => lists[k].length);
      S.sel = k ? { list: k, paths: new Set([lists[k][0].path]), focus: lists[k][0].path, anchor: lists[k][0].path } : { list: null, paths: new Set(), focus: null, anchor: null };
    }
  }

  const branchLabel = () => S.refs.currentBranch || (S.refs.head ? `HEAD separado (${S.refs.head.slice(0, 7)})` : 'sin commits');
  const currentLocal = () => S.refs.local.find((b) => b.current) || null;

  // ---------------------------------------------------------------- notifications
  function toast(text, kind = 'success') {
    const color = { info: 'var(--accent)', success: 'var(--green)', warn: 'var(--yellow)', error: 'var(--red)' }[kind];
    const ic = kind === 'success' ? 'check' : kind === 'info' ? 'eye' : 'alert';
    const el = document.createElement('div');
    el.className = 'toast';
    el.innerHTML = `<span style="color:${color}">${icon(ic, 15)}</span><span>${esc(text)}</span>`;
    el.onclick = () => el.remove();
    $('#toasts').prepend(el);
    setTimeout(() => el.remove(), kind === 'error' ? 8000 : kind === 'warn' ? 6000 : 3000);
  }

  // ---------------------------------------------------------------- context menus
  function menu(x, y, items, header) {
    closeMenu();
    const el = document.createElement('div');
    el.className = 'menu';
    el.innerHTML = (header ? `<div class="mh">${esc(header)}</div>` : '') + items.map((it, i) => (it === '-' ? '<div class="ms"></div>' : `<div class="mi${it.danger ? ' danger' : ''}${it.disabled ? ' disabled' : ''}" data-i="${i}">${esc(it.label)}</div>`)).join('');
    document.body.appendChild(el);
    const r = el.getBoundingClientRect();
    el.style.left = Math.min(x, innerWidth - r.width - 8) + 'px';
    el.style.top = Math.min(y, innerHeight - r.height - 8) + 'px';
    el.onclick = (e) => {
      const mi = e.target.closest('.mi');
      if (!mi) return;
      closeMenu();
      items[+mi.dataset.i].action();
    };
    setTimeout(() => document.addEventListener('pointerdown', outside), 0);
  }
  function outside(e) {
    if (!e.target.closest('.menu')) closeMenu();
  }
  function closeMenu() {
    document.querySelectorAll('.menu').forEach((m) => m.remove());
    document.removeEventListener('pointerdown', outside);
  }

  // ---------------------------------------------------------------- simulated actions (actions.rs)
  function op(label, fn, success) {
    if (S.busy) return toast(`Espera a que termine la operación en curso (${S.busy}).`, 'warn');
    S.busy = label;
    renderStatus();
    setTimeout(() => {
      S.busy = null;
      try {
        fn();
        if (success) toast(success);
      } catch (e) {
        Dialogs.error(label, e.message);
      }
      recompute();
      render();
    }, 250);
  }

  // app.rs › progress_ui: network operations with git's output
  function remoteOp(title, lines, fn, success) {
    Dialogs.progress(title, lines, () => {
      fn && fn();
      recompute();
      render();
      if (success) toast(success);
    });
  }

  function moveFiles(files, from, to) {
    const paths = new Set(files.map((f) => f.path));
    const moving = S.status[from].filter((f) => paths.has(f.path));
    S.status[from] = S.status[from].filter((f) => !paths.has(f.path));
    for (const f of moving) {
      if (!S.status[to].some((x) => x.path === f.path)) S.status[to].push({ ...f, status: to === 'staged' && f.status === '?' ? 'A' : to === 'unstaged' && f.status === 'A' ? '?' : f.status });
    }
    S.status[to].sort((a, b) => a.path.localeCompare(b.path));
  }

  const actions = {
    stage: (files) => op('Preparar', () => moveFiles(files, 'unstaged', 'staged')),
    unstage: (files) => op('Quitar del área de preparación', () => moveFiles(files, 'staged', 'unstaged')),
    stageAll: () => op('Preparar todo', () => moveFiles([...S.status.unstaged], 'unstaged', 'staged')),
    unstageAll: () => op('Quitar todo', () => moveFiles([...S.status.staged], 'staged', 'unstaged')),
    discard: (files) => {
      const untracked = files.filter((f) => f.status === '?').length;
      const tracked = files.length - untracked;
      const parts = [];
      if (tracked) parts.push(`se descartarán los cambios de ${plural(tracked, 'archivo', 'archivos')}`);
      if (untracked) parts.push(plural(untracked, 'archivo sin seguimiento irá a la papelera', 'archivos sin seguimiento irán a la papelera'));
      const msg = parts.join(' y ') + '.';
      Dialogs.confirm(files.length === 1 ? `Descartar «${splitPath(files[0].path)[1]}»` : 'Descartar cambios', msg[0].toUpperCase() + msg.slice(1), tracked ? 'Los cambios descartados de archivos con seguimiento no se pueden recuperar.' : null, 'Descartar', true, () => op('Descartar', () => {
        const p = new Set(files.map((f) => f.path));
        S.status.unstaged = S.status.unstaged.filter((f) => !p.has(f.path));
      }));
    },
    commit: () => {
      const [ok] = canCommit();
      if (!ok) return;
      const msg = S.message;
      op(S.amend ? 'Modificar commit' : 'Commit', () => {
        const b = currentLocal();
        const now = Math.floor(Date.now() / 1000);
        const hash = [...Array(40)].map(() => '0123456789abcdef'[Math.floor(Math.random() * 16)]).join('');
        const parents = S.amend ? S.commits[S.index.get(S.refs.head)].parents : [S.refs.head];
        const c = { hash, parents, author: 'Ana', email: 'ana@x.com', time: now, subject: msg.split('\n')[0] };
        if (S.amend) S.commits.splice(S.index.get(S.refs.head), 1);
        S.commits.unshift(c);
        S.files[hash] = S.status.staged.map((f) => ({ ...f, diff: S.diffs[f.path] || '' }));
        S.status.staged = [];
        S.refs.head = hash;
        if (b) b.hash = hash;
        S.selected = hash;
        S.message = '';
        S.amend = false;
      }, S.amend ? 'Commit modificado' : 'Commit creado');
    },
    checkout: (name) => {
      if (S.refs.currentBranch === name) return;
      op('Cambiar de rama', () => {
        for (const b of S.refs.local) b.current = b.name === name;
        const b = S.refs.local.find((x) => x.name === name);
        S.refs.currentBranch = name;
        S.refs.head = b.hash;
      }, `Ahora estás en ${name}`);
    },
    createBranch: (name, start, checkout) => op('Crear rama', () => {
      const hash = start || S.refs.head;
      if (checkout) for (const b of S.refs.local) b.current = false;
      S.refs.local.push({ name, hash, upstream: null, current: !!checkout, ahead: 0, behind: 0 });
      S.refs.local.sort((a, b) => a.name.localeCompare(b.name));
      if (checkout) {
        S.refs.currentBranch = name;
        S.refs.head = hash;
      }
    }, `Rama ${name} creada`),
    fetch: () => remoteOp('Fetch', ['remote: Enumerating objects: 5, done.', 'remote: Counting objects: 100% (5/5), done.', 'Unpacking objects: 100% (3/3), 284 bytes | 9.00 KiB/s, done.', 'From ../demo-origin', '   b37272c..b37272c  develop    -> origin/develop'], null, 'Fetch completado'),
    push: (branch) => remoteOp('Push', ['Enumerating objects: 4, done.', 'Counting objects: 100% (4/4), done.', 'Writing objects: 100% (3/3), 301 bytes | 301.00 KiB/s, done.', 'To ../demo-origin', `   91fa5f5..b9a3424  ${branch} -> ${branch}`], () => {
      const b = S.refs.local.find((x) => x.name === branch);
      const r = S.refs.remote.find((x) => x.branch === branch);
      if (b && r) r.hash = b.hash;
      else if (b) S.refs.remote.push({ remote: 'origin', branch, name: `origin/${branch}`, hash: b.hash });
      if (b) b.upstream = `origin/${branch}`;
    }, `Push de ${branch} completado`),
    pull: () => remoteOp('Pull', ['From ../demo-origin', ' * branch            develop    -> FETCH_HEAD', 'Already up to date.'], null, 'Pull completado'),
  };

  function canCommit() {
    const s = S.status;
    const merging = ['merge', 'cherry-pick', 'revert'].includes(s.state);
    if (S.busy) return [false, 'Hay una operación en curso'];
    if (s.state === 'rebase') return [false, 'Durante un rebase usa «Continuar» en la barra superior'];
    if (s.conflicted.length) return [false, 'Resuelve los conflictos primero'];
    if (!S.message.trim()) return [false, 'Escribe un mensaje'];
    if (!S.amend && !s.staged.length && !merging) return [false, 'Prepara al menos un archivo'];
    return [true, ''];
  }

  function select(hash, center) {
    S.selected = hash;
    S.detailFile = 0;
    S.diffSel = new Set();
    if (center && Views.explorer) Views.explorer.centerOn = hash;
    renderView();
    renderSidebar();
  }

  function setView(v) {
    S.view = v;
    render();
  }

  // ---------------------------------------------------------------- render: tab bar (app.rs › tab_bar)
  function renderTabs() {
    const active = S.screen === 'repo';
    $('#tabbar').innerHTML = `
      <div class="tab${active ? ' active' : ''}" data-act="tab" title="${esc(S.repo.path)}">
        <span class="tab-icon">${icon('box', 13)}</span><span class="ellipsis">${esc(S.repo.name)}</span>
        ${S.changeCount ? '<span class="dot"></span>' : ''}
        <span class="x" data-act="close-tab">${icon('close', 8)}</span>
      </div>
      <div class="tab-home${active ? '' : ' active'}" data-act="home" title="Repositorios (Ctrl+T)">${icon('plus', 14)}</div>
      <div class="right">${S.update ? `<button class="update-btn" data-act="update" title="Tienes la ${VERSION}">${icon('pull', 14)}Actualizar a ${S.update.version}</button>` : `<button class="version-btn" data-act="check-update" title="Buscar actualizaciones">v${VERSION}</button>`}<button class="icon-text-btn" data-act="accounts">${icon('user', 14)}${S.accounts.length ? plural(S.accounts.length, 'cuenta', 'cuentas') : 'Iniciar sesión'}</button></div>`;
  }

  // ---------------------------------------------------------------- render: toolbar (app.rs › toolbar)
  function renderToolbar() {
    const b = currentLocal();
    const busy = !!S.busy, n = S.changeCount, remotes = S.refs.remotes.length, hasHead = !!S.refs.head;
    const tb = (ic, label, act, { badge = null, enabled = true, tip = label } = {}) =>
      `<button class="tool" data-act="${act}" title="${esc(tip)}" ${enabled ? '' : 'disabled'}>${icon(ic, 20)}<span>${label}</span>${badge ? `<b class="badge">${badge}</b>` : ''}</button>`;
    const noRemote = 'No hay remotos configurados';
    $('#toolbar').innerHTML = [
      tb('commit', 'Commit', 'view-changes', { badge: n || null, tip: 'Ir a los cambios (Ctrl+1)' }),
      tb('pull', 'Pull', 'pull', { badge: b && b.behind ? b.behind : null, enabled: !busy && remotes, tip: remotes ? 'Traer y fusionar cambios del remoto' : noRemote }),
      tb('push', 'Push', 'push', { badge: b && b.ahead ? b.ahead : null, enabled: !busy && remotes, tip: remotes ? 'Enviar commits al remoto' : noRemote }),
      tb('fetch', 'Fetch', 'fetch', { enabled: !busy && remotes, tip: remotes ? 'Descargar novedades de todos los remotos' : noRemote }),
      '<div class="tool-sep"></div>',
      tb('branch', 'Rama', 'new-branch', { enabled: !busy && hasHead, tip: 'Nueva rama' }),
      tb('merge', 'Merge', 'merge', { enabled: !busy && hasHead, tip: 'Fusionar otra rama en la actual' }),
      tb('stash', 'Stash', 'stash', { enabled: !busy && n > 0 && hasHead, tip: 'Guardar los cambios temporalmente' }),
      tb('discard', 'Descartar', 'discard-all', { enabled: !busy && n > 0, tip: 'Descartar todos los cambios' }),
      '<div class="tool-sep"></div>',
      tb('flow', 'Flow', 'flow', { enabled: !busy && hasHead, tip: 'Inicializar git-flow' }),
      tb('pullRequest', 'Pull request', 'create-pr', { enabled: !busy && hasHead, tip: 'Crear una pull request (GitHub, GitLab, Bitbucket, Azure)' }),
      tb('terminal', 'Terminal', 'mock-only', { tip: 'Abrir un terminal en el repositorio' }),
      tb('folder', 'Explorador', 'mock-only', { tip: 'Abrir la carpeta' }),
      `<div class="tb-repo"><div class="name">${esc(S.repo.name)}</div><div class="branch">${icon('branch', 12)}${esc(branchLabel())}</div></div>`,
      tb('refresh', 'Actualizar', 'refresh', { tip: 'Actualizar (F5)' }),
      tb('settings', 'Ajustes', 'settings'),
    ].join('');
  }

  // ---------------------------------------------------------------- render: banner (app.rs › banner)
  function renderBanner() {
    const s = S.status, el = $('#banner');
    if (!s.state || S.screen !== 'repo') return el.classList.add('hidden');
    const names = { merge: 'Fusión en curso', rebase: 'Rebase en curso', 'cherry-pick': 'Cherry-pick en curso', revert: 'Revert en curso' };
    const n = s.conflicted.length;
    const text = n ? `${names[s.state]} — ${plural(n, 'archivo tiene', 'archivos tienen')} conflictos. Resuélvelos y márcalos como resueltos.` : `${names[s.state]} — no quedan conflictos, ya puedes continuar.`;
    el.innerHTML = `${icon('alert', 18)}<div class="grow">${text}</div>
      ${n ? '<button class="btn" data-act="view-changes">Ver conflictos</button>' : ''}
      <button class="btn" data-act="abort">Abortar</button>
      ${s.state === 'rebase' ? '<button class="btn">Saltar commit</button>' : ''}
      <button class="btn primary" data-act="continue" ${n ? 'disabled' : ''}>${s.state === 'merge' ? 'Confirmar fusión' : 'Continuar'}</button>`;
    el.classList.remove('hidden');
  }

  // ---------------------------------------------------------------- render: sidebar (views/sidebar.rs)
  function sbItem({ ic, label, act, arg = '', selected = false, current = false, right = '', nested = false, title = '' }) {
    const [dir, base] = splitPath(label);
    return `<div class="row${selected ? ' selected' : ''}${current ? ' current' : ''}${nested ? ' nested' : ''}" data-act="${act}" data-arg="${esc(arg)}" title="${esc(title || label)}">
      ${icon(ic, 14)}<span class="label">${dir ? `<span class="dir">${esc(dir)}/</span>` : ''}${esc(base)}</span>${right}</div>`;
  }
  function section(key, title, count, body, addAct) {
    const collapsed = !!S.settings.collapsed[key];
    return `<div class="sb-title" data-act="toggle-section" data-arg="${key}">${icon(collapsed ? 'chevronRight' : 'chevronDown', 11)}<span>${title.toUpperCase()}${count !== null ? `&nbsp;&nbsp;${count}` : ''}</span>
      ${addAct ? `<span class="add" data-act="${addAct}" title="Añadir">${icon('plus', 10)}</span>` : ''}</div>${collapsed ? '' : body}`;
  }
  function renderSidebar() {
    const r = S.refs, v = S.view, n = S.changeCount;
    let h = '';
    h += section('workspace', 'Área de trabajo', null,
      sbItem({ ic: 'changes', label: 'Cambios', act: 'view-changes', selected: v === 'changes', right: n ? pill(n, 'var(--accent)') : '', title: 'Cambios en el directorio de trabajo (Ctrl+1)' }) +
      sbItem({ ic: 'graph', label: 'Historial', act: 'view-history', selected: v === 'history', title: 'Explorador de ramas (Ctrl+2)' }) +
      sbItem({ ic: 'history', label: 'Reflog', act: 'view', arg: 'reflog', selected: v === 'reflog' }) +
      sbItem({ ic: 'pullRequest', label: 'Pull requests', act: 'view', arg: 'prs', selected: v === 'prs' }));
    h += section('branches', 'Ramas', r.local.length, r.local.map((b) => {
      const track = b.ahead || b.behind ? pill(`${b.ahead ? '↑' + b.ahead : ''} ${b.behind ? '↓' + b.behind : ''}`.trim(), 'var(--yellow)') : '';
      return sbItem({ ic: b.current ? 'check' : 'branch', label: b.name, act: 'reveal-branch', arg: b.name, current: b.current, right: track, title: `${b.name}${b.upstream ? ' → ' + b.upstream : ''}\nDoble clic para cambiar a esta rama` });
    }).join('') || '<div class="sb-empty">Aún no hay ramas</div>', 'new-branch');
    h += section('remotes', 'Remotos', r.remotes.length, r.remotes.map((rn) => {
      const k = 'remote:' + rn, col = !!S.settings.collapsed[k];
      return sbItem({ ic: col ? 'chevronRight' : 'chevronDown', label: rn, act: 'toggle-section', arg: k }) +
        (col ? '' : r.remote.filter((x) => x.remote === rn).map((rb) => sbItem({ ic: 'cloud', label: rb.branch, act: 'reveal-remote', arg: rb.name, nested: true, title: `${rb.name}\nDoble clic para crear una rama local y cambiar a ella` })).join(''));
    }).join('') || '<div class="sb-empty">Sin remotos</div>', 'add-remote');
    h += section('tags', 'Etiquetas', r.tags.length, r.tags.map((t) => sbItem({ ic: 'tag', label: t.name, act: 'reveal-hash', arg: t.hash })).join('') || '<div class="sb-empty">Sin etiquetas</div>', 'new-tag');
    h += section('stashes', 'Stashes', r.stashes.length, '<div class="sb-empty">Sin stashes</div>');
    h += section('more', 'Más', null,
      sbItem({ ic: 'box', label: 'Submódulos', act: 'view', arg: 'submodules', selected: v === 'submodules' }) +
      sbItem({ ic: 'layers', label: 'Worktrees', act: 'view', arg: 'worktrees', selected: v === 'worktrees' }) +
      sbItem({ ic: 'lock', label: 'Git LFS', act: 'view', arg: 'lfs', selected: v === 'lfs' }));
    const sb = $('#sidebar');
    const scroll = sb.scrollTop;
    sb.innerHTML = h;
    sb.scrollTop = scroll;
  }

  // ---------------------------------------------------------------- render: status bar (app.rs › status_bar)
  function renderStatus() {
    const b = currentLocal();
    let h = '';
    if (S.screen === 'repo') {
      h += `<div class="part">${icon('branch', 12)}${esc(branchLabel())}</div>`;
      if (b) h += `<div class="part">${icon('cloud', 13)}${b.upstream ? `${esc(b.upstream)} · ↑${b.ahead} ↓${b.behind}` : 'Sin rama de seguimiento'}</div>`;
    }
    if (S.busy) h += `<div class="part">⟳ ${esc(S.busy)}…</div>`;
    h += `<div class="right">${S.screen === 'repo' ? `<span>${S.commits.length} commits</span><span class="ellipsis">${esc(S.repo.path)}</span>` : '<span>git version 2.45.2.windows.1</span>'}</div>`;
    $('#statusbar').innerHTML = h;
  }

  function renderView() {
    Views.render($('#central'));
  }

  function render() {
    document.documentElement.dataset.theme = S.settings.theme;
    document.documentElement.style.setProperty('--sidebar-w', S.settings.sidebarW + 'px');
    const repo = S.screen === 'repo';
    $('#toolbar').classList.toggle('hidden', !repo);
    $('#workspace').classList.toggle('hidden', !repo);
    $('#home').classList.toggle('hidden', repo);
    renderTabs();
    if (repo) {
      renderToolbar();
      renderSidebar();
      renderView();
    } else {
      Views.home($('#home'));
    }
    renderBanner();
    renderStatus();
  }

  // ---------------------------------------------------------------- events
  function onClick(e) {
    const el = e.target.closest('[data-act]');
    if (!el || el.disabled) return;
    const act = el.dataset.act, arg = el.dataset.arg;
    switch (act) {
      case 'tab': if (!e.target.closest('.x')) { S.screen = 'repo'; render(); } break;
      case 'close-tab': S.screen = 'home'; render(); break;
      case 'home': S.screen = 'home'; render(); break;
      case 'view-changes': setView('changes'); break;
      case 'view-history': setView('history'); break;
      case 'view': setView(arg); break;
      case 'toggle-section': S.settings.collapsed[arg] = !S.settings.collapsed[arg]; renderSidebar(); break;
      case 'reveal-branch': case 'reveal-remote': {
        const ref = act === 'reveal-branch' ? S.refs.local.find((b) => b.name === arg) : S.refs.remote.find((b) => b.name === arg);
        if (ref) { S.view = 'history'; render(); select(ref.hash, true); }
        break;
      }
      case 'reveal-hash': S.view = 'history'; render(); select(arg, true); break;
      case 'new-branch': e.stopPropagation(); Dialogs.newBranch(null); break;
      case 'new-tag': e.stopPropagation(); Dialogs.tag(S.refs.head); break;
      case 'add-remote': e.stopPropagation(); Dialogs.addRemote(); break;
      case 'merge': Dialogs.merge(); break;
      case 'stash': Dialogs.stash(); break;
      case 'discard-all': Dialogs.discardAll(); break;
      case 'pull': Dialogs.pull(); break;
      case 'push': Dialogs.push(); break;
      case 'fetch': actions.fetch(); break;
      case 'flow': Dialogs.flowInit(); break;
      case 'create-pr': Dialogs.createPr(); break;
      case 'settings': Dialogs.settings(); break;
      case 'accounts': Dialogs.accounts(); break;
      case 'update': Dialogs.update(); break;
      case 'check-update': S.update ? Dialogs.update() : toast(`Ya tienes la última versión (${VERSION})`); break;
      case 'refresh': recompute(); render(); break;
      case 'abort': Dialogs.confirm('Abortar operación', 'Se descartará la operación en curso y el repositorio volverá al estado anterior.', null, 'Abortar', true, () => op('Abortar', () => { S.status.state = null; S.status.conflicted = []; }, 'Operación abortada')); break;
      case 'continue': op('Continuar', () => { S.status.state = null; }, 'Operación completada'); break;
      case 'mock-only': toast('En la app real abre el programa del sistema.', 'info'); break;
      default: Views.onAct && Views.onAct(act, arg, el, e);
    }
  }

  function onDblClick(e) {
    const el = e.target.closest('[data-act]');
    if (!el) return;
    if (el.dataset.act === 'reveal-branch') actions.checkout(el.dataset.arg);
    if (el.dataset.act === 'reveal-remote') Dialogs.checkoutRemote(el.dataset.arg);
  }

  function onContext(e) {
    const el = e.target.closest('[data-act]');
    if (!el) return;
    const act = el.dataset.act, arg = el.dataset.arg;
    if (act === 'reveal-branch') {
      e.preventDefault();
      const b = S.refs.local.find((x) => x.name === arg);
      menu(e.clientX, e.clientY, [
        { label: 'Cambiar a esta rama', action: () => actions.checkout(arg), disabled: b.current },
        { label: 'Merge en la rama actual…', action: () => Dialogs.merge(arg), disabled: b.current },
        { label: 'Rebase de la rama actual sobre esta…', action: () => toast('Rebase simulado', 'info'), disabled: b.current },
        { label: 'Comparar con la rama actual', action: () => Views.openCompare('HEAD', b.hash, 'HEAD', arg), disabled: b.current },
        '-',
        { label: 'Push…', action: () => Dialogs.push(arg) },
        { label: 'Crear pull request…', action: () => Dialogs.createPr(arg) },
        { label: 'Nueva rama desde aquí…', action: () => Dialogs.newBranch(b.hash) },
        { label: 'Renombrar…', action: () => toast('Renombrar simulado', 'info') },
        { label: 'Copiar nombre', action: () => navigator.clipboard.writeText(arg) },
        '-',
        { label: 'Eliminar…', danger: true, disabled: b.current, action: () => Dialogs.confirm('Eliminar rama', `¿Eliminar la rama local ${arg}?`, null, 'Eliminar', true, () => op('Eliminar rama', () => { S.refs.local = S.refs.local.filter((x) => x.name !== arg); }, `Rama ${arg} eliminada`)) },
      ], arg);
    }
  }

  function onKey(e) {
    if ($('#modal-root')) {
      if (e.key === 'Escape') Dialogs.close();
      return;
    }
    const mod = e.ctrlKey || e.metaKey;
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement.tagName);
    if (e.key === 'F5') { e.preventDefault(); recompute(); render(); }
    else if (mod && e.key === '1') { e.preventDefault(); setView('changes'); }
    else if (mod && e.key === '2') { e.preventDefault(); setView('history'); }
    else if (mod && e.key.toLowerCase() === 't') { e.preventDefault(); S.screen = 'home'; render(); }
    else if (mod && e.key.toLowerCase() === 'f' && !typing) { e.preventDefault(); setView('history'); const f = $('#filter'); f && f.focus(); }
    else if (!typing && Views.onKey) Views.onKey(e);
  }

  function mockbar(e) {
    const t = e.target.closest('[data-mock]');
    if (!t) return;
    const k = t.dataset.mock;
    if (k === 'theme') { S.settings.theme = S.settings.theme === 'dark' ? 'light' : 'dark'; render(); }
    if (k === 'orientation') { S.settings.vertical = !S.settings.vertical; if (Views.explorer) Views.explorer.reset(); recompute(); render(); }
    if (k === 'conflict') {
      S.status.state = 'merge';
      S.status.conflicted = [{ path: 'app.txt', status: 'UU' }];
      S.status.unstaged = S.status.unstaged.filter((f) => f.path !== 'app.txt');
      S.untracked['app.txt'] = 'v0\na\n<<<<<<< HEAD\nfix\n=======\nfix alternativo\n>>>>>>> feature/x\n';
      S.sel = { list: null, paths: new Set(), focus: null, anchor: null };
      recompute(); S.view = 'changes'; render();
    }
    if (k === 'update') { S.update = S.update ? null : clone(MOCK_UPDATE); if (S.update) toast(`CodeSprout ${S.update.version} está disponible. Pulsa «Actualizar» arriba a la derecha.`, 'info'); render(); }
    if (k === 'reset') { loadData(); render(); }
  }
  function mockSize(e) {
    const v = e.target.value, app = $('#app');
    if (!v) { app.style.cssText = ''; }
    else {
      const [w, h] = v.split('x').map(Number);
      app.style.cssText = `width:${w}px;height:${h}px;margin:${Math.max(0, (innerHeight - h) / 2)}px auto 0;outline:1px solid #888;`;
    }
    Views.explorer && Views.explorer.draw();
  }

  function sidebarResize() {
    const handle = $('#sidebar-resize');
    handle.addEventListener('pointerdown', (e) => {
      handle.setPointerCapture(e.pointerId);
      const move = (ev) => {
        S.settings.sidebarW = Math.min(480, Math.max(180, ev.clientX - $('#app').getBoundingClientRect().left));
        document.documentElement.style.setProperty('--sidebar-w', S.settings.sidebarW + 'px');
        Views.explorer && Views.explorer.draw();
      };
      handle.addEventListener('pointermove', move);
      handle.addEventListener('pointerup', () => handle.removeEventListener('pointermove', move), { once: true });
    });
  }

  function start() {
    loadData();
    document.addEventListener('click', onClick);
    document.addEventListener('dblclick', onDblClick);
    document.addEventListener('contextmenu', onContext);
    document.addEventListener('keydown', onKey);
    $('#mockbar').addEventListener('click', mockbar);
    $('#mockbar select').addEventListener('change', mockSize);
    sidebarResize();
    // URL parameters open a given state directly: ?view=changes&vertical=1&theme=light&dialog=rebase&conflict=1&home=1
    const q = new URLSearchParams(location.search);
    if (q.get('theme')) S.settings.theme = q.get('theme');
    if (q.get('vertical') === '1') S.settings.vertical = true;
    if (q.get('list') === '1') S.settings.historyMode = 'list';
    if (q.get('home') === '1') S.screen = 'home';
    if (q.get('conflict') === '1') mockbar({ target: { closest: () => ({ dataset: { mock: 'conflict' } }) } });
    if (q.get('view')) S.view = q.get('view');
    recompute();
    render();
    const dialogs = { update: () => { S.update = clone(MOCK_UPDATE); render(); Dialogs.update(); }, accounts: () => Dialogs.accounts(), settings: () => Dialogs.settings(), rebase: () => { S.status = { state: null, staged: [], unstaged: [], conflicted: [] }; recompute(); render(); Dialogs.interactiveRebase(S.commits[10].hash); }, newbranch: () => Dialogs.newBranch(null), push: () => Dialogs.push(), clone: () => Dialogs.clone(), pr: () => Dialogs.createPr() };
    if (dialogs[q.get('dialog')]) dialogs[q.get('dialog')]();
  }

  return { S, start, render, renderSidebar, renderView, renderStatus, recompute, toast, menu, closeMenu, op, actions, select, setView, canCommit, branchLabel, currentLocal, remoteOp };
})();
