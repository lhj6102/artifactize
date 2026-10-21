//! Operating-system fixtures for the unit and integration tests alike: the Unix utilities
//! the tests name, executable scripts, links and junctions, extra file permissions, and
//! process control. Unix keeps the real utilities and signals; Windows gets the stand-ins
//! described with each helper.
#![expect(dead_code, reason = "each test binary uses a different part")]

#[path = "os/checkpoints.rs"]
mod checkpoints;
#[path = "os/creation.rs"]
mod creation;
#[path = "os/files.rs"]
mod files;

/// Reject new files in a fixture directory, restoring its permissions before teardown.
pub fn deny_creates(directory: &Path) -> creation::CreationDenied {
    creation::deny_creates(directory)
}

/// Choose a fixture directory spelling supported by its volume.
pub fn case_alias_or_original(directory: &Path, alternate_name: &str) -> std::path::PathBuf {
    files::case_alias_or_original(directory, alternate_name)
}

/// Observe released SQLite handles without mutating the fixture's database or names.
pub fn database_released(path: &std::path::Path) -> std::io::Result<bool> {
    files::database_released(path)
}
#[path = "os/observation.rs"]
mod observation;
#[path = "os/tcp.rs"]
mod tcp;

/// Observe closed TCP after the caller has completed its owning operation.
/// Every application response byte is retained, including bytes before a reset.
pub async fn closed_tcp_probe(
    stream: &mut tokio::io::BufReader<tokio::net::TcpStream>,
    probe: &[u8],
) -> std::io::Result<Vec<u8>> {
    tcp::closed_probe(stream, probe).await
}
#[path = "os/worker.rs"]
mod worker;

use serde_json::{Value, json};
use std::fs;
use std::{path::Path, process::Command};

pub type OwnedExecutable = worker::Executable;

#[cfg(unix)]
pub type BeforeIdentity = checkpoints::BeforeIdentity;

#[cfg(windows)]
pub fn process_in_job(
    child: &tokio::process::Child,
    job: &std::os::windows::io::OwnedHandle,
) -> bool {
    checkpoints::process_in_job(child, job)
}

/// The test process owns an immutable copy of Cargo's declared CLI until its workers end.
/// Cargo may replace its published binary during concurrent library fixture builds.
pub fn cli() -> std::path::PathBuf {
    assert!(
        option_env!("CARGO_BIN_EXE_artifactize").is_some(),
        "tests that start the CLI live in the artifactize package"
    );
    worker::executable()
        .expect("the test process owns its declared CLI executable")
        .to_owned()
}

/// A semantic snapshot, without struct padding, of the input terminal's current modes.
/// Compare flags, control characters and speeds, not just whether echo is enabled.
#[derive(Debug, PartialEq, Eq)]
pub struct InputTerminalMode {
    #[cfg(unix)]
    flags: [libc::tcflag_t; 4],
    #[cfg(unix)]
    controls: [libc::cc_t; libc::NCCS],
    #[cfg(unix)]
    speeds: [libc::speed_t; 2],
    #[cfg(windows)]
    mode: u32,
}

pub fn input_terminal_mode() -> InputTerminalMode {
    #[cfg(unix)]
    {
        let mut mode = std::mem::MaybeUninit::uninit();
        // SAFETY: tcgetattr fills the valid out pointer; it is read only on success.
        assert_eq!(
            unsafe { libc::tcgetattr(libc::STDIN_FILENO, mode.as_mut_ptr()) },
            0
        );
        // SAFETY: the preceding call initialized the termios structure.
        let mode = unsafe { mode.assume_init() };
        InputTerminalMode {
            flags: [mode.c_iflag, mode.c_oflag, mode.c_cflag, mode.c_lflag],
            controls: mode.c_cc,
            // SAFETY: these functions only inspect the initialized termios structure.
            speeds: unsafe { [libc::cfgetispeed(&mode), libc::cfgetospeed(&mode)] },
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE};
        // SAFETY: GetStdHandle reads the table; GetConsoleMode writes to a valid DWORD.
        let mut mode = 0;
        assert_ne!(
            unsafe { GetConsoleMode(GetStdHandle(STD_INPUT_HANDLE), &mut mode) },
            0
        );
        InputTerminalMode { mode }
    }
}

/// Build the sibling standalone tools CLI once, including when testing only this crate.
pub fn tools_binary(artifactize: &Path) -> &'static Path {
    static BINARY: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    BINARY.get_or_init(|| {
        // cli() now returns a private candidate; retain the declared Cargo target/profile
        // for the sibling tools build instead of building below that temporary directory.
        let declared = option_env!("CARGO_BIN_EXE_artifactize")
            .map(Path::new)
            .unwrap_or(artifactize);
        let directory = declared.parent().unwrap();
        let mut command = Command::new(env!("CARGO"));
        command
            .args([
                "build",
                "--quiet",
                "--locked",
                "--bin",
                "artifactize-tools",
                "--message-format=json-render-diagnostics",
            ])
            .arg("--manifest-path")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../artifactize-tools/Cargo.toml"))
            .arg("--target-dir")
            .arg(directory.parent().unwrap());
        if directory.file_name().unwrap() == "release" {
            command.arg("--release");
        }
        let built = command
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .output()
            .expect("Cargo builds the standalone tools fixture");
        assert!(
            built.status.success(),
            "tools fixture build: {}",
            built.status
        );
        worker::executable_from_receipt(&built.stdout, "artifactize-tools")
            .expect("Cargo reports the standalone tools executable")
    })
}

/// Whether an open SQLite file can be renamed or removed without closing its connection.
/// Unix unlinks names independently of open handles; Windows SQLite denies delete sharing.
pub fn can_replace_open_database() -> bool {
    cfg!(unix)
}

/// A temporary fixture below the physical OS temp root, not macOS /var or Windows 8.3
/// aliases. Scoped file reads intentionally refuse symlink traversal.
pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir_in(temp_root()).unwrap()
}

/// Keep a file in Windows' pending-delete disposition until the returned handle closes.
/// A delete-on-close handle establishes the disposition when it closes, while a second
/// read/delete-sharing handle keeps the name present but inaccessible to new opens.
/// `remove_file` alone instead unlinks by POSIX semantics on current Windows.
#[cfg(windows)]
pub fn pending_delete(path: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;

    // Both opens must coexist for read and allow the delete-on-close disposition.
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_DELETE_ON_CLOSE: u32 = 0x0400_0000;
    let deleter = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(path)
        .unwrap();
    let holder = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .open(path)
        .unwrap();
    drop(deleter);
    holder
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

/// The POSIX shell, resolved through [`bin`] like any other named utility: a semantic call
/// for test code that just needs a shell, so the shell's own path lives only here.
pub fn shell() -> String {
    bin("/bin/sh")
}

/// A program that exits 0 and does nothing else, resolved through [`bin`].
pub fn true_program() -> String {
    bin("/bin/true")
}

/// A program that exits 1 and does nothing else, resolved through [`bin`].
pub fn false_program() -> String {
    bin("/bin/false")
}

/// A program that prints its arguments, resolved through [`bin`].
pub fn echo_program() -> String {
    bin("/bin/echo")
}

/// A program that prints a file's contents, resolved through [`bin`].
pub fn cat_program() -> String {
    bin("/bin/cat")
}

/// A program that waits for the given duration and exits 0, resolved through [`bin`].
pub fn sleep_program() -> String {
    bin("/bin/sleep")
}

/// A program that runs another program with an adjusted environment, resolved through
/// [`bin`].
pub fn env_program() -> String {
    bin("/usr/bin/env")
}

/// A program that formats and prints its arguments, resolved through [`bin`].
pub fn printf_program() -> String {
    bin("/usr/bin/printf")
}

/// A program that creates a file or updates its modified time, resolved through [`bin`].
pub fn touch_program() -> String {
    bin("/usr/bin/touch")
}

/// A Windows drive-letter absolute path naming `suffix`, rejected by the portable
/// declaration path grammar on every platform: a sample value, never a real path this
/// test opens.
pub fn windows_drive(suffix: &str) -> String {
    format!("C:/{suffix}")
}

/// The same drive-letter absolute path, with native Windows separators.
pub fn windows_drive_native(suffix: &str) -> String {
    format!("C:\\{suffix}")
}

/// A well-known Windows system file path, rejected the same way as any other escape
/// attempt: a sample value, never a real path this test opens.
pub fn windows_system_file() -> &'static str {
    "C:/Windows/win.ini"
}

/// A well-known Unix system file path, rejected the same way as any other escape
/// attempt: a sample value, never a real path this test opens.
pub fn unix_system_file() -> &'static str {
    "/etc/passwd"
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

/// Poll `condition` until it holds, or panic after `patience(limit)`: the shared "wait for
/// an external process or file to reach a state" loop, scaled for a slower CI host rather
/// than checking once or sleeping a fixed real-time span.
pub fn wait_until(limit: std::time::Duration, mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + patience(limit);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "condition was not reached"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
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

/// The compiler of Cargo's selected toolchain, unless the runner explicitly supplies one.
/// Isolated environment probes remove Rust from PATH and replace HOME, so do not ask a
/// rustup shim to rediscover the toolchain from that probe's environment.
fn fixture_compiler() -> std::ffi::OsString {
    std::env::var_os("RUSTC").unwrap_or_else(|| {
        Path::new(env!("CARGO"))
            .with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX))
            .into_os_string()
    })
}

/// Build a program named `name` in `directory` from Rust `source`. A compiled program runs the
/// same on every system and needs no shell.
fn compiled_program(directory: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let file = directory.join(format!("{name}-source.rs"));
    std::fs::write(&file, source).unwrap();
    let program = directory.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let rustc = fixture_compiler();
    let output = Command::new(rustc)
        .args(["--edition", "2024", "-o"])
        .arg(&program)
        .arg(&file)
        .output()
        .expect("rustc builds the test program");
    assert!(
        output.status.success(),
        "building the test program {name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    program
}

/// An executable-resolution fixture that reports its cwd as portable path text.
pub fn working_directory_program(directory: &Path, name: &str) -> std::path::PathBuf {
    compiled_program(
        directory,
        name,
        r#"fn main() {
    let directory = std::env::current_dir().unwrap();
    let text = directory.to_str().unwrap();
    #[cfg(windows)]
    let text = text.replace('\\', "/");
    print!("{text}");
}
"#,
    )
}

