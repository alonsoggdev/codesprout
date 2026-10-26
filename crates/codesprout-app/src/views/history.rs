//! History view: branch explorer (or list), search, and the commit detail panel.

use codesprout_core::history::SearchMode;
use codesprout_core::{Commit, FileChange, RemoteBranch};
use eframe::egui::{self, vec2, Align, Layout, RichText};

use super::Defer;
use crate::app::App;
use crate::icons::{self, Icon};
use crate::repo_state::{RefKind, View, WORKTREE};
use crate::store::{ActionScope, HistoryMode};
use crate::theme;
use crate::widgets;

pub fn show(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    header(ui, app, idx, defer);
    let avail = ui.available_rect_before_wrap();
    let detail_h = app.store.settings.detail_height.clamp(140.0, (avail.height() - 140.0).max(140.0));
    let top = egui::Rect::from_min_max(avail.min, egui::pos2(avail.max.x, avail.max.y - detail_h - 4.0));
    let split = egui::Rect::from_min_max(egui::pos2(avail.min.x, top.max.y), egui::pos2(avail.max.x, top.max.y + 4.0));
    let bottom = egui::Rect::from_min_max(egui::pos2(avail.min.x, split.max.y), avail.max);

    let has_results = app.tabs[idx].search.results.is_some() || app.tabs[idx].search.running || app.tabs[idx].search.error.is_some();
    let mut top_ui = ui.new_child(egui::UiBuilder::new().max_rect(top));
    if has_results {
        let results_w = 360.0f32.min(top.width() * 0.45);
        let left = egui::Rect::from_min_max(top.min, egui::pos2(top.max.x - results_w, top.max.y));
        let right = egui::Rect::from_min_max(egui::pos2(left.max.x, top.min.y), top.max);
        let mut l = top_ui.new_child(egui::UiBuilder::new().max_rect(left));
        main_area(&mut l, app, idx, defer);
        let mut r = top_ui.new_child(egui::UiBuilder::new().max_rect(right));
        search_results(&mut r, app, idx, defer);
    } else {
        main_area(&mut top_ui, app, idx, defer);
    }
    let c = theme::p();
    ui.painter().rect_filled(split, 0.0, c.border);
    let nh = widgets::splitter_h(ui, "detail-split", split, detail_h, 140.0, avail.height() - 140.0, true);
    if (nh - app.store.settings.detail_height).abs() > 0.5 {
        app.store.settings.detail_height = nh;
        app.persist();
    }
    let mut b = ui.new_child(egui::UiBuilder::new().max_rect(bottom));
    detail(&mut b, app, idx, defer);
    ui.allocate_rect(avail, egui::Sense::hover());
}

fn main_area(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    match app.store.settings.history_mode {
        HistoryMode::Explorer => super::explorer::show(ui, app, idx, defer),
        HistoryMode::List => list(ui, app, idx, defer),
    }
}

fn header(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let focus = std::mem::take(&mut app.focus_search);
    let s = &mut app.store.settings;
    let t = &mut app.tabs[idx];
    let explorer = s.history_mode == HistoryMode::Explorer;
    egui::Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut narrow = false;
        ui.horizontal(|ui| {
            // explorer / list
            if icons::button_sized(ui, Icon::Graph, "Explorador de ramas", 26.0, true).clicked() {
                s.history_mode = HistoryMode::Explorer;
                defer.push(Box::new(|a| a.persist()));
            }
            if explorer {
                let r = ui.min_rect();
                ui.painter().line_segment([egui::pos2(r.min.x + 4.0, r.max.y + 2.0), egui::pos2(r.min.x + 22.0, r.max.y + 2.0)], egui::Stroke::new(2.0_f32, c.accent));
            }
            if icons::button_sized(ui, Icon::List, "Lista de commits", 26.0, true).clicked() {
                s.history_mode = HistoryMode::List;
                defer.push(Box::new(|a| a.persist()));
            }
            ui.separator();
            // quick filter (highlights in the explorer, filters the list)
            icons::show(ui, Icon::Search, 14.0, c.muted);
            let r = ui.add(egui::TextEdit::singleline(&mut t.explorer.filter).hint_text("Filtrar: mensaje, autor, hash, rama…").desired_width(220.0));
            if focus {
                r.request_focus();
            }
            if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                t.explorer.filter.clear();
            }
            ui.separator();
            // server-side search
            egui::ComboBox::from_id_salt("search-mode").selected_text(t.search.mode.label()).width(150.0).show_ui(ui, |ui| {
                for m in SearchMode::ALL {
                    ui.selectable_value(&mut t.search.mode, m, m.label());
                }
            });
            let r = ui.add(egui::TextEdit::singleline(&mut t.search.query).hint_text("Buscar en todo el historial").desired_width(200.0));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter || ui.add_enabled(!t.search.query.trim().is_empty(), egui::Button::new("Buscar")).clicked() {
                defer.push(Box::new(|a| a.run_search()));
            }
            // the controls on the right need about 620 px
            narrow = ui.available_width() < 620.0;
            if !narrow {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| view_controls(ui, s, t, explorer, defer));
            }
        });
        if narrow {
            // not enough room on one line: the view controls go on a second one
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| view_controls(ui, s, t, explorer, defer));
            });
        }
    });
}

