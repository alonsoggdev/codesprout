//! Secondary views: reflog, file history, blame, compare, pull requests, submodules,
//! worktrees and Git LFS.

use codesprout_core::extras::{self, SubmoduleState};
use eframe::egui::{self, vec2, Align, Layout, RichText, Sense};

use super::{view_header, Defer};
use crate::app::App;
use crate::dialogs::Dialog;
use crate::icons::{self, Icon};
use crate::repo_state::View;
use crate::theme;
use crate::widgets;

pub fn repo_error(ui: &mut egui::Ui, e: &str, defer: &mut Defer) {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        icons::show(ui, Icon::Alert, 36.0, theme::p().red);
        ui.heading("No se puede abrir el repositorio");
        ui.set_max_width(560.0);
        widgets::error_box(ui, e);
        ui.horizontal(|ui| {
            if ui.button("Reintentar").clicked() {
                defer.push(Box::new(|a| a.refresh(true)));
            }
            if widgets::danger_button(ui, "Quitar de la lista", true).clicked() {
                defer.push(Box::new(|a| {
                    if let Some(k) = a.active_key() {
                        a.remove_repo(&k);
                    }
                }));
            }
        });
    });
}

/// Splits the remaining area into a left list and a right panel.
fn split(ui: &mut egui::Ui, left_w: f32) -> (egui::Ui, egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    let lw = left_w.min(rect.width() * 0.5);
    let l = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + lw, rect.max.y));
    let r = egui::Rect::from_min_max(egui::pos2(l.max.x + 1.0, rect.min.y), rect.max);
    ui.painter().vline(l.max.x, rect.y_range(), egui::Stroke::new(1.0_f32, theme::p().border));
    ui.allocate_rect(rect, Sense::hover());
    (ui.new_child(egui::UiBuilder::new().max_rect(l)), ui.new_child(egui::UiBuilder::new().max_rect(r)))
}

pub fn reflog(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    view_header(ui, "Reflog", "Cada movimiento de HEAD: checkouts, commits, resets, rebases… Úsalo para recuperar commits perdidos.", |ui| {
        if icons::button(ui, Icon::Refresh, "Actualizar").clicked() {
            defer.push(Box::new(|a| {
                if let Some(k) = a.active_key() {
                    a.load_reflog(&k);
                }
            }));
        }
    });
    let (mut lu, mut ru) = split(ui, 560.0);
    let t = &mut app.tabs[idx];
    let Some(entries) = t.reflog.clone() else {
        lu.centered_and_justified(|ui| ui.spinner());
        return;
    };
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(&mut lu, 40.0, entries.len(), |ui, range| {
        for i in range {
            let e = &entries[i];
            let sel = t.reflog_selected == Some(i);
            let width = ui.available_width();
            let (rect, r) = ui.allocate_exact_size(vec2(width, 40.0), Sense::click());
            if sel {
                ui.painter().rect_filled(rect, 5.0, c.sel);
            } else if r.hovered() {
                ui.painter().rect_filled(rect, 5.0, c.hover);
            }
            let p = ui.painter().with_clip_rect(rect.shrink(2.0));
            let kind = e.action.split(':').next().unwrap_or("");
            let color = match kind.split_whitespace().next().unwrap_or("") {
                "commit" => c.green,
                "reset" => c.red,
                "checkout" => c.accent,
                "rebase" | "rebase(finish)" | "rebase(start)" | "rebase(pick)" => c.purple,
                "merge" | "pull" => c.yellow,
                _ => c.muted,
            };
            p.text(rect.min + vec2(10.0, 12.0), egui::Align2::LEFT_CENTER, &e.action, egui::FontId::proportional(13.0), c.text);
            p.text(rect.min + vec2(10.0, 29.0), egui::Align2::LEFT_CENTER, format!("{} · {} · {} · {}", e.selector, &e.hash[..8], widgets::rel_time(e.time), e.subject), egui::FontId::proportional(11.0), c.muted);
            p.circle_filled(rect.min + vec2(4.0, 20.0), 3.0, color);
            if r.clicked() {
                t.reflog_selected = Some(i);
                let h = e.hash.clone();
                let k = t.key.clone();
                defer.push(Box::new(move |a| a.load_detail(&k, h)));
            }
            let h = e.hash.clone();
            r.context_menu(|ui| {
                if ui.button("Crear rama aquí (rescatar)…").clicked() {
                    let h = h.clone();
                    defer.push(Box::new(move |a| a.open_new_branch(Some(h))));
                    ui.close_menu();
                }
                if ui.button("Checkout de este punto…").clicked() {
                    let h = h.clone();
                    defer.push(Box::new(move |a| a.checkout_commit(h)));
                    ui.close_menu();
                }
                if ui.button("Reset de la rama actual a este punto…").clicked() {
                    let h = h.clone();
                    defer.push(Box::new(move |a| a.open_reset(h)));
                    ui.close_menu();
                }
                if ui.button("Ver en el historial").clicked() {
                    let h = h.clone();
                    defer.push(Box::new(move |a| a.reveal_commit(&h)));
                    ui.close_menu();
                }
                if ui.button("Copiar hash").clicked() {
                    ui.ctx().copy_text(h.clone());
                    ui.close_menu();
                }
            });
        }
    });
    super::history::detail(&mut ru, app, idx, defer);
}

