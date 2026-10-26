//! Diff viewer: virtualized rows, intra-line highlights, hunk and line staging, image and
//! conflict views.

use std::collections::HashSet;

use ds_core::diff::{build_patch, ConflictPart, LineKind};
use eframe::egui::{self, pos2, text::LayoutJob, vec2, Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, TextFormat};

use super::Defer;
use crate::icons::{self, Icon};
use crate::repo_state::{DiffMode, DiffModel, Row};
use crate::store::Settings;
use crate::theme;
use crate::widgets;

pub enum DiffAction {
    /// patch, cached, reverse
    Apply(String, bool, bool),
    /// Confirm before discarding working-tree lines.
    Discard(String, usize),
    Reload,
}

pub fn apply_actions(acts: Vec<DiffAction>, defer: &mut Defer) {
    for a in acts {
        match a {
            DiffAction::Apply(p, cached, reverse) => defer.push(Box::new(move |app| app.apply_partial(p, cached, reverse))),
            DiffAction::Discard(p, n) => defer.push(Box::new(move |app| {
                let msg = format!("Se descartarán {} del directorio de trabajo. No se puede deshacer.", widgets::plural(n, "línea cambiada", "líneas cambiadas"));
                app.confirm("Descartar cambios", &msg, None, "Descartar", true, move |app| app.apply_partial(p, false, true));
            })),
            DiffAction::Reload => defer.push(Box::new(|app| {
                app.persist();
                if let Some(k) = app.active_key() {
                    app.load_changes_diff(&k);
                    app.load_detail_diff(&k);
                    app.load_compare_diff(&k);
                    app.load_file_history_diff(&k);
                }
            })),
        }
    }
}

/// File title bar with buttons on the right.
pub fn header(ui: &mut egui::Ui, path: &str, orig: Option<&str>, right: impl FnOnce(&mut egui::Ui)) {
    let c = theme::p();
    egui::Frame::new().fill(c.panel).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            icons::show(ui, Icon::File, 14.0, c.muted);
            let (dir, base) = widgets::split_path(path);
            ui.label(RichText::new(base).strong());
            if !dir.is_empty() {
                ui.label(widgets::muted(dir));
            }
            if let Some(o) = orig {
                ui.label(RichText::new(format!("← {o}")).color(c.purple));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), right);
        });
    });
}

fn options_bar(ui: &mut egui::Ui, m: &DiffModel, s: &mut Settings) -> bool {
    let c = theme::p();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if !matches!(m.mode, DiffMode::Untracked | DiffMode::Conflict) {
            changed |= ui.checkbox(&mut s.ignore_whitespace, "Ignorar espacios").on_hover_text("Con esta opción no se puede preparar por líneas").changed();
            changed |= ui.checkbox(&mut s.word_diff, "Palabras").on_hover_text("Resaltar las palabras cambiadas dentro de cada línea").changed();
            ui.label(widgets::small("Contexto"));
            let mut ctx = s.context_lines;
            if ui.add(egui::DragValue::new(&mut ctx).range(0..=50).speed(0.2)).changed() {
                s.context_lines = ctx;
                changed = true;
            }
        }
        if m.truncated {
            ui.label(RichText::new("Diff truncado (archivo muy grande)").color(c.yellow).size(12.0));
        }
        if m.lossy {
            ui.label(RichText::new("Contiene caracteres que no son UTF-8").color(c.yellow).size(12.0));
        }
        if m.partial && m.mode != DiffMode::Commit {
            ui.label(widgets::small("Clic en líneas para seleccionarlas · Mayús+clic: rango"));
        }
    });
    changed
}

