//! Test desktop handoff in a subprocess so PATH never races other tests.
// Windows opens through ShellExecute, which runs no program a test can stand in for.
#![cfg(unix)]

#[path = "support/os.rs"]
mod os;

use std::{ffi::OsStr, fs};

#[test]
fn target_is_one_literal_argument() {
    if let Some(record) = std::env::var_os(os::RECORD) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for target in [
            "notes with spaces.md",
            "https://example.invalid/?a=1&b=two words",
        ] {
            runtime
                .block_on(artifactize_tools::opener::open(OsStr::new(target)))
                .unwrap();
        }
        assert_eq!(
            fs::read_to_string(record).unwrap(),
            "1\nnotes with spaces.md\n1\nhttps://example.invalid/?a=1&b=two words\n"
        );
        return;
    }
    let root = os::tempdir();
    os::recording_program(root.path(), os::OPENER);
    let output = os::rerun("target_is_one_literal_argument")
        .env("PATH", root.path())
        .env(os::RECORD, root.path().join("record"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
