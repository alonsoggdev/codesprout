// Views: history (explorer / list, search, detail), changes, diff viewer, secondary views and
// home. Mirrors crates/codesprout-app/src/views/*.rs.
'use strict';

const Views = (() => {
  const S = App.S;
  let explorer = null;
  let canvas = null;

  // ---------------------------------------------------------------- diff model (codesprout-core diff.rs + repo_state.rs)
  function parseDiff(text) {
    const files = [];
    let file = null, hunk = null;
    for (const line of String(text || '').replace(/\n$/, '').split('\n')) {
      if (line.startsWith('diff --git ')) {
        file = { header: [line], hunks: [] };
        files.push(file);
        hunk = null;
        continue;
      }
      if (!file) continue;
      const m = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/.exec(line);
      if (m) {
        hunk = { oldStart: +m[1], oldCount: m[2] === undefined ? 1 : +m[2], newStart: +m[3], newCount: m[4] === undefined ? 1 : +m[4], context: m[5].trim(), lines: [] };
        file.hunks.push(hunk);
        continue;
      }
      if (!hunk) file.header.push(line);
      else hunk.lines.push(line);
    }
    return files;
  }

  // diff.rs › intraline (word granularity, skipped when the line is almost fully rewritten)
  function intraline(a, b) {
    const ta = a.split(/(\s+)/).filter((x) => x !== ''), tb = b.split(/(\s+)/).filter((x) => x !== '');
    const n = ta.length, m = tb.length;
    if (n * m > 40000) return null;
    const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
    for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = ta[i] === tb[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
    const ra = [], rb = [];
    let i = 0, j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && ta[i] === tb[j]) { ra.push([ta[i], 0]); rb.push([tb[j], 0]); i++; j++; }
      else if (j < m && (i === n || dp[i][j + 1] >= dp[i + 1][j])) rb.push([tb[j++], 1]);
      else ra.push([ta[i++], 1]);
    }
    const changed = (r) => r.filter((x) => x[1]).reduce((s, x) => s + x[0].length, 0);
    if (changed(ra) * 10 > a.length * 7 && changed(rb) * 10 > b.length * 7) return null;
    return [ra, rb];
  }
  const markup = (tokens) => tokens.map(([t, c]) => (c ? `<mark>${esc(t)}</mark>` : esc(t))).join('');

  function diffRows(text) {
    const files = parseDiff(text);
    const rows = [];
    files.forEach((f, fi) => {
      f.hunks.forEach((h, hi) => {
        rows.push({ type: 'hunk', fi, hi, h });
        const hl = {};
        if (S.settings.wordDiff) {
          for (let i = 0; i < h.lines.length;) {
            if (!h.lines[i].startsWith('-')) { i++; continue; }
            const ds = i;
            while (i < h.lines.length && h.lines[i].startsWith('-')) i++;
            const as = i;
            while (i < h.lines.length && h.lines[i].startsWith('+')) i++;
            for (let k = 0; k < Math.min(as - ds, i - as); k++) {
              const r = intraline(h.lines[ds + k].slice(1), h.lines[as + k].slice(1));
              if (r) { hl[ds + k] = r[0]; hl[as + k] = r[1]; }
            }
          }
        }
        let o = h.oldStart, nw = h.newStart;
        h.lines.forEach((l, li) => {
          const t = l[0];
          const kind = t === '+' ? 'add' : t === '-' ? 'del' : 'ctx';
          rows.push({ type: 'line', fi, hi, li, kind, old: kind === 'add' ? '' : o++, new: kind === 'del' ? '' : nw++, text: l.slice(1), sign: t === ' ' ? '' : t, hl: hl[li] });
        });
      });
    });
    if (!rows.length) rows.push({ type: 'info', text: 'No hay diferencias que mostrar.' });
    return rows;
  }

  // views/diff.rs › body
  function diffBody(model) {
    const partial = model.partial;
    const opts = model.mode === 'untracked' || model.mode === 'conflict' ? '' : `
      <label class="check"><input type="checkbox" data-act="opt-ws" ${S.settings.ignoreWhitespace ? 'checked' : ''}>Ignorar espacios</label>
      <label class="check"><input type="checkbox" data-act="opt-word" ${S.settings.wordDiff ? 'checked' : ''}>Palabras</label>
      <span class="small">Contexto</span><input type="text" class="num" value="${S.settings.context}" data-act="opt-ctx">`;
    let h = `<div class="diff-opts">${opts}${partial ? '<span class="small">Clic en líneas para seleccionarlas · Mayús+clic: rango</span>' : ''}${model.note ? `<span class="small">${esc(model.note)}</span>` : ''}</div><div class="diff-body">`;
    for (const r of model.rows) {
      if (r.type === 'info') h += `<div class="dl info">${esc(r.text)}</div>`;
      else if (r.type === 'text') {
        const bg = { marker: 'color-mix(in srgb, var(--purple) 20%, transparent)', ours: 'var(--green-soft)', theirs: 'var(--accent-soft)', base: 'var(--yellow-soft)' }[r.part] || (r.added ? 'var(--add-bg)' : 'transparent');
        h += `<div class="dl" style="background:${bg}"><span class="o">${r.no}</span><span class="sg"></span><span class="tx" style="${r.part === 'marker' ? 'color:var(--purple)' : ''}">${esc(r.text)}</span></div>`;
      } else if (r.type === 'hunk') {
        const sel = [...S.diffSel].filter((k) => k.startsWith(`${r.fi}:${r.hi}:`)).length;
        let btns = '';
        if (partial) {
          btns = model.mode === 'unstaged'
            ? `<button class="hbtn danger" data-act="hunk" data-arg="discard:${r.fi}:${r.hi}">${sel ? `Descartar líneas (${sel})` : 'Descartar bloque'}</button><button class="hbtn" data-act="hunk" data-arg="stage:${r.fi}:${r.hi}">${sel ? `Preparar líneas (${sel})` : 'Preparar bloque'}</button>`
            : `<button class="hbtn" data-act="hunk" data-arg="unstage:${r.fi}:${r.hi}">${sel ? `Quitar líneas (${sel})` : 'Quitar bloque'}</button>`;
        }
        h += `<div class="dl hunk"><span>@@ -${r.h.oldStart},${r.h.oldCount} +${r.h.newStart},${r.h.newCount} @@ ${esc(r.h.context)}</span><span class="hbtns">${btns}</span></div>`;
      } else {
        const key = `${r.fi}:${r.hi}:${r.li}`;
        const pick = partial && r.kind !== 'ctx';
        const body = r.hl ? markup(r.hl) : esc(r.text);
        h += `<div class="dl ${r.kind}${S.diffSel.has(key) ? ' sel' : ''}${pick ? ' pick' : ''}" ${pick ? `data-act="line" data-arg="${key}"` : ''}><span class="o">${r.old}</span><span class="n">${r.new}</span><span class="sg">${r.sign}</span><span class="tx">${body}</span></div>`;
      }
    }
    return h + '</div>' + imagePreview(model);
  }

  function imagePreview() {
    return '';
  }

  function diffHeader(path, orig, buttons) {
    const [dir, base] = splitPath(path);
    return `<div class="diff-header">${icon('file', 14)}<span class="base">${esc(base)}</span>${dir ? `<span class="muted">${esc(dir)}</span>` : ''}${orig ? `<span style="color:var(--purple)">← ${esc(orig)}</span>` : ''}<div class="right">${buttons}</div></div>`;
  }

  const fileLabel = (f) => {
    const [dir, base] = splitPath(f.path);
    return `${statusBadge(f.status)}<span class="fname">${esc(base)}${dir ? `<span class="dir">${esc(dir)}</span>` : ''}${f.orig ? `<span class="orig">← ${esc(f.orig)}</span>` : ''}</span>`;
  };

  // ---------------------------------------------------------------- history (views/history.rs)
  function historyHeader() {
    const s = S.settings, ex = s.historyMode === 'explorer';
    return `<div class="hist-header">
      <div class="left">
        <button class="icon-btn mode-btn${ex ? ' on' : ''}" data-act="mode" data-arg="explorer" title="Explorador de ramas">${icon('graph', 15)}</button>
        <button class="icon-btn mode-btn${ex ? '' : ' on'}" data-act="mode" data-arg="list" title="Lista de commits">${icon('list', 15)}</button>
        <div class="sep-v"></div>
        <span class="muted">${icon('search', 14)}</span>
        <input type="text" id="filter" placeholder="Filtrar: mensaje, autor, hash, rama…" style="width:220px" value="${esc(S.filter)}">
        <div class="sep-v"></div>
        <select id="search-mode" style="width:150px">${['Mensaje', 'Autor', 'Contenido (añadido/quitado)', 'Diff (expresión regular)', 'Ruta de archivo'].map((m) => `<option ${m === S.search.mode ? 'selected' : ''}>${m}</option>`).join('')}</select>
        <input type="text" id="search" placeholder="Buscar en todo el historial" style="width:200px" value="${esc(S.search.query)}">
        <button class="btn" data-act="search" ${S.search.query.trim() ? '' : 'disabled'}>Buscar</button>
      </div>
      <div class="right">
        <label class="check"><input type="checkbox" data-act="all-branches" ${s.allBranches ? 'checked' : ''}>Todas las ramas</label>
        ${ex ? `<button class="seg${s.vertical ? '' : ' on'}" data-act="orientation" data-arg="h" title="Ramas en filas, el tiempo avanza hacia la derecha">Horizontal</button>
        <button class="seg${s.vertical ? ' on' : ''}" data-act="orientation" data-arg="v" title="Ramas en columnas, lo más reciente arriba, con el mensaje de cada commit al lado">Vertical</button>
        <div class="sep-v"></div>
        ${s.vertical ? '' : `<button class="seg${s.chronological ? ' on' : ''}" data-act="chrono" title="Una columna por commit: izquierda a derecha es estrictamente el orden temporal">Cronológico</button>`}
        <button class="icon-btn" data-act="zoom" data-arg="out" title="Alejar">${icon('minus', 14)}</button>
        <button class="icon-btn" data-act="zoom" data-arg="in" title="Acercar">${icon('plus', 14)}</button>` : ''}
        <button class="icon-btn" data-act="go-head" title="Ir a HEAD">${icon('target', 14)}</button>
      </div>
    </div>`;
  }

  function listView() {
    const q = S.filter.trim().toLowerCase();
    const rows = [];
    if (S.layout.worktree !== null && !q) rows.push(`<div class="lrow${S.selected === 'WORKTREE' ? ' selected' : ''}" data-act="select" data-arg="WORKTREE"><span style="width:18px"></span><i style="color:var(--yellow)">Cambios sin confirmar (${S.changeCount})</i></div>`);
    S.commits.forEach((c, i) => {
      const refs = S.refMap.get(c.hash) || [];
      if (q && !(c.subject.toLowerCase().includes(q) || c.author.toLowerCase().includes(q) || c.hash.startsWith(q) || refs.some((r) => r.name.toLowerCase().includes(q)))) return;
      const lane = S.layout.lanes[S.layout.nodes[i].lane];
      rows.push(`<div class="lrow${S.selected === c.hash ? ' selected' : ''}" data-act="select" data-arg="${c.hash}">
        <span class="dot" style="background:var(--lane-${lane.color % 10})"></span>${refBadges(refs)}
        <span class="subject">${esc(c.subject)}</span><span class="author">${esc(c.author)}</span>
        <span class="date" title="${fullTime(c.time)}">${relTime(c.time)}</span><span class="hash">${c.hash.slice(0, 8)}</span></div>`);
    });
    return `<div class="list">${rows.join('')}</div>`;
  }

  function refBadges(refs) {
    return refs.slice(0, 3).map((r) => {
      const [color, prefix] = { tag: ['var(--yellow)', '⏷ '], remote: ['var(--purple)', '☁ '], head: ['var(--red)', ''] }[r.kind] || ['var(--accent)', r.current ? '✓ ' : ''];
      return pill(prefix + r.name, color);
    }).join('') + (refs.length > 3 ? pill(`+${refs.length - 3}`, 'var(--muted)') : '');
  }

  function searchResults() {
    if (!S.search.results) return '';
    return `<div class="results"><div style="display:flex;align-items:center"><b>Resultados · ${S.search.results.length}</b><button class="icon-btn" style="margin-left:auto" data-act="close-search">${icon('close', 12)}</button></div>
      <div style="overflow:auto;flex:1">${S.search.results.map((c) => `<div class="r" data-act="reveal" data-arg="${c.hash}"><div>${esc(c.subject)}</div><div class="small">${c.hash.slice(0, 8)} · ${esc(c.author)} · ${relTime(c.time)}</div></div>`).join('') || '<div class="muted">Ningún commit coincide.</div>'}</div></div>`;
  }

  function detail() {
    if (S.selected === 'WORKTREE') return `<div class="placeholder"><div class="t">Cambios sin confirmar</div><button class="btn primary" data-act="view-changes">Ver cambios</button></div>`;
    const c = S.commits[S.index.get(S.selected)];
    if (!c) return `<div class="placeholder">${icon('commit', 34)}<div class="t">Selecciona un commit</div><div>Haz clic en un nodo del explorador</div></div>`;
    const files = S.files[c.hash] || [];
    const f = files[S.detailFile] || null;
    const left = `<div class="detail-left">
      <div class="d-subject">${esc(c.subject)}</div>
      <div class="who"><div class="avatar" style="background:${hashColor(c.email)}">${initials(c.author)}</div>
        <div><div><b>${esc(c.author)}</b> <span class="muted">&lt;${esc(c.email)}&gt;</span></div><div class="small">${relTime(c.time)} · ${fullTime(c.time)}</div></div></div>
      <div class="kv"><span class="k">Commit</span><span><span class="link" data-act="copy" data-arg="${c.hash}">${c.hash.slice(0, 12)}</span> <button class="icon-btn" style="display:inline-grid;vertical-align:middle;width:20px;height:20px" data-act="copy" data-arg="${c.hash}" title="Copiar hash completo">${icon('copy', 12)}</button></span>
        ${c.parents.length ? `<span class="k">${c.parents.length > 1 ? 'Padres' : 'Padre'}</span><span>${c.parents.map((p) => `<span class="link" data-act="reveal" data-arg="${p}">${p.slice(0, 8)}</span>`).join(' ')}</span>` : ''}</div>
      <div class="section-label">ARCHIVOS · ${files.length}</div>
      ${files.map((x, i) => `<div class="frow${i === S.detailFile ? ' selected' : ''}" data-act="detail-file" data-arg="${i}" title="${esc(x.path)}">${fileLabel(x)}</div>`).join('') || '<div class="muted">Este commit no modifica archivos.</div>'}
    </div>`;
    let right = '<div class="placeholder">Sin archivos</div>';
    if (f) {
      const model = { mode: 'commit', partial: false, rows: diffRows(f.diff), note: c.parents.length > 1 ? 'Commit de fusión: se muestran los cambios respecto al primer padre.' : null };
      right = diffHeader(f.path, f.orig, `<button class="icon-btn" data-act="file-history" data-arg="${esc(f.path)}" title="Historial del archivo">${icon('history', 14)}</button><button class="icon-btn" data-act="blame" data-arg="${esc(f.path)}" title="Blame en este commit">${icon('user', 14)}</button><button class="icon-btn" title="Abrir en la herramienta de diff externa">${icon('external', 14)}</button>`) + diffBody(model);
    }
    return left + `<div class="detail-right">${right}</div>`;
  }

  function history(el) {
    const ex = S.settings.historyMode === 'explorer';
    el.innerHTML = `<div class="view">${historyHeader()}<div class="hist-body">
      <div class="hist-top">${ex ? '<div class="canvas-host" id="canvas-host"></div>' : listView()}${searchResults()}</div>
      <div class="splitter-h" id="detail-split"></div>
      <div class="detail" style="height:${S.settings.detailH}px">${detail()}</div></div></div>`;
    if (ex) {
      if (!canvas) {
        canvas = document.createElement('canvas');
        explorer = new Explorer(canvas, {
          getState: () => ({ ...S, filter: S.filter }),
          select: (h) => App.select(h),
          toggleCompare: (h) => { S.compareWith = S.compareWith === h ? null : h; explorer.draw(); },
          dblclick: (h) => dblclickCommit(h),
          contextMenu: (x, y, h) => commitMenu(x, y, h),
          setZoom: (z) => { S.settings.zoom = z; },
        });
        Views.explorer = explorer;
      }
      $('#canvas-host').appendChild(canvas);
      requestAnimationFrame(() => explorer.draw());
    }
    bindHistoryInputs();
    splitter($('#detail-split'), 'y', () => S.settings.detailH, (v) => { S.settings.detailH = Math.max(140, v); $('.detail').style.height = S.settings.detailH + 'px'; explorer && explorer.draw(); }, true);
  }

  function bindHistoryInputs() {
    const f = $('#filter');
    f.oninput = () => { S.filter = f.value; if (S.settings.historyMode === 'explorer') explorer.draw(); else { const pos = f.selectionStart; App.renderView(); const nf = $('#filter'); nf.focus(); nf.setSelectionRange(pos, pos); } };
    const q = $('#search');
    q.oninput = () => { S.search.query = q.value; document.querySelector('[data-act="search"]').disabled = !q.value.trim(); };
    q.onkeydown = (e) => { if (e.key === 'Enter') runSearch(); };
    $('#search-mode').onchange = (e) => { S.search.mode = e.target.value; };
  }

  function runSearch() {
    const q = S.search.query.trim().toLowerCase();
    if (!q) return;
    const m = S.search.mode;
    S.search.results = S.commits.filter((c) => (m === 'Autor' ? c.author.toLowerCase().includes(q) : m === 'Ruta de archivo' ? (S.files[c.hash] || []).some((f) => f.path.toLowerCase().includes(q)) : m === 'Mensaje' ? c.subject.toLowerCase().includes(q) : (S.files[c.hash] || []).some((f) => f.diff.toLowerCase().includes(q))));
    App.renderView();
  }

  function dblclickCommit(h) {
    if (h === 'WORKTREE') return App.setView('changes');
    if (S.refs.head === h) return;
    const b = S.refs.local.find((x) => x.hash === h && !x.current);
    if (b) App.actions.checkout(b.name);
    else Dialogs.confirm('Checkout de un commit', `Vas a pasar a «HEAD separado» en ${h.slice(0, 8)}. Los commits que hagas no pertenecerán a ninguna rama salvo que crees una.`, null, 'Hacer checkout', false, () => App.toast(`HEAD en ${h.slice(0, 8)}`));
  }

  // views/history.rs › commit_menu
  function commitMenu(x, y, h) {
    if (h === 'WORKTREE') {
      return App.menu(x, y, [
        { label: 'Ver cambios', action: () => App.setView('changes') },
        { label: 'Guardar en stash…', action: () => Dialogs.stash() },
        { label: 'Descartar todos los cambios…', action: () => Dialogs.discardAll() },
      ]);
    }
    if (!h) return App.menu(x, y, [{ label: 'Ir a HEAD', action: () => App.select(S.refs.head, true) }, { label: 'Nueva rama…', action: () => Dialogs.newBranch(null) }]);
    const c = S.commits[S.index.get(h)];
    const short = h.slice(0, 8);
    const sim = (what) => () => App.toast(`${what} (simulado)`, 'info');
    App.menu(x, y, [
      { label: 'Checkout de este commit…', action: () => dblclickCommit(h) },
      { label: 'Crear rama aquí…', action: () => Dialogs.newBranch(h) },
      { label: 'Crear etiqueta aquí…', action: () => Dialogs.tag(h) },
      '-',
      { label: 'Merge en la rama actual…', action: () => Dialogs.merge(short) },
      { label: 'Rebase de la rama actual sobre este commit…', action: sim('Rebase') },
      { label: 'Rebase interactivo desde aquí…', action: () => Dialogs.interactiveRebase(h) },
      { label: 'Cherry-pick…', action: sim('Cherry-pick') },
      { label: 'Revertir commit…', action: sim('Revert') },
      { label: 'Reset de la rama actual a este commit…', action: () => Dialogs.reset(h) },
      '-',
      { label: 'Comparar con la versión actual (HEAD)', action: () => openCompare(h, 'HEAD', short, 'HEAD') },
      { label: 'Comparar con el commit marcado (Ctrl+clic)', action: () => (S.compareWith && S.compareWith !== h ? openCompare(S.compareWith, h, S.compareWith.slice(0, 8), short) : App.toast('Marca primero otro commit con Ctrl+clic.', 'info')) },
      { label: 'Guardar como parche…', action: sim('Parche guardado') },
      { label: 'Exportar como archivo (zip)…', action: sim('Archivo exportado') },
      '-',
      { label: 'Copiar hash', action: () => navigator.clipboard.writeText(h) },
      { label: 'Copiar mensaje', action: () => navigator.clipboard.writeText(c.subject) },
    ], `${short} · ${c.subject}`);
  }

  // ---------------------------------------------------------------- changes (views/changes.rs)
  function changes(el) {
    const s = S.status;
    const panel = (kind, title, files) => {
      const btn = kind === 'staged' ? `<button class="btn small" data-act="unstage-all" ${files.length && !S.busy ? '' : 'disabled'}>Quitar todo</button>`
        : kind === 'unstaged' ? `<button class="btn small" data-act="stage-all" ${files.length && !S.busy ? '' : 'disabled'}>Preparar todo</button>` : '<span class="small">Doble clic: abrir</span>';
      const rows = files.map((f) => {
        const sel = S.sel.list === kind && S.sel.paths.has(f.path);
        const cb = kind === 'conflicted' ? '' : `<span class="cbox${kind === 'staged' ? ' on' : ''}" data-cb="1" title="${kind === 'staged' ? 'Quitar del área de preparación' : 'Preparar para el commit'}">${kind === 'staged' ? icon('check', 10) : ''}</span>`;
        return `<div class="frow${sel ? ' selected' : ''}" data-act="file" data-arg="${kind}|${esc(f.path)}" title="${esc(f.path)}">${cb}${fileLabel(f)}</div>`;
      }).join('');
      return `<div class="fpanel"><div class="fpanel-head">${kind === 'conflicted' ? `<span style="color:var(--red)">${icon('alert', 14)}</span>` : ''}<span class="title">${title}</span>${pill(files.length, 'var(--muted)')}<span class="right">${btn}</span></div>
        <div class="fpanel-body">${rows || `<div class="empty">${kind === 'staged' ? 'Nada preparado. Marca archivos de arriba para incluirlos en el commit.' : 'No hay cambios en el directorio de trabajo.'}</div>`}</div></div>`;
    };
    const [ok, why] = App.canCommit();
    const first = (S.message.split('\n')[0] || '').length;
    const label = S.amend ? 'Modificar último commit' : s.staged.length ? `Confirmar ${plural(s.staged.length, 'archivo', 'archivos')}` : 'Confirmar';
    el.innerHTML = `<div class="changes">
      <div class="changes-left" style="width:${S.settings.changesW}px">
        ${s.conflicted.length ? panel('conflicted', 'Conflictos', s.conflicted) : ''}
        ${panel('unstaged', 'Sin preparar', s.unstaged)}
        ${panel('staged', 'Preparados', s.staged)}
        <div class="commit-box">
          <div class="head"><b>Mensaje del commit</b><span class="counter${first > 72 ? ' warn' : ''}">${first ? first + '/72' : ''}</span></div>
          <textarea id="commit-msg" rows="4" placeholder="Resumen en la primera línea…&#10;&#10;Descripción opcional">${esc(S.message)}</textarea>
          <div class="opts"><label class="check"><input type="checkbox" data-act="amend" ${S.amend ? 'checked' : ''}>Modificar último</label><label class="check"><input type="checkbox" data-act="push-after" ${S.pushAfter ? 'checked' : ''}>Push al confirmar</label><label class="check"><input type="checkbox" data-act="sign" ${S.sign ? 'checked' : ''}>Firmar</label></div>
          <div class="foot"><span class="small">Ctrl+Enter</span><button class="btn primary" data-act="commit" ${ok ? '' : 'disabled'} title="${esc(why)}">${label}</button></div>
        </div>
      </div>
      <div class="splitter-v" id="changes-split"></div>
      <div class="changes-right">${changesDiff()}</div></div>`;
    const ta = $('#commit-msg');
    ta.oninput = () => {
      S.message = ta.value;
      const n = (S.message.split('\n')[0] || '').length;
      const cnt = $('.commit-box .counter');
      cnt.textContent = n ? n + '/72' : '';
      cnt.classList.toggle('warn', n > 72);
      const [ok2, why2] = App.canCommit();
      const b = $('[data-act="commit"]');
      b.disabled = !ok2;
      b.title = why2;
    };
    ta.onkeydown = (e) => { if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); App.actions.commit(); } };
    splitter($('#changes-split'), 'x', () => S.settings.changesW, (v) => { S.settings.changesW = Math.min(900, Math.max(260, v)); $('.changes-left').style.width = S.settings.changesW + 'px'; });
  }

  function focusedFile() {
    if (!S.sel.list || !S.sel.focus) return null;
    const f = S.status[S.sel.list].find((x) => x.path === S.sel.focus);
    return f ? [S.sel.list, f] : null;
  }

  function changesDiff() {
    if (!S.changeCount) return `<div class="placeholder">${icon('check', 34)}<div class="t">Todo limpio</div><div>No hay cambios pendientes en el directorio de trabajo.</div></div>`;
    const ff = focusedFile();
    if (!ff) return `<div class="placeholder">${icon('file', 34)}<div class="t">Selecciona un archivo para ver sus cambios</div></div>`;
    const [kind, f] = ff;
    let model, buttons;
    if (kind === 'conflicted') {
      const text = S.untracked[f.path] || '';
      let part = 'normal';
      const rows = text.replace(/\n$/, '').split('\n').map((l, i) => {
        let p = part;
        if (l.startsWith('<<<<<<<')) { part = 'ours'; p = 'marker'; }
        else if (l.startsWith('=======')) { part = 'theirs'; p = 'marker'; }
        else if (l.startsWith('>>>>>>>')) { part = 'normal'; p = 'marker'; }
        return { type: 'text', no: i + 1, text: l, part: p };
      });
      model = { mode: 'conflict', rows, note: `Conflicto (${f.status}). Edita el archivo, o elige una versión entera con los botones de arriba.` };
      buttons = '<button class="btn">Usar la mía</button><button class="btn">Usar la suya</button><button class="btn">Herramienta de fusión</button><button class="btn primary" data-act="resolve">Marcar resuelto</button>';
    } else if (f.status === '?') {
      const text = S.untracked[f.path] || '';
      model = { mode: 'untracked', rows: text.replace(/\n$/, '').split('\n').map((l, i) => ({ type: 'text', no: i + 1, text: l, added: true })) };
      buttons = `<button class="icon-btn" title="Abrir archivo">${icon('file', 14)}</button><button class="icon-btn" title="Mostrar en la carpeta">${icon('folder', 14)}</button><button class="btn" data-act="discard-file">Eliminar</button><button class="btn primary" data-act="stage-file">Preparar archivo</button>`;
    } else {
      model = { mode: kind, partial: !S.settings.ignoreWhitespace, rows: diffRows(S.diffs[f.path]) };
      buttons = `<button class="icon-btn" title="Abrir archivo">${icon('file', 14)}</button><button class="icon-btn" title="Mostrar en la carpeta">${icon('folder', 14)}</button><button class="icon-btn" title="Abrir en la herramienta de diff externa">${icon('external', 14)}</button>` +
        (kind === 'staged' ? '<button class="btn" data-act="unstage-file">Quitar archivo</button>' : '<button class="btn" data-act="discard-file">Descartar archivo</button><button class="btn primary" data-act="stage-file">Preparar archivo</button>');
    }
    return diffHeader(f.path, f.orig, buttons) + diffBody(model);
  }

  function fileClick(kind, path, e) {
    const files = S.status[kind];
    const f = files.find((x) => x.path === path);
    if (e.target.closest('[data-cb]')) {
      const targets = S.sel.list === kind && S.sel.paths.has(path) && S.sel.paths.size > 1 ? files.filter((x) => S.sel.paths.has(x.path)) : [f];
      return kind === 'staged' ? App.actions.unstage(targets) : App.actions.stage(targets);
    }
    if (S.sel.list !== kind) S.sel = { list: kind, paths: new Set(), focus: null, anchor: null };
    if (e.ctrlKey || e.metaKey) {
      if (S.sel.paths.has(path) && S.sel.paths.size > 1) S.sel.paths.delete(path);
      else S.sel.paths.add(path);
      S.sel.anchor = path;
    } else if (e.shiftKey && S.sel.anchor) {
      const a = files.findIndex((x) => x.path === S.sel.anchor), b = files.findIndex((x) => x.path === path);
      S.sel.paths = new Set(files.slice(Math.min(a, b), Math.max(a, b) + 1).map((x) => x.path));
    } else {
      S.sel.paths = new Set([path]);
      S.sel.anchor = path;
    }
    S.sel.focus = path;
    S.diffSel = new Set();
    App.renderView();
  }

  // ---------------------------------------------------------------- secondary views (views/extra.rs)
  function viewHeader(title, sub, right = '') {
    return `<div class="view-header"><span class="title">${esc(title)}</span>${sub ? `<span class="muted">${esc(sub)}</span>` : ''}<div class="right">${right}</div></div>`;
  }
  const back = `<button class="icon-text-btn" data-act="view-history">${icon('chevronRight', 14)}Volver al historial</button>`;
  const card = (inner) => `<div style="background:var(--panel);border:1px solid var(--border);border-radius:8px;padding:10px;margin-bottom:6px">${inner}</div>`;

  function reflog(el) {
    const entries = S.commits.slice(0, 20).map((c, i) => ({ c, sel: `HEAD@{${i}}`, action: i === 3 ? `checkout: moving from main to ${S.refs.currentBranch}` : `commit: ${c.subject}` }));
    el.innerHTML = `<div class="view">${viewHeader('Reflog', 'Cada movimiento de HEAD: checkouts, commits, resets, rebases… Úsalo para recuperar commits perdidos.')}
      <div style="flex:1;display:flex;min-height:0"><div style="width:560px;overflow:auto;border-right:1px solid var(--border);padding:4px">
      ${entries.map((e) => `<div class="ritem${S.selected === e.c.hash ? ' selected' : ''}" data-act="select" data-arg="${e.c.hash}"><div><span style="color:${e.action.startsWith('checkout') ? 'var(--accent)' : 'var(--green)'}">●</span> ${esc(e.action)}</div><div class="small">${e.sel} · ${e.c.hash.slice(0, 8)} · ${relTime(e.c.time)} · ${esc(e.c.subject)}</div></div>`).join('')}
      </div><div class="detail" style="flex:1;height:auto">${detail()}</div></div></div>`;
  }

  function fileHistory(el, path) {
    const revs = S.commits.filter((c) => (S.files[c.hash] || []).some((f) => f.path === path));
    S.fhSel = S.fhSel && revs.some((c) => c.hash === S.fhSel) ? S.fhSel : revs[0] && revs[0].hash;
    const f = S.fhSel && S.files[S.fhSel].find((x) => x.path === path);
    el.innerHTML = `<div class="view">${viewHeader('Historial del archivo', path, back + `<button class="btn" data-act="blame" data-arg="${esc(path)}">Blame</button>`)}
      <div style="flex:1;display:flex;min-height:0"><div style="width:380px;overflow:auto;border-right:1px solid var(--border);padding:4px">
      ${revs.map((c) => `<div class="ritem${c.hash === S.fhSel ? ' selected' : ''}" data-act="fh-select" data-arg="${c.hash}"><div>${esc(c.subject)}</div><div class="small">${c.hash.slice(0, 8)} · ${esc(c.author)} · ${relTime(c.time)}</div></div>`).join('')}
      </div><div class="detail-right">${f ? diffBody({ mode: 'commit', rows: diffRows(f.diff) }) : ''}</div></div></div>`;
  }

  function blame(el, path) {
    const revs = S.commits.filter((c) => (S.files[c.hash] || []).some((f) => f.path === path)).reverse();
    const lines = [];
    for (const c of revs) for (const l of S.files[c.hash].find((f) => f.path === path).diff.split('\n')) if (l.startsWith('+') && !l.startsWith('+++')) lines.push({ c, text: l.slice(1) });
    el.innerHTML = `<div class="view">${viewHeader('Blame', `${path} (versión del directorio de trabajo)`, back)}
      <div class="diff-body">${lines.map((l, i) => `<div class="dl" data-act="reveal" data-arg="${l.c.hash}" style="cursor:pointer"><span style="width:3px;background:${hashColor(l.c.hash)}"></span><span style="width:300px;padding-left:8px;font-family:var(--font);font-size:11.5px;color:var(--text2)">${i === 0 || lines[i - 1].c !== l.c ? `${l.c.hash.slice(0, 8)}  ${fullTime(l.c.time).slice(0, 8)}  ${esc(l.c.author)}` : ''}</span><span class="o" style="width:44px">${i + 1}</span><span class="tx">${esc(l.text)}</span></div>`).join('')}</div></div>`;
  }

  function compare(el) {
    const c = S.compare;
    const files = S.files[c.to === 'HEAD' ? S.refs.head : c.to] || [];
    c.sel = Math.min(c.sel || 0, Math.max(0, files.length - 1));
    const f = files[c.sel];
    el.innerHTML = `<div class="view">${viewHeader('Comparar', `${c.fromLabel}  →  ${c.toLabel}`, back + '<button class="btn" data-act="cmp-swap">Intercambiar</button><button class="btn">Guardar como parche…</button>')}
      <div style="flex:1;display:flex;min-height:0"><div style="width:360px;overflow:auto;border-right:1px solid var(--border);padding:4px"><div class="small" style="padding:4px 6px">ARCHIVOS · ${files.length}</div>
      ${files.map((x, i) => `<div class="frow${i === c.sel ? ' selected' : ''}" data-act="cmp-file" data-arg="${i}">${fileLabel(x)}</div>`).join('')}</div>
      <div class="detail-right">${f ? diffBody({ mode: 'compare', rows: diffRows(f.diff) }) : ''}</div></div></div>`;
  }

  function prs(el) {
    const list = [
      { n: 12, title: 'Caché de respuestas para la API', author: 'ana', src: 'feature/api', dst: 'develop', draft: false },
      { n: 11, title: 'Nueva página de ajustes', author: 'luis', src: 'feature/settings', dst: 'develop', draft: true },
    ];
    el.innerHTML = `<div class="view">${viewHeader('Pull requests', 'GitHub · ana/demo', '<button class="btn">Abrir en el navegador</button><button class="icon-btn" title="Actualizar">' + icon('refresh', 14) + '</button><button class="btn primary" data-act="create-pr">Nueva pull request</button>')}
      <div style="padding:12px;overflow:auto">${list.map((p) => card(`<div style="display:flex;align-items:center;gap:8px"><span style="color:${p.draft ? 'var(--muted)' : 'var(--green)'}">${icon('pullRequest', 16)}</span><b>#${p.n} ${esc(p.title)}</b>${p.draft ? pill('borrador', 'var(--muted)') : ''}<span style="margin-left:auto;display:flex;gap:6px"><button class="btn">Checkout</button><button class="btn">Abrir</button></span></div><div class="small">${p.author} quiere fusionar ${p.src} → ${p.dst}</div>`)).join('')}</div></div>`;
  }

  function submodules(el) {
    el.innerHTML = `<div class="view">${viewHeader('Submódulos', '', '<button class="btn">Sincronizar URLs</button><button class="btn">Actualizar todos</button><button class="btn primary">Añadir submódulo</button>')}
      <div style="padding:12px">${card(`<div style="display:flex;align-items:center;gap:8px"><span style="color:var(--accent)">${icon('box', 16)}</span><b>vendor/lib</b>${pill('Al día', 'var(--green)')}<span style="margin-left:auto;display:flex;gap:6px"><button class="btn">Desactivar</button><button class="btn">Actualizar</button><button class="btn">Abrir</button></span></div><div class="small">https://github.com/ana/lib.git · 3f2a9c1d (v0.4.2)</div>`)}</div></div>`;
  }

  function worktrees(el) {
    el.innerHTML = `<div class="view">${viewHeader('Worktrees', '', '<button class="btn">Limpiar</button><button class="btn primary">Nuevo worktree</button>')}
      <div style="padding:12px">${card(`<div style="display:flex;align-items:center;gap:8px"><span style="color:var(--accent)">${icon('layers', 16)}</span><b>${esc(S.repo.path)}</b>${pill('principal', 'var(--accent)')}</div><div class="small">${S.refs.currentBranch} · ${S.refs.head.slice(0, 8)}</div>`)}
      ${card(`<div style="display:flex;align-items:center;gap:8px"><span style="color:var(--accent)">${icon('layers', 16)}</span><b>${esc(S.repo.path)}-wt</b><span style="margin-left:auto;display:flex;gap:6px"><button class="btn">Abrir</button><button class="btn">Eliminar</button></span></div><div class="small">feature/api · 18646380</div>`)}</div></div>`;
  }

  function lfs(el) {
    el.innerHTML = `<div class="view">${viewHeader('Git LFS', 'Archivos grandes guardados fuera del repositorio', '<button class="btn">Descargar (lfs pull)</button><button class="btn primary">Seguir patrón…</button>')}
      <div style="padding:12px"><div class="small">git-lfs/3.5.1 (GitHub; windows amd64; go 1.21.7)</div><p><b>Patrones seguidos</b></p>
      <div style="display:flex;gap:8px;align-items:center"><span class="mono">*.psd</span><button class="btn small">Dejar de seguir</button></div>
      <p><b>Archivos · 2</b></p>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:4px">${pill('descargado', 'var(--green)')}<span>arte/portada.psd</span><span style="margin-left:auto">${pill('bloqueado por luis', 'var(--yellow)')} <button class="btn small">Desbloquear</button></span></div>
      <div style="display:flex;gap:8px;align-items:center">${pill('solo puntero', 'var(--muted)')}<span>arte/iconos.psd</span><span style="margin-left:auto"><button class="btn small">Bloquear</button></span></div></div></div>`;
  }

  function openCompare(from, to, fromLabel, toLabel) {
    S.compare = { from, to, fromLabel, toLabel, sel: 0 };
    App.setView('compare');
  }

  // ---------------------------------------------------------------- home (views/home.rs)
  function home(el) {
    const card2 = (ic, t, s, act) => `<div class="card" data-act="${act}"><div class="ac">${icon(ic, 19)}</div><div><div class="t">${t}</div><div class="s">${s}</div></div></div>`;
    el.innerHTML = `<div class="home"><div class="home-left"><h1>CodeSprout</h1><p>Tu cliente Git: explorador de ramas, preparación por líneas, rebase interactivo, git-flow y pull requests.</p>
      ${card2('fetch', 'Clonar repositorio', 'Desde una URL o tus repos de GitHub/GitLab/Bitbucket', 'clone')}
      ${card2('folder', 'Añadir repositorio existente', 'Una carpeta que ya usa Git', 'mock-only')}
      ${card2('plus', 'Crear repositorio nuevo', 'Inicializa Git en una carpeta', 'mock-only')}
      ${card2('user', 'Cuentas', 'Inicia sesión con SSO en GitHub, GitLab, Bitbucket o Azure', 'accounts')}</div>
      <div class="home-right"><div class="head"><h2>Repositorios · 2</h2><input type="text" placeholder="Filtrar repositorios" style="margin-left:auto;width:220px"></div>
      ${[[S.repo.name, S.repo.path], ['codesprout', 'D:\\GITHUB REPOS\\codesprout']].map(([n, p]) => `<div class="repo-row" data-act="tab"><div class="ri" style="background:${hashColor(n)}">${initials(n)}</div><div><div style="font-size:14px">${esc(n)}</div><div class="small">${esc(p)}</div></div></div>`).join('')}</div></div>`;
  }

  // ---------------------------------------------------------------- shared helpers
  function splitter(el, axis, get, set, invert = false) {
    if (!el) return;
    el.addEventListener('pointerdown', (e) => {
      el.setPointerCapture(e.pointerId);
      el.classList.add('drag');
      const start = axis === 'y' ? e.clientY : e.clientX, v0 = get();
      const move = (ev) => {
        const d = (axis === 'y' ? ev.clientY : ev.clientX) - start;
        set(v0 + (invert ? -d : d));
      };
      el.addEventListener('pointermove', move);
      el.addEventListener('pointerup', () => { el.removeEventListener('pointermove', move); el.classList.remove('drag'); }, { once: true });
    });
  }

  function render(el) {
    App.closeMenu();
    switch (S.view) {
      case 'history': return history(el);
      case 'changes': return changes(el);
      case 'reflog': return reflog(el);
      case 'file-history': return fileHistory(el, S.fhPath);
      case 'blame': return blame(el, S.blamePath);
      case 'compare': return compare(el);
      case 'prs': return prs(el);
      case 'submodules': return submodules(el);
      case 'worktrees': return worktrees(el);
      case 'lfs': return lfs(el);
    }
  }

  function onAct(act, arg, el, e) {
    switch (act) {
      case 'mode': S.settings.historyMode = arg; App.renderView(); break;
      case 'orientation': S.settings.vertical = arg === 'v'; explorer && explorer.reset(); explorer && (explorer.centerOn = S.selected); App.recompute(); App.renderView(); break;
      case 'chrono': S.settings.chronological = !S.settings.chronological; App.recompute(); App.renderView(); break;
      case 'zoom': S.settings.zoom = Math.min(2.5, Math.max(0.4, S.settings.zoom * (arg === 'in' ? 1.2 : 1 / 1.2))); explorer.draw(); break;
      case 'go-head': App.select(S.refs.head, true); break;
      case 'all-branches': S.settings.allBranches = el.checked; break;
      case 'search': runSearch(); break;
      case 'close-search': S.search.results = null; App.renderView(); break;
      case 'select': App.select(arg, false); break;
      case 'reveal': S.view = S.view === 'reflog' ? 'reflog' : 'history'; App.render(); App.select(arg, true); break;
      case 'detail-file': S.detailFile = +arg; App.renderView(); break;
      case 'copy': navigator.clipboard.writeText(arg); App.toast('Hash copiado'); break;
      case 'file-history': S.fhPath = arg; App.setView('file-history'); break;
      case 'fh-select': S.fhSel = arg; App.renderView(); break;
      case 'blame': S.blamePath = arg; App.setView('blame'); break;
      case 'cmp-file': S.compare.sel = +arg; App.renderView(); break;
      case 'cmp-swap': { const c = S.compare; openCompare(c.to, c.from, c.toLabel, c.fromLabel); break; }
      case 'file': { const i = arg.indexOf('|'); fileClick(arg.slice(0, i), arg.slice(i + 1), e); break; }
      case 'stage-all': App.actions.stageAll(); break;
      case 'unstage-all': App.actions.unstageAll(); break;
      case 'stage-file': App.actions.stage([focusedFile()[1]]); break;
      case 'unstage-file': App.actions.unstage([focusedFile()[1]]); break;
      case 'discard-file': App.actions.discard([focusedFile()[1]]); break;
      case 'resolve': App.op('Marcar como resuelto', () => { const f = focusedFile()[1]; S.status.conflicted = S.status.conflicted.filter((x) => x.path !== f.path); S.status.staged.push({ path: f.path, status: 'M' }); }); break;
      case 'commit': App.actions.commit(); break;
      case 'amend': S.amend = el.checked; if (S.amend && !S.message.trim()) S.message = S.commits[S.index.get(S.refs.head)].subject; App.renderView(); break;
      case 'push-after': S.pushAfter = el.checked; break;
      case 'sign': S.sign = el.checked; break;
      case 'opt-ws': S.settings.ignoreWhitespace = el.checked; App.renderView(); break;
      case 'opt-word': S.settings.wordDiff = el.checked; App.renderView(); break;
      case 'line': {
        const [fi, hi] = arg.split(':');
        for (const k of [...S.diffSel]) if (!k.startsWith(`${fi}:${hi}:`)) S.diffSel.delete(k);
        if (S.diffSel.has(arg)) S.diffSel.delete(arg); else S.diffSel.add(arg);
        App.renderView();
        break;
      }
      case 'hunk': {
        const [what] = arg.split(':');
        const f = focusedFile()[1];
        const n = S.diffSel.size;
        S.diffSel = new Set();
        if (what === 'stage') App.op('Aplicar cambios parciales', () => { if (!S.status.staged.some((x) => x.path === f.path)) S.status.staged.push({ ...f }); }, n ? `${plural(n, 'línea preparada', 'líneas preparadas')} (simulado)` : 'Bloque preparado (simulado)');
        else if (what === 'unstage') App.op('Aplicar cambios parciales', () => {}, 'Bloque quitado (simulado)');
        else Dialogs.confirm('Descartar cambios', `Se descartarán ${n ? plural(n, 'línea cambiada', 'líneas cambiadas') : 'las líneas del bloque'} del directorio de trabajo. No se puede deshacer.`, null, 'Descartar', true, () => App.toast('Bloque descartado (simulado)'));
        break;
      }
      case 'clone': Dialogs.clone(); break;
    }
  }

  function onKey(e) {
    if (S.view !== 'changes' || !S.sel.list) return;
    const files = S.status[S.sel.list];
    const i = files.findIndex((f) => f.path === S.sel.focus);
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const next = files[Math.max(0, Math.min(files.length - 1, i + (e.key === 'ArrowDown' ? 1 : -1)))];
      if (next) fileClick(S.sel.list, next.path, { target: document.body, shiftKey: e.shiftKey });
    } else if (e.key === ' ' || e.key === 'Enter') {
      e.preventDefault();
      const sel = files.filter((f) => S.sel.paths.has(f.path));
      if (S.sel.list === 'staged') App.actions.unstage(sel);
      else if (S.sel.list === 'unstaged') App.actions.stage(sel);
    } else if (e.key === 'Delete' && S.sel.list === 'unstaged') {
      App.actions.discard(files.filter((f) => S.sel.paths.has(f.path)));
    }
  }

  return { render, home, onAct, onKey, openCompare, explorer: null };
})();
