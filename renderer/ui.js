'use strict';
/* global window, document */

// ---------------------------------------------------------------- DOM helper

function h(tag, props, ...children) {
  const el = document.createElement(tag);
  if (props) {
    for (const [k, v] of Object.entries(props)) {
      if (v == null || v === false) continue;
      if (k === 'class') el.className = v;
      else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
      else if (k === 'dataset') Object.assign(el.dataset, v);
      else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2).toLowerCase(), v);
      else if (k === 'value') el.value = v;
      else if (typeof v === 'boolean') el[k] = v;
      else el.setAttribute(k, v);
    }
  }
  append(el, children);
  return el;
}

function append(el, children) {
  for (const c of children) {
    if (c == null || c === false) continue;
    if (Array.isArray(c)) append(el, c);
    else el.appendChild(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

// Like Element.append, but skips null/false (native append would insert the text "null").
function put(el, ...children) {
  return append(el, children);
}

function clear(el) {
  while (el.firstChild) el.removeChild(el.firstChild);
  return el;
}

// ---------------------------------------------------------------- icons (stroke icons, 24x24 grid)

const ICONS = {
  commit: '<circle cx="12" cy="12" r="3"/><line x1="3" x2="9" y1="12" y2="12"/><line x1="15" x2="21" y1="12" y2="12"/>',
  pull: '<path d="M12 17V3"/><path d="m6 11 6 6 6-6"/><path d="M19 21H5"/>',
  push: '<path d="m18 9-6-6-6 6"/><path d="M12 3v14"/><path d="M5 21h14"/>',
  fetch: '<path d="M12 13v8l-4-4"/><path d="m12 21 4-4"/><path d="M4.393 15.269A7 7 0 1 1 15.71 8h1.79a4.5 4.5 0 0 1 2.436 8.284"/>',
  branch: '<line x1="6" x2="6" y1="3" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>',
  merge: '<circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M6 21V9a9 9 0 0 0 9 9"/>',
  stash: '<rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/>',
  discard: '<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"/>',
  terminal: '<polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>',
  folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  folderPlus: '<path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  settings: '<line x1="21" x2="14" y1="4" y2="4"/><line x1="10" x2="3" y1="4" y2="4"/><line x1="21" x2="12" y1="12" y2="12"/><line x1="8" x2="3" y1="12" y2="12"/><line x1="21" x2="16" y1="20" y2="20"/><line x1="12" x2="3" y1="20" y2="20"/><line x1="14" x2="14" y1="2" y2="6"/><line x1="8" x2="8" y1="10" y2="14"/><line x1="16" x2="16" y1="18" y2="22"/>',
  refresh: '<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>',
  plus: '<path d="M5 12h14"/><path d="M12 5v14"/>',
  minus: '<path d="M5 12h14"/>',
  x: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  tag: '<path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r="1" fill="currentColor"/>',
  cloud: '<path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z"/>',
  chevronRight: '<path d="m9 18 6-6-6-6"/>',
  chevronDown: '<path d="m6 9 6 6 6-6"/>',
  file: '<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>',
  history: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>',
  changes: '<path d="M12 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.375 2.625a1 1 0 0 1 3 3l-9.013 9.014a2 2 0 0 1-.853.505l-2.873.84a.5.5 0 0 1-.62-.62l.84-2.873a2 2 0 0 1 .506-.852z"/>',
  copy: '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  trash: '<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2"/>',
  alert: '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
  checkCircle: '<circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/>',
  download: '<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>',
  external: '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  repo: '<path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"/>',
  cherry: '<path d="M2 17a5 5 0 0 0 10 0c0-2.76-2.5-5-5-3-2.5-2-5 .24-5 3Z"/><path d="M12 17a5 5 0 0 0 10 0c0-2.76-2.5-5-5-3-2.5-2-5 .24-5 3Z"/><path d="M7 14c3.22-2.91 4.29-8.75 5-12 1.66 2.38 4.94 9 5 12"/><path d="M22 9c-4.29 0-7.14-2.33-10-7 5.71 0 10 4.67 10 7Z"/>',
  revert: '<path d="M3 7v6h6"/><path d="M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13"/>',
  reset: '<path d="M2.5 2v6h6M2.66 15.57a10 10 0 1 0 .57-8.38"/>',
  rebase: '<circle cx="6" cy="19" r="3"/><circle cx="18" cy="5" r="3"/><path d="M12 19h4.5a3.5 3.5 0 0 0 0-7h-8a3.5 3.5 0 0 1 0-7H12"/>',
  pencil: '<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>',
  eye: '<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>',
  eyeOff: '<path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/><path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/><path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/><path d="m2 2 20 20"/>',
  play: '<polygon points="6 3 20 12 6 21 6 3"/>',
  skip: '<polygon points="5 4 15 12 5 20 5 4"/><line x1="19" x2="19" y1="5" y2="19"/>',
  globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>',
  arrowUp: '<path d="m5 12 7-7 7 7"/><path d="M12 19V5"/>',
  arrowDown: '<path d="M12 5v14"/><path d="m19 12-7 7-7-7"/>',
  user: '<path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/>',
  home: '<path d="M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8"/><path d="M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
};

function icon(name, size = 16, strokeWidth = 1.9) {
  const span = document.createElement('span');
  span.className = 'icon';
  span.innerHTML = `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round">${ICONS[name] || ''}</svg>`;
  return span;
}

// ---------------------------------------------------------------- formatting

const rtf = new Intl.RelativeTimeFormat('es', { numeric: 'auto' });
const dtf = new Intl.DateTimeFormat('es', { dateStyle: 'medium', timeStyle: 'short' });
const shortDtf = new Intl.DateTimeFormat('es', { day: 'numeric', month: 'short', year: 'numeric' });

function relTime(unix) {
  const diff = unix - Date.now() / 1000;
  const abs = Math.abs(diff);
  if (abs < 60) return 'ahora';
  if (abs < 3600) return rtf.format(Math.round(diff / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), 'hour');
  if (abs < 86400 * 7) return rtf.format(Math.round(diff / 86400), 'day');
  return shortDtf.format(new Date(unix * 1000));
}

function fullTime(unix) {
  return dtf.format(new Date(unix * 1000));
}

function hashColor(str) {
  let x = 0;
  for (let i = 0; i < str.length; i++) x = (x * 31 + str.charCodeAt(i)) | 0;
  const hue = Math.abs(x) % 360;
  return `hsl(${hue} 55% 48%)`;
}

function initials(name) {
  const parts = String(name || '?').trim().split(/\s+/).filter(Boolean);
  return ((parts[0] || '?')[0] + (parts.length > 1 ? parts[parts.length - 1][0] : '')).toUpperCase();
}

function splitPath(p) {
  const i = p.lastIndexOf('/');
  return i === -1 ? { dir: '', base: p } : { dir: p.slice(0, i + 1), base: p.slice(i + 1) };
}

function fmtBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function plural(n, one, many) {
  return `${n} ${n === 1 ? one : many}`;
}

// ---------------------------------------------------------------- toasts

function toast(message, type = 'info', timeout) {
  let root = document.getElementById('toasts');
  if (!root) {
    root = h('div', { id: 'toasts' });
    document.body.appendChild(root);
  }
  const iconName = { success: 'checkCircle', error: 'alert', warn: 'alert', info: 'info' }[type] || 'info';
  const el = h('div', { class: `toast ${type}` }, icon(iconName, 17), h('div', { class: 'msg grow' }, message),
    h('button', { class: 'icon-btn', title: 'Cerrar', onClick: () => dismiss() }, icon('x', 14)));
  root.appendChild(el);
  let timer = null;
  const dismiss = () => {
    clearTimeout(timer);
    el.classList.add('leaving');
    setTimeout(() => el.remove(), 200);
  };
  const ms = timeout != null ? timeout : type === 'error' ? 9000 : 3800;
  const arm = () => {
    if (ms > 0) timer = setTimeout(dismiss, ms);
  };
  el.addEventListener('mouseenter', () => clearTimeout(timer));
  el.addEventListener('mouseleave', arm);
  arm();
  return dismiss;
}

// ---------------------------------------------------------------- modal dialogs

let modalDepth = 0;

function isModalOpen() {
  return modalDepth > 0;
}

// actions: [{ label, value, primary, danger, handler }]. A handler may be async; if it throws, the
// error is shown inside the dialog and it stays open. Its return value (if not undefined) resolves the dialog.
function showDialog({ title, body, actions = [], width, dismissable = true, onOpen }) {
  return new Promise((resolve) => {
    const errBox = h('div', { class: 'modal-error hidden' });
    const footer = h('div', { class: 'modal-footer' });
    const bodyEl = h('div', { class: 'modal-body' }, body, errBox);
    const closeBtn = dismissable ? h('button', { class: 'icon-btn', title: 'Cerrar (Esc)', onClick: () => !busy && close(null) }, icon('x', 16)) : null;
    const modal = h('div', { class: 'modal', style: width ? { width: `${width}px` } : null, role: 'dialog' },
      h('div', { class: 'modal-header' }, h('div', { class: 'grow' }, title), closeBtn), bodyEl, footer);
    const overlay = h('div', { class: 'modal-overlay' }, modal);
    let busy = false;
    let closed = false;
    const buttons = [];

    const close = (value) => {
      if (closed) return;
      closed = true;
      modalDepth--;
      document.removeEventListener('keydown', onKey, true);
      overlay.remove();
      resolve(value);
    };
    const setBusy = (v, btn) => {
      busy = v;
      for (const b of buttons) b.disabled = v;
      if (closeBtn) closeBtn.disabled = v;
      if (btn) btn.classList.toggle('loading', v);
    };
    const showError = (msg) => {
      errBox.textContent = msg;
      errBox.classList.toggle('hidden', !msg);
      if (msg) errBox.scrollIntoView({ block: 'nearest' });
    };

    for (const a of actions) {
      const btn = h('button', { class: `btn ${a.primary ? 'primary' : a.danger ? 'danger' : ''}` }, a.label);
      btn.addEventListener('click', async () => {
        if (busy) return;
        if (!a.handler) {
          close(a.value === undefined ? null : a.value);
          return;
        }
        showError('');
        setBusy(true, btn);
        try {
          const r = await a.handler({ showError, close });
          setBusy(false, btn);
          if (r !== false && !closed) close(r === undefined ? (a.value === undefined ? true : a.value) : r);
        } catch (e) {
          setBusy(false, btn);
          showError((e && e.message) || String(e));
        }
      });
      if (a.primary || a.danger) btn.dataset.default = '1';
      buttons.push(btn);
      footer.appendChild(btn);
    }

    const onKey = (e) => {
      if (overlay !== document.querySelector('#overlay-root > .modal-overlay:last-child')) return;
      if (e.key === 'Escape' && dismissable && !busy) {
        e.preventDefault();
        e.stopPropagation();
        close(null);
      } else if (e.key === 'Enter' && !e.shiftKey && e.target.tagName !== 'TEXTAREA' && e.target.tagName !== 'BUTTON') {
        const def = buttons.find((b) => b.dataset.default);
        if (def && !def.disabled) {
          e.preventDefault();
          def.click();
        }
      }
    };
    document.addEventListener('keydown', onKey, true);
    if (dismissable) {
      let downOnOverlay = false;
      overlay.addEventListener('mousedown', (e) => { downOnOverlay = e.target === overlay; });
      overlay.addEventListener('click', (e) => {
        if (e.target === overlay && downOnOverlay && !busy) close(null);
      });
    }
    modalDepth++;
    document.getElementById('overlay-root').appendChild(overlay);
    const focusable = modal.querySelector('input:not([type=checkbox]):not([type=radio]), textarea, select');
    if (focusable) {
      focusable.focus();
      if (focusable.select && focusable.tagName === 'INPUT') focusable.select();
    } else {
      const def = buttons.find((b) => b.dataset.default) || buttons[buttons.length - 1];
      if (def) def.focus();
    }
    if (onOpen) onOpen({ close, showError, setBusy });
  });
}

// Non-dismissable dialog that streams git progress. Returns { line, success, fail, closed }.
function progressDialog(title, onCancel) {
  const status = h('div', { class: 'small muted ellipsis' }, 'Iniciando…');
  const bar = h('div', { class: 'progress-bar' });
  const log = h('div', { class: 'progress-log' });
  const errBox = h('div', { class: 'modal-error hidden' });
  const btn = h('button', { class: 'btn' }, 'Cancelar');
  const overlay = h('div', { class: 'modal-overlay' },
    h('div', { class: 'modal', style: { width: '560px' }, role: 'dialog' },
      h('div', { class: 'modal-header' }, h('div', { class: 'grow' }, title)),
      h('div', { class: 'modal-body' }, status, bar, log, errBox),
      h('div', { class: 'modal-footer' }, btn)));
  let running = true;
  let resolveClosed;
  const closed = new Promise((r) => { resolveClosed = r; });
  let last = null;
  const close = () => {
    if (!overlay.isConnected) return;
    modalDepth--;
    document.removeEventListener('keydown', onKey, true);
    overlay.remove();
    resolveClosed();
  };
  const onKey = (e) => {
    if (overlay !== document.querySelector('#overlay-root > .modal-overlay:last-child')) return;
    if (!running && (e.key === 'Escape' || e.key === 'Enter')) {
      e.preventDefault();
      close();
    }
  };
  btn.addEventListener('click', () => {
    if (running) {
      btn.disabled = true;
      btn.textContent = 'Cancelando…';
      onCancel();
    } else close();
  });
  document.addEventListener('keydown', onKey, true);
  modalDepth++;
  document.getElementById('overlay-root').appendChild(overlay);
  return {
    closed,
    line(text) {
      status.textContent = text;
      // progress lines ("Receiving objects:  45% (…)") update in place
      const key = text.replace(/:\s+\d+%.*$/, '');
      if (last && last.key === key && /\d+%/.test(text)) last.el.textContent = text;
      else {
        const el = h('div', null, text);
        log.appendChild(el);
        last = { key, el };
      }
      log.scrollTop = log.scrollHeight;
    },
    success() {
      running = false;
      bar.classList.add('done');
      close();
    },
    fail(msg) {
      running = false;
      bar.classList.add('failed');
      status.textContent = 'La operación no se ha completado.';
      errBox.textContent = msg;
      errBox.classList.remove('hidden');
      btn.disabled = false;
      btn.textContent = 'Cerrar';
      btn.classList.add('primary');
      btn.focus();
    },
  };
}

function confirmDialog({ title, message, confirmLabel = 'Aceptar', danger = false, detail }) {
  return showDialog({
    title,
    body: [h('p', null, message), detail ? h('p', { class: 'muted small' }, detail) : null],
    actions: [
      { label: 'Cancelar', value: false },
      { label: confirmLabel, value: true, primary: !danger, danger },
    ],
  }).then((v) => v === true);
}

function field(label, control, hint) {
  return h('label', { class: 'field' }, h('span', null, label, hint ? h('span', { class: 'hint' }, ` — ${hint}`) : null), control);
}

// ---------------------------------------------------------------- context menu

let openMenu = null;

function closeMenu() {
  if (openMenu) {
    openMenu.remove();
    openMenu = null;
  }
}

// items: [{ label, icon, action, danger, disabled }] | 'sep' | { header }
function showMenu(x, y, items) {
  closeMenu();
  const menu = h('div', { class: 'ctx', role: 'menu' });
  for (const it of items) {
    if (!it) continue;
    if (it === 'sep') {
      if (menu.lastChild && !menu.lastChild.classList.contains('ctx-sep')) menu.appendChild(h('div', { class: 'ctx-sep' }));
      continue;
    }
    if (it.header) {
      menu.appendChild(h('div', { class: 'ctx-head' }, it.header));
      continue;
    }
    const row = h('div', { class: `ctx-item${it.danger ? ' danger' : ''}${it.disabled ? ' disabled' : ''}` },
      it.icon ? icon(it.icon, 15) : h('span', { class: 'icon', style: { width: '15px' } }), h('span', { class: 'label' }, it.label));
    row.addEventListener('click', (e) => {
      e.stopPropagation();
      if (it.disabled) return;
      closeMenu();
      Promise.resolve().then(it.action);
    });
    menu.appendChild(row);
  }
  while (menu.lastChild && menu.lastChild.classList.contains('ctx-sep')) menu.lastChild.remove();
  document.body.appendChild(menu);
  const r = menu.getBoundingClientRect();
  const left = Math.max(4, Math.min(x, window.innerWidth - r.width - 6));
  const top = y + r.height > window.innerHeight - 6 ? Math.max(4, y - r.height) : y;
  menu.style.left = `${left}px`;
  menu.style.top = `${top}px`;
  openMenu = menu;
}

window.addEventListener('mousedown', (e) => {
  if (openMenu && !openMenu.contains(e.target)) closeMenu();
}, true);
window.addEventListener('blur', closeMenu);
window.addEventListener('resize', closeMenu);
window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && openMenu) {
    e.stopPropagation();
    closeMenu();
  }
}, true);
document.addEventListener('scroll', closeMenu, true);

