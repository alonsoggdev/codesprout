//! Branch explorer canvas (Plastic SCM style): one lane per branch, forks drawn from the
//! parent commit into the new lane and merges drawn as green arrows into the merge commit.
//!
//! Two orientations:
//! - horizontal: lanes are rows and time flows left to right; the lane name sits inside the band.
//! - vertical: lanes are columns and time flows top (newest) to bottom, one commit per row,
//!   with the commit's message, refs, author and date written next to the graph.
//!
//! Text is never rotated nor scaled below a readable size: zooming changes the spacing of the
//! graph, not the size of the labels.

use std::sync::Arc;

use codesprout_core::explorer::{EdgeKind, LaneKind, Layout};
use eframe::egui::{self, epaint::CubicBezierShape, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke};

use super::Defer;
use crate::app::App;
use crate::repo_state::{RefKind, WORKTREE};
use crate::theme;
use crate::widgets;

// horizontal
const COL_W: f32 = 34.0;
const ROW_H: f32 = 72.0;
/// Band extent above and below the node centre line (the top part holds the lane name).
const BAND_TOP: f32 = 25.0;
const BAND_BOTTOM: f32 = 12.0;
const PAD_X: f32 = 60.0;
const PAD_Y: f32 = 40.0;
// vertical
const V_ROW: f32 = 28.0;
const V_LANE: f32 = 24.0;
const V_PAD_X: f32 = 22.0;
const V_PAD_Y: f32 = 18.0;
const V_TEXT_W: f32 = 900.0;
/// Smallest font used for any label.
const MIN_FONT: f32 = 11.0;

struct Geo {
    origin: Pos2,
    zoom: f32,
    vertical: bool,
    columns: u32,
    /// Spacing along the time axis.
    step: f32,
    /// Spacing between lanes.
    lane: f32,
}

impl Geo {
    fn new(origin: Pos2, zoom: f32, vertical: bool, columns: u32) -> Self {
        if vertical {
            Self { origin, zoom, vertical, columns, step: V_ROW, lane: (V_LANE * zoom).clamp(14.0, 48.0) }
        } else {
            Self { origin, zoom, vertical, columns, step: COL_W * zoom, lane: ROW_H * zoom }
        }
    }

    fn pos(&self, col: u32, row: u32) -> Pos2 {
        if self.vertical {
            let t = self.columns.saturating_sub(1).saturating_sub(col) as f32;
            pos2(self.origin.x + V_PAD_X + row as f32 * self.lane, self.origin.y + V_PAD_Y + t * self.step)
        } else {
            pos2(self.origin.x + PAD_X * self.zoom + col as f32 * self.step, self.origin.y + self.pad_y() + row as f32 * self.lane)
        }
    }

    /// Fractional column under a screen point.
    fn time_at(&self, p: Pos2) -> f32 {
        if self.vertical {
            self.columns as f32 - 1.0 - (p.y - self.origin.y - V_PAD_Y) / self.step
        } else {
            (p.x - self.origin.x - PAD_X * self.zoom) / self.step
        }
    }

    /// Fractional lane row under a screen point.
    fn lane_at(&self, p: Pos2) -> f32 {
        if self.vertical {
            (p.x - self.origin.x - V_PAD_X) / self.lane
        } else {
            (p.y - self.origin.y - self.pad_y()) / self.lane
        }
    }

    fn content(&self, rows: u32) -> egui::Vec2 {
        if self.vertical {
            vec2(V_PAD_X * 2.0 + rows as f32 * self.lane + V_TEXT_W, V_PAD_Y * 2.0 + self.columns as f32 * self.step)
        } else {
            vec2(PAD_X * self.zoom * 2.0 + self.columns as f32 * self.step, self.pad_y() * 2.0 + rows as f32 * self.lane)
        }
    }

    /// Top margin; it never shrinks below the height of a lane name drawn above its band.
    fn pad_y(&self) -> f32 {
        (PAD_Y * self.zoom).max(24.0)
    }

    fn node_radius(&self) -> f32 {
        if self.vertical {
            (self.lane * 0.26).clamp(4.5, 8.0)
        } else {
            7.0 * self.zoom
        }
    }

