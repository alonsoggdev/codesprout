//! Home screen: clone / add / create, and the repository list.

use eframe::egui::{self, vec2, Align, Layout, RichText, Sense};

use super::{run, Defer};
use crate::actions::pick_folder;
use crate::app::{App, ToastKind};
use crate::icons::{self, Icon};
use crate::theme;
use crate::widgets;

fn card(ui: &mut egui::Ui, icon: Icon, title: &str, sub: &str) -> bool {
    let c = theme::p();
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click());
    ui.painter().rect(rect, 10.0, if r.hovered() { c.hover } else { c.panel }, egui::Stroke::new(1.0_f32, c.border), egui::StrokeKind::Inside);
    let ic = egui::Rect::from_center_size(rect.left_center() + vec2(30.0, 0.0), vec2(36.0, 36.0));
    ui.painter().rect_filled(ic, 8.0, c.accent_soft);
    icons::paint(ui.painter(), ic.shrink(9.0), icon, c.accent);
    ui.painter().text(rect.min + vec2(60.0, 20.0), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(14.0), c.text);
    ui.painter().text(rect.min + vec2(60.0, 39.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(12.0), c.muted);
    r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let c = theme::p();
    let mut defer: Defer = vec![];
    egui::CentralPanel::default().frame(egui::Frame::new().fill(c.surface).inner_margin(32)).show(ctx, |ui| {
        let w = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2((w * 0.36).clamp(280.0, 420.0), ui.available_height()), Layout::top_down(Align::Min), |ui| {
                ui.label(RichText::new("CodeSprout").size(30.0).strong());
                ui.label(RichText::new("Tu cliente Git: historial con gráfico de ramas, preparación por líneas, rebase interactivo, git-flow y pull requests.").color(c.text2));
                ui.add_space(18.0);
                if card(ui, Icon::Fetch, "Clonar repositorio", "Desde una URL o tus repos de GitHub/GitLab/Bitbucket") {
                    defer.push(Box::new(|a| a.open_clone(None)));
                }
                ui.add_space(8.0);
                if card(ui, Icon::Folder, "Añadir repositorio existente", "Una carpeta que ya usa Git") {
                    defer.push(Box::new(|a| {
                        if let Some(d) = pick_folder("Selecciona la carpeta del repositorio") {
                            a.open_repo(d);
                        }
                    }));
                }
                ui.add_space(8.0);
                if card(ui, Icon::Plus, "Crear repositorio nuevo", "Inicializa Git en una carpeta") {
                    defer.push(Box::new(create_new));
                }
                ui.add_space(8.0);
                if card(ui, Icon::User, "Cuentas", "Inicia sesión con SSO en GitHub, GitLab, Bitbucket o Azure") {
                    defer.push(Box::new(|a| a.dialogs.push(crate::dialogs::Dialog::accounts())));
                }
            });
            ui.add_space(28.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Repositorios · {}", app.store.repos.len())).size(17.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let id = egui::Id::new("home-filter");
                        let mut q: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut q).hint_text("Filtrar repositorios").desired_width(220.0)).changed() {
                            ui.data_mut(|d| d.insert_temp(id, q));
                        }
                    });
                });
                let q: String = ui.data(|d| d.get_temp::<String>(egui::Id::new("home-filter"))).unwrap_or_default().to_lowercase();
                ui.add_space(6.0);
                if app.store.repos.is_empty() {
                    widgets::placeholder(ui, Icon::Box, "Aún no has añadido repositorios.", "Clona uno, añade una carpeta existente o arrastra una carpeta a esta ventana.");
                    return;
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for r in app.store.repos.iter().filter(|r| q.is_empty() || r.name.to_lowercase().contains(&q) || r.path.to_lowercase().contains(&q)) {
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 8.0, c.hover);
                        }
                        let ic = egui::Rect::from_center_size(rect.left_center() + vec2(26.0, 0.0), vec2(32.0, 32.0));
                        ui.painter().rect_filled(ic, 8.0, widgets::hash_color(&r.name));
                        ui.painter().text(ic.center(), egui::Align2::CENTER_CENTER, widgets::initials(&r.name), egui::FontId::proportional(13.0), egui::Color32::WHITE);
                        ui.painter().text(rect.min + vec2(52.0, 17.0), egui::Align2::LEFT_CENTER, &r.name, egui::FontId::proportional(14.0), c.text);
                        ui.painter().text(rect.min + vec2(52.0, 35.0), egui::Align2::LEFT_CENTER, &r.path, egui::FontId::proportional(11.5), c.muted);
                        let path = r.path.clone();
                        if resp.clicked() {
                            let p = path.clone();
                            defer.push(Box::new(move |a| a.open_repo(p.into())));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Abrir").clicked() {
                                let p = path.clone();
                                defer.push(Box::new(move |a| a.open_repo(p.into())));
                                ui.close_menu();
                            }
                            if ui.button("Mostrar en el explorador").clicked() {
                                let _ = open::that(&path);
                                ui.close_menu();
                            }
                            if ui.button("Copiar ruta").clicked() {
                                ui.ctx().copy_text(path.clone());
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button(RichText::new("Quitar de la lista").color(c.red)).clicked() {
                                let p = path.clone();
                                defer.push(Box::new(move |a| a.remove_repo(&p)));
                                ui.close_menu();
                            }
                        });
                    }
                });
            });
        });
    });
    run(app, defer);
}

fn create_new(app: &mut App) {
    let Some(dir) = pick_folder("Selecciona o crea la carpeta del nuevo repositorio") else { return };
    app.bus.spawn(
        move || -> anyhow::Result<(std::path::PathBuf, bool)> {
            if let Ok(existing) = codesprout_core::repo::resolve_repo(&dir) {
                if crate::app::same_path(&existing.to_string_lossy(), &dir.to_string_lossy()) {
                    return Ok((existing, false));
                }
            }
            Ok((codesprout_core::repo::init(&dir)?, true))
        },
        |app, r| match r {
            Ok((p, created)) => {
                app.toast(if created { "Repositorio creado" } else { "Esa carpeta ya es un repositorio; se ha abierto." }, if created { ToastKind::Success } else { ToastKind::Info });
                app.open_resolved(p);
                if created {
                    app.set_view(crate::repo_state::View::Changes);
                }
            }
            Err(e) => app.error_dialog("Crear repositorio", &e.to_string()),
        },
    );
}
