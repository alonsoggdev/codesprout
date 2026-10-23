'use strict';
/* global api, h, put, clear, icon, toast, showDialog, confirmDialog, field, showMenu, isModalOpen, initResizers,
   relTime, fullTime, hashColor, initials, splitPath, plural, progressDialog, Graph, DiffView */

const ROW_H = 28;
const $ = (sel) => document.querySelector(sel);

const S = {
  settings: {
    theme: 'dark',
    maxCommits: 3000,
    autoFetchMin: 0,
    allBranches: true,
    pullRebase: false,
    cloneDir: '',
    layout: {},
    collapsed: {},
  },
  repos: [],
  tabs: [],
  active: null,
  platform: 'win32',
  busy: null,
  repo: new Map(),
};

// ---------------------------------------------------------------- basics

async function call(promise) {
  const r = await promise;
  if (!r || !r.ok) throw new Error((r && r.error) || 'Error desconocido');
  return r.data;
}

const g = (method, ...args) => call(api.git(method, ...args));
const safely = (fn) => Promise.resolve().then(fn).catch((e) => toast(e.message || String(e), 'error'));

function basename(p) {
  return String(p).split(/[\\/]/).filter(Boolean).pop() || p;
}

function samePath(a, b) {
  return S.platform === 'linux' ? a === b : String(a).toLowerCase() === String(b).toLowerCase();
}

function joinPath(dir, name) {
  const sep = S.platform === 'win32' ? '\\' : '/';
  return dir.replace(/[\\/]+$/, '') + sep + name;
}

let saveTimer = null;
function persist() {
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    api.writeStore({ settings: S.settings, repos: S.repos, tabs: S.tabs, active: S.active });
  }, 250);
}

function RS(p) {
  let st = S.repo.get(p);
  if (!st) {
    st = {
      path: p,
      view: 'history',
      status: null,
      refs: null,
      sig: null,
      commits: [],
      list: [],
      visible: [],
      graph: null,
      graphW: 30,
      refMap: new Map(),
      loaded: false,
      error: null,
      filter: '',
      selected: null,
      detail: null,
      detailFile: null,
      detailDiff: null,
      detailToken: 0,
      diffToken: 0,
      sel: { list: null, paths: new Set(), focus: null, anchor: null },
      message: '',
      amend: false,
      pushAfter: false,
      mergePrefilled: false,
      lastFetch: Date.now(),
    };
    S.repo.set(p, st);
  }
  return st;
}

const cur = () => (S.active ? RS(S.active) : null);
const currentLocal = (st) => (st && st.refs ? st.refs.local.find((b) => b.current) || null : null);
const changeCount = (s) => (s ? s.staged.length + s.unstaged.length + s.conflicted.length : 0);

// ---------------------------------------------------------------- operations

async function withBusy(label, fn) {
  if (S.busy) throw new Error(`Espera a que termine la operación en curso (${S.busy}).`);
  S.busy = label;
  renderBusy();
  try {
    return await fn();
  } finally {
    S.busy = null;
    renderBusy();
  }
}

function showError(title, e) {
  const msg = (e && e.message) || String(e);
  const identity = /user\.email|user\.name|tell me who you are|empty ident/i.test(msg);
  showDialog({
    title,
    width: 560,
    body: [
      h('div', { class: 'modal-error' }, msg),
      identity ? h('p', null, 'Git necesita tu nombre y tu email para firmar los commits.') : null,
    ],
    actions: identity
      ? [{ label: 'Cerrar' }, { label: 'Configurar identidad', primary: true, value: 'identity' }]
      : [{ label: 'Aceptar', primary: true }],
  }).then((v) => {
    if (v === 'identity') openIdentityDialog();
  });
}

// Runs a local git operation with the busy lock; errors are shown in a dialog. Returns undefined on failure.
async function op(label, fn, { success } = {}) {
  if (S.busy) {
    toast(`Espera a que termine la operación en curso (${S.busy}).`, 'warn');
    return undefined;
  }
  try {
    const res = await withBusy(label, fn);
    if (success) toast(success, 'success');
    return res === undefined ? true : res;
  } catch (e) {
    showError(label, e);
    return undefined;
  } finally {
    refresh();
  }
}

// For dialog handlers: errors propagate so the dialog shows them inline.
async function dialogOp(label, fn) {
  try {
    const res = await withBusy(label, fn);
    return res === undefined ? true : res;
  } finally {
    refresh();
  }
}

let opSeq = 0;
const progressHandlers = new Map();

// Network operation with a progress dialog and cancel support.
async function remoteOp(title, method, args, successMsg) {
  if (S.busy) {
    toast(`Espera a que termine la operación en curso (${S.busy}).`, 'warn');
    return undefined;
  }
  const opId = `op-${Date.now()}-${++opSeq}`;
  const pd = progressDialog(title, () => api.cancel(opId));
  progressHandlers.set(opId, (line) => pd.line(line));
  try {
    const res = await withBusy(title, () => g(method, ...args, opId));
    pd.success();
    if (successMsg) toast(successMsg, 'success');
    return res === undefined || res === null ? true : res;
  } catch (e) {
    pd.fail(e.message);
    await pd.closed;
    return undefined;
  } finally {
    progressHandlers.delete(opId);
    refresh();
  }
}

function handleConflicts(res, what) {
  if (res && res.conflicts) {
    toast(`${what} ha generado conflictos. Resuélvelos en «Cambios» y confirma.`, 'warn', 9000);
    setView('changes');
    return true;
  }
  return false;
}

// ---------------------------------------------------------------- refresh

let refreshPromise = null;
let refreshAgain = false;
let refreshAgainFull = false;

function refresh(full = false) {
  if (!S.active) return Promise.resolve();
  if (refreshPromise) {
    refreshAgain = true;
    refreshAgainFull = refreshAgainFull || full;
    return refreshPromise;
  }
  refreshPromise = doRefresh(S.active, full).finally(() => {
    refreshPromise = null;
    if (refreshAgain) {
      const f = refreshAgainFull;
      refreshAgain = false;
      refreshAgainFull = false;
      refresh(f);
    }
  });
  return refreshPromise;
}

let refreshTimer = null;
function scheduleRefresh(delay = 150) {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => {
    if (!S.busy) refresh();
  }, delay);
}

async function doRefresh(p, full) {
  const st = RS(p);
  try {
    const [status, refs] = await Promise.all([g('status', p), g('refs', p)]);
    let commits = null;
    if (full || !st.loaded || refs.sig !== st.sig) {
      commits = await g('log', p, { max: S.settings.maxCommits, all: S.settings.allBranches });
    }
    const hadConflicts = st.status ? st.status.conflicted.length > 0 : false;
    if (!hadConflicts && status.conflicted.length) {
      // conflicts just appeared: put the first one in front of the user
      const first = status.conflicted[0].path;
      st.sel = { list: 'conflicted', paths: new Set([first]), focus: first, anchor: first };
      st.lastFocus = first;
    }
    st.status = status;
    st.refs = refs;
    st.sig = refs.sig;
    st.error = null;
    if (commits) {
      st.commits = commits;
      st.loaded = true;
    }
    if (!status.state) {
      st.mergePrefilled = false;
      // drop an untouched, auto-filled merge message once the merge is aborted or finished
      if (st.mergeMsgPrefill && st.message === st.mergeMsgPrefill) {
        st.message = '';
        if (S.active === p) $('#commit-message').value = '';
      }
      st.mergeMsgPrefill = null;
    }
    buildRefMap(st);
    rebuildList(st);
    if (S.active !== p) return;
    renderAll();
    afterRefresh(st);
  } catch (e) {
    st.error = e.message || String(e);
    if (S.active === p) renderAll();
  }
}

function afterRefresh(st) {
  // history selection
  const inList = (hash) => st.list.some((c) => c.hash === hash);
  const isStash = (hash) => st.refs.stashes.some((s) => s.hash === hash);
  if (st.selected && !inList(st.selected) && !isStash(st.selected)) st.selected = null;
  if (!st.selected && st.list.length) {
    const head = st.list.find((c) => c.hash === st.refs.head && !c.pseudo);
    st.selected = (head || st.list.find((c) => !c.pseudo) || st.list[0]).hash;
  }
  if (st.view === 'history') {
    renderHistoryRows();
    if (st.selected && st.selected !== 'WORKTREE' && (!st.detail || st.detail.hash !== st.selected)) loadDetail(st, st.selected);
    else if (!st.selected || st.selected === 'WORKTREE') renderDetail(st);
  } else {
    pruneSelection(st);
    renderChanges();
    loadChangeDiff(st);
  }
  maybePrefillMerge(st);
}

function buildRefMap(st) {
  const map = new Map();
  const add = (hash, ref) => {
    if (!hash) return;
    if (!map.has(hash)) map.set(hash, []);
    map.get(hash).push(ref);
  };
  const r = st.refs;
  if (!r.currentBranch && r.head) add(r.head, { type: 'head', name: 'HEAD', title: 'HEAD separado' });
  for (const b of r.local) add(b.hash, { type: 'branch', name: b.name, current: b.current, title: b.current ? `Rama actual: ${b.name}` : b.name });
  for (const b of r.remote) add(b.hash, { type: 'remote', name: b.name });
  for (const t of r.tags) add(t.hash, { type: 'tag', name: t.name });
  for (const list of map.values()) list.sort((a, b) => (b.current ? 1 : 0) - (a.current ? 1 : 0));
  st.refMap = map;
}

function rebuildList(st) {
  const n = changeCount(st.status);
  const list = st.commits.slice();
  if (n > 0) {
    list.unshift({
      hash: 'WORKTREE',
      parents: st.refs && st.refs.head ? [st.refs.head] : [],
      subject: `Cambios sin confirmar (${n})`,
      author: '',
      time: null,
      pseudo: true,
    });
  }
  st.list = list;
  st.graph = Graph.computeGraph(list);
  st.graphW = Math.max(64, Math.min(st.graph.maxLanes, 16) * Graph.LANE_W + 12);
  applyFilter(st);
}

function applyFilter(st) {
  const q = st.filter.trim().toLowerCase();
  if (!q) {
    st.visible = st.list;
    return;
  }
  st.visible = st.list.filter((c) => !c.pseudo && (
    c.subject.toLowerCase().includes(q)
    || c.author.toLowerCase().includes(q)
    || (c.email || '').toLowerCase().includes(q)
    || c.hash.startsWith(q)
    || (st.refMap.get(c.hash) || []).some((r) => r.name.toLowerCase().includes(q))));
}

// ---------------------------------------------------------------- tabs / activation

async function activate(p) {
  S.active = p;
  if (p && !S.tabs.some((t) => t === p)) S.tabs.push(p);
  persist();
  call(api.watch(p || null)).catch(() => {});
  const st = cur();
  syncCommitBox(st);
  if (st) $('#history-filter').value = st.filter;
  renderAll();
  if (st) {
    hist.scroll.scrollTop = st.historyScroll || 0;
    renderHistoryRows();
  }
  if (st && st.loaded) afterRefresh(st);
  if (p) await refresh();
}

function closeTab(p) {
  const idx = S.tabs.indexOf(p);
  if (idx === -1) return;
  S.tabs.splice(idx, 1);
  S.repo.delete(p);
  if (S.active === p) activate(S.tabs[Math.min(idx, S.tabs.length - 1)] || null);
  else {
    persist();
    renderTabs();
  }
}

function addRepo(p) {
  const existing = S.repos.findIndex((r) => samePath(r.path, p));
  if (existing !== -1) S.repos.splice(existing, 1);
  S.repos.unshift({ path: p, name: basename(p) });
  persist();
}

async function openRepo(dir) {
  try {
    const p = await g('resolveRepo', dir);
    addRepo(p);
    const tab = S.tabs.find((t) => samePath(t, p));
    await activate(tab || p);
  } catch (e) {
    toast(e.message, 'error');
  }
}

function setView(view) {
  const st = cur();
  if (!st) return;
  st.view = view;
  renderAll();
  if (view === 'changes') {
    pruneSelection(st);
    renderChanges();
    loadChangeDiff(st);
  } else {
    renderHistoryRows();
    if (st.selected && st.selected !== 'WORKTREE' && (!st.detail || st.detail.hash !== st.selected)) loadDetail(st, st.selected);
  }
}

// ---------------------------------------------------------------- render: shell

function renderAll() {
  renderTabs();
  const st = cur();
  $('#home').classList.toggle('hidden', !!st);
  $('#workspace').classList.toggle('hidden', !st);
  if (!st) {
    renderHome();
    renderStatusBar();
    return;
  }
  const err = $('#repo-error');
  if (st.error && !st.status) {
    err.classList.remove('hidden');
    renderRepoError(st);
  } else {
    err.classList.add('hidden');
  }
  renderToolbar();
  renderBanner();
  renderSidebar();
  $('#view-history').classList.toggle('hidden', st.view !== 'history');
  $('#view-changes').classList.toggle('hidden', st.view !== 'changes');
  if (st.view === 'history') renderHistory();
  else renderChanges();
  renderStatusBar();
}

function renderBusy() {
  if (S.active) renderToolbar();
  renderStatusBar();
  updateCommitControls(cur());
}

