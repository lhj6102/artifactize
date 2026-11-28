//! Explicit Workspace Trust gate for ACP agents (`$STATE/chat/trust.json`).
//!
//! Because ACP agents run with `cwd = session.worktree` and native coding tools, they can read
//! repository-committed hooks, settings and MCP configs on startup before any permission prompt.
//! Consent binds `(agent_id, TrustScope, LauncherFingerprint)` and hashes the resolved
//! executable's bytes directly at the Trust gate before every spawn or warm reuse.

use std::{
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    auth::storage::{Storage, Tokens},
    platform, repository,
    types::Timestamp,
};

use super::AgentId;

const TRUST_FILE: &str = "trust.json";
const TRUST_LOCK: &str = "trust";
/// Streaming buffer for direct SHA-256 hashing of resolved ACP executables.
const HASH_CHUNK_BYTES: usize = 64 * 1024;
/// Current schema version of `$STATE/chat/trust.json`.
const TRUST_SCHEMA_VERSION: u32 = 1;

/// What repository or directory the user explicitly trusted for an ACP agent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TrustScope {
    /// Canonical git `common_dir`: covers all branches and worktrees of this repository.
    GitRepo {
        #[serde(with = "crate::platform::path_serde")]
        common_dir: PathBuf,
    },
    /// Canonical absolute path for non-git folders (never collapses into `Identity::default()`).
    Directory {
        #[serde(with = "crate::platform::path_serde")]
        canonical_path: PathBuf,
    },
}

impl TrustScope {
    /// Derive the exact trust scope for a worktree and its git identity.
    pub fn for_worktree(worktree: &Path, identity: &repository::Identity) -> Result<Self, String> {
        let canonical =
            crate::workspace::canonical_target(worktree).map_err(|error| error.to_string())?;
        if let Some(common_dir) = &identity.common_dir {
            let common = crate::workspace::canonical_target(common_dir)
                .map_err(|error| error.to_string())?;
            Ok(Self::GitRepo { common_dir: common })
        } else {
            Ok(Self::Directory {
                canonical_path: canonical,
            })
        }
    }

    pub fn display_label(&self) -> String {
        match self {
            Self::GitRepo { common_dir } => {
                format!("Git repository ({})", platform::path_text(common_dir))
            }
            Self::Directory { canonical_path } => platform::path_text(canonical_path),
        }
    }

    pub fn disclosure(&self) -> &'static str {
        match self {
            Self::GitRepo { .. } => {
                "Trusting a Git repository covers all branches and worktrees of that repository; checking out an untrusted branch will not re-prompt."
            }
            Self::Directory { .. } => {
                "Trusting this directory allows the ACP agent to run natively inside it and read any local configuration files before prompting."
            }
        }
    }
}

/// Complete executable + argument identity bound to a workspace trust entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherFingerprint {
    #[serde(with = "crate::platform::path_serde")]
    pub resolved_executable: PathBuf,
    pub executable_sha256: String,
    pub args_hash: String,
}

impl LauncherFingerprint {
    /// Compute the launcher fingerprint by hashing the executable bytes directly (no mtime/size cache).
    pub fn compute(resolved_executable: &Path, args: &[String]) -> Result<Self, String> {
        let canonical = platform::canonicalize(resolved_executable).map_err(|error| {
            format!(
                "Cannot resolve ACP executable {}: {error}.",
                platform::path_text(resolved_executable)
            )
        })?;
        let executable_sha256 = hash_executable(&canonical)?;
        let args_hash = hash_args(args);
        Ok(Self {
            resolved_executable: canonical,
            executable_sha256,
            args_hash,
        })
    }

    /// Compute the launcher fingerprint directly from an already-opened executable handle so the
    /// caller can bind spawn to that exact verified handle.
    pub(crate) fn compute_from_file(
        resolved_executable: PathBuf,
        file: &mut std::fs::File,
        args: &[String],
    ) -> Result<Self, String> {
        let executable_sha256 = hash_file(file, &resolved_executable)?;
        let args_hash = hash_args(args);
        Ok(Self {
            resolved_executable,
            executable_sha256,
            args_hash,
        })
    }

    /// Combine with `auth_generation` into the `AcpBinding::runtime_fingerprint` key.
    pub fn runtime_fingerprint(&self, auth_generation: u64, environment: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(platform::path_text(&self.resolved_executable).as_bytes());
        hasher.update([0]);
        hasher.update(self.executable_sha256.as_bytes());
        hasher.update([0]);
        hasher.update(self.args_hash.as_bytes());
        hasher.update([0]);
        hasher.update(auth_generation.to_le_bytes());
        hasher.update([0]);
        hasher.update(environment.as_bytes());
        hex_digest(hasher.finalize().as_slice())
    }
}

