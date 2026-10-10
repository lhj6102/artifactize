//! The store boundary refuses database links before SQLite sees them.
use super::*;

#[test]
fn database_and_sidecars_require_regular_entries_even_for_dangling_links() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let target = root.path().join("target");
    std::fs::write(&target, "outside").unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let path = state.join(format!("{DATABASE}{suffix}"));
        std::fs::create_dir(&path).unwrap();
        assert!(
            receipts::regular_files(&state)
                .unwrap_err()
                .contains("regular files")
        );
        std::fs::remove_dir(&path).unwrap();
        for target in [&target, &root.path().join("missing")] {
            if crate::test_os::symlink_file(target, &path).is_some() {
                assert!(
                    receipts::regular_files(&state)
                        .unwrap_err()
                        .contains("regular files")
                );
                std::fs::remove_file(&path).unwrap();
            }
        }
        std::fs::write(&path, "regular").unwrap();
        receipts::regular_files(&state).unwrap();
        std::fs::remove_file(path).unwrap();
    }
    assert_eq!(std::fs::read_to_string(target).unwrap(), "outside");
}

/// Deleting a file while another handle still holds it open (with `FILE_SHARE_DELETE`, as
/// `DeleteFile` requires) marks its name "pending delete": any new open, even attribute-only
/// and even with compatible sharing, fails `ERROR_ACCESS_DENIED` until every handle, including
/// the one that was already open, closes. This is the exact race a WAL sidecar's own close can
/// leave behind. `regular_files` must retry past it rather than surface a flake.
#[cfg(windows)]
#[test]
fn regular_files_retries_past_a_windows_pending_delete_sidecar() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;

    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    std::fs::create_dir(&state).unwrap();
    for suffix in ["", "-wal", "-shm"] {
        std::fs::write(state.join(format!("{DATABASE}{suffix}")), "regular").unwrap();
    }
    let wal = state.join(format!("{DATABASE}-wal"));
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_DELETE)
        .open(&wal)
        .unwrap();
    std::fs::remove_file(&wal).unwrap();
    // Confirm this test actually reproduces the race: a direct, non-retrying probe sees it.
    let direct = platform::path_kind(&wal).unwrap_err();
    assert!(platform::transient_file_access(&direct), "{direct}");
    let released = std::thread::spawn(move || {
        std::thread::sleep(super::TRANSIENT_ACCESS_BACKOFF * 2);
        drop(holder);
    });
    // Retrying past the same race (the sidecar is actually gone once `holder` closes, which
    // `regular_files` also accepts) succeeds instead of surfacing "Access is denied".
    receipts::regular_files(&state).unwrap();
    released.join().unwrap();
}
