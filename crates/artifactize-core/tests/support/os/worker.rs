//! The executable a library fixture uses for its durable worker is built from this checkout.
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
};

/// Capture a prepared worker fixture command through the shared native process boundary.
pub fn output(command: &mut Command) -> io::Result<std::process::Output> {
    command.output()
}

/// Python used by the native fixture probes and the isolated executable copier on every CI OS.
pub fn python_program() -> &'static str {
    "python3"
}

/// Portable path text for executable ownership diagnostics, without changing native argv.
pub fn path_text(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        text.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        text.into_owned()
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "reason")]
enum CargoMessage {
    #[serde(rename = "compiler-artifact")]
    Artifact {
        target: CargoTarget,
        executable: Option<PathBuf>,
    },
    #[serde(other)]
    Other,
}

#[derive(serde::Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
}

/// A private executable inode: another Cargo build may replace its published source,
/// but cannot unlink the program this fixture and its durable workers are running.
pub struct Executable {
    path: PathBuf,
    _directory: tempfile::TempDir,
}

impl Executable {
    pub fn copy_from(source: &Path) -> Result<Self, String> {
        let root =
            option_env!("CARGO_TARGET_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let directory = tempfile::Builder::new()
            .prefix("artifactize-worker-")
            .tempdir_in(root)
            .map_err(|error| error.to_string())?;
        let path = directory.path().join(
            source
                .file_name()
                .ok_or_else(|| "Fixture executable has no filename".to_owned())?,
        );
        // Copy, not hard-link: Cargo owns the source inode as well as its pathname.
        // Only this single-threaded child opens the candidate writable. A parent-side
        // fs::copy lets unrelated concurrent forks inherit its writer even with CLOEXEC:
        // after the parent closes it, exec can still fail with ETXTBUSY until those
        // children exec. Reaping the copier releases every writer before publication.
        let copied = Command::new(python_program())
            .args([
                "-I",
                "-c",
                "import shutil, sys; shutil.copy(sys.argv[1], sys.argv[2])",
            ])
            .arg(source)
            .arg(&path)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("Starting the fixture executable copier: {error}"))?;
        if !copied.status.success() {
            return Err(format!(
                "Fixture executable copy failed ({}): {}",
                copied.status,
                String::from_utf8_lossy(&copied.stderr)
            ));
        }
        Ok(Self {
            path,
            _directory: directory,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

// Several fixtures in one test binary share the candidate until all their workers end.
// Unlike a leaked TempDir in a OnceLock, this owner also cleans up on libtest failure.
static OWNED: Mutex<Option<Executable>> = Mutex::new(None);

#[cfg(unix)]
extern "C" fn cleanup() {
    let executable = OWNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    drop(executable);
}

/// Rust exits Windows processes with ExitProcess, bypassing the executable's CRT exit
/// table. A separate owner waits on this exact live parent's native handle instead. It
/// inherits stderr so a caller collecting the parent's output also observes cleanup before
/// EOF. The ready handshake happens before any fixture path is published.
#[cfg(windows)]
pub fn cleanup_after_exit(directory: &Path) -> Result<(), String> {
    use std::io::BufRead;
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };

    let mut creation = FILETIME::default();
    let (mut exit, mut kernel, mut user) = (creation, creation, creation);
    // SAFETY: the current-process pseudo-handle needs no closing; each out pointer is
    // valid and the returned birth time is read only after a successful query.
    if unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(format!(
            "Reading fixture owner identity: {}",
            io::Error::last_os_error()
        ));
    }
    let birth = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);

    const CLEANUP: &str = r#"
import ctypes
from ctypes import wintypes
import shutil
import sys

kernel = ctypes.WinDLL('kernel32', use_last_error=True)
open_process = kernel.OpenProcess
open_process.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
open_process.restype = wintypes.HANDLE
wait = kernel.WaitForSingleObject
wait.argtypes = [wintypes.HANDLE, wintypes.DWORD]
wait.restype = wintypes.DWORD
close = kernel.CloseHandle
close.argtypes = [wintypes.HANDLE]
close.restype = wintypes.BOOL
times = kernel.GetProcessTimes
times.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
times.restype = wintypes.BOOL
SYNCHRONIZE = 0x00100000
PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
INFINITE = 0xffffffff
WAIT_OBJECT_0 = 0

try:
    parent = open_process(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, False, int(sys.argv[1]))
    if not parent:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        creation, exit, kernel_time, user = [wintypes.FILETIME() for _ in range(4)]
        if not times(parent, ctypes.byref(creation), ctypes.byref(exit), ctypes.byref(kernel_time), ctypes.byref(user)):
            raise ctypes.WinError(ctypes.get_last_error())
        birth = (creation.dwHighDateTime << 32) | creation.dwLowDateTime
        if birth != int(sys.argv[2]):
            raise RuntimeError('Fixture owner process identity changed before registration')
        # Publish readiness only after pinning the exact parent identity. Its native
        # handle cannot be retargeted by PID reuse, including after an abrupt exit.
        print('READY', flush=True)
        if wait(parent, INFINITE) != WAIT_OBJECT_0:
            raise ctypes.WinError(ctypes.get_last_error())
    finally:
        close(parent)
    try:
        shutil.rmtree(sys.argv[3])
    except FileNotFoundError as error:
        # Initialization can unwind after registration but before publication; its
        # local TempDir then removes this exact root, not a missing file inside it.
        if error.filename != sys.argv[3]:
            raise
except Exception:
    print('Fixture exit cleanup failed; retained root: ' + sys.argv[3], file=sys.stderr)
    raise
"#;
    let directory = fs::canonicalize(directory).map_err(|error| error.to_string())?;
    let mut child = Command::new(python_program())
        .args(["-I", "-c", CLEANUP])
        .arg(std::process::id().to_string())
        .arg(birth.to_string())
        .arg(&directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("Starting fixture exit cleanup: {error}"))?;
    let mut ready = String::new();
    let read = std::io::BufReader::new(child.stdout.take().expect("cleanup readiness pipe"))
        .read_line(&mut ready);
    if read.is_err() || ready.trim_end() != "READY" {
        // No path was published. Reap the failed registration before its local owner
        // removes the directory; never leave a half-registered cleanup process behind.
        let _ = child.kill();
        let status = child.wait();
        return Err(format!(
            "Fixture exit cleanup registration failed for {}: {read:?}, {ready:?}, {status:?}",
            path_text(&directory)
        ));
    }
    // Dropping Child closes only our process handle, not the cleanup process. The
    // cleanup owns exactly this root after the native parent handle becomes signalled.
    Ok(())
}

pub fn executable() -> Result<&'static Path, String> {
    static BUILT: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let candidate = Executable::copy_from(&source_executable()?)?;
            // SAFETY: this OnceLock registers once; cleanup only drops this test process's
            // candidate after its fixtures have released their callers and durable workers.
            #[cfg(unix)]
            let registered = unsafe { libc::atexit(cleanup) };
            #[cfg(unix)]
            if registered != 0 {
                return Err("Fixture executable exit cleanup could not be registered".into());
            }
            #[cfg(windows)]
            cleanup_after_exit(candidate.path().parent().expect("owned executable root"))?;
            let path = candidate.path().to_owned();
            *OWNED.lock().unwrap() = Some(candidate);
            Ok(path)
        })
        .as_deref()
        .map_err(Clone::clone)
}

