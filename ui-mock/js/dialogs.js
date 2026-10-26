// Modal dialogs. Mirrors crates/ds-app/src/dialogs.rs: same titles, fields, texts and buttons.
'use strict';

const Dialogs = (() => {
  const S = App.S;
  let onClose = null;

  // dialogs.rs › show_dialogs (Modal frame) + buttons()
  function open(width, html, bind) {
    close();
    const root = document.createElement('div');
    root.id = 'modal-root';
    root.innerHTML = `<div class="dialog" style="width:${width}px">${html}</div>`;
    document.body.appendChild(root);
    const d = root.firstElementChild;
    bind && bind(d);
    const first = d.querySelector('input[type="text"], textarea');
    first && first.focus();
    return d;
  }
  function close() {
    const r = document.getElementById('modal-root');
    if (r) r.remove();
    if (onClose) { const f = onClose; onClose = null; f(); }
  }
  const actions = (primary, { danger = false, disabled = false } = {}) =>
    `<div class="actions"><button class="btn" data-x="cancel">Cancelar</button><button class="btn ${danger ? 'danger' : 'primary'}" data-x="ok" ${disabled ? 'disabled' : ''}>${primary}</button></div>`;
  const field = (label, input) => `<div class="field-label">${label}</div>${input}`;
  const text = (id, value = '', ph = '') => `<input type="text" id="${id}" value="${esc(value)}" placeholder="${esc(ph)}">`;
  const check = (id, label, on = false, style = '') => `<label class="check" style="${style}"><input type="checkbox" id="${id}" ${on ? 'checked' : ''}>${label}</label>`;
  const select = (id, options, value) => `<select id="${id}">${options.map((o) => `<option ${o === value ? 'selected' : ''}>${esc(o)}</option>`).join('')}</select>`;
  const err = (msg) => `<div class="error-box">${esc(msg)}</div>`;
  function wire(d, onOk) {
    d.querySelector('[data-x="cancel"]').onclick = close;
    const ok = d.querySelector('[data-x="ok"]');
    if (ok) ok.onclick = () => {
      const r = onOk(d);
      if (typeof r === 'string') {
        let e = d.querySelector('.error-box');
        if (!e) { e = document.createElement('div'); e.className = 'error-box'; d.querySelector('.actions').before(e); }
        e.textContent = r;
      } else if (r !== false) close();
    };
    d.addEventListener('keydown', (e) => { if (e.key === 'Enter' && e.target.tagName === 'INPUT') ok && ok.click(); });
  }
  const val = (d, id) => d.querySelector('#' + id).value.trim();
  const on = (d, id) => d.querySelector('#' + id).checked;

  function error(title, msg) {
    const d = open(460, `<h2>${esc(title)}</h2>${err(msg)}<div class="actions"><button class="btn" data-x="copy">Copiar</button><button class="btn primary" data-x="cancel">Aceptar</button></div>`);
    d.querySelector('[data-x="cancel"]').onclick = close;
    d.querySelector('[data-x="copy"]').onclick = () => navigator.clipboard.writeText(msg);
  }

  function confirm(title, msg, detail, label, danger, fn) {
    const d = open(460, `<h2>${esc(title)}</h2><div>${esc(msg)}</div>${detail ? `<div class="small">${esc(detail)}</div>` : ''}${actions(label, { danger })}`);
    wire(d, () => { setTimeout(fn, 0); });
  }

  // app.rs › progress_ui
  function progress(title, lines, done) {
    const d = open(560, `<div style="display:flex;align-items:center;gap:8px"><span id="pg-icon">⟳</span><h2 style="margin:0">${esc(title)}</h2></div>
      <div style="background:var(--bg);border-radius:6px;padding:8px;height:150px;overflow:auto;font:11.5px var(--mono);color:var(--text2)" id="pg-log"><div class="small">Conectando…</div></div>
      <div class="actions"><span class="small" id="pg-time">0 s</span><button class="btn" data-x="cancel">Cancelar</button></div>`);
    d.querySelector('[data-x="cancel"]').onclick = close;
    const log = d.querySelector('#pg-log');
    let i = 0;
    const t = setInterval(() => {
      if (!document.body.contains(d)) return clearInterval(t);
      if (i === 0) log.innerHTML = '';
      if (i < lines.length) {
        log.insertAdjacentHTML('beforeend', `<div>${esc(lines[i++])}</div>`);
        log.scrollTop = log.scrollHeight;
      } else {
        clearInterval(t);
        close();
        done();
      }
    }, 280);
  }

  function newBranch(start) {
    const c = start && S.commits[S.index.get(start)];
    const from = start ? `${start.slice(0, 8)} · ${c ? c.subject : ''}` : `${App.branchLabel()} (HEAD)`;
    const d = open(460, `<h2>Nueva rama</h2>${field('Nombre', text('name', '', 'feature/mi-rama'))}<div class="small">Desde: ${esc(from)}</div>${check('co', 'Cambiar a la nueva rama', true)}${actions('Crear rama')}`);
    d.querySelector('#name').oninput = (e) => { e.target.value = e.target.value.replace(/\s/g, '-'); };
    wire(d, () => {
      const n = val(d, 'name');
      if (!n) return 'Escribe un nombre para la rama.';
      if (S.refs.local.some((b) => b.name === n)) return `Ya existe una rama llamada «${n}».`;
      App.actions.createBranch(n, start, on(d, 'co'));
    });
  }

  function merge(fixed) {
    const cur = App.branchLabel();
    const options = S.refs.local.filter((b) => !b.current).map((b) => b.name).concat(S.refs.remote.map((b) => b.name));
    const body = fixed ? `<div>Fusionar ${esc(fixed)} en ${esc(cur)}.</div>` : `<div>Fusionar una rama en ${esc(cur)}.</div>${field('Rama a fusionar', select('target', options, options[0]))}`;
    const d = open(460, `<h2>Merge</h2>${body}${check('noff', 'Crear siempre un commit de fusión (--no-ff)')}${actions('Fusionar')}`);
    wire(d, () => App.op('Merge', () => {}, 'Fusión completada (simulado)'));
  }

  function stash() {
    const d = open(460, `<h2>Guardar cambios en stash</h2><div>Guarda temporalmente tus cambios y deja el directorio de trabajo limpio.</div>${field('Mensaje', text('msg', '', 'Descripción (opcional)'))}${check('ut', 'Incluir archivos sin seguimiento', true)}${check('ki', 'Mantener los cambios preparados en el índice')}${actions('Guardar stash')}`);
    wire(d, () => App.op('Stash', () => {
      S.refs.stashes.push({ reference: 'stash@{0}', hash: S.refs.head, message: `On ${S.refs.currentBranch}: ${val(d, 'msg') || 'WIP'}` });
      S.status = { state: null, staged: [], unstaged: [], conflicted: [] };
    }, 'Cambios guardados en stash'));
  }

  function discardAll() {
    const s = S.status;
    const ut = s.unstaged.filter((f) => f.status === '?').length;
    const tr = s.staged.length + s.unstaged.length + s.conflicted.length - ut;
    const d = open(460, `<h2>Descartar todos los cambios</h2><div>Se descartarán los cambios de ${plural(tr, 'archivo con seguimiento', 'archivos con seguimiento')} (preparados y sin preparar).</div>
      ${ut ? check('ut', `Eliminar también ${plural(ut, 'archivo sin seguimiento', 'archivos sin seguimiento')}`) : ''}<div class="small">Esta acción no se puede deshacer. Si dudas, usa Stash.</div>${actions('Descartar todo', { danger: true })}`);
    wire(d, () => App.op('Descartar todo', () => {
      const keep = ut && !on(d, 'ut') ? s.unstaged.filter((f) => f.status === '?') : [];
      S.status = { state: null, staged: [], unstaged: keep, conflicted: [] };
    }, 'Cambios descartados'));
  }

  function pull() {
    const b = App.currentLocal();
    if (!b) return App.toast('No estás en ninguna rama (HEAD separado). Cambia a una rama para hacer pull.', 'warn');
    const branches = S.refs.remote.filter((r) => r.remote === 'origin').map((r) => r.branch);
    const d = open(460, `<h2>Pull</h2><div>Traer cambios a ${esc(b.name)}.</div><div class="cols"><div>${field('Remoto', select('remote', S.refs.remotes, 'origin'))}</div><div>${field('Rama remota', select('branch', branches, branches.includes(b.name) ? b.name : branches[0]))}</div></div>${check('rb', 'Hacer rebase en lugar de merge (--rebase)')}${actions('Pull')}`);
    wire(d, () => { setTimeout(App.actions.pull, 0); });
  }

  function push(branch) {
    const b = S.refs.local.find((x) => x.name === branch) || App.currentLocal() || S.refs.local[0];
    const d = open(460, `<h2>Push</h2><div class="cols"><div>${field('Rama local', select('local', S.refs.local.map((x) => x.name), b.name))}</div><div>${field('Remoto', select('remote', S.refs.remotes, 'origin'))}</div></div>
      ${field('Rama remota', text('rb', b.name))}${check('up', 'Establecer como rama de seguimiento (upstream)', !b.upstream)}${check('tags', 'Enviar también las etiquetas anotadas (--follow-tags)')}${check('force', 'Forzar push (--force-with-lease)', false, 'color:var(--red)')}${actions('Push')}`);
    wire(d, () => {
      if (!val(d, 'rb')) return 'Indica la rama remota.';
      const name = val(d, 'local');
      if (on(d, 'force')) setTimeout(() => confirm('Push forzado', `Se sobrescribirá origin/${val(d, 'rb')} con tu versión local. Los commits remotos que no tengas se perderán.`, null, 'Forzar push', true, () => App.actions.push(name)), 0);
      else setTimeout(() => App.actions.push(name), 0);
    });
  }

  function tag(hash) {
    const d = open(460, `<h2>Nueva etiqueta</h2>${field('Nombre', text('name', '', 'v1.0.0'))}<div class="small">En el commit ${hash.slice(0, 8)}</div>
      ${field('Mensaje', '<textarea id="msg" rows="3" placeholder="Opcional: si escribes un mensaje se crea una etiqueta anotada"></textarea>')}${check('sign', 'Firmar la etiqueta (GPG/SSH)')}
      <div style="display:flex;gap:8px;align-items:center">${check('push', 'Enviar al remoto')}${select('remote', S.refs.remotes, 'origin').replace('<select', '<select style="width:auto"')}</div>${actions('Crear etiqueta')}`);
    wire(d, () => {
      const n = val(d, 'name');
      if (!n) return 'Escribe un nombre.';
      if (S.refs.tags.some((t) => t.name === n)) return `Ya existe la etiqueta «${n}».`;
      App.op('Crear etiqueta', () => S.refs.tags.push({ name: n, hash }), 'Etiqueta creada');
    });
  }

  function addRemote() {
    const d = open(460, `<h2>Añadir remoto</h2>${field('Nombre', text('name', '', 'origin'))}${field('URL', text('url', '', 'https://github.com/usuario/repo.git'))}${actions('Añadir')}`);
    wire(d, () => {
      const n = val(d, 'name');
      if (!n || !val(d, 'url')) return 'Indica nombre y URL.';
      App.op('Añadir remoto', () => S.refs.remotes.push(n), `Remoto ${n} añadido`);
    });
  }

  function checkoutRemote(name) {
    const rb = S.refs.remote.find((x) => x.name === name);
    const exists = S.refs.local.some((b) => b.name === rb.branch);
    const d = open(460, `<h2>Crear rama local</h2><div>Se creará una rama local que sigue a ${esc(name)} y se cambiará a ella.</div>${exists ? '<div class="small">Ya existe una rama local con ese nombre que no sigue a esta rama remota.</div>' : ''}${field('Nombre de la rama local', text('name', exists ? `${rb.remote}-${rb.branch}` : rb.branch))}${actions('Crear y cambiar')}`);
    wire(d, () => App.actions.createBranch(val(d, 'name'), rb.hash, true));
  }

  function reset(hash) {
    const c = S.commits[S.index.get(hash)];
    const modes = [['soft', 'Soft', 'Mueve la rama; los cambios quedan preparados.'], ['mixed', 'Mixed (recomendado)', 'Mueve la rama; los cambios quedan en el directorio de trabajo sin preparar.'], ['hard', 'Hard', 'Mueve la rama y DESCARTA todos los cambios. No se puede deshacer.']];
    const d = open(460, `<h2>Reset</h2><div>Mover ${esc(App.branchLabel())} a ${hash.slice(0, 8)} · ${esc(c.subject)}.</div>
      ${modes.map(([v, t, desc]) => `<label class="check" style="flex-direction:column;align-items:flex-start;gap:0"><span style="display:flex;gap:6px;align-items:center"><input type="radio" name="mode" value="${v}" ${v === 'mixed' ? 'checked' : ''} style="accent-color:var(--accent)"><b>${t}</b></span><span class="small" style="padding-left:22px">${desc}</span></label>`).join('')}${actions('Reset', { danger: true })}`);
    wire(d, () => App.op('Reset', () => {}, `Reset (${d.querySelector('input[name=mode]:checked').value}) completado (simulado)`));
  }

  // dialogs.rs › accounts_body
  function accounts() {
    const providers = [['GitHub', 'github.com'], ['GitLab', 'gitlab.com'], ['Bitbucket', 'bitbucket.org'], ['Azure DevOps', 'dev.azure.com']];
    let pv = 0;
    const render = () => {
      const list = S.accounts.length
        ? S.accounts.map((a, i) => `<div style="background:var(--panel2);border-radius:6px;padding:8px;display:flex;align-items:center;gap:8px"><span style="color:var(--accent)">${icon('user', 16)}</span><b>${esc(a.username)}</b><span class="muted">${a.provider} · ${a.host}</span><button class="btn" style="margin-left:auto" data-logout="${i}">Cerrar sesión</button></div>`).join('')
        : '<div class="muted">Todavía no has iniciado sesión en ninguna plataforma.</div>';
      return `<h2>Cuentas</h2><div>Inicia sesión una vez y Destiny Stone (y Git) usarán esa sesión para clonar, hacer push y gestionar pull requests. El inicio de sesión se hace en el navegador, con el SSO de tu organización si lo tiene.</div>
        <div class="small">Git Credential Manager 2.5.0</div>${list}<div style="height:1px;background:var(--border);margin:4px 0"></div><b>Iniciar sesión</b>
        <div style="display:flex;gap:4px">${providers.map(([p], i) => `<button class="seg${i === pv ? ' on' : ''}" data-pv="${i}">${p}</button>`).join('')}</div>
        <div style="display:flex;gap:8px;align-items:center"><span class="small">Servidor</span><input type="text" id="host" value="${providers[pv][1]}" style="width:240px"></div>
        <div class="actions" id="acc-actions"><button class="btn" data-x="cancel">Cerrar</button><button class="btn primary" data-x="login">Iniciar sesión en ${providers[pv][0]}</button></div>`;
    };
    const d = open(560, render());
    const bind = () => {
      d.querySelector('[data-x="cancel"]').onclick = close;
      d.querySelectorAll('[data-pv]').forEach((b) => (b.onclick = () => { pv = +b.dataset.pv; d.innerHTML = render(); bind(); }));
      d.querySelectorAll('[data-logout]').forEach((b) => (b.onclick = () => { S.accounts.splice(+b.dataset.logout, 1); d.innerHTML = render(); bind(); App.render(); }));
      d.querySelector('[data-x="login"]').onclick = () => {
        const host = d.querySelector('#host').value;
        d.querySelector('#acc-actions').innerHTML = `<span>⟳</span><span>Completa el inicio de sesión en el navegador (${esc(host)})…</span>`;
        setTimeout(() => {
          S.accounts.push({ provider: providers[pv][0], host, username: 'ana' });
          App.toast(`Sesión iniciada como ana en ${providers[pv][0]}`);
          d.innerHTML = render();
          bind();
          App.render();
        }, 1400);
      };
    };
    bind();
  }

  // dialogs.rs › settings_body
  function settings() {
    const s = S.settings;
    const d = open(640, `<h2>Ajustes</h2>
      <div class="cols"><div>${field('Tema', `<select id="theme"><option value="dark" ${s.theme === 'dark' ? 'selected' : ''}>Oscuro</option><option value="light" ${s.theme === 'light' ? 'selected' : ''}>Claro</option><option value="system">Según el sistema</option></select>`)}</div>
      <div>${field('Fetch automático', select('af', ['Desactivado', 'Cada 5 minutos', 'Cada 10 minutos', 'Cada 15 minutos', 'Cada 30 minutos'], 'Desactivado'))}</div></div>
      ${field('Commits a cargar en el historial (100 – 1.000.000)', text('max', String(s.maxCommits)).replace('<input', '<input style="width:140px"'))}
      ${check('chrono', 'Explorador de ramas cronológico (una columna por commit)', s.chronological)}${check('word', 'Resaltar las palabras cambiadas dentro de cada línea', s.wordDiff)}${check('signc', 'Firmar los commits por defecto (GPG/SSH)')}
      <div style="height:1px;background:var(--border);margin:4px 0"></div><b>Repositorio · ${esc(S.repo.name)}</b>
      <div class="cols"><div>${field('Nombre del autor', text('an', 'Ana', 'Tu nombre'))}</div><div>${field('Email', text('ae', 'ana@x.com', 'tu@email.com'))}</div></div>
      ${check('glob', 'Guardar la identidad para todos los repositorios (global)', true)}
      <div style="height:1px;background:var(--border);margin:4px 0"></div><b>Herramientas externas</b>
      <div class="cols"><div>${field('Herramienta de diff (diff.tool)', text('dt', '', 'vscode, bc, meld, kdiff3…'))}</div><div>${field('Herramienta de fusión (merge.tool)', text('mt', '', 'vscode, bc, meld, kdiff3…'))}</div></div>
      <div class="small">Usa los nombres que entiende git (git difftool --tool-help). Para VS Code: «vscode», configurado con difftool.vscode.cmd.</div>
      <div style="height:1px;background:var(--border);margin:4px 0"></div><b>Acciones personalizadas</b>
      <div class="small">Programas que aparecen en los menús contextuales. En los argumentos puedes usar $REPO, $SHA, $FILE y $BRANCH.</div>
      <div><button class="btn">Añadir acción</button></div>${actions('Guardar')}`);
    wire(d, () => {
      const mc = Number(val(d, 'max').replace(/\./g, ''));
      if (!(mc >= 100 && mc <= 1000000)) return 'El número de commits debe estar entre 100 y 1.000.000.';
      s.theme = d.querySelector('#theme').value === 'light' ? 'light' : 'dark';
      s.maxCommits = mc;
      s.chronological = on(d, 'chrono');
      s.wordDiff = on(d, 'word');
      App.recompute();
      App.render();
      App.toast('Ajustes guardados');
    });
  }

  // dialogs.rs › InteractiveRebase
  function interactiveRebase(base) {
    if (S.changeCount) return App.toast('Confirma o guarda en stash tus cambios antes de un rebase interactivo.', 'warn');
    const idx = S.index.get(base);
    const range = S.commits.slice(0, idx).filter((c) => c.parents.length < 2).reverse();
    const hasMerges = S.commits.slice(0, idx).some((c) => c.parents.length > 1);
    const ACTIONS = ['Mantener', 'Cambiar mensaje', 'Combinar (squash)', 'Combinar sin mensaje (fixup)', 'Parar para editar', 'Eliminar'];
    const rows = range.map((c) => ({ c, a: 0, msg: c.subject }));
    const c = S.commits[idx];
    const render = () => `<h2>Rebase interactivo</h2><div>Reescribe los commits posteriores a ${base.slice(0, 8)} · ${esc(c.subject)}: reordénalos, combínalos, cambia sus mensajes o elimínalos.</div>
      ${hasMerges ? '<div class="info-box" style="background:var(--yellow-soft)">Hay commits de fusión en el rango: el rebase los aplanará (desaparecerán los merges).</div>' : ''}
      <div class="small">Orden de aplicación: el primero es el más antiguo.</div>
      <div style="max-height:380px;overflow:auto;display:flex;flex-direction:column;gap:6px">${rows.map((r, i) => `<div class="rb-row"><div class="line">
        <button class="icon-btn" data-up="${i}" ${i ? '' : 'disabled'} title="Subir">${icon('chevronUp', 12)}</button><button class="icon-btn" data-down="${i}" ${i < rows.length - 1 ? '' : 'disabled'} title="Bajar">${icon('chevronDown', 12)}</button>
        <select data-act-i="${i}" style="width:190px">${ACTIONS.map((a, k) => `<option value="${k}" ${k === r.a ? 'selected' : ''}>${a}</option>`).join('')}</select>
        <span class="mono muted" style="font-size:11.5px">${r.c.hash.slice(0, 8)}</span><span class="${r.a === 5 ? 'drop' : ''}">${esc(r.c.subject)}</span></div>
        ${r.a === 1 ? `<textarea data-msg="${i}" rows="2">${esc(r.msg)}</textarea>` : ''}</div>`).join('')}</div>
      ${actions('Empezar rebase')}`;
    const d = open(640, render());
    const bind = () => {
      wire(d, () => {
        const first = rows.findIndex((r) => r.a !== 5);
        if (first >= 0 && (rows[first].a === 2 || rows[first].a === 3)) return `«${rows[first].c.subject}» no puede combinarse: no hay ningún commit anterior que se mantenga.`;
        if (rows.every((r) => r.a === 5)) return 'No puedes eliminar todos los commits con un rebase interactivo. Usa «Reset» en su lugar.';
        App.op('Rebase interactivo', () => {}, 'Rebase interactivo completado (simulado)');
      });
      d.querySelectorAll('[data-up]').forEach((b) => (b.onclick = () => { const i = +b.dataset.up; [rows[i - 1], rows[i]] = [rows[i], rows[i - 1]]; d.innerHTML = render(); bind(); }));
      d.querySelectorAll('[data-down]').forEach((b) => (b.onclick = () => { const i = +b.dataset.down; [rows[i + 1], rows[i]] = [rows[i], rows[i + 1]]; d.innerHTML = render(); bind(); }));
      d.querySelectorAll('[data-act-i]').forEach((s) => (s.onchange = () => { rows[+s.dataset.actI].a = +s.value; d.innerHTML = render(); bind(); }));
      d.querySelectorAll('[data-msg]').forEach((t) => (t.oninput = () => { rows[+t.dataset.msg].msg = t.value; }));
    };
    bind();
  }

  function flowInit() {
    const branches = S.refs.local.map((b) => b.name);
    const d = open(460, `<h2>Inicializar git-flow</h2><div>git-flow organiza el trabajo en ramas de features, releases y hotfixes. Es compatible con la extensión git-flow.</div>
      <div class="cols"><div>${field('Rama de producción', select('m', branches, 'main'))}</div><div>${field('Rama de desarrollo', text('dv', 'develop'))}</div></div>
      <div class="cols" style="grid-template-columns:1fr 1fr 1fr"><div>${field('Prefijo features', text('pf', 'feature/'))}</div><div>${field('Prefijo releases', text('pr', 'release/'))}</div><div>${field('Prefijo hotfixes', text('ph', 'hotfix/'))}</div></div>
      ${field('Prefijo de las etiquetas de versión', text('pv', '', 'v (opcional)'))}${actions('Inicializar')}`);
    wire(d, () => App.op('Inicializar git-flow', () => {}, 'git-flow inicializado'));
  }

  function createPr(source) {
    const src = source || S.refs.currentBranch;
    const locals = S.refs.local.map((b) => b.name);
    const targets = S.refs.remote.filter((b) => b.branch !== src).map((b) => b.branch);
    const d = open(560, `<h2>Nueva pull request</h2><div class="small">GitHub · ana/demo</div>
      <div class="cols"><div>${field('Desde (rama)', select('src', locals, src))}</div><div>${field('Hacia', select('dst', targets, targets.includes('develop') && src !== 'develop' ? 'develop' : 'main'))}</div></div>
      ${field('Título', text('title', S.commits[S.index.get(S.refs.local.find((b) => b.name === src).hash)].subject))}${field('Descripción', '<textarea id="body" rows="5"></textarea>')}${check('draft', 'Crear como borrador')}
      ${!S.refs.local.find((b) => b.name === src).upstream ? '<div class="info-box" style="background:var(--yellow-soft)">Esta rama tiene commits sin enviar o no existe en el remoto: haz push antes de crear la pull request.</div>' : ''}
      <div class="actions"><button class="btn" data-x="cancel">Cancelar</button><button class="btn" data-x="web">Abrir en el navegador</button><button class="btn primary" data-x="ok">Crear</button></div>`);
    wire(d, () => App.op('Crear pull request', () => {}, 'Pull request creada (simulado)'));
    d.querySelector('[data-x="web"]').onclick = () => { close(); App.toast('En la app real abre la página de creación en el navegador.', 'info'); };
  }

  function clone() {
    const d = open(640, `<h2>Clonar repositorio</h2>${S.accounts.length ? '<div class="small">Tus repositorios: elige una cuenta</div>' : '<div class="small">Inicia sesión desde «Cuentas» para elegir entre tus repositorios de GitHub, GitLab o Bitbucket.</div>'}
      ${field('URL del repositorio', text('url', '', 'https://github.com/usuario/proyecto.git'))}
      ${field('Carpeta de destino', `<div style="display:flex;gap:8px">${text('dir', 'C:\\Users\\Ana\\proyectos', 'Carpeta donde se creará el clon')}<button class="btn">Examinar…</button></div>`)}
      ${field('Nombre de la carpeta', text('name', '', 'proyecto'))}${actions('Clonar')}`);
    d.querySelector('#url').oninput = (e) => { d.querySelector('#name').value = (e.target.value.trim().replace(/\/$/, '').replace(/\.git$/, '').split(/[/\\:]/).pop() || ''); };
    wire(d, () => {
      if (!val(d, 'url')) return 'Escribe la URL del repositorio.';
      setTimeout(() => App.remoteOp('Clonar', ['Cloning into \'proyecto\'...', 'remote: Enumerating objects: 120, done.', 'Receiving objects: 100% (120/120), 48.20 KiB, done.', 'Resolving deltas: 100% (31/31), done.'], null, 'Repositorio clonado'), 0);
    });
  }

  return { open, close, error, confirm, progress, newBranch, merge, stash, discardAll, pull, push, tag, addRemote, checkoutRemote, reset, accounts, settings, interactiveRebase, flowInit, createPr, clone };
})();