pub fn file_history(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &mut app.tabs[idx];
    let Some(fh) = &mut t.file_history else { return };
    let path = fh.path.clone();
    view_header(ui, "Historial del archivo", &path, |ui| {
        super::history::back_to_history(ui, defer);
        if ui.button("Blame").clicked() {
            let p = path.clone();
            defer.push(Box::new(move |a| a.open_blame(&p, None)));
        }
    });
    if let Some(e) = &fh.error {
        widgets::error_box(ui, e);
    }
    let (mut lu, mut ru) = split(ui, 380.0);
    let Some(revs) = fh.revs.clone() else {
        lu.centered_and_justified(|ui| ui.spinner());
        return;
    };
    if revs.is_empty() {
        lu.label(widgets::muted("Este archivo no tiene historial."));
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(&mut lu, 44.0, revs.len(), |ui, range| {
        for i in range {
            let rv = &revs[i];
            let width = ui.available_width();
            let (rect, r) = ui.allocate_exact_size(vec2(width, 44.0), Sense::click());
            if fh.selected == Some(i) {
                ui.painter().rect_filled(rect, 5.0, c.sel);
            } else if r.hovered() {
                ui.painter().rect_filled(rect, 5.0, c.hover);
            }
            let p = ui.painter().with_clip_rect(rect.shrink(2.0));
            p.text(rect.min + vec2(10.0, 13.0), egui::Align2::LEFT_CENTER, &rv.commit.subject, egui::FontId::proportional(13.0), c.text);
            let renamed = if rv.status == "R" { format!(" · renombrado a {}", rv.path) } else { String::new() };
            p.text(rect.min + vec2(10.0, 31.0), egui::Align2::LEFT_CENTER, format!("{} · {} · {}{}", &rv.commit.hash[..8], rv.commit.author, widgets::rel_time(rv.commit.time), renamed), egui::FontId::proportional(11.0), c.muted);
            if r.clicked() {
                fh.selected = Some(i);
                let k = t.key.clone();
                defer.push(Box::new(move |a| a.load_file_history_diff(&k)));
            }
            let cm = rv.commit.clone();
            r.context_menu(|ui| super::history::commit_menu(ui, &cm, defer));
        }
    });
    match &mut fh.diff {
        Some(m) => {
            let acts = super::diff::body(&mut ru, m, &mut app.store.settings);
            super::diff::apply_actions(acts, defer);
        }
        None => {
            ru.centered_and_justified(|ui| ui.spinner());
        }
    }
}

pub fn blame(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &mut app.tabs[idx];
    let Some(b) = &mut t.blame else { return };
    let sub = match &b.rev {
        Some(r) => format!("{} @ {}", b.path, &r[..8.min(r.len())]),
        None => format!("{} (versión del directorio de trabajo)", b.path),
    };
    let path = b.path.clone();
    view_header(ui, "Blame", &sub, |ui| {
        super::history::back_to_history(ui, defer);
        if ui.button("Historial del archivo").clicked() {
            let p = path.clone();
            defer.push(Box::new(move |a| a.open_file_history(&p)));
        }
    });
    if let Some(e) = &b.error {
        widgets::error_box(ui, e);
        return;
    }
    let Some(bl) = &b.blame else {
        ui.centered_and_justified(|ui| ui.spinner());
        return;
    };
    let font = theme::mono(12.5);
    let mut new_sel = None;
    egui::ScrollArea::both().auto_shrink([false, false]).show_rows(ui, 19.0, bl.lines.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let l = &bl.lines[i];
            let info = bl.commits.get(&l.hash);
            let first = i == 0 || bl.lines[i - 1].hash != l.hash;
            let width = ui.available_width().max(1200.0);
            let (rect, r) = ui.allocate_exact_size(vec2(width, 19.0), Sense::click());
            let p = ui.painter();
            let color = widgets::hash_color(&l.hash);
            let sel = b.selected.as_deref() == Some(l.hash.as_str());
            if sel {
                p.rect_filled(rect, 0.0, c.sel);
            } else if r.hovered() {
                p.rect_filled(rect, 0.0, c.hover);
            }
            p.rect_filled(egui::Rect::from_min_size(rect.min, vec2(3.0, 19.0)), 0.0, color);
            if first {
                let who = info.map(|i| format!("{}  {}  {}", &l.hash[..8], widgets::short_date(i.time), i.author)).unwrap_or_default();
                p.with_clip_rect(egui::Rect::from_min_size(rect.min, vec2(300.0, 19.0))).text(rect.min + vec2(10.0, 9.5), egui::Align2::LEFT_CENTER, who, egui::FontId::proportional(11.5), c.text2);
            }
            p.text(rect.min + vec2(345.0, 9.5), egui::Align2::RIGHT_CENTER, l.line.to_string(), egui::FontId::monospace(11.0), c.muted);
            p.text(rect.min + vec2(356.0, 9.5), egui::Align2::LEFT_CENTER, &l.content, font.clone(), c.text);
            let r = match info {
                Some(i) => r.on_hover_text(format!("{}\n{} · {}", i.summary, i.author, widgets::full_time(i.time))),
                None => r,
            };
            if r.clicked() {
                new_sel = Some(l.hash.clone());
            }
            if r.double_clicked() {
                let h = l.hash.clone();
                defer.push(Box::new(move |a| a.reveal_commit(&h)));
            }
        }
    });
    if new_sel.is_some() {
        b.selected = new_sel;
    }
}

