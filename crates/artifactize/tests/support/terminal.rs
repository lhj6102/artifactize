//! The private directory of a worktree terminal's endpoint, named from the public namespace
//! spelling, the OS user and the worktree's canonical Git directory. It outlives its host by
//! design (the election lock is never unlinked), so a fixture removes it once its host ended.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The hex characters of the digest that name the shared runtime directory.
const RUNTIME_ID_PREFIX_HEX_CHARS: usize = 24;

pub fn endpoint_directory(worktree: &Path) -> PathBuf {
    let identity = artifactize_core::repository::worktree_id(worktree).unwrap();
    let state = artifactize_core::workspace::canonical_target(&identity).unwrap();
    let user = user();
    let mut digest = Sha256::new();
    digest.update(b"terminal\0");
    digest.update(user.as_bytes());
    digest.update([0]);
    digest.update(state.as_os_str().as_encoded_bytes());
    let hex: String = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    directory(&user, &hex[..RUNTIME_ID_PREFIX_HEX_CHARS])
}

#[cfg(unix)]
fn user() -> String {
    crate::support::os::current_uid().to_string()
}

/// The current token's SID, as `whoami` prints it.
#[cfg(windows)]
fn user() -> String {
    let output = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let line = String::from_utf8(output.stdout).unwrap();
    line.trim()
        .rsplit(',')
        .next()
        .unwrap()
        .trim_matches('"')
        .to_owned()
}

#[cfg(unix)]
fn directory(user: &str, prefix: &str) -> PathBuf {
    let root = if cfg!(target_os = "macos") {
        "/private/tmp"
    } else {
        "/tmp"
    };
    Path::new(root).join(format!("artifactize-ipc-{user}-{prefix}"))
}

#[cfg(windows)]
fn directory(_user: &str, prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("artifactize-ipc-{prefix}"))
}
