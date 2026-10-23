'use strict';
// Commit graph lane assignment + SVG rendering for one row.
(function (root) {
  const LANE_W = 14;
  const COLORS = ['#4f8dff', '#f5a524', '#17c964', '#f0457a', '#a066f0', '#14b8c4', '#ff7a45', '#8fd14f', '#e05cc9', '#5aa7a0'];

  // commits must be ordered children-before-parents (git log --date-order / --topo-order).
  function computeGraph(commits) {
    const lanes = [];
    const rows = [];
    let maxLanes = 1;
    for (const c of commits) {
      const before = lanes.slice();
      let col = lanes.indexOf(c.hash);
      if (col === -1) {
        col = lanes.indexOf(null);
        if (col === -1) {
          col = lanes.length;
          lanes.push(null);
        }
      }
      for (let i = 0; i < lanes.length; i++) if (lanes[i] === c.hash) lanes[i] = null;
      const parentLanes = [];
      c.parents.forEach((p, idx) => {
        // The first parent always continues this lane; lanes expecting the same commit converge on it later.
        if (idx === 0) {
          lanes[col] = p;
          parentLanes.push(col);
          return;
        }
        const existing = lanes.indexOf(p);
        if (existing !== -1) {
          parentLanes.push(existing);
          return;
        }
        let slot = lanes.indexOf(null);
        if (slot === -1) {
          slot = lanes.length;
          lanes.push(null);
        }
        lanes[slot] = p;
        parentLanes.push(slot);
      });
      while (lanes.length && lanes[lanes.length - 1] === null) lanes.pop();
      rows.push({ hash: c.hash, col, before, parentLanes });
      maxLanes = Math.max(maxLanes, before.length, lanes.length, col + 1);
    }
    return { rows, maxLanes };
  }

  const x = (k) => k * LANE_W + LANE_W / 2 + 4;
  const color = (k) => COLORS[k % COLORS.length];

  function curve(x1, y1, x2, y2, stroke) {
    if (x1 === x2) return `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${stroke}"/>`;
    const my = (y1 + y2) / 2;
    return `<path d="M${x1} ${y1} C${x1} ${my} ${x2} ${my} ${x2} ${y2}" stroke="${stroke}" fill="none"/>`;
  }

  function rowSvg(row, { height, width, kind }) {
    const mid = height / 2;
    let s = `<svg width="${width}" height="${height}" class="graph-svg" stroke-width="2" stroke-linecap="round">`;
    row.before.forEach((h, k) => {
      if (h == null) return;
      if (h === row.hash) s += curve(x(k), 0, x(row.col), mid, color(k));
      else s += curve(x(k), 0, x(k), height, color(k));
    });
    for (const k of row.parentLanes) s += curve(x(row.col), mid, x(k), height, color(k));
    const cx = x(row.col);
    const c = color(row.col);
    if (kind === 'worktree') {
      s += `<circle cx="${cx}" cy="${mid}" r="5" fill="var(--bg)" stroke="${c}" stroke-dasharray="2.5 2"/>`;
    } else if (kind === 'merge') {
      s += `<circle cx="${cx}" cy="${mid}" r="4" fill="var(--bg)" stroke="${c}" stroke-width="2.5"/>`;
    } else if (kind === 'head') {
      s += `<circle cx="${cx}" cy="${mid}" r="6" fill="${c}" stroke="var(--bg)" stroke-width="2"/><circle cx="${cx}" cy="${mid}" r="2.2" fill="var(--bg)" stroke="none"/>`;
    } else {
      s += `<circle cx="${cx}" cy="${mid}" r="4.5" fill="${c}" stroke="var(--bg)" stroke-width="1.5"/>`;
    }
    return s + '</svg>';
  }

  const api = { computeGraph, rowSvg, LANE_W, COLORS, laneColor: color };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.Graph = api;
})(typeof window !== 'undefined' ? window : globalThis);
