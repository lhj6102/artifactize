//! Operating-system fixtures for the unit and integration tests alike: unique temporary
//! roots, links, executable files and a recording stand-in for the desktop opener.
#![expect(
    dead_code,
    reason = "each test binary uses a different subset of OS fixtures"
)]

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

pub fn case_policy(root: &Path) -> CasePolicy {
    let probe = tempfile::Builder::new()
        .prefix("case-Probe-")
        .tempfile_in(root)
        .unwrap();
    let name = probe.path().file_name().unwrap().to_str().unwrap();
    if root.join(name.to_ascii_uppercase()).exists() {
        CasePolicy::Insensitive
    } else {
        CasePolicy::Sensitive
    }
}

/// The command Cargo built, with native process setup kept in the OS fixture layer.
pub fn tools(binary: &str) -> Command {
    Command::new(binary)
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

/// A help program with ordinary, oversized, and never-finishing output modes.
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
            Some("wait") => loop { std::thread::park(); },
            _ => println!("stub help: {}", args.join("|")),
        }
    }"#,
    );
}

/// The child's TCP connection is the handshake: its established stream proves startup,
/// and EOF proves the child died. No polling, PID reuse, or scheduler delay is involved.
pub fn timeout_tree(binary: &str, root: &Path) -> std::process::Output {
    use std::io::Read;
    compile(
        root,
        "spawner",
        r#"fn main() {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("child") {
            let stream = std::net::TcpStream::connect(&args[2]).unwrap();
            // Keep the established connection alive until the process tree is killed.
            let _stream = stream;
            loop { std::thread::park(); }
        } else {
            // Spawn immediately, so the seam checks admission before the first instruction.
            std::process::Command::new(std::env::current_exe().unwrap())
                .arg("child").arg(&args[1]).spawn().unwrap();
            loop { std::thread::park(); }
        }
    }"#,
    );
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let mut command = tools(binary);
    let child = command
        .current_dir(root)
        .env("PATH", root)
        .args(["help", "spawner", &address])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let (connected, connection) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stream = listener.accept().map(|(stream, _)| stream);
        let _ = connected.send(stream);
    });
    let output = child.wait_with_output().unwrap();
    // A watchdog bounds broken startup or cleanup; success never depends on elapsed time.
    const CLEANUP_WATCHDOG: std::time::Duration = std::time::Duration::from_secs(20);
    let mut stream = connection
        .recv_timeout(CLEANUP_WATCHDOG)
        .expect("the help child completed its startup handshake")
        .unwrap();
    stream.set_read_timeout(Some(CLEANUP_WATCHDOG)).unwrap();
    // The child's end closes when it ends: an orderly close on Unix, a reset on Windows,
    // which tears down the connections of a terminated process.
    match stream.read(&mut [0]) {
        Ok(read) => assert_eq!(read, 0, "the help child outlived the timeout"),
        Err(error) => assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ),
            "the help child outlived the timeout: {error}"
        ),
    }
    output
}
