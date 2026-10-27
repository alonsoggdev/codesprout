// Embeds the icon and version info in the Windows executable. The window icon set at runtime is
// only the small one: the taskbar, Explorer and shortcuts take the big one from the .exe.
//
// On Windows it also embeds the WSL agent (crate codesprout-wsl, built for
// x86_64-unknown-linux-musl) when it has been built: from `CODESPROUT_WSL_AGENT`, or from its usual
// place in target/. Without it, repositories in WSL still work, through wsl.exe and more slowly.
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../../build/icon.ico");
    println!("cargo::rustc-check-cfg=cfg(wsl_agent)");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../build/icon.ico");
        res.set("ProductName", "CodeSprout");
        res.set("FileDescription", "CodeSprout");
        res.compile().expect("no se pudo incrustar el icono en el ejecutable");
        embed_wsl_agent();
    }
}

fn embed_wsl_agent() {
    println!("cargo:rerun-if-env-changed=CODESPROUT_WSL_AGENT");
    let default = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/x86_64-unknown-linux-musl/release/codesprout-wsl-agent");
    println!("cargo:rerun-if-changed={}", default.display());
    let explicit = std::env::var_os("CODESPROUT_WSL_AGENT").map(PathBuf::from);
    let agent = explicit.clone().unwrap_or(default);
    if !agent.is_file() {
        // the release build asks for it explicitly and must not ship without it
        assert!(explicit.is_none(), "CODESPROUT_WSL_AGENT no apunta a un archivo: {}", agent.display());
        println!("cargo:warning=Sin el agente de WSL: compílalo con `cargo build -p codesprout-wsl --release --target x86_64-unknown-linux-musl`");
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("wsl-agent");
    std::fs::copy(&agent, out).expect("no se pudo copiar el agente de WSL");
    println!("cargo:rustc-cfg=wsl_agent");
}
