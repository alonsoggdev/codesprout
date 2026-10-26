//! Left sidebar: workspace views, branches, remotes, tags, stashes and the extra panels.

use eframe::egui::{self, RichText};

use super::{run, Defer};
use crate::app::App;
use crate::icons::{self, Icon};
use crate::repo_state::View;
use crate::theme;
use crate::widgets;

fn item(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool, current: bool, right: Option<(String, egui::Color32)>, nested: bool) -> egui::Response {
    let c = theme::p();
    widgets::row(ui, selected, 26.0, |ui| {
        if nested {
            ui.add_space(14.0);
        }
        icons::show(ui, icon, 14.0, if current { c.accent } else { c.muted });
        let (dir, base) = widgets::split_path(label);
        let mut job = egui::text::LayoutJob::default();
        if !dir.is_empty() {
            job.append(&format!("{dir}/"), 0.0, egui::TextFormat { color: c.muted, font_id: egui::FontId::proportional(13.0), ..Default::default() });
        }
        job.append(base, 0.0, egui::TextFormat { color: if current { c.text } else { c.text2 }, font_id: egui::FontId::proportional(13.0), ..Default::default() });
        ui.add(egui::Label::new(job).truncate().selectable(false));
        if let Some((text, color)) = right {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                widgets::pill(ui, text, color);
            });
        }
    })
}

type Collapsed = std::collections::HashMap<String, bool>;

#[allow(clippy::too_many_arguments)]
fn section(cm: &Collapsed, ui: &mut egui::Ui, key: &str, title: &str, count: Option<usize>, add: Option<(&str, Box<dyn FnOnce(&mut App)>)>, defer: &mut Defer, body: impl FnOnce(&mut egui::Ui, &mut Defer)) {
    let collapsed = cm.get(key).copied().unwrap_or(false);
    ui.add_space(4.0);
    let (resp, rect) = widgets::section_title(ui, title, count, collapsed);
    if let Some((tip, f)) = add {
        let r = egui::Rect::from_center_size(egui::pos2(rect.max.x - 12.0, rect.center().y), egui::vec2(20.0, 20.0));
        // the title row takes the click; on the "+" it means "add" instead of collapse
        let over = resp.hover_pos().is_some_and(|p| r.contains(p));
        if over {
            ui.painter().rect_filled(r, 4.0, theme::p().hover);
            resp.clone().on_hover_text(tip);
        }
        icons::paint(ui.painter(), r.shrink(5.0), Icon::Plus, theme::p().text2);
        if resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| r.contains(p)) {
            defer.push(f);
            return;
        }
    }
    if resp.clicked() {
        let k = key.to_string();
        defer.push(Box::new(move |a| {
            a.store.settings.collapsed.insert(k, !collapsed);
            a.persist();
        }));
    }
    if !collapsed {
        body(ui, defer);
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let c = theme::p();
    let Some(idx) = app.active else { return };
    let mut defer: Defer = vec![];
    let resp = egui::SidePanel::left("sidebar")
        .resizable(true)
        .default_width(app.store.settings.sidebar_width)
        .width_range(180.0..=480.0)
        .frame(egui::Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(8, 6)))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| body(app, ui, idx, &mut defer));
        });
    let w = resp.response.rect.width();
    if (w - app.store.settings.sidebar_width).abs() > 1.0 {
        app.store.settings.sidebar_width = w;
        app.persist();
    }
    run(app, defer);
}