fn source_executable() -> Result<PathBuf, String> {
    // Cargo names the executable for its own package's integration tests. Library
    // tests instead declare and build the sibling CLI, never discover an installed one.
    if let Some(path) = option_env!("CARGO_BIN_EXE_artifactize") {
        return Ok(PathBuf::from(path));
    }
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace crate parent")
        .join("artifactize")
        .join("Cargo.toml");
    let built = Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "--locked",
            "--bin",
            "artifactize",
            "--message-format=json-render-diagnostics",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| error.to_string())?;
    if !built.status.success() {
        return Err(format!("Fixture CLI build failed: {}", built.status));
    }
    executable_from_receipt(&built.stdout, "artifactize")
}

/// Read Cargo's executable receipt, including target-dir, target triples, profiles and
/// native suffixes. The fixture never guesses a path from another executable's location.
pub fn executable_from_receipt(receipt: &[u8], binary: &str) -> Result<PathBuf, String> {
    for line in receipt
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        if let CargoMessage::Artifact {
            target,
            executable: Some(path),
        } = serde_json::from_slice(line).map_err(|error| error.to_string())?
            && target.name == binary
            && target.kind.iter().any(|kind| kind == "bin")
        {
            return Ok(path);
        }
    }
    Err(format!(
        "Cargo did not report the {binary} fixture executable"
    ))
}
