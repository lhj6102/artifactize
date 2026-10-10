//! Operating-system fixtures for the unit and integration tests alike: the Unix utilities
//! the tests name, executable scripts, links and junctions, extra file permissions, and
//! process control. Unix keeps the real utilities and signals; Windows gets the stand-ins
//! described with each helper.
#![allow(dead_code, reason = "each test binary uses a different part")]

use std::{path::Path, process::Command};

/// A temporary fixture below the physical OS temp root, not macOS /var or Windows 8.3
/// aliases. Scoped file reads intentionally refuse symlink traversal.
pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
}

/// The program a test names a Unix utility by, such as `/bin/sh` or `cat`: unchanged on
/// Unix. On Windows, `fixture.rs` built once and linked under the utility's file name, a
/// stand-in for the shell and the utilities the test scripts use. Setting
/// `ARTIFACTIZE_TEST_STAND_INS` runs the stand-ins on Unix too, to check them there.
pub fn bin(path: &str) -> String {
    if !stand_ins() {
        // BSD installs these utilities in /usr/bin, not Linuxs /bin.
        #[cfg(target_os = "macos")]
        if matches!(path, "/bin/true" | "/bin/false") {
            return format!("/usr{path}");
        }
        return path.to_owned();
    }
    let name = path.rsplit('/').next().unwrap();
    fixture::directory()
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
        .to_str()
        .unwrap()
        .to_owned()
}

/// Whether tests run the stand-ins `bin` names.
pub fn stand_ins() -> bool {
    cfg!(windows) || std::env::var_os("ARTIFACTIZE_TEST_STAND_INS").is_some()
}

/// A deadline for a review that should finish, not time out: unchanged on Unix, ten times as
/// long on Windows, where each process start costs more and a busy runner starts dozens at
/// once.
pub fn slow(milliseconds: u64) -> u64 {
    if cfg!(windows) {
        milliseconds * 10
    } else {
        milliseconds
    }
}

/// How long a test waits for something that should happen, scaled like `slow`. Only a
/// failing test waits that long.
pub fn patience(duration: std::time::Duration) -> std::time::Duration {
    std::time::Duration::from_millis(slow(duration.as_millis() as u64))
}

/// `PATH` for a spawned artifactize. With the stand-ins they come first, so that bare names
/// such as `cat` and `true` resolve to them as they do to the utilities on Unix.
pub fn path() -> std::ffi::OsString {
    let path = std::env::var_os("PATH").unwrap_or_default();
    if !stand_ins() {
        return path;
    }
    let mut paths = vec![fixture::directory().to_owned()];
    paths.extend(std::env::split_paths(&path));
    std::env::join_paths(paths).unwrap()
}

/// Make the script at `path` runnable by its path, as a test's tool or hook is: mode 0700 on
/// Unix. Windows has no `#!`, so the fixture is copied to `<path>.exe`, which `Command`
/// runs for an extensionless path and which runs `<path>` as a shell script.
pub fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    {
        let program = fixture::directory().join("sh.exe");
        std::fs::copy(program, path.with_extension("exe")).unwrap();
    }
}

/// Give a script its execute bits (0755) on Unix, for a declaration that runs it by path
/// through its `#!` line. Windows has no execute bit and runs a script by its extension.
pub fn allow_execution(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(windows)]
    let _ = path;
}