pub fn compare(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let t = &mut app.tabs[idx];
    let Some(cmp) = &mut t.compare else { return };
    let sub = format!("{}  →  {}", cmp.from_label, cmp.to_label);
    let (f, to, fl, tl) = (cmp.from.clone(), cmp.to.clone(), cmp.from_label.clone(), cmp.to_label.clone());
    view_header(ui, "Comparar", &sub, |ui| {
        super::history::back_to_history(ui, defer);
        if ui.button("Intercambiar").clicked() {
            let (f, to, fl, tl) = (f.clone(), to.clone(), fl.clone(), tl.clone());
            defer.push(Box::new(move |a| a.open_compare(to, f, tl, fl)));
        }
        if ui.button("Guardar como parche…").clicked() {
            let (f, to) = (f.clone(), to.clone());
            defer.push(Box::new(move |a| {
                if let (Some(p), Some(file)) = (a.active_path(), rfd::FileDialog::new().set_file_name("cambios.patch").save_file()) {
                    a.op("Guardar parche", move || extras::save_diff_patch(&p, &f, &to, &file), |a, _| a.ok("Parche guardado"));
                }
            }));
        }
    });
    if let Some(e) = &cmp.error {
        widgets::error_box(ui, e);
    }
    let (mut lu, mut ru) = split(ui, 360.0);
    let Some(files) = cmp.files.clone() else {
        lu.centered_and_justified(|ui| ui.spinner());
        return;
    };
    lu.label(widgets::small(format!("  ARCHIVOS · {}", files.len())));
    if files.is_empty() {
        lu.label(widgets::muted("  No hay diferencias."));
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(&mut lu, 24.0, files.len(), |ui, range| {
        for i in range {
            let r = widgets::row(ui, cmp.selected == Some(i), 24.0, |ui| super::history::file_label(ui, &files[i]));
            if r.clicked() {
                cmp.selected = Some(i);
                let k = t.key.clone();
                defer.push(Box::new(move |a| a.load_compare_diff(&k)));
            }
        }
    });
    if let Some(m) = &mut cmp.diff {
        let acts = super::diff::body(&mut ru, m, &mut app.store.settings);
        super::diff::apply_actions(acts, defer);
    }
}

pub fn pull_requests(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &app.tabs[idx];
    let sub = t.prs.remote.as_ref().map(|r| format!("{} · {}", r.provider.label(), r.path)).unwrap_or_default();
    view_header(ui, "Pull requests", &sub, |ui| {
        if widgets::primary_button(ui, "Nueva pull request", true).clicked() {
            defer.push(Box::new(|a| a.open_create_pr(None)));
        }
        if icons::button(ui, Icon::Refresh, "Actualizar").clicked() {
            defer.push(Box::new(|a| {
                if let Some(k) = a.active_key() {
                    a.load_prs(&k);
                }
            }));
        }
        if let Some(r) = &t.prs.remote {
            if ui.button("Abrir en el navegador").clicked() {
                let _ = open::that(r.web_url());
            }
        }
    });
    egui::Frame::new().inner_margin(12).show(ui, |ui| {
        if t.prs.loading {
            ui.spinner();
            return;
        }
        if let Some(e) = &t.prs.error {
            widgets::error_box(ui, e);
            if ui.button("Iniciar sesión…").clicked() {
                defer.push(Box::new(|a| a.dialogs.push(Dialog::accounts())));
            }
            return;
        }
        let Some(list) = &t.prs.list else {
            ui.label(widgets::muted("No se ha reconocido ningún remoto de GitHub, GitLab o Bitbucket en este repositorio."));
            return;
        };
        if list.is_empty() {
            ui.label(widgets::muted("No hay pull requests abiertas."));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for pr in list {
                egui::Frame::new().fill(c.panel).stroke(egui::Stroke::new(1.0_f32, c.border)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        icons::show(ui, Icon::PullRequest, 16.0, if pr.draft { c.muted } else { c.green });
                        ui.label(RichText::new(format!("#{} {}", pr.number, pr.title)).strong());
                        if pr.draft {
                            widgets::pill(ui, "borrador", c.muted);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Abrir").clicked() {
                                let _ = open::that(&pr.url);
                            }
                            if ui.button("Checkout").on_hover_text("Crear una rama local con la rama de la pull request").clicked() {
                                let src = pr.source.clone();
                                defer.push(Box::new(move |a| {
                                    let remote = a.tab().and_then(|t| t.refs.as_ref()).and_then(|r| r.default_remote(None)).unwrap_or_else(|| "origin".into());
                                    let name = format!("{remote}/{src}");
                                    let known = a.tab().and_then(|t| t.refs.as_ref()).is_some_and(|r| r.remote.iter().any(|b| b.name == name));
                                    if known {
                                        a.checkout_remote_branch(name, src);
                                    } else {
                                        a.toast("Esa rama no está en tu copia local: haz fetch primero (puede venir de un fork).", crate::app::ToastKind::Info);
                                    }
                                }));
                            }
                        });
                    });
                    ui.label(widgets::small(format!("{} quiere fusionar {} → {}", pr.author, pr.source, pr.target)));
                });
                ui.add_space(6.0);
            }
        });
    });
}

