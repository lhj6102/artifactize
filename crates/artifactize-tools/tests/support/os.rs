//! Operating-system fixtures for the unit and integration tests alike: unique temporary
//! roots, links, executable files and a recording stand-in for the desktop opener.
#![expect(
    dead_code,
    reason = "each test binary uses a different subset of OS fixtures"
)]

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// A temporary fixture below the physical OS temp root, not macOS /var or Windows 8.3
/// aliases, so that paths the tools print compare equal to the fixture's own.
pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
}

/// Make `path` runnable by name: mode 0700 on Unix. Windows runs any file whose
/// extension is in `PATHEXT`, so nothing changes there.
pub fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    let _ = path;
}

/// A symbolic link to a file, which may not exist. On Windows this needs Developer Mode or
/// an elevated process; `None` then, after saying the check is skipped.
pub fn symlink_file(target: impl AsRef<Path>, link: impl AsRef<Path>) -> Option<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).unwrap();
        Some(())
    }
    #[cfg(windows)]
    {
        // ERROR_PRIVILEGE_NOT_HELD
        match std::os::windows::fs::symlink_file(target, link) {
            Err(error) if error.raw_os_error() == Some(1314) => {
                eprintln!("skipping a symbolic-link check: creating one needs Developer Mode");
                None
            }
            created => {
                created.unwrap();
                Some(())
            }
        }
    }
}

/// The program a Unix desktop opens files with: `open` on macOS, `xdg-open` elsewhere.
/// Windows opens through ShellExecute and runs no program by name.
#[cfg(unix)]
pub const OPENER: &str = if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

/// This test binary, set to run only the test `name`: for a test that runs itself again in
/// an environment of its own, such as a `PATH` that holds only stand-ins.
pub fn rerun(name: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", name, "--nocapture"]);
    command
}

/// The environment variable naming the file a recording program appends to.
pub const RECORD: &str = "ARTIFACTIZE_OPENER_RECORD";

/// Compile one Rust source file into the program `name` in `directory`, with the compiler
/// Cargo names in `RUSTC`. A compiled stand-in needs no shell on any system.
pub fn compile(directory: &Path, name: &str, source: &str) -> PathBuf {
    let file = directory.join(format!("{name}-source.rs"));
    fs::write(&file, source).unwrap();
    let program = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .args(["--edition", "2024", "-o"])
        .arg(&program)
        .arg(&file)
        .output()
        .expect("rustc builds the stand-in program");
    assert!(
        output.status.success(),
        "building the stand-in program failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    make_executable(&program);
    program
}

/// Build a program named `name` in `directory` that appends its argument count and first
/// argument, one per line, to the file named by `RECORD`.
pub fn recording_program(directory: &Path, name: &str) -> PathBuf {
    compile(
        directory,
        name,
        r#"use std::io::Write;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let record = std::env::var_os("ARTIFACTIZE_OPENER_RECORD").expect("record path");
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(record).unwrap();
    writeln!(file, "{}", args.len()).unwrap();
    writeln!(file, "{}", args.first().map(String::as_str).unwrap_or_default()).unwrap();
}
"#,
    )
}

/// The namespace policy of the actual fixture volume, independent of OS defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasePolicy {
    Sensitive,
    Insensitive,
}

pub fn case_policy(root: &Path, existing: &str) -> CasePolicy {
    // The caller owns this pre-existing fixture entry and has not created its alias.
    // Looking up that alias observes the namespace without changing the directory.
    assert!(root.join(existing).exists());
    let alias = existing.to_ascii_uppercase();
    assert_ne!(alias, existing);
    if root.join(alias).exists() {
        CasePolicy::Insensitive
    } else {
        CasePolicy::Sensitive
    }
}

pub fn executable_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// A file used only for executable lookup, never launched by a test.
pub fn lookup_program(directory: &Path, name: &str, extension: Option<&str>) -> PathBuf {
    let name = extension.map_or_else(
        || executable_name(name),
        |extension| format!("{name}.{extension}"),
    );
    let path = directory.join(name);
    fs::write(&path, "lookup fixture").unwrap();
    make_executable(&path);
    path
}

/// Change only a fixture entry's spelling, including on systems whose direct case-only
/// rename would keep the old spelling. Setup owns the temporary intermediate name.
pub fn rename_case(root: &Path, from: &str, to: &str) {
    let staging = tempfile::Builder::new()
        .prefix("case-rename-")
        .tempdir_in(root)
        .unwrap();
    fs::remove_dir(staging.path()).unwrap();
    fs::rename(root.join(from), staging.path()).unwrap();
    fs::rename(staging.path(), root.join(to)).unwrap();
}

/// The command Cargo built, with native process setup kept in the OS fixture layer.
pub fn tools(binary: &str) -> Command {
    Command::new(binary)
}

pub fn succeeded(status: &std::process::ExitStatus) -> bool {
    status.success()
}

pub fn tools_output(binary: &str, args: &[&str]) -> std::process::Output {
    tools(binary).args(args).output().unwrap()
}

