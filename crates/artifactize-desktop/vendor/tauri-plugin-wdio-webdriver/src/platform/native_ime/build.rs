pub fn compile() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").expect("Cargo target OS");
    match os.as_str() {
        "linux" => {
            let webkit = pkg_config::Config::new()
                .probe("webkit2gtk-4.1")
                .expect("WebKitGTK development files");
            let mut build = cc::Build::new();
            build.file("src/platform/native_ime/gtk.c");
            for include in webkit.include_paths {
                build.include(include);
            }
            build.compile("wdio_native_ime");
        }
        "macos" => {
            cc::Build::new()
                .file("src/platform/native_ime/cocoa.m")
                .flag("-fobjc-arc")
                .compile("wdio_native_ime");
            println!("cargo:rustc-link-lib=framework=AppKit");
            println!("cargo:rustc-link-lib=framework=WebKit");
        }
        _ => {}
    }
    println!("cargo:rerun-if-changed=src/platform/native_ime/gtk.c");
    println!("cargo:rerun-if-changed=src/platform/native_ime/cocoa.m");
}
