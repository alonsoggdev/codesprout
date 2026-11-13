//! Main area views.

pub mod changes;
pub mod diff;
pub mod explorer;
pub mod extra;
pub mod history;
pub mod home;
pub mod sidebar;

use eframe::egui::{self, Rect};

use crate::app::App;
use crate::repo_state::View;
use crate::theme;

/// Actions collected while drawing (when the app is borrowed) and applied afterwards.
pub type Defer = Vec<Box<dyn FnOnce(&mut App)>>;

pub fn run(app: &mut App, defer: Defer) {
    for f in defer {
        f(app);
    }
}

pub fn central(app: &mut App, ctx: &egui::Context) {
    let c = theme::p();
    let Some(idx) = app.active else { return };
    if app.split.len() >= 2 {
        return split_central(app, ctx);
    }
    let mut defer: Defer = vec![];
    egui::CentralPanel::default().frame(egui::Frame::new().fill(c.surface)).show(ctx, |ui| {
        pane_body(ui, app, idx, &mut defer);
    });
    run(app, defer);
}

/// The view of repository `idx` (its history, changes, blame...), filling `ui`.
fn pane_body(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let t = &app.tabs[idx];
    if t.status.is_none() {
        if let Some(e) = t.error.clone() {
            extra::repo_error(ui, &e, defer);
        } else {
            crate::widgets::placeholder(ui, crate::icons::Icon::History, "Cargando repositorio…", "");
        }
        return;
    }
    match t.view {
        View::History => history::show(ui, app, idx, defer),
        View::Changes => changes::show(ui, app, idx, defer),
        View::Reflog => extra::reflog(ui, app, idx, defer),
        View::FileHistory => extra::file_history(ui, app, idx, defer),
        View::Blame => extra::blame(ui, app, idx, defer),
        View::Compare => extra::compare(ui, app, idx, defer),
        View::PullRequests => extra::pull_requests(ui, app, idx, defer),
        View::Submodules => extra::submodules(ui, app, idx, defer),
        View::Worktrees => extra::worktrees(ui, app, idx, defer),
        View::Lfs => extra::lfs(ui, app, idx, defer),
    }
}

const GAP: f32 = 6.0;
const MIN_PANE: f32 = 220.0;