pub fn body(ui: &mut egui::Ui, m: &mut DiffModel, s: &mut Settings) -> Vec<DiffAction> {
    let c = theme::p();
    let mut acts = vec![];
    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
        if options_bar(ui, m, s) {
            acts.push(DiffAction::Reload);
        }
        if let Some(n) = &m.note {
            ui.label(RichText::new(n).color(c.text2).size(12.0));
        }
    });
    if let Some(img) = &mut m.image {
        image_view(ui, img);
    }
    let font = theme::mono(12.5);
    let row_h = 19.0;
    let char_w = ui.fonts(|f| f.glyph_width(&font, 'm'));
    let gutter = if m.rows.iter().any(|r| matches!(r, Row::Line { .. })) { 92.0 } else { 52.0 };
    let content_w = (gutter + 16.0 + m.max_chars as f32 * char_w).max(ui.available_width());
    let partial = m.partial && matches!(m.mode, DiffMode::Staged | DiffMode::Unstaged);
    let mut click: Option<(usize, bool)> = None;
    let mut hunk_action: Option<(usize, usize, u8)> = None; // file, hunk, 0 stage/unstage, 1 discard
    egui::ScrollArea::both().auto_shrink([false, false]).id_salt(&m.key).show_rows(ui, row_h, m.rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let visible_right = ui.clip_rect().max.x;
        for ri in range {
            // only diff lines take clicks, so the hunk buttons get theirs
            let sense = if matches!(m.rows[ri], Row::Line { .. }) { Sense::click() } else { Sense::hover() };
            let (rect, resp) = ui.allocate_exact_size(vec2(content_w, row_h), sense);
            let p = ui.painter();
            match &m.rows[ri] {
                Row::Info(text) => {
                    p.text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, text, FontId::proportional(13.0), c.muted);
                }
                Row::Hunk { file, hunk } => {
                    p.rect_filled(rect, 0.0, c.panel2);
                    let h = &m.files[*file].hunks[*hunk];
                    p.text(rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, format!("@@ -{},{} +{},{} @@ {}", h.old_start, h.old_count, h.new_start, h.new_count, h.context), font.clone(), c.muted);
                    if partial {
                        let sel_here = m.selected.iter().filter(|(f, hh, _)| f == file && hh == hunk).count();
                        let labels: Vec<(String, u8)> = match m.mode {
                            DiffMode::Unstaged => vec![
                                (if sel_here > 0 { format!("Descartar líneas ({sel_here})") } else { "Descartar bloque".into() }, 1),
                                (if sel_here > 0 { format!("Preparar líneas ({sel_here})") } else { "Preparar bloque".into() }, 0),
                            ],
                            _ => vec![(if sel_here > 0 { format!("Quitar líneas ({sel_here})") } else { "Quitar bloque".into() }, 0)],
                        };
                        let mut x = visible_right - 8.0;
                        for (label, kind) in labels.iter().rev() {
                            let g = p.layout_no_wrap(label.clone(), FontId::proportional(11.5), if *kind == 1 { c.red } else { c.accent });
                            let br = Rect::from_min_max(pos2(x - g.size().x - 14.0, rect.min.y + 1.5), pos2(x, rect.max.y - 1.5));
                            let r = ui.interact(br, ui.id().with(("hunkbtn", ri, *kind)), Sense::click());
                            let p = ui.painter();
                            p.rect(br, 4.0, if r.hovered() { c.hover } else { c.panel }, Stroke::new(1.0_f32, c.border_strong), egui::StrokeKind::Inside);
                            p.galley(pos2(br.min.x + 7.0, br.center().y - g.size().y / 2.0), g, c.text);
                            if r.clicked() {
                                hunk_action = Some((*file, *hunk, *kind));
                            }
                            x = br.min.x - 6.0;
                        }
                    }
                }
                Row::Line { file, hunk, line, kind, old, new, hl } => {
                    let text = &m.files[*file].hunks[*hunk].lines[*line];
                    let (bg, hl_bg, sign_col) = match kind {
                        LineKind::Add => (c.add_bg, c.add_hl, c.green),
                        LineKind::Del => (c.del_bg, c.del_hl, c.red),
                        _ => (Color32::TRANSPARENT, Color32::TRANSPARENT, c.muted),
                    };
                    let selected = m.selected.contains(&(*file, *hunk, *line));
                    p.rect_filled(rect, 0.0, if selected { c.line_sel } else { bg });
                    if partial && matches!(kind, LineKind::Add | LineKind::Del) && resp.hovered() {
                        p.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, theme::with_alpha(c.accent, 120)), egui::StrokeKind::Inside);
                    }
                    let num = |n: &Option<u32>| n.map(|v| v.to_string()).unwrap_or_default();
                    p.text(pos2(rect.min.x + 38.0, rect.center().y), Align2::RIGHT_CENTER, num(old), FontId::monospace(11.0), c.muted);
                    p.text(pos2(rect.min.x + 80.0, rect.center().y), Align2::RIGHT_CENTER, num(new), FontId::monospace(11.0), c.muted);
                    let body = text.get(1..).unwrap_or("").trim_end_matches('\r');
                    p.text(pos2(rect.min.x + gutter - 2.0, rect.center().y), Align2::RIGHT_CENTER, text.get(..1).unwrap_or(""), font.clone(), sign_col);
                    let mut job = LayoutJob::default();
                    let normal = TextFormat { font_id: font.clone(), color: c.text, ..Default::default() };
                    let strong = TextFormat { font_id: font.clone(), color: c.text, background: hl_bg, ..Default::default() };
                    let mut pos = 0;
                    for &(a, b) in hl {
                        let (a, b) = (a.min(body.len()), b.min(body.len()));
                        if a > pos && body.is_char_boundary(pos) && body.is_char_boundary(a) {
                            job.append(&body[pos..a], 0.0, normal.clone());
                        }
                        if b > a && body.is_char_boundary(a) && body.is_char_boundary(b) {
                            job.append(&body[a..b], 0.0, strong.clone());
                        }
                        pos = b;
                    }
                    if pos < body.len() && body.is_char_boundary(pos) {
                        job.append(&body[pos..], 0.0, normal.clone());
                    }
                    let g = ui.fonts(|f| f.layout_job(job));
                    ui.painter().galley(pos2(rect.min.x + gutter + 8.0, rect.center().y - g.size().y / 2.0), g, c.text);
                    if partial && matches!(kind, LineKind::Add | LineKind::Del) && resp.clicked() {
                        click = Some((ri, ui.input(|i| i.modifiers.shift)));
                    }
                }
                Row::Text { no, text, part, added } => {
                    let bg = match part {
                        ConflictPart::Marker => theme::with_alpha(c.purple, 50),
                        ConflictPart::Ours => c.green_soft,
                        ConflictPart::Theirs => c.accent_soft,
                        ConflictPart::Base => c.yellow_soft,
                        ConflictPart::Normal if *added => c.add_bg,
                        ConflictPart::Normal => Color32::TRANSPARENT,
                    };
                    p.rect_filled(rect, 0.0, bg);
                    p.text(pos2(rect.min.x + 40.0, rect.center().y), Align2::RIGHT_CENTER, no.to_string(), FontId::monospace(11.0), c.muted);
                    let col = if *part == ConflictPart::Marker { c.purple } else { c.text };
                    p.text(pos2(rect.min.x + gutter, rect.center().y), Align2::LEFT_CENTER, text.trim_end_matches('\r'), font.clone(), col);
                }
            }
        }
    });

    if let Some((ri, shift)) = click {
        if let Row::Line { file, hunk, .. } = m.rows[ri] {
            let anchor = m.anchor.filter(|&a| shift && matches!(m.rows.get(a), Some(Row::Line { file: f, hunk: h, .. }) if *f == file && *h == hunk));
            match anchor {
                Some(a) => {
                    let (lo, hi) = (a.min(ri), a.max(ri));
                    for r in &m.rows[lo..=hi] {
                        if let Row::Line { file, hunk, line, kind: LineKind::Add | LineKind::Del, .. } = r {
                            m.selected.insert((*file, *hunk, *line));
                        }
                    }
                }
                None => {
                    // a new selection in another hunk replaces the old one
                    m.selected.retain(|(f, h, _)| *f == file && *h == hunk);
                    if let Row::Line { line, .. } = m.rows[ri] {
                        if !m.selected.remove(&(file, hunk, line)) {
                            m.selected.insert((file, hunk, line));
                        }
                    }
                    m.anchor = Some(ri);
                }
            }
        }
    }
    if let Some((fi, hi, kind)) = hunk_action {
        let lines: HashSet<usize> = m.selected.iter().filter(|(f, h, _)| *f == fi && *h == hi).map(|x| x.2).collect();
        let sel = if lines.is_empty() { None } else { Some(&lines) };
        let file = &m.files[fi];
        let hunk = &file.hunks[hi];
        let changed = sel.map_or_else(|| hunk.lines.iter().filter(|l| l.starts_with('+') || l.starts_with('-')).count(), |s| s.len());
        match (m.mode, kind) {
            (DiffMode::Unstaged, 0) => {
                if let Some(p) = build_patch(file, hunk, sel, false) {
                    acts.push(DiffAction::Apply(p, true, false));
                }
            }
            (DiffMode::Unstaged, _) => {
                if let Some(p) = build_patch(file, hunk, sel, true) {
                    acts.push(DiffAction::Discard(p, changed));
                }
            }
            (DiffMode::Staged, _) => {
                if let Some(p) = build_patch(file, hunk, sel, true) {
                    acts.push(DiffAction::Apply(p, true, true));
                }
            }
            _ => {}
        }
        m.selected.clear();
    }
    acts
}

