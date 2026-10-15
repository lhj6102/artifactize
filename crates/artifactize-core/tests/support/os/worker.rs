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

extern "C" fn cleanup() {
    let executable = OWNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    drop(executable);
}

// Register with the executable's own CRT, including MSVC/MinGW and crt-static.
#[cfg(windows)]
unsafe extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
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
            // SAFETY: the same lifetime contract applies to the Windows CRT exit table.
            #[cfg(windows)]
            let registered = unsafe { atexit(cleanup) };
            if registered != 0 {
                return Err("Fixture executable exit cleanup could not be registered".into());
            }
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
