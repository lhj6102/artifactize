fn main() {
    // Tauri embeds ui/dist when this crate compiles, and asks for a rebuild only when a file it
    // embedded changes: a dist built after a build that saw it half-written (or without a new
    // asset, such as the layout worker) would stay out of the app. Watching the whole directory
    // rebuilds on any added, removed or changed file.
    println!("cargo:rerun-if-changed=ui/dist");
    tauri_build::build();
}