pub fn submodules(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &app.tabs[idx];
    view_header(ui, "Submódulos", "", |ui| {
        if widgets::primary_button(ui, "Añadir submódulo", true).clicked() {
            defer.push(Box::new(|a| a.dialogs.push(Dialog::SubmoduleAdd { url: String::new(), path: String::new(), error: None })));
        }
        if ui.button("Actualizar todos").on_hover_text("git submodule update --init --recursive").clicked() {
            defer.push(Box::new(|a| {
                let Some(p) = a.active_path() else { return };
                a.remote_op("Actualizar submódulos", move |h| extras::submodule_update(&p, None, Some(h)), |a, _| {
                    a.ok("Submódulos actualizados");
                    if let Some(k) = a.active_key() {
                        a.load_submodules(&k);
                    }
                });
            }));
        }
        if ui.button("Sincronizar URLs").clicked() {
            defer.push(Box::new(|a| {
                let Some(p) = a.active_path() else { return };
                a.op("Sincronizar submódulos", move || extras::submodule_sync(&p), |a, _| a.ok("URLs sincronizadas"));
            }));
        }
    });
    egui::Frame::new().inner_margin(12).show(ui, |ui| {
        let Some(list) = &t.submodules else {
            ui.spinner();
            return;
        };
        if list.is_empty() {
            ui.label(widgets::muted("Este repositorio no tiene submódulos."));
        }
        for s in list {
            egui::Frame::new().fill(c.panel).stroke(egui::Stroke::new(1.0_f32, c.border)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    icons::show(ui, Icon::Box, 16.0, c.accent);
                    ui.label(RichText::new(&s.path).strong());
                    let color = match s.state {
                        SubmoduleState::UpToDate => c.green,
                        SubmoduleState::NotInitialized => c.muted,
                        SubmoduleState::Modified => c.yellow,
                        SubmoduleState::Conflict => c.red,
                    };
                    widgets::pill(ui, s.state.label(), color);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if s.state != SubmoduleState::NotInitialized && ui.button("Abrir").on_hover_text("Abrir el submódulo en una pestaña").clicked() {
                            let p = s.path.clone();
                            defer.push(Box::new(move |a| {
                                if let Some(root) = a.active_path() {
                                    a.open_repo(root.join(p));
                                }
                            }));
                        }
                        let label = if s.state == SubmoduleState::NotInitialized { "Inicializar" } else { "Actualizar" };
                        if ui.button(label).clicked() {
                            let p = s.path.clone();
                            defer.push(Box::new(move |a| {
                                let Some(root) = a.active_path() else { return };
                                a.remote_op("Actualizar submódulo", move |h| extras::submodule_update(&root, Some(&p), Some(h)), |a, _| {
                                    if let Some(k) = a.active_key() {
                                        a.load_submodules(&k);
                                    }
                                });
                            }));
                        }
                        if s.state != SubmoduleState::NotInitialized && ui.button("Desactivar").on_hover_text("git submodule deinit: vacía la carpeta pero lo mantiene registrado").clicked() {
                            let p = s.path.clone();
                            defer.push(Box::new(move |a| {
                                let Some(root) = a.active_path() else { return };
                                a.op("Desactivar submódulo", move || extras::submodule_deinit(&root, &p), |a, _| {
                                    if let Some(k) = a.active_key() {
                                        a.load_submodules(&k);
                                    }
                                });
                            }));
                        }
                    });
                });
                ui.label(widgets::small(format!("{} · {}{}", s.url, &s.hash[..8.min(s.hash.len())], if s.describe.is_empty() { String::new() } else { format!(" ({})", s.describe) })));
            });
            ui.add_space(6.0);
        }
    });
}