function renderTabs() {
  const nav = $('#tabs');
  clear(nav);
  for (const p of S.tabs) {
    const st = S.repo.get(p);
    const dirty = st && changeCount(st.status) > 0;
    const tab = h('div', {
      class: `tab${S.active === p ? ' active' : ''}`,
      title: p,
      onMousedown: (e) => {
        if (e.button === 1) {
          e.preventDefault();
          closeTab(p);
        }
      },
      onClick: () => S.active !== p && activate(p),
      onContextmenu: (e) => {
        e.preventDefault();
        showMenu(e.clientX, e.clientY, [
          { label: 'Cerrar pestaña', icon: 'x', action: () => closeTab(p) },
          { label: 'Cerrar las demás', action: () => { for (const t of S.tabs.filter((x) => x !== p)) closeTab(t); } },
          'sep',
          { label: 'Mostrar en el explorador', icon: 'folder', action: () => safely(() => call(api.reveal(p, ''))) },
          { label: 'Abrir terminal aquí', icon: 'terminal', action: () => safely(() => call(api.openTerminal(p))) },
          { label: 'Copiar ruta', icon: 'copy', action: () => call(api.copy(p)) },
        ]);
      },
    },
    icon('repo', 14),
    h('span', { class: 'tab-name' }, basename(p)),
    dirty ? h('span', { class: 'dot', title: 'Hay cambios sin confirmar' }) : null,
    h('span', { class: 'tab-close', title: 'Cerrar pestaña', onClick: (e) => { e.stopPropagation(); closeTab(p); } }, icon('x', 12)));
    nav.appendChild(tab);
  }
  nav.appendChild(h('div', {
    class: `tab add${S.active === null ? ' active' : ''}`,
    title: 'Repositorios (Ctrl+T)',
    onClick: () => S.active !== null && activate(null),
  }, icon(S.tabs.length ? 'plus' : 'home', 15)));
}

function tbBtn(iconName, label, onClick, { badge = null, disabled = false, title = null } = {}) {
  return h('button', { class: 'tb-btn', disabled, title: title || label, onClick },
    icon(iconName, 20, 1.7), h('span', null, label), badge ? h('span', { class: 'badge-count' }, String(badge)) : null);
}

function renderToolbar() {
  const st = cur();
  const tb = $('#toolbar');
  clear(tb);
  if (!st) return;
  const busy = !!S.busy;
  const ready = !!st.status;
  const b = currentLocal(st);
  const remotes = st.refs ? st.refs.remotes.length : 0;
  const n = changeCount(st.status);
  const noRemote = 'No hay remotos configurados';
  put(tb,
    tbBtn('commit', 'Commit', () => setView('changes'), { badge: n || null, disabled: !ready }),
    tbBtn('pull', 'Pull', () => openPullDialog(), { badge: b && b.behind ? b.behind : null, disabled: busy || !remotes, title: remotes ? 'Traer y fusionar cambios del remoto' : noRemote }),
    tbBtn('push', 'Push', () => openPushDialog(), { badge: b && b.ahead ? b.ahead : null, disabled: busy || !remotes, title: remotes ? 'Enviar commits al remoto' : noRemote }),
    tbBtn('fetch', 'Fetch', () => doFetch(), { disabled: busy || !remotes, title: remotes ? 'Descargar novedades de todos los remotos' : noRemote }),
    h('div', { class: 'tb-sep' }),
    tbBtn('branch', 'Rama', () => openBranchDialog(), { disabled: busy || !ready || !(st.refs && st.refs.head) }),
    tbBtn('merge', 'Merge', () => openMergeDialog(), { disabled: busy || !ready || !(st.refs && st.refs.head) }),
    tbBtn('stash', 'Stash', () => openStashDialog(), { disabled: busy || !n || !(st.refs && st.refs.head) }),
    tbBtn('discard', 'Descartar', () => openDiscardAllDialog(), { disabled: busy || !n }),
    h('div', { class: 'tb-sep' }),
    tbBtn('terminal', 'Terminal', () => safely(() => call(api.openTerminal(st.path)))),
    tbBtn('folder', 'Explorador', () => safely(() => call(api.reveal(st.path, '')))),
    h('div', { class: 'tb-spacer' }),
    h('div', { class: 'tb-repo' },
      h('div', { class: 'name ellipsis' }, basename(st.path)),
      h('div', { class: 'branch ellipsis' }, icon('branch', 12), branchLabel(st))),
    tbBtn('refresh', 'Actualizar', () => refresh(true), { title: 'Actualizar (F5)' }),
    tbBtn('settings', 'Ajustes', () => openSettings()),
  );
}

function branchLabel(st) {
  if (!st.refs) return '…';
  if (st.refs.currentBranch) return st.refs.currentBranch;
  if (st.refs.head) return `HEAD separado (${st.refs.head.slice(0, 7)})`;
  return 'sin commits';
}

function renderBanner() {
  const st = cur();
  const el = $('#banner');
  const s = st && st.status;
  if (!s || !s.state) {
    el.classList.add('hidden');
    return;
  }
  const names = { merge: 'Fusión en curso', rebase: 'Rebase en curso', 'cherry-pick': 'Cherry-pick en curso', revert: 'Revert en curso' };
  const n = s.conflicted.length;
  const text = n
    ? `${names[s.state]} — ${plural(n, 'archivo tiene', 'archivos tienen')} conflictos. Resuélvelos y márcalos como resueltos.`
    : `${names[s.state]} — no quedan conflictos, ya puedes continuar.`;
  const continueLabel = s.state === 'merge' ? 'Confirmar fusión' : 'Continuar';
  put(clear(el),
    icon('alert', 18),
    h('div', { class: 'grow' }, text),
    n ? h('button', { class: 'btn small', onClick: () => setView('changes') }, 'Ver conflictos') : null,
    h('button', { class: 'btn small', onClick: () => abortOperation(s.state) }, 'Abortar'),
    s.state === 'rebase' ? h('button', { class: 'btn small', onClick: () => skipRebase() }, icon('skip', 12), 'Saltar commit') : null,
    h('button', { class: 'btn small primary', disabled: n > 0 || !!S.busy, onClick: () => continueOperation(s.state) }, icon('play', 12), continueLabel),
  );
  el.classList.remove('hidden');
}

function renderStatusBar() {
  const bar = $('#statusbar');
  clear(bar);
  const st = cur();
  if (st && st.refs) {
    const b = currentLocal(st);
    bar.appendChild(h('div', { class: 'sb-part' }, icon('branch', 13), branchLabel(st)));
    if (b && b.upstream) {
      bar.appendChild(h('div', { class: 'sb-part', title: 'Rama remota de seguimiento' }, icon('cloud', 13), b.upstream,
        b.gone ? ' (eliminada en el remoto)' : ` · ↑${b.ahead} ↓${b.behind}`));
    } else if (b) {
      bar.appendChild(h('div', { class: 'sb-part' }, icon('cloud', 13), 'Sin rama de seguimiento'));
    }
  }
  if (S.busy) bar.appendChild(h('div', { class: 'sb-part busy' }, h('span', { class: 'spinner' }), `${S.busy}…`));
  bar.appendChild(h('div', { class: 'grow' }));
  if (st && st.loaded) {
    const capped = st.commits.length >= S.settings.maxCommits;
    bar.appendChild(h('div', { class: 'sb-part', title: capped ? 'Límite configurable en Ajustes' : '' },
      `${st.commits.length.toLocaleString('es')} commits${capped ? ' (límite)' : ''}`));
  }
  if (st) bar.appendChild(h('div', { class: 'sb-part ellipsis', title: st.path }, st.path));
}

function renderRepoError(st) {
  const el = $('#repo-error');
  clear(el).appendChild(h('div', { class: 'error-card' },
    icon('alert', 36),
    h('h2', null, 'No se puede abrir el repositorio'),
    h('pre', null, st.error),
    h('div', { style: { display: 'flex', gap: '8px', justifyContent: 'center', marginTop: '14px' } },
      h('button', { class: 'btn', onClick: () => refresh(true) }, icon('refresh', 14), 'Reintentar'),
      h('button', { class: 'btn danger', onClick: () => { S.repos = S.repos.filter((r) => r.path !== st.path); closeTab(st.path); } }, 'Quitar de la lista'))));
}

// ---------------------------------------------------------------- render: sidebar

function sbItem({ iconName, label, title, selected, current, pill, pillAccent, track, nested, onClick, onDblclick, onContextmenu }) {
  return h('div', {
    class: `sb-item${selected ? ' selected' : ''}${current ? ' current' : ''}${nested ? ' nested' : ''}`,
    title: title || (typeof label === 'string' ? label : null),
    onClick,
    onDblclick,
    onContextmenu: onContextmenu ? (e) => { e.preventDefault(); onContextmenu(e); } : null,
  },
  iconName ? icon(iconName, 15) : null,
  h('span', { class: 'label' }, label),
  track || null,
  pill != null ? h('span', { class: `pill${pillAccent ? ' accent' : ''}` }, String(pill)) : null);
}

function sbSection(key, title, count, items, addBtn) {
  const collapsed = !!S.settings.collapsed[key];
  const head = h('div', {
    class: 'sb-title',
    onClick: () => {
      S.settings.collapsed[key] = !collapsed;
      persist();
      renderSidebar();
    },
  },
  icon(collapsed ? 'chevronRight' : 'chevronDown', 13),
  h('span', { class: 'grow' }, title, count != null ? h('span', { class: 'count' }, ` ${count}`) : null),
  addBtn ? h('button', { class: 'icon-btn', title: addBtn.title, onClick: (e) => { e.stopPropagation(); addBtn.onClick(); } }, icon(addBtn.icon, 14)) : null);
  return h('div', { class: 'sb-section' }, head, collapsed ? null : items);
}

function refLabel(name) {
  const { dir, base } = splitPath(name);
  return [dir ? h('span', { class: 'prefix' }, dir) : null, base];
}

function renderSidebar() {
  const st = cur();
  const sb = $('#sidebar');
  const scroll = sb.scrollTop;
  clear(sb);
  if (!st) return;
  const n = changeCount(st.status);
  sb.appendChild(sbSection('workspace', 'Área de trabajo', null, [
    sbItem({ iconName: 'changes', label: 'Cambios', selected: st.view === 'changes', pill: n || null, pillAccent: n > 0, onClick: () => setView('changes'), title: 'Cambios en el directorio de trabajo (Ctrl+1)' }),
    sbItem({ iconName: 'history', label: 'Historial', selected: st.view === 'history', onClick: () => setView('history'), title: 'Historial de commits (Ctrl+2)' }),
  ]));
  const r = st.refs;
  if (!r) return;

  const branches = r.local.map((b) => sbItem({
    iconName: b.current ? 'check' : 'branch',
    label: refLabel(b.name),
    title: `${b.name}${b.upstream ? ` → ${b.upstream}` : ''}\nDoble clic para cambiar a esta rama`,
    current: b.current,
    track: b.gone
      ? h('span', { class: 'track', title: 'La rama remota de seguimiento ya no existe' }, 'remota eliminada')
      : (b.ahead || b.behind) ? h('span', { class: 'track' }, b.ahead ? `↑${b.ahead}` : null, b.behind ? `↓${b.behind}` : null) : null,
    onClick: () => revealCommit(b.hash),
    onDblclick: () => checkoutBranch(b.name),
    onContextmenu: (e) => localBranchMenu(e, b),
  }));
  sb.appendChild(sbSection('branches', 'Ramas', r.local.length,
    branches.length ? branches : h('div', { class: 'sb-empty' }, 'Aún no hay ramas'),
    r.head ? { icon: 'plus', title: 'Nueva rama', onClick: () => openBranchDialog() } : null));

  const remoteItems = [];
  for (const rn of r.remotes) {
    const key = `remote:${rn}`;
    const collapsed = !!S.settings.collapsed[key];
    remoteItems.push(sbItem({
      iconName: collapsed ? 'chevronRight' : 'chevronDown',
      label: rn,
      onClick: () => {
        S.settings.collapsed[key] = !collapsed;
        persist();
        renderSidebar();
      },
      onContextmenu: (e) => remoteMenu(e, rn),
    }));
    if (!collapsed) {
      const list = r.remote.filter((x) => x.remote === rn);
      if (!list.length) remoteItems.push(h('div', { class: 'sb-empty', style: { paddingLeft: '30px' } }, 'Sin ramas (haz fetch)'));
      for (const rb of list) {
        remoteItems.push(sbItem({
          iconName: 'cloud',
          label: refLabel(rb.branch),
          title: `${rb.name}\nDoble clic para crear una rama local y cambiar a ella`,
          nested: true,
          onClick: () => revealCommit(rb.hash),
          onDblclick: () => checkoutRemoteBranch(rb),
          onContextmenu: (e) => remoteBranchMenu(e, rb),
        }));
      }
    }
  }
  sb.appendChild(sbSection('remotes', 'Remotos', r.remotes.length,
    remoteItems.length ? remoteItems : h('div', { class: 'sb-empty' }, 'Sin remotos'),
    { icon: 'plus', title: 'Añadir remoto', onClick: () => openAddRemoteDialog() }));

  sb.appendChild(sbSection('tags', 'Etiquetas', r.tags.length,
    r.tags.length ? r.tags.map((t) => sbItem({
      iconName: 'tag',
      label: t.name,
      onClick: () => revealCommit(t.hash),
      onContextmenu: (e) => tagMenu(e, t),
    })) : h('div', { class: 'sb-empty' }, 'Sin etiquetas'),
    r.head ? { icon: 'plus', title: 'Nueva etiqueta', onClick: () => openTagDialog(r.head) } : null));

  sb.appendChild(sbSection('stashes', 'Stashes', r.stashes.length,
    r.stashes.length ? r.stashes.map((s) => sbItem({
      iconName: 'stash',
      label: s.message.replace(/^(WIP on|On) [^:]+: /, '') || s.ref,
      title: `${s.ref}: ${s.message}\nDoble clic para aplicar`,
      selected: st.view === 'history' && st.selected === s.hash,
      onClick: () => showStash(s),
      onDblclick: () => applyStash(s, false),
      onContextmenu: (e) => stashMenu(e, s),
    })) : h('div', { class: 'sb-empty' }, 'Sin stashes')));
  sb.scrollTop = scroll;
}

