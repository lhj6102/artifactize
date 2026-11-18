//! Test the Unix opener-process argument boundary without racing the process PATH.
// Only Unix starts an opener process. Windows tests the same literal-target contract
// through its injected native ShellExecute handoff in platform/windows.rs instead.
#![cfg(unix)]

#[path = "support/os.rs"]
mod os;

use std::{ffi::OsStr, fs};

#[test]
fn target_is_one_literal_argument() {
    let root = os::tempdir();
    os::recording_program(root.path(), os::OPENER);
    let record = root.path().join("record");
    let output = os::rerun_recording(root.path(), &record, "open_fixture_targets");
    assert!(
        os::succeeded(&output.status),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(record).unwrap(),
        "1\nnotes with spaces.md\n1\nhttps://example.invalid/?a=1&b=two words\n"
    );
}

#[tokio::test]
#[ignore = "child entry point run with the fixture's isolated environment"]
async fn open_fixture_targets() {
    if !os::is_recording_child("open_fixture_targets") {
        return;
    }
    for target in [
        "notes with spaces.md",
        "https://example.invalid/?a=1&b=two words",
    ] {
        artifactize_tools::opener::open(OsStr::new(target))
            .await
            .unwrap();
    }
}
