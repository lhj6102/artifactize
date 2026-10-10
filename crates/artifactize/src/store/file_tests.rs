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

/// Opening a file with `FILE_FLAG_DELETE_ON_CLOSE` marks its name "delete pending" from that
/// instant, not just once the handle closes: a concurrent probe of the same name fails with
/// `ERROR_ACCESS_DENIED` until every handle (this one included) closes, even though nothing
/// holds an incompatible lock on it. This is the exact race a WAL sidecar's own close can leave
/// behind on Windows. `regular_files` must retry past it rather than surface a flake.
#[cfg(windows)]
#[test]
fn regular_files_retries_past_a_windows_delete_pending_sidecar() {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_FLAG_DELETE_ON_CLOSE (winnt.h), not otherwise exposed by `std`.
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;

    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    std::fs::create_dir(&state).unwrap();
    for suffix in ["", "-wal", "-shm"] {
        std::fs::write(state.join(format!("{DATABASE}{suffix}")), "regular").unwrap();
    }
    let wal = state.join(format!("{DATABASE}-wal"));
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(&wal)
        .unwrap();
    // Confirm this test actually reproduces the race: a direct, non-retrying probe sees it.
    let direct = platform::path_kind(&wal).unwrap_err();
    assert!(platform::transient_file_access(&direct), "{direct}");
    let released = std::thread::spawn(move || {
        std::thread::sleep(super::TRANSIENT_ACCESS_BACKOFF * 2);
        drop(holder);
    });
    // Retrying past the same race (the sidecar is gone once `holder` closes, which
    // `regular_files` also accepts) succeeds instead of surfacing "Access is denied".
    receipts::regular_files(&state).unwrap();
    released.join().unwrap();
}