/// Build a program named `name` in `directory` that appends its argument count and first
/// argument, one per line, to the file named by `RECORD`.
pub fn recording_program(directory: &Path, name: &str) -> std::path::PathBuf {
    compiled_program(
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

/// A program in `directory` that prints `output` and exits with `code`.
pub fn exiting_program(directory: &Path, code: i32, output: &str) -> String {
    path_text(&compiled_program(
        directory,
        "exiting",
        &format!("fn main() {{ print!({output:?}); std::process::exit({code}); }}"),
    ))
}

/// A program in `directory` that waits until the file named by its first argument exists, then
/// exits 0.
pub fn waiting_program(directory: &Path) -> String {
    path_text(&compiled_program(
        directory,
        "waiting",
        r#"fn main() {
    let file = std::env::args().nth(1).expect("the file to wait for");
    while !std::path::Path::new(&file).exists() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
"#,
    ))
}

/// A program in `directory` that runs the steps its arguments spell, in order, in its working
/// directory, and then exits 0: `pid FILE` appends its process id as a line, `append FILE TEXT`
/// appends a line, `touch FILE` creates a file, `wait FILE` waits until a file exists,
/// `print TEXT` prints, `equals A B` exits 1 unless A is B, `exit CODE` exits, and `from FILE`
/// runs the steps FILE holds, one argument per line, followed by the rest.
pub fn steps_program(directory: &Path) -> String {
    path_text(&compiled_program(
        directory,
        "steps",
        r#"use std::io::Write;
fn line(path: &str, text: &str) {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    writeln!(file, "{text}").unwrap();
}
fn next(steps: &mut std::vec::IntoIter<String>) -> String {
    steps.next().expect("the step's argument")
}
fn run(steps: Vec<String>) {
    let mut steps = steps.into_iter();
    while let Some(step) = steps.next() {
        match step.as_str() {
            "pid" => line(&next(&mut steps), &std::process::id().to_string()),
            "append" => {
                let path = next(&mut steps);
                line(&path, &next(&mut steps));
            }
            "touch" => {
                std::fs::OpenOptions::new().create(true).append(true).open(next(&mut steps)).unwrap();
            }
            "wait" => {
                let path = next(&mut steps);
                while !std::path::Path::new(&path).exists() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            "print" => {
                print!("{}", next(&mut steps));
                std::io::stdout().flush().unwrap();
            }
            "equals" => {
                if next(&mut steps) != next(&mut steps) {
                    std::process::exit(1);
                }
            }
            "exit" => std::process::exit(next(&mut steps).parse().unwrap()),
            "from" => {
                let file = std::fs::read_to_string(next(&mut steps)).unwrap();
                let mut steps: Vec<String> = file.lines().map(str::to_owned).chain(steps).collect();
                steps.retain(|step| !step.is_empty());
                return run(steps);
            }
            other => panic!("unknown step {other}"),
        }
    }
}
fn main() {
    run(std::env::args().skip(1).collect());
}
"#,
    ))
}

/// A program in `directory` that exits 1 while the file named by its first argument exists, and
/// 0 once it is gone.
pub fn fails_while_present_program(directory: &Path) -> String {
    path_text(&compiled_program(
        directory,
        "fails-while-present",
        r#"fn main() {
    let file = std::env::args().nth(1).expect("the file to look for");
    std::process::exit(i32::from(std::path::Path::new(&file).exists()));
}
"#,
    ))
}

/// Run `command` as the signed-in user `name`, whichever variable the system names the user by.
pub fn sign_in_as<'a>(command: &'a mut Command, name: &str) -> &'a mut Command {
    command
        .env("USER", name)
        .env("LOGNAME", name)
        .env("USERNAME", name)
}

/// Every supported spelling used by portable child tools (including Python tempfile)
/// points to the same fixture-owned temporary directory.
pub fn temporary_environment<'a>(command: &'a mut Command, path: &Path) -> &'a mut Command {
    command
        .env("TMPDIR", path)
        .env("TMP", path)
        .env("TEMP", path)
}

/// The value of the environment variable `name` in this process (Windows ignores the name's
/// case), for tests that run as a probe a parent test starts with a prepared environment.
pub fn environment_variable(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name)
}

/// Run `command` to its end and collect its output.
pub fn run(command: &mut Command) -> std::process::Output {
    command.output().expect("the command runs")
}

/// Start `command` with its output piped, to collect with [`finish`] once it ends.
pub fn start(command: &mut Command) -> std::process::Child {
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("the command starts")
}

/// Observe and reap an exited standard child without changing its caller's ownership.
pub fn child_exited(child: &mut std::process::Child) -> bool {
    child
        .try_wait()
        .expect("observe the owned child exit")
        .is_some()
}

/// Query whether the supplied native stream is attached to a terminal.
pub fn is_terminal(stream: &impl std::io::IsTerminal) -> bool {
    stream.is_terminal()
}

/// Wait for an owned standard child without consuming its caller's cleanup/output owner.
/// The watchdog bounds failure only; completion is observed from the child itself.
pub fn wait_child(child: &mut std::process::Child, what: &str) {
    const EXIT_BOUND: std::time::Duration = std::time::Duration::from_secs(20);
    // Standard children have no portable blocking wait with a deadline.
    const EXIT_POLL: std::time::Duration = std::time::Duration::from_millis(20);
    let deadline = std::time::Instant::now() + patience(EXIT_BOUND);
    while !child_exited(child) {
        assert!(
            std::time::Instant::now() <= deadline,
            "{what} did not finish"
        );
        std::thread::sleep(EXIT_POLL);
    }
}

/// Wait for a spawned `child` to end within the usual patience; a child that does not is killed
/// and the test fails naming `what`.
pub fn finish(child: std::process::Child, what: &str) -> std::process::Output {
    // Reap even if polling fails or the deadline panics; Child's own Drop does neither.
    struct Reap(Option<std::process::Child>);
    impl Drop for Reap {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut child = Reap(Some(child));
    wait_child(child.0.as_mut().unwrap(), what);
    child.0.take().unwrap().wait_with_output().unwrap()
}

/// A piped async fixture process, owned through native exit observation and reaping even
/// when its original current-thread reactor is suspended by assertion unwind.
pub struct PipedTestChild(Option<tokio::process::Child>);

/// Start a protocol fixture with piped input/output and no unused stderr pipe that could
/// fill and block its replies. The returned owner also handles construction failure unwind.
pub fn spawn_piped_test(command: Command) -> PipedTestChild {
    PipedTestChild(Some(
        tokio::process::Command::from(command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("the piped test process starts"),
    ))
}

impl PipedTestChild {
    /// Transfer the protocol input pipe exactly once to its caller.
    pub fn take_stdin(&mut self) -> tokio::process::ChildStdin {
        self.0.as_mut().unwrap().stdin.take().unwrap()
    }

    /// Transfer the protocol output pipe exactly once to its caller.
    pub fn take_stdout(&mut self) -> tokio::process::ChildStdout {
        self.0.as_mut().unwrap().stdout.take().unwrap()
    }

    /// The active fixture process's identity, for read-only lifetime observations.
    pub fn id(&self) -> u32 {
        self.0
            .as_ref()
            .unwrap()
            .id()
            .expect("the fixture is running")
    }

    /// Observe the process's exit; callers retain their protocol-specific failure bounds.
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.0.as_mut().unwrap().wait().await
    }
}

impl Drop for PipedTestChild {
    fn drop(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = child.start_kill();
        // kill_on_drop alone does not reap. Poll on a separate reactor: the original
        // reactor may be suspended inside this synchronous Drop during unwind.
        let released = std::thread::scope(|scope| {
            scope
                .spawn(move || {
                    const EXIT_BOUND: std::time::Duration = std::time::Duration::from_secs(30);
                    // Observe native completion without borrowing the original signal driver.
                    const EXIT_POLL: std::time::Duration = std::time::Duration::from_millis(10);
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    runtime.block_on(async {
                        tokio::time::timeout(patience(EXIT_BOUND), async {
                            loop {
                                if let Some(status) = child.try_wait()? {
                                    return Ok::<_, std::io::Error>(status);
                                }
                                tokio::time::sleep(EXIT_POLL).await;
                            }
                        })
                        .await
                    })
                })
                .join()
        });
        if !std::thread::panicking() {
            released
                .expect("piped fixture cleanup thread")
                .expect("piped fixture exits")
                .expect("piped fixture reaped");
        }
    }
}

/// A self-test child whose native spawn, exit observation and reaping stay in this OS
/// boundary. Dropping the owner always stops and reaps an unfinished probe.
pub struct TestChild(std::process::Child);

/// Spawn a prepared self-test command without borrowing Tokio's runtime for its child
/// lifetime. The returned owner also cleans up during assertion unwind.
pub fn spawn_test(command: &mut tokio::process::Command) -> TestChild {
    TestChild(
        command
            .as_std_mut()
            .spawn()
            .expect("the self-test probe starts"),
    )
}

impl TestChild {
    /// The identity of this owned native child, for read-only fixture barriers.
    pub fn id(&self) -> u32 {
        self.0.id()
    }

    /// Observe the exact exit and reap it; the bound is a failure watchdog, never a
    /// sleep-based synchronization assumption in the caller.
    pub async fn wait(&mut self) -> std::process::ExitStatus {
        const EXIT_BOUND: std::time::Duration = std::time::Duration::from_secs(20);
        // Standard children expose nonblocking polling rather than an async exit handle.
        const EXIT_POLL: std::time::Duration = std::time::Duration::from_millis(10);
        tokio::time::timeout(patience(EXIT_BOUND), async {
            loop {
                if let Some(status) = self.0.try_wait().expect("observe the self-test exit") {
                    return status;
                }
                tokio::time::sleep(EXIT_POLL).await;
            }
        })
        .await
        .expect("the self-test probe did not exit")
    }

    /// Explicitly stop an owned probe; exit observation remains a separate action.
    pub fn kill(&mut self) -> std::io::Result<()> {
        self.0.kill()
    }
}

impl Drop for TestChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
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

/// A directory whose known descendants remain reachable without allowing its entries to
/// be listed or changed. Declare this after its TempDir so Drop restores cleanup access.
/// Windows has no Unix search-only permission mode, so only Unix exposes this fixture.
#[cfg(unix)]
pub struct SearchOnlyDirectory {
    path: std::path::PathBuf,
    original: fs::Permissions,
}

#[cfg(unix)]
impl SearchOnlyDirectory {
    /// Restrict an existing, fixture-owned directory immediately.
    pub fn new(path: &Path) -> Self {
        let metadata = path.metadata().unwrap();
        assert!(metadata.is_dir(), "the search-only fixture is a directory");
        let directory = Self {
            path: path.to_owned(),
            original: metadata.permissions(),
        };
        directory.restrict();
        directory
    }

    /// Temporarily let the owner list and modify entries without discarding other bits.
    pub fn restore(&self) {
        use std::os::unix::fs::PermissionsExt;
        // Owner rwx is needed for fixture updates and recursive TempDir cleanup.
        const OWNER_ACCESS: u32 = 0o700;
        set_mode(&self.path, self.original.mode() | OWNER_ACCESS);
    }

    /// Keep traversal for known paths, but remove directory listing and write access.
    pub fn restrict(&self) {
        // Execute on a directory means search, not permission to enumerate its entries.
        const SEARCH_ONLY: u32 = 0o111;
        set_mode(&self.path, SEARCH_ONLY);
    }
}

#[cfg(unix)]
impl Drop for SearchOnlyDirectory {
    fn drop(&mut self) {
        // Best effort during unwind, before the fixture owner's recursive cleanup runs.
        let _ = fs::set_permissions(&self.path, self.original.clone());
    }
}

/// Let everyone list and enter a directory: mode 0755 on Unix, and on Windows an
/// inheritable Everyone read ACE, so newly created children would also be shared.
pub fn share_dir(path: &Path) {
    #[cfg(unix)]
    set_mode(path, 0o755);
    #[cfg(windows)]
    {
        // Everyone's well-known SID is language independent; OI/CI makes the
        // fixture exercise inheritance rather than only the parent's own access.
        let output = Command::new("icacls")
            .arg(path)
            .args(["/grant", "*S-1-1-0:(OI)(CI)(R)"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}

/// The effective user ID of this process.
#[cfg(unix)]
pub fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and does not change process state.
    unsafe { libc::geteuid() }
}

/// A temporary directory below macOS's `/tmp`, the system alias of `/private/tmp`, as an
/// operator's state path may name it.
#[cfg(target_os = "macos")]
pub fn tempdir_below_tmp_alias() -> tempfile::TempDir {
    tempfile::tempdir_in("/tmp").unwrap()
}

/// A file name with whitespace in it: with a line break where the system allows one in a
/// name, only a space on Windows, which forbids line breaks in names.
pub fn whitespace_name() -> &'static str {
    if cfg!(windows) {
        "other space"
    } else {
        "other space\nline"
    }
}

/// Whether the volume holding `directory` opens a name in any case, as Windows and default
/// macOS volumes do.
pub fn case_insensitive(directory: &Path) -> bool {
    let probe = directory.join("case-probe");
    std::fs::write(&probe, "").unwrap();
    let insensitive = other_case(&probe).exists();
    std::fs::remove_file(probe).unwrap();
    insensitive
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
    // Evaluations put TMPDIR under their private run output. That full path can exceed
    // sockaddr_un even though the filesystem accepts it. Bind through a short, owned
    // alias; the actual socket entry remains at `path` after the alias is removed.
    let alias = tempfile::tempdir_in("/tmp").unwrap();
    let parent = alias.path().join("parent");
    std::os::unix::fs::symlink(path.parent().unwrap().canonicalize().unwrap(), &parent).unwrap();
    std::os::unix::net::UnixListener::bind(parent.join(path.file_name().unwrap())).unwrap()
}

/// Special native entries held for tool input/output rejection tests. Windows has no
/// filesystem FIFO or Unix socket entry, so its portable fixture exposes no such paths.
pub struct SpecialFiles {
    paths: &'static [&'static str],
    #[cfg(unix)]
    root: std::path::PathBuf,
    #[cfg(unix)]
    _socket: std::os::unix::net::UnixListener,
}

impl SpecialFiles {
    pub fn paths(&self) -> &[&'static str] {
        self.paths
    }
}

pub fn special_files(directory: &Path) -> SpecialFiles {
    #[cfg(unix)]
    {
        let files = SpecialFiles {
            paths: &["socket", "pipe"],
            root: directory.to_owned(),
            _socket: socket(&directory.join("socket")),
        };
        fifo(&directory.join("pipe"));
        files
    }
    #[cfg(windows)]
    {
        let _ = directory;
        SpecialFiles { paths: &[] }
    }
}

impl Drop for SpecialFiles {
    fn drop(&mut self) {
        #[cfg(unix)]
        for name in self.paths {
            let _ = fs::remove_file(self.root.join(name));
        }
    }
}

/// Only Unix exposes ENOTDIR for this fixture; Windows reports a different path error,
/// so callers assert this particular native error only when the OS helper supplies it.
pub fn not_a_directory_error() -> Option<std::io::Error> {
    #[cfg(unix)]
    {
        Some(not_a_directory())
    }
    #[cfg(windows)]
    {
        None
    }
}

/// The error opening a path below a regular file reports on Unix.
#[cfg(unix)]
pub fn not_a_directory() -> std::io::Error {
    std::io::Error::from_raw_os_error(libc::ENOTDIR)
}

/// The signal number of SIGTERM.
#[cfg(unix)]
pub const SIGTERM: i32 = libc::SIGTERM;

/// A shell command whose own shell sends itself SIGTERM: meaningful only where a signal
/// can end a process, so only Unix code reads this value.
pub const SELF_TERMINATE: &str = "kill -TERM $$";

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

/// Block until `pid` exits, without reaping it: `waitid` with `WNOWAIT`, so a caller can
/// still probe the zombie afterward.
#[cfg(unix)]
pub fn wait_for_zombie(pid: u32) {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::uninit();
    // SAFETY: waitid writes a full siginfo_t for our child; WNOWAIT synchronizes on exit
    // while deliberately keeping the zombie unreaped.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOWAIT,
        )
    };
    assert_eq!(result, 0, "{}", std::io::Error::last_os_error());
}