/// Orientation, zoom, "all branches" and "go to HEAD" (laid out right to left).
fn view_controls(ui: &mut egui::Ui, s: &mut crate::store::Settings, t: &mut crate::repo_state::RepoTab, explorer: bool, defer: &mut Defer) {
    if icons::button(ui, Icon::Target, "Ir a HEAD").clicked() {
        if let Some(h) = t.refs.as_ref().and_then(|r| r.head.clone()) {
            defer.push(Box::new(move |a| a.select_commit(h, true)));
        }
    }
    if explorer {
        if icons::button(ui, Icon::Plus, "Acercar").clicked() {
            s.zoom = (s.zoom * 1.2).min(2.5);
        }
        if icons::button(ui, Icon::Minus, "Alejar").clicked() {
            s.zoom = (s.zoom / 1.2).max(0.4);
        }
        if !s.vertical {
            let mut chrono = s.chronological;
            if ui.toggle_value(&mut chrono, "Cronológico").on_hover_text("Una columna por commit: izquierda a derecha es estrictamente el orden temporal").changed() {
                s.chronological = chrono;
                defer.push(Box::new(|a| {
                    a.persist();
                    a.refresh(true);
                }));
            }
        }
        ui.separator();
        // right-to-left layout: added in reverse visual order
        let mut vertical = s.vertical;
        ui.selectable_value(&mut vertical, true, "Vertical").on_hover_text("Ramas en columnas, lo más reciente arriba, con el mensaje de cada commit al lado");
        ui.selectable_value(&mut vertical, false, "Horizontal").on_hover_text("Ramas en filas, el tiempo avanza hacia la derecha");
        if vertical != s.vertical {
            s.vertical = vertical;
            defer.push(Box::new(|a| {
                for t in &mut a.tabs {
                    t.explorer.initialized = false;
                    t.explorer.center_on = t.selected.clone();
                }
                a.persist();
                a.refresh(true);
            }));
        }
    }
    let mut all = s.all_branches;
    if ui.checkbox(&mut all, "Todas las ramas").changed() {
        s.all_branches = all;
        defer.push(Box::new(|a| {
            a.persist();
            a.refresh(true);
        }));
    }
}

/// Whether a commit matches the quick filter.
pub fn matches(t: &crate::repo_state::RepoTab, c: &Commit, q: &str) -> bool {
    c.subject.to_lowercase().contains(q)
        || c.author.to_lowercase().contains(q)
        || c.email.to_lowercase().contains(q)
        || c.hash.starts_with(q)
        || t.ref_map.get(&c.hash).is_some_and(|v| v.iter().any(|r| r.name.to_lowercase().contains(q)))
}

