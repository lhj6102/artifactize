//! Operating-system specifics behind one interface: owner-only files and directories, scoped
//! opens and listings that never follow a link, process trees held until admitted, stop
//! signals, hidden terminal input, and host details.
//!
//! Unix uses modes, `openat` with `O_NOFOLLOW`, `/proc`, and process groups. Windows uses
//! protected owner-only DACLs, handle-relative opens that refuse every reparse point, and
//! kill-on-close Job Objects.
//!
//! Kept as is (#201): process-wrap 10.0.1 is used for Unix process groups, but its Windows
//! JobObject assigns a job after spawning and waits for the whole job, not just the leader.
//! It exposes neither creation-time job assignment nor our separately owned admission gate;
//! keep the current suspended-spawn/assignment and detached-launch policy on Windows.
//! rpassword 7.5.4 exposes blocking password reads, not a caller-owned mode guard that can
//! restore the terminal when a pending read is cancelled. Keep the echo-only guard and the
//! bounded stdin reader; its restoration contract is tested in `hidden_input`.
//! Program lookup and desktop-opener crate choices are documented in artifactize-tools'
//! platform module, where those implementations live.

pub(crate) mod desktop_app;
pub use desktop_app::launch as launch_desktop;
pub mod desktop_commands;
mod detached;
pub mod environment;
mod executable;
pub use executable::current_executable;
mod hidden_input;
pub(crate) mod install;
pub(crate) mod net;
pub(crate) mod path_serde;
pub(crate) mod paths;
pub(crate) mod program;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as os;
#[cfg(windows)]
use windows as os;

pub(crate) use detached::{DetachedChild, spawn_detached, spawn_run, spawn_watched};
pub use hidden_input::{SecretInput, secret_input, stdin_is_terminal};
pub use os::HiddenInput;
pub use os::stop_requested;
pub(crate) use os::{
    CRLF_LINE_ENDINGS, Child, ENV_NAMES_IGNORE_CASE, PrivateTempDir, TEMP_VARIABLES,
    USER_VARIABLES, create_private_dir, create_private_dir_all, file_identity, hide_console_window,
    home_directory, host_name, ipc, is_owner_only, is_private_dir, is_private_file,
    kill_saved_group, kill_saved_session, open_verified_executable, path_from_bytes,
    private_named_dir, private_options, private_tempdir_in, process_start_time,
    release_free_memory, remove_tree, resolve_system_aliases, restrict_executable_file,
    restrict_file, running_process_start_time, spawn_gated, spawn_now, spawn_verified,
    spawn_windowless, sync_dir, transient_file_access, venv_bin_dir,
};
pub(crate) use os::{PRIVATE_DIRECTORY, PRIVATE_FILE};

/// A share in a large read's memory, such as an opened Run's: everyone who keeps part of the
/// read keeps a share, declared after what it keeps so that it is dropped last. When the last
/// share goes, the read's memory has been freed, and it is given back to the system then, once
/// (see [`release_free_memory`]), rather than staying with a long-lived process.
#[derive(Debug, Clone)]
pub struct Footprint {
    _release: std::sync::Arc<Release>,
}

impl Footprint {
    pub(crate) fn new() -> Self {
        Self {
            _release: std::sync::Arc::new(Release),
        }
    }
}

#[derive(Debug)]
struct Release;

impl Drop for Release {
    fn drop(&mut self) {
        release_free_memory();
    }
}

/// A file's identity on its volume. Two open files with equal identities are the same file,
/// whatever paths reached them; a replaced file gets a new identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FileIdentity {
    volume: u64,
    index: u64,
}

pub use artifactize_tools::launch::{ProcessEnd, ProcessOutcome};

/// Poll contended process-shared file locks without blocking the async runtime;
/// 25 ms keeps session sends and credential refreshes responsive without busy-waiting.
pub(crate) const FILE_LOCK_RETRY_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(25);

pub(crate) use artifactize_tools::files::{
    EntryName, FileKind, canonicalize, entry_kind, link_target, open_directory, open_entry,
    open_no_follow, read_dir,
};
pub use artifactize_tools::files::{current_directory, open_nonblocking, path_text};