    /// Control points of a fork / merge connection from `a` (older) to `b` (newer).
    fn curve(&self, a: Pos2, b: Pos2) -> [Pos2; 4] {
        if self.vertical {
            let dy = (a.y - b.y).max(self.step);
            [a, pos2(a.x, a.y - dy * 0.5), pos2(b.x, b.y + dy * 0.5), b]
        } else {
            let dx = (b.x - a.x).max(self.step);
            [a, pos2(a.x + dx * 0.5, a.y), pos2(b.x - dx * 0.5, b.y), b]
        }
    }
}

/// Rounded label; returns the width used.
fn badge(p: &Painter, x: f32, cy: f32, text: &str, color: Color32) -> f32 {
    let c = theme::p();
    let g = p.layout_no_wrap(text.to_string(), FontId::proportional(MIN_FONT), color);
    let r = Rect::from_min_size(pos2(x, cy - 9.0), vec2(g.size().x + 10.0, 18.0));
    p.rect_filled(r, 9.0, theme::with_alpha(c.bg, 235));
    p.rect_filled(r, 9.0, theme::with_alpha(color, 40));
    p.galley(pos2(x + 5.0, cy - g.size().y / 2.0), g, color);
    r.width() + 4.0
}

fn ref_style(kind: &RefKind) -> (Color32, &'static str) {
    let c = theme::p();
    match kind {
        RefKind::Tag => (c.yellow, "⏷ "),
        RefKind::Remote => (c.purple, "☁ "),
        RefKind::Head => (c.red, ""),
        RefKind::Branch { current: true } => (c.accent, "✓ "),
        RefKind::Branch { .. } => (c.accent, ""),
    }
}