fn list(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &mut app.tabs[idx];
    if !t.loaded {
        widgets::placeholder(ui, Icon::History, "Cargando historial…", "");
        return;
    }
    let q = t.explorer.filter.trim().to_lowercase();
    let mut rows: Vec<usize> = (0..t.commits.len()).filter(|&i| q.is_empty() || matches(t, &t.commits[i], &q)).collect();
    let wt = t.layout.worktree.is_some() && q.is_empty();
    if t.commits.is_empty() && !wt {
        widgets::placeholder(ui, Icon::Commit, "Sin commits", "Este repositorio todavía no tiene commits. Prepara archivos en «Cambios» y crea el primero.");
        return;
    }
    let row_h = 26.0;
    let scroll_to = t.list_scroll_to.take();
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("list", idx));
    if let Some(i) = scroll_to {
        if let Some(pos) = rows.iter().position(|&r| r == i) {
            area = area.vertical_scroll_offset(((pos + wt as usize) as f32 * row_h - 120.0).max(0.0));
        }
    }
    let total = rows.len() + wt as usize;
    if wt {
        rows.insert(0, usize::MAX);
    }
    area.show_rows(ui, row_h, total, |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for k in range {
            let i = rows[k];
            if i == usize::MAX {
                let n = t.change_count();
                let sel = t.selected.as_deref() == Some(WORKTREE);
                let r = widgets::row(ui, sel, row_h, |ui| {
                    ui.add_space(18.0);
                    ui.label(RichText::new(format!("Cambios sin confirmar ({n})")).italics().color(c.yellow));
                });
                if r.clicked() {
                    t.selected = Some(WORKTREE.into());
                }
                if r.double_clicked() {
                    defer.push(Box::new(|a| a.set_view(View::Changes)));
                }
                continue;
            }
            let cm = &t.commits[i];
            let sel = t.selected.as_deref() == Some(cm.hash.as_str());
            let cmp = t.compare_with.as_deref() == Some(cm.hash.as_str());
            let lane = t.layout.nodes.get(i).and_then(|n| t.layout.lanes.get(n.lane));
            let color = lane.map_or(c.accent, |l| theme::lane_color(l.color));
            let is_head = t.refs.as_ref().and_then(|r| r.head.as_deref()) == Some(cm.hash.as_str());
            let r = widgets::row(ui, sel || cmp, row_h, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), if is_head { 5.5 } else { 4.5 }, color);
                if cmp {
                    ui.painter().circle_stroke(rect.center(), 7.0, egui::Stroke::new(1.5_f32, c.yellow));
                }
                ref_badges(ui, t.ref_map.get(&cm.hash));
                let w = ui.available_width();
                ui.allocate_ui_with_layout(vec2((w - 330.0).max(60.0), row_h), Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(&cm.subject).truncate().selectable(false));
                });
                ui.allocate_ui_with_layout(vec2(140.0, row_h), Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(&cm.author).color(c.text2)).truncate().selectable(false));
                });
                ui.allocate_ui_with_layout(vec2(110.0, row_h), Layout::left_to_right(Align::Center), |ui| {
                    ui.label(RichText::new(widgets::rel_time(cm.time)).color(c.muted).size(12.0)).on_hover_text(widgets::full_time(cm.time));
                });
                ui.label(RichText::new(&cm.hash[..8]).font(theme::mono(11.5)).color(c.muted));
            });
            let hash = cm.hash.clone();
            if r.clicked() {
                if ui.input(|i| i.modifiers.command) {
                    t.compare_with = Some(hash.clone());
                } else {
                    let h = hash.clone();
                    defer.push(Box::new(move |a| a.select_commit(h, false)));
                }
            }
            if r.double_clicked() {
                let h = hash.clone();
                defer.push(Box::new(move |a| double_click_commit(a, h)));
            }
            let commit = cm.clone();
            r.context_menu(|ui| commit_menu(ui, &commit, defer));
        }
    });
}

pub fn ref_badges(ui: &mut egui::Ui, refs: Option<&Vec<crate::repo_state::RefBadge>>) {
    let Some(refs) = refs else { return };
    let c = theme::p();
    for r in refs.iter().take(3) {
        let (color, prefix) = match r.kind {
            RefKind::Branch { current: true } => (c.accent, "✓ "),
            RefKind::Branch { .. } => (c.accent, ""),
            RefKind::Head => (c.red, ""),
            RefKind::Remote => (c.purple, "☁ "),
            RefKind::Tag => (c.yellow, "⏷ "),
        };
        widgets::pill(ui, format!("{prefix}{}", r.name), color);
    }
    if refs.len() > 3 {
        widgets::pill(ui, format!("+{}", refs.len() - 3), c.muted).on_hover_text(refs.iter().skip(3).map(|r| r.name.clone()).collect::<Vec<_>>().join("\n"));
    }
}