// ---------------------------------------------------------------- render: history

const hist = {};

function renderHistory() {
  const st = cur();
  $('#view-history').style.setProperty('--graph-w', `${st.filter ? 64 : st.graphW}px`);
  $('#all-branches').checked = !!S.settings.allBranches;
  const count = $('#history-count');
  count.textContent = st.filter ? `${st.visible.length} coincidencias` : '';
  renderHistoryRows();
  renderDetail(st);
}

function renderHistoryRows() {
  const st = cur();
  if (!st || st.view !== 'history') return;
  const list = st.visible;
  hist.spacer.style.height = `${list.length * ROW_H}px`;
  clear(hist.rows);
  if (!st.loaded) {
    hist.rows.appendChild(h('div', { class: 'history-empty' }, 'Cargando historial…'));
    return;
  }
  if (!list.length) {
    hist.rows.appendChild(h('div', { class: 'history-empty' }, st.filter ? 'Ningún commit coincide con la búsqueda.' : 'Este repositorio todavía no tiene commits. Prepara archivos en «Cambios» y crea el primero.'));
    return;
  }
  const top = hist.scroll.scrollTop;
  const height = hist.scroll.clientHeight || 600;
  const first = Math.max(0, Math.floor(top / ROW_H) - 10);
  const last = Math.min(list.length, Math.ceil((top + height) / ROW_H) + 10);
  const frag = document.createDocumentFragment();
  for (let i = first; i < last; i++) frag.appendChild(historyRow(st, list[i], i));
  hist.rows.appendChild(frag);
}

function refBadges(st, hash) {
  const refs = st.refMap.get(hash);
  if (!refs) return null;
  return refs.map((r) => h('span', { class: `ref ${r.type}${r.current ? ' current' : ''}`, title: r.title || r.name },
    r.type === 'tag' ? icon('tag', 10, 2.2) : r.type === 'remote' ? icon('cloud', 11, 2.2) : r.current ? icon('check', 11, 2.6) : null,
    r.name));
}

function historyRow(st, c, i) {
  const isHead = st.refs && c.hash === st.refs.head;
  const row = h('div', {
    class: `hrow${st.selected === c.hash ? ' selected' : ''}${c.pseudo ? ' pseudo' : ''}${isHead ? ' is-head' : ''}`,
    style: { top: `${i * ROW_H}px` },
    dataset: { i: String(i) },
  });
  const graphCell = h('div', { class: 'c-graph' });
  if (!st.filter && st.graph.rows[i]) {
    const kind = c.pseudo ? 'worktree' : isHead ? 'head' : c.parents.length > 1 ? 'merge' : 'normal';
    graphCell.innerHTML = Graph.rowSvg(st.graph.rows[i], { height: ROW_H, width: st.graphW, kind });
  }
  put(row,
    graphCell,
    h('div', { class: 'c-desc' }, refBadges(st, c.hash), h('span', { class: 'subject', title: c.subject }, c.subject)),
    h('div', { class: 'c-author', title: c.email || '' }, c.author),
    h('div', { class: 'c-date', title: c.time ? fullTime(c.time) : '' }, c.time ? relTime(c.time) : ''),
    h('div', { class: 'c-hash' }, c.pseudo ? '' : c.hash.slice(0, 8)),
  );
  return row;
}

function selectCommit(st, c, { scroll = false } = {}) {
  st.selected = c.hash;
  if (scroll) {
    const idx = st.visible.indexOf(c);
    const top = idx * ROW_H;
    const view = hist.scroll;
    if (top < view.scrollTop) view.scrollTop = top;
    else if (top + ROW_H > view.scrollTop + view.clientHeight) view.scrollTop = top + ROW_H - view.clientHeight;
  }
  renderHistoryRows();
  if (c.pseudo) renderDetail(st);
  else loadDetail(st, c.hash);
}

function revealCommit(hash) {
  const st = cur();
  if (!st) return;
  if (st.view !== 'history') setView('history');
  let c = st.visible.find((x) => x.hash === hash);
  if (!c && st.filter) {
    st.filter = '';
    $('#history-filter').value = '';
    applyFilter(st);
    renderHistory();
    c = st.visible.find((x) => x.hash === hash);
  }
  if (!c) {
    toast('Ese commit no está entre los commits cargados (activa «Todas las ramas» o aumenta el límite en Ajustes).', 'info');
    return;
  }
  const idx = st.visible.indexOf(c);
  hist.scroll.scrollTop = Math.max(0, idx * ROW_H - hist.scroll.clientHeight / 3);
  selectCommit(st, c);
  hist.scroll.focus();
}

function showStash(s) {
  const st = cur();
  if (st.view !== 'history') setView('history');
  st.selected = s.hash;
  renderHistoryRows();
  renderSidebar();
  loadDetail(st, s.hash);
}

async function loadDetail(st, hash) {
  const token = ++st.detailToken;
  try {
    const d = await g('commitDetails', st.path, hash);
    if (token !== st.detailToken) return;
    st.detail = d;
    st.detailFile = d.files[0] || null;
    st.detailDiff = null;
    if (S.active === st.path && st.view === 'history') renderDetail(st);
    if (st.detailFile) loadCommitFileDiff(st);
  } catch (e) {
    if (token !== st.detailToken) return;
    st.detail = null;
    if (S.active === st.path) {
      clear($('#detail-left')).appendChild(h('div', { class: 'diff-placeholder' }, icon('alert', 28), e.message));
      DiffView.placeholder($('#detail-diff'), 'file', 'Sin detalles');
    }
  }
}

async function loadCommitFileDiff(st) {
  const d = st.detail;
  const f = st.detailFile;
  if (!d || !f) return;
  const token = ++st.diffToken;
  try {
    const res = await g('commitFileDiff', st.path, d.base, d.hash, f);
    if (token !== st.diffToken) return;
    st.detailDiff = {
      key: `${st.path}|${d.hash}|${f.path}`,
      title: f.path,
      titleExtra: f.orig ? h('span', { class: 'dir' }, `  ← ${f.orig}`) : null,
      mode: 'commit',
      ...res,
      note: d.parents.length > 1 ? 'Commit de fusión: se muestran los cambios respecto al primer padre.' : null,
      actions: commitFileActions(st, f),
    };
    if (S.active === st.path && st.view === 'history') DiffView.render($('#detail-diff'), st.detailDiff);
  } catch (e) {
    if (token === st.diffToken && S.active === st.path) DiffView.placeholder($('#detail-diff'), 'alert', 'No se pudo cargar el diff', e.message);
  }
}

function commitFileActions(st, f) {
  if (f.status === 'D') return null;
  return [
    h('button', { class: 'icon-btn', title: 'Abrir archivo (versión actual)', onClick: () => safely(() => call(api.openPath(st.path, f.path))) }, icon('external', 15)),
    h('button', { class: 'icon-btn', title: 'Mostrar en la carpeta', onClick: () => safely(() => call(api.reveal(st.path, f.path))) }, icon('folder', 15)),
  ];
}

function statusBadge(status) {
  const names = { M: 'Modificado', A: 'Añadido', D: 'Eliminado', R: 'Renombrado', C: 'Copiado', T: 'Cambio de tipo', '?': 'Sin seguimiento', U: 'Conflicto' };
  const letter = status.length > 1 ? 'U' : status;
  const shown = letter === '?' ? '+' : letter;
  return h('span', { class: `st st-${letter}`, title: names[letter] || status }, shown);
}

function fileName(f) {
  const { dir, base } = splitPath(f.path);
  return h('div', { class: 'fname' }, base, dir ? h('span', { class: 'dir' }, `  ${dir.replace(/\/$/, '')}`) : null,
    f.orig ? h('span', { class: 'orig' }, `  ← ${f.orig}`) : null);
}

function renderDetail(st) {
  const left = $('#detail-left');
  const diffHost = $('#detail-diff');
  if (!st.selected || st.selected === 'WORKTREE') {
    clear(left).appendChild(h('div', { class: 'diff-placeholder' }, icon('commit', 30, 1.5),
      st.selected === 'WORKTREE' ? 'Cambios sin confirmar' : 'Selecciona un commit',
      st.selected === 'WORKTREE' ? h('button', { class: 'btn primary', onClick: () => setView('changes') }, 'Ver cambios') : null));
    DiffView.placeholder(diffHost, 'file', 'Sin archivo seleccionado');
    return;
  }
  const d = st.detail;
  if (!d || d.hash !== st.selected) {
    clear(left).appendChild(h('div', { class: 'diff-placeholder' }, h('span', { class: 'spinner' }), 'Cargando…'));
    return;
  }
  const lines = d.message.split('\n');
  const subject = lines[0];
  const bodyText = lines.slice(1).join('\n').trim();
  const stash = st.refs && st.refs.stashes.find((s) => s.hash === d.hash);
  const meta = h('div', { class: 'meta' },
    stash ? h('div', { style: { marginBottom: '6px' } }, h('span', { class: 'ref stash' }, icon('stash', 11), stash.ref)) : null,
    h('div', { class: 'subject' }, subject),
    bodyText ? h('pre', { class: 'body' }, bodyText) : null,
    h('div', { class: 'who' },
      h('div', { class: 'avatar', style: { background: hashColor(d.email || d.author) } }, initials(d.author)),
      h('div', { class: 'grow' },
        h('div', { class: 'ellipsis selectable' }, h('strong', null, d.author), ' ', h('span', { class: 'muted' }, `<${d.email}>`)),
        h('div', { class: 'muted small', title: fullTime(d.time) }, `${relTime(d.time)} · ${fullTime(d.time)}`))),
    h('div', { class: 'kv' },
      h('div', { class: 'k' }, 'Commit'),
      h('div', { class: 'v copy-hash' }, h('span', { class: 'link selectable', title: 'Copiar hash', onClick: () => copy(d.hash, 'Hash copiado') }, d.hash.slice(0, 12)),
        h('button', { class: 'icon-btn', title: 'Copiar hash completo', onClick: () => copy(d.hash, 'Hash copiado') }, icon('copy', 13))),
      d.parents.length ? h('div', { class: 'k' }, d.parents.length > 1 ? 'Padres' : 'Padre') : null,
      d.parents.length ? h('div', { class: 'v' }, d.parents.map((p, i) => [i ? ', ' : '', h('span', { class: 'link', title: p, onClick: () => revealCommit(p) }, p.slice(0, 8))])) : null,
      d.committer !== d.author ? h('div', { class: 'k' }, 'Committer') : null,
      d.committer !== d.author ? h('div', { class: 'v ellipsis' }, `${d.committer} · ${relTime(d.commitTime)}`) : null));

  const files = h('div', { class: 'file-list' });
  for (const f of d.files) {
    const selected = st.detailFile && st.detailFile.path === f.path;
    files.appendChild(h('div', {
      class: `frow${selected ? ' selected' : ''}`,
      title: f.orig ? `${f.orig} → ${f.path}` : f.path,
      onClick: () => {
        st.detailFile = f;
        renderDetail(st);
        loadCommitFileDiff(st);
      },
      onContextmenu: (e) => {
        e.preventDefault();
        showMenu(e.clientX, e.clientY, [
          { header: f.path },
          { label: 'Copiar ruta', icon: 'copy', action: () => copy(f.path, 'Ruta copiada') },
          f.status !== 'D' ? { label: 'Abrir archivo', icon: 'external', action: () => safely(() => call(api.openPath(st.path, f.path))) } : null,
          f.status !== 'D' ? { label: 'Mostrar en la carpeta', icon: 'folder', action: () => safely(() => call(api.reveal(st.path, f.path))) } : null,
        ]);
      },
    }, statusBadge(f.status), fileName(f)));
  }
  if (!d.files.length) files.appendChild(h('div', { class: 'panel-empty' }, 'Este commit no modifica archivos.'));
  put(clear(left), meta, h('div', { class: 'file-list-head' }, `Archivos · ${d.files.length}`), files);
  const expectedKey = st.detailFile ? `${st.path}|${d.hash}|${st.detailFile.path}` : null;
  if (st.detailDiff && st.detailDiff.key === expectedKey) {
    DiffView.render(diffHost, st.detailDiff);
  } else if (!st.detailFile) {
    DiffView.placeholder(diffHost, 'file', 'Sin archivos');
  } else if (diffHost._key !== expectedKey) {
    DiffView.placeholder(diffHost, 'file', 'Cargando diff…');
  }
}

// ---------------------------------------------------------------- render: changes

const PANEL_TITLES = { conflicted: 'Conflictos', staged: 'Preparados', unstaged: 'Sin preparar' };

function pruneSelection(st) {
  const s = st.status;
  if (!s) return;
  const sel = st.sel;
  if (sel.list) {
    const list = s[sel.list];
    const exists = new Set(list.map((f) => f.path));
    for (const p of [...sel.paths]) if (!exists.has(p)) sel.paths.delete(p);
    if (sel.focus && !exists.has(sel.focus)) sel.focus = null;
    if (!sel.focus && sel.paths.size) sel.focus = [...sel.paths][0];
  }
  if (!sel.focus) {
    // follow a file that moved between lists (e.g. after staging), else pick the first file
    const order = ['conflicted', 'unstaged', 'staged'];
    const prev = st.lastFocus;
    let pick = null;
    if (prev) for (const k of order) if (!pick && s[k].some((f) => f.path === prev)) pick = { list: k, path: prev };
    if (!pick) for (const k of order) if (!pick && s[k].length) pick = { list: k, path: s[k][0].path };
    st.sel = pick ? { list: pick.list, paths: new Set([pick.path]), focus: pick.path, anchor: pick.path } : { list: null, paths: new Set(), focus: null, anchor: null };
  }
  st.lastFocus = st.sel.focus;
}

