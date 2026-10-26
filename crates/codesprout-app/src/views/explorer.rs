//! History graph, laid out like Sourcetree: a compact graph column and then description, date,
//! author and commit columns under a header, one commit per row, newest on top. Forks are drawn
//! from the parent commit into the new lane and merges as green arrows into the merge commit.
//!
//! Zooming (Ctrl+wheel) changes the spacing between lanes, never the size of the text.

use std::sync::Arc;

use codesprout_core::explorer::{EdgeKind, LaneKind, Layout};
use eframe::egui::{self, epaint::CubicBezierShape, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke};

use super::Defer;
use crate::app::App;
use crate::repo_state::{RefKind, WORKTREE};
use crate::theme;
use crate::widgets;

const ROW: f32 = 24.0;
const LANE: f32 = 16.0;
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 12.0;
/// Room the columns after the graph need before the view scrolls sideways.
const TEXT_W: f32 = 420.0;
/// Column header strip.
const HEADER: f32 = 24.0;
/// Font of the ref badges.
const BADGE_FONT: f32 = 11.0;

struct Geo {
    origin: Pos2,
    columns: u32,
    /// Spacing between lanes.
    lane: f32,
}

impl Geo {
    fn new(origin: Pos2, zoom: f32, columns: u32) -> Self {
        Self { origin, columns, lane: (LANE * zoom).clamp(10.0, 40.0) }
    }

    /// Centre of a node: `col` is its position in time (newest = highest), `row` its lane.
    fn pos(&self, col: u32, row: u32) -> Pos2 {
        let t = self.columns.saturating_sub(1).saturating_sub(col) as f32;
        pos2(self.origin.x + PAD_X + row as f32 * self.lane, self.origin.y + PAD_Y + t * ROW)
    }

    /// Fractional column under a screen y.
    fn time_at(&self, y: f32) -> f32 {
        self.columns as f32 - 1.0 - (y - self.origin.y - PAD_Y) / ROW
    }

    fn content(&self, rows: u32) -> egui::Vec2 {
        vec2(PAD_X * 2.0 + rows as f32 * self.lane + TEXT_W, PAD_Y * 2.0 + self.columns as f32 * ROW)
    }

    fn node_radius(&self) -> f32 {
        (self.lane * 0.25).clamp(3.5, 6.0)
    }

    /// Control points of a fork / merge connection from `a` (older) to `b` (newer).
    fn curve(&self, a: Pos2, b: Pos2) -> [Pos2; 4] {
        let dy = (a.y - b.y).max(ROW);
        [a, pos2(a.x, a.y - dy * 0.5), pos2(b.x, b.y + dy * 0.5), b]
    }
}

/// x of each column after the graph, shared by the header and the rows.
struct Columns {
    text: f32,
    date: f32,
    author: f32,
    hash: f32,
    right: f32,
    /// Date, author and commit fit next to the description.
    meta: bool,
}

impl Columns {
    const GAP: f32 = 14.0;

    fn new(text: f32, right: f32) -> Self {
        let hash = right - 64.0;
        let author = hash - Self::GAP - 150.0;
        let date = author - Self::GAP - 112.0;
        Self { text, date, author, hash, right, meta: date - text > 220.0 }
    }

    fn description_end(&self) -> f32 {
        if self.meta { self.date - Self::GAP } else { self.right }
    }
}

