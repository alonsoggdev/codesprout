//! Colors (same palette as the Electron version), fonts and egui style.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};

static DARK: AtomicBool = AtomicBool::new(true);

pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub panel: Color32,
    pub panel2: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub sel: Color32,
    pub green: Color32,
    pub green_soft: Color32,
    pub red: Color32,
    pub red_soft: Color32,
    pub yellow: Color32,
    pub yellow_soft: Color32,
    pub purple: Color32,
    pub add_bg: Color32,
    pub add_hl: Color32,
    pub del_bg: Color32,
    pub del_hl: Color32,
    pub line_sel: Color32,
    pub merge: Color32,
    pub lanes: [Color32; 10],
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    // premultiplied
    Color32::from_rgba_premultiplied(
        ((r as u16 * a as u16) / 255) as u8,
        ((g as u16 * a as u16) / 255) as u8,
        ((b as u16 * a as u16) / 255) as u8,
        a,
    )
}

pub const DARK_PALETTE: Palette = Palette {
    bg: rgb(0x14, 0x16, 0x1b),
    surface: rgb(0x19, 0x1c, 0x22),
    panel: rgb(0x1e, 0x21, 0x28),
    panel2: rgb(0x23, 0x27, 0x2f),
    hover: rgb(0x26, 0x2a, 0x33),
    active: rgb(0x2c, 0x32, 0x40),
    border: rgb(0x2a, 0x2e, 0x37),
    border_strong: rgb(0x36, 0x3b, 0x46),
    text: rgb(0xe4, 0xe7, 0xee),
    text2: rgb(0xb3, 0xba, 0xc7),
    muted: rgb(0x7e, 0x86, 0x96),
    accent: rgb(0x5b, 0x93, 0xff),
    accent_soft: rgba(0x5b, 0x93, 0xff, 41),
    sel: rgba(0x5b, 0x93, 0xff, 56),
    green: rgb(0x3c, 0xcf, 0x7e),
    green_soft: rgba(0x3c, 0xcf, 0x7e, 33),
    red: rgb(0xf2, 0x64, 0x64),
    red_soft: rgba(0xf2, 0x64, 0x64, 33),
    yellow: rgb(0xf2, 0xb3, 0x3d),
    yellow_soft: rgba(0xf2, 0xb3, 0x3d, 36),
    purple: rgb(0xa9, 0x84, 0xff),
    add_bg: rgba(46, 160, 67, 38),
    add_hl: rgba(46, 160, 67, 90),
    del_bg: rgba(248, 81, 73, 36),
    del_hl: rgba(248, 81, 73, 90),
    line_sel: rgba(0x5b, 0x93, 0xff, 89),
    merge: rgb(0x3c, 0xcf, 0x7e),
    lanes: [
        rgb(0x5b, 0x93, 0xff),
        rgb(0xf2, 0xb3, 0x3d),
        rgb(0xa9, 0x84, 0xff),
        rgb(0x4f, 0xd1, 0xc5),
        rgb(0xf2, 0x7a, 0x9e),
        rgb(0x9b, 0xd1, 0x5a),
        rgb(0xff, 0x9a, 0x5c),
        rgb(0x6c, 0xc4, 0xff),
        rgb(0xd9, 0x8c, 0xf2),
        rgb(0xe8, 0xd4, 0x4d),
    ],
};

