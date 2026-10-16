//! The checked-in TypeScript bindings match the commands and events. Regenerate them with
//! `UPDATE_BINDINGS=1 cargo test -p artifactize-desktop --test bindings`.
use std::path::Path;

#[path = "../../artifactize-core/tests/support/os.rs"]
mod os;

#[test]
fn checked_in_bindings_match_the_commands() {
    let checked_in = Path::new(env!("CARGO_MANIFEST_DIR")).join(artifactize_desktop::BINDINGS);
    if os::environment_variable("UPDATE_BINDINGS").is_some() {
        artifactize_desktop::export(&checked_in).unwrap();
        return;
    }
    let scratch = os::tempdir_in(Path::new(env!("CARGO_TARGET_TMPDIR")), "bindings-");
    let generated = scratch.path().join("bindings.ts");
    artifactize_desktop::export(&generated).unwrap();
    let expected = std::fs::read_to_string(&generated).unwrap();
    let actual = std::fs::read_to_string(&checked_in).unwrap_or_default();
    assert!(
        expected == actual,
        "{} is out of date; regenerate it with UPDATE_BINDINGS=1 cargo test -p artifactize-desktop --test bindings",
        artifactize_tools::files::path_text(&checked_in)
    );
}
