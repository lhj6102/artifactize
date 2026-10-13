//! The checked-in TypeScript bindings match the commands and events. Regenerate them with
//! `UPDATE_BINDINGS=1 cargo test -p artifactize-desktop --test bindings`.
use std::path::Path;

#[test]
fn checked_in_bindings_match_the_commands() {
    let checked_in = Path::new(env!("CARGO_MANIFEST_DIR")).join(artifactize_desktop::BINDINGS);
    if std::env::var_os("UPDATE_BINDINGS").is_some() {
        artifactize_desktop::export(&checked_in).unwrap();
        return;
    }
    let scratch = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let generated = scratch.path().join("bindings.ts");
    artifactize_desktop::export(&generated).unwrap();
    let expected = std::fs::read_to_string(&generated).unwrap();
    let actual = std::fs::read_to_string(&checked_in).unwrap_or_default();
    assert!(
        expected == actual,
        "{} is out of date; regenerate it with UPDATE_BINDINGS=1 cargo test -p artifactize-desktop --test bindings",
        checked_in.display()
    );
}
