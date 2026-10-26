//! Background work: git commands run on worker threads and hand a closure back to the UI
//! thread, which applies the result to the app state on the next frame.

use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui;

use crate::app::App;

pub type Apply = Box<dyn FnOnce(&mut App) + Send>;

pub struct Bus {
    tx: Sender<Apply>,
    rx: Receiver<Apply>,
    ctx: egui::Context,
}

impl Bus {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, ctx }
    }

    pub fn sender(&self) -> BusSender {
        BusSender { tx: self.tx.clone(), ctx: self.ctx.clone() }
    }

    /// Runs `work` on a thread, then `done` with its result on the UI thread.
    pub fn spawn<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut App, T) + Send + 'static,
    ) {
        let s = self.sender();
        std::thread::spawn(move || {
            let r = work();
            s.send(move |app| done(app, r));
        });
    }

    pub fn drain(&self) -> Vec<Apply> {
        self.rx.try_iter().collect()
    }
}

#[derive(Clone)]
pub struct BusSender {
    tx: Sender<Apply>,
    ctx: egui::Context,
}

impl BusSender {
    pub fn send(&self, f: impl FnOnce(&mut App) + Send + 'static) {
        let _ = self.tx.send(Box::new(f));
        self.ctx.request_repaint();
    }
}