pub fn double_click_commit(app: &mut App, hash: String) {
    let Some(t) = app.tab() else { return };
    if hash == WORKTREE {
        return app.set_view(View::Changes);
    }
    let Some(r) = &t.refs else { return };
    if r.head.as_deref() == Some(hash.as_str()) {
        return;
    }
    match r.local.iter().find(|b| b.hash == hash && !b.current) {
        Some(b) => {
            let n = b.name.clone();
            app.checkout_branch(n)
        }
        None => app.checkout_commit(hash),
    }
}

fn search_results(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &mut app.tabs[idx];
    egui::Frame::new().fill(c.panel).stroke(egui::Stroke::new(1.0_f32, c.border)).inner_margin(8).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.horizontal(|ui| {
            let n = t.search.results.as_ref().map_or(0, |r| r.len());
            ui.label(RichText::new(format!("Resultados · {n}")).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Close, "Cerrar búsqueda").clicked() {
                    t.search.results = None;
                    t.search.error = None;
                }
            });
        });
        if t.search.running {
            ui.spinner();
        }
        if let Some(e) = &t.search.error {
            widgets::error_box(ui, e);
        }
        let Some(results) = &t.search.results else { return };
        if results.is_empty() {
            ui.label(widgets::muted("Ningún commit coincide."));
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, 40.0, results.len(), |ui, range| {
            for cm in &results[range] {
                let sel = t.selected.as_deref() == Some(cm.hash.as_str());
                let width = ui.available_width();
                let (rect, r) = ui.allocate_exact_size(vec2(width, 40.0), egui::Sense::click());
                if sel {
                    ui.painter().rect_filled(rect, 5.0, c.sel);
                } else if r.hovered() {
                    ui.painter().rect_filled(rect, 5.0, c.hover);
                }
                let p = ui.painter().with_clip_rect(rect.shrink(4.0));
                p.text(rect.min + vec2(8.0, 12.0), egui::Align2::LEFT_CENTER, &cm.subject, egui::FontId::proportional(13.0), c.text);
                p.text(rect.min + vec2(8.0, 29.0), egui::Align2::LEFT_CENTER, format!("{} · {} · {}", &cm.hash[..8], cm.author, widgets::rel_time(cm.time)), egui::FontId::proportional(11.0), c.muted);
                if r.clicked() {
                    let h = cm.hash.clone();
                    defer.push(Box::new(move |a| a.reveal_commit(&h)));
                }
                let commit = cm.clone();
                r.context_menu(|ui| commit_menu(ui, &commit, defer));
            }
        });
    });
}

// ---------------------------------------------------------------- detail panel

