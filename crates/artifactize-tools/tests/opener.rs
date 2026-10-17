//! Test the Unix opener-process argument boundary without racing the process PATH.
// Only Unix starts an opener process. Windows tests the same literal-target contract
// through its injected native ShellExecute handoff in platform/windows.rs instead.
#![cfg(unix)]

#[path = "support/os.rs"]
mod os;

use std::{ffi::OsStr, fs};

#[test]
fn target_is_one_literal_argument() {
    if let Some(record) = os::record_path() {
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
    let output = os::rerun_recording(root.path(), "target_is_one_literal_argument");
    assert!(
        os::succeeded(&output.status),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