/// Construct a FIFO where that native file kind exists; Windows has no FIFO pathname.
pub fn fifo_if_supported(path: &Path) -> Option<()> {
    #[cfg(unix)]
    {
        fifo(path);
        Some(())
    }
    #[cfg(windows)]
    {
        let _ = path;
        None
    }
}

/// A pipe named like the one Git Bash's mintty hands a program: the writer and the reader,
/// which `is_terminal` takes for a terminal although it has no console.
#[cfg(windows)]
pub fn mintty_pipe() -> (std::fs::File, std::fs::File) {
    // Small byte-stream buffers are sufficient: this fixture observes pipe naming,
    // not throughput, and never needs to allocate an unbounded console transcript.
    const PIPE_BUFFER_BYTES: u32 = 4096;
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
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
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
    use super::fixture_compiler;
    use std::{
        env::consts::EXE_SUFFIX,
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::{Mutex, OnceLock},
    };

    /// Every utility answered by the test process's own stand-in executable.
    const UTILITIES: &[&str] = &[
        "sh", "cat", "cksum", "echo", "env", "false", "grep", "head", "ls", "mkdir", "printf",
        "pwd", "rm", "sleep", "test", "touch", "tr", "true",
    ];
    // Paths may be handed to several concurrent fixtures, so the test process owns this
    // directory until exit. Unix drops its TempDir in atexit; Windows delegates removal
    // to a child holding the parent's native process handle, since ExitProcess skips atexit.
    static OWNED: Mutex<Option<tempfile::TempDir>> = Mutex::new(None);

    #[cfg(unix)]
    extern "C" fn cleanup() {
        let directory = OWNED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        drop(directory);
    }

    /// Built once for this test process. The local TempDir cleans up an initialization
    /// panic; after publication, exit cleanup also runs when libtest reports panicking tests.
    pub(super) fn directory() -> &'static Path {
        static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
        DIRECTORY.get_or_init(|| {
            let root =
                option_env!("CARGO_TARGET_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
            fs::create_dir_all(&root).unwrap();
            let directory = tempfile::Builder::new()
                .prefix("artifactize-fixture-")
                .tempdir_in(&root)
                .unwrap();
            let program = directory.path().join(format!("fixture{EXE_SUFFIX}"));
            let rustc = fixture_compiler();
            let output = Command::new(rustc)
                .args(["--edition", "2024", "--crate-name", "fixture", "-o"])
                .arg(&program)
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../artifactize-core/tests/support/fixture.rs"
                ))
                .output()
                .expect("rustc builds the test fixture");
            assert!(
                output.status.success(),
                "building the test fixture failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            for name in UTILITIES {
                fs::hard_link(
                    &program,
                    directory.path().join(format!("{name}{EXE_SUFFIX}")),
                )
                .unwrap();
            }
            // SAFETY: cleanup is process-global, registered once by this OnceLock, and
            // only drops the directory owned by this test process after its fixtures end.
            #[cfg(unix)]
            let registered = unsafe { libc::atexit(cleanup) };
            #[cfg(unix)]
            assert_eq!(registered, 0, "fixture exit cleanup is registered");
            #[cfg(windows)]
            super::worker::cleanup_after_exit(directory.path())
                .expect("fixture exit cleanup is registered before publication");
            let path = directory.path().to_path_buf();
            *OWNED.lock().unwrap() = Some(directory);
            path
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

/// The current test process's native identity, for fixtures that need a live owner.
pub fn current_pid() -> u32 {
    std::process::id()
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

/// Launch with a fresh console or no console, independent of the test runner's terminal.
/// Only Windows has this desktop console attachment distinction.
#[cfg(windows)]
pub fn console_launch(command: &mut Command, with_console: bool) -> &mut Command {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_CONSOLE, CREATE_NO_WINDOW};

    command.creation_flags(if with_console {
        CREATE_NEW_CONSOLE
    } else {
        CREATE_NO_WINDOW
    })
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
pub fn kill(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: the PID is a process this test owns, not a group or the caller.
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    }
    #[cfg(windows)]
    {
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
}
/// Whether a process is running, excluding a Unix zombie or a signalled Windows handle.
pub fn running(pid: u32) -> bool {
    observation::running(pid)
}

/// A Unix process entry still present, including a zombie awaiting its parent.
#[cfg(unix)]
pub fn exists(pid: u32) -> bool {
    observation::exists(pid)
}

/// A program running on a pseudo-terminal of its own: an openpty(3) pair on Unix, whose
/// terminal becomes the program's controlling terminal, and a ConPTY on Windows. The program
/// and its arguments are passed as they are, with no shell. What it draws arrives in chunks
/// from `start`'s receiver, which ends once `finish` has closed the terminal.
pub struct PseudoTerminal {
    #[cfg(unix)]
    child: Option<std::process::Child>,
    /// The terminal's controlling side, which what is typed is written to.
    #[cfg(unix)]
    input: Option<std::fs::File>,
    #[cfg(windows)]
    console: Option<conpty::Console>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl PseudoTerminal {
    /// Start `program` with `args` on a terminal `columns` wide and `rows` high, with the
    /// test's environment except the variables named in `without`.
    pub fn start(
        program: &Path,
        args: &[&str],
        without: &[&str],
        columns: u16,
        rows: u16,
    ) -> (Self, std::sync::mpsc::Receiver<Vec<u8>>) {
        #[cfg(unix)]
        let (terminal, output) = {
            use std::os::{
                fd::{FromRawFd, OwnedFd},
                unix::process::CommandExt,
            };
            let mut size = libc::winsize {
                ws_row: rows,
                ws_col: columns,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let (mut master, mut slave) = (0, 0);
            // SAFETY: openpty writes two new descriptors, owned below, and reads the size.
            let opened = unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    // Linux takes the size as const, macOS as mut; a raw pointer suits both.
                    &raw mut size,
                )
            };
            assert_eq!(opened, 0, "{}", std::io::Error::last_os_error());
            // SAFETY: both descriptors were just opened and are owned exactly once.
            let (master, slave) = unsafe {
                (
                    std::fs::File::from_raw_fd(master),
                    OwnedFd::from_raw_fd(slave),
                )
            };
            let mut command = Command::new(program);
            command
                .args(args)
                .stdin(slave.try_clone().unwrap())
                .stdout(slave.try_clone().unwrap())
                .stderr(slave);
            for name in without {
                command.env_remove(name);
            }
            // SAFETY: the child calls only setsid and ioctl, which are async-signal-safe.
            unsafe {
                command.pre_exec(|| {
                    // A new session whose controlling terminal is the pseudo-terminal.
                    if libc::setsid() == -1
                        || libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0) == -1
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            // Complete fallible descriptor setup before a child exists to clean up.
            let input = master.try_clone().unwrap();
            let child = command.spawn().unwrap();
            // Only the program holds the terminal now, so its exit ends the output.
            drop(command);
            (
                Self {
                    child: Some(child),
                    input: Some(input),
                    reader: None,
                },
                master,
            )
        };
        #[cfg(windows)]
        let (terminal, output) = {
            let (console, output) = conpty::Console::start(program, args, without, columns, rows);
            (
                Self {
                    console: Some(console),
                    reader: None,
                },
                output,
            )
        };
        let mut terminal = terminal;
        let (sender, receiver) = std::sync::mpsc::channel();
        terminal.reader = Some(std::thread::spawn(move || {
            use std::io::Read;
            let mut output = output;
            // Stream terminal output in small chunks; transcript size is not a stack allocation.
            const READ_CHUNK_BYTES: usize = 4096;
            let mut buffer = [0; READ_CHUNK_BYTES];
            while let Ok(read @ 1..) = output.read(&mut buffer) {
                if sender.send(buffer[..read].to_vec()).is_err() {
                    break;
                }
            }
        }));
        (terminal, receiver)
    }

    /// Type `text` at the terminal.
    pub fn type_text(&mut self, text: &[u8]) {
        use std::io::Write;
        #[cfg(unix)]
        self.input.as_mut().unwrap().write_all(text).unwrap();
        #[cfg(windows)]
        self.console
            .as_mut()
            .unwrap()
            .input
            .write_all(text)
            .unwrap();
    }

    /// Wait for the program to exit, then close the terminal so that its output ends.
    /// Whether the program succeeded.
    pub fn finish(mut self) -> bool {
        let deadline = std::time::Instant::now() + patience(std::time::Duration::from_secs(10));
        #[cfg(unix)]
        {
            loop {
                let child = self.child.as_ref().unwrap();
                // Keep the exited leader unreaped until its group is closed. Reaping it
                // first could let its PID be reused as someone else's signal target.
                // SAFETY: siginfo_t is plain C data; zero means waitid reported no event.
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                // SAFETY: observe only this owned child with a valid output pointer.
                let observed = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        child.id(),
                        &raw mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                assert_eq!(observed, 0, "{}", std::io::Error::last_os_error());
                if info.si_signo != 0 {
                    kill_tree(child.id());
                    let status = self.child.as_mut().unwrap().wait().unwrap();
                    self.child.take();
                    return status.success();
                }
                assert!(
                    std::time::Instant::now() <= deadline,
                    "the terminal program did not exit"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        #[cfg(windows)]
        {
            self.console
                .as_mut()
                .unwrap()
                .wait(deadline.saturating_duration_since(std::time::Instant::now()))
        }
    }
}

impl Drop for PseudoTerminal {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // start created a separate session/group: stop any descendants holding a slave,
            // then reap the leader before joining the reader blocked on the master.
            if let Some(mut child) = self.child.take() {
                kill_tree(child.id());
                let _ = child.kill();
                let _ = child.wait();
            }
            drop(self.input.take());
        }
        #[cfg(windows)]
        drop(self.console.take());
        if let Some(reader) = self.reader.take() {
            let result = reader.join();
            if !std::thread::panicking()
                && let Err(panic) = result
            {
                std::panic::resume_unwind(panic);
            }
        }
    }
}

/// Check that a full-screen program left the terminal as it found it, from what the
/// terminal showed between its start and its close. On Unix script(1) passes the program's
/// own sequences through: it enters the alternate screen first and leaves it, showing the
/// cursor, last. A ConPTY renders the console instead and sends its own sequences: after the
/// program's last frame the screen is redrawn without it, and the cursor shown.
pub fn assert_restored(output: &str, frame: &str) {
    #[cfg(unix)]
    {
        let _ = frame;
        assert!(output.starts_with("\x1b[?1049h"), "{output:?}");
        assert!(output.ends_with("\x1b[?25h\x1b[?1049l"), "{output:?}");
    }
    #[cfg(windows)]
    {
        let last = output.rfind(frame).expect("the program drew its frame");
        let after = &output[last..];
        assert!(
            ["\x1b[?1049l", "\x1b[2J", "\x1b[J", "\x1b[0J"]
                .iter()
                .any(|erase| after.contains(erase)),
            "the last frame is not erased: {after:?}"
        );
        assert!(
            output.rfind("\x1b[?25h") > output.rfind("\x1b[?25l"),
            "the cursor is not shown again: {after:?}"
        );
    }
}

/// Unix terminals pass bracketed-paste modes through; ConPTY keeps input modes inside
/// its console, so their ordering is observable only through the Unix terminal pipe.
pub fn assert_bracketed_paste(output: &str) {
    #[cfg(unix)]
    {
        let on = output.find("\x1b[?2004h").expect("bracketed paste enabled");
        let off = output
            .rfind("\x1b[?2004l")
            .expect("bracketed paste disabled");
        assert!(on < off, "{output:?}");
    }
    #[cfg(windows)]
    let _ = output;
}

/// A ConPTY driven through its pipes, the Windows pseudo-terminal.
#[cfg(windows)]
mod conpty {
    use std::{
        io::{PipeReader, PipeWriter},
        os::windows::io::AsRawHandle,
        path::Path,
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
        System::{
            Console::{COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON},
            Threading::{
                CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
                EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE,
                InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
                UpdateProcThreadAttribute, WaitForSingleObject,
            },
        },
    };

    struct PseudoConsole(HPCON);

    impl Drop for PseudoConsole {
        fn drop(&mut self) {
            // SAFETY: this owns the console from creation, including partial setup failure.
            unsafe { ClosePseudoConsole(self.0) };
        }
    }

    struct Attributes(*mut std::ffi::c_void);

    impl Drop for Attributes {
        fn drop(&mut self) {
            // SAFETY: created only after initialization; the backing buffer still lives.
            unsafe { DeleteProcThreadAttributeList(self.0) };
        }
    }

    pub struct Console {
        pub input: PipeWriter,
        console: Option<PseudoConsole>,
        process: HANDLE,
    }

    impl Console {
        pub fn start(
            program: &Path,
            args: &[&str],
            without: &[&str],
            columns: u16,
            rows: u16,
        ) -> (Self, PipeReader) {
            let (console_input, input) = std::io::pipe().unwrap();
            let (output, console_output) = std::io::pipe().unwrap();
            let size = COORD {
                X: columns as i16,
                Y: rows as i16,
            };
            let mut console = 0;
            // SAFETY: both pipe ends are open handles; the console duplicates them.
            let result = unsafe {
                CreatePseudoConsole(
                    size,
                    console_input.as_raw_handle(),
                    console_output.as_raw_handle(),
                    0,
                    &mut console,
                )
            };
            assert!(result >= 0, "CreatePseudoConsole: HRESULT {result:#x}");
            let console = PseudoConsole(console);
            let mut bytes = 0;
            // SAFETY: a null list asks for the size the list needs.
            unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut bytes) };
            let mut list = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
            let attributes = list.as_mut_ptr().cast();
            // SAFETY: the buffer has the size asked for and is aligned for the list.
            unsafe {
                assert_ne!(
                    InitializeProcThreadAttributeList(attributes, 1, 0, &mut bytes),
                    0,
                    "{}",
                    std::io::Error::last_os_error()
                );
            }
            let attributes = Attributes(attributes);
            // SAFETY: the list is initialized, and the value is the live console handle
            // itself, as the attribute requires; both stay owned throughout startup.
            unsafe {
                assert_ne!(
                    UpdateProcThreadAttribute(
                        attributes.0,
                        0,
                        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                        console.0 as *const std::ffi::c_void,
                        size_of::<HPCON>(),
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    ),
                    0,
                    "{}",
                    std::io::Error::last_os_error()
                );
            }
            // SAFETY: STARTUPINFOEXW is plain data; zero is its empty value.
            let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            // Without these the program would write to the test's own redirected handles
            // instead of the console.
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
            startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
            startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
            startup.lpAttributeList = attributes.0;
            let mut command_line: Vec<u16> = std::iter::once(program.to_str().unwrap())
                .chain(args.iter().copied())
                .map(quote)
                .collect::<Vec<_>>()
                .join(" ")
                .encode_utf16()
                .chain([0])
                .collect();
            // The environment block: `name=value` entries, each ended by a NUL, then a NUL.
            let mut variables: Vec<_> = std::env::vars_os()
                .filter(|(name, _)| !without.iter().any(|left| name.eq_ignore_ascii_case(left)))
                .collect();
            variables.sort_by_key(|(name, _)| name.to_ascii_uppercase());
            let mut environment: Vec<u16> = Vec::new();
            for (name, value) in variables {
                let mut entry = name;
                entry.push("=");
                entry.push(value);
                environment.extend(std::os::windows::ffi::OsStrExt::encode_wide(
                    entry.as_os_str(),
                ));
                environment.push(0);
            }
            environment.push(0);
            // SAFETY: PROCESS_INFORMATION is plain data that CreateProcessW fills.
            let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
            // SAFETY: the command line is a writable NUL-terminated buffer, the environment a
            // double-NUL-terminated UTF-16 block, and the startup information holds the
            // initialized attribute list; all outlive the call.
            let created = unsafe {
                CreateProcessW(
                    std::ptr::null(),
                    command_line.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                    environment.as_ptr().cast(),
                    std::ptr::null(),
                    &startup.StartupInfo,
                    &mut process,
                )
            };
            let error = std::io::Error::last_os_error();
            drop(attributes);
            assert_ne!(created, 0, "CreateProcessW: {error}");
            // SAFETY: the thread handle is open and not used again.
            unsafe { CloseHandle(process.hThread) };
            // The console holds its own copies of its pipe ends.
            drop((console_input, console_output));
            (
                Self {
                    input,
                    console: Some(console),
                    process: process.hProcess,
                },
                output,
            )
        }

        /// Wait up to `limit` for the program to exit. Whether it exited with status 0.
        pub fn wait(&mut self, limit: Duration) -> bool {
            let milliseconds = u32::try_from(limit.as_millis()).unwrap_or(u32::MAX - 1);
            // SAFETY: the process handle is open until drop.
            let waited = unsafe { WaitForSingleObject(self.process, milliseconds) };
            assert_eq!(waited, WAIT_OBJECT_0, "the terminal program did not exit");
            let mut code = 0;
            // SAFETY: as above; the exit code is written to a local.
            assert_ne!(unsafe { GetExitCodeProcess(self.process, &mut code) }, 0);
            code == 0
        }

        /// Close the console, ending its output.
        pub fn close(&mut self) {
            drop(self.console.take());
        }
    }

    impl Drop for Console {
        fn drop(&mut self) {
            // SAFETY: the process handle is open with termination/synchronization access.
            // Termination is asynchronous: wait before closing its handle and output console.
            unsafe {
                TerminateProcess(self.process, 1);
                WaitForSingleObject(self.process, INFINITE);
                CloseHandle(self.process);
            }
            self.close();
        }
    }

    /// One argument as the C runtime splits a command line: quoted when it holds a space,
    /// tab or quote, with backslashes doubled before a quote and at its end.
    fn quote(arg: &str) -> String {
        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            return arg.to_owned();
        }
        let mut quoted = String::from('"');
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                    backslashes = 0;
                }
                _ => {
                    quoted.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                }
            }
            if c != '\\' {
                quoted.push(c);
            }
        }
        quoted.push_str(&"\\".repeat(backslashes * 2));
        quoted.push('"');
        quoted
    }
}

