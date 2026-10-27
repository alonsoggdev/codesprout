//! Small UI helpers shared by the views.

use eframe::egui::{
    self, pos2, vec2, Align, Color32, CursorIcon, FontId, Layout, Rect, Response, RichText, Sense, Stroke, Ui,
};

use crate::icons::{self, Icon};
use crate::theme;

pub fn rel_time(ts: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let d = (now - ts).max(0);
    let (n, unit_s, unit_p) = match d {
        0..=59 => return "ahora mismo".into(),
        60..=3599 => (d / 60, "minuto", "minutos"),
        3600..=86_399 => (d / 3600, "hora", "horas"),
        86_400..=2_591_999 => (d / 86_400, "día", "días"),
        2_592_000..=31_535_999 => (d / 2_592_000, "mes", "meses"),
        _ => (d / 31_536_000, "año", "años"),
    };
    format!("hace {n} {}", if n == 1 { unit_s } else { unit_p })
}

pub fn full_time(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(ts, 0) {
        chrono::LocalResult::Single(t) => t.format("%d/%m/%Y %H:%M").to_string(),
        _ => String::new(),
    }
}

pub fn short_date(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(ts, 0) {
        chrono::LocalResult::Single(t) => t.format("%d/%m/%y").to_string(),
        _ => String::new(),
    }
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn hash_color(s: &str) -> Color32 {
    let mut h: u32 = 0;
    for b in s.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    theme::lane_color((h % 10) as usize)
}

pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name.split(|c: char| c.is_whitespace() || c == '-' || c == '_' || c == '.').filter(|s| !s.is_empty()).collect();
    let mut s: String = parts.iter().take(2).filter_map(|p| p.chars().next()).collect();
    if s.is_empty() {
        s = "?".into();
    }
    s.to_uppercase()
}

pub fn split_path(p: &str) -> (&str, &str) {
    match p.rfind('/') {
        Some(i) => (&p[..i], &p[i + 1..]),
        None => ("", p),
    }
}

pub fn status_name(status: &str) -> &'static str {
    match status.chars().next().unwrap_or(' ') {
        'M' => "Modificado",
        'A' => "Añadido",
        'D' => "Eliminado",
        'R' => "Renombrado",
        'C' => "Copiado",
        'T' => "Cambio de tipo",
        '?' => "Sin seguimiento",
        _ if status.len() > 1 => "Conflicto",
        _ => "Cambio",
    }
}

/// Colored square with the status letter.
pub fn status_badge(ui: &mut Ui, status: &str) -> Response {
    let c = theme::p();
    let letter = if status.len() > 1 { 'U' } else { status.chars().next().unwrap_or(' ') };
    let (color, shown) = match letter {
        'M' => (c.yellow, "M"),
        'A' => (c.green, "A"),
        '?' => (c.green, "+"),
        'D' => (c.red, "D"),
        'R' | 'C' => (c.purple, if letter == 'R' { "R" } else { "C" }),
        'U' => (c.red, "!"),
        _ => (c.muted, "T"),
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, theme::with_alpha(color, 40));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, shown, FontId::proportional(10.5), color);
    resp.on_hover_text(status_name(status))
}

/// Full-width clickable row with hover / selection background. The closure draws the content.
pub fn row(ui: &mut Ui, selected: bool, height: f32, add: impl FnOnce(&mut Ui)) -> Response {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let c = theme::p();
    if selected {
        ui.painter().rect_filled(rect, 5.0, c.sel);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, c.hover);
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child);
    resp
}

pub fn pill(ui: &mut Ui, text: impl Into<String>, color: Color32) -> Response {
    let text = text.into();
    let font = FontId::proportional(11.0);
    let galley = ui.painter().layout_no_wrap(text, font, color);
    let size = vec2(galley.size().x + 12.0, 17.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 8.5, theme::with_alpha(color, 36));
    ui.painter().galley(pos2(rect.min.x + 6.0, rect.center().y - galley.size().y / 2.0), galley, color);
    resp
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(theme::p().muted)
}

pub fn small(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(11.5).color(theme::p().muted)
}

pub fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let c = theme::p();
    let b = egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(if enabled { c.accent } else { c.muted }).stroke(Stroke::NONE);
    ui.add_enabled(enabled, b)
}