/// Take a file's execute bits away (0600). Only Unix has them.
#[cfg(unix)]
pub fn deny_execution(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// The program a Unix desktop opens files with: `open` on macOS, `xdg-open` elsewhere.
/// Windows opens through ShellExecute and runs no program by name.
#[cfg(unix)]
pub const OPENER: &str = if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

/// The environment variable naming the file a recording program appends to.
pub const RECORD: &str = "ARTIFACTIZE_OPENER_RECORD";

/// Build a program named `name` in `directory` that appends its argument count and first
/// argument, one per line, to the file named by `RECORD`. A compiled program needs no shell.
pub fn recording_program(directory: &Path, name: &str) -> std::path::PathBuf {
    let source = directory.join(format!("{name}-recorder.rs"));
    std::fs::write(
        &source,
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
    .unwrap();
    let program = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = Command::new(rustc)
        .args(["--edition", "2024", "-o"])
        .arg(&program)
        .arg(&source)
        .output()
        .expect("rustc builds the recording program");
    assert!(
        output.status.success(),
        "building the recording program failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    program
}

/// Set a Unix permission mode, for checks of modes artifactize refuses or must tolerate.
#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A Unix permission mode, without the file type bits.
#[cfg(unix)]
pub fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    path.metadata().unwrap().permissions().mode() & 0o7777
}

/// Let everyone list and enter a directory: mode 0755 on Unix, and on Windows an extra
/// Everyone read ACE. Either way it is no longer private.
pub fn share_dir(path: &Path) {
    #[cfg(unix)]
    set_mode(path, 0o755);
    #[cfg(windows)]
    grant_everyone_read(path);
}

/// The effective user ID of this process.
#[cfg(unix)]
pub fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and does not change process state.
    unsafe { libc::geteuid() }
}

/// The null device: `/dev/null` on Unix, `NUL` on Windows.
pub fn null_device() -> &'static Path {
    Path::new(if cfg!(windows) { "NUL" } else { "/dev/null" })
}

/// A FIFO, an entry that is neither a file nor a directory. Windows has none.
#[cfg(unix)]
pub fn fifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: a live NUL-terminated path; mkfifo retains no pointer and 0600 is a valid mode.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

/// A bound Unix socket at `path`, another special entry; it stays bound while held.
#[cfg(unix)]
pub fn socket(path: &Path) -> std::os::unix::net::UnixListener {
    std::os::unix::net::UnixListener::bind(path).unwrap()
}

/// The error opening a path below a regular file reports on Unix.
#[cfg(unix)]
pub fn not_a_directory() -> std::io::Error {
    std::io::Error::from_raw_os_error(libc::ENOTDIR)
}

/// The signal number of SIGTERM.
#[cfg(unix)]
pub const SIGTERM: i32 = libc::SIGTERM;

/// End a process and everything in its tree at once: SIGKILL to its process group on Unix;
/// on Windows the process, as its Job Object holds the rest.
pub fn kill_tree(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: a plain signal to the group of a child this test started; one that has
        // already gone is fine.
        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    }
    #[cfg(windows)]
    kill(pid);
}

/// Whether a Unix process leads its own process group.
#[cfg(unix)]
pub fn leads_group(pid: u32) -> bool {
    // SAFETY: getpgid only queries a process.
    unsafe { libc::getpgid(pid as i32) == pid as i32 }
}

/// Whether a Unix process leads its own session, detached from the caller's terminal.
#[cfg(unix)]
pub fn leads_session(pid: u32) -> bool {
    // SAFETY: getsid only queries a process.
    unsafe { libc::getsid(pid as i32) == pid as i32 }
}

/// A pseudo-terminal pair from openpty(3): the master to drive and the slave to hand a child.
#[cfg(unix)]
pub fn pty() -> (std::fs::File, std::os::fd::OwnedFd) {
    use std::os::fd::{FromRawFd, OwnedFd};
    let (mut master, mut slave) = (0, 0);
    // SAFETY: openpty writes two new descriptors, owned below.
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0);
    // SAFETY: both descriptors were just opened and are owned exactly once.
    unsafe {
        (
            std::fs::File::from_raw_fd(master),
            OwnedFd::from_raw_fd(slave),
        )
    }
}