/// The birth time of an owned process, in the platform's persisted units.
pub fn start_time(pid: u32) -> u64 {
    observation::start_time(pid)
}

/// Observe the native parent of this exact PID; callers guard lifetime identity separately.
pub fn parent_pid(pid: u32) -> Option<u32> {
    observation::parent_pid(pid)
}

/// Read a birth identity; disappearance before capture is an ordinary observation result.
pub fn observed_start_time(pid: u32) -> Option<u64> {
    observation::observed_start_time(pid)
}

/// A read-only lifetime check including birth time, so PID reuse is not an owned worker.
pub fn same_process_running(pid: u32, start_time: u64) -> bool {
    observation::same_process_running(pid, start_time)
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

/// Whether the object received any inherited ACE, rather than its own explicit DACL.
#[cfg(windows)]
pub fn has_inherited_aces(path: &Path) -> bool {
    acl::has_inherited_aces(path)
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
            GetSecurityDescriptorDacl, GetTokenInformation, INHERIT_ONLY_ACE, INHERITED_ACE,
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

    /// Inspect the inheritance bit independently of which principals an ACE grants access.
    pub(super) fn has_inherited_aces(path: &Path) -> bool {
        // SAFETY: the OS allocates a descriptor; all ACE reads stay within its DACL and the
        // descriptor is freed only after the last read.
        unsafe {
            let mut dacl: *mut ACL = ptr::null_mut();
            let mut descriptor = ptr::null_mut();
            assert_eq!(
                GetNamedSecurityInfoW(
                    wide(path).as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut dacl,
                    ptr::null_mut(),
                    &mut descriptor,
                ),
                ERROR_SUCCESS
            );
            assert!(!dacl.is_null());
            let mut size = ACL_SIZE_INFORMATION::default();
            assert_ne!(
                GetAclInformation(
                    dacl,
                    (&raw mut size).cast(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                ),
                0
            );
            let inherited = (0..size.AceCount).any(|index| {
                let mut ace: *mut c_void = ptr::null_mut();
                assert_ne!(GetAce(dacl, index, &mut ace), 0);
                let header = ace.cast::<ACE_HEADER>().read_unaligned();
                u32::from(header.AceFlags) & INHERITED_ACE != 0
            });
            LocalFree(descriptor);
            inherited
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

/// Run one test in a fresh process with a literal environment, without mutating the
/// parallel test runner's environment.
pub fn run_test(test: &str, environment: &[(&str, std::ffi::OsString)]) -> std::process::Output {
    test_command(test, environment).output().unwrap()
}

/// Run one exact probe without selected inherited variables; the parent environment stays intact.
pub fn run_test_without(
    test: &str,
    environment: &[(&str, std::ffi::OsString)],
    without: &[&str],
) -> std::process::Output {
    let mut command = test_command(test, environment);
    for name in without {
        command.env_remove(name);
    }
    command.output().unwrap()
}

/// The same isolated probe, started in the directory whose CLI behavior it checks.
pub fn run_test_in(
    test: &str,
    directory: &Path,
    environment: &[(&str, std::ffi::OsString)],
) -> std::process::Output {
    test_command(test, environment)
        .current_dir(directory)
        .output()
        .unwrap()
}

/// Prepare an exact child probe for a caller to customize before owning it with start/finish.
pub fn test_command(test: &str, environment: &[(&str, std::ffi::OsString)]) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", test, "--nocapture"]);
    for (name, value) in environment {
        command.env(name, value);
    }
    worker::prepare_probe(&mut command);
    command
}

/// Create or replace an owner-only fixture file. Replacement keeps the inode for tests
/// of same-sized rewrites; atomic replacement is set up explicitly by those tests.
pub fn rewrite_private_file(path: &Path, contents: impl AsRef<[u8]>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap();
    }
    #[cfg(windows)]
    {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .unwrap();
        acl::restrict(path);
    }
    std::fs::write(path, contents).unwrap();
}

/// Pin a fixture's last-write time independently of clock resolution and scheduling.
pub fn set_modified(path: &Path, time: std::time::SystemTime) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

/// Create independent Git repository inputs with the names supported by this OS.
/// Windows forbids CR/LF in filenames; Unix must preserve both, including a trailing CR.
pub fn git_path_cases(root: &Path) -> Vec<std::path::PathBuf> {
    let names: &[&str] = if cfg!(unix) {
        &["plain", "trailing\r", "embedded\nline"]
    } else {
        &["plain"]
    };
    names
        .iter()
        .map(|name| {
            let path = root.join(name);
            fs::create_dir(&path).unwrap();
            path
        })
        .collect()
}

/// Run Git against a fixture repository; no shell or process-status decoding in tests.
pub fn git(path: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Point a copied declaration's Unix commands at the stand-ins, and with `deadlines` give
/// its deadlines of a second or more `os::slow` room. The runtime fixture's `check.sh`
/// compares its working folder as a file rather than as text: Windows spells that folder
/// with `\`, where the script appends `/review`.
pub fn port_fixture(path: &Path, deadlines: bool) {
    if !stand_ins() && !cfg!(target_os = "macos") {
        return;
    }
    /// Whether anything changed.
    fn commands(value: &mut Value, deadlines: bool) -> bool {
        match value {
            Value::Object(object) => {
                let mut changed = false;
                for (key, value) in object.iter_mut() {
                    changed |= match value {
                        Value::String(command)
                            if key == "command"
                                && (command.starts_with("/bin/")
                                    || command.starts_with("/usr/bin/")
                                    || command == "sh") =>
                        {
                            let resolved = bin(command);
                            let changed = *command != resolved;
                            *command = resolved;
                            changed
                        }
                        Value::Number(deadline)
                            if deadlines
                                && key == "timeout_ms"
                                && deadline.as_u64().is_some_and(|ms| ms >= 1000) =>
                        {
                            *value = json!(slow(deadline.as_u64().unwrap()));
                            true
                        }
                        other => commands(other, deadlines),
                    };
                }
                changed
            }
            Value::Array(items) => {
                let mut changed = false;
                for item in items {
                    changed |= commands(item, deadlines);
                }
                changed
            }
            _ => false,
        }
    }
    match path.file_name().and_then(|name| name.to_str()) {
        // Rewritten only when needed: a declaration's bytes can be part of a fingerprint.
        Some(name) if name.ends_with(".artf") => {
            let mut declaration: Value =
                toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
            if commands(&mut declaration, deadlines) {
                fs::write(path, toml::to_string_pretty(&declaration).unwrap()).unwrap();
            }
        }
        Some("check.sh") => {
            let script = fs::read_to_string(path).unwrap();
            let script = script.replace(r#"test "$PWD" = "#, r#"test "$PWD" -ef "#);
            fs::write(path, script).unwrap();
        }
        _ => {}
    }
}

/// A shell loop that runs until a file named `release` exists in its working directory, for
/// a process that must outlive a test's checks and end with the test.
pub const UNTIL_RELEASED: &str = "while [ ! -e release ]; do sleep 0.01; done";

/// Ends the processes waiting in `directory` with [`UNTIL_RELEASED`] when dropped, however
/// the test ends.
pub struct Release(pub std::path::PathBuf);

impl Drop for Release {
    fn drop(&mut self) {
        let _ = std::fs::write(self.0.join("release"), "");
    }
}

/// The declared Python fixture blocks on an unset event, with no natural timer-based exit.
const LINGERING_PYTHON: &str = "import threading; threading.Event().wait()";

/// Literal argv for a child that can end only when its owner terminates it. Python is the
/// shared declared fixture interpreter on all supported CI platforms.
pub fn lingering_command() -> (String, Vec<String>) {
    (
        python_program().to_owned(),
        vec![
            "-X".into(),
            "utf8".into(),
            "-c".into(),
            LINGERING_PYTHON.into(),
        ],
    )
}

/// A declared Python process tree: record the leader and its child at the given argv path,
/// then wait for the child without a timer. The caller owns and stops the whole tree.
pub fn process_tree_probe() -> &'static str {
    r#"import os, pathlib, subprocess, sys
child = subprocess.Popen([sys.executable, '-c', 'import threading; threading.Event().wait()'])
try:
    pathlib.Path(sys.argv[1]).write_bytes(f'{os.getpid()} {child.pid}\n'.encode('ascii'))
    child.wait()
finally:
    if child.poll() is None:
        child.kill()
    child.wait()
"#
}

/// The same unbounded child for shell fixtures; no sleep duration has to outlast the test.
pub const LINGERING: &str = "python3 -X utf8 -c 'import threading; threading.Event().wait()'";

/// A shell command whose leader ignores SIGTERM and waits for a child that, in the
/// background, writes its PID and a newline to `$ARTIFACTIZE_OUTPUT_DIR/grandchild` and
/// then blocks without a timeout until its owner kills it.
pub const LINGERING_GRANDCHILD: &str = "trap '' TERM; sh -c 'python3 -X utf8 -c \"import threading; threading.Event().wait()\" & echo $! > \"$ARTIFACTIZE_OUTPUT_DIR/grandchild\"; wait' & wait";

/// Give the file at `original` a second name, `link`: a hard link on every system.
pub fn hard_link(original: &Path, link: &Path) {
    std::fs::hard_link(original, link).unwrap();
}

/// The identity of the file at `path` on its volume: the device and inode on Unix, the
/// volume serial number and file index on Windows. A file replaced by a rename has a new one.
pub fn file_identity(path: &Path) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).unwrap();
        (metadata.dev(), metadata.ino())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let file = std::fs::File::open(path).unwrap();
        let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: the handle is open for the call, and the record is read only once filled.
        let information = unsafe {
            assert_ne!(
                GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()),
                0,
                "{}",
                std::io::Error::last_os_error()
            );
            information.assume_init()
        };
        (
            u64::from(information.dwVolumeSerialNumber),
            u64::from(information.nFileIndexHigh) << 32 | u64::from(information.nFileIndexLow),
        )
    }
}