fn image_view(ui: &mut egui::Ui, img: &mut crate::repo_state::ImagePair) {
    let c = theme::p();
    if img.textures.is_none() {
        let ctx = ui.ctx().clone();
        let load = |name: &str, i: &Option<egui::ColorImage>| i.clone().map(|im| ctx.load_texture(name, im, egui::TextureOptions::LINEAR));
        img.textures = Some((load("diff-old", &img.old), load("diff-new", &img.new)));
    }
    let Some((old, new)) = &img.textures else { return };
    let avail = ui.available_width() - 24.0;
    let max_h = 260.0;
    egui::Frame::new().inner_margin(10).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for (label, tex, color) in [("Antes", old, c.red), ("Después", new, c.green)] {
                ui.vertical(|ui| {
                    ui.label(RichText::new(label).color(color).size(12.0));
                    match tex {
                        Some(t) => {
                            let sz = t.size_vec2();
                            let scale = ((avail / 2.0 - 10.0) / sz.x).min(max_h / sz.y).min(1.0);
                            let (r, _) = ui.allocate_exact_size(sz * scale, Sense::hover());
                            checker(ui.painter(), r);
                            ui.painter().image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.5_f32, color), egui::StrokeKind::Outside);
                            ui.label(widgets::small(format!("{} × {} px", sz.x as u32, sz.y as u32)));
                        }
                        None => {
                            ui.label(widgets::muted("(no existe)"));
                        }
                    }
                });
                ui.add_space(10.0);
            }
        });
    });
}

fn checker(p: &egui::Painter, r: Rect) {
    let s = 8.0;
    let a = Color32::from_gray(200);
    let b = Color32::from_gray(160);
    p.rect_filled(r, 0.0, a);
    let mut y = r.min.y;
    let mut row = 0;
    while y < r.max.y {
        let mut x = r.min.x + if row % 2 == 0 { 0.0 } else { s };
        while x < r.max.x {
            p.rect_filled(Rect::from_min_max(pos2(x, y), pos2((x + s).min(r.max.x), (y + s).min(r.max.y))), 0.0, b);
            x += s * 2.0;
        }
        y += s;
        row += 1;
    }
}
