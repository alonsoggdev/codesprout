//! CodeSprout's Git engine. Every operation runs the `git` executable directly (no shell)
//! and parses its porcelain output, so it behaves exactly like git on the command line:
//! hooks, credential helpers and the user's configuration all apply.

pub mod credentials;
pub mod diff;
pub mod explorer;
pub mod extras;
pub mod flow;
pub mod history;
pub mod hosting;
pub mod model;
pub mod rebase;
pub mod repo;
pub mod runner;
pub mod update;
pub mod wsl;

pub use model::*;
pub use runner::OpHandle;