pub fn show(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let rect = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, c.bg);

    let zoom = app.store.settings.zoom;
    let vertical = app.store.settings.vertical;
    let t = &mut app.tabs[idx];
    if !t.loaded {
        painter.text(rect.center(), Align2::CENTER_CENTER, "Cargando historial…", FontId::proportional(14.0), c.muted);
        return;
    }
    let layout: Arc<Layout> = t.layout.clone();
    if layout.nodes.is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, "Este repositorio todavía no tiene commits.", FontId::proportional(14.0), c.muted);
        return;
    }
    let ptr = Arc::as_ptr(&layout) as usize;
    if t.explorer.grid_for != ptr {
        t.explorer.grid = layout.nodes.iter().enumerate().map(|(i, n)| ((n.col, n.row), i)).collect();
        t.explorer.by_col = layout.nodes.iter().enumerate().map(|(i, n)| (n.col, i)).collect();
        t.explorer.grid_for = ptr;
    }
    let content = Geo::new(Pos2::ZERO, zoom, vertical, layout.columns).content(layout.rows);

    // ---- navigation
    let st = &mut t.explorer;
    if !st.initialized {
        // newest commits in view: right end when horizontal, top when vertical
        st.offset = if vertical { vec2(0.0, 0.0) } else { vec2((content.x - rect.width()).max(0.0), 0.0) };
        st.initialized = true;
        st.last_zoom = zoom;
    }
    if (st.last_zoom - zoom).abs() > f32::EPSILON {
        // keep the view centre steady when zooming with the buttons
        let k = zoom / st.last_zoom;
        let centre = st.offset + rect.size() / 2.0;
        st.offset = if vertical { vec2(centre.x * k - rect.width() / 2.0, st.offset.y) } else { centre * k - rect.size() / 2.0 };
        st.last_zoom = zoom;
    }
    if resp.dragged() {
        st.offset -= resp.drag_delta();
    }
    let mut new_zoom = zoom;
    if resp.hovered() {
        let (scroll, zd, mods) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers));
        if mods.command || (zd - 1.0).abs() > 1e-3 {
            let factor = if (zd - 1.0).abs() > 1e-3 { zd } else { 1.0 + scroll.y * 0.002 };
            new_zoom = (zoom * factor).clamp(0.4, 2.5);
            if let Some(p) = resp.hover_pos() {
                let k = new_zoom / zoom;
                let anchor = p - rect.min + st.offset;
                let scaled = anchor * k - (p - rect.min);
                // vertically the rows keep their height: only the lanes are scaled
                st.offset = if vertical { vec2(scaled.x, st.offset.y) } else { scaled };
                st.last_zoom = new_zoom;
            }
        } else {
            st.offset -= scroll;
        }
    }
    let max_off = (content - rect.size()).max(vec2(0.0, 0.0));
    let geo_tmp = Geo::new(rect.min - st.offset, zoom, vertical, layout.columns);
    if let Some(h) = st.center_on.take() {
        let i = if h == WORKTREE { layout.worktree } else { t.commit_index.get(&h).copied() };
        if let Some(i) = i {
            let n = layout.nodes[i];
            let p = geo_tmp.pos(n.col, n.row) - geo_tmp.origin;
            st.offset = if vertical { vec2(0.0, p.y - rect.height() * 0.4) } else { vec2(p.x - rect.width() * 0.6, p.y - rect.height() * 0.4) };
        }
    }
    st.offset = st.offset.clamp(vec2(0.0, 0.0), max_off);
    if (new_zoom - zoom).abs() > f32::EPSILON {
        app.store.settings.zoom = new_zoom;
        defer.push(Box::new(|a| a.persist()));
    }
    let t = &mut app.tabs[idx];
    let geo = Geo::new(rect.min - t.explorer.offset, zoom, vertical, layout.columns);

    // visible range of columns (time axis)
    let (ta, tb) = (geo.time_at(rect.min), geo.time_at(rect.max));
    let c0 = (ta.min(tb).floor() - 2.0).max(0.0) as u32;
    let c1 = (ta.max(tb).ceil() + 2.0).max(0.0) as u32;
    let visible = |col: u32| col >= c0 && col <= c1;

    let q = t.explorer.filter.trim().to_lowercase();
    let n_commits = t.commits.len();
    let dim = |i: usize| -> bool {
        if q.is_empty() {
            return false;
        }
        if i >= n_commits {
            return true;
        }
        let lane = &layout.lanes[layout.nodes[i].lane];
        !(super::history::matches(t, &t.commits[i], &q) || lane.name.to_lowercase().contains(&q))
    };

    // ---- hovered node: by grid cell (horizontal) or by row (vertical)
    let r = geo.node_radius();
    let hover_pos = resp.hover_pos();
    let mut hovered: Option<usize> = None;
    let mut over_node = false;
    if let Some(p) = hover_pos {
        let lc = geo.time_at(p).round();
        if lc >= 0.0 {
            if vertical {
                if let Some(&i) = t.explorer.by_col.get(&(lc as u32)) {
                    hovered = Some(i);
                    let n = layout.nodes[i];
                    over_node = geo.pos(n.col, n.row).distance(p) <= r + 5.0;
                }
            } else {
                let lr = geo.lane_at(p).round();
                if lr >= 0.0 {
                    if let Some(&i) = t.explorer.grid.get(&(lc as u32, lr as u32)) {
                        let n = layout.nodes[i];
                        if geo.pos(n.col, n.row).distance(p) <= r + 5.0 {
                            hovered = Some(i);
                            over_node = true;
                        }
                    }
                }
            }
        }
    }
    let head = t.refs.as_ref().and_then(|r| r.head.clone());
    let selected = t.selected.clone();
    let compare = t.compare_with.clone();
    let hash_of = |i: usize| if Some(i) == layout.worktree { WORKTREE } else { t.commits[i].hash.as_str() };

    // ---- vertical: row highlights behind everything
    if vertical {
        for (&col, &i) in t.explorer.by_col.iter().filter(|(col, _)| visible(**col)) {
            let y = geo.pos(col, 0).y;
            let row = Rect::from_min_max(pos2(rect.min.x, y - geo.step / 2.0), pos2(rect.max.x, y + geo.step / 2.0));
            let h = hash_of(i);
            if selected.as_deref() == Some(h) {
                painter.rect_filled(row, 0.0, c.sel);
            } else if compare.as_deref() == Some(h) {
                painter.rect_filled(row, 0.0, c.yellow_soft);
            } else if hovered == Some(i) {
                painter.rect_filled(row, 0.0, c.hover);
            }
        }
    }

    // ---- lanes (bands; horizontal lanes also get their name, painted last)
    let mut labels = Vec::new();
    for lane in &layout.lanes {
        if lane.end < c0 || lane.start > c1 {
            continue;
        }
        let color = theme::lane_color(lane.color);
        let a = geo.pos(lane.start, lane.row);
        let b = geo.pos(lane.end, lane.row);
        let band = if vertical {
            let half = geo.lane * 0.4;
            Rect::from_min_max(pos2(a.x - half, b.y - geo.step * 0.45), pos2(a.x + half, a.y + geo.step * 0.45))
        } else {
            Rect::from_min_max(pos2(a.x - geo.step * 0.5, a.y - BAND_TOP * zoom), pos2(b.x + geo.step * 0.5, b.y + BAND_BOTTOM * zoom))
        };
        let radius = if vertical { geo.lane * 0.4 } else { 6.0 * zoom };
        let alpha = if theme::is_dark() { 26 } else { 20 };
        painter.rect_filled(band, radius, theme::with_alpha(color, alpha));
        if lane.kind == LaneKind::Inferred {
            let pts = [band.left_top(), band.right_top(), band.right_bottom(), band.left_bottom(), band.left_top()];
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0_f32, theme::with_alpha(color, 90)), 4.0, 3.0));
        } else {
            painter.rect_stroke(band, radius, Stroke::new(if lane.is_current { 1.6_f32 } else { 1.0 }, theme::with_alpha(color, if lane.is_current { 160 } else { 80 })), egui::StrokeKind::Inside);
        }
        if vertical {
            continue;
        }
        // label inside the band, sticky to the left edge of the view; it may run past the end
        // of a short band (it gets its own background then)
        let label_x = band.min.x.max(rect.min.x + 6.0);
        if label_x < band.max.x - 8.0 {
            let font = FontId::proportional((if lane.is_main { 12.0 } else { MIN_FONT }) * zoom.clamp(1.0, 1.4));
            let galley = painter.layout_no_wrap(lane_label(lane), font, if lane.kind == LaneKind::Inferred { theme::with_alpha(color, 190) } else { color });
            // inside the band's top strip when it fits, otherwise just above the band
            // it fits when it ends above the nodes (centre line minus the node radius)
            let inside = 2.0 + galley.size().y <= (BAND_TOP - 7.0) * zoom;
            let pos = if inside { pos2(label_x + 5.0, band.min.y + 2.0) } else { pos2(label_x + 2.0, band.min.y - galley.size().y - 1.0) };
            let clip = Rect::from_min_max(pos2(label_x - 1.0, pos.y - 1.0), pos2(band.max.x.max(label_x + 240.0), pos.y + galley.size().y + 1.0));
            let bg = Rect::from_min_size(pos - vec2(3.0, 0.0), galley.size() + vec2(6.0, 0.0));
            labels.push((clip.intersect(rect), bg, pos, galley, color));
        }
    }

    // ---- edges
    let stroke_w = if vertical { 2.0 } else { (2.0 * zoom).max(1.2) };
    for e in &layout.edges {
        let (pn, cn) = (layout.nodes[e.from], layout.nodes[e.to]);
        let (lo, hi) = (pn.col.min(cn.col), pn.col.max(cn.col));
        if hi < c0 || lo > c1 {
            continue;
        }
        let a = geo.pos(pn.col, pn.row);
        let b = geo.pos(cn.col, cn.row);
        let faded = dim(e.from) && dim(e.to);
        match e.kind {
            EdgeKind::Line => {
                let color = theme::lane_color(layout.lanes[cn.lane].color);
                let color = if faded { theme::with_alpha(color, 60) } else { color };
                if Some(e.to) == layout.worktree {
                    painter.extend(Shape::dashed_line(&[a, b], Stroke::new(stroke_w, c.yellow), 4.0, 3.0));
                } else {
                    painter.line_segment([a, b], Stroke::new(stroke_w, color));
                }
            }
            EdgeKind::Branch => {
                let color = theme::lane_color(layout.lanes[cn.lane].color);
                let color = if faded { theme::with_alpha(color, 60) } else { color };
                painter.add(CubicBezierShape::from_points_stroke(geo.curve(a, b), false, Color32::TRANSPARENT, Stroke::new(stroke_w, color)));
            }
            EdgeKind::Merge => {
                let color = if faded { theme::with_alpha(c.merge, 60) } else { c.merge };
                let pts = geo.curve(a, b);
                painter.add(CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(stroke_w * 0.85, color)));
                // arrow head touching the merge commit
                let dir = (b - pts[2]).normalized();
                let tip = b - dir * (r + 1.5);
                let side = vec2(-dir.y, dir.x);
                let s = if vertical { 6.0 } else { 6.0 * zoom.max(0.7) };
                painter.add(Shape::convex_polygon(vec![tip, tip - dir * s + side * s * 0.6, tip - dir * s - side * s * 0.6], color, Stroke::NONE));
            }
        }
    }

    // ---- nodes
    let wt_count = t.change_count();
    let ring = if vertical { 3.0 } else { 4.0 * zoom };
    for (i, n) in layout.nodes.iter().enumerate() {
        if !visible(n.col) {
            continue;
        }
        let p = geo.pos(n.col, n.row);
        if p.y < rect.min.y - 40.0 || p.y > rect.max.y + 40.0 || p.x < rect.min.x - 40.0 || p.x > rect.max.x + 40.0 {
            continue;
        }
        let lane = &layout.lanes[n.lane];
        let color = theme::lane_color(lane.color);
        let h = hash_of(i);
        if Some(i) == layout.worktree {
            let pts: Vec<Pos2> = (0..=24).map(|k| p + vec2((k as f32 / 24.0 * std::f32::consts::TAU).cos(), (k as f32 / 24.0 * std::f32::consts::TAU).sin()) * r).collect();
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.8_f32, c.yellow), 3.0, 2.5));
            if !vertical && zoom >= 0.6 {
                painter.text(p + vec2(r + 6.0, 0.0), Align2::LEFT_CENTER, format!("{wt_count} cambios"), FontId::proportional(MIN_FONT), c.yellow);
            }
        } else {
            let fill = if dim(i) { theme::with_alpha(color, 50) } else { color };
            painter.circle(p, r, fill, Stroke::new(2.0_f32, c.bg));
            if t.commits[i].parents.len() > 1 {
                painter.circle_filled(p, r * 0.38, c.bg);
            }
            if head.as_deref() == Some(h) {
                painter.circle_stroke(p, r + ring, Stroke::new(2.0_f32, c.text));
            }
        }
        if !vertical {
            // vertically the whole row is highlighted instead
            if selected.as_deref() == Some(h) {
                painter.circle_stroke(p, r + 7.0 * zoom, Stroke::new(2.0_f32, c.accent));
            }
            if compare.as_deref() == Some(h) {
                painter.circle_stroke(p, r + 7.0 * zoom, Stroke::new(2.0_f32, c.yellow));
            }
        }
        if hovered == Some(i) && over_node {
            painter.circle_stroke(p, r + 2.0, Stroke::new(1.5_f32, theme::with_alpha(c.text, 140)));
        }
        // horizontal: ref badges under the node (tags, tips that differ from the lane name)
        if !vertical && zoom >= 0.7 && Some(i) != layout.worktree {
            if let Some(refs) = t.ref_map.get(h) {
                let mut y = p.y + BAND_BOTTOM * zoom + 12.0;
                for rf in refs.iter().filter(|rf| rf.name != lane.name).take(2) {
                    let (col, prefix) = ref_style(&rf.kind);
                    let text = format!("{prefix}{}", rf.name);
                    let w = painter.layout_no_wrap(text.clone(), FontId::proportional(MIN_FONT), col).size().x + 10.0;
                    badge(&painter, p.x - w / 2.0, y, &text, col);
                    y += 20.0;
                }
            }
        }
    }

    for (clip, bg, pos, galley, color) in labels {
        let p = painter.with_clip_rect(clip);
        p.rect_filled(bg, 3.0, theme::with_alpha(c.bg, 225));
        p.galley(pos, galley, color);
    }

    // ---- vertical: commit text next to the graph
    if vertical {
        let text_x = (geo.origin.x + V_PAD_X + layout.rows as f32 * geo.lane + 6.0).max(rect.min.x + 8.0);
        let right = rect.max.x - 10.0;
        let wide = right - text_x > 620.0;
        let tips: std::collections::HashMap<usize, &codesprout_core::explorer::Lane> =
            layout.lanes.iter().filter(|l| l.kind == LaneKind::Inferred).map(|l| (l.tip, l)).collect();
        for (&col, &i) in t.explorer.by_col.iter().filter(|(col, _)| visible(**col)) {
            let y = geo.pos(col, 0).y;
            if y < rect.min.y - V_ROW || y > rect.max.y + V_ROW {
                continue;
            }
            let row_clip = Rect::from_min_max(pos2(text_x, y - geo.step / 2.0), pos2(right, y + geo.step / 2.0)).intersect(rect);
            let p = painter.with_clip_rect(row_clip);
            if Some(i) == layout.worktree {
                p.text(pos2(text_x, y), Align2::LEFT_CENTER, format!("Cambios sin confirmar ({wt_count})"), FontId::proportional(13.0), c.yellow);
                continue;
            }
            let cm = &t.commits[i];
            let faded = dim(i);
            let mut x = text_x;
            if let Some(refs) = t.ref_map.get(&cm.hash) {
                for rf in refs.iter().take(3) {
                    let (col, prefix) = ref_style(&rf.kind);
                    x += badge(&p, x, y, &format!("{prefix}{}", rf.name), col);
                }
                if refs.len() > 3 {
                    x += badge(&p, x, y, &format!("+{}", refs.len() - 3), c.muted);
                }
            }
            if let Some(lane) = tips.get(&i) {
                x += badge(&p, x, y, &lane_label(lane), theme::lane_color(lane.color));
            }
            let meta_w = if wide { 340.0 } else { 0.0 };
            let subject_clip = Rect::from_min_max(pos2(x, row_clip.min.y), pos2((right - meta_w - 10.0).max(x), row_clip.max.y)).intersect(rect);
            let text_color = if faded { theme::with_alpha(c.text, 90) } else { c.text };
            painter.with_clip_rect(subject_clip).text(pos2(x + 2.0, y), Align2::LEFT_CENTER, &cm.subject, FontId::proportional(13.0), text_color);
            if wide {
                let muted = if faded { theme::with_alpha(c.muted, 90) } else { c.muted };
                let author_clip = Rect::from_min_max(pos2(right - meta_w, row_clip.min.y), pos2(right - meta_w + 150.0, row_clip.max.y)).intersect(rect);
                painter.with_clip_rect(author_clip).text(pos2(right - meta_w, y), Align2::LEFT_CENTER, &cm.author, FontId::proportional(12.5), if faded { muted } else { c.text2 });
                p.text(pos2(right - 180.0, y), Align2::LEFT_CENTER, widgets::rel_time(cm.time), FontId::proportional(12.0), muted);
                p.text(pos2(right, y), Align2::RIGHT_CENTER, &cm.hash[..8], theme::mono(11.5), muted);
            }
        }
    }

    // ---- minimal legend (vertically it would cover the last row's text)
    if !vertical {
        let legend = format!("{} ramas · {} commits · arrastra para moverte, Ctrl+rueda para zoom, Ctrl+clic para marcar y comparar", layout.lanes.len(), n_commits);
        let lg = painter.layout_no_wrap(legend, FontId::proportional(MIN_FONT), c.muted);
        let lpos = rect.left_bottom() + vec2(10.0, -10.0 - lg.size().y);
        painter.rect_filled(Rect::from_min_size(lpos - vec2(4.0, 2.0), lg.size() + vec2(8.0, 4.0)), 4.0, theme::with_alpha(c.bg, 220));
        painter.galley(lpos, lg, c.muted);
    }

    // ---- interaction
    let menu_open = resp.context_menu_opened();
    // vertically the row already shows the details: the tooltip appears only over the node
    if let Some(i) = hovered.filter(|_| !menu_open && over_node) {
        let tip_id = ui.id().with(("explorer-tip", i));
        if Some(i) == layout.worktree {
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), tip_id, |ui| {
                ui.label(format!("Cambios sin confirmar ({wt_count})"));
            });
        } else {
            let cm = t.commits[i].clone();
            let lane = layout.lanes[layout.nodes[i].lane].name.clone();
            let refs = t.ref_map.get(&cm.hash).cloned();
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), tip_id, |ui| {
                ui.set_max_width(420.0);
                ui.label(egui::RichText::new(&cm.subject).strong());
                ui.label(widgets::muted(format!("{} · {} ({})", cm.author, widgets::rel_time(cm.time), widgets::full_time(cm.time))));
                ui.label(egui::RichText::new(&cm.hash[..12]).font(theme::mono(11.5)).color(c.muted));
                ui.label(widgets::small(format!("Rama: {lane}")));
                if refs.is_some() {
                    ui.horizontal_wrapped(|ui| super::history::ref_badges(ui, refs.as_ref()));
                }
            });
        }
    }
    if hovered.is_some() && !resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if resp.clicked() {
        if let Some(i) = hovered {
            let h = hash_of(i).to_string();
            if ui.input(|i| i.modifiers.command) && h != WORKTREE {
                t.compare_with = if t.compare_with.as_deref() == Some(h.as_str()) { None } else { Some(h) };
            } else if h == WORKTREE {
                t.selected = Some(h);
            } else {
                defer.push(Box::new(move |a| a.select_commit(h, false)));
            }
        }
    }
    if resp.double_clicked() {
        if let Some(i) = hovered {
            let h = hash_of(i).to_string();
            defer.push(Box::new(move |a| super::history::double_click_commit(a, h)));
        }
    }
    if resp.secondary_clicked() {
        let id = ui.id().with("ctx-node");
        ui.ctx().data_mut(|d| d.insert_temp(id, hovered));
    }
    let menu_node: Option<usize> = ui.ctx().data(|d| d.get_temp(ui.id().with("ctx-node"))).flatten();
    let menu_commit = menu_node.filter(|&i| i < n_commits).map(|i| t.commits[i].clone());
    let menu_wt = menu_node.is_some() && menu_node == layout.worktree;
    resp.context_menu(|ui| {
        if let Some(cm) = &menu_commit {
            super::history::commit_menu(ui, cm, defer);
        } else if menu_wt {
            if ui.button("Ver cambios").clicked() {
                defer.push(Box::new(|a| a.set_view(crate::repo_state::View::Changes)));
                ui.close_menu();
            }
            if ui.button("Guardar en stash…").clicked() {
                defer.push(Box::new(|a| a.dialogs.push(crate::dialogs::Dialog::stash())));
                ui.close_menu();
            }
            if ui.button("Descartar todos los cambios…").clicked() {
                defer.push(Box::new(|a| a.open_discard_all()));
                ui.close_menu();
            }
        } else {
            if ui.button("Ir a HEAD").clicked() {
                if let Some(h) = head.clone() {
                    defer.push(Box::new(move |a| a.select_commit(h, true)));
                }
                ui.close_menu();
            }
            if ui.button("Nueva rama…").clicked() {
                defer.push(Box::new(|a| a.open_new_branch(None)));
                ui.close_menu();
            }
        }
    });
}

fn lane_label(lane: &codesprout_core::explorer::Lane) -> String {
    let mut text = lane.name.clone();
    match lane.kind {
        LaneKind::Inferred => text.push_str(" (eliminada)"),
        LaneKind::Remote => text = format!("☁ {text}"),
        LaneKind::Detached if lane.name == "HEAD" => text = "HEAD separado".into(),
        _ => {}
    }
    if lane.is_current {
        text = format!("● {text}");
    }
    text
}