pub const LIGHT_PALETTE: Palette = Palette {
    bg: rgb(0xee, 0xf0, 0xf4),
    surface: rgb(0xf7, 0xf8, 0xfa),
    panel: rgb(0xff, 0xff, 0xff),
    panel2: rgb(0xf3, 0xf4, 0xf7),
    hover: rgb(0xec, 0xee, 0xf3),
    active: rgb(0xe2, 0xe7, 0xf2),
    border: rgb(0xe1, 0xe4, 0xea),
    border_strong: rgb(0xcf, 0xd4, 0xdd),
    text: rgb(0x1d, 0x21, 0x29),
    text2: rgb(0x3f, 0x46, 0x54),
    muted: rgb(0x6f, 0x77, 0x87),
    accent: rgb(0x2f, 0x6f, 0xed),
    accent_soft: rgba(0x2f, 0x6f, 0xed, 26),
    sel: rgba(0x2f, 0x6f, 0xed, 36),
    green: rgb(0x1a, 0x9a, 0x55),
    green_soft: rgba(0x1a, 0x9a, 0x55, 26),
    red: rgb(0xd9, 0x3c, 0x3c),
    red_soft: rgba(0xd9, 0x3c, 0x3c, 23),
    yellow: rgb(0xb7, 0x79, 0x0f),
    yellow_soft: rgba(0xb7, 0x79, 0x0f, 28),
    purple: rgb(0x7c, 0x4d, 0xdb),
    add_bg: rgba(26, 160, 70, 26),
    add_hl: rgba(26, 160, 70, 70),
    del_bg: rgba(220, 50, 50, 20),
    del_hl: rgba(220, 50, 50, 64),
    line_sel: rgba(0x2f, 0x6f, 0xed, 64),
    merge: rgb(0x1a, 0x9a, 0x55),
    lanes: [
        rgb(0x2f, 0x6f, 0xed),
        rgb(0xc9, 0x84, 0x0c),
        rgb(0x7c, 0x4d, 0xdb),
        rgb(0x0f, 0x9b, 0x8e),
        rgb(0xd0, 0x3f, 0x74),
        rgb(0x4f, 0x9a, 0x1e),
        rgb(0xd9, 0x63, 0x1f),
        rgb(0x13, 0x8b, 0xd4),
        rgb(0xa8, 0x3f, 0xc9),
        rgb(0x9a, 0x86, 0x00),
    ],
};

pub fn p() -> &'static Palette {
    if DARK.load(Ordering::Relaxed) {
        &DARK_PALETTE
    } else {
        &LIGHT_PALETTE
    }
}

pub fn lane_color(idx: usize) -> Color32 {
    let l = &p().lanes;
    l[idx % l.len()]
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Loads Segoe UI / Cascadia from the system when available (native look on Windows),
/// keeping egui's bundled fonts as fallback for symbols.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let candidates: &[(&str, &[&str])] = &[
        ("ui", &["C:\\Windows\\Fonts\\segoeui.ttf", "/System/Library/Fonts/SFNS.ttf", "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf", "/usr/share/fonts/noto/NotoSans-Regular.ttf"]),
        ("mono", &["C:\\Windows\\Fonts\\CascadiaMono.ttf", "C:\\Windows\\Fonts\\consola.ttf", "/System/Library/Fonts/SFNSMono.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"]),
        ("symbols", &["C:\\Windows\\Fonts\\seguisym.ttf"]),
    ];
    for (name, paths) in candidates {
        if let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) {
            fonts.font_data.insert((*name).to_string(), std::sync::Arc::new(FontData::from_owned(bytes)));
            match *name {
                "ui" => fonts.families.entry(FontFamily::Proportional).or_default().insert(0, name.to_string()),
                "mono" => fonts.families.entry(FontFamily::Monospace).or_default().insert(0, name.to_string()),
                _ => {
                    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
                        fonts.families.entry(fam).or_default().push(name.to_string());
                    }
                }
            }
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
    let c = p();
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = c.surface;
    v.window_fill = c.panel;
    v.extreme_bg_color = c.bg;
    v.faint_bg_color = c.panel2;
    v.code_bg_color = c.panel2;
    v.window_stroke = Stroke::new(1.0_f32, c.border_strong);
    v.selection.bg_fill = c.sel;
    v.selection.stroke = Stroke::new(1.0_f32, c.accent);
    v.hyperlink_color = c.accent;
    v.warn_fg_color = c.yellow;
    v.error_fg_color = c.red;
    v.window_corner_radius = 10.into();
    v.menu_corner_radius = 8.into();
    v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 110 } else { 40 }) };
    v.window_shadow = egui::Shadow { offset: [0, 10], blur: 40, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 45 }) };
    v.override_text_color = Some(c.text);
    for (w, fill, stroke) in [
        (&mut v.widgets.noninteractive, c.panel, c.border),
        (&mut v.widgets.inactive, c.panel2, c.border_strong),
        (&mut v.widgets.hovered, c.hover, c.border_strong),
        (&mut v.widgets.active, c.active, c.accent),
        (&mut v.widgets.open, c.active, c.border_strong),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(1.0_f32, stroke);
        w.corner_radius = 6.into();
        w.fg_stroke.color = c.text;
    }
    v.widgets.noninteractive.fg_stroke.color = c.text2;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, c.border);
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Body, FontId::proportional(13.5)),
        (TextStyle::Button, FontId::proportional(13.5)),
        (TextStyle::Heading, FontId::proportional(19.0)),
        (TextStyle::Monospace, mono(12.5)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.interact_size.y = 26.0;
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;
    style.interaction.tooltip_delay = 0.35;
    ctx.set_style(style);
}
