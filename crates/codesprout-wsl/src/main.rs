//! CodeSprout's agent inside WSL. See the protocol in `lib.rs`.

#[cfg(target_os = "linux")]
mod agent;

fn main() {
    #[cfg(target_os = "linux")]
    agent::run();
    #[cfg(not(target_os = "linux"))]
    eprintln!("El agente de CodeSprout solo funciona dentro de Linux (WSL).");
}
