// Embeds the icon and version info in the Windows executable. The window icon set at runtime is
// only the small one: the taskbar, Explorer and shortcuts take the big one from the .exe.
fn main() {
    println!("cargo:rerun-if-changed=../../build/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../build/icon.ico");
        res.set("ProductName", "CodeSprout");
        res.set("FileDescription", "CodeSprout");
        res.compile().expect("no se pudo incrustar el icono en el ejecutable");
    }
}
