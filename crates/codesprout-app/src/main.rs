#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! CodeSprout: desktop Git client.

mod actions;
mod app;
mod askpass;
mod bus;
mod dialogs;
mod icons;
mod repo_state;
mod store;
mod theme;
mod updater;
mod views;
mod widgets;

use eframe::egui;

fn icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../../../build/icon.png");
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // GIT_ASKPASS / SSH_ASKPASS: git runs us with the prompt as the only argument
    if std::env::var_os("CODESPROUT_ASKPASS").is_some_and(|v| !v.is_empty()) && args.len() == 2 {
        std::process::exit(askpass::run(args[1].clone()));
    }
    // used by the Linux installer
    if args.get(1).is_some_and(|a| a == "--version") {
        println!("{}", codesprout_core::update::CURRENT);
        return;
    }
    codesprout_core::update::cleanup();
    if let Ok(exe) = std::env::current_exe() {
        codesprout_core::runner::set_helper_exe(exe);
    }

    let store = store::Store::load();
    let mut viewport = egui::ViewportBuilder::default().with_title("CodeSprout").with_min_inner_size([960.0, 600.0]).with_drag_and_drop(true);
    viewport = match store.settings.window {
        Some([x, y, w, h]) if w >= 600.0 && h >= 400.0 => viewport.with_inner_size([w, h]).with_position([x, y]),
        _ => viewport.with_inner_size([1440.0, 900.0]),
    };
    if store.settings.maximized {
        viewport = viewport.with_maximized(true);
    }
    if let Some(i) = icon() {
        viewport = viewport.with_icon(i);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    if let Err(e) = eframe::run_native("CodeSprout", options, Box::new(|cc| Ok(Box::new(app::App::new(cc))))) {
        eprintln!("No se pudo iniciar CodeSprout: {e}");
    }
}
