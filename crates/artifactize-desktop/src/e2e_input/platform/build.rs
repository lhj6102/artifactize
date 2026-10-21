//! Build the engine-only text input adapters only for the desktop's E2E feature.

/// Compile the adapter of the target's engine; an error says what the build machine lacks, such
/// as WebKitGTK's development files.
pub fn compile() -> Result<(), String> {
    // Cargo sets the target OS for every build script.
    match std::env::var("CARGO_CFG_TARGET_OS")
        .unwrap_or_default()
        .as_str()
    {
        "linux" => {
            let webkit = pkg_config::Config::new()
                .probe("webkit2gtk-4.1")
                .map_err(|error| {
                    format!("the E2E input adapter needs WebKitGTK's development files: {error}")
                })?;
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
    Ok(())
}
