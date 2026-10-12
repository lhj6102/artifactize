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

// Pending delete is a Windows-only disposition: Unix unlinks the name immediately.
#[cfg(windows)]
#[test]
fn regular_files_retries_past_a_windows_pending_delete_sidecar() {
    let root = crate::test_os::tempdir();
    let state = root.path().join("state");
    std::fs::create_dir(&state).unwrap();
    for suffix in ["", "-wal", "-shm"] {
        std::fs::write(state.join(format!("{DATABASE}{suffix}")), "regular").unwrap();
    }
    let wal = state.join(format!("{DATABASE}-wal"));
    let holder = crate::test_os::pending_delete(&wal);
    // Prove the fixture reproduced the race before entering the production probe path.
    let direct = platform::path_kind(&wal).unwrap_err();
    assert!(platform::transient_file_access(&direct), "{direct}");
    let mut holder = Some(holder);
    receipts::regular_files_with_retry(&state, || {
        // This hook runs only after a failed transient probe and before its next attempt.
        // Closing synchronously makes progress independent of clocks and thread scheduling.
        drop(
            holder
                .take()
                .expect("only the pending-delete probe retries"),
        );
    })
    .unwrap();
    assert!(
        holder.is_none(),
        "the regular-files probe must have retried"
    );
    assert!(
        !wal.exists(),
        "the sidecar is gone when its last handle closes"
    );
}