pub fn detail(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let t = &app.tabs[idx];
    if t.selected.as_deref() == Some(WORKTREE) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new("Cambios sin confirmar").size(15.0));
            if widgets::primary_button(ui, "Ver cambios", true).clicked() {
                defer.push(Box::new(|a| a.set_view(View::Changes)));
            }
        });
        return;
    }
    let Some(d) = t.detail.clone() else {
        if t.detail_loading {
            ui.centered_and_justified(|ui| ui.spinner());
        } else {
            widgets::placeholder(ui, Icon::Commit, "Selecciona un commit", "Haz clic en un nodo del explorador");
        }
        return;
    };
    let rect = ui.available_rect_before_wrap();
    let left_w = (rect.width() * 0.36).clamp(260.0, 460.0);
    let left = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + left_w, rect.max.y));
    let right = egui::Rect::from_min_max(egui::pos2(left.max.x + 1.0, rect.min.y), rect.max);
    ui.painter().vline(left.max.x, rect.y_range(), egui::Stroke::new(1.0_f32, c.border));
    let stash = t.refs.as_ref().and_then(|r| r.stashes.iter().find(|s| s.hash == d.hash)).cloned();
    let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(left.shrink(10.0)));
    egui::ScrollArea::vertical().id_salt("detail-left").auto_shrink([false, false]).show(&mut lu, |ui| {
        if let Some(s) = &stash {
            widgets::pill(ui, format!("stash · {}", s.reference), c.purple);
        }
        let (subject, body) = d.message.split_once('\n').map(|(a, b)| (a, b.trim())).unwrap_or((d.message.as_str(), ""));
        ui.add(egui::Label::new(RichText::new(subject).strong().size(14.5)).wrap());
        if !body.is_empty() {
            ui.add(egui::Label::new(RichText::new(body).color(c.text2)).wrap());
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 15.0, widgets::hash_color(&d.email));
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, widgets::initials(&d.author), egui::FontId::proportional(12.0), egui::Color32::WHITE);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&d.author).strong());
                    ui.label(widgets::muted(format!("<{}>", d.email)));
                });
                ui.label(widgets::small(format!("{} · {}", widgets::rel_time(d.time), widgets::full_time(d.time))));
            });
        });
        egui::Grid::new("kv").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
            ui.label(widgets::small("Commit"));
            ui.horizontal(|ui| {
                if ui.link(RichText::new(&d.hash[..12]).font(theme::mono(12.0))).on_hover_text("Copiar hash").clicked() {
                    let h = d.hash.clone();
                    let ctx = ui.ctx().clone();
                    defer.push(Box::new(move |a| a.copy(&ctx, h, "Hash copiado")));
                }
                if icons::button_sized(ui, Icon::Copy, "Copiar hash completo", 20.0, true).clicked() {
                    let h = d.hash.clone();
                    let ctx = ui.ctx().clone();
                    defer.push(Box::new(move |a| a.copy(&ctx, h, "Hash copiado")));
                }
            });
            ui.end_row();
            if !d.parents.is_empty() {
                ui.label(widgets::small(if d.parents.len() > 1 { "Padres" } else { "Padre" }));
                ui.horizontal(|ui| {
                    for p in &d.parents {
                        if ui.link(RichText::new(&p[..8]).font(theme::mono(12.0))).clicked() {
                            let h = p.clone();
                            defer.push(Box::new(move |a| a.reveal_commit(&h)));
                        }
                    }
                });
                ui.end_row();
            }
            if d.committer != d.author {
                ui.label(widgets::small("Committer"));
                ui.label(format!("{} · {}", d.committer, widgets::rel_time(d.commit_time)));
                ui.end_row();
            }
            if let Some(sg) = d.signature.as_ref().filter(|s| s.is_signed()) {
                ui.label(widgets::small("Firma"));
                let color = match sg.code {
                    'G' | 'U' => c.green,
                    'B' | 'R' => c.red,
                    _ => c.yellow,
                };
                widgets::pill(ui, sg.label(), color).on_hover_text(format!("{}\nClave: {}", sg.signer, sg.key));
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        ui.label(widgets::small(format!("ARCHIVOS · {}", d.files.len())));
        if d.files.is_empty() {
            ui.label(widgets::muted("Este commit no modifica archivos."));
        }
        let selected = app.tabs[idx].detail_file;
        for (i, f) in d.files.iter().enumerate() {
            let r = widgets::row(ui, selected == Some(i), 24.0, |ui| file_label(ui, f)).on_hover_text(match &f.orig {
                Some(o) => format!("{o} → {}", f.path),
                None => f.path.clone(),
            });
            if r.clicked() {
                defer.push(Box::new(move |a| {
                    if let Some(t) = a.tab_mut() {
                        t.detail_file = Some(i);
                        let k = t.key.clone();
                        a.load_detail_diff(&k);
                    }
                }));
            }
            let (f, hash, base) = (f.clone(), d.hash.clone(), d.base.clone());
            r.context_menu(|ui| commit_file_menu(ui, &f, &hash, &base, defer));
        }
    });
    let mut ru = ui.new_child(egui::UiBuilder::new().max_rect(right));
    let t = &mut app.tabs[idx];
    match &mut t.detail_diff {
        Some(m) => {
            let f = m.file.clone();
            let (hash, base) = (d.hash.clone(), d.base.clone());
            super::diff::header(&mut ru, &f.path, f.orig.as_deref(), |ui| {
                if icons::button(ui, Icon::History, "Historial del archivo").clicked() {
                    let p = f.path.clone();
                    defer.push(Box::new(move |a| a.open_file_history(&p)));
                }
                if icons::button(ui, Icon::User, "Blame en este commit").clicked() {
                    let (p, h) = (f.path.clone(), hash.clone());
                    defer.push(Box::new(move |a| a.open_blame(&p, Some(h))));
                }
                if icons::button(ui, Icon::External, "Abrir en la herramienta de diff externa").clicked() {
                    let ff = f.clone();
                    defer.push(Box::new(move |a| a.open_difftool(&ff, false, Some((base, hash)))));
                }
            });
            let acts = super::diff::body(&mut ru, m, &mut app.store.settings);
            super::diff::apply_actions(acts, defer);
        }
        None if d.files.is_empty() => widgets::placeholder(&mut ru, Icon::File, "Sin archivos", ""),
        None => {
            ru.centered_and_justified(|ui| ui.spinner());
        }
    }
}

