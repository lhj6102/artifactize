//! Test desktop handoff in a subprocess so PATH never races other tests.
#![cfg(unix)]

use std::{ffi::OsStr, fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn target_is_one_literal_argument() {
    if let Some(record) = std::env::var_os("ARTIFACTIZE_OPENER_RECORD") {
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
    let root = tempfile::tempdir().unwrap();
    let opener = root.path().join(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    });
    fs::write(
        &opener,
        "#!/bin/sh\nprintf '%s\\n' \"$#\" \"$1\" >> \"$ARTIFACTIZE_OPENER_RECORD\"\n",
    )
    .unwrap();
    fs::set_permissions(opener, fs::Permissions::from_mode(0o700)).unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "target_is_one_literal_argument", "--nocapture"])
        .env("PATH", root.path())
        .env("ARTIFACTIZE_OPENER_RECORD", root.path().join("record"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