/// The absolute local folder a `file:` URI names, by this OS's path rules (a drive letter
/// or UNC host on Windows); `None` for another scheme, a remote host or a relative path.
pub fn file_uri_path(uri: &str) -> Option<std::path::PathBuf> {
    url::Url::parse(uri)
        .ok()?
        .to_file_path()
        .ok()
        .filter(|path| path.is_absolute())
}

/// The absolute folder a caller names; a relative one would resolve against this
/// process's working directory, which the caller cannot see.
pub fn absolute_folder(text: &str) -> Option<std::path::PathBuf> {
    artifactize_tools::files::path_from_text(text)
        .ok()
        .filter(|path| path.is_absolute())
}

/// Whether lookup resolves to a directory, following native links and aliases.
pub fn is_directory(path: &std::path::Path) -> bool {
    path.is_dir()
}

/// Whether lookup resolves to an existing target. Other I/O errors leave it unknown.
pub fn target_exists(path: &std::path::Path) -> std::io::Result<bool> {
    paths::target_exists(path)
}

/// Whether native lookup reaches a regular file, following links and honoring the volume's
/// name matching. Missing or inaccessible entries do not count.
pub fn is_regular_file(path: &std::path::Path) -> bool {
    path.is_file()
}

/// Open a caller-selected file, following links but never blocking on a special file.
/// Returns `None` when the opened handle is not a regular file.
pub fn open_regular_input(path: &std::path::Path) -> std::io::Result<Option<std::fs::File>> {
    let file = open_nonblocking(path)?;
    Ok(file.metadata()?.is_file().then_some(file))
}

/// Open an existing regular file for reading without following a link in its last
/// component (on Windows, any reparse point), and check what was opened.
pub fn open_regular(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let file = open_no_follow(std::fs::OpenOptions::new().read(true), path)?;
    if file.metadata()?.is_file() {
        Ok(file)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file",
        ))
    }
}

/// The civil day on this machine at `instant`, using its offset at that instant (including
/// daylight-saving changes). If the system cannot describe it, use the UTC date.
pub(crate) fn local_day(instant: time::OffsetDateTime) -> time::Date {
    let offset = chrono::DateTime::from_timestamp(instant.unix_timestamp(), instant.nanosecond())
        .map(|utc| utc.with_timezone(&chrono::Local).offset().local_minus_utc())
        .and_then(|seconds| time::UtcOffset::from_whole_seconds(seconds).ok())
        .unwrap_or(time::UtcOffset::UTC);
    instant.to_offset(offset).date()
}

/// Captured program text with terminal control codes removed.
pub(crate) fn clean_program_text(bytes: &[u8]) -> String {
    String::from_utf8(crate::runtime::clean_output(bytes)).expect("clean output is UTF-8")
}

/// A program's textual output, lossy only where its bytes are not UTF-8.
pub fn output_text(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

/// A program's textual output as lines ending in `\n`, whichever ending the program wrote.
pub fn output_lines(bytes: &[u8]) -> String {
    output_text(bytes).replace("\r\n", "\n")
}

/// An OS string (an argument, a file name) as Unicode text: `None` when it is not (non-UTF-8
/// bytes on Unix, an unpaired UTF-16 unit on Windows).
pub fn os_text(value: &std::ffi::OsStr) -> Option<&str> {
    value.to_str()
}

/// A filesystem name for display when a lossless path key is not needed.
pub(crate) fn display_name(value: &std::ffi::OsStr) -> String {
    value.to_string_lossy().into_owned()
}

/// A captured output line, accepting either common ending even when the program
/// (rather than the host) explicitly chose which ending to emit.
pub(crate) fn output_line(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

/// Decode one native tool path line. Unix paths may end in a literal carriage return;
/// only Windows accepts CRLF as the line terminator rather than part of the path.
pub(crate) fn path_from_line(bytes: &[u8]) -> std::path::PathBuf {
    let path = if CRLF_LINE_ENDINGS {
        bytes
            .strip_suffix(b"\r\n")
            .or_else(|| bytes.strip_suffix(b"\n"))
            .unwrap_or(bytes)
    } else {
        bytes.strip_suffix(b"\n").unwrap_or(bytes)
    };
    path_from_bytes(path)
}

/// This system's directory for temporary files.
pub(crate) fn temporary_directory() -> std::path::PathBuf {
    std::env::temp_dir()
}

const STATE_HOME_OVERRIDE: &str = "ARTIFACTIZE_STATE_HOME";

/// The environment variables consulted for state storage, in precedence order.
pub(crate) fn state_home_variables() -> impl Iterator<Item = &'static str> {
    std::iter::once(STATE_HOME_OVERRIDE)
        .chain(os::STATE_VARIABLES.iter().copied())
        .chain(os::HOME_VARIABLES.iter().copied())
}

/// Default state directory policy belongs with the platform's environment names.
pub(crate) fn state_directory() -> Option<std::path::PathBuf> {
    resolve_state_directory(
        environment::var(STATE_HOME_OVERRIDE).map(Into::into),
        os::STATE_VARIABLES
            .iter()
            .find_map(|name| environment::var(name).filter(|v| !v.is_empty()))
            .map(Into::into),
        home_directory(),
    )
}

fn resolve_state_directory(
    artifactize: Option<std::path::PathBuf>,
    state: Option<std::path::PathBuf>,
    home: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    artifactize
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            state
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.join("artifactize"))
        })
        .or_else(|| {
            home.filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.join(".local/state/artifactize"))
        })
}