function renderChanges() {
  const st = cur();
  if (!st || st.view !== 'changes') return;
  const s = st.status;
  if (!s) return;
  for (const kind of ['conflicted', 'staged', 'unstaged']) renderFilePanel(st, kind, s[kind]);
  updateCommitControls(st);
}

function renderFilePanel(st, kind, files) {
  const panel = $(`#${kind}-panel`);
  if (kind === 'conflicted') panel.classList.toggle('hidden', !files.length);
  const prevBody = panel.querySelector('.panel-body');
  const prevScroll = prevBody ? prevBody.scrollTop : 0;
  const hadFocus = prevBody && prevBody === document.activeElement;
  const busy = !!S.busy;
  const btns = [];
  if (kind === 'staged') {
    btns.push(h('button', { class: 'btn small', disabled: busy || !files.length, onClick: () => unstageAll() }, icon('minus', 13), 'Quitar todo'));
  } else if (kind === 'unstaged') {
    btns.push(h('button', { class: 'btn small', disabled: busy || !files.length, onClick: () => stageAll() }, icon('plus', 13), 'Preparar todo'));
  } else {
    btns.push(h('span', { class: 'muted small' }, 'Doble clic: abrir'));
  }
  const head = h('div', { class: 'panel-head' },
    h('div', { class: 'title' }, kind === 'conflicted' ? icon('alert', 14) : null, PANEL_TITLES[kind], h('span', { class: 'count' }, String(files.length))),
    ...btns);
  const body = h('div', { class: 'panel-body', tabindex: '0', onKeydown: (e) => onFileKey(e, kind, files) });
  if (!files.length) {
    body.appendChild(h('div', { class: 'panel-empty' }, kind === 'staged' ? 'Nada preparado. Marca archivos de abajo para incluirlos en el commit.' : 'No hay cambios en el directorio de trabajo.'));
  }
  for (const f of files) body.appendChild(fileRow(st, kind, f, files));
  put(clear(panel), head, body);
  body.scrollTop = prevScroll;
  if (hadFocus) body.focus();
}

function fileRow(st, kind, f, files) {
  const selected = st.sel.list === kind && st.sel.paths.has(f.path);
  const focused = selected && st.sel.focus === f.path;
  const cb = kind === 'conflicted' ? null : h('input', {
    type: 'checkbox',
    checked: kind === 'staged',
    title: kind === 'staged' ? 'Quitar del área de preparación' : 'Preparar para el commit',
    onClick: (e) => {
      e.stopPropagation();
      const targets = selected && st.sel.paths.size > 1 ? files.filter((x) => st.sel.paths.has(x.path)) : [f];
      if (kind === 'staged') unstageFiles(targets);
      else stageFiles(targets);
    },
  });
  return h('div', {
    class: `frow${selected ? ' selected' : ''}${focused ? ' focused' : ''}`,
    title: f.orig ? `${f.orig} → ${f.path}` : f.path,
    onClick: (e) => onFileClick(st, kind, f, files, e),
    onDblclick: (e) => {
      if (e.target.tagName === 'INPUT') return;
      if (kind === 'staged') unstageFiles([f]);
      else if (kind === 'unstaged') stageFiles([f]);
      else safely(() => call(api.openPath(st.path, f.path)));
    },
    onContextmenu: (e) => {
      e.preventDefault();
      if (!selected) onFileClick(st, kind, f, files, {});
      fileMenu(e, kind, files.filter((x) => st.sel.paths.has(x.path)));
    },
  }, cb, statusBadge(f.status), fileName(f));
}

function onFileClick(st, kind, f, files, e) {
  if (st.sel.list !== kind) st.sel = { list: kind, paths: new Set(), focus: null, anchor: null };
  const sel = st.sel;
  if (e.ctrlKey || e.metaKey) {
    if (sel.paths.has(f.path) && sel.paths.size > 1) sel.paths.delete(f.path);
    else sel.paths.add(f.path);
    sel.anchor = f.path;
  } else if (e.shiftKey && sel.anchor) {
    const a = files.findIndex((x) => x.path === sel.anchor);
    const b = files.findIndex((x) => x.path === f.path);
    sel.paths = new Set(files.slice(Math.min(a, b), Math.max(a, b) + 1).map((x) => x.path));
  } else {
    sel.paths = new Set([f.path]);
    sel.anchor = f.path;
  }
  sel.focus = f.path;
  st.lastFocus = f.path;
  renderChanges();
  loadChangeDiff(st);
}

function onFileKey(e, kind, files) {
  const st = cur();
  if (!files.length || st.sel.list !== kind) return;
  const idx = files.findIndex((f) => f.path === st.sel.focus);
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    const next = files[Math.max(0, Math.min(files.length - 1, idx + (e.key === 'ArrowDown' ? 1 : -1)))];
    onFileClick(st, kind, next, files, { shiftKey: e.shiftKey });
    const row = $(`#${kind}-panel .frow.focused`);
    if (row) row.scrollIntoView({ block: 'nearest' });
  } else if (e.key === ' ' || e.key === 'Enter') {
    e.preventDefault();
    const targets = files.filter((f) => st.sel.paths.has(f.path));
    if (kind === 'staged') unstageFiles(targets);
    else if (kind === 'unstaged') stageFiles(targets);
  } else if (e.key === 'Delete' && kind === 'unstaged') {
    e.preventDefault();
    discardFiles(files.filter((f) => st.sel.paths.has(f.path)));
  } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
    e.preventDefault();
    st.sel.paths = new Set(files.map((f) => f.path));
    renderChanges();
  }
}

async function loadChangeDiff(st) {
  const host = $('#changes-diff');
  const s = st.status;
  const kind = st.sel.list;
  const f = kind && st.sel.focus ? s[kind].find((x) => x.path === st.sel.focus) : null;
  if (!f) {
    if (changeCount(s) === 0) DiffView.placeholder(host, 'checkCircle', 'Todo limpio', 'No hay cambios pendientes en el directorio de trabajo.');
    else DiffView.placeholder(host, 'file', 'Selecciona un archivo para ver sus cambios');
    return;
  }
  const token = ++st.diffToken;
  const opts = {
    key: `${st.path}|${kind}|${f.path}`,
    title: f.path,
    titleExtra: f.orig ? h('span', { class: 'dir' }, `  ← ${f.orig}`) : null,
    mode: kind === 'staged' ? 'staged' : 'unstaged',
    actions: changeFileActions(st, kind, f),
    actionsSig: `${kind}${f.status}${S.busy ? 1 : 0}`,
  };
  try {
    if (kind === 'conflicted') {
      try {
        const file = await g('readWorkingFile', st.path, f.path);
        if (file.binary) opts.untracked = file;
        else opts.conflict = file.text;
        opts.note = `Conflicto (${f.status}). Edita el archivo, o elige una versión entera con los botones de arriba.`;
      } catch {
        opts.conflict = '';
        opts.note = 'El archivo no existe en el directorio de trabajo (una de las ramas lo eliminó). Elige una versión o márcalo como resuelto.';
      }
    } else if (f.status === '?') {
      opts.untracked = await g('readWorkingFile', st.path, f.path);
    } else {
      Object.assign(opts, await g('workingDiff', st.path, f, kind === 'staged'));
      opts.onPatch = (patch, flags) => op('Aplicar cambios parciales', () => g('applyPatch', st.path, patch, flags));
    }
    if (token !== st.diffToken || S.active !== st.path || st.view !== 'changes') return;
    DiffView.render(host, opts);
  } catch (e) {
    if (token === st.diffToken && S.active === st.path) DiffView.placeholder(host, 'alert', 'No se pudo cargar el diff', e.message);
  }
}

function changeFileActions(st, kind, f) {
  const busy = !!S.busy;
  const canOpen = f.status !== 'D';
  const out = [];
  if (canOpen) {
    out.push(h('button', { class: 'icon-btn', title: 'Abrir archivo', onClick: () => safely(() => call(api.openPath(st.path, f.path))) }, icon('external', 15)));
    out.push(h('button', { class: 'icon-btn', title: 'Mostrar en la carpeta', onClick: () => safely(() => call(api.reveal(st.path, f.path))) }, icon('folder', 15)));
  }
  if (kind === 'unstaged') {
    out.push(h('button', { class: 'btn small', disabled: busy, onClick: () => discardFiles([f]) }, icon(f.status === '?' ? 'trash' : 'discard', 13), f.status === '?' ? 'Eliminar' : 'Descartar archivo'));
    out.push(h('button', { class: 'btn small primary', disabled: busy, onClick: () => stageFiles([f]) }, icon('plus', 13), 'Preparar archivo'));
  } else if (kind === 'staged') {
    out.push(h('button', { class: 'btn small', disabled: busy, onClick: () => unstageFiles([f]) }, icon('minus', 13), 'Quitar archivo'));
  } else {
    out.push(h('button', { class: 'btn small', disabled: busy, title: 'Quedarse con la versión de la rama actual (ours)', onClick: () => resolveConflict(f, 'ours') }, 'Usar la mía'));
    out.push(h('button', { class: 'btn small', disabled: busy, title: 'Quedarse con la versión entrante (theirs)', onClick: () => resolveConflict(f, 'theirs') }, 'Usar la suya'));
    out.push(h('button', { class: 'btn small primary', disabled: busy, onClick: () => markResolved([f]) }, icon('check', 13), 'Marcar resuelto'));
  }
  return out;
}

function fileMenu(e, kind, files) {
  const st = cur();
  if (!files.length) return;
  const one = files.length === 1 ? files[0] : null;
  const untracked = files.filter((f) => f.status === '?');
  const items = [{ header: one ? one.path : `${files.length} archivos seleccionados` }];
  if (kind === 'unstaged') {
    items.push({ label: 'Preparar', icon: 'plus', action: () => stageFiles(files) });
    items.push({ label: untracked.length === files.length ? 'Eliminar (a la papelera)' : 'Descartar cambios', icon: 'discard', danger: true, action: () => discardFiles(files) });
    if (untracked.length) {
      items.push('sep');
      items.push({ label: untracked.length > 1 ? 'Ignorar archivos (.gitignore)' : 'Ignorar archivo (.gitignore)', icon: 'eyeOff', action: () => ignoreFiles(untracked, false) });
      const ext = one && /\.[^./]+$/.exec(one.path);
      if (ext) items.push({ label: `Ignorar todos los *${ext[0]}`, icon: 'eyeOff', action: () => ignoreFiles(untracked, true) });
    }
  } else if (kind === 'staged') {
    items.push({ label: 'Quitar del área de preparación', icon: 'minus', action: () => unstageFiles(files) });
  } else {
    items.push({ label: 'Marcar como resuelto', icon: 'check', action: () => markResolved(files) });
    if (one) {
      items.push({ label: 'Resolver usando la mía (ours)', action: () => resolveConflict(one, 'ours') });
      items.push({ label: 'Resolver usando la suya (theirs)', action: () => resolveConflict(one, 'theirs') });
    }
  }
  items.push('sep');
  if (one && one.status !== 'D') {
    items.push({ label: 'Abrir archivo', icon: 'external', action: () => safely(() => call(api.openPath(st.path, one.path))) });
    items.push({ label: 'Mostrar en la carpeta', icon: 'folder', action: () => safely(() => call(api.reveal(st.path, one.path))) });
  }
  items.push({ label: one ? 'Copiar ruta' : 'Copiar rutas', icon: 'copy', action: () => copy(files.map((f) => f.path).join('\n'), 'Copiado') });
  showMenu(e.clientX, e.clientY, items);
}

// ---------------------------------------------------------------- commit box

function syncCommitBox(st) {
  const ta = $('#commit-message');
  ta.value = st ? st.message : '';
  $('#commit-amend').checked = !!(st && st.amend);
  $('#commit-push').checked = !!(st && st.pushAfter);
  updateCommitControls(st);
}

function maybePrefillMerge(st) {
  const s = st.status;
  if (s && s.state && s.state !== 'rebase' && s.mergeMsg && !st.message.trim() && !st.mergePrefilled) {
    st.message = s.mergeMsg;
    st.mergeMsgPrefill = s.mergeMsg;
    st.mergePrefilled = true;
    if (S.active === st.path) $('#commit-message').value = st.message;
    updateCommitControls(st);
  }
}

function updateCommitControls(st) {
  const btn = $('#commit-btn');
  const counter = $('#commit-counter');
  if (!st || !st.status) {
    btn.disabled = true;
    btn.textContent = 'Confirmar';
    counter.textContent = '';
    return;
  }
  const s = st.status;
  const staged = s.staged.length;
  const msg = st.message;
  const firstLen = (msg.split('\n')[0] || '').length;
  counter.textContent = firstLen ? `${firstLen}/72` : '';
  counter.classList.toggle('warn', firstLen > 72);
  const merging = s.state === 'merge' || s.state === 'cherry-pick' || s.state === 'revert';
  btn.textContent = st.amend ? 'Modificar último commit' : staged ? `Confirmar ${plural(staged, 'archivo', 'archivos')}` : merging ? 'Confirmar fusión' : 'Confirmar';
  btn.disabled = !!S.busy || !msg.trim() || s.conflicted.length > 0 || s.state === 'rebase' || (!st.amend && !staged && !merging);
  btn.title = s.state === 'rebase' ? 'Durante un rebase usa «Continuar» en la barra superior' : s.conflicted.length ? 'Resuelve los conflictos primero' : !msg.trim() ? 'Escribe un mensaje' : '';
}

