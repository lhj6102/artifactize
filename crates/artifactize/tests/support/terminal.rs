//! The private directory of a worktree terminal's endpoint, named from the public namespace
//! spelling, the OS user and the worktree's canonical Git directory. It outlives its host by
//! design (the election lock is never unlinked), so a fixture removes it once its host ended.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::support::os;

/// The hex characters of the digest that name the shared runtime directory.
const RUNTIME_ID_PREFIX_HEX_CHARS: usize = 24;

pub fn endpoint_directory(worktree: &Path) -> PathBuf {
    let identity = artifactize_core::repository::worktree_id(worktree).unwrap();
    let state = artifactize_core::workspace::canonical_target(&identity).unwrap();
    let user = os::ipc_user();
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
    os::ipc_directory(&user, &hex[..RUNTIME_ID_PREFIX_HEX_CHARS])
}