/// Printable user metadata, not an authenticated principal.
pub(crate) fn user_name() -> Option<String> {
    USER_VARIABLES
        .iter()
        .find_map(|name| environment::var_text(name).filter(|value| !value.is_empty()))
}

/// Strip one platform-supported script line ending and name the accepted endings.
pub(crate) fn fingerprint_line_ending(stdout: &[u8]) -> (&[u8], &'static str) {
    if CRLF_LINE_ENDINGS {
        (
            stdout
                .strip_suffix(b"\r\n")
                .or_else(|| stdout.strip_suffix(b"\n"))
                .unwrap_or(stdout),
            "LF or CRLF",
        )
    } else {
        (stdout.strip_suffix(b"\n").unwrap_or(stdout), "LF")
    }
}

/// Classify the final component without following links or Windows reparse points.
pub(crate) fn path_kind(path: &std::path::Path) -> std::io::Result<FileKind> {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => entry_kind(&open_directory(parent)?, name),
        _ => open_directory(path).map(|_| FileKind::Directory),
    }
}

/// Decode an authored program before path inspection or native lookup. Legacy declarations
/// can use a native Windows verbatim spelling, whose literal slashes must not be translated.
pub(crate) fn program_path(command: &str) -> std::io::Result<std::path::PathBuf> {
    #[cfg(windows)]
    if command.starts_with(r"\\?\") {
        return Ok(command.into());
    }
    artifactize_tools::files::path_from_text(command)
}

/// A declared program as reversible path text, using `/` in its readable spelling.
pub(crate) fn program_text(command: &str) -> String {
    program_path(command).map_or_else(|_| command.to_owned(), |path| path_text(&path))
}

/// Serialize a declared program as [`program_text`].
pub(crate) fn serialize_program<S: serde::Serializer>(
    command: &str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&program_text(command))
}

/// Render serialized path lists with portable separators, just like single saved paths.
pub(crate) fn serialize_paths<S: serde::Serializer>(
    paths: &[std::path::PathBuf],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    paths
        .iter()
        .map(path_text)
        .collect::<Vec<_>>()
        .serialize(serializer)
}

/// Normalize an external path reported by an ACP agent so stored and displayed paths always
/// use `/` separators on every OS (`P-PATHS`).
pub(crate) fn normalize_external_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    path_text(std::path::Path::new(trimmed)).replace('\\', "/")
}

/// Official ACP Registry platform key for the current OS and CPU architecture (`P-ONE-LAYER`).
pub(crate) fn registry_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("linux", "x86_64") => "linux-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => "unknown",
    }
}

/// Write an owner-only executable binary atomically into `target_dir` through the platform layer
/// (`P-FILES`), setting execute permissions on Unix and owner-only DACL on Windows.
pub(crate) fn install_executable(
    target_dir: &std::path::Path,
    name: &str,
    bytes: &[u8],
) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(target_dir)?;
    restrict_executable_file(file.as_file())?;
    file.write_all(bytes)?;
    file.flush()?;
    file.as_file().sync_all()?;
    let final_path = target_dir.join(name);
    paths::replace_open_file(file, &final_path)?;
    sync_dir(target_dir)?;
    Ok(final_path)
}

#[cfg(test)]
mod process_tests;
#[cfg(test)]
pub(crate) use process_tests::pipe_grace_armed;