async function doCommit() {
  const st = cur();
  if (!st || $('#commit-btn').disabled) return;
  const msg = st.message;
  const amend = st.amend;
  if (amend && st.refs && st.refs.local.some((b) => b.current && b.upstream && !b.ahead && !b.gone)) {
    const ok = await confirmDialog({
      title: 'Modificar un commit ya publicado',
      message: 'El último commit ya está en el remoto. Si lo modificas tendrás que hacer push forzado.',
      confirmLabel: 'Modificar igualmente',
      danger: true,
    });
    if (!ok) return;
  }
  const res = await op(amend ? 'Modificar commit' : 'Commit', () => g('commit', st.path, msg, { amend }), {
    success: amend ? 'Commit modificado' : 'Commit creado',
  });
  if (res === undefined) return;
  st.message = '';
  st.amend = false;
  st.mergePrefilled = true;
  if (S.active === st.path) syncCommitBox(st);
  if (st.pushAfter) {
    await refresh();
    quickPush();
  }
}

async function quickPush() {
  const st = cur();
  const b = currentLocal(st);
  if (b && b.upstream && b.upstreamRemote && !b.gone) {
    await remoteOp('Push', 'push', [st.path, { remote: b.upstreamRemote, branch: b.name, remoteBranch: b.upstreamBranch }], `Push de ${b.name} completado`);
  } else {
    openPushDialog();
  }
}

// ---------------------------------------------------------------- file operations

const withOrig = (files) => [...new Set(files.flatMap((f) => (f.orig ? [f.path, f.orig] : [f.path])))];

function stageFiles(files) {
  if (!files.length) return;
  return op('Preparar', () => g('stage', S.active, withOrig(files)));
}

function unstageFiles(files) {
  if (!files.length) return;
  return op('Quitar del área de preparación', () => g('unstage', S.active, withOrig(files)));
}

function stageAll() {
  return op('Preparar todo', () => g('stageAll', S.active));
}

function unstageAll() {
  return op('Quitar todo', () => g('unstageAll', S.active));
}

async function discardFiles(files) {
  if (!files.length) return;
  const untracked = files.filter((f) => f.status === '?');
  const tracked = files.filter((f) => f.status !== '?');
  const parts = [];
  if (tracked.length) parts.push(`se descartarán los cambios de ${plural(tracked.length, 'archivo', 'archivos')}`);
  if (untracked.length) parts.push(`${plural(untracked.length, 'archivo sin seguimiento irá', 'archivos sin seguimiento irán')} a la papelera`);
  const ok = await confirmDialog({
    title: files.length === 1 ? `Descartar «${splitPath(files[0].path).base}»` : 'Descartar cambios',
    message: `${parts.join(' y ')}.`.replace(/^./, (c) => c.toUpperCase()),
    detail: tracked.length ? 'Los cambios descartados de archivos con seguimiento no se pueden recuperar.' : null,
    confirmLabel: 'Descartar',
    danger: true,
  });
  if (!ok) return;
  await op('Descartar', () => g('discard', S.active, tracked.map((f) => f.path), untracked.map((f) => f.path)));
}

function ignoreFiles(files, byExtension) {
  return op('Ignorar', () => g('ignorePaths', S.active, files.map((f) => f.path), byExtension), { success: 'Añadido a .gitignore' });
}

function markResolved(files) {
  return op('Marcar como resuelto', () => g('stage', S.active, files.map((f) => f.path)));
}

async function resolveConflict(f, side) {
  const ok = await confirmDialog({
    title: side === 'ours' ? 'Usar mi versión' : 'Usar la versión entrante',
    message: `Se sustituirá «${f.path}» por la versión ${side === 'ours' ? 'de la rama actual (ours)' : 'entrante (theirs)'} y se marcará como resuelto.`,
    detail: cur().status.state === 'rebase' ? 'Durante un rebase, «mía» es la rama sobre la que se reaplica y «suya» el commit que se está aplicando.' : null,
    confirmLabel: 'Resolver',
  });
  if (!ok) return;
  await op('Resolver conflicto', () => g('resolveConflict', S.active, f.path, side));
}

async function abortOperation(kind) {
  const ok = await confirmDialog({
    title: 'Abortar operación',
    message: 'Se descartará la operación en curso y el repositorio volverá al estado anterior.',
    confirmLabel: 'Abortar',
    danger: true,
  });
  if (!ok) return;
  await op('Abortar', () => g('abortOperation', S.active, kind), { success: 'Operación abortada' });
}

async function continueOperation(kind) {
  const st = cur();
  if (kind === 'merge' && st.message.trim()) {
    // use the message the user may have edited in the commit box
    const res = await op('Confirmar fusión', () => g('commit', st.path, st.message, {}), { success: 'Fusión completada' });
    if (res !== undefined) {
      st.message = '';
      syncCommitBox(st);
    }
    return;
  }
  const res = await op('Continuar', () => g('continueOperation', st.path, kind));
  if (res !== undefined && !handleConflicts(res, 'La operación')) toast('Operación completada', 'success');
}

async function skipRebase() {
  const ok = await confirmDialog({ title: 'Saltar commit', message: 'Se omitirá el commit que se está aplicando y sus cambios se perderán del rebase.', confirmLabel: 'Saltar', danger: true });
  if (!ok) return;
  const res = await op('Saltar commit', () => g('skipRebase', S.active));
  handleConflicts(res, 'El rebase');
}

function copy(text, msg) {
  call(api.copy(text)).then(() => msg && toast(msg, 'success', 1500)).catch(() => {});
}

// ---------------------------------------------------------------- branch / commit operations

async function checkoutBranch(name) {
  const st = cur();
  if (st.refs.currentBranch === name) return;
  await op('Cambiar de rama', () => g('checkout', st.path, name), { success: `Ahora estás en ${name}` });
}

async function checkoutRemoteBranch(rb) {
  const st = cur();
  const existing = st.refs.local.find((b) => b.name === rb.branch);
  if (existing && existing.upstream === rb.name) {
    await checkoutBranch(existing.name);
    return;
  }
  const name = h('input', { type: 'text', value: existing ? `${rb.remote}-${rb.branch}` : rb.branch });
  await showDialog({
    title: 'Crear rama local',
    body: [
      h('p', null, `Se creará una rama local que sigue a `, h('strong', null, rb.name), ' y se cambiará a ella.'),
      existing ? h('p', { class: 'muted small' }, `Ya existe una rama local «${rb.branch}» que no sigue a ${rb.name}.`) : null,
      field('Nombre de la rama local', name),
    ],
    actions: [
      { label: 'Cancelar' },
      { label: 'Crear y cambiar', primary: true, handler: () => dialogOp('Checkout', () => g('checkoutRemote', st.path, rb.name, name.value.trim())) },
    ],
  });
}

async function checkoutCommit(hash) {
  const ok = await confirmDialog({
    title: 'Checkout de un commit',
    message: `Vas a pasar a «HEAD separado» en ${hash.slice(0, 8)}. Los commits que hagas no pertenecerán a ninguna rama salvo que crees una.`,
    confirmLabel: 'Hacer checkout',
  });
  if (!ok) return;
  await op('Checkout', () => g('checkoutDetached', S.active, hash), { success: `HEAD en ${hash.slice(0, 8)}` });
}

function sanitizeRefInput(input) {
  input.addEventListener('input', () => {
    const pos = input.selectionStart;
    const v = input.value.replace(/\s/g, '-');
    if (v !== input.value) {
      input.value = v;
      input.setSelectionRange(pos, pos);
    }
  });
  return input;
}

function openBranchDialog(startHash) {
  const st = cur();
  if (!st || !st.refs || !st.refs.head) return;
  const name = sanitizeRefInput(h('input', { type: 'text', placeholder: 'feature/mi-rama' }));
  const doCheckout = h('input', { type: 'checkbox', checked: true });
  const start = startHash || null;
  const c = start && st.commits.find((x) => x.hash === start);
  showDialog({
    title: 'Nueva rama',
    body: [
      field('Nombre', name),
      h('p', { class: 'small muted' }, 'Desde: ', h('strong', null, start ? `${start.slice(0, 8)} ${c ? `· ${c.subject}` : ''}` : `${branchLabel(st)} (HEAD)`)),
      h('label', { class: 'check' }, doCheckout, 'Cambiar a la nueva rama'),
    ],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Crear rama',
        primary: true,
        handler: () => {
          const n = name.value.trim();
          if (!n) throw new Error('Escribe un nombre para la rama.');
          if (st.refs.local.some((b) => b.name === n)) throw new Error(`Ya existe una rama llamada «${n}».`);
          return dialogOp('Crear rama', () => g('createBranch', st.path, n, start, doCheckout.checked)).then(() => toast(`Rama ${n} creada`, 'success'));
        },
      },
    ],
  });
}

function openRenameBranchDialog(b) {
  const st = cur();
  const name = sanitizeRefInput(h('input', { type: 'text', value: b.name }));
  showDialog({
    title: 'Renombrar rama',
    body: [field('Nuevo nombre', name)],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Renombrar',
        primary: true,
        handler: () => {
          const n = name.value.trim();
          if (!n || n === b.name) throw new Error('Escribe un nombre distinto.');
          return dialogOp('Renombrar rama', () => g('renameBranch', st.path, b.name, n));
        },
      },
    ],
  });
}

function openDeleteBranchDialog(b) {
  const st = cur();
  const force = h('input', { type: 'checkbox' });
  const remoteToo = h('input', { type: 'checkbox' });
  const hasRemote = b.upstream && b.upstreamRemote && !b.gone;
  showDialog({
    title: 'Eliminar rama',
    body: [
      h('p', null, '¿Eliminar la rama local ', h('strong', null, b.name), '?'),
      h('label', { class: 'check' }, force, 'Forzar aunque tenga commits sin fusionar'),
      hasRemote ? h('label', { class: 'check' }, remoteToo, `Eliminar también ${b.upstream} en el remoto`) : null,
    ],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Eliminar',
        danger: true,
        handler: async () => {
          await dialogOp('Eliminar rama', () => g('deleteBranch', st.path, b.name, force.checked));
          toast(`Rama ${b.name} eliminada`, 'success');
          return remoteToo.checked ? 'remote' : true;
        },
      },
    ],
  }).then((v) => {
    if (v === 'remote') remoteOp('Eliminar rama remota', 'deleteRemoteBranch', [st.path, b.upstreamRemote, b.upstreamBranch], `${b.upstream} eliminada`);
  });
}

async function mergeInto(ref, label) {
  const st = cur();
  const noFF = h('input', { type: 'checkbox' });
  const res = await showDialog({
    title: 'Merge',
    body: [
      h('p', null, 'Fusionar ', h('strong', null, label || ref), ' en ', h('strong', null, branchLabel(st)), '.'),
      h('label', { class: 'check' }, noFF, 'Crear siempre un commit de fusión (--no-ff)'),
    ],
    actions: [
      { label: 'Cancelar' },
      { label: 'Fusionar', primary: true, handler: () => dialogOp('Merge', () => g('merge', st.path, ref, { noFF: noFF.checked })) },
    ],
  });
  if (res && typeof res === 'object' && !handleConflicts(res, 'La fusión')) toast('Fusión completada', 'success');
}

async function rebaseOnto(ref, label) {
  const st = cur();
  const ok = await confirmDialog({
    title: 'Rebase',
    message: `Se reaplicarán los commits de «${branchLabel(st)}» encima de «${label || ref}».`,
    detail: 'No hagas rebase de commits que ya hayas publicado y compartido con otras personas.',
    confirmLabel: 'Hacer rebase',
  });
  if (!ok) return;
  const res = await op('Rebase', () => g('rebase', st.path, ref));
  if (res !== undefined && !handleConflicts(res, 'El rebase')) toast('Rebase completado', 'success');
}

function openMergeDialog() {
  const st = cur();
  const r = st.refs;
  const current = r.currentBranch;
  const options = [
    ...r.local.filter((b) => b.name !== current).map((b) => [b.name, b.name]),
    ...r.remote.map((b) => [b.name, b.name]),
  ];
  if (!options.length) {
    toast('No hay otras ramas que fusionar.', 'info');
    return;
  }
  const sel = selectEl(options, options[0][0]);
  const noFF = h('input', { type: 'checkbox' });
  showDialog({
    title: 'Merge',
    body: [
      h('p', null, 'Fusionar una rama en ', h('strong', null, branchLabel(st)), '.'),
      field('Rama a fusionar', sel),
      h('label', { class: 'check' }, noFF, 'Crear siempre un commit de fusión (--no-ff)'),
    ],
    actions: [
      { label: 'Cancelar' },
      { label: 'Fusionar', primary: true, handler: () => dialogOp('Merge', () => g('merge', st.path, sel.value, { noFF: noFF.checked })) },
    ],
  }).then((res) => {
    if (res && typeof res === 'object' && !handleConflicts(res, 'La fusión')) toast('Fusión completada', 'success');
  });
}

async function cherryPick(c) {
  const ok = await confirmDialog({ title: 'Cherry-pick', message: `Aplicar «${c.subject}» (${c.hash.slice(0, 8)}) sobre ${branchLabel(cur())}.`, confirmLabel: 'Cherry-pick' });
  if (!ok) return;
  const res = await op('Cherry-pick', () => g('cherryPick', S.active, c.hash, c.parents.length > 1));
  if (res !== undefined && !handleConflicts(res, 'El cherry-pick')) toast('Cherry-pick completado', 'success');
}

