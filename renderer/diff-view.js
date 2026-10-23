'use strict';
/* global h, clear, icon, splitPath, fmtBytes, confirmDialog, DiffCore */

const MAX_RENDER_LINES = 5000;

const DiffView = (() => {
  function pathTitle(p, extra) {
    const { dir, base } = splitPath(p || '');
    return h('div', { class: 'path', title: p }, h('span', { class: 'dir' }, dir), base, extra || null);
  }

  function placeholder(host, iconName, text, sub) {
    host._key = null;
    host._sig = null;
    clear(host).appendChild(h('div', { class: 'diff-placeholder' }, icon(iconName, 34, 1.5), h('div', null, text), sub ? h('div', { class: 'small' }, sub) : null));
  }

  function lineRow(type, oldNo, newNo, sign, text) {
    // 'ctx' would collide with the context-menu class, so context lines are 'plain'
    return h('div', { class: `dl ${type === 'ctx' ? 'plain' : type}` },
      h('span', { class: 'ln' }, oldNo == null ? '' : String(oldNo)),
      h('span', { class: 'ln' }, newNo == null ? '' : String(newNo)),
      h('span', { class: 'sign' }, sign),
      h('span', { class: 'code' }, text));
  }

  // opts: { key, title, titleExtra, actions[], mode: 'unstaged'|'staged'|'commit',
  //         text, truncated, lossy, untracked, conflict, onPatch(patch, flags), note }
  function render(host, opts) {
    const sig = JSON.stringify([opts.text, opts.untracked && opts.untracked.text, opts.conflict, opts.mode, opts.truncated]);
    const sameKey = host._key === opts.key;
    if (sameKey && host._sig === sig && host._actionsSig === opts.actionsSig) return;
    const prevScroll = sameKey && host.querySelector('.diff-scroll') ? host.querySelector('.diff-scroll').scrollTop : 0;
    host._key = opts.key;
    host._sig = sig;
    host._actionsSig = opts.actionsSig;

    const head = h('div', { class: 'diff-head' }, pathTitle(opts.title, opts.titleExtra), opts.actions || null);
    const scroll = h('div', { class: 'diff-scroll' });
    const body = h('div', { class: 'diff-body' });
    scroll.appendChild(body);
    clear(host);
    host.appendChild(head);
    host.appendChild(scroll);

    if (opts.note) body.appendChild(h('div', { class: 'diff-hint' }, opts.note));

    if (opts.conflict != null) renderConflict(body, opts.conflict);
    else if (opts.untracked) renderUntracked(body, opts.untracked);
    else renderUnified(body, opts);

    scroll.scrollTop = prevScroll;
  }

  function renderUntracked(body, u) {
    if (u.directory) {
      body.appendChild(h('div', { class: 'diff-note' }, 'Carpeta sin seguimiento (probablemente un repositorio anidado o submódulo).'));
      return;
    }
    if (u.binary) {
      body.appendChild(h('div', { class: 'diff-note' }, `Archivo binario nuevo · ${fmtBytes(u.size)}`));
      return;
    }
    const lines = u.text.split('\n');
    if (lines.length && lines[lines.length - 1] === '') lines.pop();
    if (!lines.length) {
      body.appendChild(h('div', { class: 'diff-note' }, 'Archivo vacío'));
      return;
    }
    const hunk = h('div', { class: 'hunk' }, h('div', { class: 'hunk-head' }, h('span', { class: 'h-text' }, `Archivo nuevo · ${lines.length} líneas`)));
    const shown = lines.slice(0, MAX_RENDER_LINES);
    shown.forEach((l, i) => hunk.appendChild(lineRow('add', null, i + 1, '+', l.replace(/\r$/, ''))));
    body.appendChild(hunk);
    if (lines.length > shown.length || u.truncated) {
      body.appendChild(h('div', { class: 'diff-note' }, 'El archivo es demasiado grande para mostrarse completo.'));
    }
  }

  function renderConflict(body, text) {
    const lines = text.split('\n');
    if (lines.length && lines[lines.length - 1] === '') lines.pop();
    const hunk = h('div', { class: 'hunk' }, h('div', { class: 'hunk-head' },
      h('span', { class: 'h-text' }, 'Contenido actual con marcadores de conflicto')));
    let region = null;
    lines.slice(0, MAX_RENDER_LINES * 2).forEach((raw, i) => {
      const l = raw.replace(/\r$/, '');
      let cls = region === 'ours' ? 'conflict-ours' : region === 'theirs' ? 'conflict-theirs' : region === 'base' ? 'meta' : 'ctx';
      if (l.startsWith('<<<<<<<')) {
        region = 'ours';
        cls = 'conflict-marker';
      } else if (region && l.startsWith('|||||||')) {
        region = 'base';
        cls = 'conflict-marker';
      } else if (region && l.startsWith('=======')) {
        region = 'theirs';
        cls = 'conflict-marker';
      } else if (region && l.startsWith('>>>>>>>')) {
        region = null;
        cls = 'conflict-marker';
      }
      hunk.appendChild(lineRow(cls, null, i + 1, ' ', l));
    });
    body.appendChild(hunk);
  }

  function renderUnified(body, opts) {
    const files = DiffCore.parseDiff(opts.text);
    if (!files.length) {
      body.appendChild(h('div', { class: 'diff-note' }, opts.emptyText || 'Sin diferencias de contenido.'));
      return;
    }
    const patchable = (opts.mode === 'unstaged' || opts.mode === 'staged') && !opts.truncated && !opts.lossy && typeof opts.onPatch === 'function';
    let budget = MAX_RENDER_LINES;
    for (const file of files) {
      if (file.binary) {
        body.appendChild(h('div', { class: 'diff-note' }, 'Archivo binario modificado.'));
        continue;
      }
      if (!file.hunks.length) {
        const what = file.renamedFrom ? `Renombrado desde ${file.renamedFrom} sin cambios de contenido.`
          : file.modeChange ? 'Solo ha cambiado el modo (permisos) del archivo.'
            : file.isNew ? 'Archivo nuevo vacío.'
              : file.isDeleted ? 'Archivo vacío eliminado.' : 'Sin cambios de contenido.';
        body.appendChild(h('div', { class: 'diff-note' }, what));
        continue;
      }
      const partial = patchable && DiffCore.canPartial(file);
      if (patchable && !partial && (file.isNew || file.isDeleted)) {
        body.appendChild(h('div', { class: 'diff-hint' }, file.isNew ? 'Archivo nuevo: usa los botones del archivo para prepararlo o quitarlo entero.' : 'Archivo eliminado: usa los botones del archivo para prepararlo o quitarlo.'));
      } else if (partial) {
        body.appendChild(h('div', { class: 'diff-hint' }, 'Consejo: haz clic en las líneas añadidas/eliminadas (Shift+clic para un rango) para preparar solo esas líneas.'));
      }
      for (const hunk of file.hunks) {
        if (budget <= 0) break;
        body.appendChild(renderHunk(file, hunk, partial ? opts : null));
        budget -= hunk.lines.length;
      }
      if (budget <= 0) {
        const more = h('div', { class: 'diff-note' }, 'El diff es muy grande y se ha recortado. ',
          h('button', { class: 'btn small', onClick: () => { clear(body); renderAll(body, files); } }, 'Mostrar todo'));
        body.appendChild(more);
        break;
      }
    }
    if (opts.truncated) body.appendChild(h('div', { class: 'diff-note' }, 'El diff supera el tamaño máximo y se muestra incompleto.'));
    if (opts.lossy && patchable) body.appendChild(h('div', { class: 'diff-hint' }, 'El archivo no está en UTF-8: la preparación parcial está desactivada para evitar corromperlo.'));
  }

  function renderAll(body, files) {
    for (const file of files) for (const hunk of file.hunks) body.appendChild(renderHunk(file, hunk, null));
  }

  function renderHunk(file, hunk, ctx) {
    const nums = DiffCore.numberHunk(hunk);
    const selected = new Set();
    let anchor = null;
    const actions = h('div', { class: 'h-actions' });
    const head = h('div', { class: 'hunk-head' }, h('span', { class: 'h-text', title: hunk.header }, hunk.header), actions);
    const wrap = h('div', { class: `hunk${ctx ? ' selectable-lines' : ''}` }, head);
    const rows = hunk.lines.map((l, i) => {
      const n = nums[i];
      const text = n.type === 'meta' ? l.slice(2) : l.slice(1).replace(/\r$/, '');
      const row = lineRow(n.type, n.old, n.new, n.type === 'add' ? '+' : n.type === 'del' ? '-' : ' ', text);
      wrap.appendChild(row);
      return row;
    });

    if (!ctx) return wrap;

    const isChange = (i) => nums[i].type === 'add' || nums[i].type === 'del';
    const run = async (kind) => {
      const sel = selected.size ? selected : null;
      const reverse = kind !== 'stage';
      const patch = DiffCore.buildPatch(file, hunk, sel, reverse);
      if (!patch) return;
      if (kind === 'discard') {
        const ok = await confirmDialog({
          title: sel ? 'Descartar líneas seleccionadas' : 'Descartar bloque',
          message: sel ? `¿Descartar ${selected.size} línea(s) seleccionada(s) del directorio de trabajo?` : '¿Descartar los cambios de este bloque del directorio de trabajo?',
          detail: 'Esta acción no se puede deshacer.',
          confirmLabel: 'Descartar',
          danger: true,
        });
        if (!ok) return;
      }
      const flags = kind === 'stage' ? { cached: true } : kind === 'unstage' ? { cached: true, reverse: true } : { reverse: true };
      for (const b of actions.querySelectorAll('button')) b.disabled = true;
      try {
        await ctx.onPatch(patch, flags);
      } finally {
        for (const b of actions.querySelectorAll('button')) b.disabled = false;
      }
    };

    const updateActions = () => {
      clear(actions);
      const suffix = selected.size ? `líneas (${selected.size})` : 'bloque';
      if (selected.size) {
        actions.appendChild(h('button', { class: 'btn small ghost', onClick: () => { selected.clear(); paint(); } }, 'Limpiar'));
      }
      if (ctx.mode === 'unstaged') {
        actions.appendChild(h('button', { class: 'btn small', onClick: () => run('discard') }, icon('discard', 13), `Descartar ${suffix}`));
        actions.appendChild(h('button', { class: 'btn small primary', onClick: () => run('stage') }, icon('plus', 13), `Preparar ${suffix}`));
      } else {
        actions.appendChild(h('button', { class: 'btn small', onClick: () => run('unstage') }, icon('minus', 13), `Quitar ${suffix}`));
      }
    };
    const paint = () => {
      rows.forEach((r, i) => r.classList.toggle('picked', selected.has(i)));
      updateActions();
    };

    rows.forEach((row, i) => {
      if (!isChange(i)) return;
      row.addEventListener('mousedown', (e) => {
        if (e.shiftKey) e.preventDefault();
      });
      row.addEventListener('click', (e) => {
        if (e.button !== 0) return;
        const textSel = window.getSelection();
        if (textSel && !textSel.isCollapsed && textSel.toString()) return; // user is selecting text to copy
        if (e.shiftKey && anchor != null) {
          const [a, b] = anchor < i ? [anchor, i] : [i, anchor];
          for (let k = a; k <= b; k++) if (isChange(k)) selected.add(k);
        } else if (selected.has(i)) selected.delete(i);
        else selected.add(i);
        anchor = i;
        paint();
      });
    });
    updateActions();
    return wrap;
  }

  return { render, placeholder };
})();