/// A file made read-only until this drops, when its original permissions come back: the
/// Unix mode, or the Windows read-only attribute.
pub struct ReadOnly {
    path: std::path::PathBuf,
    original: std::fs::Permissions,
}

/// Make the file at `path` read-only while the result lives.
pub fn read_only(path: &Path) -> ReadOnly {
    let original = std::fs::metadata(path).unwrap().permissions();
    let mut permissions = original.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(path, permissions).unwrap();
    ReadOnly {
        path: path.to_owned(),
        original,
    }
}

impl Drop for ReadOnly {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, self.original.clone());
    }
}

/// Physical temporary root, without aliases that scoped reads intentionally reject.
pub fn temp_root() -> std::path::PathBuf {
    // Not `\\?\` on Windows: a verbatim path takes `/` literally, but test scripts join with it.
    canonical(&std::env::temp_dir())
}

/// Atomically reserve a named fixture under an explicitly chosen physical parent.
pub fn tempdir_in(parent: &Path, prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(canonical(parent))
        .unwrap()
}

/// Write a directly runnable shell fixture with the platform's interpreter.
pub fn write_script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!{}\n{body}", bin("/bin/sh"))).unwrap();
    make_executable(path);
}

/// Native separators for assertions about OS error messages (not persisted path text).
pub fn native(path: &str) -> String {
    path.replace('/', std::path::MAIN_SEPARATOR_STR)
}

