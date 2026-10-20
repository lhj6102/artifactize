//! A program that artifactize starts from inside an AppImage gets the person's environment,
//! not the one the bundle's AppRun and hooks gave the app; anywhere else nothing changes.
// AppImages exist only on Linux; elsewhere the environment is always passed unchanged.
#![cfg(target_os = "linux")]

#[path = "support/os.rs"]
mod os;

use std::{collections::BTreeMap, ffi::OsStr, fs, path::Path};

/// The person's environment before the AppImage started: their opener's directory first.
fn person(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("PATH".into(), text(&root.join("bin"))),
        (
            "XDG_DATA_DIRS".into(),
            "/usr/local/share/:/usr/share/:/var/lib/flatpak/exports/share".into(),
        ),
        ("AUTHOR_VARIABLE".into(), "from-the-terminal".into()),
        (os::RECORD.into(), text(&root.join("record"))),
    ])
}

/// What the AppImage runtime, the linuxdeploy GTK hook and AppRun make of `person`, for the
/// bundle `bundle`, in the forms they write it (with their doubled and trailing slashes).
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
                format!("{b}/usr/share/glib-2.0/schemas/:{b}//usr/share/glib-2.0/schemas"),
            ),
            (
                "XDG_DATA_DIRS",
                format!(
                    "{b}/usr/share/:{b}/usr/share:/usr/share:{}",
                    person["XDG_DATA_DIRS"]
                ),
            ),
            (
                "PATH",
                format!("{b}/usr/bin/:{b}/usr/sbin/:{b}/bin/:{}", person["PATH"]),
            ),
            ("LD_LIBRARY_PATH", format!("{b}/usr/lib/:{b}/lib/:")),
            (
                "PERLLIB",
                format!("{b}/usr/share/perl5/:{b}/usr/lib/perl5/:"),
            ),
            ("PYTHONHOME", format!("{b}/usr/")),
            ("PYTHONDONTWRITEBYTECODE", "1".into()),
        ]
        .map(|(name, value)| (name.to_owned(), value)),
    );
    environment
}

/// Run this test binary, as `executable`, with exactly `environment`, and return the
/// environment the opener it starts received.
fn opened_with(
    root: &Path,
    executable: &Path,
    test: &str,
    environment: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let output = std::process::Command::new(executable)
        .args(["--exact", test, "--nocapture"])
        .env_clear()
        .envs(environment)
        .output()
        .unwrap();
    assert!(
        os::succeeded(&output.status),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let record = fs::read_to_string(root.join("record")).unwrap();
    fs::remove_file(root.join("record")).unwrap();
    record
        .lines()
        .map(|line| {
            let (name, value) = line.split_once('=').unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

/// A bundle holding a copy of this test binary where the app's executable would be, and a
/// stand-in opener in the person's own program directory.
fn fixture(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let bundle = root.join("squashfs-root");
    let bin = bundle.join("usr/bin");
    fs::create_dir_all(&bin).unwrap();
    let inside = bin.join("artifactize-desktop");
    fs::copy(std::env::current_exe().unwrap(), &inside).unwrap();
    fs::create_dir(root.join("bin")).unwrap();
    os::compile(
        &root.join("bin"),
        os::OPENER,
        r#"use std::io::Write;
fn main() {
    let record = std::env::var_os("ARTIFACTIZE_OPENER_RECORD").expect("record path");
    let mut file = std::fs::File::create(record).unwrap();
    for (name, value) in std::env::vars() {
        writeln!(file, "{name}={value}").unwrap();
    }
}
"#,
    );
    (bundle, inside)
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
    let root = os::tempdir();
    let (bundle, inside) = fixture(root.path());
    let person = person(root.path());
    let received = opened_with(
        root.path(),
        &inside,
        "a_program_started_from_inside_an_appimage_gets_the_persons_environment",
        &bundled(&bundle, &person),
    );
    assert_eq!(received, person);
}

#[test]
fn without_an_appimage_a_started_program_gets_the_environment_unchanged() {
    if open_when_rerun() {
        return;
    }
    let test = "without_an_appimage_a_started_program_gets_the_environment_unchanged";
    let root = os::tempdir();
    let (bundle, inside) = fixture(root.path());
    let bundled = bundled(&bundle, &person(root.path()));
    // The same environment without APPDIR, as when nothing exported it.
    let mut unnamed = bundled.clone();
    unnamed.remove("APPDIR");
    assert_eq!(opened_with(root.path(), &inside, test, &unnamed), unnamed);
    // An APPDIR this program is not inside, as in a terminal started from another AppImage.
    let outside = std::env::current_exe().unwrap();
    assert_eq!(opened_with(root.path(), &outside, test, &bundled), bundled);
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}