// ---------------------------------------------------------------- resizable panes

function initResizers(getSizes, onChange) {
  for (const handle of document.querySelectorAll('.resizer')) {
    const target = document.getElementById(handle.dataset.target);
    const horizontal = handle.classList.contains('resizer-x');
    const min = Number(handle.dataset.min) || 100;
    const max = Number(handle.dataset.max) || 2000;
    const key = handle.dataset.key;
    const saved = getSizes()[key];
    if (saved) target.style[horizontal ? 'width' : 'height'] = `${saved}px`;
    handle.addEventListener('mousedown', (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      const start = horizontal ? e.clientX : e.clientY;
      const startSize = horizontal ? target.offsetWidth : target.offsetHeight;
      const parent = target.parentElement;
      const limit = (horizontal ? parent.clientWidth : parent.clientHeight) - 160;
      handle.classList.add('dragging');
      document.body.classList.add(horizontal ? 'resizing-x' : 'resizing-y');
      const move = (ev) => {
        const delta = (horizontal ? ev.clientX : ev.clientY) - start;
        const size = Math.round(Math.max(min, Math.min(max, limit, startSize + delta)));
        target.style[horizontal ? 'width' : 'height'] = `${size}px`;
      };
      const up = () => {
        window.removeEventListener('mousemove', move);
        window.removeEventListener('mouseup', up);
        handle.classList.remove('dragging');
        document.body.classList.remove('resizing-x', 'resizing-y');
        onChange(key, horizontal ? target.offsetWidth : target.offsetHeight);
      };
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', up);
    });
  }
}
