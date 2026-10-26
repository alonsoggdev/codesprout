//! Changes view: conflicted / unstaged / staged file lists, commit box and diff.

use codesprout_core::{FileChange, RepoState};
use eframe::egui::{self, vec2, Align, Key, Layout, RichText, Sense};

use super::Defer;
use crate::app::App;
use crate::icons::{self, Icon};
use crate::repo_state::{DiffMode, FileSel, ListKind};
use crate::store::ActionScope;
use crate::theme;
use crate::widgets;

pub fn show(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let rect = ui.available_rect_before_wrap();
    let lw = app.store.settings.changes_width.clamp(260.0, (rect.width() - 300.0).max(260.0));
    let left = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + lw, rect.max.y));
    let split = egui::Rect::from_min_max(egui::pos2(left.max.x, rect.min.y), egui::pos2(left.max.x + 4.0, rect.max.y));
    let right = egui::Rect::from_min_max(egui::pos2(split.max.x, rect.min.y), rect.max);
    ui.painter().rect_filled(split, 0.0, c.border);
    let r = ui.interact(split, ui.id().with("changes-split"), Sense::drag());
    if r.hovered() || r.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if r.dragged() {
        app.store.settings.changes_width = (lw + r.drag_delta().x).clamp(260.0, rect.width() - 300.0);
        app.persist();
    }
    let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(left));
    lists(&mut lu, app, idx, defer);
    let mut ru = ui.new_child(egui::UiBuilder::new().max_rect(right));
    diff_area(&mut ru, app, idx, defer);
    ui.allocate_rect(rect, Sense::hover());
    keyboard(ui, app, idx, defer);
}

fn lists(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let busy = app.busy.is_some();
    let t = &app.tabs[idx];
    let Some(s) = t.status.clone() else { return };
    let commit_h = 210.0;
    let rect = ui.available_rect_before_wrap();
    let lists_rect = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - commit_h));
    let commit_rect = egui::Rect::from_min_max(egui::pos2(rect.min.x, lists_rect.max.y), rect.max);
    let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(lists_rect));
    let has_conf = !s.conflicted.is_empty();
    let parts = if has_conf { 3.0 } else { 2.0 };
    let each = lists_rect.height() / parts;
    if has_conf {
        panel(&mut lu, app, idx, ListKind::Conflicted, &s.conflicted, each, busy, defer);
    }
    panel(&mut lu, app, idx, ListKind::Unstaged, &s.unstaged, each, busy, defer);
    panel(&mut lu, app, idx, ListKind::Staged, &s.staged, each, busy, defer);
    let mut cu = ui.new_child(egui::UiBuilder::new().max_rect(commit_rect));
    egui::Frame::new().fill(c.panel).inner_margin(10).show(&mut cu, |ui| {
        ui.set_min_size(ui.available_size());
        commit_box(ui, app, idx, defer);
    });
}