/// A pipe named like the one Git Bash's mintty hands a program: the writer and the reader,
/// which `is_terminal` takes for a terminal although it has no console.
#[cfg(windows)]
pub fn mintty_pipe() -> (std::fs::File, std::fs::File) {
    use std::{os::windows::io::FromRawHandle, ptr};
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, OPEN_EXISTING, PIPE_ACCESS_OUTBOUND},
        System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT},
    };
    let name: Vec<u16> = format!(r"\\.\pipe\msys-{}-pty0-from-master", std::process::id())
        .encode_utf16()
        .chain([0])
        .collect();
    // SAFETY: a NUL-terminated name; each handle is checked, then owned by one File.
    unsafe {
        let server_end = CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_OUTBOUND,
            PIPE_TYPE_BYTE | PIPE_WAIT,
            1,
            4096,
            4096,
            0,
            ptr::null(),
        );
        assert_ne!(server_end, INVALID_HANDLE_VALUE);
        let client_end = CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            0,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        );
        assert_ne!(client_end, INVALID_HANDLE_VALUE);
        (
            std::fs::File::from_raw_handle(server_end),
            std::fs::File::from_raw_handle(client_end),
        )
    }
}

mod fixture {
    use std::{
        env::consts::EXE_SUFFIX,
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::OnceLock,
    };

    use sha2::{Digest, Sha256};

    /// Every name the fixture answers to.
    const UTILITIES: &[&str] = &[
        "sh", "cat", "cksum", "echo", "env", "false", "grep", "head", "ls", "mkdir", "printf",
        "pwd", "rm", "sleep", "test", "touch", "tr", "true",
    ];

    /// The directory of fixture programs, built at most once per source: integration tests
    /// share Cargo's temporary directory and unit tests the system's.
    pub(super) fn directory() -> &'static Path {
        static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
        DIRECTORY.get_or_init(|| {
            let source = include_str!("fixture.rs");
            let digest = Sha256::digest(source.as_bytes());
            let tag: String = digest[..8]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            let root =
                option_env!("CARGO_TARGET_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
            let directory = root.join(format!("artifactize-fixture-{tag}"));
            let shell = format!("sh{EXE_SUFFIX}");
            if directory.join(&shell).is_file() {
                return directory;
            }
            fs::create_dir_all(&root).unwrap();
            let staging = tempfile::tempdir_in(&root).unwrap();
            let program = staging.path().join(format!("fixture{EXE_SUFFIX}"));
            let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
            let output = Command::new(rustc)
                .args(["--edition", "2024", "--crate-name", "fixture", "-o"])
                .arg(&program)
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/support/fixture.rs"
                ))
                .output()
                .expect("rustc builds the test fixture");
            assert!(
                output.status.success(),
                "building the test fixture failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            for name in UTILITIES {
                let link = staging.path().join(format!("{name}{EXE_SUFFIX}"));
                fs::hard_link(&program, link).unwrap();
            }
            // Another test process may have finished first; its copy is the same program.
            let staged = staging.keep();
            if fs::rename(&staged, &directory).is_err() {
                let _ = fs::remove_dir_all(&staged);
                assert!(
                    directory.join(&shell).is_file(),
                    "fixture directory missing"
                );
            }
            directory
        })
    }
}

/// A link to a directory that needs no privilege: a symlink on Unix, and on Windows a
/// junction, which artifactize refuses and resolves just as it does a directory symlink.
pub fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    symlink_dir(target, link).unwrap();
    #[cfg(windows)]
    junction(target, link);
}

/// Remove a link `link_dir` made, without touching its target: Windows removes a directory
/// link as a directory.
pub fn remove_link_dir(link: &Path) {
    #[cfg(unix)]
    std::fs::remove_file(link).unwrap();
    #[cfg(windows)]
    std::fs::remove_dir(link).unwrap();
}

/// Whether a process has exited for good: on Unix also reaped, so no zombie entry remains;
/// on Windows no longer running.
pub fn gone(pid: u32) -> bool {
    #[cfg(unix)]
    {
        !exists(pid)
    }
    #[cfg(windows)]
    {
        !running(pid)
    }
}