/// Several repositories at once. Each pane is a complete repository view; the one with the focus
/// is the active repository, the one the toolbar and the sidebar work on. While a pane is drawn
/// (and its deferred actions run) it is made the active one, so everything it does lands on its
/// own repository.
fn split_central(app: &mut App, ctx: &egui::Context) {
    let c = theme::p();
    egui::CentralPanel::default().frame(egui::Frame::new().fill(c.bg).inner_margin(egui::Margin::same(5))).show(ctx, |ui| {
        let keys = app.split.clone();
        let n = keys.len();
        let vertical = app.split_vertical;
        let full = ui.available_rect_before_wrap();
        if app.split_weights.len() != n {
            app.split_weights = vec![1.0; n];
        }
        // pane rectangles from the weights
        let span = if vertical { full.height() } else { full.width() } - GAP * (n - 1) as f32;
        let sum: f32 = app.split_weights.iter().sum();
        let mut rects = Vec::with_capacity(n);
        let mut at = if vertical { full.min.y } else { full.min.x };
        for w in &app.split_weights {
            let len = span * w / sum;
            rects.push(if vertical {
                Rect::from_min_size(egui::pos2(full.min.x, at), egui::vec2(full.width(), len))
            } else {
                Rect::from_min_size(egui::pos2(at, full.min.y), egui::vec2(len, full.height()))
            });
            at += len + GAP;
        }
        // dividers: dragging one moves the boundary between its two panes
        for i in 0..n - 1 {
            let (a, b) = (rects[i], rects[i + 1]);
            let bar = if vertical {
                Rect::from_min_max(egui::pos2(full.min.x, a.max.y), egui::pos2(full.max.x, b.min.y))
            } else {
                Rect::from_min_max(egui::pos2(a.max.x, full.min.y), egui::pos2(b.min.x, full.max.y))
            };
            let resp = ui.interact(bar.expand(2.0), ui.id().with(("divider", i)), egui::Sense::drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(if vertical { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
                ui.painter().rect_filled(bar, 2.0, theme::with_alpha(c.accent, 110));
            }
            if resp.dragged() {
                let d = if vertical { resp.drag_delta().y } else { resp.drag_delta().x };
                let per_px = sum / span;
                let (wa, wb) = (app.split_weights[i], app.split_weights[i + 1]);
                let min = MIN_PANE * per_px;
                let d = (d * per_px).clamp(min - wa, wb - min);
                if wa + wb > 2.0 * min {
                    app.split_weights[i] = wa + d;
                    app.split_weights[i + 1] = wb - d;
                }
            }
        }
        // a click inside a pane gives it the focus (on press, so what is clicked acts on that repository)
        let pressed = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None });
        if let Some(p) = pressed {
            let hit = rects.iter().position(|r| r.contains(p)).and_then(|i| app.tabs.iter().position(|t| t.key == keys[i]));
            if let Some(idx) = hit.filter(|&i| app.active != Some(i)) {
                app.activate(Some(idx));
                ui.ctx().request_repaint();
            }
        }
        let focus_key = app.active_key();
        let search = std::mem::take(&mut app.focus_search);
        for (i, key) in keys.iter().enumerate() {
            let Some(idx) = app.tabs.iter().position(|t| &t.key == key) else { continue };
            let focused = focus_key.as_ref() == Some(key);
            let rect = rects[i];
            app.active = Some(idx);
            app.focus_search = search && focused;
            let mut defer: Defer = vec![];
            let mut unsplit = false;
            ui.painter().rect_filled(rect, 6.0, c.surface);
            let mut pane = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)).id_salt(("pane", key)));
            pane.set_clip_rect(rect.intersect(ui.clip_rect()));
            pane_header(&mut pane, app, idx, focused, &mut unsplit, &mut defer);
            pane_body(&mut pane, app, idx, &mut defer);
            ui.painter().rect_stroke(rect, 6.0, egui::Stroke::new(if focused { 1.5_f32 } else { 1.0 }, if focused { c.accent } else { c.border }), egui::StrokeKind::Inside);
            run(app, defer);
            app.focus_search = false;
            if unsplit {
                app.unsplit(key);
                return;
            }
        }
        // back to the focused pane (the tabs may have changed while the panes ran their actions)
        app.active = focus_key.and_then(|k| app.tabs.iter().position(|t| t.key == k));
    });
}

/// Strip at the top of a pane: repository, branch, quick view switch and the button that removes it.
fn pane_header(ui: &mut egui::Ui, app: &mut App, idx: usize, focused: bool, unsplit: &mut bool, defer: &mut Defer) {
    use crate::icons::{self, Icon};
    let c = theme::p();
    let t = &app.tabs[idx];
    let (name, branch, n, view) = (t.name(), t.branch_label(), t.change_count(), t.view);
    let fill = if focused { theme::with_alpha(c.accent, 30) } else { c.panel };
    egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            icons::show(ui, Icon::Box, 13.0, if focused { c.accent } else { c.muted });
            ui.add(egui::Label::new(egui::RichText::new(&name).strong().size(13.5)).truncate().selectable(false));
            icons::show(ui, Icon::Branch, 12.0, c.muted);
            ui.add(egui::Label::new(egui::RichText::new(&branch).size(12.0).color(c.text2)).truncate().selectable(false));
            if n > 0 {
                crate::widgets::pill(ui, n.to_string(), c.yellow).on_hover_text("Cambios sin confirmar");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 4.0, c.active);
                }
                icons::paint(ui.painter(), rect.shrink(6.0), Icon::Close, c.muted);
                if resp.on_hover_text("Quitar de la pantalla dividida").clicked() {
                    *unsplit = true;
                }
                ui.add_space(4.0);
                for (v, label) in [(View::Changes, "Cambios"), (View::History, "Historial")] {
                    if ui.selectable_label(view == v, label).clicked() {
                        defer.push(Box::new(move |app| app.set_view(v)));
                    }
                }
            });
        });
    });
}

/// Header strip at the top of a view: title on the left, controls on the right.
pub fn view_header(ui: &mut egui::Ui, title: &str, sub: &str, right: impl FnOnce(&mut egui::Ui)) {
    let c = theme::p();
    egui::Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).strong().size(14.5));
            if !sub.is_empty() {
                ui.label(crate::widgets::muted(sub));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
        });
    });
}
