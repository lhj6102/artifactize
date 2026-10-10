use std::{ffi::OsStr, fs};

use artifactize_tools::files::{self, FileKind};

#[test]
fn listings_and_kinds_stay_relative_to_the_pinned_directory() {
    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("original");
    fs::create_dir(&original).unwrap();
    fs::write(original.join("input"), "pinned").unwrap();
    let directory = files::open_directory(&original).unwrap();
    fs::rename(&original, root.path().join("moved")).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(original.join("replacement"), "not pinned").unwrap();

    // Independent scans must start at the beginning, including while another scan is live.
    let first = files::read_dir(&directory).unwrap();
    for entries in [first, files::read_dir(&directory).unwrap()] {
        let entries: Vec<_> = entries.map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(entries, [OsStr::new("input")]);
    }
    assert_eq!(
        files::entry_kind(&directory, OsStr::new("input")).unwrap(),
        FileKind::File
    );
    assert_eq!(
        files::entry_kind(&directory, OsStr::new("replacement"))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}

#[cfg(unix)]
#[test]
fn listings_and_entry_kind_inspect_links_without_following_them() {
    let root = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("missing", root.path().join("link")).unwrap();
    let directory = files::open_directory(root.path()).unwrap();
    let entry = files::read_dir(&directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(entry.file_name(), "link");
    assert_eq!(entry.file_type().unwrap(), FileKind::Symlink);
    assert_eq!(
        files::entry_kind(&directory, OsStr::new("link")).unwrap(),
        FileKind::Symlink
    );
    let name = files::EntryName::new(OsStr::new("link")).unwrap();
    assert!(files::is_link_refusal(
        &files::open_entry(&directory, &name).unwrap_err()
    ));
}
