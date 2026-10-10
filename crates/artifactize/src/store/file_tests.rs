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