pub fn call(binary: &str, root: &Path, args: &[&str]) -> std::process::Output {
    tools(binary).current_dir(root).args(args).output().unwrap()
}

pub fn help(binary: &str, root: &Path, program: &str, args: &[&str]) -> std::process::Output {
    tools(binary)
        .current_dir(root)
        .env("PATH", root)
        .arg("help")
        .arg(program)
        .args(args)
        .output()
        .unwrap()
}

pub fn record_path() -> Option<std::ffi::OsString> {
    std::env::var_os(RECORD)
}

pub fn rerun_recording(root: &Path, test: &str) -> std::process::Output {
    rerun(test)
        .env("PATH", root)
        .env(RECORD, root.join("record"))
        .output()
        .unwrap()
}

pub fn open_recording(binary: &str, root: &Path, target: &str) -> std::process::Output {
    tools(binary)
        .current_dir(root)
        .env("PATH", root)
        .env(RECORD, root.join("record"))
        .args(["open", target])
        .output()
        .unwrap()
}

/// A help program with ordinary and oversized output modes.
pub fn help_program(root: &Path) {
    compile(
        root,
        "stub",
        r#"fn main() {
        let args: Vec<_> = std::env::args().skip(1).collect();
        assert_eq!(args.last().map(String::as_str), Some("--help"));
        match args.first().map(String::as_str) {
            Some("large") => {
                // One byte beyond the documented 64 KiB text budget.
                const OVERSIZED_HELP_BYTES: usize = 64 * 1024 + 1;
                print!("{}", "x".repeat(OVERSIZED_HELP_BYTES));
            }
            _ => println!("stub help: {}", args.join("|")),
        }
    }"#,
    );
}

/// A path list of `entries` in this system's form, in order; an empty entry stays empty.
pub fn path_list<I: AsRef<std::ffi::OsStr>>(entries: impl IntoIterator<Item = I>) -> String {
    std::env::join_paths(entries)
        .unwrap()
        .into_string()
        .unwrap()
}

/// A copy of this test binary at `path`, runnable as that program.
pub fn copy_test_binary(path: &Path) {
    fs::copy(std::env::current_exe().unwrap(), path).unwrap();
    make_executable(path);
}

/// `program`, a copy of this test binary, set to run only the test `name` with exactly
/// `environment`; its output once it has succeeded.
pub fn rerun_as(
    program: &Path,
    name: &str,
    environment: &std::collections::BTreeMap<String, String>,
) -> std::process::Output {
    let output = Command::new(program)
        .args(["--exact", name, "--nocapture"])
        .env_clear()
        .envs(environment)
        .output()
        .unwrap();
    assert!(
        succeeded(&output.status),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Build a program named `name` in `directory` that writes its whole environment, one
/// `NAME=value` line each, to the file named by `RECORD`.
pub fn environment_recording_program(directory: &Path, name: &str) -> PathBuf {
    compile(
        directory,
        name,
        r#"use std::io::Write;
fn main() {
    let record = std::env::var_os("ARTIFACTIZE_OPENER_RECORD").expect("record path");
    let mut file = std::fs::File::create(record).unwrap();
    for (name, value) in std::env::vars() {
        writeln!(file, "{name}={value}").unwrap();
    }
}
"#,
    )
}

/// The environment an environment-recording program wrote to `record`.
pub fn recorded_environment(record: &Path) -> std::collections::BTreeMap<String, String> {
    let text = fs::read_to_string(record).unwrap();
    text.lines()
        .map(|line| {
            let (name, value) = line.split_once('=').unwrap();
            (name.to_owned(), value.to_owned())
        })
        .collect()
}

/// The code `output`'s process exited with; a process a signal ended has none and fails here.
pub fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .unwrap_or_else(|| panic!("the process ended without an exit code: {}", output.status))
}

// The environment of an AppImage, which exists only on Linux: the person's own, and what the
// AppImage runtime, its GTK hook and AppRun make of it.

/// The person's environment before the AppImage started: their opener's directory first.
pub fn appimage_person(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("PATH".into(), path_list([root.join("bin")])),
        (
            "XDG_DATA_DIRS".into(),
            path_list([
                "/usr/local/share/",
                "/usr/share/",
                "/var/lib/flatpak/exports/share",
            ]),
        ),
        ("AUTHOR_VARIABLE".into(), "from-the-terminal".into()),
        (RECORD.into(), text(&root.join("record"))),
    ])
}

/// What the AppImage runtime, the linuxdeploy GTK hook and AppRun make of `person`, for the
/// bundle `bundle`, in the forms they write it: with doubled slashes, and with an empty last
/// entry where the person had no value.
pub fn appimage_bundled(
    bundle: &Path,
    person: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
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
                path_list([
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
                path_list([format!("{b}/usr/lib/"), format!("{b}/lib/"), String::new()]),
            ),
            (
                "PERLLIB",
                path_list([
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

/// The path list `list` with `entries` before its own.
fn prepended<const N: usize>(entries: [String; N], list: &str) -> String {
    path_list(
        entries
            .into_iter()
            .map(PathBuf::from)
            .chain(std::env::split_paths(list)),
    )
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}
