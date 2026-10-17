#[cfg(feature = "e2e")]
#[path = "src/e2e_input/platform/build.rs"]
mod input_build;

fn main() {
    #[cfg(feature = "e2e")]
    input_build::compile();
    // Tauri embeds ui/dist when this crate compiles, and asks for a rebuild only when a file it
    // embedded changes: a dist built after a build that saw it half-written (or without a new
    // asset, such as the layout worker) would stay out of the app. Watching the whole directory
    // rebuilds on any added, removed or changed file.
    println!("cargo:rerun-if-changed=ui/dist");
    windowless_release();
    println!("cargo:rerun-if-changed=capabilities");
    println!("cargo:rerun-if-changed=capabilities-e2e");
    let pattern = if std::env::var_os("CARGO_FEATURE_E2E").is_some() {
        "capabilities*/*.json"
    } else {
        "capabilities/*.json"
    };
    tauri_build::try_build(tauri_build::Attributes::new().capabilities_path_pattern(pattern))
        .expect("desktop capabilities must build");
}

/// The release app has no console window on Windows. That is a property of how the program is
/// linked on that system, so this build step sets it rather than the program's own code.
fn windowless_release() {
    let target = |name: &str| std::env::var(name).unwrap_or_default();
    if target("CARGO_CFG_TARGET_OS") != "windows" || target("PROFILE") != "release" {
        return;
    }
    if target("CARGO_CFG_TARGET_ENV") == "msvc" {
        println!("cargo:rustc-link-arg-bins=/SUBSYSTEM:WINDOWS");
        println!("cargo:rustc-link-arg-bins=/ENTRY:mainCRTStartup");
    } else {
        println!("cargo:rustc-link-arg-bins=-mwindows");
    }
}
