//! A program that artifactize starts from inside an AppImage gets the person's environment,
//! not the one the bundle's AppRun and hooks gave the app; anywhere else nothing changes.
// AppImages exist only on Linux; elsewhere the environment is always passed unchanged.
#![cfg(target_os = "linux")]

#[path = "support/os.rs"]
mod os;

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

/// Run `program`, a copy of this test binary, with exactly `environment`, and return the
/// environment the opener it starts received.
fn opened_with(
    root: &Path,
    program: &Path,
    environment: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    // No record of an earlier opener: only this run's can be read.
    let record = root.join("record");
    if let Err(error) = std::fs::remove_file(&record) {
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
    }
    os::rerun_recording_as(program, "open_fixture_target", environment);
    os::recorded_environment(&record)
}

/// A bundle holding a copy of this test binary where the app's executable would be, a copy
/// outside it, and a stand-in opener in the person's own program directory.
struct Fixture {
    root: tempfile::TempDir,
    bundle: PathBuf,
    inside: PathBuf,
    outside: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = os::tempdir();
        let bundle = root.path().join("squashfs-root");
        let bin = bundle.join("usr/bin");
        fs::create_dir_all(&bin).unwrap();
        let inside = bin.join("artifactize-desktop");
        os::copy_test_binary(&inside);
        let outside = root.path().join("artifactize-desktop");
        os::copy_test_binary(&outside);
        fs::create_dir(root.path().join("bin")).unwrap();
        os::environment_recording_program(&root.path().join("bin"), os::opener_program());
        Self {
            root,
            bundle,
            inside,
            outside,
        }
    }
}

/// In the rerun, hand one target to the desktop opener.
#[tokio::test]
#[ignore = "child entry point run with the fixture's isolated environment"]
async fn open_fixture_target() {
    if !os::is_recording_child("open_fixture_target") {
        return;
    }
    artifactize_tools::opener::open(OsStr::new("notes.md"))
        .await
        .unwrap();
}

#[test]
fn a_program_started_from_inside_an_appimage_gets_the_persons_environment() {
    let fixture = Fixture::new();
    let root = fixture.root.path();
    let person = os::appimage_person(root);
    let received = opened_with(
        root,
        &fixture.inside,
        &os::appimage_bundled(&fixture.bundle, &person),
    );
    assert_eq!(received, person);
}

#[test]
fn without_an_appimage_a_started_program_gets_the_environment_unchanged() {
    let fixture = Fixture::new();
    let root = fixture.root.path();
    let bundled = os::appimage_bundled(&fixture.bundle, &os::appimage_person(root));
    let unnamed = os::appimage_without_bundle_marker(bundled.clone());
    assert_eq!(opened_with(root, &fixture.inside, &unnamed), unnamed);
    // The program is outside the bundle, as in a terminal started from another AppImage.
    assert_eq!(opened_with(root, &fixture.outside, &bundled), bundled);
}