pub fn danger_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    let c = theme::p();
    let b = egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(c.red).stroke(Stroke::NONE);
    ui.add_enabled(enabled, b)
}

pub fn icon_text_button(ui: &mut Ui, icon: Icon, text: &str, enabled: bool) -> Response {
    let c = theme::p();
    let font = FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, if enabled { c.text } else { c.muted });
    let size = vec2(galley.size().x + 36.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let fill = if enabled && resp.hovered() { c.hover } else { c.panel2 };
    ui.painter().rect(rect, 6.0, fill, Stroke::new(1.0_f32, c.border_strong), egui::StrokeKind::Inside);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.min.x + 15.0, rect.center().y), vec2(14.0, 14.0)), icon, if enabled { c.text2 } else { c.muted });
    ui.painter().galley(pos2(rect.min.x + 27.0, rect.center().y - galley.size().y / 2.0), galley, c.text);
    if enabled {
        resp.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Collapsible section title used in the sidebar. Returns true when clicked.
pub fn section_title(ui: &mut Ui, title: &str, count: Option<usize>, collapsed: bool) -> (Response, Rect) {
    let c = theme::p();
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(rect.min.x + 8.0, rect.center().y), vec2(11.0, 11.0)),
        if collapsed { Icon::ChevronRight } else { Icon::ChevronDown },
        c.muted,
    );
    let mut text = title.to_uppercase();
    if let Some(n) = count {
        text.push_str(&format!("  {n}"));
    }
    ui.painter().text(
        pos2(rect.min.x + 18.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(11.0),
        if resp.hovered() { c.text2 } else { c.muted },
    );
    (resp, rect)
}

/// Right-aligned row one widget tall, for the buttons at the bottom of modals. Not
/// `with_layout(right_to_left)`: that takes all the remaining height, and since a modal
/// remembers its size, the dialog never shrinks back and can end up taller than the window.
pub fn right_row<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    ui.allocate_ui_with_layout(vec2(ui.available_width(), ui.spacing().interact_size.y), Layout::right_to_left(Align::Center), add)
}

pub fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(theme::p().text2));
}

/// Text edit filling the width.
pub fn text_field(ui: &mut Ui, value: &mut String, hint: &str) -> Response {
    ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(f32::INFINITY))
}

pub fn combo(ui: &mut Ui, id: &str, value: &mut String, options: &[String]) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(value.clone()).width(ui.available_width().min(320.0)).show_ui(ui, |ui| {
        for o in options {
            if ui.selectable_label(value == o, o).clicked() {
                *value = o.clone();
                changed = true;
            }
        }
    });
    changed
}


pub fn error_box(ui: &mut Ui, text: &str) {
    let c = theme::p();
    egui::Frame::new().fill(c.red_soft).corner_radius(6).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(text).color(c.red).font(theme::mono(12.0)));
    });
}

pub fn info_box(ui: &mut Ui, text: &str, color: Color32) {
    egui::Frame::new().fill(theme::with_alpha(color, 30)).corner_radius(6).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(text).color(theme::p().text));
    });
}

pub fn placeholder(ui: &mut Ui, icon: Icon, title: &str, sub: &str) {
    let c = theme::p();
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space((ui.available_height() / 2.0 - 50.0).max(10.0));
        icons::show(ui, icon, 34.0, c.muted);
        ui.add_space(6.0);
        ui.label(RichText::new(title).size(15.0).color(c.text2));
        if !sub.is_empty() {
            ui.label(RichText::new(sub).color(c.muted));
        }
    });
}

/// Draggable splitter between panels; returns the new size.
pub fn splitter_h(ui: &mut Ui, id: &str, rect: Rect, value: f32, min: f32, max: f32, invert: bool) -> f32 {
    let resp = ui.interact(rect, ui.id().with(id), Sense::drag());
    let c = theme::p();
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        ui.painter().rect_filled(rect, 0.0, theme::with_alpha(c.accent, 90));
    }
    let mut v = value;
    if resp.dragged() {
        let d = resp.drag_delta().y;
        v += if invert { -d } else { d };
    }
    v.clamp(min, max.max(min))
}