/// Ask a process to terminate: SIGTERM on Unix, and on Windows Ctrl-Break to the group
/// `new_group` gave it, the closest request it has.
pub fn terminate(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: a plain signal to a child this test spawned.
        assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGTERM) }, 0);
    }
    #[cfg(windows)]
    interrupt(pid);
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
        privileged(std::os::windows::fs::symlink_file(target, link))
    }
}

/// A symbolic link to a directory; see `symlink_file`.
pub fn symlink_dir(target: impl AsRef<Path>, link: impl AsRef<Path>) -> Option<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).unwrap();
        Some(())
    }
    #[cfg(windows)]
    {
        privileged(std::os::windows::fs::symlink_dir(target, link))
    }
}

#[cfg(windows)]
fn privileged(created: std::io::Result<()>) -> Option<()> {
    // ERROR_PRIVILEGE_NOT_HELD
    match created {
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

/// A junction (mount point) to an existing directory, which needs no privilege.
#[cfg(windows)]
pub fn junction(target: &Path, link: &Path) {
    // mklink reads a `/` as the start of a switch, and wants plain absolute paths.
    let native = |path: &Path| {
        let path = std::path::absolute(path).unwrap();
        let path = path.to_str().unwrap().replace('/', r"\");
        path.strip_prefix(r"\\?\")
            .map_or(path.clone(), str::to_owned)
    };
    let output = Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(native(link))
        .arg(native(target))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

/// Also let everyone read `path`: an extra allow ACE on Windows, mode 0644 on Unix.
pub fn grant_everyone_read(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    #[cfg(windows)]
    {
        // S-1-1-0 is Everyone, whatever the system language.
        let output = Command::new("icacls")
            .arg(path)
            .args(["/grant", "*S-1-1-0:(R)"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

/// Start `command` where `interrupt` can reach it alone: on Windows, its own console process
/// group, which Ctrl-Break addresses; Unix signals address one process already.
pub fn new_group(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
    }
    command
}

/// Ask a process to stop as Ctrl-C would: SIGINT on Unix, and on Windows Ctrl-Break to the
/// group `new_group` gave it, the event artifactize treats the same way.
pub fn interrupt(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: a plain signal to a child this test spawned.
        assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGINT) }, 0);
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
        // SAFETY: a nonzero group id reaches only that process group.
        let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) };
        assert_ne!(sent, 0, "Ctrl-Break: {}", std::io::Error::last_os_error());
    }
}

/// Freeze a process: SIGSTOP on Unix, and on Windows every thread suspended.
pub fn suspend(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: a plain signal to a process this test started.
        assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGSTOP) }, 0);
    }
    #[cfg(windows)]
    nt_process(pid, true);
}

/// Let a process frozen by `suspend` run again: SIGCONT on Unix.
pub fn resume(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: a plain signal to a process this test started.
        assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGCONT) }, 0);
    }
    #[cfg(windows)]
    nt_process(pid, false);
}

#[cfg(windows)]
fn nt_process(pid: u32, suspend: bool) {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME},
    };
    #[link(name = "ntdll", kind = "raw-dylib")]
    unsafe extern "system" {
        fn NtSuspendProcess(process: HANDLE) -> i32;
        fn NtResumeProcess(process: HANDLE) -> i32;
    }
    // SAFETY: the handle is checked, used for one call, and closed.
    unsafe {
        let process = OpenProcess(PROCESS_SUSPEND_RESUME, 0, pid);
        assert!(!process.is_null(), "{}", std::io::Error::last_os_error());
        let status = if suspend {
            NtSuspendProcess(process)
        } else {
            NtResumeProcess(process)
        };
        CloseHandle(process);
        assert!(status >= 0, "NTSTATUS {status:#x}");
    }
}

/// End a process at once, as SIGKILL does; one that has already gone is fine.
#[cfg(windows)]
pub fn kill(pid: u32) {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
    };
    // SAFETY: the handle is checked, used once, and closed.
    unsafe {
        let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !process.is_null() {
            TerminateProcess(process, 1);
            CloseHandle(process);
        }
    }
}

