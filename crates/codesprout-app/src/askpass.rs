//! Credential prompt mode. Git and SSH run this same executable as GIT_ASKPASS / SSH_ASKPASS
//! with the prompt as the only argument; we show a small window and print the answer.

use eframe::egui;

use crate::theme;

struct Prompt {
    prompt: String,
    secret: bool,
    value: String,
    done: bool,
    answer: Option<String>,
}

impl eframe::App for Prompt {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::p().panel).inner_margin(18)).show(ctx, |ui| {
            ui.label(egui::RichText::new("CodeSprout · Git necesita una credencial").strong());
            ui.add_space(4.0);
            ui.label(&self.prompt);
            ui.add_space(6.0);
            let r = ui.add(egui::TextEdit::singleline(&mut self.value).password(self.secret).desired_width(f32::INFINITY));
            r.request_focus();
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if crate::widgets::primary_button(ui, "Aceptar", true).clicked() || enter {
                    self.answer = Some(self.value.clone());
                    self.done = true;
                }
                if ui.button("Cancelar").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.done = true;
                }
            });
        });
        if self.done {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Returns the process exit code.
pub fn run(prompt: String) -> i32 {
    let lower = prompt.to_lowercase();
    // SSH asks to confirm unknown host keys with a yes/no question
    let secret = !(lower.contains("username") || lower.contains("usuario") || lower.contains("(yes/no"));
    let answer = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let out = answer.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([440.0, 190.0]).with_resizable(false).with_always_on_top().with_title("CodeSprout"),
        centered: true,
        ..Default::default()
    };
    let _ = eframe::run_native(
        "CodeSprout · credencial",
        options,
        Box::new(move |cc| {
            theme::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx, cc.egui_ctx.system_theme().map_or(true, |t| t == egui::Theme::Dark));
            Ok(Box::new(Guard { inner: Prompt { prompt, secret, value: String::new(), done: false, answer: None }, out }))
        }),
    );
    let a = answer.lock().unwrap().clone();
    match a {
        Some(v) => {
            println!("{v}");
            0
        }
        None => 1,
    }
}

struct Guard {
    inner: Prompt,
    out: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl eframe::App for Guard {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.inner.update(ctx, frame);
        if self.inner.done {
            *self.out.lock().unwrap() = self.inner.answer.clone();
        }
    }
}