#[cfg(test)]
mod program_tests {
    #[test]
    fn a_declared_program_is_written_with_forward_slashes() {
        assert_eq!(super::program_text("pnpm"), "pnpm");
        assert_eq!(super::program_text("tools/run.sh"), "tools/run.sh");
        // Windows interprets backslashes as separators; Unix permits them in filenames.
        #[cfg(windows)]
        assert_eq!(super::program_text(r"tools\run.cmd"), "tools/run.cmd");
    }
}

#[cfg(test)]
mod private_directory_tests {
    use super::*;

    #[test]
    fn kept_temporary_directory_transfers_cleanup_responsibility() {
        let parent = crate::test_os::tempdir();
        let directory = private_tempdir_in("kept-", parent.path()).unwrap();
        let path = directory.keep();
        assert!(path.is_dir());
        assert!(is_private_dir(&path).unwrap());
        remove_tree(&path).unwrap();
        assert!(!path.exists());
    }
}

#[cfg(test)]
mod state_directory_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn state_home_precedence_and_empty_values() {
        let artifactize = PathBuf::from("custom/state");
        let state = PathBuf::from("state");
        let home = PathBuf::from("reviewer");
        assert_eq!(
            resolve_state_directory(
                Some(artifactize.clone()),
                Some(state.clone()),
                Some(home.clone())
            ),
            Some(artifactize)
        );
        assert_eq!(
            resolve_state_directory(None, Some(state.clone()), Some(home.clone())),
            Some(state.join("artifactize"))
        );
        assert_eq!(
            resolve_state_directory(None, None, Some(home.clone())),
            Some(home.join(".local/state/artifactize"))
        );
        let empty = Some(PathBuf::new());
        assert_eq!(
            resolve_state_directory(empty.clone(), empty.clone(), Some(home.clone())),
            Some(home.join(".local/state/artifactize"))
        );
        assert_eq!(
            resolve_state_directory(empty.clone(), empty.clone(), empty),
            None
        );
        assert_eq!(resolve_state_directory(None, None, None), None);
    }
}

mod console;
pub use console::pause_after_help;

mod interaction;
pub use interaction::paths_equal;
pub(crate) use interaction::{is_within, label, private_anonymous_file};

/// Whether an entry exists without following links (including dangling links).
pub(crate) fn entry_exists(path: &std::path::Path) -> std::io::Result<bool> {
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    let directory = match open_directory(parent) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let Some(name) = path.file_name() else {
        return Ok(false);
    };
    match entry_kind(&directory, name) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// A no-follow stat of `path`. Unlike [`entry_exists`], this never opens `path`'s parent, so
/// it still works on a search-only ancestor (executable but unreadable): opening a directory
/// needs read permission on it, while naming one of its entries by an exact path needs only
/// search (execute) permission on every ancestor up to it.
pub(crate) fn marker_metadata(path: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
    path.symlink_metadata()
}

#[cfg(test)]
pub(crate) fn successful_outcome() -> ProcessOutcome {
    ProcessOutcome::from_native(std::process::ExitStatus::default())
}

#[cfg(test)]
mod stop_tests {
    use super::*;
    use std::{path::PathBuf, time::Duration};

    const READY: &str = "ARTIFACTIZE_TEST_STOP_PROBE_READY";
    const WATCHDOG: Duration = Duration::from_secs(15);

    #[tokio::test]
    async fn a_terminal_interrupt_completes_stop_requested() {
        let root = crate::test_os::tempdir();
        let ready = root.path().join("ready");
        let mut child = crate::test_os::test_command(
            "platform::stop_tests::terminal_interrupt_probe",
            &[(READY, ready.clone().into_os_string())],
        );
        crate::test_os::new_group(&mut child);
        let mut child = crate::test_os::spawn_test(&mut tokio::process::Command::from(child));
        crate::test_os::wait_until(WATCHDOG, || ready.is_file());
        crate::test_os::interrupt(child.id());
        assert!(child.wait().await.success());
    }

    #[tokio::test]
    async fn terminal_interrupt_probe() {
        let Some(ready) = crate::test_os::environment_variable(READY) else {
            return;
        };
        let stop = stop_requested().unwrap();
        std::fs::write(PathBuf::from(ready), "subscribed").unwrap();
        stop.await;
    }
}
