//! Main area views.

pub mod changes;
pub mod diff;
pub mod explorer;
pub mod extra;
pub mod history;
pub mod home;
pub mod sidebar;

use eframe::egui;

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
    let mut defer: Defer = vec![];
    egui::CentralPanel::default().frame(egui::Frame::new().fill(c.surface)).show(ctx, |ui| {
        let t = &app.tabs[idx];
        if t.status.is_none() {
            if let Some(e) = t.error.clone() {
                extra::repo_error(ui, &e, &mut defer);
            } else {
                crate::widgets::placeholder(ui, crate::icons::Icon::History, "Cargando repositorio…", "");
            }
            return;
        }
        match t.view {
            View::History => history::show(ui, app, idx, &mut defer),
            View::Changes => changes::show(ui, app, idx, &mut defer),
            View::Reflog => extra::reflog(ui, app, idx, &mut defer),
            View::FileHistory => extra::file_history(ui, app, idx, &mut defer),
            View::Blame => extra::blame(ui, app, idx, &mut defer),
            View::Compare => extra::compare(ui, app, idx, &mut defer),
            View::PullRequests => extra::pull_requests(ui, app, idx, &mut defer),
            View::Submodules => extra::submodules(ui, app, idx, &mut defer),
            View::Worktrees => extra::worktrees(ui, app, idx, &mut defer),
            View::Lfs => extra::lfs(ui, app, idx, &mut defer),
        }
    });
    run(app, defer);
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