pub fn worktrees(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &app.tabs[idx];
    let parent = t.path.parent().map(|p| p.to_path_buf());
    let name = t.name();
    view_header(ui, "Worktrees", "", |ui| {
        if widgets::primary_button(ui, "Nuevo worktree", true).clicked() {
            let suggested = parent.as_ref().map(|p| p.join(format!("{name}-wt")).to_string_lossy().into_owned()).unwrap_or_default();
            defer.push(Box::new(move |a| a.dialogs.push(Dialog::WorktreeAdd { path: suggested, branch: String::new(), new_branch: true, error: None })));
        }
        if ui.button("Limpiar").on_hover_text("git worktree prune: olvida worktrees cuya carpeta ya no existe").clicked() {
            defer.push(Box::new(|a| {
                let Some(p) = a.active_path() else { return };
                a.op("Limpiar worktrees", move || extras::worktree_prune(&p), |a, _| {
                    if let Some(k) = a.active_key() {
                        a.load_worktrees(&k);
                    }
                });
            }));
        }
    });
    egui::Frame::new().inner_margin(12).show(ui, |ui| {
        let Some(list) = &t.worktrees else {
            ui.spinner();
            return;
        };
        for w in list {
            egui::Frame::new().fill(c.panel).stroke(egui::Stroke::new(1.0_f32, c.border)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    icons::show(ui, Icon::Layers, 16.0, c.accent);
                    ui.label(RichText::new(w.path.to_string_lossy()).strong());
                    if w.main {
                        widgets::pill(ui, "principal", c.accent);
                    }
                    if w.locked {
                        widgets::pill(ui, "bloqueado", c.yellow);
                    }
                    if w.prunable {
                        widgets::pill(ui, "carpeta eliminada", c.red);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !w.main && ui.button("Eliminar").clicked() {
                            let p = w.path.clone();
                            defer.push(Box::new(move |a| {
                                let msg = format!("Se eliminará la carpeta {} (la rama se conserva).", p.display());
                                a.confirm("Eliminar worktree", &msg, None, "Eliminar", true, move |a| {
                                    let Some(root) = a.active_path() else { return };
                                    a.op("Eliminar worktree", move || extras::worktree_remove(&root, &p, false), |a, _| {
                                        if let Some(k) = a.active_key() {
                                            a.load_worktrees(&k);
                                        }
                                    });
                                });
                            }));
                        }
                        if !w.prunable && ui.button("Abrir").clicked() {
                            let p = w.path.clone();
                            defer.push(Box::new(move |a| a.open_repo(p)));
                        }
                    });
                });
                ui.label(widgets::small(format!("{} · {}", w.branch.clone().unwrap_or_else(|| "HEAD separado".into()), &w.head[..8.min(w.head.len())])));
            });
            ui.add_space(6.0);
        }
    });
}

