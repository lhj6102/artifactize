//! Operating-system specifics behind one interface: owner-only files and directories, scoped
//! opens and listings that never follow a link, process trees held until admitted, stop
//! signals, hidden terminal input, and host details.
//!
//! Unix uses modes, `openat` with `O_NOFOLLOW`, `/proc`, and process groups. Windows uses
//! protected owner-only DACLs, handle-relative opens that refuse every reparse point, and
//! kill-on-close Job Objects.

pub(crate) mod environment;
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

pub(crate) use os::{
    CACHE_VARIABLES, CRLF_LINE_ENDINGS, Child, DEFAULT_EDITOR, ENV_NAMES_IGNORE_CASE,
    HOME_VARIABLE, HOME_VARIABLES, HiddenInput, PrivateTempDir, STATE_VARIABLES, SYSTEM_VARIABLES,
    TEMP_VARIABLES, USER_VARIABLES, VISIBLE_INPUT_FALLBACK, create_private_dir,
    create_private_dir_all, editor, exit_signal, file_identity, home_directory, host_name, ipc,
    is_owner_only, is_private_dir, is_private_file, path_from_bytes, private_options,
    private_tempdir_in, process_start_time, resolve_system_aliases, restrict_file, spawn_detached,
    spawn_gated, stop_requested, sync_dir, transient_file_access,
};
pub(crate) use os::{PRIVATE_DIRECTORY, PRIVATE_FILE};

/// A file's identity on its volume. Two open files with equal identities are the same file,
/// whatever paths reached them; a replaced file gets a new identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    volume: u64,
    index: u64,
}

/// How a finished process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessEnd {
    /// It exited with this code.
    Exited(i32),
    /// It was ended without an exit code: on Unix by a signal, when known.
    Signaled(Option<i32>),
}

/// How the process whose `status` this is ended.
pub(crate) fn process_end(status: &std::process::ExitStatus) -> ProcessEnd {
    match status.code() {
        Some(code) => ProcessEnd::Exited(code),
        None => ProcessEnd::Signaled(exit_signal(status)),
    }
}

/// How a finished process ended, for a message: its exit code, or on Unix the signal that
/// ended it, in the system's own wording.
pub(crate) fn exit_description(status: &std::process::ExitStatus) -> String {
    status.to_string()
}

/// Poll contended process-shared file locks without blocking the async runtime;
/// 25 ms keeps session sends and credential refreshes responsive without busy-waiting.
pub(crate) const FILE_LOCK_RETRY_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(25);

pub(crate) use artifactize_tools::files::{
    EntryName, FileKind, canonicalize, entry_kind, link_target, open_directory, open_entry,
    open_no_follow, open_nonblocking, path_text, read_dir,
};

/// Open an existing regular file for reading without following a link in its last
/// component (on Windows, any reparse point), and check what was opened.
pub(crate) fn open_regular(path: &std::path::Path) -> std::io::Result<std::fs::File> {
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

/// Default state directory policy belongs with the platform's environment names.
pub(crate) fn state_directory() -> Option<std::path::PathBuf> {
    resolve_state_directory(
        environment::var("ARTIFACTIZE_STATE_HOME").map(Into::into),
        STATE_VARIABLES
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

/// Render serialized path lists with portable separators, just like single saved paths.
pub(crate) fn serialize_paths<S: serde::Serializer>(
    paths: &[std::path::PathBuf],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    paths
        .iter()
        .map(|path| path_text(path))
        .collect::<Vec<_>>()
        .serialize(serializer)
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

mod interaction;
pub(crate) use interaction::{
    is_within, label, paths_equal, private_anonymous_file, private_temp_file, run_editor,
    selected_editor,
};

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
