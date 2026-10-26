fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }
    println!("cargo:rerun-if-changed=images/dubSuite.ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("images/dubSuite.ico");
    res.compile()
        .expect("intégration de images/dubSuite.ico dans l'exécutable");
}