/// Rounded label; returns the width used.
fn badge(p: &Painter, x: f32, cy: f32, text: &str, color: Color32) -> f32 {
    let c = theme::p();
    let g = p.layout_no_wrap(text.to_string(), FontId::proportional(BADGE_FONT), color);
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
    let zoom = app.store.settings.zoom;
    let full = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(full, Sense::click_and_drag());
    ui.painter().rect_filled(full, 0.0, c.bg);
    // the column header stays on top while the rows scroll under it
    let header = Rect::from_min_max(full.min, pos2(full.max.x, full.min.y + HEADER));
    let rect = Rect::from_min_max(pos2(full.min.x, header.max.y), full.max);
    let painter = ui.painter().with_clip_rect(rect);
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
        t.explorer.by_col = layout.nodes.iter().enumerate().map(|(i, n)| (n.col, i)).collect();
        t.explorer.grid_for = ptr;
    }
    let content = Geo::new(Pos2::ZERO, zoom, layout.columns).content(layout.rows);

    // ---- navigation
    let st = &mut t.explorer;
    if !st.initialized {
        // newest commits (top) in view
        st.offset = vec2(0.0, 0.0);
        st.initialized = true;
        st.last_zoom = zoom;
    }
    if (st.last_zoom - zoom).abs() > f32::EPSILON {
        // keep the view centre steady when zooming with the buttons
        let k = zoom / st.last_zoom;
        let centre = st.offset.x + rect.width() / 2.0;
        st.offset.x = centre * k - rect.width() / 2.0;
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
                // the rows keep their height: only the lanes are scaled
                let anchor = p.x - rect.min.x + st.offset.x;
                st.offset.x = anchor * new_zoom / zoom - (p.x - rect.min.x);
                st.last_zoom = new_zoom;
            }
        } else {
            st.offset -= scroll;
        }
    }
    let max_off = (content - rect.size()).max(vec2(0.0, 0.0));
    if let Some(h) = st.center_on.take() {
        let i = if h == WORKTREE { layout.worktree } else { t.commit_index.get(&h).copied() };
        if let Some(i) = i {
            let geo = Geo::new(Pos2::ZERO, zoom, layout.columns);
            let n = layout.nodes[i];
            st.offset = vec2(0.0, geo.pos(n.col, n.row).y - rect.height() * 0.4);
        }
    }
    st.offset = st.offset.clamp(vec2(0.0, 0.0), max_off);
    if (new_zoom - zoom).abs() > f32::EPSILON {
        app.store.settings.zoom = new_zoom;
        defer.push(Box::new(|a| a.persist()));
    }
    let t = &mut app.tabs[idx];
    let geo = Geo::new(rect.min - t.explorer.offset, zoom, layout.columns);

    // visible range of columns (time axis)
    let (ta, tb) = (geo.time_at(rect.min.y), geo.time_at(rect.max.y));
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

    // ---- hovered row (and whether the pointer is over its node)
    let r = geo.node_radius();
    let mut hovered: Option<usize> = None;
    let mut over_node = false;
    if let Some(p) = resp.hover_pos().filter(|p| rect.contains(*p)) {
        let lc = geo.time_at(p.y).round();
        if lc >= 0.0 {
            if let Some(&i) = t.explorer.by_col.get(&(lc as u32)) {
                hovered = Some(i);
                let n = layout.nodes[i];
                over_node = geo.pos(n.col, n.row).distance(p) <= r + 5.0;
            }
        }
    }
    let head = t.refs.as_ref().and_then(|r| r.head.clone());
    let selected = t.selected.clone();
    let compare = t.compare_with.clone();
    let hash_of = |i: usize| if Some(i) == layout.worktree { WORKTREE } else { t.commits[i].hash.as_str() };

    // ---- row highlights behind everything
    for (&col, &i) in t.explorer.by_col.iter().filter(|(col, _)| visible(**col)) {
        let y = geo.pos(col, 0).y;
        let row = Rect::from_min_max(pos2(rect.min.x, y - ROW / 2.0), pos2(rect.max.x, y + ROW / 2.0));
        let h = hash_of(i);
        if selected.as_deref() == Some(h) {
            painter.rect_filled(row, 0.0, c.sel);
        } else if compare.as_deref() == Some(h) {
            painter.rect_filled(row, 0.0, c.yellow_soft);
        } else if hovered == Some(i) {
            painter.rect_filled(row, 0.0, c.hover);
        }
    }

    // ---- edges
    let stroke_w = 1.6_f32;
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
                painter.add(CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, Stroke::new(stroke_w * 0.85_f32, color)));
                // arrow head touching the merge commit
                let dir = (b - pts[2]).normalized();
                let tip = b - dir * (r + 1.5);
                let side = vec2(-dir.y, dir.x);
                let s = 6.0;
                painter.add(Shape::convex_polygon(vec![tip, tip - dir * s + side * s * 0.6, tip - dir * s - side * s * 0.6], color, Stroke::NONE));
            }
        }
    }

    // ---- nodes
    let wt_count = t.change_count();
    for (i, n) in layout.nodes.iter().enumerate() {
        if !visible(n.col) {
            continue;
        }
        let p = geo.pos(n.col, n.row);
        if p.y < rect.min.y - 40.0 || p.y > rect.max.y + 40.0 || p.x < rect.min.x - 40.0 || p.x > rect.max.x + 40.0 {
            continue;
        }
        let color = theme::lane_color(layout.lanes[n.lane].color);
        if Some(i) == layout.worktree {
            let pts: Vec<Pos2> = (0..=24).map(|k| p + vec2((k as f32 / 24.0 * std::f32::consts::TAU).cos(), (k as f32 / 24.0 * std::f32::consts::TAU).sin()) * r).collect();
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.8_f32, c.yellow), 3.0, 2.5));
        } else {
            let fill = if dim(i) { theme::with_alpha(color, 50) } else { color };
            painter.circle(p, r, fill, Stroke::new(2.0_f32, c.bg));
            if t.commits[i].parents.len() > 1 {
                painter.circle_filled(p, r * 0.38, c.bg);
            }
            if head.as_deref() == Some(hash_of(i)) {
                painter.circle_stroke(p, r + 3.0, Stroke::new(2.0_f32, c.text));
            }
        }
        if hovered == Some(i) && over_node {
            painter.circle_stroke(p, r + 2.0, Stroke::new(1.5_f32, theme::with_alpha(c.text, 140)));
        }
    }

    // ---- description, date, author and commit columns next to the graph
    let cols = Columns::new((geo.origin.x + PAD_X + layout.rows as f32 * geo.lane).max(rect.min.x + 8.0), rect.max.x - 10.0);
    let tips: std::collections::HashMap<usize, &codesprout_core::explorer::Lane> =
        layout.lanes.iter().filter(|l| l.kind == LaneKind::Inferred).map(|l| (l.tip, l)).collect();
    for (&col, &i) in t.explorer.by_col.iter().filter(|(col, _)| visible(**col)) {
        let y = geo.pos(col, 0).y;
        if y < rect.min.y - ROW || y > rect.max.y + ROW {
            continue;
        }
        let cell = |from: f32, to: f32| painter.with_clip_rect(Rect::from_min_max(pos2(from, y - ROW / 2.0), pos2(to, y + ROW / 2.0)).intersect(rect));
        let p = cell(cols.text, cols.description_end());
        if Some(i) == layout.worktree {
            p.text(pos2(cols.text, y), Align2::LEFT_CENTER, format!("Cambios sin confirmar ({wt_count})"), FontId::proportional(13.0), c.yellow);
            continue;
        }
        let cm = &t.commits[i];
        let faded = dim(i);
        let mut x = cols.text;
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
            x += badge(&p, x, y, &format!("{} (eliminada)", lane.name), theme::lane_color(lane.color));
        }
        let text_color = if faded { theme::with_alpha(c.text, 90) } else { c.text };
        p.text(pos2(x + 2.0, y), Align2::LEFT_CENTER, &cm.subject, FontId::proportional(13.0), text_color);
        if cols.meta {
            let muted = if faded { theme::with_alpha(c.muted, 90) } else { c.muted };
            let text2 = if faded { muted } else { c.text2 };
            cell(cols.date, cols.author - Columns::GAP).text(pos2(cols.date, y), Align2::LEFT_CENTER, widgets::full_time(cm.time), FontId::proportional(12.0), text2);
            cell(cols.author, cols.hash - Columns::GAP).text(pos2(cols.author, y), Align2::LEFT_CENTER, &cm.author, FontId::proportional(12.5), text2);
            cell(cols.hash, cols.right).text(pos2(cols.hash, y), Align2::LEFT_CENTER, &cm.hash[..8], theme::mono(11.5), muted);
        }
    }

    // ---- column header
    let hp = ui.painter().with_clip_rect(header);
    hp.rect_filled(header, 0.0, c.panel);
    hp.hline(header.x_range(), header.max.y - 0.5, Stroke::new(1.0_f32, c.border));
    let mut titles = vec![(header.min.x + 8.0, "Gráfico"), (cols.text, "Descripción")];
    if cols.meta {
        titles.extend([(cols.date, "Fecha"), (cols.author, "Autor"), (cols.hash, "Commit")]);
    }
    for (k, &(x, title)) in titles.iter().enumerate() {
        let end = titles.get(k + 1).map_or(header.max.x, |n| n.0 - Columns::GAP / 2.0);
        if k > 0 {
            hp.vline(x - Columns::GAP / 2.0, header.y_range().shrink(5.0), Stroke::new(1.0_f32, c.border));
        }
        hp.with_clip_rect(Rect::from_min_max(pos2(x, header.min.y), pos2(end - 4.0, header.max.y)).intersect(header))
            .text(pos2(x, header.center().y), Align2::LEFT_CENTER, title, FontId::proportional(12.0), c.text2);
    }

    // ---- interaction
    let menu_open = resp.context_menu_opened();
    // the row already shows the details: the tooltip appears only over the node
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
