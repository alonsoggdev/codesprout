// Branch explorer layout. Line-by-line port of crates/ds-core/src/explorer.rs (fn layout):
// lanes from first-parent walks in priority order, inferred lanes for deleted branches named
// from merge messages, compact / chronological columns, rows packed under the parent lane.
// If the algorithm changes in Rust, change it here too (and the other way round).
(function () {
  const MAIN_NAMES = ['main', 'master', 'trunk', 'default'];
  const isMain = (n) => MAIN_NAMES.includes(n);

  function mergedBranchName(subject) {
    const s = subject.trim();
    const quoted = (t) => {
      const a = t.indexOf("'");
      if (a < 0) return null;
      const b = t.indexOf("'", a + 1);
      return b < 0 ? null : t.slice(a + 1, b);
    };
    let m;
    if (s.startsWith('Merge remote-tracking branch ')) {
      const n = quoted(s.slice(29));
      return n ? (n.includes('/') ? n.slice(n.indexOf('/') + 1) : n) : null;
    }
    if (s.startsWith('Merge branch ')) return quoted(s.slice(13));
    if ((m = /^Merge pull request \S+ from (\S+)/.exec(s))) return m[1].includes('/') ? m[1].slice(m[1].indexOf('/') + 1) : m[1];
    if ((m = /^Merged in (.+?) \(/.exec(s))) return m[1].trim() || null;
    if ((m = /^Merge (\S+) into /.exec(s))) return m[1].replace(/'/g, '');
    return null;
  }

  function colorFor(name) {
    let h = 2166136261 >>> 0;
    for (const ch of new TextEncoder().encode(name)) {
      h ^= ch;
      h = Math.imul(h, 16777619) >>> 0;
    }
    return h;
  }

  // commits: newest first (git log --date-order). refs: { local, remote, tags, head, currentBranch }
  function layout(commits, refs, worktree, chronological) {
    const n = commits.length;
    const index = new Map(commits.map((c, i) => [c.hash, i]));
    const parents = commits.map((c) => c.parents.map((p) => index.get(p)).filter((x) => x !== undefined));
    const firstOk = commits.map((c) => c.parents.length > 0 && index.has(c.parents[0]));

    // ---- branch tips in priority order
    const tips = [];
    const rank = (name) => (isMain(name) ? 0 : ['develop', 'development', 'dev'].includes(name) ? 1 : 3);
    const locals = refs.local.filter((b) => index.has(b.hash)).map((b) => [b, index.get(b.hash)]);
    locals.sort((x, y) => {
      const kx = Math.min(rank(x[0].name), x[0].current ? 2 : 3), ky = Math.min(rank(y[0].name), y[0].current ? 2 : 3);
      return kx - ky || x[1] - y[1];
    });
    for (const [b, i] of locals) tips.push({ name: b.name, kind: 'local', idx: i, isMain: isMain(b.name), isCurrent: b.current });
    const remotes = refs.remote.filter((b) => index.has(b.hash)).map((b) => [b, index.get(b.hash)]);
    remotes.sort((x, y) => (isMain(x[0].branch) ? 0 : 1) - (isMain(y[0].branch) ? 0 : 1) || x[1] - y[1]);
    const hasLocalMain = tips.some((t) => t.isMain);
    for (const [b, i] of remotes) {
      const main = isMain(b.branch) && !hasLocalMain;
      const tip = { name: b.name, kind: 'remote', idx: i, isMain: main, isCurrent: false };
      if (main) tips.unshift(tip);
      else tips.push(tip);
    }
    if (!refs.currentBranch && refs.head && index.has(refs.head)) tips.push({ name: 'HEAD', kind: 'detached', idx: index.get(refs.head), isMain: false, isCurrent: true });
    for (const t of refs.tags) if (index.has(t.hash)) tips.push({ name: t.name, kind: 'detached', idx: index.get(t.hash), isMain: false, isCurrent: false });

    // ---- assign commits to lanes
    const laneOf = new Array(n).fill(-1);
    const lanes = [];
    const newLane = (t) => {
      if (laneOf[t.idx] >= 0) return;
      const id = lanes.length;
      let c = t.idx;
      let base = null;
      for (;;) {
        if (laneOf[c] >= 0) {
          base = c;
          break;
        }
        laneOf[c] = id;
        if (!firstOk[c]) break;
        c = parents[c][0];
      }
      lanes.push({ name: t.name, kind: t.kind, row: 0, start: 0, end: 0, tip: t.idx, base, parentLane: null, mergedInto: null, isMain: t.isMain, isCurrent: t.isCurrent, color: t.isMain ? 0 : colorFor(t.name) });
    };
    tips.forEach(newLane);

    const mergedBy = new Map();
    parents.forEach((ps, i) => {
      ps.slice(firstOk[i] ? 1 : 0).forEach((p) => {
        if (!mergedBy.has(p)) mergedBy.set(p, i);
      });
    });
    for (let i = 0; i < n; i++) {
      if (laneOf[i] < 0) {
        const m = mergedBy.get(i);
        const name = (m !== undefined && mergedBranchName(commits[m].subject)) || '(rama eliminada)';
        newLane({ name, kind: 'inferred', idx: i, isMain: false, isCurrent: false });
      }
    }
    for (const l of lanes) l.mergedInto = mergedBy.has(l.tip) ? mergedBy.get(l.tip) : null;

    // ---- columns
    const col = new Array(n).fill(0);
    if (chronological) {
      for (let i = 0; i < n; i++) col[i] = n - 1 - i;
    } else {
      for (let i = n - 1; i >= 0; i--) col[i] = parents[i].reduce((m, p) => Math.max(m, col[p] + 1), 0);
    }
    let columns = n ? Math.max(...col) + 1 : 0;
    const nodes = col.map((c, i) => ({ col: c, row: 0, lane: Math.max(0, laneOf[i]) }));

    // ---- working tree node
    let wtIdx = null;
    if (worktree) {
      const headIdx = refs.head !== null && index.has(refs.head) ? index.get(refs.head) : null;
      let lane;
      if (headIdx !== null) lane = nodes[headIdx].lane;
      else {
        lanes.push({ name: refs.currentBranch || 'main', kind: 'local', row: 0, start: 0, end: 0, tip: n, base: null, parentLane: null, mergedInto: null, isMain: true, isCurrent: true, color: 0 });
        lane = lanes.length - 1;
      }
      // chronologically the uncommitted changes are the newest thing: always the last column
      const c = chronological ? columns : headIdx === null ? 0 : nodes[headIdx].col + 1;
      columns = Math.max(columns, c + 1);
      nodes.push({ col: c, row: 0, lane });
      wtIdx = n;
    }

    // ---- lane extents
    for (const l of lanes) {
      l.start = Infinity;
      l.end = 0;
    }
    for (const nd of nodes) {
      const l = lanes[nd.lane];
      l.start = Math.min(l.start, nd.col);
      l.end = Math.max(l.end, nd.col);
    }
    for (const l of lanes) {
      if (l.start === Infinity) l.start = 0;
      l.parentLane = l.base !== null ? nodes[l.base].lane : null;
    }

    // ---- rows: main on top, every other lane in the first free row under its parent
    const depth = (i) => {
      let d = 0;
      while (lanes[i].parentLane !== null && d <= lanes.length) {
        d++;
        i = lanes[i].parentLane;
      }
      return d;
    };
    const order = lanes.map((_, i) => i);
    order.sort((a, b) => (lanes[a].isMain ? 0 : 1) - (lanes[b].isMain ? 0 : 1) || depth(a) - depth(b) || a - b);
    const occupied = [];
    for (const i of order) {
      const l = lanes[i];
      const from = l.base !== null ? nodes[l.base].col : l.start;
      const to = Math.max(l.end, l.mergedInto !== null ? Math.max(0, nodes[l.mergedInto].col - 1) : 0);
      const minRow = l.isMain ? 0 : Math.max(1, l.parentLane !== null ? lanes[l.parentLane].row + 1 : 1);
      let row = minRow;
      for (;;) {
        while (occupied.length <= row) occupied.push([]);
        if (occupied[row].every(([s, e]) => to < s || from > e)) break;
        row++;
      }
      occupied[row].push([from, to]);
      l.row = row;
    }
    for (const nd of nodes) nd.row = lanes[nd.lane].row;
    const rows = lanes.reduce((m, l) => Math.max(m, l.row + 1), 1);

    // ---- edges
    const edges = [];
    parents.forEach((ps, i) => {
      ps.forEach((p, k) => {
        const kind = k === 0 && firstOk[i] ? (nodes[p].lane === nodes[i].lane ? 'line' : 'branch') : 'merge';
        edges.push({ from: p, to: i, kind });
      });
    });
    if (wtIdx !== null && refs.head && index.has(refs.head)) edges.push({ from: index.get(refs.head), to: wtIdx, kind: 'line' });

    return { nodes, lanes, edges, columns, rows, worktree: wtIdx };
  }

  window.ExplorerLayout = { layout, mergedBranchName };
})();
