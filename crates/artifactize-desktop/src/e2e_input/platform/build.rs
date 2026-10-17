//! Build the engine-only text input adapters only for the desktop's E2E feature.
pub fn compile() {
    match std::env::var("CARGO_CFG_TARGET_OS")
        .expect("Cargo target OS")
        .as_str()
    {
        "linux" => {
            let webkit = pkg_config::Config::new()
                .probe("webkit2gtk-4.1")
                .expect("WebKitGTK development files");
            let mut build = cc::Build::new();
            build.file("src/e2e_input/platform/gtk.c");
            for path in webkit.include_paths {
                build.include(path);
            }
            build.compile("artifactize_e2e_input");
        }
        "macos" => {
            cc::Build::new()
                .file("src/e2e_input/platform/cocoa.m")
                .flag("-fobjc-arc")
                .compile("artifactize_e2e_input");
            println!("cargo:rustc-link-lib=framework=AppKit");
            println!("cargo:rustc-link-lib=framework=WebKit");
        }
        _ => {}
    }
    println!("cargo:rerun-if-changed=src/e2e_input/platform/gtk.c");
    println!("cargo:rerun-if-changed=src/e2e_input/platform/cocoa.m");
}