#[allow(clippy::too_many_arguments)]
fn panel(ui: &mut egui::Ui, app: &mut App, idx: usize, kind: ListKind, files: &[FileChange], height: f32, busy: bool, defer: &mut Defer) {
    let c = theme::p();
    let t = &mut app.tabs[idx];
    let title = match kind {
        ListKind::Conflicted => "Conflictos",
        ListKind::Staged => "Preparados",
        ListKind::Unstaged => "Sin preparar",
    };
    let rect = ui.allocate_space(vec2(ui.available_width(), height)).1;
    let mut pu = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    egui::Frame::new().fill(c.panel).stroke(egui::Stroke::new(1.0_f32, c.border)).inner_margin(egui::Margin::symmetric(6, 4)).show(&mut pu, |ui| {
        ui.set_min_size(ui.available_size());
        ui.horizontal(|ui| {
            if kind == ListKind::Conflicted {
                icons::show(ui, Icon::Alert, 14.0, c.red);
            }
            ui.label(RichText::new(title).strong());
            widgets::pill(ui, files.len().to_string(), c.muted);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match kind {
                ListKind::Staged => {
                    if ui.add_enabled(!busy && !files.is_empty(), egui::Button::new("Quitar todo").small()).clicked() {
                        defer.push(Box::new(|a| a.unstage_all()));
                    }
                }
                ListKind::Unstaged => {
                    if ui.add_enabled(!busy && !files.is_empty(), egui::Button::new("Preparar todo").small()).clicked() {
                        defer.push(Box::new(|a| a.stage_all()));
                    }
                }
                ListKind::Conflicted => {
                    ui.label(widgets::small("Doble clic: abrir"));
                }
            });
        });
        if files.is_empty() {
            ui.label(widgets::muted(if kind == ListKind::Staged { "Nada preparado. Marca archivos de arriba para incluirlos en el commit." } else { "No hay cambios en el directorio de trabajo." }));
            return;
        }
        egui::ScrollArea::vertical().id_salt(("files", kind)).auto_shrink([false, false]).show_rows(ui, 24.0, files.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for f in &files[range] {
                let selected = t.sel.list == Some(kind) && t.sel.paths.contains(&f.path);
                // the row takes the click; it toggles staging when it lands on the checkbox
                let mut cbox = egui::Rect::NOTHING;
                let r = widgets::row(ui, selected, 24.0, |ui| {
                    if kind != ListKind::Conflicted {
                        let (cr, cresp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                        cbox = cr.expand(3.0);
                        let checked = kind == ListKind::Staged;
                        ui.painter().rect(cr.shrink(1.0), 3.0, if checked { c.accent } else { c.panel2 }, egui::Stroke::new(1.0_f32, if checked { c.accent } else { c.border_strong }), egui::StrokeKind::Inside);
                        if checked {
                            icons::paint(ui.painter(), cr.shrink(3.0), Icon::Check, egui::Color32::WHITE);
                        }
                        cresp.on_hover_text(if checked { "Quitar del área de preparación" } else { "Preparar para el commit" });
                    }
                    super::history::file_label(ui, f);
                });
                let r = r.on_hover_text(match &f.orig {
                    Some(o) => format!("{o} → {}", f.path),
                    None => f.path.clone(),
                });
                let toggle = r.clicked() && r.interact_pointer_pos().is_some_and(|p| cbox.contains(p));
                if toggle {
                    let targets = if selected && t.sel.paths.len() > 1 { files.iter().filter(|x| t.sel.paths.contains(&x.path)).cloned().collect() } else { vec![f.clone()] };
                    push_toggle(kind, targets, defer);
                } else if r.double_clicked() {
                    match kind {
                        ListKind::Conflicted => {
                            let p = t.path.join(&f.path);
                            let _ = open::that(p);
                        }
                        _ => push_toggle(kind, vec![f.clone()], defer),
                    }
                } else if r.clicked() || r.secondary_clicked() && !selected {
                    let mods = ui.input(|i| i.modifiers);
                    click(t, kind, f, files, mods.command && r.clicked(), mods.shift && r.clicked());
                    let k = t.key.clone();
                    defer.push(Box::new(move |a| a.load_changes_diff(&k)));
                }
                r.context_menu(|ui| file_menu(ui, t, kind, files, defer));
            }
        });
    });
}

fn push_toggle(kind: ListKind, files: Vec<FileChange>, defer: &mut Defer) {
    match kind {
        ListKind::Staged => defer.push(Box::new(move |a| a.unstage_files(files))),
        ListKind::Unstaged => defer.push(Box::new(move |a| a.stage_files(files))),
        ListKind::Conflicted => {}
    }
}