pub fn file_label(ui: &mut egui::Ui, f: &FileChange) {
    let c = theme::p();
    widgets::status_badge(ui, &f.status);
    let (dir, base) = widgets::split_path(&f.path);
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::proportional(13.0);
    job.append(base, 0.0, egui::TextFormat { color: c.text, font_id: font.clone(), ..Default::default() });
    if !dir.is_empty() {
        job.append(&format!("  {dir}"), 0.0, egui::TextFormat { color: c.muted, font_id: egui::FontId::proportional(12.0), ..Default::default() });
    }
    if let Some(o) = &f.orig {
        job.append(&format!("  ← {o}"), 0.0, egui::TextFormat { color: c.purple, font_id: egui::FontId::proportional(12.0), ..Default::default() });
    }
    ui.add(egui::Label::new(job).truncate().selectable(false));
}

// ---------------------------------------------------------------- context menus

fn mi(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let r = ui.add_enabled(enabled, egui::Button::new(text));
    if r.clicked() {
        ui.close_menu();
        true
    } else {
        false
    }
}

fn mi_danger(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let r = ui.add_enabled(enabled, egui::Button::new(RichText::new(text).color(theme::p().red)));
    if r.clicked() {
        ui.close_menu();
        true
    } else {
        false
    }
}

pub fn commit_menu(ui: &mut egui::Ui, c: &Commit, defer: &mut Defer) {
    let short = c.hash[..8].to_string();
    ui.label(widgets::small(format!("{short} · {}", c.subject.chars().take(50).collect::<String>())));
    ui.separator();
    let (h, s, merge) = (c.hash.clone(), c.subject.clone(), c.parents.len() > 1);
    macro_rules! act {
        ($text:expr, $f:expr) => {
            if mi(ui, $text, true) {
                let (h, s) = (h.clone(), s.clone());
                let _ = &s;
                defer.push(Box::new(move |a: &mut App| $f(a, h, s)));
            }
        };
    }
    act!("Checkout de este commit…", |a: &mut App, h, _| a.checkout_commit(h));
    act!("Crear rama aquí…", |a: &mut App, h, _| a.open_new_branch(Some(h)));
    act!("Crear etiqueta aquí…", |a: &mut App, h, _| a.open_tag(h));
    ui.separator();
    act!("Merge en la rama actual…", |a: &mut App, h: String, _| {
        let l = h[..8].to_string();
        a.open_merge_into(h, l)
    });
    act!("Rebase de la rama actual sobre este commit…", |a: &mut App, h: String, _| {
        let l = h[..8].to_string();
        a.rebase_onto(h, l)
    });
    act!("Rebase interactivo desde aquí…", |a: &mut App, h, _| a.open_interactive_rebase(Some(h)));
    act!("Cherry-pick…", move |a: &mut App, h, s| a.cherry_pick(h, s, merge));
    act!("Revertir commit…", move |a: &mut App, h, s| a.revert_commit(h, s, merge));
    act!("Reset de la rama actual a este commit…", |a: &mut App, h, _| a.open_reset(h));
    ui.separator();
    act!("Comparar con la versión actual (HEAD)", |a: &mut App, h: String, _| {
        let l = h[..8].to_string();
        a.open_compare(h, "HEAD".into(), l, "HEAD".into())
    });
    act!("Comparar con el commit marcado (Ctrl+clic)", |a: &mut App, h: String, _| {
        match a.tab().and_then(|t| t.compare_with.clone()) {
            Some(o) if o != h => {
                let (lo, lh) = (o[..8].to_string(), h[..8].to_string());
                a.open_compare(o, h, lo, lh)
            }
            _ => a.toast("Marca primero otro commit con Ctrl+clic.", crate::app::ToastKind::Info),
        }
    });
    act!("Guardar como parche…", |a: &mut App, h: String, _| {
        if let Some(dir) = crate::actions::pick_folder("Carpeta donde guardar el parche") {
            if let Some(p) = a.active_path() {
                a.op("Crear parche", move || codesprout_core::extras::create_patches(&p, &[h], &dir), |a, files| {
                    a.ok(format!("Parche guardado: {}", files.first().map(|f| f.display().to_string()).unwrap_or_default()))
                });
            }
        }
    });
    act!("Exportar como archivo (zip)…", |a: &mut App, h: String, _| {
        if let Some(file) = rfd::FileDialog::new().set_file_name(format!("{}.zip", &h[..8])).add_filter("Zip", &["zip"]).add_filter("Tar", &["tar", "tar.gz"]).save_file() {
            if let Some(p) = a.active_path() {
                a.op("Exportar", move || codesprout_core::extras::archive(&p, &h, &file), |a, _| a.ok("Archivo exportado"));
            }
        }
    });
    custom_actions_menu(ui, ActionScope::Commit, Some(h.clone()), None, defer);
    ui.separator();
    if mi(ui, "Copiar hash", true) {
        ui.ctx().copy_text(h.clone());
    }
    if mi(ui, "Copiar mensaje", true) {
        ui.ctx().copy_text(s.clone());
    }
}

