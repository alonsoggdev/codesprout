//! Vector icons painted with egui shapes, so they stay crisp at any scale and theme.

use eframe::egui::{self, epaint::CubicBezierShape, pos2, vec2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Commit,
    Pull,
    Push,
    Fetch,
    Branch,
    Merge,
    Stash,
    Discard,
    Terminal,
    Folder,
    Refresh,
    Settings,
    Plus,
    Minus,
    Check,
    Close,
    Tag,
    Cloud,
    Alert,
    Search,
    History,
    Changes,
    File,
    Copy,
    External,
    Trash,
    User,
    Flow,
    PullRequest,
    Graph,
    Target,
    ChevronUp,
    Box,
    Layers,
    Lock,
    ChevronRight,
    ChevronDown,
    Home,
    Eye,
    Patch,
    Undo,
}

fn line(p: &Painter, a: Pos2, b: Pos2, s: Stroke) {
    p.line_segment([a, b], s);
}

fn poly(p: &Painter, pts: Vec<Pos2>, s: Stroke) {
    p.add(Shape::line(pts, s));
}

/// Paints `icon` centered in `rect`.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let c = rect.center();
    let u = s / 16.0; // design grid: 16x16
    let at = |x: f32, y: f32| pos2(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    let st = Stroke::new((1.5 * u).max(1.2), color);
    match icon {
        Icon::Commit => {
            p.circle_stroke(at(8.0, 8.0), 3.0 * u, st);
            line(p, at(1.0, 8.0), at(5.0, 8.0), st);
            line(p, at(11.0, 8.0), at(15.0, 8.0), st);
        }
        Icon::Pull | Icon::Fetch => {
            line(p, at(8.0, 2.0), at(8.0, 11.0), st);
            poly(p, vec![at(4.5, 7.5), at(8.0, 11.0), at(11.5, 7.5)], st);
            if icon == Icon::Pull {
                line(p, at(3.0, 14.0), at(13.0, 14.0), st);
            } else {
                poly(p, vec![at(3.0, 11.0), at(3.0, 14.0), at(13.0, 14.0), at(13.0, 11.0)], st);
            }
        }
        Icon::Push => {
            line(p, at(8.0, 14.0), at(8.0, 5.0), st);
            poly(p, vec![at(4.5, 8.5), at(8.0, 5.0), at(11.5, 8.5)], st);
            line(p, at(3.0, 2.0), at(13.0, 2.0), st);
        }
        Icon::Branch => {
            p.circle_stroke(at(4.5, 3.5), 1.8 * u, st);
            p.circle_stroke(at(4.5, 12.5), 1.8 * u, st);
            p.circle_stroke(at(11.5, 4.5), 1.8 * u, st);
            line(p, at(4.5, 5.3), at(4.5, 10.7), st);
            bezier(p, at(11.5, 6.3), at(11.5, 9.0), at(4.5, 8.0), at(4.5, 10.7), st);
        }
        Icon::Merge => {
            p.circle_stroke(at(4.5, 3.5), 1.8 * u, st);
            p.circle_stroke(at(4.5, 12.5), 1.8 * u, st);
            p.circle_stroke(at(11.5, 10.5), 1.8 * u, st);
            line(p, at(4.5, 5.3), at(4.5, 10.7), st);
            bezier(p, at(4.5, 5.3), at(4.5, 9.0), at(8.0, 10.5), at(9.7, 10.5), st);
        }
        Icon::Stash => {
            poly(p, vec![at(2.0, 6.0), at(2.0, 14.0), at(14.0, 14.0), at(14.0, 6.0)], st);
            line(p, at(1.0, 6.0), at(15.0, 6.0), st);
            line(p, at(8.0, 1.5), at(8.0, 10.0), st);
            poly(p, vec![at(5.0, 7.0), at(8.0, 10.0), at(11.0, 7.0)], st);
        }
        Icon::Undo => {
            poly(p, vec![at(6.0, 2.5), at(2.5, 6.0), at(6.0, 9.5)], st);
            bezier(p, at(2.5, 6.0), at(10.5, 6.0), at(13.5, 7.0), at(13.5, 10.5), st);
            line(p, at(13.5, 10.5), at(13.5, 13.0), st);
        }
        Icon::Discard => {
            bezier(p, at(3.0, 9.0), at(3.0, 14.0), at(13.0, 14.0), at(13.0, 9.0), st);
            bezier(p, at(13.0, 9.0), at(13.0, 4.0), at(6.0, 3.0), at(4.0, 5.5), st);
            poly(p, vec![at(3.0, 2.0), at(3.5, 6.0), at(7.5, 5.5)], st);
        }
        Icon::Terminal => {
            p.rect_stroke(Rect::from_min_max(at(1.5, 2.5), at(14.5, 13.5)), 2.0 * u, st, egui::StrokeKind::Middle);
            poly(p, vec![at(4.0, 6.0), at(6.5, 8.0), at(4.0, 10.0)], st);
            line(p, at(8.0, 10.5), at(12.0, 10.5), st);
        }
        Icon::Folder => {
            poly(
                p,
                vec![at(1.5, 4.0), at(6.0, 4.0), at(7.5, 5.5), at(14.5, 5.5), at(14.5, 13.0), at(1.5, 13.0), at(1.5, 4.0)],
                st,
            );
        }
        Icon::Refresh => {
            arc(p, at(8.0, 8.0), 5.5 * u, 0.6, 5.6, st);
            let tip = at(8.0, 8.0) + vec2(5.5 * u * 5.6f32.cos(), 5.5 * u * 5.6f32.sin());
            poly(p, vec![tip + vec2(-3.0 * u, -0.3 * u), tip, tip + vec2(0.4 * u, -3.0 * u)], st);
        }
        Icon::Settings => {
            p.circle_stroke(at(8.0, 8.0), 2.3 * u, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                line(p, at(8.0, 8.0) + d * 4.3 * u, at(8.0, 8.0) + d * 6.5 * u, st);
            }
            p.circle_stroke(at(8.0, 8.0), 4.4 * u, st);
        }
        Icon::Plus => {
            line(p, at(8.0, 3.0), at(8.0, 13.0), st);
            line(p, at(3.0, 8.0), at(13.0, 8.0), st);
        }
        Icon::Minus => line(p, at(3.0, 8.0), at(13.0, 8.0), st),
        Icon::Check => poly(p, vec![at(3.0, 8.5), at(6.5, 12.0), at(13.0, 4.5)], st),
        Icon::Close => {
            line(p, at(4.0, 4.0), at(12.0, 12.0), st);
            line(p, at(12.0, 4.0), at(4.0, 12.0), st);
        }
        Icon::Tag => {
            poly(p, vec![at(2.0, 2.0), at(8.0, 2.0), at(14.0, 8.0), at(8.0, 14.0), at(2.0, 8.0), at(2.0, 2.0)], st);
            p.circle_filled(at(5.0, 5.0), 1.1 * u, color);
        }
        Icon::Cloud => {
            arc(p, at(5.5, 9.5), 3.0 * u, 1.6, 4.9, st);
            arc(p, at(9.0, 7.0), 4.0 * u, 3.6, 6.1, st);
            arc(p, at(12.0, 10.0), 2.5 * u, 4.9, 7.9, st);
            line(p, at(5.5, 12.5), at(12.0, 12.5), st);
        }
        Icon::Alert => {
            poly(p, vec![at(8.0, 1.5), at(15.0, 14.0), at(1.0, 14.0), at(8.0, 1.5)], st);
            line(p, at(8.0, 6.0), at(8.0, 9.5), st);
            p.circle_filled(at(8.0, 11.8), 0.9 * u, color);
        }
        Icon::Search => {
            p.circle_stroke(at(6.5, 6.5), 4.5 * u, st);
            line(p, at(10.0, 10.0), at(14.5, 14.5), st);
        }
        Icon::History => {
            p.circle_stroke(at(8.0, 8.0), 6.0 * u, st);
            poly(p, vec![at(8.0, 4.5), at(8.0, 8.0), at(10.5, 9.5)], st);
        }
        Icon::Changes => {
            p.rect_stroke(Rect::from_min_max(at(2.5, 1.5), at(13.5, 14.5)), 1.5 * u, st, egui::StrokeKind::Middle);
            line(p, at(8.0, 4.5), at(8.0, 9.5), st);
            line(p, at(5.5, 7.0), at(10.5, 7.0), st);
            line(p, at(5.5, 12.0), at(10.5, 12.0), st);
        }
        Icon::File | Icon::Patch => {
            poly(p, vec![at(3.0, 1.5), at(9.5, 1.5), at(13.0, 5.0), at(13.0, 14.5), at(3.0, 14.5), at(3.0, 1.5)], st);
            poly(p, vec![at(9.5, 1.5), at(9.5, 5.0), at(13.0, 5.0)], st);
            if icon == Icon::Patch {
                line(p, at(6.0, 8.5), at(10.0, 8.5), st);
                line(p, at(8.0, 6.5), at(8.0, 10.5), st);
                line(p, at(6.0, 12.0), at(10.0, 12.0), st);
            }
        }
        Icon::Copy => {
            p.rect_stroke(Rect::from_min_max(at(5.0, 5.0), at(14.0, 14.0)), 1.5 * u, st, egui::StrokeKind::Middle);
            poly(p, vec![at(2.0, 10.5), at(2.0, 2.0), at(10.5, 2.0)], st);
        }
        Icon::External => {
            poly(p, vec![at(7.0, 3.0), at(2.5, 3.0), at(2.5, 13.5), at(13.0, 13.5), at(13.0, 9.0)], st);
            line(p, at(7.5, 8.5), at(14.0, 2.0), st);
            poly(p, vec![at(9.5, 2.0), at(14.0, 2.0), at(14.0, 6.5)], st);
        }
        Icon::Trash => {
            line(p, at(2.0, 4.0), at(14.0, 4.0), st);
            poly(p, vec![at(6.0, 4.0), at(6.0, 2.0), at(10.0, 2.0), at(10.0, 4.0)], st);
            poly(p, vec![at(3.5, 4.0), at(4.5, 14.5), at(11.5, 14.5), at(12.5, 4.0)], st);
        }
        Icon::User => {
            p.circle_stroke(at(8.0, 5.0), 3.0 * u, st);
            bezier(p, at(2.0, 15.0), at(2.0, 9.5), at(14.0, 9.5), at(14.0, 15.0), st);
        }
        Icon::Flow => {
            line(p, at(2.0, 4.0), at(14.0, 4.0), st);
            line(p, at(2.0, 12.0), at(14.0, 12.0), st);
            bezier(p, at(4.0, 4.0), at(7.0, 4.0), at(6.0, 12.0), at(9.0, 12.0), st);
            p.circle_filled(at(4.0, 4.0), 1.5 * u, color);
            p.circle_filled(at(12.0, 12.0), 1.5 * u, color);
        }
        Icon::PullRequest => {
            p.circle_stroke(at(4.0, 3.5), 1.8 * u, st);
            p.circle_stroke(at(4.0, 12.5), 1.8 * u, st);
            p.circle_stroke(at(12.0, 12.5), 1.8 * u, st);
            line(p, at(4.0, 5.3), at(4.0, 10.7), st);
            poly(p, vec![at(12.0, 10.7), at(12.0, 5.0), at(8.0, 5.0)], st);
            poly(p, vec![at(10.0, 3.0), at(8.0, 5.0), at(10.0, 7.0)], st);
        }
        Icon::Graph => {
            line(p, at(2.0, 4.0), at(14.0, 4.0), st);
            line(p, at(6.0, 11.0), at(14.0, 11.0), st);
            bezier(p, at(3.0, 4.0), at(5.0, 4.0), at(4.0, 11.0), at(6.0, 11.0), st);
            p.circle_filled(at(9.0, 4.0), 1.6 * u, color);
            p.circle_filled(at(10.0, 11.0), 1.6 * u, color);
        }
        Icon::Target => {
            p.circle_stroke(at(8.0, 8.0), 5.5 * u, st);
            p.circle_filled(at(8.0, 8.0), 2.0 * u, color);
        }
        Icon::Box => {
            poly(p, vec![at(8.0, 1.5), at(14.0, 4.5), at(14.0, 11.5), at(8.0, 14.5), at(2.0, 11.5), at(2.0, 4.5), at(8.0, 1.5)], st);
            poly(p, vec![at(2.0, 4.5), at(8.0, 7.5), at(14.0, 4.5)], st);
            line(p, at(8.0, 7.5), at(8.0, 14.5), st);
        }
        Icon::Layers => {
            poly(p, vec![at(8.0, 2.0), at(14.5, 5.5), at(8.0, 9.0), at(1.5, 5.5), at(8.0, 2.0)], st);
            poly(p, vec![at(1.5, 8.5), at(8.0, 12.0), at(14.5, 8.5)], st);
            poly(p, vec![at(1.5, 11.5), at(8.0, 15.0), at(14.5, 11.5)], st);
        }
        Icon::Lock => {
            p.rect_stroke(Rect::from_min_max(at(3.0, 7.0), at(13.0, 14.5)), 1.5 * u, st, egui::StrokeKind::Middle);
            arc(p, at(8.0, 7.0), 3.0 * u, 3.14159, 6.28318, st);
        }
        Icon::ChevronRight => poly(p, vec![at(6.0, 3.5), at(10.5, 8.0), at(6.0, 12.5)], st),
        Icon::ChevronUp => poly(p, vec![at(3.5, 10.0), at(8.0, 5.5), at(12.5, 10.0)], st),
        Icon::ChevronDown => poly(p, vec![at(3.5, 6.0), at(8.0, 10.5), at(12.5, 6.0)], st),
        Icon::Home => {
            poly(p, vec![at(1.5, 8.0), at(8.0, 2.0), at(14.5, 8.0)], st);
            poly(p, vec![at(3.5, 6.5), at(3.5, 14.0), at(12.5, 14.0), at(12.5, 6.5)], st);
        }
        Icon::Eye => {
            bezier(p, at(1.0, 8.0), at(4.0, 2.5), at(12.0, 2.5), at(15.0, 8.0), st);
            bezier(p, at(1.0, 8.0), at(4.0, 13.5), at(12.0, 13.5), at(15.0, 8.0), st);
            p.circle_stroke(at(8.0, 8.0), 2.2 * u, st);
        }
    }
}

