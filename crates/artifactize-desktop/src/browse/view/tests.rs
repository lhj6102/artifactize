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