pub fn lfs(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &app.tabs[idx];
    view_header(ui, "Git LFS", "Archivos grandes guardados fuera del repositorio", |ui| {
        if t.lfs.as_ref().is_some_and(|l| l.version.is_some()) {
            if widgets::primary_button(ui, "Seguir patrón…", true).clicked() {
                defer.push(Box::new(|a| a.dialogs.push(Dialog::LfsTrack { pattern: String::new(), error: None })));
            }
            if ui.button("Descargar (lfs pull)").clicked() {
                defer.push(Box::new(|a| {
                    let Some(p) = a.active_path() else { return };
                    a.remote_op("LFS pull", move |h| extras::lfs_pull(&p, Some(h)), |a, _| a.ok("Archivos LFS descargados"));
                }));
            }
        }
    });
    egui::Frame::new().inner_margin(12).show(ui, |ui| {
        let Some(info) = &t.lfs else {
            ui.spinner();
            return;
        };
        let Some(v) = &info.version else {
            widgets::info_box(ui, "Git LFS no está instalado. Viene con Git para Windows; si no, descárgalo de git-lfs.com.", c.yellow);
            return;
        };
        ui.label(widgets::small(v.as_str()));
        if info.patterns.is_empty() && info.files.is_empty() {
            ui.label("Este repositorio no usa LFS todavía.");
            if ui.button("Activar LFS en este repositorio (git lfs install)").clicked() {
                defer.push(Box::new(|a| {
                    let Some(p) = a.active_path() else { return };
                    a.op("LFS install", move || extras::lfs_install(&p), |a, _| a.ok("LFS activado"));
                }));
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Patrones seguidos").strong());
        for pat in &info.patterns {
            ui.horizontal(|ui| {
                ui.label(RichText::new(pat).font(theme::mono(12.5)));
                if ui.small_button("Dejar de seguir").clicked() {
                    let pt = pat.clone();
                    defer.push(Box::new(move |a| {
                        let Some(p) = a.active_path() else { return };
                        a.op("LFS untrack", move || extras::lfs_untrack(&p, &pt), |a, _| {
                            if let Some(k) = a.active_key() {
                                a.load_lfs(&k);
                            }
                        });
                    }));
                }
            });
        }
        ui.add_space(6.0);
        let locks = t.lfs_locks.clone().unwrap_or_default();
        ui.label(RichText::new(format!("Archivos · {}", info.files.len())).strong());
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for f in &info.files {
                ui.horizontal(|ui| {
                    widgets::pill(ui, if f.downloaded { "descargado" } else { "solo puntero" }, if f.downloaded { c.green } else { c.muted });
                    ui.label(&f.path);
                    let lock = locks.iter().find(|l| l.path == f.path);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| match lock {
                        Some(l) => {
                            widgets::pill(ui, format!("bloqueado por {}", l.owner), c.yellow);
                            if ui.small_button("Desbloquear").clicked() {
                                let p = f.path.clone();
                                defer.push(Box::new(move |a| {
                                    let Some(r) = a.active_path() else { return };
                                    a.op("LFS unlock", move || extras::lfs_unlock(&r, &p, false), |a, _| {
                                        if let Some(k) = a.active_key() {
                                            a.load_lfs(&k);
                                        }
                                    });
                                }));
                            }
                        }
                        None => {
                            if ui.small_button("Bloquear").on_hover_text("Avisa al resto del equipo de que estás editando este archivo").clicked() {
                                let p = f.path.clone();
                                defer.push(Box::new(move |a| {
                                    let Some(r) = a.active_path() else { return };
                                    a.op("LFS lock", move || extras::lfs_lock(&r, &p), |a, _| {
                                        if let Some(k) = a.active_key() {
                                            a.load_lfs(&k);
                                        }
                                    });
                                }));
                            }
                        }
                    });
                });
            }
        });
    });
    let _ = View::Lfs;
}