async function revertCommit(c) {
  const ok = await confirmDialog({ title: 'Revertir commit', message: `Se creará un nuevo commit que deshace «${c.subject}» (${c.hash.slice(0, 8)}).`, confirmLabel: 'Revertir' });
  if (!ok) return;
  const res = await op('Revertir', () => g('revert', S.active, c.hash, c.parents.length > 1));
  if (res !== undefined && !handleConflicts(res, 'El revert')) toast('Commit revertido', 'success');
}

function openResetDialog(c) {
  const st = cur();
  const modes = [
    ['soft', 'Soft', 'Mueve la rama; los cambios quedan preparados.'],
    ['mixed', 'Mixed (recomendado)', 'Mueve la rama; los cambios quedan en el directorio de trabajo sin preparar.'],
    ['hard', 'Hard', 'Mueve la rama y DESCARTA todos los cambios. No se puede deshacer.'],
  ];
  const radios = modes.map(([v, t, d]) => {
    const input = h('input', { type: 'radio', name: 'reset-mode', value: v, checked: v === 'mixed' });
    return { input, el: h('label', { class: 'radio-card' }, input, h('div', null, h('div', { class: 't' }, t), h('div', { class: 'd' }, d))) };
  });
  showDialog({
    title: 'Reset',
    body: [
      h('p', null, 'Mover ', h('strong', null, branchLabel(st)), ' a ', h('strong', null, `${c.hash.slice(0, 8)} · ${c.subject}`), '.'),
      h('div', { class: 'option-list' }, radios.map((r) => r.el)),
    ],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Reset',
        danger: true,
        handler: () => {
          const mode = radios.find((r) => r.input.checked).input.value;
          return dialogOp('Reset', () => g('reset', st.path, c.hash, mode)).then(() => toast(`Reset (${mode}) completado`, 'success'));
        },
      },
    ],
  });
}

function openTagDialog(hash) {
  const st = cur();
  const name = sanitizeRefInput(h('input', { type: 'text', placeholder: 'v1.0.0' }));
  const message = h('textarea', { rows: '3', placeholder: 'Opcional: si escribes un mensaje se crea una etiqueta anotada' });
  const remotes = st.refs.remotes;
  const pushIt = h('input', { type: 'checkbox' });
  const remoteSel = selectEl(remotes, remotes.includes('origin') ? 'origin' : remotes[0]);
  showDialog({
    title: 'Nueva etiqueta',
    body: [
      field('Nombre', name),
      h('p', { class: 'small muted' }, 'En el commit ', h('strong', null, hash.slice(0, 8))),
      field('Mensaje', message),
      remotes.length ? h('div', { class: 'row-fields', style: { alignItems: 'center' } }, h('label', { class: 'check' }, pushIt, 'Enviar al remoto'), remoteSel) : null,
    ],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Crear etiqueta',
        primary: true,
        handler: async () => {
          const n = name.value.trim();
          if (!n) throw new Error('Escribe un nombre.');
          if (st.refs.tags.some((t) => t.name === n)) throw new Error(`Ya existe la etiqueta «${n}».`);
          await dialogOp('Crear etiqueta', () => g('createTag', st.path, n, hash, message.value.trim()));
          return pushIt.checked ? { push: n, remote: remoteSel.value } : true;
        },
      },
    ],
  }).then((v) => {
    if (v && v.push) remoteOp('Enviar etiqueta', 'pushTag', [st.path, v.remote, v.push], `Etiqueta ${v.push} enviada`);
    else if (v) toast('Etiqueta creada', 'success');
  });
}

// ---------------------------------------------------------------- context menus

function commitMenu(e, c) {
  const st = cur();
  if (c.pseudo) {
    showMenu(e.clientX, e.clientY, [
      { label: 'Ver cambios', icon: 'changes', action: () => setView('changes') },
      { label: 'Guardar en stash…', icon: 'stash', action: () => openStashDialog(), disabled: !st.refs.head },
      { label: 'Descartar todos los cambios…', icon: 'discard', danger: true, action: () => openDiscardAllDialog() },
    ]);
    return;
  }
  const isHead = c.hash === st.refs.head;
  const onBranch = !!st.refs.currentBranch;
  const busy = !!S.busy;
  showMenu(e.clientX, e.clientY, [
    { header: `${c.hash.slice(0, 8)} · ${c.subject}` },
    { label: 'Checkout de este commit…', icon: 'commit', action: () => checkoutCommit(c.hash), disabled: busy || isHead },
    { label: 'Crear rama aquí…', icon: 'branch', action: () => openBranchDialog(c.hash), disabled: busy },
    { label: 'Crear etiqueta aquí…', icon: 'tag', action: () => openTagDialog(c.hash), disabled: busy },
    'sep',
    { label: `Merge en ${branchLabel(st)}…`, icon: 'merge', action: () => mergeInto(c.hash, c.hash.slice(0, 8)), disabled: busy || isHead },
    { label: `Rebase de ${branchLabel(st)} sobre este commit…`, icon: 'rebase', action: () => rebaseOnto(c.hash, c.hash.slice(0, 8)), disabled: busy || isHead || !onBranch },
    { label: 'Cherry-pick…', icon: 'cherry', action: () => cherryPick(c), disabled: busy || isHead },
    { label: 'Revertir commit…', icon: 'revert', action: () => revertCommit(c), disabled: busy },
    { label: `Reset de ${branchLabel(st)} a este commit…`, icon: 'reset', action: () => openResetDialog(c), disabled: busy || isHead },
    'sep',
    { label: 'Copiar hash', icon: 'copy', action: () => copy(c.hash, 'Hash copiado') },
    { label: 'Copiar mensaje', icon: 'copy', action: () => copy(c.subject, 'Mensaje copiado') },
  ]);
}

function localBranchMenu(e, b) {
  const st = cur();
  const busy = !!S.busy;
  const remotes = st.refs.remotes.length > 0;
  showMenu(e.clientX, e.clientY, [
    { header: b.name },
    { label: 'Cambiar a esta rama', icon: 'check', action: () => checkoutBranch(b.name), disabled: busy || b.current },
    { label: `Merge en ${branchLabel(st)}…`, icon: 'merge', action: () => mergeInto(b.name), disabled: busy || b.current },
    { label: `Rebase de ${branchLabel(st)} sobre ${b.name}…`, icon: 'rebase', action: () => rebaseOnto(b.name), disabled: busy || b.current || !st.refs.currentBranch },
    'sep',
    { label: 'Push…', icon: 'push', action: () => openPushDialog(b.name), disabled: busy || !remotes },
    b.current ? { label: 'Pull…', icon: 'pull', action: () => openPullDialog(), disabled: busy || !remotes } : null,
    { label: 'Nueva rama desde aquí…', icon: 'branch', action: () => openBranchDialog(b.hash), disabled: busy },
    { label: 'Renombrar…', icon: 'pencil', action: () => openRenameBranchDialog(b), disabled: busy },
    { label: 'Eliminar…', icon: 'trash', danger: true, action: () => openDeleteBranchDialog(b), disabled: busy || b.current },
    'sep',
    { label: 'Copiar nombre', icon: 'copy', action: () => copy(b.name, 'Nombre copiado') },
  ]);
}

function remoteBranchMenu(e, rb) {
  const st = cur();
  const busy = !!S.busy;
  showMenu(e.clientX, e.clientY, [
    { header: rb.name },
    { label: 'Checkout (crear rama local)…', icon: 'check', action: () => checkoutRemoteBranch(rb), disabled: busy },
    { label: `Merge en ${branchLabel(st)}…`, icon: 'merge', action: () => mergeInto(rb.name), disabled: busy },
    { label: `Rebase de ${branchLabel(st)} sobre ${rb.name}…`, icon: 'rebase', action: () => rebaseOnto(rb.name), disabled: busy || !st.refs.currentBranch },
    { label: 'Nueva rama desde aquí…', icon: 'branch', action: () => openBranchDialog(rb.hash), disabled: busy },
    'sep',
    {
      label: 'Eliminar del remoto…',
      icon: 'trash',
      danger: true,
      disabled: busy,
      action: async () => {
        const ok = await confirmDialog({ title: 'Eliminar rama remota', message: `¿Eliminar ${rb.branch} del remoto ${rb.remote}? Afecta a todas las personas que usan el repositorio.`, confirmLabel: 'Eliminar', danger: true });
        if (ok) remoteOp('Eliminar rama remota', 'deleteRemoteBranch', [st.path, rb.remote, rb.branch], `${rb.name} eliminada`);
      },
    },
    'sep',
    { label: 'Copiar nombre', icon: 'copy', action: () => copy(rb.name, 'Nombre copiado') },
  ]);
}

function webUrl(url) {
  let m;
  if (/^https?:\/\//i.test(url)) return url.replace(/\/\/[^@/]+@/, '//').replace(/\.git\/?$/, '');
  if ((m = /^ssh:\/\/(?:[^@/]+@)?([^:/]+)(?::\d+)?\/(.+?)(?:\.git)?\/?$/.exec(url))) return `https://${m[1]}/${m[2]}`;
  if ((m = /^(?:[^@/]+@)?([^:/]+\.[^:/]+):(?!\/)(.+?)(?:\.git)?\/?$/.exec(url))) return `https://${m[1]}/${m[2]}`;
  return null;
}

async function remoteMenu(e, rn) {
  const st = cur();
  const busy = !!S.busy;
  let url = null;
  try {
    const info = await g('remotesInfo', st.path);
    const r = info.find((x) => x.name === rn);
    url = r ? r.url : null;
  } catch { /* menu still useful */ }
  const web = url && webUrl(url);
  showMenu(e.clientX, e.clientY, [
    { header: url ? `${rn} · ${url}` : rn },
    { label: `Fetch de ${rn}`, icon: 'fetch', action: () => remoteOp(`Fetch ${rn}`, 'fetch', [st.path, { remote: rn }], `Fetch de ${rn} completado`), disabled: busy },
    web ? { label: 'Abrir en el navegador', icon: 'globe', action: () => safely(() => call(api.openExternal(web))) } : null,
    url ? { label: 'Copiar URL', icon: 'copy', action: () => copy(url, 'URL copiada') } : null,
    'sep',
    {
      label: 'Eliminar remoto…',
      icon: 'trash',
      danger: true,
      disabled: busy,
      action: async () => {
        const ok = await confirmDialog({ title: 'Eliminar remoto', message: `¿Quitar el remoto «${rn}» de este repositorio? No borra nada en el servidor.`, confirmLabel: 'Eliminar', danger: true });
        if (ok) op('Eliminar remoto', () => g('removeRemote', st.path, rn), { success: `Remoto ${rn} eliminado` });
      },
    },
  ]);
}

function tagMenu(e, t) {
  const st = cur();
  const busy = !!S.busy;
  const remotes = st.refs.remotes;
  const remote = remotes.includes('origin') ? 'origin' : remotes[0];
  showMenu(e.clientX, e.clientY, [
    { header: t.name },
    { label: 'Checkout de la etiqueta…', icon: 'commit', action: () => checkoutCommit(t.hash), disabled: busy },
    { label: `Merge en ${branchLabel(st)}…`, icon: 'merge', action: () => mergeInto(t.name), disabled: busy },
    remote ? { label: `Enviar a ${remote}`, icon: 'push', action: () => remoteOp('Enviar etiqueta', 'pushTag', [st.path, remote, t.name], `Etiqueta ${t.name} enviada`), disabled: busy } : null,
    'sep',
    {
      label: 'Eliminar…',
      icon: 'trash',
      danger: true,
      disabled: busy,
      action: () => {
        const remoteToo = h('input', { type: 'checkbox' });
        showDialog({
          title: 'Eliminar etiqueta',
          body: [h('p', null, '¿Eliminar la etiqueta ', h('strong', null, t.name), '?'), remote ? h('label', { class: 'check' }, remoteToo, `Eliminarla también de ${remote}`) : null],
          actions: [
            { label: 'Cancelar' },
            {
              label: 'Eliminar',
              danger: true,
              handler: async () => {
                await dialogOp('Eliminar etiqueta', () => g('deleteTag', st.path, t.name));
                return remoteToo.checked ? 'remote' : true;
              },
            },
          ],
        }).then((v) => {
          if (v === 'remote') remoteOp('Eliminar etiqueta remota', 'deleteRemoteTag', [st.path, remote, t.name], `Etiqueta ${t.name} eliminada del remoto`);
          else if (v) toast('Etiqueta eliminada', 'success');
        });
      },
    },
    { label: 'Copiar nombre', icon: 'copy', action: () => copy(t.name, 'Nombre copiado') },
  ]);
}

function stashMenu(e, s) {
  const busy = !!S.busy;
  showMenu(e.clientX, e.clientY, [
    { header: `${s.ref} · ${s.message}` },
    { label: 'Ver contenido', icon: 'eye', action: () => showStash(s) },
    { label: 'Aplicar', icon: 'download', action: () => applyStash(s, false), disabled: busy },
    { label: 'Aplicar y eliminar (pop)', icon: 'download', action: () => applyStash(s, true), disabled: busy },
    'sep',
    { label: 'Eliminar…', icon: 'trash', danger: true, action: () => dropStash(s), disabled: busy },
  ]);
}

async function applyStash(s, pop) {
  const res = await op(pop ? 'Pop stash' : 'Aplicar stash', () => g('stashApply', S.active, s.ref, pop));
  if (res !== undefined && !handleConflicts(res, 'Aplicar el stash')) toast(pop ? 'Stash aplicado y eliminado' : 'Stash aplicado', 'success');
}

async function dropStash(s) {
  const ok = await confirmDialog({ title: 'Eliminar stash', message: `¿Eliminar «${s.message}»? No se puede deshacer.`, confirmLabel: 'Eliminar', danger: true });
  if (ok) await op('Eliminar stash', () => g('stashDrop', S.active, s.ref), { success: 'Stash eliminado' });
}

// ---------------------------------------------------------------- toolbar dialogs

function selectEl(options, value) {
  const s = h('select');
  for (const o of options) {
    const [v, label] = Array.isArray(o) ? o : [o, o];
    s.appendChild(h('option', { value: v }, label));
  }
  if (value != null) s.value = value;
  return s;
}

let datalistSeq = 0;
function comboInput(values, value) {
  const id = `dl-${++datalistSeq}`;
  const list = h('datalist', { id }, values.map((v) => h('option', { value: v })));
  const input = h('input', { type: 'text', list: id, value: value || '' });
  return { input, el: h('div', null, input, list), setValues(vs) { clear(list); for (const v of vs) list.appendChild(h('option', { value: v })); } };
}

function defaultRemote(st, b) {
  const remotes = st.refs.remotes;
  if (b && b.upstreamRemote && remotes.includes(b.upstreamRemote)) return b.upstreamRemote;
  return remotes.includes('origin') ? 'origin' : remotes[0];
}

function openPullDialog() {
  const st = cur();
  const r = st.refs;
  const b = currentLocal(st);
  if (!r.remotes.length) return toast('Este repositorio no tiene remotos.', 'warn');
  if (!b) return toast('No estás en ninguna rama (HEAD separado). Cambia a una rama para hacer pull.', 'warn');
  const remote = selectEl(r.remotes, defaultRemote(st, b));
  const branchesOf = (rn) => r.remote.filter((x) => x.remote === rn).map((x) => x.branch);
  const pick = (rn) => {
    const list = branchesOf(rn);
    if (b.upstreamRemote === rn && b.upstreamBranch) return b.upstreamBranch;
    return list.includes(b.name) ? b.name : list[0] || '';
  };
  const branch = comboInput(branchesOf(remote.value), pick(remote.value));
  remote.addEventListener('change', () => {
    branch.setValues(branchesOf(remote.value));
    branch.input.value = pick(remote.value);
  });
  const rebase = h('input', { type: 'checkbox', checked: !!S.settings.pullRebase });
  showDialog({
    title: 'Pull',
    body: [
      h('p', null, 'Traer cambios a ', h('strong', null, b.name), '.'),
      h('div', { class: 'row-fields' }, field('Remoto', remote), field('Rama remota', branch.el)),
      h('label', { class: 'check' }, rebase, 'Hacer rebase en lugar de merge (--rebase)'),
    ],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Pull',
        primary: true,
        handler: () => {
          const br = branch.input.value.trim();
          if (!br) throw new Error('Indica la rama remota (haz fetch si no aparece ninguna).');
          S.settings.pullRebase = rebase.checked;
          persist();
          return { remote: remote.value, branch: br, rebase: rebase.checked };
        },
      },
    ],
  }).then(async (v) => {
    if (!v) return;
    const res = await remoteOp('Pull', 'pull', [st.path, v]);
    if (res !== undefined && !handleConflicts(res, 'El pull')) toast('Pull completado', 'success');
  });
}