fn click(t: &mut crate::repo_state::RepoTab, kind: ListKind, f: &FileChange, files: &[FileChange], ctrl: bool, shift: bool) {
    if t.sel.list != Some(kind) {
        t.sel = FileSel { list: Some(kind), ..Default::default() };
    }
    let sel = &mut t.sel;
    if ctrl {
        if sel.paths.contains(&f.path) && sel.paths.len() > 1 {
            sel.paths.remove(&f.path);
        } else {
            sel.paths.insert(f.path.clone());
        }
        sel.anchor = Some(f.path.clone());
    } else if shift && sel.anchor.is_some() {
        let a = files.iter().position(|x| Some(&x.path) == sel.anchor.as_ref()).unwrap_or(0);
        let b = files.iter().position(|x| x.path == f.path).unwrap_or(0);
        sel.paths = files[a.min(b)..=a.max(b)].iter().map(|x| x.path.clone()).collect();
    } else {
        sel.paths = [f.path.clone()].into();
        sel.anchor = Some(f.path.clone());
    }
    sel.focus = Some(f.path.clone());
    t.last_focus = Some(f.path.clone());
}

fn keyboard(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    if ui.ctx().wants_keyboard_input() || !app.dialogs.is_empty() {
        return;
    }
    let t = &mut app.tabs[idx];
    let Some(kind) = t.sel.list else { return };
    let files: Vec<FileChange> = t.list(kind).to_vec();
    if files.is_empty() {
        return;
    }
    let (down, up, space, del, all, shift) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) || i.consume_key(egui::Modifiers::SHIFT, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) || i.consume_key(egui::Modifiers::SHIFT, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::Space) || i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Delete),
            i.consume_key(egui::Modifiers::COMMAND, Key::A),
            i.modifiers.shift,
        )
    });
    let cur = files.iter().position(|f| Some(&f.path) == t.sel.focus.as_ref()).unwrap_or(0);
    if down || up {
        let next = if down { (cur + 1).min(files.len() - 1) } else { cur.saturating_sub(1) };
        click(t, kind, &files[next], &files, false, shift);
        let k = t.key.clone();
        defer.push(Box::new(move |a| a.load_changes_diff(&k)));
    }
    let selected: Vec<FileChange> = files.iter().filter(|f| t.sel.paths.contains(&f.path)).cloned().collect();
    if space {
        push_toggle(kind, selected.clone(), defer);
    }
    if del && kind == ListKind::Unstaged {
        defer.push(Box::new(move |a| a.discard_files(selected)));
    }
    if all {
        t.sel.paths = files.iter().map(|f| f.path.clone()).collect();
    }
}

