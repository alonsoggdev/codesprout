// Branch explorer canvas. Port of crates/codesprout-app/src/views/explorer.rs: same constants,
// same drawing order (row highlights, bands, edges, nodes, badges, lane names, row text),
// same interaction (drag, wheel, Ctrl+wheel zoom, click, Ctrl+click, double click, menu).
(function () {
  // ---- constants: keep in sync with views/explorer.rs
  const COL_W = 34, ROW_H = 72, BAND_TOP = 25, BAND_BOTTOM = 12, PAD_X = 60, PAD_Y = 40;
  const V_ROW = 28, V_LANE = 24, V_PAD_X = 22, V_PAD_Y = 18, V_TEXT_W = 900;
  const MIN_FONT = 11;

  const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  const alpha = (hex, a) => {
    const h = hex.replace('#', '');
    const r = parseInt(h.slice(0, 2), 16), g = parseInt(h.slice(2, 4), 16), b = parseInt(h.slice(4, 6), 16);
    return `rgba(${r},${g},${b},${(a / 255).toFixed(3)})`;
  };
  const laneColor = (i) => css(`--lane-${i % 10}`);
  const FONT = () => css('--font');
  const MONO = () => css('--mono');

  class Geo {
    constructor(ox, oy, zoom, vertical, columns) {
      Object.assign(this, { ox, oy, zoom, vertical, columns });
      if (vertical) {
        this.step = V_ROW;
        this.lane = Math.min(48, Math.max(14, V_LANE * zoom));
      } else {
        this.step = COL_W * zoom;
        this.lane = ROW_H * zoom;
      }
    }
    padY() { return Math.max(PAD_Y * this.zoom, 24); }
    pos(col, row) {
      if (this.vertical) {
        const t = Math.max(0, this.columns - 1 - col);
        return { x: this.ox + V_PAD_X + row * this.lane, y: this.oy + V_PAD_Y + t * this.step };
      }
      return { x: this.ox + PAD_X * this.zoom + col * this.step, y: this.oy + this.padY() + row * this.lane };
    }
    timeAt(x, y) {
      return this.vertical ? this.columns - 1 - (y - this.oy - V_PAD_Y) / this.step : (x - this.ox - PAD_X * this.zoom) / this.step;
    }
    laneAt(x, y) {
      return this.vertical ? (x - this.ox - V_PAD_X) / this.lane : (y - this.oy - this.padY()) / this.lane;
    }
    content(rows) {
      return this.vertical
        ? { w: V_PAD_X * 2 + rows * this.lane + V_TEXT_W, h: V_PAD_Y * 2 + this.columns * this.step }
        : { w: PAD_X * this.zoom * 2 + this.columns * this.step, h: this.padY() * 2 + rows * this.lane };
    }
    radius() { return this.vertical ? Math.min(8, Math.max(4.5, this.lane * 0.26)) : 7 * this.zoom; }
    curve(a, b) {
      if (this.vertical) {
        const dy = Math.max(a.y - b.y, this.step);
        return [a, { x: a.x, y: a.y - dy * 0.5 }, { x: b.x, y: b.y + dy * 0.5 }, b];
      }
      const dx = Math.max(b.x - a.x, this.step);
      return [a, { x: a.x + dx * 0.5, y: a.y }, { x: b.x - dx * 0.5, y: b.y }, b];
    }
  }

  function laneLabel(l) {
    let t = l.name;
    if (l.kind === 'inferred') t += ' (eliminada)';
    else if (l.kind === 'remote') t = '☁ ' + t;
    else if (l.kind === 'detached' && l.name === 'HEAD') t = 'HEAD separado';
    if (l.isCurrent) t = '● ' + t;
    return t;
  }

  function refStyle(kind, current) {
    switch (kind) {
      case 'tag': return [css('--yellow'), '⏷ '];
      case 'remote': return [css('--purple'), '☁ '];
      case 'head': return [css('--red'), ''];
      default: return [css('--accent'), current ? '✓ ' : ''];
    }
  }

  class Explorer {
    constructor(canvas, host) {
      this.canvas = canvas;
      this.host = host; // { getState(), select(hash), toggleCompare(hash), dblclick(hash), contextMenu(x, y, hash), setZoom(z) }
      this.offset = { x: 0, y: 0 };
      this.initialized = false;
      this.lastZoom = null;
      this.centerOn = null;
      this.hovered = null;
      this.overNode = false;
      this.drag = null;
      this.tip = document.getElementById('tooltip');
      this.bind();
    }

    reset() { this.initialized = false; }

    bind() {
      const c = this.canvas;
      c.addEventListener('pointerdown', (e) => {
        if (e.button !== 0) return;
        this.drag = { x: e.clientX, y: e.clientY, moved: false };
        c.setPointerCapture(e.pointerId);
      });
      c.addEventListener('pointermove', (e) => {
        const r = c.getBoundingClientRect();
        this.mouse = { x: e.clientX - r.left, y: e.clientY - r.top, cx: e.clientX, cy: e.clientY };
        if (this.drag) {
          const dx = e.clientX - this.drag.x, dy = e.clientY - this.drag.y;
          if (Math.abs(dx) + Math.abs(dy) > 2) this.drag.moved = true;
          if (this.drag.moved) {
            this.offset.x -= dx;
            this.offset.y -= dy;
            this.drag.x = e.clientX;
            this.drag.y = e.clientY;
          }
        }
        this.draw();
      });
      c.addEventListener('pointerup', (e) => {
        const d = this.drag;
        this.drag = null;
        if (d && !d.moved && this.hovered !== null) {
          const h = this.hashOf(this.hovered);
          if ((e.ctrlKey || e.metaKey) && h !== 'WORKTREE') this.host.toggleCompare(h);
          else this.host.select(h);
        }
        this.draw();
      });
      c.addEventListener('pointerleave', () => {
        this.mouse = null;
        this.tip.classList.add('hidden');
        this.draw();
      });
      c.addEventListener('dblclick', () => {
        if (this.hovered !== null) this.host.dblclick(this.hashOf(this.hovered));
      });
      c.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        this.host.contextMenu(e.clientX, e.clientY, this.hovered === null ? null : this.hashOf(this.hovered));
      });
      c.addEventListener('wheel', (e) => {
        e.preventDefault();
        const st = this.host.getState();
        if (e.ctrlKey || e.metaKey) {
          const zoom = st.settings.zoom;
          const nz = Math.min(2.5, Math.max(0.4, zoom * (1 - e.deltaY * 0.002)));
          const k = nz / zoom;
          const r = c.getBoundingClientRect();
          const px = e.clientX - r.left, py = e.clientY - r.top;
          const sx = (px + this.offset.x) * k - px, sy = (py + this.offset.y) * k - py;
          this.offset = st.settings.vertical ? { x: sx, y: this.offset.y } : { x: sx, y: sy };
          this.lastZoom = nz;
          this.host.setZoom(nz);
        } else {
          this.offset.x += e.shiftKey ? e.deltaY : e.deltaX;
          this.offset.y += e.shiftKey ? 0 : e.deltaY;
        }
        this.draw();
      }, { passive: false });
      // the canvas fills its host (inset: 0), so observing it follows the host's size
      new ResizeObserver(() => c.isConnected && this.draw()).observe(c);
    }

    hashOf(i) {
      const st = this.host.getState();
      return i === st.layout.worktree ? 'WORKTREE' : st.commits[i].hash;
    }

    draw() {
      const st = this.host.getState();
      const canvas = this.canvas;
      const W = canvas.parentElement.clientWidth, H = canvas.parentElement.clientHeight;
      const dpr = window.devicePixelRatio || 1;
      if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
        canvas.width = Math.round(W * dpr);
        canvas.height = Math.round(H * dpr);
        canvas.style.width = W + 'px';
        canvas.style.height = H + 'px';
      }
      const g = canvas.getContext('2d');
      g.setTransform(dpr, 0, 0, dpr, 0, 0);
      g.fillStyle = css('--bg');
      g.fillRect(0, 0, W, H);
      const L = st.layout;
      if (!L.nodes.length) {
        this.text(g, 'Este repositorio todavía no tiene commits.', W / 2, H / 2, `14px ${FONT()}`, css('--muted'), 'center');
        return;
      }
      const zoom = st.settings.zoom, vertical = st.settings.vertical;
      const content = new Geo(0, 0, zoom, vertical, L.columns).content(L.rows);

      // ---- navigation
      if (!this.initialized) {
        this.offset = vertical ? { x: 0, y: 0 } : { x: Math.max(0, content.w - W), y: 0 };
        this.initialized = true;
        this.lastZoom = zoom;
      }
      if (this.lastZoom !== zoom) {
        const k = zoom / this.lastZoom;
        const cx = this.offset.x + W / 2, cy = this.offset.y + H / 2;
        this.offset = vertical ? { x: cx * k - W / 2, y: this.offset.y } : { x: cx * k - W / 2, y: cy * k - H / 2 };
        this.lastZoom = zoom;
      }
      const tmp = new Geo(-this.offset.x, -this.offset.y, zoom, vertical, L.columns);
      if (this.centerOn) {
        const i = this.centerOn === 'WORKTREE' ? L.worktree : st.index.get(this.centerOn);
        if (i !== undefined && i !== null) {
          const n = L.nodes[i];
          const p = tmp.pos(n.col, n.row);
          const px = p.x + this.offset.x, py = p.y + this.offset.y;
          this.offset = vertical ? { x: 0, y: py - H * 0.4 } : { x: px - W * 0.6, y: py - H * 0.4 };
        }
        this.centerOn = null;
      }
      this.offset.x = Math.min(Math.max(0, content.w - W), Math.max(0, this.offset.x));
      this.offset.y = Math.min(Math.max(0, content.h - H), Math.max(0, this.offset.y));
      const geo = new Geo(-this.offset.x, -this.offset.y, zoom, vertical, L.columns);

      const ta = geo.timeAt(0, 0), tb = geo.timeAt(W, H);
      const c0 = Math.max(0, Math.floor(Math.min(ta, tb)) - 2), c1 = Math.ceil(Math.max(ta, tb)) + 2;
      const visible = (c) => c >= c0 && c <= c1;
      const q = st.filter.trim().toLowerCase();
      const n = st.commits.length;
      const dim = (i) => {
        if (!q) return false;
        if (i >= n) return true;
        const cm = st.commits[i];
        const lane = L.lanes[L.nodes[i].lane];
        const refs = st.refMap.get(cm.hash) || [];
        return !(cm.subject.toLowerCase().includes(q) || cm.author.toLowerCase().includes(q) || cm.hash.startsWith(q) || refs.some((r) => r.name.toLowerCase().includes(q)) || lane.name.toLowerCase().includes(q));
      };

      // ---- hover
      const r = geo.radius();
      this.hovered = null;
      this.overNode = false;
      const byCol = new Map(L.nodes.map((nd, i) => [nd.col, i]));
      const grid = new Map(L.nodes.map((nd, i) => [nd.col + ',' + nd.row, i]));
      if (this.mouse && !(this.drag && this.drag.moved)) {
        const lc = Math.round(geo.timeAt(this.mouse.x, this.mouse.y));
        if (lc >= 0) {
          if (vertical) {
            const i = byCol.get(lc);
            if (i !== undefined) {
              this.hovered = i;
              const p = geo.pos(L.nodes[i].col, L.nodes[i].row);
              this.overNode = Math.hypot(p.x - this.mouse.x, p.y - this.mouse.y) <= r + 5;
            }
          } else {
            const lr = Math.round(geo.laneAt(this.mouse.x, this.mouse.y));
            const i = grid.get(lc + ',' + lr);
            if (i !== undefined) {
              const p = geo.pos(L.nodes[i].col, L.nodes[i].row);
              if (Math.hypot(p.x - this.mouse.x, p.y - this.mouse.y) <= r + 5) {
                this.hovered = i;
                this.overNode = true;
              }
            }
          }
        }
      }
      const hashOf = (i) => (i === L.worktree ? 'WORKTREE' : st.commits[i].hash);

      // ---- vertical: row highlights
      if (vertical) {
        for (const [col, i] of byCol) {
          if (!visible(col)) continue;
          const y = geo.pos(col, 0).y;
          const h = hashOf(i);
          let fill = null;
          if (st.selected === h) fill = css('--sel');
          else if (st.compareWith === h) fill = css('--yellow-soft');
          else if (this.hovered === i) fill = css('--hover');
          if (fill) {
            g.fillStyle = fill;
            g.fillRect(0, y - geo.step / 2, W, geo.step);
          }
        }
      }

      // ---- lanes
      const labels = [];
      const dark = document.documentElement.dataset.theme !== 'light';
      for (const lane of L.lanes) {
        if (lane.end < c0 || lane.start > c1) continue;
        const color = laneColor(lane.color);
        const a = geo.pos(lane.start, lane.row), b = geo.pos(lane.end, lane.row);
        let band;
        if (vertical) {
          const half = geo.lane * 0.4;
          band = { x0: a.x - half, y0: b.y - geo.step * 0.45, x1: a.x + half, y1: a.y + geo.step * 0.45 };
        } else {
          band = { x0: a.x - geo.step * 0.5, y0: a.y - BAND_TOP * zoom, x1: b.x + geo.step * 0.5, y1: b.y + BAND_BOTTOM * zoom };
        }
        const radius = vertical ? geo.lane * 0.4 : 6 * zoom;
        this.roundRect(g, band, radius);
        g.fillStyle = alpha(color, dark ? 26 : 20);
        g.fill();
        if (lane.kind === 'inferred') {
          g.setLineDash([4, 3]);
          g.strokeStyle = alpha(color, 90);
          g.lineWidth = 1;
          g.stroke();
          g.setLineDash([]);
        } else {
          g.strokeStyle = alpha(color, lane.isCurrent ? 160 : 80);
          g.lineWidth = lane.isCurrent ? 1.6 : 1;
          g.stroke();
        }
        if (vertical) continue;
        const labelX = Math.max(band.x0, 6);
        if (labelX < band.x1 - 8) {
          const size = (lane.isMain ? 12 : MIN_FONT) * Math.min(1.4, Math.max(1, zoom));
          const font = `${size}px ${FONT()}`;
          g.font = font;
          const tw = g.measureText(laneLabel(lane)).width, th = size * 1.3;
          // inside the band's top strip when it fits, otherwise just above the band
          const inside = 2 + th <= (BAND_TOP - 7) * zoom;
          const pos = inside ? { x: labelX + 5, y: band.y0 + 2 } : { x: labelX + 2, y: band.y0 - th - 1 };
          labels.push({ pos, tw, th, font, text: laneLabel(lane), color: lane.kind === 'inferred' ? alpha(color, 190) : color, clipX: Math.max(band.x1, labelX + 240) });
        }
      }

      // ---- edges
      const sw = vertical ? 2 : Math.max(1.2, 2 * zoom);
      for (const e of L.edges) {
        const pn = L.nodes[e.from], cn = L.nodes[e.to];
        if (Math.max(pn.col, cn.col) < c0 || Math.min(pn.col, cn.col) > c1) continue;
        const a = geo.pos(pn.col, pn.row), b = geo.pos(cn.col, cn.row);
        const faded = dim(e.from) && dim(e.to);
        g.lineWidth = sw;
        if (e.kind === 'line') {
          const color = laneColor(L.lanes[cn.lane].color);
          g.strokeStyle = e.to === L.worktree ? css('--yellow') : faded ? alpha(color, 60) : color;
          if (e.to === L.worktree) g.setLineDash([4, 3]);
          g.beginPath();
          g.moveTo(a.x, a.y);
          g.lineTo(b.x, b.y);
          g.stroke();
          g.setLineDash([]);
        } else {
          const color = e.kind === 'merge' ? css('--merge') : laneColor(L.lanes[cn.lane].color);
          const pts = geo.curve(a, b);
          g.strokeStyle = faded ? alpha(color, 60) : color;
          g.lineWidth = e.kind === 'merge' ? sw * 0.85 : sw;
          g.beginPath();
          g.moveTo(pts[0].x, pts[0].y);
          g.bezierCurveTo(pts[1].x, pts[1].y, pts[2].x, pts[2].y, pts[3].x, pts[3].y);
          g.stroke();
          if (e.kind === 'merge') {
            const dx = b.x - pts[2].x, dy = b.y - pts[2].y, len = Math.hypot(dx, dy) || 1;
            const ux = dx / len, uy = dy / len;
            const tip = { x: b.x - ux * (r + 1.5), y: b.y - uy * (r + 1.5) };
            const s = vertical ? 6 : 6 * Math.max(0.7, zoom);
            g.fillStyle = g.strokeStyle;
            g.beginPath();
            g.moveTo(tip.x, tip.y);
            g.lineTo(tip.x - ux * s - uy * s * 0.6, tip.y - uy * s + ux * s * 0.6);
            g.lineTo(tip.x - ux * s + uy * s * 0.6, tip.y - uy * s - ux * s * 0.6);
            g.fill();
          }
        }
      }

      // ---- nodes
      const ring = vertical ? 3 : 4 * zoom;
      L.nodes.forEach((nd, i) => {
        if (!visible(nd.col)) return;
        const p = geo.pos(nd.col, nd.row);
        if (p.y < -40 || p.y > H + 40 || p.x < -40 || p.x > W + 40) return;
        const lane = L.lanes[nd.lane];
        const color = laneColor(lane.color);
        const h = hashOf(i);
        if (i === L.worktree) {
          g.setLineDash([3, 2.5]);
          g.strokeStyle = css('--yellow');
          g.lineWidth = 1.8;
          g.beginPath();
          g.arc(p.x, p.y, r, 0, Math.PI * 2);
          g.stroke();
          g.setLineDash([]);
          if (!vertical && zoom >= 0.6) this.text(g, `${st.changeCount} cambios`, p.x + r + 6, p.y, `${MIN_FONT}px ${FONT()}`, css('--yellow'), 'left');
        } else {
          g.fillStyle = dim(i) ? alpha(color, 50) : color;
          g.strokeStyle = css('--bg');
          g.lineWidth = 2;
          g.beginPath();
          g.arc(p.x, p.y, r, 0, Math.PI * 2);
          g.fill();
          g.stroke();
          if (st.commits[i].parents.length > 1) {
            g.fillStyle = css('--bg');
            g.beginPath();
            g.arc(p.x, p.y, r * 0.38, 0, Math.PI * 2);
            g.fill();
          }
          if (st.refs.head === h) this.ring(g, p, r + ring, css('--text'), 2);
        }
        if (!vertical) {
          if (st.selected === h) this.ring(g, p, r + 7 * zoom, css('--accent'), 2);
          if (st.compareWith === h) this.ring(g, p, r + 7 * zoom, css('--yellow'), 2);
        }
        if (this.hovered === i && this.overNode) this.ring(g, p, r + 2, alpha(css('--text'), 140), 1.5);
        if (!vertical && zoom >= 0.7 && i !== L.worktree) {
          let y = p.y + BAND_BOTTOM * zoom + 12;
          for (const rf of (st.refMap.get(h) || []).filter((rf) => rf.name !== lane.name).slice(0, 2)) {
            const [col, prefix] = refStyle(rf.kind, rf.current);
            g.font = `${MIN_FONT}px ${FONT()}`;
            const w = g.measureText(prefix + rf.name).width + 10;
            this.badge(g, p.x - w / 2, y, prefix + rf.name, col);
            y += 20;
          }
        }
      });

      // ---- lane names, on top of everything
      for (const lb of labels) {
        g.save();
        g.beginPath();
        g.rect(lb.pos.x - 4, lb.pos.y - 1, lb.clipX - lb.pos.x + 4, lb.th + 2);
        g.clip();
        g.fillStyle = alpha(css('--bg'), 225);
        this.roundRect(g, { x0: lb.pos.x - 3, y0: lb.pos.y, x1: lb.pos.x + lb.tw + 3, y1: lb.pos.y + lb.th }, 3);
        g.fill();
        g.font = lb.font;
        g.fillStyle = lb.color;
        g.textBaseline = 'top';
        g.fillText(lb.text, lb.pos.x, lb.pos.y + 1);
        g.restore();
      }

      // ---- vertical: commit text next to the graph
      if (vertical) {
        const textX = Math.max(geo.ox + V_PAD_X + L.rows * geo.lane + 6, 8);
        const right = W - 10;
        const wide = right - textX > 620;
        const tips = new Map(L.lanes.filter((l) => l.kind === 'inferred').map((l) => [l.tip, l]));
        for (const [col, i] of byCol) {
          if (!visible(col)) continue;
          const y = geo.pos(col, 0).y;
          if (y < -V_ROW || y > H + V_ROW) continue;
          g.save();
          g.beginPath();
          g.rect(textX, y - geo.step / 2, right - textX, geo.step);
          g.clip();
          if (i === L.worktree) {
            this.text(g, `Cambios sin confirmar (${st.changeCount})`, textX, y, `13px ${FONT()}`, css('--yellow'), 'left');
            g.restore();
            continue;
          }
          const cm = st.commits[i];
          const faded = dim(i);
          let x = textX;
          const refs = st.refMap.get(cm.hash) || [];
          for (const rf of refs.slice(0, 3)) {
            const [col2, prefix] = refStyle(rf.kind, rf.current);
            x += this.badge(g, x, y, prefix + rf.name, col2);
          }
          if (refs.length > 3) x += this.badge(g, x, y, `+${refs.length - 3}`, css('--muted'));
          if (tips.has(i)) {
            const lane = tips.get(i);
            x += this.badge(g, x, y, laneLabel(lane), laneColor(lane.color));
          }
          const metaW = wide ? 340 : 0;
          g.save();
          g.beginPath();
          g.rect(x, y - geo.step / 2, Math.max(0, right - metaW - 10 - x), geo.step);
          g.clip();
          this.text(g, cm.subject, x + 2, y, `13px ${FONT()}`, faded ? alpha(css('--text'), 90) : css('--text'), 'left');
          g.restore();
          if (wide) {
            const muted = faded ? alpha(css('--muted'), 90) : css('--muted');
            g.save();
            g.beginPath();
            g.rect(right - metaW, y - geo.step / 2, 150, geo.step);
            g.clip();
            this.text(g, cm.author, right - metaW, y, `12.5px ${FONT()}`, faded ? muted : css('--text2'), 'left');
            g.restore();
            this.text(g, relTime(cm.time), right - 180, y, `12px ${FONT()}`, muted, 'left');
            this.text(g, cm.hash.slice(0, 8), right, y, `11.5px ${MONO()}`, muted, 'right');
          }
          g.restore();
        }
      }

      // ---- legend (horizontal only: vertically it would cover the last row)
      if (!vertical) {
        const legend = `${L.lanes.length} ramas · ${n} commits · arrastra para moverte, Ctrl+rueda para zoom, Ctrl+clic para marcar y comparar`;
        g.font = `${MIN_FONT}px ${FONT()}`;
        const lw = g.measureText(legend).width;
        g.fillStyle = alpha(css('--bg'), 220);
        g.fillRect(6, H - 28, lw + 8, 18);
        this.text(g, legend, 10, H - 19, `${MIN_FONT}px ${FONT()}`, css('--muted'), 'left');
      }

      // ---- tooltip (only over the node)
      this.canvas.style.cursor = this.drag && this.drag.moved ? 'grabbing' : this.hovered !== null ? 'pointer' : 'default';
      if (this.hovered !== null && this.overNode && this.mouse && !document.querySelector('.menu')) {
        const i = this.hovered;
        let html;
        if (i === L.worktree) html = `Cambios sin confirmar (${st.changeCount})`;
        else {
          const cm = st.commits[i];
          html = `<b>${esc(cm.subject)}</b><div class="muted">${esc(cm.author)} · ${relTime(cm.time)} (${fullTime(cm.time)})</div><div class="mono muted">${cm.hash.slice(0, 12)}</div><div class="small muted">Rama: ${esc(L.lanes[L.nodes[i].lane].name)}</div>`;
        }
        this.tip.innerHTML = html;
        this.tip.style.left = this.mouse.cx + 14 + 'px';
        this.tip.style.top = this.mouse.cy + 16 + 'px';
        this.tip.classList.remove('hidden');
      } else {
        this.tip.classList.add('hidden');
      }
    }

    // ---- drawing helpers
    text(g, s, x, y, font, color, align) {
      g.font = font;
      g.fillStyle = color;
      g.textAlign = align;
      g.textBaseline = 'middle';
      g.fillText(s, x, y);
      g.textAlign = 'left';
    }
    ring(g, p, rad, color, w) {
      g.strokeStyle = color;
      g.lineWidth = w;
      g.beginPath();
      g.arc(p.x, p.y, rad, 0, Math.PI * 2);
      g.stroke();
    }
    roundRect(g, b, r) {
      g.beginPath();
      g.roundRect(b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0, Math.min(r, (b.x1 - b.x0) / 2, (b.y1 - b.y0) / 2));
    }
    badge(g, x, cy, text, color) {
      g.font = `${MIN_FONT}px ${FONT()}`;
      const w = g.measureText(text).width + 10;
      this.roundRect(g, { x0: x, y0: cy - 9, x1: x + w, y1: cy + 9 }, 9);
      g.fillStyle = alpha(css('--bg'), 235);
      g.fill();
      g.fillStyle = color.startsWith('#') ? alpha(color, 40) : color;
      g.fill();
      this.text(g, text, x + 5, cy, `${MIN_FONT}px ${FONT()}`, color, 'left');
      return w + 4;
    }
  }

  window.Explorer = Explorer;
})();