fn bezier(p: &Painter, a: Pos2, b: Pos2, c: Pos2, d: Pos2, s: Stroke) {
    p.add(CubicBezierShape::from_points_stroke([a, b, c, d], false, Color32::TRANSPARENT, s));
}

fn arc(p: &Painter, center: Pos2, r: f32, from: f32, to: f32, s: Stroke) {
    let n = 24;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            center + vec2(a.cos() * r, a.sin() * r)
        })
        .collect();
    p.add(Shape::line(pts, s));
}

/// Allocates space for an icon and paints it.
pub fn show(ui: &mut Ui, icon: Icon, size: f32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint(ui.painter(), rect, icon, color);
    resp
}

/// Small square button with only an icon.
pub fn button(ui: &mut Ui, icon: Icon, tooltip: &str) -> Response {
    button_sized(ui, icon, tooltip, 24.0, true)
}

pub fn button_sized(ui: &mut Ui, icon: Icon, tooltip: &str, size: f32, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), if enabled { Sense::click() } else { Sense::hover() });
    let c = theme::p();
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, c.hover);
    }
    let color = if !enabled { c.muted.gamma_multiply(0.6) } else if resp.hovered() { c.text } else { c.text2 };
    paint(ui.painter(), rect.shrink(size * 0.22), icon, color);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}