fn file_menu(ui: &mut egui::Ui, t: &crate::repo_state::RepoTab, kind: ListKind, files: &[FileChange], defer: &mut Defer) {
    let sel: Vec<FileChange> = files.iter().filter(|f| t.sel.paths.contains(&f.path)).cloned().collect();
    if sel.is_empty() {
        return;
    }
    let one = (sel.len() == 1).then(|| sel[0].clone());
    ui.label(widgets::small(one.as_ref().map_or(format!("{} archivos seleccionados", sel.len()), |f| f.path.clone())));
    ui.separator();
    let untracked: Vec<FileChange> = sel.iter().filter(|f| f.is_untracked()).cloned().collect();
    let close = |ui: &mut egui::Ui, r: egui::Response| {
        if r.clicked() {
            ui.close_menu();
            true
        } else {
            false
        }
    };
    match kind {
        ListKind::Unstaged => {
            let r = ui.button("Preparar");
            if close(ui, r) {
                let s = sel.clone();
                defer.push(Box::new(move |a| a.stage_files(s)));
            }
            let r = ui.button(RichText::new(if untracked.len() == sel.len() { "Eliminar (a la papelera)" } else { "Descartar cambios" }).color(theme::p().red));
            if close(ui, r) {
                let s = sel.clone();
                defer.push(Box::new(move |a| a.discard_files(s)));
            }
            if !untracked.is_empty() {
                ui.separator();
                let r = ui.button(if untracked.len() > 1 { "Ignorar archivos (.gitignore)" } else { "Ignorar archivo (.gitignore)" });
                if close(ui, r) {
                    let u = untracked.clone();
                    defer.push(Box::new(move |a| a.ignore_files(u, false)));
                }
                if let Some(ext) = one.as_ref().and_then(|f| std::path::Path::new(&f.path).extension()).map(|e| e.to_string_lossy().into_owned()) {
                    let r = ui.button(format!("Ignorar todos los *.{ext}"));
                    if close(ui, r) {
                        let u = untracked.clone();
                        defer.push(Box::new(move |a| a.ignore_files(u, true)));
                    }
                }
            }
        }
        ListKind::Staged => {
            let r = ui.button("Quitar del área de preparación");
            if close(ui, r) {
                let s = sel.clone();
                defer.push(Box::new(move |a| a.unstage_files(s)));
            }
        }
        ListKind::Conflicted => {
            let r = ui.button("Marcar como resuelto");
            if close(ui, r) {
                let s = sel.clone();
                defer.push(Box::new(move |a| a.mark_resolved(s)));
            }
            if let Some(f) = &one {
                for (label, ours) in [("Resolver usando la mía (ours)", true), ("Resolver usando la suya (theirs)", false)] {
                    let r = ui.button(label);
                    if close(ui, r) {
                        let f = f.clone();
                        defer.push(Box::new(move |a| a.resolve_conflict(f, ours)));
                    }
                }
                let r = ui.button("Abrir en la herramienta de fusión");
                if close(ui, r) {
                    let f = f.clone();
                    defer.push(Box::new(move |a| a.open_mergetool(f)));
                }
            }
        }
    }
    ui.separator();
    if let Some(f) = &one {
        if !f.is_untracked() {
            let r = ui.button("Historial del archivo");
            if close(ui, r) {
                let p = f.path.clone();
                defer.push(Box::new(move |a| a.open_file_history(&p)));
            }
            let r = ui.button("Blame");
            if close(ui, r) && f.status != "D" {
                let p = f.path.clone();
                defer.push(Box::new(move |a| a.open_blame(&p, None)));
            }
        }
        if f.status != "D" {
            let r = ui.button("Abrir archivo");
            if close(ui, r) {
                let _ = open::that(t.path.join(&f.path));
            }
            let r = ui.button("Mostrar en la carpeta");
            if close(ui, r) {
                let p = t.path.join(&f.path);
                let _ = open::that(p.parent().unwrap_or(&t.path));
            }
        }
        super::history::custom_actions_menu(ui, ActionScope::File, None, Some(f.path.clone()), defer);
    }
    let r = ui.button(if one.is_some() { "Copiar ruta" } else { "Copiar rutas" });
    if close(ui, r) {
        ui.ctx().copy_text(sel.iter().map(|f| f.path.clone()).collect::<Vec<_>>().join("\n"));
    }
}

fn commit_box(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let c = theme::p();
    let (can, why) = app.can_commit();
    let t = &mut app.tabs[idx];
    let Some(s) = t.status.clone() else { return };
    let merging = matches!(s.state, Some(RepoState::Merge | RepoState::CherryPick | RepoState::Revert));
    let first_len = t.message.lines().next().map_or(0, |l| l.chars().count());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Mensaje del commit").strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if first_len > 0 {
                ui.label(RichText::new(format!("{first_len}/72")).size(11.5).color(if first_len > 72 { c.yellow } else { c.muted }));
            }
            if let Some(tpl) = t.template.clone() {
                if ui.small_button("Plantilla").on_hover_text("Insertar la plantilla de commit (commit.template)").clicked() && t.message.trim().is_empty() {
                    t.message = tpl;
                }
            }
        });
    });
    let r = ui.add(egui::TextEdit::multiline(&mut t.message).hint_text("Resumen en la primera línea…\n\nDescripción opcional").desired_rows(4).desired_width(f32::INFINITY));
    if r.has_focus() && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Enter)) {
        defer.push(Box::new(|a| a.do_commit()));
    }
    ui.horizontal(|ui| {
        let mut amend = t.amend;
        if ui.checkbox(&mut amend, "Modificar último").on_hover_text("git commit --amend").changed() {
            defer.push(Box::new(|a| a.toggle_amend()));
        }
        ui.checkbox(&mut t.push_after, "Push al confirmar");
        ui.checkbox(&mut t.sign, "Firmar").on_hover_text("Firmar el commit con tu clave GPG/SSH (-S)");
    });
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let label = if t.amend {
            "Modificar último commit".to_string()
        } else if !s.staged.is_empty() {
            format!("Confirmar {}", widgets::plural(s.staged.len(), "archivo", "archivos"))
        } else if merging {
            "Confirmar fusión".into()
        } else {
            "Confirmar".into()
        };
        let r = widgets::primary_button(ui, &label, can);
        let r = if why.is_empty() { r } else { r.on_disabled_hover_text(why) };
        if r.clicked() {
            defer.push(Box::new(|a| a.do_commit()));
        }
        ui.label(widgets::small("Ctrl+Enter"));
    });
}