function openPushDialog(branchName) {
  const st = cur();
  const r = st.refs;
  if (!r.remotes.length) return toast('Este repositorio no tiene remotos. Añade uno desde la barra lateral.', 'warn');
  if (!r.local.length) return toast('No hay ramas locales que enviar.', 'warn');
  const initial = r.local.find((b) => b.name === branchName) || currentLocal(st) || r.local[0];
  const local = selectEl(r.local.map((b) => b.name), initial.name);
  const remote = selectEl(r.remotes, defaultRemote(st, initial));
  const remoteBranch = comboInput([], '');
  const upstream = h('input', { type: 'checkbox' });
  const tags = h('input', { type: 'checkbox' });
  const force = h('input', { type: 'checkbox' });
  const sync = () => {
    const b = r.local.find((x) => x.name === local.value);
    remoteBranch.setValues(r.remote.filter((x) => x.remote === remote.value).map((x) => x.branch));
    remoteBranch.input.value = b.upstreamRemote === remote.value && b.upstreamBranch ? b.upstreamBranch : b.name;
    upstream.checked = !b.upstream || b.gone;
  };
  local.addEventListener('change', sync);
  remote.addEventListener('change', sync);
  sync();
  showDialog({
    title: 'Push',
    body: [
      h('div', { class: 'row-fields' }, field('Rama local', local), field('Remoto', remote)),
      field('Rama remota', remoteBranch.el),
      h('label', { class: 'check' }, upstream, 'Establecer como rama de seguimiento (upstream)'),
      h('label', { class: 'check' }, tags, 'Enviar también las etiquetas anotadas (--follow-tags)'),
      h('label', { class: 'check', style: { color: 'var(--red)' } }, force, 'Forzar push (--force-with-lease)'),
    ],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Push',
        primary: true,
        handler: async () => {
          const rb = remoteBranch.input.value.trim();
          if (!rb) throw new Error('Indica la rama remota.');
          if (force.checked) {
            const ok = await confirmDialog({ title: 'Push forzado', message: `Se sobrescribirá ${remote.value}/${rb} con tu versión local. Los commits remotos que no tengas se perderán.`, confirmLabel: 'Forzar push', danger: true });
            if (!ok) return false;
          }
          return { remote: remote.value, branch: local.value, remoteBranch: rb, setUpstream: upstream.checked, tags: tags.checked, force: force.checked };
        },
      },
    ],
  }).then((v) => {
    if (v) remoteOp('Push', 'push', [st.path, v], `Push de ${v.branch} completado`);
  });
}

function doFetch() {
  const st = cur();
  st.lastFetch = Date.now();
  return remoteOp('Fetch', 'fetch', [st.path, { prune: true }], 'Fetch completado');
}

function openStashDialog() {
  const st = cur();
  const message = h('input', { type: 'text', placeholder: 'Descripción (opcional)' });
  const untracked = h('input', { type: 'checkbox', checked: true });
  const keepIndex = h('input', { type: 'checkbox' });
  showDialog({
    title: 'Guardar cambios en stash',
    body: [
      h('p', null, 'Guarda temporalmente tus cambios y deja el directorio de trabajo limpio.'),
      field('Mensaje', message),
      h('label', { class: 'check' }, untracked, 'Incluir archivos sin seguimiento'),
      h('label', { class: 'check' }, keepIndex, 'Mantener los cambios preparados en el índice'),
    ],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Guardar stash',
        primary: true,
        handler: () => dialogOp('Stash', () => g('stashSave', st.path, { message: message.value.trim(), untracked: untracked.checked, keepIndex: keepIndex.checked })).then(() => toast('Cambios guardados en stash', 'success')),
      },
    ],
  });
}

function openDiscardAllDialog() {
  const st = cur();
  const s = st.status;
  const untrackedCount = s.unstaged.filter((f) => f.status === '?').length;
  const trackedCount = changeCount(s) - untrackedCount;
  const withUntracked = h('input', { type: 'checkbox' });
  showDialog({
    title: 'Descartar todos los cambios',
    body: [
      h('p', null, `Se descartarán los cambios de ${plural(trackedCount, 'archivo con seguimiento', 'archivos con seguimiento')} (preparados y sin preparar).`),
      untrackedCount ? h('label', { class: 'check' }, withUntracked, `Eliminar también ${plural(untrackedCount, 'archivo sin seguimiento', 'archivos sin seguimiento')}`) : null,
      h('p', { class: 'muted small' }, 'Esta acción no se puede deshacer. Si dudas, usa Stash.'),
    ],
    actions: [
      { label: 'Cancelar' },
      { label: 'Descartar todo', danger: true, handler: () => dialogOp('Descartar todo', () => g('discardAll', st.path, withUntracked.checked)).then(() => toast('Cambios descartados', 'success')) },
    ],
  });
}

function openAddRemoteDialog() {
  const st = cur();
  const name = h('input', { type: 'text', value: st.refs.remotes.includes('origin') ? '' : 'origin', placeholder: 'origin' });
  const url = h('input', { type: 'text', placeholder: 'https://github.com/usuario/repo.git' });
  showDialog({
    title: 'Añadir remoto',
    body: [field('Nombre', name), field('URL', url)],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Añadir',
        primary: true,
        handler: () => {
          const n = name.value.trim();
          const u = url.value.trim();
          if (!n || !u) throw new Error('Indica nombre y URL.');
          return dialogOp('Añadir remoto', () => g('addRemote', st.path, n, u)).then(() => toast(`Remoto ${n} añadido`, 'success'));
        },
      },
    ],
  });
}

async function openIdentityDialog() {
  const st = cur();
  if (!st) return;
  let id = { name: '', email: '', globalName: '', globalEmail: '' };
  try {
    id = await g('identity', st.path);
  } catch { /* empty form */ }
  const name = h('input', { type: 'text', value: id.name, placeholder: 'Tu nombre' });
  const email = h('input', { type: 'text', value: id.email, placeholder: 'tu@email.com' });
  const scope = selectEl([['global', 'Todos los repositorios (global)'], ['local', 'Solo este repositorio']], 'global');
  showDialog({
    title: 'Identidad de Git',
    body: [field('Nombre', name), field('Email', email), field('Aplicar a', scope)],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Guardar',
        primary: true,
        handler: () => {
          if (!name.value.trim() || !email.value.trim()) throw new Error('Nombre y email son obligatorios.');
          return g('setIdentity', st.path, { name: name.value, email: email.value, global: scope.value === 'global' }).then(() => toast('Identidad guardada', 'success'));
        },
      },
    ],
  });
}

async function openSettings() {
  const st = cur();
  const theme = selectEl([['dark', 'Oscuro'], ['light', 'Claro'], ['system', 'Según el sistema']], S.settings.theme);
  const maxCommits = h('input', { type: 'number', min: '100', max: '100000', step: '100', value: String(S.settings.maxCommits) });
  const autoFetch = selectEl([['0', 'Desactivado'], ['5', 'Cada 5 minutos'], ['10', 'Cada 10 minutos'], ['15', 'Cada 15 minutos'], ['30', 'Cada 30 minutos']], String(S.settings.autoFetchMin));
  theme.addEventListener('change', () => applyTheme(theme.value));
  const body = [
    h('div', { class: 'row-fields' }, field('Tema', theme), field('Fetch automático', autoFetch)),
    field('Commits a cargar en el historial', maxCommits, '100 – 100.000'),
  ];
  let name;
  let email;
  let scope;
  if (st) {
    let id = { name: '', email: '' };
    let remotes = [];
    try {
      [id, remotes] = await Promise.all([g('identity', st.path), g('remotesInfo', st.path)]);
    } catch { /* show defaults */ }
    name = h('input', { type: 'text', value: id.name, placeholder: 'Tu nombre' });
    email = h('input', { type: 'text', value: id.email, placeholder: 'tu@email.com' });
    scope = selectEl([['local', 'Solo este repositorio'], ['global', 'Global (todos los repositorios)']], id.name && id.name === id.globalName ? 'global' : 'local');
    body.push(h('div', { class: 'settings-section' },
      h('h3', null, `Repositorio · ${basename(st.path)}`),
      h('div', { class: 'row-fields' }, field('Nombre del autor', name), field('Email', email)),
      field('Guardar identidad en', scope),
      h('div', { class: 'field' }, h('span', null, 'Remotos'),
        remotes.length ? remotes.map((rm) => h('div', { class: 'remote-item' }, icon('cloud', 15), h('div', { class: 'grow' }, h('div', null, rm.name), h('div', { title: rm.url }, rm.url))))
          : h('div', { class: 'muted small' }, 'Sin remotos. Añádelos desde la barra lateral (+ en «Remotos»).'))));
  }
  const original = S.settings.theme;
  const res = await showDialog({
    title: 'Ajustes',
    width: 560,
    body,
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Guardar',
        primary: true,
        handler: async () => {
          const mc = Math.round(Number(maxCommits.value));
          if (!Number.isFinite(mc) || mc < 100 || mc > 100000) throw new Error('El número de commits debe estar entre 100 y 100.000.');
          if (st && name) {
            const id = await g('identity', st.path);
            if (name.value.trim() !== id.name || email.value.trim() !== id.email) {
              await g('setIdentity', st.path, { name: name.value, email: email.value, global: scope.value === 'global' });
            }
          }
          const reload = mc !== S.settings.maxCommits;
          S.settings.theme = theme.value;
          S.settings.maxCommits = mc;
          S.settings.autoFetchMin = Number(autoFetch.value);
          persist();
          return reload ? 'reload' : true;
        },
      },
    ],
  });
  if (!res) applyTheme(original);
  else {
    toast('Ajustes guardados', 'success', 1500);
    if (res === 'reload') refresh(true);
  }
}

// ---------------------------------------------------------------- home screen