/// Set a spawned program's actual home rather than merely a Unix-only variable.
pub fn home_env() -> &'static str {
    "HOME"
}

/// The variables that name the author's home for `home`: `HOME`, and on Windows `USERPROFILE`,
/// which programs there (Python's `expanduser` among them) read instead.
pub fn home_variables(
    home: impl AsRef<std::ffi::OsStr>,
) -> Vec<(&'static str, std::ffi::OsString)> {
    let home = home.as_ref().to_owned();
    let mut variables = vec![(home_env(), home.clone())];
    if cfg!(windows) {
        variables.push(("USERPROFILE", home));
    }
    variables
}

/// A shell fixture reads this variable to observe its platform home.
pub fn home_expression() -> &'static str {
    "$HOME"
}

/// A cancellation fixture keeps the registration PID stable on Unix and cannot finish
/// naturally while its caller observes the started acknowledgment.
pub fn cancellation_script() -> &'static str {
    if cfg!(windows) {
        "touch started; python3 -X utf8 -c 'import threading; threading.Event().wait()'"
    } else {
        "touch started; exec python3 -X utf8 -c 'import threading; threading.Event().wait()'"
    }
}

/// Platform-independent assertion of the group containment established at registration.
pub fn assert_group(pid: u32) {
    #[cfg(unix)]
    assert!(leads_group(pid), "separate process group");
    #[cfg(windows)]
    assert!(running(pid), "registered job member is alive");
}

/// Detached launches also leave the caller's terminal session on Unix.
pub fn assert_detached(pid: u32) {
    assert_group(pid);
    #[cfg(unix)]
    assert!(leads_session(pid), "separate session");
}

/// Missing executables may be reported as a missing component on Windows.
pub fn missing_program(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
        || (cfg!(windows) && error.kind() == std::io::ErrorKind::NotADirectory)
}

/// Python's UTF-8 pipe mode is explicit on every platform.
pub fn python(source: &str) -> Vec<&str> {
    vec!["-X", "utf8", "-c", source]
}

/// A process status includes the platform's native description.
pub fn exit_status(code: i32) -> String {
    if cfg!(windows) {
        format!("exit code: {code}")
    } else {
        format!("exit status: {code}")
    }
}

/// Rejected states including system-specific name aliases and link traversal.
pub fn reviewed_state_paths(root: &Path, repo: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = vec![repo.join("state"), root.join("alias/state")];
    #[cfg(windows)]
    paths.push(other_case(repo).join("state"));
    #[cfg(unix)]
    paths.push(root.join("nested-alias/../state"));
    paths
}

/// Execute permissions can only be revoked on Unix.
pub fn revoke_execution(path: &Path) -> bool {
    #[cfg(unix)]
    {
        deny_execution(path);
        true
    }
    #[cfg(windows)]
    {
        let _ = path;
        false
    }
}

/// Every per-run folder artifactize gives a command, under the variables that name it.
pub fn runtime_directories() -> Vec<(&'static str, &'static str)> {
    vec![
        ("ARTIFACTIZE_OUTPUT_DIR", "output"),
        ("ARTIFACTIZE_TMP_DIR", "tmp"),
        ("TMPDIR", "tmp"),
        ("TMP", "tmp"),
        ("TEMP", "tmp"),
    ]
}

/// Portable executable fixture name; Windows uses its adjacent executable stand-in.
pub const TOOL: &str = "env-tool";

/// Whether `lines` of environment output (`NAME=value`) hold `name` set to `value`; Windows
/// compares names without regard to case, and keeps the casing the system first gave a name.
pub fn has_variable(lines: &[&str], name: &str, value: &str) -> bool {
    lines.iter().any(|line| {
        line.split_once('=').is_some_and(|(found, text)| {
            text == value
                && if cfg!(windows) {
                    found.eq_ignore_ascii_case(name)
                } else {
                    found == name
                }
        })
    })
}

/// A search path for the explicit-environment probe, without its build tools.
pub fn system_path() -> std::ffi::OsString {
    if cfg!(windows) {
        std::env::var_os("SystemRoot")
            .map(|root| {
                std::path::PathBuf::from(root)
                    .join("System32")
                    .into_os_string()
            })
            .unwrap()
    } else {
        "/usr/bin:/bin".into()
    }
}

/// Zero streams used to test concurrent pipe drainage and truncation.
pub fn zero_output(stdout: usize, stderr: usize) -> String {
    format!("head -c {stdout} /dev/zero & head -c {stderr} /dev/zero >&2 & wait")
}
pub fn letter_output(stdout: usize, stderr: usize) -> String {
    format!(
        "head -c {stdout} /dev/zero | tr '\\000' x & head -c {stderr} /dev/zero | tr '\\000' y >&2 & wait"
    )
}
pub fn capped_output(bytes: usize) -> String {
    format!("head -c {bytes} /dev/zero; printf discarded")
}

/// HOME and desktop variables inherited by a human-facing child.
pub fn human_environment<'a>(command: &'a mut Command, root: &Path) -> &'a mut Command {
    command
        .envs(home_variables(root.join("real-home")))
        .env("DISPLAY", ":77")
        .env("WAYLAND_DISPLAY", "wayland-test")
        .env("XDG_CONFIG_HOME", root.join("real-config"))
}

/// Launch the isolated Human-environment probe with the platform's home and desktop
/// variables and its command fixtures prepared before the child needs them.
pub fn run_human_test(
    test: &str,
    root: &Path,
    environment: &[(&str, std::ffi::OsString)],
) -> std::process::Output {
    shell();
    let mut command = test_command(test, environment);
    human_environment(&mut command, root);
    command.output().unwrap()
}

pub fn human_environment_probe() -> String {
    format!(
        "import os; print(os.environ['ARTIFACTIZE_HUMAN_MARKER']); print(os.environ['{}']); print(os.environ['DISPLAY']); print(os.environ['WAYLAND_DISPLAY']); print(os.environ['XDG_CONFIG_HOME'])",
        home_env()
    )
}
pub fn inherited_home_probe() -> String {
    format!(
        "import os; print(os.environ.get('ARTIFACTIZE_HUMAN_MARKER','absent')); print(os.environ['{}'])",
        home_env()
    )
}

/// A directly runnable Python script uses #! only where the OS supports it.
pub fn python_script(path: &Path, source: &str) -> (String, Vec<Value>) {
    std::fs::write(path, source).unwrap();
    if cfg!(windows) {
        (
            "python3".into(),
            vec![
                json!("-X"),
                json!("utf8"),
                json!(path.file_name().unwrap().to_str().unwrap()),
            ],
        )
    } else {
        let python = Command::new("python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(python.status.success());
        std::fs::write(
            path,
            format!(
                "#!{}\n{source}",
                String::from_utf8(python.stdout).unwrap().trim()
            ),
        )
        .unwrap();
        allow_execution(path);
        (
            format!("./{}", path.file_name().unwrap().to_str().unwrap()),
            vec![],
        )
    }
}

/// CRLF is accepted only on Windows; every other invalid fingerprint is common.
pub fn invalid_fingerprints() -> Vec<Vec<u8>> {
    let mut values: Vec<Vec<u8>> = [
        b"".as_slice(),
        b" value",
        b"value ",
        b"value\t",
        b"value\r\r\n",
        b"value\n\n",
        b"one\ntwo",
        b"value/key",
        b"\xff",
        b"ok\0",
        b"\x1b[31mok\x1b[0m",
        &[b'x'; 129],
    ]
    .into_iter()
    .map(<[u8]>::to_vec)
    .collect();
    if !cfg!(windows) {
        values.push(b"value\r\n".to_vec());
    }
    values
}
pub fn valid_fingerprints() -> Vec<Vec<u8>> {
    let mut values = vec![b"Aa0._:-".to_vec(), b"Aa0._:-\n".to_vec(), vec![b'x'; 128]];
    if cfg!(windows) {
        values.push(b"Aa0._:-\r\n".to_vec());
    }
    values
}

/// A Python program that ends itself abnormally: by SIGKILL on Unix. Windows has no
/// signals; Python's `os.kill` there ends the process with TerminateProcess.
pub fn self_terminating_python() -> String {
    let signal = if cfg!(windows) { "SIGTERM" } else { "SIGKILL" };
    format!("import os, signal\nos.kill(os.getpid(), signal.{signal})")
}

/// Only Unix offers an independently observable signal termination status.
pub fn signal_fingerprint() -> Option<Value> {
    if cfg!(windows) {
        None
    } else {
        Some(json!({"script":{"command":bin("/bin/sh"),"args":["-c","kill -TERM $$"]}}))
    }
}

/// A PATH shim exercises native lookup: batch/PATHEXT on Windows, #! on Unix.
pub fn lookup_shim(directory: &Path) {
    #[cfg(windows)]
    std::fs::write(
        directory.join("artifactize-lookup-shim.cmd"),
        "@echo off\r\npython3 \"%~dp0argv.py\" %*\r\n",
    )
    .unwrap();
    #[cfg(unix)]
    write_script(
        &directory.join("artifactize-lookup-shim"),
        "exec python3 \"$(dirname \"$0\")/argv.py\" \"$@\"\n",
    );
}

/// Python expression naming every process held by `lookup_shim`'s launch probe.
/// Unix exec replaces the shim; Windows keeps its batch parent waiting for Python.
pub fn launch_probe_pids() -> &'static str {
    #[cfg(unix)]
    {
        "[os.getpid()]"
    }
    #[cfg(windows)]
    {
        "[os.getppid(), os.getpid()]"
    }
}