pub fn custom_actions_menu(ui: &mut egui::Ui, scope: ActionScope, sha: Option<String>, file: Option<String>, defer: &mut Defer) {
    let actions: Vec<(usize, String)> = ui.ctx().data(|d| d.get_temp::<Vec<(usize, String, ActionScope)>>(egui::Id::new("custom-actions"))).unwrap_or_default().into_iter().filter(|(_, _, s)| *s == scope).map(|(i, n, _)| (i, n)).collect();
    if actions.is_empty() {
        return;
    }
    ui.separator();
    ui.menu_button("Acciones personalizadas", |ui| {
        for (i, name) in actions {
            if mi(ui, &name, true) {
                let (s, f) = (sha.clone(), file.clone());
                defer.push(Box::new(move |a| a.run_custom_action(i, s, f)));
            }
        }
    });
}

pub fn branch_menu(ui: &mut egui::Ui, name: &str, hash: &str, current: bool, defer: &mut Defer) {
    ui.label(widgets::small(name));
    ui.separator();
    let n = name.to_string();
    let h = hash.to_string();
    if mi(ui, "Cambiar a esta rama", !current) {
        let n = n.clone();
        defer.push(Box::new(move |a| a.checkout_branch(n)));
    }
    if mi(ui, "Merge en la rama actual…", !current) {
        let n = n.clone();
        defer.push(Box::new(move |a| a.open_merge_into(n.clone(), n)));
    }
    if mi(ui, "Rebase de la rama actual sobre esta…", !current) {
        let n = n.clone();
        defer.push(Box::new(move |a| a.rebase_onto(n.clone(), n)));
    }
    if mi(ui, "Comparar con la rama actual", !current) {
        let n = n.clone();
        defer.push(Box::new(move |a| a.open_compare("HEAD".into(), n.clone(), "HEAD".into(), n)));
    }
    ui.separator();
    if mi(ui, "Push…", true) {
        let n = n.clone();
        defer.push(Box::new(move |a| a.open_push(Some(n))));
    }
    if current && mi(ui, "Pull…", true) {
        defer.push(Box::new(|a| a.open_pull()));
    }
    if mi(ui, "Crear pull request…", true) {
        let n = n.clone();
        defer.push(Box::new(move |a| a.open_create_pr(Some(n))));
    }
    if mi(ui, "Nueva rama desde aquí…", true) {
        let h = h.clone();
        defer.push(Box::new(move |a| a.open_new_branch(Some(h))));
    }
    if mi(ui, "Renombrar…", true) {
        let n = n.clone();
        defer.push(Box::new(move |a| a.open_rename_branch(n)));
    }
    if mi(ui, "Copiar nombre", true) {
        ui.ctx().copy_text(n.clone());
    }
    ui.separator();
    if mi_danger(ui, "Eliminar…", !current) {
        defer.push(Box::new(move |a| a.open_delete_branch(n)));
    }
}