fn body(app: &mut App, ui: &mut egui::Ui, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &app.tabs[idx];
    let view = t.view;
    let n = t.change_count();
    let Some(refs) = t.refs.clone() else { return };
    let selected_hash = t.selected.clone();
    let lfs = app.lfs_version.is_some();
    let sub_count = t.submodules.as_ref().map(|v| v.len());
    let wt_count = t.worktrees.as_ref().map(|v| v.len());
    let pr_count = t.prs.list.as_ref().map(|v| v.len());
    let cm = app.store.settings.collapsed.clone();

    section(&cm, ui, "workspace", "Área de trabajo", None, None, defer, |ui, defer| {
        let pill = (n > 0).then(|| (n.to_string(), c.accent));
        if item(ui, Icon::Changes, "Cambios", view == View::Changes, false, pill, false).on_hover_text("Cambios en el directorio de trabajo (Ctrl+1)").clicked() {
            defer.push(Box::new(|a| a.set_view(View::Changes)));
        }
        if item(ui, Icon::Graph, "Historial", view == View::History, false, None, false).on_hover_text("Explorador de ramas (Ctrl+2)").clicked() {
            defer.push(Box::new(|a| a.set_view(View::History)));
        }
        if item(ui, Icon::History, "Reflog", view == View::Reflog, false, None, false).on_hover_text("Todo lo que ha pasado por HEAD: recupera commits perdidos").clicked() {
            defer.push(Box::new(|a| a.set_view(View::Reflog)));
        }
        if item(ui, Icon::PullRequest, "Pull requests", view == View::PullRequests, false, pr_count.filter(|&x| x > 0).map(|x| (x.to_string(), c.purple)), false).clicked() {
            defer.push(Box::new(|a| a.set_view(View::PullRequests)));
        }
    });

    let has_head = refs.head.is_some();
    section(&cm, ui, "branches", "Ramas", Some(refs.local.len()), has_head.then(|| ("Nueva rama", Box::new(|a: &mut App| a.open_new_branch(None)) as Box<dyn FnOnce(&mut App)>)), defer, |ui, defer| {
        if refs.local.is_empty() {
            ui.label(widgets::small("Aún no hay ramas"));
        }
        for b in &refs.local {
            let track = if b.gone {
                Some(("remota eliminada".to_string(), c.red))
            } else if b.ahead > 0 || b.behind > 0 {
                let mut s = String::new();
                if b.ahead > 0 {
                    s.push_str(&format!("↑{}", b.ahead));
                }
                if b.behind > 0 {
                    s.push_str(&format!(" ↓{}", b.behind));
                }
                Some((s.trim().to_string(), c.yellow))
            } else {
                None
            };
            let r = item(ui, if b.current { Icon::Check } else { Icon::Branch }, &b.name, false, b.current, track, false);
            let tip = format!("{}{}\nDoble clic para cambiar a esta rama", b.name, b.upstream.as_ref().map(|u| format!(" → {u}")).unwrap_or_default());
            let r = r.on_hover_text(tip);
            let (hash, name) = (b.hash.clone(), b.name.clone());
            if r.double_clicked() {
                let n = name.clone();
                defer.push(Box::new(move |a| a.checkout_branch(n)));
            } else if r.clicked() {
                let h = hash.clone();
                defer.push(Box::new(move |a| a.reveal_commit(&h)));
            }
            let current = b.current;
            r.context_menu(|ui| super::history::branch_menu(ui, &name, &hash, current, defer));
        }
    });

    section(&cm, ui, "remotes", "Remotos", Some(refs.remotes.len()), Some(("Añadir remoto", Box::new(|a: &mut App| a.open_add_remote()))), defer, |ui, defer| {
        if refs.remotes.is_empty() {
            ui.label(widgets::small("Sin remotos"));
        }
        for rn in &refs.remotes {
            let key = format!("remote:{rn}");
            let collapsed = cm.get(&key).copied().unwrap_or(false);
            let r = item(ui, if collapsed { Icon::ChevronRight } else { Icon::ChevronDown }, rn, false, false, None, false);
            if r.clicked() {
                let k = key.clone();
                defer.push(Box::new(move |a| {
                    a.store.settings.collapsed.insert(k, !collapsed);
                    a.persist();
                }));
            }
            let name = rn.clone();
            r.context_menu(|ui| {
                if ui.button(format!("Fetch de {name}")).clicked() {
                    let n = name.clone();
                    defer.push(Box::new(move |a| a.do_fetch(Some(n))));
                    ui.close_menu();
                }
                if ui.button("Editar (nombre y URL)…").clicked() {
                    let n = name.clone();
                    defer.push(Box::new(move |a| a.open_edit_remote(n)));
                    ui.close_menu();
                }
                if ui.button("Abrir en el navegador").clicked() {
                    let n = name.clone();
                    defer.push(Box::new(move |a| {
                        if let Some(p) = a.active_path() {
                            if let Some(r) = ds_core::repo::remotes_info(&p).ok().and_then(|l| l.into_iter().find(|r| r.name == n)) {
                                match ds_core::hosting::RemoteRef::parse(&r.url) {
                                    Some(rr) => {
                                        let _ = open::that(rr.web_url());
                                    }
                                    None => a.toast("No se reconoce la URL del remoto.", crate::app::ToastKind::Info),
                                }
                            }
                        }
                    }));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(RichText::new("Eliminar remoto…").color(theme::p().red)).clicked() {
                    let n = name.clone();
                    defer.push(Box::new(move |a| {
                        let msg = format!("¿Quitar el remoto «{n}» de este repositorio? No borra nada en el servidor.");
                        a.confirm("Eliminar remoto", &msg, None, "Eliminar", true, move |a| {
                            let Some(p) = a.active_path() else { return };
                            let n2 = n.clone();
                            a.op("Eliminar remoto", move || ds_core::repo::remove_remote(&p, &n2), |a, _| a.ok("Remoto eliminado"));
                        });
                    }));
                    ui.close_menu();
                }
            });
            if !collapsed {
                let list: Vec<_> = refs.remote.iter().filter(|x| &x.remote == rn).collect();
                if list.is_empty() {
                    ui.label(widgets::small("      Sin ramas (haz fetch)"));
                }
                for rb in list {
                    let r = item(ui, Icon::Cloud, &rb.branch, false, false, None, true).on_hover_text(format!("{}\nDoble clic para crear una rama local y cambiar a ella", rb.name));
                    if r.double_clicked() {
                        let (n, b) = (rb.name.clone(), rb.branch.clone());
                        defer.push(Box::new(move |a| a.checkout_remote_branch(n, b)));
                    } else if r.clicked() {
                        let h = rb.hash.clone();
                        defer.push(Box::new(move |a| a.reveal_commit(&h)));
                    }
                    let rb = rb.clone();
                    r.context_menu(|ui| super::history::remote_branch_menu(ui, &rb, defer));
                }
            }
        }
    });

    section(&cm, ui, "tags", "Etiquetas", Some(refs.tags.len()), has_head.then(|| {
        let h = refs.head.clone().unwrap_or_default();
        ("Nueva etiqueta", Box::new(move |a: &mut App| a.open_tag(h)) as Box<dyn FnOnce(&mut App)>)
    }), defer, |ui, defer| {
        if refs.tags.is_empty() {
            ui.label(widgets::small("Sin etiquetas"));
        }
        for tg in &refs.tags {
            let r = item(ui, Icon::Tag, &tg.name, false, false, None, false);
            if r.clicked() {
                let h = tg.hash.clone();
                defer.push(Box::new(move |a| a.reveal_commit(&h)));
            }
            let (name, hash) = (tg.name.clone(), tg.hash.clone());
            r.context_menu(|ui| {
                if ui.button("Checkout de la etiqueta…").clicked() {
                    let h = hash.clone();
                    defer.push(Box::new(move |a| a.checkout_commit(h)));
                    ui.close_menu();
                }
                if ui.button("Merge en la rama actual…").clicked() {
                    let n = name.clone();
                    defer.push(Box::new(move |a| a.open_merge_into(n.clone(), n)));
                    ui.close_menu();
                }
                if ui.button("Enviar al remoto").clicked() {
                    let n = name.clone();
                    defer.push(Box::new(move |a| {
                        if let Some(r) = a.tab().and_then(|t| t.refs.as_ref()).and_then(|r| r.default_remote(None)) {
                            a.push_tag(r, n);
                        }
                    }));
                    ui.close_menu();
                }
                if ui.button("Copiar nombre").clicked() {
                    ui.ctx().copy_text(name.clone());
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(RichText::new("Eliminar…").color(theme::p().red)).clicked() {
                    let n = name.clone();
                    defer.push(Box::new(move |a| a.open_delete_tag(n)));
                    ui.close_menu();
                }
            });
        }
    });

    section(&cm, ui, "stashes", "Stashes", Some(refs.stashes.len()), None, defer, |ui, defer| {
        if refs.stashes.is_empty() {
            ui.label(widgets::small("Sin stashes"));
        }
        for s in &refs.stashes {
            let label = s.message.split_once(": ").map(|x| x.1).unwrap_or(&s.message).to_string();
            let sel = view == View::History && selected_hash.as_deref() == Some(s.hash.as_str());
            let r = item(ui, Icon::Stash, &label, sel, false, None, false).on_hover_text(format!("{}: {}\nDoble clic para aplicar", s.reference, s.message));
            let (h, rf, msg) = (s.hash.clone(), s.reference.clone(), s.message.clone());
            if r.double_clicked() {
                let rf = rf.clone();
                defer.push(Box::new(move |a| a.apply_stash(rf, false)));
            } else if r.clicked() {
                let h = h.clone();
                defer.push(Box::new(move |a| {
                    if let Some(t) = a.tab_mut() {
                        t.view = View::History;
                    }
                    a.select_commit(h, false);
                }));
            }
            r.context_menu(|ui| {
                if ui.button("Aplicar").clicked() {
                    let rf = rf.clone();
                    defer.push(Box::new(move |a| a.apply_stash(rf, false)));
                    ui.close_menu();
                }
                if ui.button("Aplicar y eliminar (pop)").clicked() {
                    let rf = rf.clone();
                    defer.push(Box::new(move |a| a.apply_stash(rf, true)));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(RichText::new("Eliminar…").color(theme::p().red)).clicked() {
                    let (rf, m) = (rf.clone(), msg.clone());
                    defer.push(Box::new(move |a| a.drop_stash(rf, m)));
                    ui.close_menu();
                }
            });
        }
    });

    section(&cm, ui, "more", "Más", None, None, defer, |ui, defer| {
        let label = |n: Option<usize>| n.filter(|&x| x > 0).map(|x| (x.to_string(), c.muted));
        if item(ui, Icon::Box, "Submódulos", view == View::Submodules, false, label(sub_count), false).clicked() {
            defer.push(Box::new(|a| a.set_view(View::Submodules)));
        }
        if item(ui, Icon::Layers, "Worktrees", view == View::Worktrees, false, label(wt_count.filter(|&x| x > 1)), false).clicked() {
            defer.push(Box::new(|a| a.set_view(View::Worktrees)));
        }
        if item(ui, Icon::Lock, "Git LFS", view == View::Lfs, false, (!lfs).then(|| ("no instalado".to_string(), c.muted)), false).clicked() {
            defer.push(Box::new(|a| a.set_view(View::Lfs)));
        }
    });
}