/// Human/model and persisted path spelling, independent of native separators.
pub fn path_text(path: &Path) -> String {
    let text = path.to_str().expect("fixture paths are Unicode");
    #[cfg(windows)]
    {
        text.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        text.to_owned()
    }
}

/// A Python fixture's displayed or recorded paths use the same portable spelling.
/// The probe already imports pathlib; keep native path details in the shared helper.
pub fn portable_path_probe(source: &str) -> String {
    source.replace(
        "PATH_TEXT_FUNCTION",
        "def path_text(path):\n    return pathlib.Path(path).as_posix()\n",
    )
}

/// Native executable suffix lookup used by the batch shim fixture.
pub const PATHEXT: (&str, &str) = ("PATHEXT", ".CMD;.EXE;.BAT;.COM");

/// Runtime operational failures supported by this platform.
pub fn runtime_failures() -> Vec<(String, Vec<&'static str>, &'static str)> {
    let cases = vec![
        (bin("artifactize-missing-command"), vec![], "SPAWN_FAILED"),
        (
            bin("/bin/cat"),
            vec!["{test}/missing"],
            "PREPARATION_FAILED",
        ),
    ];
    cases.into_iter().chain(signal_failure()).collect()
}

fn signal_failure() -> Option<(String, Vec<&'static str>, &'static str)> {
    if cfg!(unix) {
        Some((shell(), vec!["-c", SELF_TERMINATE], "ABNORMAL_EXIT"))
    } else {
        None
    }
}

/// Fingerprint probe's private directory and permission checks differ by OS.
pub fn fingerprint_probe(source: &str) -> String {
    source.replace("PRIVATE_DIRECTORY_CHECK", "for key in ['TMPDIR', 'ARTIFACTIZE_OUTPUT_DIR']:\n    path = pathlib.Path(os.environ[key])\n    assert not path.is_relative_to(pathlib.Path(os.environ['ARTIFACTIZE_WORKSPACE_DIR']))\n    assert os.name == 'nt' or stat.S_IMODE(path.stat().st_mode) == 0o700\n    (path / 'discard').write_text('scratch')\nassert not pathlib.Path(os.path.expanduser('~')).is_relative_to(pathlib.Path(os.environ['ARTIFACTIZE_OUTPUT_DIR']).parent)")
}

/// Always select the portable stand-in, even on a Unix test runner.
pub fn stand_in_bin(name: &str) -> std::path::PathBuf {
    fixture::directory().join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// Prepend test programs to the native search path without reimplementing its separator.
pub fn prepend_path(directory: &Path) -> std::ffi::OsString {
    std::env::join_paths(
        std::iter::once(directory.to_owned()).chain(std::env::split_paths(&path())),
    )
    .unwrap()
}

/// Expected inherited human tool home and environment prefix.
pub fn human_tool_home() -> &'static str {
    "/reviewer-home"
}
pub fn human_tool_prefix() -> &'static str {
    "/reviewer-home|real-environment|"
}

/// Node used by the publication runtime fixture, installed on every CI platform.
pub fn node_program() -> &'static str {
    "node"
}

/// Python used by multimodal command-tool fixtures, installed on all CI platforms.
pub fn python_program() -> &'static str {
    worker::python_program()
}

/// Ensure fixture stdout is Unicode even with Windows' default ANSI pipe encoding.
pub fn write_python_script(path: &Path, code: &str) {
    std::fs::write(
        path,
        format!("import sys\nsys.stdout.reconfigure(encoding='utf-8')\n{code}"),
    )
    .unwrap();
}

/// A command runs in the author's environment: the real home, the per-run private temporary
/// and output directories, and none of artifactize's own variables.
pub fn python_authors_environment() -> String {
    r#"
assert os.path.expanduser('~') != os.environ['TMPDIR']
assert context['tmpDir'] == os.environ['TMPDIR'].replace('\\', '/')
assert context['outputDir'] == os.environ['ARTIFACTIZE_OUTPUT_DIR'].replace('\\', '/')
own = ('ARTIFACTIZE_WORKSPACE_DIR', 'ARTIFACTIZE_OUTPUT_DIR', 'ARTIFACTIZE_TMP_DIR', 'ARTIFACTIZE_SESSION_DIR')
assert not [name for name in os.environ if name.upper().startswith('ARTIFACTIZE_') and name not in own]
"#
    .to_owned()
}

/// A file/directory redirection in an output fixture: Windows junctions need no privilege.
pub fn python_output_links() -> (&'static str, &'static str) {
    if cfg!(windows) {
        (
            "import _winapi; path = 'link'; _winapi.CreateJunction(os.getcwd(), os.path.join(root, path))",
            "import _winapi; path = 'linkdir/source'; _winapi.CreateJunction(os.getcwd(), os.path.join(root, 'linkdir'))",
        )
    } else {
        (
            "path = 'link'; os.symlink(os.path.abspath('source'), os.path.join(root, path))",
            "path = 'linkdir/source'; os.symlink(os.getcwd(), os.path.join(root, 'linkdir'))",
        )
    }
}

/// Write an owner-relative program that emits a fixed fixture string.
pub fn output_script(path: &Path, output: &str) {
    std::fs::write(path, format!("#!/bin/sh\nprintf '%s' '{output}'\n")).unwrap();
    make_executable(path);
}

/// A bare PATH command and literal argv for the program-resolution seam.
pub fn path_output(output: &str) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        (
            python_program(),
            vec![
                "-c".into(),
                format!("import sys; sys.stdout.write({output:?})"),
            ],
        )
    } else {
        ("printf", vec!["%s".into(), output.into()])
    }
}

/// Expected IPC address independently pins the public namespace spelling and hash length.
pub fn changes_address(user: &str, identity: &str) -> std::path::PathBuf {
    if cfg!(windows) {
        std::path::PathBuf::from(format!(r"\\.\pipe\artifactize-changes-{identity}"))
    } else {
        Path::new(if cfg!(target_os = "macos") {
            "/private/tmp"
        } else {
            "/tmp"
        })
        .join(format!("artifactize-ipc-{user}-{}", &identity[..24]))
        .join("hub.sock")
    }
}

/// Launch one exact test in this binary for multi-process IPC/Job Object fixtures.
pub fn self_test(name: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name, "--nocapture"])
        .kill_on_drop(true);
    command
}

/// Windows process fixture that emits a marker when admitted, or exits with a given code.
/// Both are this library test binary's own probes, given literal arguments; no shell runs.
#[cfg(windows)]
pub fn windows_marker(directory: &Path) -> tokio::process::Command {
    let mut command = self_test("platform::windows::process::tests::marker_probe");
    command
        .env("ARTIFACTIZE_MARKER_PROBE", "1")
        .current_dir(directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null());
    command
}

#[cfg(windows)]
pub fn windows_exit(code: u32) -> tokio::process::Command {
    let mut command = self_test("platform::windows::process::exit_tests::exit_probe");
    command.env("ARTIFACTIZE_EXIT_PROBE", code.to_string());
    command
}

/// Put this disposable probe process in a Windows kill-on-close Job without breakaway.
/// Windows Jobs cannot be left; the caller must retain the handle until process exit.
#[cfg(windows)]
pub fn windows_job_without_breakaway() -> std::os::windows::io::OwnedHandle {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        },
        Threading::GetCurrentProcess,
    };
    // SAFETY: no attributes or name create a private, non-inheritable Job.
    let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    assert!(!handle.is_null(), "{}", std::io::Error::last_os_error());
    // SAFETY: this successful creation returned a new handle, owned only here.
    let job = unsafe { OwnedHandle::from_raw_handle(handle) };
    let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: the information class matches the initialized structure and size.
    let configured = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    };
    assert_ne!(configured, 0, "{}", std::io::Error::last_os_error());
    // SAFETY: the Job handle is open; GetCurrentProcess is a valid pseudo-handle.
    let assigned = unsafe { AssignProcessToJobObject(job.as_raw_handle(), GetCurrentProcess()) };
    assert_ne!(assigned, 0, "{}", std::io::Error::last_os_error());
    job
}

/// An inert child controlled by the parent's stdin; no scheduler/timing assumption.
pub fn stdin_waiter() -> tokio::process::Command {
    let mut command = tokio::process::Command::new(python_program());
    command
        .args(["-c", "import sys; sys.stdin.buffer.read()"])
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    command
}

/// Run a relative script file through the platform's shared shell fixture, as argv.
pub fn script_file(path: &Path) -> (String, Vec<String>) {
    (shell(), vec![path.to_str().unwrap().to_owned()])
}

/// The native Windows Ctrl-C termination status is an ordinary signed process exit code.
/// Only Windows has this status code and distinguishes it from Unix signal termination.
#[cfg(windows)]
pub fn control_c_exit() -> (String, Vec<String>, i32) {
    let code = windows_sys::Win32::Foundation::STATUS_CONTROL_C_EXIT;
    (shell(), vec!["-c".into(), format!("exit {code}")], code)
}

/// The standalone shell fixture, even on hosts where the real shell is available.
pub fn stand_in_shell() -> std::path::PathBuf {
    stand_in_bin("sh")
}

/// Native process and environment operations used by the portable session harness.
pub fn session_process_probe() -> &'static str {
    r#"import os, pathlib, subprocess, sys
def launch_session_child(log, name):
    return subprocess.Popen([sys.executable, __file__, 'child', str(log), name])
def session_pid():
    return os.getpid()
def session_home():
    return pathlib.Path.home().as_posix()
def session_directory():
    return pathlib.Path(os.environ['ARTIFACTIZE_SESSION_DIR'])
def exit_session(code):
    sys.exit(code)
"#
}

/// Session descendants keep this file open until killed. Windows deliberately
/// omits FILE_SHARE_DELETE; Unix unlink semantics permit an ordinary open handle.
pub fn session_file_holder() -> &'static str {
    #[cfg(unix)]
    {
        "def hold_session_file(path):\n    return open(path, 'w')\n"
    }
    #[cfg(windows)]
    {
        r#"def hold_session_file(path):
    import ctypes
    from ctypes import wintypes
    create = ctypes.windll.kernel32.CreateFileW
    create.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    create.restype = wintypes.HANDLE
    handle = create(str(path), 0x40000000, 0, None, 2, 0x80, None)
    if handle == wintypes.HANDLE(-1).value: raise ctypes.WinError()
    return handle
"#
    }
}

/// Hold an existing fixture file without delete sharing. Only Windows has this mode;
/// an open Unix file never prevents its name from being unlinked.
#[cfg(windows)]
pub fn hold_without_delete_sharing(path: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(path)
        .unwrap()
}

/// Observe delete-sharing denial without deleting the fixture if containment regresses.
#[cfg(windows)]
pub fn assert_delete_blocked(path: &Path) {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Foundation::ERROR_SHARING_VIOLATION,
        Storage::FileSystem::{DELETE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE},
    };

    let opened = fs::OpenOptions::new()
        .access_mode(DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path);
    assert_eq!(
        opened.unwrap_err().raw_os_error(),
        Some(ERROR_SHARING_VIOLATION as i32),
        "descendant must deny delete sharing"
    );
}