/// Whether a process has not exited. A PID that no longer names a process, or names one
/// this user may not open, counts as exited.
#[cfg(windows)]
pub fn running(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
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

/// A process entry still present, including a zombie waiting for its parent to reap it.
#[cfg(unix)]
pub fn exists(pid: u32) -> bool {
    #[cfg(not(target_os = "macos"))]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(target_os = "macos")]
    {
        bsd_info(pid).is_some()
    }
}

/// Whether a Unix process has not exited; zombies are present but no longer running.
#[cfg(unix)]
pub fn running(pid: u32) -> bool {
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            !stat
                .rsplit_once(')')
                .unwrap()
                .1
                .trim_start()
                .starts_with('Z')
        })
    }
    #[cfg(target_os = "macos")]
    {
        bsd_info(pid).is_some_and(|info| info.pbi_status != libc::SZOMB)
    }
}

#[cfg(target_os = "macos")]
fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: the writable output buffer has the exact size required by PROC_PIDTBSDINFO.
    let read = unsafe {
        libc::proc_pidinfo(
            i32::try_from(pid).ok()?,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    // SAFETY: only a complete successful result is read.
    (read == size).then(|| unsafe { info.assume_init() })
}

/// script(1)'s equivalent PTY invocation on util-linux and BSD/macOS.
#[cfg(unix)]
pub fn pty_command(command: &str) -> Command {
    let mut pty = Command::new("script");
    #[cfg(target_os = "macos")]
    pty.args(["-q", "/dev/null", "/bin/sh", "-c", command]);
    #[cfg(not(target_os = "macos"))]
    pty.args(["-qec", command, "/dev/null"]);
    pty
}

/// A process's start time as artifactize records it: field 22 of `/proc/PID/stat` on Linux,
/// microseconds since the epoch on macOS, and the creation time in 100 ns units since 1601 on Windows.
pub fn start_time(pid: u32) -> u64 {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        stat.rsplit_once(')')
            .unwrap()
            .1
            .split_whitespace()
            .nth(19)
            .unwrap()
            .parse()
            .unwrap()
    }
    #[cfg(target_os = "macos")]
    {
        let info = bsd_info(pid).expect("process information");
        info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, FILETIME},
            System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        };
        // SAFETY: the handle is checked, queried once, and closed.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            assert!(!process.is_null(), "{}", std::io::Error::last_os_error());
            let mut creation = FILETIME::default();
            let (mut exit, mut kernel, mut user) = (creation, creation, creation);
            let queried =
                GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user);
            CloseHandle(process);
            assert_ne!(queried, 0);
            (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime)
        }
    }
}

/// `path` with the ASCII letters of its last component in the other case: on case-insensitive volumes
/// the same file or folder, otherwise another name.
pub fn other_case(path: &Path) -> std::path::PathBuf {
    let name: String = path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect();
    path.with_file_name(name)
}

/// The path artifactize reports for an existing `path`: canonical, and on Windows without
/// the `\\?\` prefix, as long as the test paths stay short and plain.
pub fn canonical(path: &Path) -> std::path::PathBuf {
    let path = std::fs::canonicalize(path).unwrap();
    #[cfg(windows)]
    {
        let text = path.to_str().unwrap();
        if let Some(plain) = text.strip_prefix(r"\\?\")
            && !plain.starts_with("UNC\\")
        {
            return plain.into();
        }
    }
    path
}

/// A directory only the current user may use, with its missing parents: mode 0700 on Unix,
/// and on Windows a protected DACL granting the user alone, which new files inherit.
pub fn create_private_dir_all(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .unwrap();
    }
    #[cfg(windows)]
    {
        std::fs::create_dir_all(path).unwrap();
        acl::restrict(path);
    }
}

