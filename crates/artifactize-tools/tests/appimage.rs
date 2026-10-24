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

/// The person's environment before the AppImage started: their opener's directory first.
fn person(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("PATH".into(), os::path_list([root.join("bin")])),
        (
            "XDG_DATA_DIRS".into(),
            os::path_list([
                "/usr/local/share/",
                "/usr/share/",
                "/var/lib/flatpak/exports/share",
            ]),
        ),
        ("AUTHOR_VARIABLE".into(), "from-the-terminal".into()),
        (os::RECORD.into(), text(&root.join("record"))),
    ])
}

/// What the AppImage runtime, the linuxdeploy GTK hook and AppRun make of `person`, for the
/// bundle `bundle`, in the forms they write it: with doubled slashes, and with an empty last
/// entry where the person had no value.
fn bundled(bundle: &Path, person: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let b = text(bundle);
    let mut environment = person.clone();
    environment.extend(
        [
            ("APPDIR", b.clone()),
            ("APPIMAGE", format!("{b}.AppImage")),
            ("ARGV0", "./Artifactize.AppImage".into()),
            ("OWD", "/home/someone".into()),
            ("GTK_THEME", "Adwaita:light".into()),
            ("GTK_DATA_PREFIX", b.clone()),
            ("GTK_EXE_PREFIX", format!("{b}//usr")),
            ("GTK_PATH", format!("{b}//usr/lib/gtk-3.0")),
            ("GIO_MODULE_DIR", format!("{b}//usr/lib/gio/modules")),
            (
                "GDK_PIXBUF_MODULE_FILE",
                format!("{b}//usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"),
            ),
            (
                "GSETTINGS_SCHEMA_DIR",
                os::path_list([
                    format!("{b}/usr/share/glib-2.0/schemas/"),
                    format!("{b}//usr/share/glib-2.0/schemas"),
                ]),
            ),
            (
                "XDG_DATA_DIRS",
                prepended(
                    [
                        format!("{b}/usr/share/"),
                        format!("{b}/usr/share"),
                        "/usr/share".into(),
                    ],
                    &person["XDG_DATA_DIRS"],
                ),
            ),
            (
                "PATH",
                prepended(
                    [
                        format!("{b}/usr/bin/"),
                        format!("{b}/usr/sbin/"),
                        format!("{b}/bin/"),
                    ],
                    &person["PATH"],
                ),
            ),
            (
                "LD_LIBRARY_PATH",
                os::path_list([format!("{b}/usr/lib/"), format!("{b}/lib/"), String::new()]),
            ),
            (
                "PERLLIB",
                os::path_list([
                    format!("{b}/usr/share/perl5/"),
                    format!("{b}/usr/lib/perl5/"),
                    String::new(),
                ]),
            ),
            ("PYTHONHOME", format!("{b}/usr/")),
            ("PYTHONDONTWRITEBYTECODE", "1".into()),
        ]
        .map(|(name, value)| (name.to_owned(), value)),
    );
    environment
}

/// Run `program`, a copy of this test binary, with exactly `environment`, and return the
/// environment the opener it starts received.
fn opened_with(
    root: &Path,
    program: &Path,
    test: &str,
    environment: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    // No record of an earlier opener: only this run's can be read.
    let record = root.join("record");
    if let Err(error) = std::fs::remove_file(&record) {
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}");
    }
    os::rerun_as(program, test, environment);
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
        os::environment_recording_program(&root.path().join("bin"), os::OPENER);
        Self {
            root,
            bundle,
            inside,
            outside,
        }
    }
}

/// In the rerun, hand one target to the desktop opener.
fn open_when_rerun() -> bool {
    if os::record_path().is_none() {
        return false;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(artifactize_tools::opener::open(OsStr::new("notes.md")))
        .unwrap();
    true
}

#[test]
fn a_program_started_from_inside_an_appimage_gets_the_persons_environment() {
    if open_when_rerun() {
        return;
    }
    let fixture = Fixture::new();
    let root = fixture.root.path();
    let person = person(root);
    let received = opened_with(
        root,
        &fixture.inside,
        "a_program_started_from_inside_an_appimage_gets_the_persons_environment",
        &bundled(&fixture.bundle, &person),
    );
    assert_eq!(received, person);
}

#[test]
fn without_an_appimage_a_started_program_gets_the_environment_unchanged() {
    if open_when_rerun() {
        return;
    }
    let test = "without_an_appimage_a_started_program_gets_the_environment_unchanged";
    let fixture = Fixture::new();
    let root = fixture.root.path();
    let bundled = bundled(&fixture.bundle, &person(root));
    // The same environment without APPDIR, as when nothing exported it.
    let mut unnamed = bundled.clone();
    unnamed.remove("APPDIR");
    assert_eq!(opened_with(root, &fixture.inside, test, &unnamed), unnamed);
    // An APPDIR this program is not inside, as in a terminal started from another AppImage.
    assert_eq!(opened_with(root, &fixture.outside, test, &bundled), bundled);
}

/// The path list `list` with `entries` before its own.
fn prepended<const N: usize>(entries: [String; N], list: &str) -> String {
    os::path_list(
        entries
            .into_iter()
            .map(PathBuf::from)
            .chain(std::env::split_paths(list)),
    )
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}