/// The interrupted fixture records a handshake before returning from its handler.
/// Freezing the owner in that handler makes crash-during-cleanup deterministic:
/// its grace-period timer cannot run before the test kills the frozen owner.
pub fn session_interrupt_observer() -> &'static str {
    #[cfg(unix)]
    {
        r#"def observe_session_interrupt(log, freeze):
    import signal
    def interrupted(signum, frame):
        if freeze: os.kill(int((log / 'owner').read_text()), signal.SIGSTOP)
        (log / 'interrupted').write_text(str(os.getpid()))
    signal.signal(signal.SIGTERM, interrupted)
"#
    }
    #[cfg(windows)]
    {
        "def observe_session_interrupt(log, freeze):\n    pass\n"
    }
}

/// Unix directory write permission creates a cleanup I/O failure without keeping
/// any process alive. Windows directory attributes do not model this permission.
pub fn session_directory_write_denial() -> &'static str {
    #[cfg(unix)]
    {
        "def deny_session_directory_writes(path):\n    os.chmod(path, 0o500)\n"
    }
    #[cfg(windows)]
    {
        "def deny_session_directory_writes(path):\n    pass\n"
    }
}

#[cfg(unix)]
pub fn restore_directory_writes(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

/// A relative path that starts with a Windows drive letter, which is an ordinary name elsewhere.
pub fn drive_relative() -> &'static str {
    "C:relative"
}

#[cfg(test)]
mod lifecycle_tests {
    // Filesystem Unix sockets have no Windows equivalent.
    #[cfg(unix)]
    #[test]
    fn special_files_remain_native_entries_under_long_review_output_paths() {
        use std::os::unix::fs::FileTypeExt;
        let root = super::tempdir();
        let directory = root.path().join("review-output-".repeat(12));
        std::fs::create_dir(&directory).unwrap();
        let files = super::special_files(&directory);
        assert!(
            std::fs::symlink_metadata(directory.join("socket"))
                .unwrap()
                .file_type()
                .is_socket()
        );
        assert!(
            std::fs::symlink_metadata(directory.join("pipe"))
                .unwrap()
                .file_type()
                .is_fifo()
        );
        drop(files);
        assert!(!directory.join("socket").exists());
        assert!(!directory.join("pipe").exists());
    }

    use super::*;

    const DIRECTORY_RECORD: &str = "ARTIFACTIZE_TEST_FIXTURE_DIRECTORY_RECORD";
    const PANIC_PROBE: &str = "ARTIFACTIZE_TEST_FIXTURE_PANIC";
    const TERMINAL_READY: &str = "TERMINAL_LIFECYCLE_READY:";

    #[tokio::test]
    async fn owned_test_child_reaps_after_completion_kill_drop_and_unwind() {
        enum End {
            Complete,
            Kill,
            Drop,
            Unwind,
        }
        let root = tempdir();
        let program = compiled_program(
            root.path(),
            "owned-test-child",
            r#"fn main() {
    let mut args = std::env::args_os().skip(1);
    let ready = args.next().unwrap();
    let release = args.next().unwrap();
    std::fs::write(ready, std::process::id().to_string()).unwrap();
    while !std::path::Path::new(&release).exists() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
"#,
        );
        for (case, end) in [End::Complete, End::Kill, End::Drop, End::Unwind]
            .into_iter()
            .enumerate()
        {
            let ready = root.path().join(format!("ready-{case}"));
            let release = root.path().join(format!("release-{case}"));
            let mut command = tokio::process::Command::new(&program);
            command.arg(&ready).arg(&release);
            let mut child = spawn_test(&mut command);
            let pid = child.id();
            let acknowledgment = pid.to_string();
            wait_until(std::time::Duration::from_secs(10), || {
                fs::read_to_string(&ready).is_ok_and(|ready| ready == acknowledgment)
            });
            let birth = start_time(pid);
            assert!(same_process_running(pid, birth));
            match end {
                End::Complete => {
                    fs::write(&release, "").unwrap();
                    assert!(child.wait().await.success());
                    drop(child);
                }
                End::Kill => {
                    child.kill().unwrap();
                    assert!(!child.wait().await.success());
                    drop(child);
                }
                End::Drop => drop(child),
                End::Unwind => {
                    let result = std::panic::catch_unwind(move || {
                        let _child = child;
                        panic!("scripted owned-child fixture unwind");
                    });
                    assert!(result.is_err());
                }
            }
            assert!(gone(pid), "the owned child was not reaped");
            assert!(!same_process_running(pid, birth));
        }
    }

    // Only Unix distinguishes directory search from directory listing permissions.
    #[cfg(unix)]
    #[test]
    fn search_only_directory_restores_access_and_original_permissions() {
        let root = tempdir();
        let marker = root.path().join("marker");
        fs::write(&marker, "known descendant").unwrap();
        // A non-default mode makes exact restoration distinct from granting owner access.
        set_mode(root.path(), 0o750);
        let directory = SearchOnlyDirectory::new(root.path());
        assert_eq!(mode(root.path()), 0o111);
        assert_eq!(fs::read_to_string(&marker).unwrap(), "known descendant");
        // Privileged users bypass directory read permission, but the mode still applies.
        if current_uid() != 0 {
            assert_eq!(
                fs::read_dir(root.path()).unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
        directory.restore();
        fs::write(root.path().join("updated"), "updated marker").unwrap();
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
        directory.restrict();
        assert_eq!(mode(root.path()), 0o111);
        drop(directory);
        assert_eq!(mode(root.path()), 0o750);
        let path = root.path().to_owned();
        root.close().unwrap();
        assert!(!path.exists(), "the restored directory must be cleaned up");
    }

    // Only Unix has the search-only mode whose cleanup access must be restored on unwind.
    #[cfg(unix)]
    #[test]
    fn search_only_directory_restores_before_tempdir_cleanup_on_unwind() {
        let root = tempdir();
        let child = root.path().join("restricted");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("marker"), "known descendant").unwrap();
        set_mode(&child, 0o750);
        let result = std::panic::catch_unwind(|| {
            let _directory = SearchOnlyDirectory::new(&child);
            panic!("scripted search-only fixture unwind");
        });
        assert!(result.is_err());
        assert_eq!(mode(&child), 0o750);
        let path = root.path().to_owned();
        let result = std::panic::catch_unwind(move || {
            let root = root;
            let _directory = SearchOnlyDirectory::new(&child);
            assert!(root.path().exists());
            panic!("scripted fixture owner unwind");
        });
        assert!(result.is_err());
        assert!(!path.exists(), "the unwound fixture must be cleaned up");
    }

    #[test]
    fn cargo_receipt_selects_the_declared_binary_and_its_reported_path() {
        let receipt = br#"{"reason":"build-script-executed"}
{"reason":"compiler-artifact","target":{"name":"other","kind":["bin"]},"executable":"wrong-target"}
{"reason":"compiler-artifact","target":{"name":"probe","kind":["test"]},"executable":"wrong-kind"}
{"reason":"compiler-artifact","target":{"name":"probe","kind":["bin"]},"executable":"custom-target/custom-profile/probe"}
{"reason":"build-finished","success":true}
"#;
        assert_eq!(
            worker::executable_from_receipt(receipt, "probe").unwrap(),
            std::path::PathBuf::from("custom-target/custom-profile/probe")
        );
        assert!(worker::executable_from_receipt(receipt, "absent").is_err());
    }

    #[test]
    fn stand_in_directory_is_removed_after_success_and_test_unwind() {
        let root = tempdir();
        for mode in ["success", "unwind", "exit"] {
            let record = root.path().join(format!("directory-{mode}"));
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["stand_in_directory_exit_probe", "--ignored", "--nocapture"])
                .env(DIRECTORY_RECORD, &record)
                .env(PANIC_PROBE, mode);
            let output = finish(start(&mut command), "stand-in directory exit probe");
            assert_eq!(
                output.status.success(),
                mode != "unwind",
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let directory = std::path::PathBuf::from(fs::read_to_string(record).unwrap());
            assert!(
                !directory.exists(),
                "test process retained its stand-in directory: {}",
                path_text(&directory)
            );
        }
    }

    /// Runs only in a child so its process-global directory can reach the exit boundary.
    #[test]
    #[ignore = "launched by the owned-directory lifecycle test"]
    fn stand_in_directory_exit_probe() {
        let record = std::env::var_os(DIRECTORY_RECORD).expect("directory record");
        let program = stand_in_bin("true");
        assert!(Command::new(&program).status().unwrap().success());
        fs::write(record, path_text(program.parent().unwrap())).unwrap();
        match std::env::var(PANIC_PROBE).unwrap().as_str() {
            "success" => {}
            "unwind" => panic!("scripted test unwind"),
            // Bypass libtest and Rust stack destructors, as Windows ExitProcess does.
            "exit" => std::process::exit(0),
            mode => panic!("unknown fixture exit mode: {mode}"),
        }
    }

    #[test]
    fn pseudo_terminal_drop_reaps_its_child_and_finishes_output_on_unwind() {
        for panicking in [false, true] {
            let (terminal, receiver) = PseudoTerminal::start(
                &std::env::current_exe().unwrap(),
                &["pseudo_terminal_drop_probe", "--ignored", "--nocapture"],
                &[],
                100,
                30,
            );
            let mut output = String::new();
            let pid = loop {
                output.push_str(&String::from_utf8_lossy(
                    &receiver
                        .recv_timeout(patience(std::time::Duration::from_secs(10)))
                        .expect("the terminal child reports readiness"),
                ));
                if let Some((_, ready)) = output.split_once(TERMINAL_READY)
                    && let Some((pid, _)) = ready.split_once('\n')
                {
                    break pid.trim().parse().unwrap();
                }
            };
            let birth = start_time(pid);
            assert!(same_process_running(pid, birth));
            assert!(!same_process_running(pid, birth.wrapping_add(1)));
            // The terminal is consumed inside the closure, with no state reused on unwind.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let terminal = terminal;
                if panicking {
                    panic!("scripted terminal test unwind");
                }
                drop(terminal);
            }));
            assert_eq!(result.is_err(), panicking);
            assert!(gone(pid), "the terminal child was not reaped");
            assert!(!same_process_running(pid, birth));
            // Buffered output may remain, but no serving thread can keep the channel open.
            while receiver.try_recv().is_ok() {}
            assert!(matches!(
                receiver.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Disconnected)
            ));
        }
    }

    /// Runs only inside its parent's owned pseudo-terminal, not the test runner's input.
    #[test]
    #[ignore = "launched by the terminal lifecycle test"]
    fn pseudo_terminal_drop_probe() {
        use std::io::Write;
        println!("{TERMINAL_READY}{}", std::process::id());
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
    }
}