/// A new file only the current user may use: mode 0600 on Unix; on Windows it inherits a
/// private directory's DACL, as artifactize's own files do.
pub fn write_private_file(path: &Path, contents: impl AsRef<[u8]>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Whether only the current user may use a directory: mode 0700 on Unix; on Windows every
/// ACE that grants access names the current user.
pub fn private_dir(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().unwrap().permissions().mode() & 0o777 == 0o700
    }
    #[cfg(windows)]
    {
        acl::owner_only(path)
    }
}

/// Whether only the current user may use a file: mode 0600 on Unix; see `private_dir`.
pub fn private_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().unwrap().permissions().mode() & 0o777 == 0o600
    }
    #[cfg(windows)]
    {
        acl::owner_only(path)
    }
}

#[cfg(windows)]
mod acl {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_SUCCESS, LocalFree},
        Security::{
            ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
            },
            DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
            GetSecurityDescriptorDacl, GetTokenInformation, INHERIT_ONLY_ACE,
            PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::{
            SystemServices::ACCESS_ALLOWED_ACE_TYPE,
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain([0]).collect()
    }

    /// This process's TOKEN_USER, aligned for the SID inside it.
    fn user() -> Vec<u64> {
        // SAFETY: the token handle is checked and closed; the buffer is sized by the first
        // call and aligned for TOKEN_USER.
        unsafe {
            let mut token = ptr::null_mut();
            assert_ne!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token),
                0
            );
            let mut length = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length);
            let mut buffer = vec![0_u64; (length as usize).div_ceil(8)];
            let read = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length,
            );
            CloseHandle(token);
            assert_ne!(read, 0);
            buffer
        }
    }

    /// Replace the DACL with a protected one granting the current user full control.
    pub(super) fn restrict(path: &Path) {
        let user = user();
        // SAFETY: the SID comes from TOKEN_USER; LocalAlloc'd results are freed once, and
        // the DACL points into the descriptor, which outlives its use.
        unsafe {
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let mut text = ptr::null_mut();
            assert_ne!(ConvertSidToStringSidW(sid, &mut text), 0);
            let length = (0..).take_while(|&index| *text.add(index) != 0).count();
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})")
                .encode_utf16()
                .chain([0])
                .collect();
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    ptr::null_mut(),
                ),
                0
            );
            let (mut present, mut defaulted) = (0, 0);
            let mut dacl: *mut ACL = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted),
                0
            );
            let set = SetNamedSecurityInfoW(
                wide(path).as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null(),
            );
            LocalFree(descriptor);
            assert_eq!(set, ERROR_SUCCESS);
        }
    }

    /// Whether every ACE that grants access to `path` names the current user.
    pub(super) fn owner_only(path: &Path) -> bool {
        let user = user();
        // SAFETY: GetNamedSecurityInfoW allocates the descriptor that `dacl` points into;
        // it is freed after the last read. ACE indexes stay below the ACL's count.
        unsafe {
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let mut dacl: *mut ACL = ptr::null_mut();
            let mut descriptor = ptr::null_mut();
            let read = GetNamedSecurityInfoW(
                wide(path).as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            );
            assert_eq!(read, ERROR_SUCCESS);
            let mut size = ACL_SIZE_INFORMATION::default();
            let mut owner_only = !dacl.is_null()
                && GetAclInformation(
                    dacl,
                    (&raw mut size).cast(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                ) != 0;
            let count = if owner_only { size.AceCount } else { 0 };
            for index in 0..count {
                let mut ace: *mut c_void = ptr::null_mut();
                assert_ne!(GetAce(dacl, index, &mut ace), 0);
                let header = ace.cast::<ACE_HEADER>().read_unaligned();
                if u32::from(header.AceFlags) & INHERIT_ONLY_ACE == 0
                    && u32::from(header.AceType) == ACCESS_ALLOWED_ACE_TYPE
                {
                    let ace_sid = &raw mut (*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart;
                    owner_only &= EqualSid(ace_sid.cast(), sid) != 0;
                }
            }
            LocalFree(descriptor);
            owner_only
        }
    }
}