fn diff_area(ui: &mut egui::Ui, app: &mut App, idx: usize, defer: &mut Defer) {
    let busy = app.busy.is_some();
    let t = &mut app.tabs[idx];
    if t.change_count() == 0 {
        widgets::placeholder(ui, Icon::Check, "Todo limpio", "No hay cambios pendientes en el directorio de trabajo.");
        return;
    }
    let Some(m) = &mut t.changes_diff else {
        widgets::placeholder(ui, Icon::File, "Selecciona un archivo para ver sus cambios", "");
        return;
    };
    let f = m.file.clone();
    let mode = m.mode;
    let root = t.path.clone();
    super::diff::header(ui, &f.path, f.orig.as_deref(), |ui| {
        match mode {
            DiffMode::Unstaged | DiffMode::Untracked => {
                if widgets::primary_button(ui, "Preparar archivo", !busy).clicked() {
                    let ff = f.clone();
                    defer.push(Box::new(move |a| a.stage_files(vec![ff])));
                }
                let label = if f.is_untracked() { "Eliminar" } else { "Descartar archivo" };
                if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                    let ff = f.clone();
                    defer.push(Box::new(move |a| a.discard_files(vec![ff])));
                }
            }
            DiffMode::Staged => {
                if ui.add_enabled(!busy, egui::Button::new("Quitar archivo")).clicked() {
                    let ff = f.clone();
                    defer.push(Box::new(move |a| a.unstage_files(vec![ff])));
                }
            }
            DiffMode::Conflict => {
                if widgets::primary_button(ui, "Marcar resuelto", !busy).clicked() {
                    let ff = f.clone();
                    defer.push(Box::new(move |a| a.mark_resolved(vec![ff])));
                }
                if ui.add_enabled(!busy, egui::Button::new("Herramienta de fusión")).clicked() {
                    let ff = f.clone();
                    defer.push(Box::new(move |a| a.open_mergetool(ff)));
                }
                if ui.add_enabled(!busy, egui::Button::new("Usar la suya")).on_hover_text("Quedarse con la versión entrante (theirs)").clicked() {
                    let ff = f.clone();
                    defer.push(Box::new(move |a| a.resolve_conflict(ff, false)));
                }
                if ui.add_enabled(!busy, egui::Button::new("Usar la mía")).on_hover_text("Quedarse con la versión de la rama actual (ours)").clicked() {
                    let ff = f.clone();
                    defer.push(Box::new(move |a| a.resolve_conflict(ff, true)));
                }
            }
            _ => {}
        }
        if !f.is_untracked() && mode != DiffMode::Conflict && icons::button(ui, Icon::External, "Abrir en la herramienta de diff externa").clicked() {
            let ff = f.clone();
            defer.push(Box::new(move |a| a.open_difftool(&ff, mode == DiffMode::Staged, None)));
        }
        if f.status != "D" {
            if icons::button(ui, Icon::Folder, "Mostrar en la carpeta").clicked() {
                let p = root.join(&f.path);
                let _ = open::that(p.parent().unwrap_or(&root));
            }
            if icons::button(ui, Icon::File, "Abrir archivo").clicked() {
                let _ = open::that(root.join(&f.path));
            }
        }
    });
    let acts = super::diff::body(ui, m, &mut app.store.settings);
    super::diff::apply_actions(acts, defer);
}
