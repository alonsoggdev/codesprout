//! Update check and installation: the button in the tab bar, the periodic check and the
//! "Actualizar y reiniciar" flow. The download itself lives in `codesprout_core::update`.

use std::time::{Duration, Instant};

use eframe::egui::{self, pos2, vec2, Align, Color32, CursorIcon, FontId, Layout, Rect, RichText, Sense};
use codesprout_core::update::{self, Release};

use crate::app::{App, ToastKind};
use crate::dialogs::Dialog;
use crate::icons::{self, Icon};
use crate::theme;
use crate::widgets;

const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct UpdateState {
    pub available: Option<Release>,
    pub checking: bool,
    pub last_check: Option<Instant>,
    /// Bytes downloaded / total while installing.
    pub installing: Option<(u64, u64)>,
}

impl App {
    pub fn check_updates(&mut self, manual: bool) {
        if self.update.checking || self.update.installing.is_some() {
            return;
        }
        self.update.checking = true;
        self.bus.spawn(update::check, move |app, r| {
            app.update.checking = false;
            app.update.last_check = Some(Instant::now());
            match r {
                Ok(Some(rel)) => {
                    let known = app.update.available.as_ref().is_some_and(|a| a.version == rel.version);
                    if !known {
                        app.toast(format!("CodeSprout {} está disponible. Pulsa «Actualizar» arriba a la derecha.", rel.version), ToastKind::Info);
                    }
                    app.update.available = Some(rel);
                    if manual {
                        app.dialogs.push(Dialog::update());
                    }
                }
                Ok(None) => {
                    app.update.available = None;
                    if manual {
                        app.ok(format!("Ya tienes la última versión ({})", update::CURRENT));
                    }
                }
                Err(e) => {
                    if manual {
                        app.toast(e.to_string(), ToastKind::Error);
                    }
                }
            }
        });
    }

    /// Called every frame: checks at startup and then every few hours.
    pub fn update_tick(&mut self, ctx: &egui::Context) {
        if !self.store.settings.check_updates {
            return;
        }
        if self.update.last_check.is_none_or(|t| t.elapsed() >= CHECK_EVERY) {
            self.check_updates(false);
        }
        ctx.request_repaint_after(CHECK_EVERY);
    }

    pub fn install_update(&mut self) {
        let Some(rel) = self.update.available.clone() else { return };
        if self.update.installing.is_some() {
            return;
        }
        self.update.installing = Some((0, 0));
        set_update_error(self, None);
        let s = self.sender();
        std::thread::spawn(move || {
            let mut last = 0u64;
            let r = update::install(&rel, |done, total| {
                // ~1 % steps (or every 256 KB when the size is unknown) to keep the UI queue short
                let step = if total > 0 { total / 100 } else { 256 * 1024 };
                if done - last >= step.max(1) || done == total {
                    last = done;
                    s.send(move |app| app.update.installing = Some((done, total)));
                }
            });
            s.send(move |app| match r {
                Ok(exe) => app.restart_into(exe),
                Err(e) => {
                    app.update.installing = None;
                    set_update_error(app, Some(e.to_string()));
                }
            });
        });
    }

    fn restart_into(&mut self, exe: std::path::PathBuf) {
        self.save_now();
        match std::process::Command::new(&exe).spawn() {
            Ok(_) => self.bus.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => {
                self.update.installing = None;
                self.update.available = None;
                self.error_dialog("Actualización instalada", &format!("La nueva versión está instalada, pero no se pudo abrir: {e}. Cierra CodeSprout y vuelve a abrirlo."));
            }
        }
    }

    /// Version label in the tab bar; green "Actualizar a X" when there is a newer release.
    pub fn update_button(&mut self, ui: &mut egui::Ui) {
        let c = theme::p();
        let font = FontId::proportional(12.5);
        if let Some(rel) = &self.update.available {
            let text = format!("Actualizar a {}", rel.version);
            let galley = ui.painter().layout_no_wrap(text, font, Color32::WHITE);
            let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 36.0, 26.0), Sense::click());
            let fill = if resp.hovered() { c.green.gamma_multiply(0.85) } else { c.green };
            ui.painter().rect_filled(rect, 6.0, fill);
            icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(14.0, 14.0)), Icon::Pull, Color32::WHITE);
            ui.painter().galley(pos2(rect.min.x + 27.0, rect.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
            if resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(format!("Tienes la {}", update::CURRENT)).clicked() {
                self.dialogs.push(Dialog::update());
            }
            return;
        }
        if self.update.checking {
            ui.add(egui::Spinner::new().size(12.0));
        }
        let text = format!("v{}", update::CURRENT);
        let galley = ui.painter().layout_no_wrap(text, font, c.muted);
        let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, 26.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, 6.0, c.hover);
        }
        ui.painter().galley(pos2(rect.min.x + 8.0, rect.center().y - galley.size().y / 2.0), galley, if resp.hovered() { c.text2 } else { c.muted });
        if resp.on_hover_text("Buscar actualizaciones").clicked() {
            self.check_updates(true);
        }
    }

    pub(crate) fn update_body(&mut self, ui: &mut egui::Ui, error: &mut Option<String>) -> bool {
        let c = theme::p();
        let Some(rel) = self.update.available.clone() else { return true };
        let installing = self.update.installing;
        ui.horizontal(|ui| {
            icons::show(ui, Icon::Pull, 18.0, c.green);
            ui.heading(format!("CodeSprout {}", rel.version));
        });
        ui.label(format!("Hay una versión nueva disponible. Tienes la {}.", update::CURRENT));
        if !rel.notes.is_empty() {
            ui.label(RichText::new("Novedades").strong());
            egui::Frame::new().fill(c.bg).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&rel.notes).color(c.text2)).wrap());
                });
            });
        }
        if let Some((done, total)) = installing {
            let mb = |b: u64| format!("{:.1}", b as f64 / 1_048_576.0).replace('.', ",");
            let text = if total > 0 { format!("Descargando… {} / {} MB", mb(done), mb(total)) } else { format!("Descargando… {} MB", mb(done)) };
            let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(frac).text(text).animate(total == 0));
        }
        if rel.asset.is_none() {
            widgets::info_box(ui, "Esta versión no trae un ejecutable para tu sistema: descárgala desde su página.", c.yellow);
        }
        if let Some(e) = error {
            ui.add_space(4.0);
            widgets::error_box(ui, e);
        }
        ui.add_space(10.0);
        let mut close = false;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if rel.asset.is_some() {
                let label = if error.is_some() { "Reintentar" } else { "Actualizar y reiniciar" };
                if widgets::primary_button(ui, label, installing.is_none()).clicked() {
                    self.install_update();
                }
            } else if widgets::primary_button(ui, "Abrir página de descarga", true).clicked() {
                let _ = open::that(&rel.page);
                close = true;
            }
            if ui.add_enabled(installing.is_none(), egui::Button::new("Más tarde")).clicked() {
                close = true;
            }
            if ui.add(egui::Button::new(RichText::new("Ver en GitHub").color(c.accent)).frame(false)).clicked() {
                let _ = open::that(&rel.page);
            }
        });
        close
    }
}

fn set_update_error(app: &mut App, msg: Option<String>) {
    for d in app.dialogs.iter_mut() {
        if let Dialog::Update { error } = d {
            *error = msg.clone();
        }
    }
}