function renderHome() {
  const home = $('#home');
  clear(home);
  const filter = h('input', { type: 'text', placeholder: 'Filtrar repositorios' });
  const list = h('div', { class: 'grow', style: { overflowY: 'auto' } });
  const rows = S.repos.map((r) => {
    const row = h('div', {
      class: 'repo-row',
      title: r.path,
      onClick: () => openRepo(r.path),
      onContextmenu: (e) => {
        e.preventDefault();
        showMenu(e.clientX, e.clientY, [
          { label: 'Abrir', icon: 'repo', action: () => openRepo(r.path) },
          { label: 'Mostrar en el explorador', icon: 'folder', action: () => safely(() => call(api.reveal(r.path, ''))) },
          { label: 'Copiar ruta', icon: 'copy', action: () => copy(r.path, 'Ruta copiada') },
          'sep',
          { label: 'Quitar de la lista', icon: 'x', danger: true, action: () => removeRepo(r.path) },
        ]);
      },
    },
    h('div', { class: 'repo-icon', style: { background: hashColor(r.name) } }, initials(r.name.replace(/[-_.]/g, ' '))),
    h('div', { class: 'grow' }, h('div', { class: 'repo-name' }, r.name), h('div', { class: 'repo-path' }, r.path)),
    h('button', { class: 'icon-btn', title: 'Quitar de la lista (no borra nada)', onClick: (e) => { e.stopPropagation(); removeRepo(r.path); } }, icon('x', 15)));
    row.dataset.q = `${r.name} ${r.path}`.toLowerCase();
    return row;
  });
  rows.forEach((r) => list.appendChild(r));
  if (!rows.length) {
    list.appendChild(h('div', { class: 'home-empty' }, icon('repo', 40, 1.3), h('p', null, 'Aún no has añadido repositorios.'),
      h('p', { class: 'small' }, 'Clona uno, añade una carpeta existente o arrastra una carpeta a esta ventana.')));
  }
  filter.addEventListener('input', () => {
    const q = filter.value.trim().toLowerCase();
    for (const r of rows) r.classList.toggle('hidden', !!q && !r.dataset.q.includes(q));
  });
  const card = (iconName, title, sub, onClick) => h('button', { class: 'action-card', onClick },
    h('div', { class: 'ac-icon' }, icon(iconName, 19)), h('div', null, h('div', { class: 'ac-title' }, title), h('div', { class: 'ac-sub' }, sub)));
  home.appendChild(h('div', { class: 'home-wrap' },
    h('div', { class: 'home-hero' },
      h('h1', null, 'Destiny Stone'),
      h('p', null, 'Tu cliente Git: historial con grafo, preparación por líneas, ramas, stashes y remotos en una interfaz limpia.'),
      card('download', 'Clonar repositorio', 'Desde una URL HTTPS o SSH', () => openCloneDialog()),
      card('folder', 'Añadir repositorio existente', 'Una carpeta que ya usa Git', () => addExisting()),
      card('folderPlus', 'Crear repositorio nuevo', 'Inicializa Git en una carpeta', () => createNew())),
    h('div', { class: 'home-list' },
      h('div', { class: 'home-list-head' }, h('h2', null, `Repositorios · ${S.repos.length}`),
        h('div', { class: 'search' }, h('span', { class: 'search-icon' }, icon('search', 14)), filter)),
      list)));
}

function removeRepo(p) {
  S.repos = S.repos.filter((r) => r.path !== p);
  if (S.tabs.includes(p)) closeTab(p);
  persist();
  if (!S.active) renderHome();
}

async function addExisting() {
  const dir = await call(api.pickFolder('Selecciona la carpeta del repositorio')).catch(() => null);
  if (dir) await openRepo(dir);
}

async function createNew() {
  const dir = await call(api.pickFolder('Selecciona o crea la carpeta del nuevo repositorio')).catch(() => null);
  if (!dir) return;
  try {
    const existing = await g('resolveRepo', dir).catch(() => null);
    if (existing && samePath(existing, dir)) {
      toast('Esa carpeta ya es un repositorio; se ha abierto.', 'info');
      await openRepo(existing);
      return;
    }
    const p = await g('init', dir);
    toast('Repositorio creado', 'success');
    addRepo(p);
    await activate(p);
    setView('changes');
  } catch (e) {
    showError('Crear repositorio', e);
  }
}

function openCloneDialog() {
  const url = h('input', { type: 'text', placeholder: 'https://github.com/usuario/proyecto.git' });
  const parent = h('input', { type: 'text', value: S.settings.cloneDir || '', placeholder: 'Carpeta donde se creará el clon' });
  const name = h('input', { type: 'text', placeholder: 'proyecto' });
  let nameTouched = false;
  name.addEventListener('input', () => { nameTouched = true; });
  url.addEventListener('input', () => {
    if (nameTouched) return;
    const m = /([^/\\:]+?)(?:\.git)?\/?$/.exec(url.value.trim());
    name.value = m ? m[1] : '';
  });
  const browse = h('button', {
    class: 'btn',
    onClick: async () => {
      const d = await call(api.pickFolder('Carpeta de destino')).catch(() => null);
      if (d) parent.value = d;
    },
  }, icon('folder', 14), 'Examinar');
  showDialog({
    title: 'Clonar repositorio',
    width: 560,
    body: [
      field('URL del repositorio', url),
      field('Carpeta de destino', h('div', { class: 'input-with-btn' }, parent, browse)),
      field('Nombre de la carpeta', name),
    ],
    actions: [
      { label: 'Cancelar' },
      {
        label: 'Clonar',
        primary: true,
        handler: () => {
          const u = url.value.trim();
          const dir = parent.value.trim();
          const n = name.value.trim();
          if (!u) throw new Error('Escribe la URL del repositorio.');
          if (!dir) throw new Error('Elige la carpeta de destino.');
          if (!n || /[\\/:*?"<>|]/.test(n)) throw new Error('El nombre de la carpeta no es válido.');
          S.settings.cloneDir = dir;
          persist();
          return { url: u, dest: joinPath(dir, n) };
        },
      },
    ],
  }).then(async (v) => {
    if (!v) return;
    const p = await remoteOp('Clonar', 'clone', [v.url, v.dest], 'Repositorio clonado');
    if (typeof p === 'string') {
      addRepo(p);
      await activate(p);
    }
  });
}

// ---------------------------------------------------------------- theme

function applyTheme(pref) {
  call(api.setTheme(pref)).then((t) => { document.documentElement.dataset.theme = t; }).catch(() => {});
}

// ---------------------------------------------------------------- startup & global events

function bindStaticEvents() {
  hist.scroll = $('#history-scroll');
  hist.rows = $('#history-rows');
  hist.spacer = $('#history-spacer');

  let raf = 0;
  hist.scroll.addEventListener('scroll', () => {
    const st = cur();
    if (st) st.historyScroll = hist.scroll.scrollTop;
    if (!raf) raf = requestAnimationFrame(() => { raf = 0; renderHistoryRows(); });
  });
  new ResizeObserver(() => renderHistoryRows()).observe(hist.scroll);

  const rowOf = (e) => {
    const el = e.target.closest('.hrow');
    const st = cur();
    return el && st ? { st, c: st.visible[Number(el.dataset.i)] } : null;
  };
  hist.rows.addEventListener('click', (e) => {
    const r = rowOf(e);
    if (r && r.c) selectCommit(r.st, r.c);
  });
  hist.rows.addEventListener('dblclick', (e) => {
    const r = rowOf(e);
    if (!r || !r.c) return;
    if (r.c.pseudo) setView('changes');
    else if (r.c.hash !== r.st.refs.head) {
      const branch = r.st.refs.local.find((b) => b.hash === r.c.hash && !b.current);
      if (branch) checkoutBranch(branch.name);
      else checkoutCommit(r.c.hash);
    }
  });
  hist.rows.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    const r = rowOf(e);
    if (!r || !r.c) return;
    if (r.st.selected !== r.c.hash) selectCommit(r.st, r.c);
    commitMenu(e, r.c);
  });
  hist.scroll.addEventListener('keydown', (e) => {
    const st = cur();
    if (!st || !st.visible.length) return;
    const idx = st.visible.findIndex((c) => c.hash === st.selected);
    const page = Math.max(1, Math.floor(hist.scroll.clientHeight / ROW_H) - 1);
    const moves = { ArrowDown: 1, ArrowUp: -1, PageDown: page, PageUp: -page };
    let next = null;
    if (e.key in moves) next = Math.max(0, Math.min(st.visible.length - 1, (idx === -1 ? 0 : idx) + moves[e.key]));
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = st.visible.length - 1;
    else if (e.key === 'Enter' && idx !== -1 && st.visible[idx].pseudo) setView('changes');
    if (next != null) {
      e.preventDefault();
      selectCommit(st, st.visible[next], { scroll: true });
    }
  });

  const filter = $('#history-filter');
  let filterTimer = null;
  filter.addEventListener('input', () => {
    clearTimeout(filterTimer);
    filterTimer = setTimeout(() => {
      const st = cur();
      if (!st) return;
      st.filter = filter.value;
      applyFilter(st);
      hist.scroll.scrollTop = 0;
      renderHistory();
    }, 120);
  });
  filter.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && filter.value) {
      e.stopPropagation();
      filter.value = '';
      filter.dispatchEvent(new Event('input'));
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      hist.scroll.focus();
    }
  });
  $('#all-branches').addEventListener('change', (e) => {
    S.settings.allBranches = e.target.checked;
    persist();
    refresh(true);
  });

  const ta = $('#commit-message');
  ta.addEventListener('input', () => {
    const st = cur();
    if (!st) return;
    st.message = ta.value;
    updateCommitControls(st);
  });
  ta.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      doCommit();
    }
  });
  $('#commit-amend').addEventListener('change', async (e) => {
    const st = cur();
    if (!st) return;
    st.amend = e.target.checked;
    if (st.amend && !st.message.trim()) {
      try {
        st.message = await g('lastCommitMessage', st.path);
        st.amendPrefill = st.message;
        ta.value = st.message;
      } catch { /* keep empty */ }
    } else if (!st.amend && st.message === st.amendPrefill) {
      st.message = '';
      ta.value = '';
    }
    updateCommitControls(st);
  });
  $('#commit-push').addEventListener('change', (e) => {
    const st = cur();
    if (st) st.pushAfter = e.target.checked;
  });
  $('#commit-btn').addEventListener('click', doCommit);

  initResizers(() => S.settings.layout, (key, size) => {
    S.settings.layout[key] = size;
    persist();
    renderHistoryRows();
  });

  document.addEventListener('keydown', (e) => {
    if (isModalOpen()) return;
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement && document.activeElement.tagName);
    const mod = e.ctrlKey || e.metaKey;
    if (e.key === 'F5') {
      e.preventDefault();
      refresh(true);
    } else if (mod && e.key.toLowerCase() === 't') {
      e.preventDefault();
      activate(null);
    } else if (mod && e.key.toLowerCase() === 'w' && S.active) {
      e.preventDefault();
      closeTab(S.active);
    } else if (mod && e.key === '1' && S.active) {
      e.preventDefault();
      setView('changes');
    } else if (mod && e.key === '2' && S.active) {
      e.preventDefault();
      setView('history');
    } else if (mod && e.key === 'Tab' && S.tabs.length > 1) {
      e.preventDefault();
      const i = S.tabs.indexOf(S.active);
      activate(S.tabs[(i + (e.shiftKey ? -1 : 1) + S.tabs.length) % S.tabs.length]);
    } else if (mod && e.key.toLowerCase() === 'f' && S.active && cur().view === 'history' && !typing) {
      e.preventDefault();
      $('#history-filter').focus();
    }
  });

  window.addEventListener('focus', () => scheduleRefresh(50));
  api.onRepoChanged((p) => {
    if (p === S.active) scheduleRefresh(100);
  });
  api.onProgress(({ opId, line }) => {
    const fn = progressHandlers.get(opId);
    if (fn) fn(line);
  });
  api.onSystemTheme((t) => {
    if (S.settings.theme === 'system') document.documentElement.dataset.theme = t;
  });

  // drag & drop a folder to add it
  const hint = $('#drop-hint');
  let dragDepth = 0;
  const hasFiles = (e) => e.dataTransfer && [...e.dataTransfer.types].includes('Files');
  window.addEventListener('dragenter', (e) => {
    if (!hasFiles(e)) return;
    dragDepth++;
    hint.classList.remove('hidden');
  });
  window.addEventListener('dragleave', () => {
    dragDepth = Math.max(0, dragDepth - 1);
    if (!dragDepth) hint.classList.add('hidden');
  });
  window.addEventListener('dragover', (e) => e.preventDefault());
  window.addEventListener('drop', (e) => {
    e.preventDefault();
    dragDepth = 0;
    hint.classList.add('hidden');
    const file = e.dataTransfer && e.dataTransfer.files[0];
    const p = file && api.pathForFile(file);
    if (p) openRepo(p);
  });

  // auto fetch
  setInterval(() => {
    const st = cur();
    const min = Number(S.settings.autoFetchMin);
    if (!st || !min || S.busy || !st.refs || !st.refs.remotes.length) return;
    if (Date.now() - st.lastFetch < min * 60000) return;
    st.lastFetch = Date.now();
    withBusy('Fetch automático', () => g('fetch', st.path, { prune: true }, null)).then(() => refresh()).catch(() => {});
  }, 30000);
}

async function init() {
  try {
    const [info, store] = await Promise.all([call(api.info()), call(api.readStore())]);
    S.platform = info.platform;
    document.body.classList.add(`platform-${info.platform}`);
    Object.assign(S.settings, store.settings || {});
    S.settings.layout = S.settings.layout || {};
    S.settings.collapsed = S.settings.collapsed || {};
    S.repos = Array.isArray(store.repos) ? store.repos.filter((r) => r && typeof r.path === 'string') : [];
    S.tabs = Array.isArray(store.tabs) ? store.tabs.filter((p) => S.repos.some((r) => r.path === p)) : [];
    S.active = S.tabs.includes(store.active) ? store.active : S.tabs[0] || null;
  } catch (e) {
    toast(`No se pudo leer la configuración: ${e.message}`, 'error');
  }
  applyTheme(S.settings.theme);
  bindStaticEvents();
  try {
    await g('version');
  } catch (e) {
    showDialog({ title: 'Git no está disponible', body: [h('div', { class: 'modal-error' }, e.message)], actions: [{ label: 'Aceptar', primary: true }] });
  }
  await activate(S.active);
}

init();
