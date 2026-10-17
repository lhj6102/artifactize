#[path = "src/platform/native_ime/build.rs"]
mod native_ime_build;

const COMMANDS: &[&str] = &[];

fn main() {
    native_ime_build::compile();
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
