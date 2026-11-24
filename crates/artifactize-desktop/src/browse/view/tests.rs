use super::*;

#[cfg(unix)]
fn not_unicode() -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from("/repos").join(std::ffi::OsStr::from_bytes(b"caf\xe9"))
}

#[cfg(windows)]
fn not_unicode() -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    // A lone surrogate is a valid Windows name and no Unicode text.
    PathBuf::from(std::ffi::OsString::from_wide(&[
        u16::from(b'C'),
        u16::from(b':'),
        u16::from(b'\\'),
        0xD800,
    ]))
}

#[test]
fn a_branch_key_brings_back_the_same_native_path_from_its_wire_text() {
    for path in [PathBuf::from("/repos/plain"), not_unicode()] {
        let key = BranchKey::new(&Place::Folder(path.clone()));
        let wire = serde_json::to_value(&key).unwrap();
        assert!(wire["path"].is_string(), "{wire}");
        let back: BranchKey = serde_json::from_value(wire).unwrap();
        assert_eq!(back.to_place(), Place::Folder(path));
    }
}

#[test]
fn overlapping_runs_show_the_older_active_run_first_with_elapsed_time() {
    let workspace = PathBuf::from("a-checkout");
    let chip = |id: &str, created_at: &str, status| live::Chip {
        id: id.parse().unwrap(),
        created_at: created_at.parse().unwrap(),
        status,
        completed_at: None,
        comment: None,
    };
    let branch = Branch {
        place: Place::Folder(workspace.clone()),
        workspace,
        project: "project".into(),
        name: "main".into(),
        runs: vec![
            chip("run-active", "2026-11-22T09:50:00Z", RunStatus::Running),
            chip("run-ended", "2026-11-22T09:55:00Z", RunStatus::Green),
        ],
    };
    let now = "2026-11-22T10:00:00Z"
        .parse::<artifactize_core::types::Timestamp>()
        .unwrap()
        .time();
    let limited = super::branch(&branch, 1, false, now);
    assert!(limited.older);
    assert_eq!(
        serde_json::to_value(limited).unwrap()["newest"]["id"],
        "run-ended"
    );
    let chips = serde_json::to_value(super::branch(&branch, 2, false, now)).unwrap();
    assert_eq!(chips["chips"][0]["id"], "run-active");
    assert_eq!(chips["chips"][0]["running"], true);
    assert_eq!(chips["chips"][0]["age"], "10m 00s");
    assert_eq!(chips["chips"][1]["id"], "run-ended");
    assert_eq!(chips["chips"][1]["running"], false);
    assert_eq!(chips["chips"][1]["age"], "5m 00s ago");
}