/// One persisted consent record in `$STATE/chat/trust.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustRecord {
    pub agent_id: AgentId,
    pub scope: TrustScope,
    pub fingerprint: LauncherFingerprint,
    pub granted_at: Timestamp,
}

/// Details returned when a turn or warm reuse requires explicit workspace trust consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustRequirement {
    pub agent_id: AgentId,
    pub agent_name: String,
    pub scope: TrustScope,
    pub fingerprint: LauncherFingerprint,
    pub disclosure: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedTrust {
    version: u32,
    #[serde(default)]
    entries: Vec<TrustRecord>,
}

impl Default for SavedTrust {
    fn default() -> Self {
        Self {
            version: TRUST_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

/// Resolve an ACP program from an empty private tempdir (never from `worktree`) and, for
/// built-in presets, reject executables located inside `worktree`.
pub fn resolve_executable(
    program: &str,
    worktree: Option<&Path>,
    reject_inside_worktree: bool,
) -> Result<PathBuf, String> {
    let probe_dir =
        platform::private_tempdir_in("artifactize-resolve-", &platform::temporary_directory())
            .map_err(|error| format!("Cannot create executable resolution tempdir: {error}."))?;
    let program_path = platform::program_path(program)
        .map_err(|error| format!("Invalid ACP program path {program:?}: {error}."))?;
    let resolved = platform::program::resolve(&program_path, probe_dir.path())
        .map_err(|error| format!("Cannot find ACP executable {program:?}: {error}."))?;
    let canonical = platform::canonicalize(&resolved).map_err(|error| {
        format!(
            "Cannot canonicalize ACP executable {}: {error}.",
            platform::path_text(&resolved)
        )
    })?;
    if reject_inside_worktree && let Some(worktree) = worktree {
        outside_worktree(&canonical, worktree)?;
    }
    Ok(canonical)
}

/// Refuse a built-in preset's resolved executable `canonical` inside `worktree` or its enclosing
/// Git worktree root: the repository's own files never run as an agent before Workspace Trust, a
/// probe included.
pub fn outside_worktree(canonical: &Path, worktree: &Path) -> Result<(), String> {
    let canonical_worktree =
        crate::workspace::canonical_target(worktree).map_err(|error| error.to_string())?;
    if platform::is_within(canonical, &canonical_worktree) {
        return Err(format!(
            "Refusing to launch built-in ACP preset executable {} located inside the workspace {}.",
            platform::path_text(canonical),
            platform::path_text(&canonical_worktree),
        ));
    }
    if let Some(root) = repository::identify(&canonical_worktree).worktree_path {
        let canonical_root = crate::workspace::canonical_target(&root).unwrap_or(root);
        if platform::is_within(canonical, &canonical_root) {
            return Err(format!(
                "Refusing to launch built-in ACP preset executable {} located inside the workspace {}.",
                platform::path_text(canonical),
                platform::path_text(&canonical_root),
            ));
        }
    }
    Ok(())
}

/// Hash the executable's bytes directly via SHA-256 (never `(mtime, size)`).
pub fn hash_executable(path: &Path) -> Result<String, String> {
    let mut file = platform::open_regular_input(path)
        .map_err(|error| {
            format!(
                "Cannot open ACP executable {}: {error}.",
                platform::path_text(path)
            )
        })?
        .ok_or_else(|| {
            format!(
                "ACP executable {} is not a regular file.",
                platform::path_text(path)
            )
        })?;
    hash_file(&mut file, path)
}

/// Hash an already-opened regular executable file directly via SHA-256 and rewind it to offset 0.
pub(crate) fn hash_file(file: &mut std::fs::File, path: &Path) -> Result<String, String> {
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        format!(
            "Cannot seek ACP executable {}: {error}.",
            platform::path_text(path)
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; HASH_CHUNK_BYTES];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            format!(
                "Cannot read ACP executable {}: {error}.",
                platform::path_text(path)
            )
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0)).map_err(|error| {
        format!(
            "Cannot rewind ACP executable {}: {error}.",
            platform::path_text(path)
        )
    })?;
    Ok(hex_digest(hasher.finalize().as_slice()))
}

/// Pure deterministic digest over an argument vector.
pub fn hash_args(args: &[String]) -> String {
    let mut hasher = Sha256::new();
    for arg in args {
        hasher.update((arg.len() as u64).to_le_bytes());
        hasher.update(arg.as_bytes());
    }
    hex_digest(hasher.finalize().as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Read all saved trust records from `$STATE/chat/trust.json`.
pub fn list_trust(state: Option<&Path>, repo: Option<&Path>) -> Result<Vec<TrustRecord>, String> {
    let storage = Storage::inspect(state, repo, Tokens::ChatTrust)?;
    Ok(storage
        .read::<SavedTrust>(TRUST_FILE)?
        .unwrap_or_default()
        .entries)
}

/// Check whether `(agent_id, scope, fingerprint)` already has explicit consent.
pub fn check_trust(
    state: Option<&Path>,
    repo: Option<&Path>,
    agent_id: &AgentId,
    agent_name: &str,
    scope: &TrustScope,
    fingerprint: &LauncherFingerprint,
) -> Result<Result<(), TrustRequirement>, String> {
    let entries = list_trust(state, repo)?;
    if entries.iter().any(|entry| {
        &entry.agent_id == agent_id && &entry.scope == scope && &entry.fingerprint == fingerprint
    }) {
        return Ok(Ok(()));
    }
    let changed_binary = entries
        .iter()
        .any(|entry| &entry.agent_id == agent_id && &entry.scope == scope);
    let reason = if changed_binary {
        format!(
            "The executable or arguments for {agent_name} changed since you last trusted this workspace."
        )
    } else {
        format!(
            "{agent_name} will run natively in this workspace and can read repository settings, hooks and MCP configs before any permission prompt."
        )
    };
    Ok(Err(TrustRequirement {
        agent_id: agent_id.clone(),
        agent_name: agent_name.to_owned(),
        scope: scope.clone(),
        fingerprint: fingerprint.clone(),
        disclosure: scope.disclosure().to_owned(),
        reason,
    }))
}

/// Grant (or update) consent for `(agent_id, scope, fingerprint)` under cross-window file lock.
pub async fn grant_trust(
    state: Option<&Path>,
    repo: Option<&Path>,
    agent_id: AgentId,
    scope: TrustScope,
    fingerprint: LauncherFingerprint,
) -> Result<TrustRecord, String> {
    let storage = Storage::new(state, repo, Tokens::ChatTrust)?;
    let _lock = storage.lock(TRUST_LOCK).await?;
    let mut saved: SavedTrust = storage.read(TRUST_FILE)?.unwrap_or_default();
    saved
        .entries
        .retain(|entry| !(entry.agent_id == agent_id && entry.scope == scope));
    let record = TrustRecord {
        agent_id,
        scope,
        fingerprint,
        granted_at: crate::broker::now(),
    };
    saved.entries.push(record.clone());
    storage.save(TRUST_FILE, &saved)?;
    Ok(record)
}

/// Revoke workspace trust for `(agent_id, scope)`. Returns whether a matching entry existed.
pub async fn revoke_trust(
    state: Option<&Path>,
    repo: Option<&Path>,
    agent_id: &AgentId,
    scope: &TrustScope,
) -> Result<bool, String> {
    let storage = Storage::new(state, repo, Tokens::ChatTrust)?;
    let _lock = storage.lock(TRUST_LOCK).await?;
    let mut saved: SavedTrust = storage.read(TRUST_FILE)?.unwrap_or_default();
    let before = saved.entries.len();
    saved
        .entries
        .retain(|entry| !(&entry.agent_id == agent_id && &entry.scope == scope));
    let removed = saved.entries.len() != before;
    if removed {
        storage.save(TRUST_FILE, &saved)?;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn trust_requires_reconsent_when_executable_bytes_change() {
        let root = crate::test_os::tempdir();
        let state = root.path().join("state");
        let worktree = root.path().join("work");
        let bin_dir = root.path().join("bin");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::create_dir_all(&bin_dir).unwrap();
        let exe = bin_dir.join("agent-bin");
        std::fs::write(&exe, b"version-1").unwrap();

        let agent: AgentId = "antigravity".parse().unwrap();
        let scope = TrustScope::for_worktree(&worktree, &repository::Identity::default()).unwrap();
        assert!(matches!(scope, TrustScope::Directory { .. }));

        let fp1 = LauncherFingerprint::compute(&exe, &["acp".to_owned()]).unwrap();
        assert!(
            check_trust(
                Some(&state),
                Some(&worktree),
                &agent,
                "Antigravity",
                &scope,
                &fp1
            )
            .unwrap()
            .is_err()
        );
        grant_trust(
            Some(&state),
            Some(&worktree),
            agent.clone(),
            scope.clone(),
            fp1.clone(),
        )
        .await
        .unwrap();
        assert!(
            check_trust(
                Some(&state),
                Some(&worktree),
                &agent,
                "Antigravity",
                &scope,
                &fp1
            )
            .unwrap()
            .is_ok()
        );

        // Same-path binary replacement forces re-consent even if size matches.
        std::fs::write(&exe, b"version-2").unwrap();
        let fp2 = LauncherFingerprint::compute(&exe, &["acp".to_owned()]).unwrap();
        assert_ne!(fp1.executable_sha256, fp2.executable_sha256);
        let req = check_trust(
            Some(&state),
            Some(&worktree),
            &agent,
            "Antigravity",
            &scope,
            &fp2,
        )
        .unwrap()
        .unwrap_err();
        assert!(req.reason.contains("changed"));

        assert!(
            revoke_trust(Some(&state), Some(&worktree), &agent, &scope)
                .await
                .unwrap()
        );
        assert!(
            list_trust(Some(&state), Some(&worktree))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn builtin_preset_rejects_executable_inside_worktree() {
        let root = crate::test_os::tempdir();
        let worktree = root.path().join("repo");
        std::fs::create_dir_all(&worktree).unwrap();
        let malicious = crate::test_os::executable_named(&worktree, "agy");
        std::fs::write(&malicious, b"fake-acp-binary\n").unwrap();
        crate::test_os::make_executable(&malicious);
        let error = resolve_executable(&platform::path_text(&malicious), Some(&worktree), true)
            .unwrap_err();
        assert!(error.contains("inside the workspace"), "{error}");
    }

    #[tokio::test]
    async fn probing_a_builtin_preset_inside_the_worktree_runs_nothing() {
        let root = crate::test_os::tempdir();
        let worktree = root.path().join("repo");
        std::fs::create_dir_all(&worktree).unwrap();
        let malicious = crate::test_os::executable_named(&worktree, "agy");
        std::fs::write(&malicious, b"fake-acp-binary\n").unwrap();
        crate::test_os::make_executable(&malicious);
        let mut spec = super::super::catalog::AgentSpec::from_preset(
            super::super::catalog::preset_by_id("antigravity").unwrap(),
        );
        spec.program = platform::path_text(&malicious);
        let probed = super::super::catalog::probe_spec(&spec, Some(&worktree), true).await;
        let super::super::catalog::AgentAvailability::Failed { error, .. } = probed else {
            panic!("the probe ran the workspace's executable: {probed:?}");
        };
        assert!(error.contains("inside the workspace"), "{error}");
    }

    #[test]
    fn builtin_preset_rejects_executable_in_enclosing_git_worktree_from_subdirectory() {
        let root = crate::test_os::tempdir();
        let repo = root.path().join("repo");
        let subdir = repo.join("packages").join("app");
        std::fs::create_dir_all(&subdir).unwrap();
        crate::test_os::git(&repo, &["init", "--quiet"]);
        let malicious = crate::test_os::executable_named(&repo, "agy");
        std::fs::write(&malicious, b"fake-acp-binary\n").unwrap();
        crate::test_os::make_executable(&malicious);
        let error =
            resolve_executable(&platform::path_text(&malicious), Some(&subdir), true).unwrap_err();
        assert!(error.contains("inside the workspace"), "{error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn verified_spawn_runs_hashed_script_even_if_path_is_replaced_before_exec() {
        use tokio::io::AsyncReadExt;

        let root = crate::test_os::tempdir();
        let script = root.path().join("agent.sh");
        std::fs::write(&script, b"#!/bin/sh\nprintf 'verified-original'\n").unwrap();
        crate::test_os::make_executable(&script);

        let mut file = platform::open_verified_executable(&script).unwrap();
        let sha256 = hash_file(&mut file, &script).unwrap();

        // Atomically replace the pathname on disk with different bytes AFTER verification.
        let replacement = root.path().join("agent.sh.new");
        std::fs::write(&replacement, b"#!/bin/sh\nprintf 'malicious-swap'\n").unwrap();
        crate::test_os::make_executable(&replacement);
        std::fs::rename(&replacement, &script).unwrap();

        let env = std::collections::BTreeMap::new();
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut child = crate::process::spawn_verified_supervised(
            &script,
            file,
            &sha256,
            &[],
            root.path(),
            &env,
            &cancel,
        )
        .await
        .unwrap();
        let mut stdout = child.stdout().take().unwrap();
        let mut out = String::new();
        stdout.read_to_string(&mut out).await.unwrap();
        let _ = child.wait().await;
        assert_eq!(out, "verified-original");
    }
}
