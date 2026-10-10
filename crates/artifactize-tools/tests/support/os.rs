//! Operating-system fixtures for the unit and integration tests alike: unique temporary
//! roots, links, executable files and a recording stand-in for the desktop opener.
#![allow(dead_code, reason = "each test binary uses a different part")]

use std::{
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

/// Whether a process has ended, waiting for it a generous while: an ended process
/// disappears asynchronously, so this polls, and only a failing test waits the whole time.
pub fn ends(pid: u32) -> bool {
    /// Long enough for a loaded CI runner to reap a killed process.
    const PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);
    /// How often to look again.
    const POLL: std::time::Duration = std::time::Duration::from_millis(20);
    let deadline = std::time::Instant::now() + PATIENCE;
    while running(pid) {
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
    true
}

/// Whether a process still exists and has not exited.
fn running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 only asks whether the process exists.
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, STILL_ACTIVE},
            System::Threading::{
                GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        };
        // SAFETY: the handle is checked, queried once, and closed.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut code = 0;
            let queried = GetExitCodeProcess(process, &mut code);
            CloseHandle(process);
            queried != 0 && code == STILL_ACTIVE as u32
        }
    }
}

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