pub fn remote_branch_menu(ui: &mut egui::Ui, rb: &RemoteBranch, defer: &mut Defer) {
    ui.label(widgets::small(&rb.name));
    ui.separator();
    let rb = rb.clone();
    if mi(ui, "Checkout (crear rama local)…", true) {
        let (n, b) = (rb.name.clone(), rb.branch.clone());
        defer.push(Box::new(move |a| a.checkout_remote_branch(n, b)));
    }
    if mi(ui, "Merge en la rama actual…", true) {
        let n = rb.name.clone();
        defer.push(Box::new(move |a| a.open_merge_into(n.clone(), n)));
    }
    if mi(ui, "Rebase de la rama actual sobre esta…", true) {
        let n = rb.name.clone();
        defer.push(Box::new(move |a| a.rebase_onto(n.clone(), n)));
    }
    if mi(ui, "Nueva rama desde aquí…", true) {
        let h = rb.hash.clone();
        defer.push(Box::new(move |a| a.open_new_branch(Some(h))));
    }
    if mi(ui, "Copiar nombre", true) {
        ui.ctx().copy_text(rb.name.clone());
    }
    ui.separator();
    if mi_danger(ui, "Eliminar del remoto…", true) {
        defer.push(Box::new(move |a| {
            let msg = format!("¿Eliminar {} del remoto {}? Afecta a todas las personas que usan el repositorio.", rb.branch, rb.remote);
            let (r, b) = (rb.remote.clone(), rb.branch.clone());
            a.confirm("Eliminar rama remota", &msg, None, "Eliminar", true, move |a| a.delete_remote_branch(r, b));
        }));
    }
}

pub fn commit_file_menu(ui: &mut egui::Ui, f: &FileChange, hash: &str, base: &str, defer: &mut Defer) {
    ui.label(widgets::small(&f.path));
    ui.separator();
    let p = f.path.clone();
    if mi(ui, "Historial del archivo", true) {
        let p = p.clone();
        defer.push(Box::new(move |a| a.open_file_history(&p)));
    }
    if mi(ui, "Blame en este commit", f.status != "D") {
        let (p, h) = (p.clone(), hash.to_string());
        defer.push(Box::new(move |a| a.open_blame(&p, Some(h))));
    }
    if mi(ui, "Abrir en la herramienta de diff externa", true) {
        let (ff, b, h) = (f.clone(), base.to_string(), hash.to_string());
        defer.push(Box::new(move |a| a.open_difftool(&ff, false, Some((b, h)))));
    }
    if mi(ui, "Abrir archivo (versión actual)", f.status != "D") {
        let p = p.clone();
        defer.push(Box::new(move |a| {
            if let Some(r) = a.active_path() {
                let _ = open::that(r.join(&p));
            }
        }));
    }
    custom_actions_menu(ui, ActionScope::File, Some(hash.to_string()), Some(p.clone()), defer);
    if mi(ui, "Copiar ruta", true) {
        ui.ctx().copy_text(p);
    }
}

pub fn back_to_history(ui: &mut egui::Ui, defer: &mut Defer) {
    if widgets::icon_text_button(ui, Icon::ChevronRight, "Volver al historial", true).clicked() {
        defer.push(Box::new(|a| a.set_view(View::History)));
    }
}
