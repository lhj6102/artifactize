//! Warm ACP child process pool (`C-ONE-TURN-OWNERSHIP`).
//!
//! Decouples long-lived ACP child process supervision (`process_cancel`) from individual turn
//! cancellation and timeouts (`turn_cancel`). Bounds Windows CWD lock lifetime with `IDLE_TTL`
//! (`P-FILES`) and caps concurrent warm children via LRU (`MAX_WARM_SESSIONS`).

use std::{
    collections::BTreeMap,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

use tokio::{io::AsyncReadExt, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{platform, process, types::SessionId};

use super::{AcpBinding, AgentId, Problem, catalog::AgentSpec, wire::Connection};

/// Bound idle warm child lifetime so Windows CWD locks do not outlive inactive sessions (`P-FILES`).
pub const IDLE_TTL: Duration = Duration::from_secs(5 * 60);
/// Maximum number of idle warm ACP children retained concurrently across sessions (LRU).
pub const MAX_WARM_SESSIONS: usize = 4;
/// Bound retained stderr bytes per ACP child process.
const STDERR_LIMIT: usize = 16 * 1024;
/// Read stderr in small chunks while draining in the background.
const STDERR_READ: usize = 4096;
/// Grace period for an ACP child to exit cleanly after closing stdin before termination.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// One supervised ACP child process and its active JSON-RPC wire connection.
pub struct WarmChild {
    pub session_id: SessionId,
    pub agent_id: AgentId,
    pub generation: u64,
    pub executable_sha256: String,
    pub runtime_fingerprint: String,
    pub remote_session_id: Option<String>,
    pub connection: Connection,
    child: platform::Child,
    stderr: JoinHandle<Vec<u8>>,
    process_cancel: CancellationToken,
    last_used: Instant,
    quiescent: bool,
}

impl WarmChild {
    /// Spawn a fresh supervised ACP child in `worktree` with `process_cancel` decoupled from
    /// any turn timeout.
    pub async fn spawn(
        session_id: SessionId,
        spec: &AgentSpec,
        resolved_executable: &Path,
        executable_sha256: String,
        runtime_fingerprint: String,
        generation: u64,
        worktree: &Path,
    ) -> Result<Self, Problem> {
        let process_cancel = CancellationToken::new();
        let command = process::Command {
            program: resolved_executable.as_os_str().to_owned(),
            args: spec.args.iter().map(Into::into).collect(),
            cwd: worktree.to_path_buf(),
            env: spec.build_env(true),
            // Process supervision is decoupled from turn deadlines; spawn_supervised does not
            // attach a fixed execution deadline to the child lifetime.
            timeout: Duration::MAX,
        };
        let mut child = process::spawn_supervised(&command, &process_cancel)
            .await
            .map_err(|error| {
                if matches!(&error, process::Error::Spawn(e) if e.kind() == std::io::ErrorKind::NotFound) {
                    Problem::Missing {
                        agent: spec.name.clone(),
                        hint: spec.install_hint.clone(),
                    }
                } else {
                    Problem::Failed(format!("Cannot start {}: {error}.", spec.name))
                }
            })?;
        let stdout = child.stdout().take().expect("ACP stdout piped");
        let stdin = child.stdin().take().expect("ACP stdin piped");
        let mut stderr = child.stderr().take().expect("ACP stderr piped");
        let stderr_task = tokio::spawn(async move {
            let mut kept = Vec::new();
            let mut buffer = [0u8; STDERR_READ];
            while let Ok(count) = stderr.read(&mut buffer).await {
                if count == 0 {
                    break;
                }
                kept.extend_from_slice(&buffer[..count]);
                if kept.len() > STDERR_LIMIT {
                    kept.drain(..kept.len() - STDERR_LIMIT);
                }
            }
            kept
        });
        let connection = Connection::new(
            &spec.name,
            &spec.install_hint,
            stdout,
            stdin,
            spec.hide_always_options,
        );
        Ok(Self {
            session_id,
            agent_id: spec.id.clone(),
            generation,
            executable_sha256,
            runtime_fingerprint,
            remote_session_id: None,
            connection,
            child,
            stderr: stderr_task,
            process_cancel,
            last_used: Instant::now(),
            quiescent: false,
        })
    }

    pub fn mark_quiescent(&mut self, remote_session_id: Option<String>) {
        self.quiescent = true;
        self.remote_session_id = remote_session_id;
        self.last_used = Instant::now();
    }

    /// Terminate the process tree via `platform`/`process` (`P-ONE-LAYER`) and return captured stderr.
    pub async fn terminate(mut self) -> Vec<u8> {
        self.process_cancel.cancel();
        drop(self.connection);
        let exited = tokio::time::timeout(STOP_GRACE, self.child.wait()).await;
        if !matches!(exited, Ok(Ok(_))) {
            let _ = process::terminate(&mut self.child).await;
        }
        let _ = self.child.cleanup().await;
        crate::task::joined(self.stderr.await).unwrap_or_default()
    }
}

/// Result of querying the warm pool for a session before starting a turn.
pub enum TakeOutcome {
    /// Path A candidate: quiescent warm child matching `generation`, `agent_id`,
    /// `executable_sha256` and `runtime_fingerprint`.
    Warm(Box<WarmChild>),
    /// A warm child existed but had to be retired (e.g. executable SHA-256 changed, generation
    /// bumped, or idle TTL expired). Caller should bump `generation` if digest mismatched.
    Retired {
        stale: Box<WarmChild>,
        digest_mismatch: bool,
    },
    /// No warm child held in this process for the session.
    Absent,
}

/// Process-local pool of idle warm ACP children.
#[derive(Default)]
pub struct Pool {
    entries: Mutex<BTreeMap<SessionId, WarmChild>>,
}

impl Pool {
    /// Check out a warm child for Path A reuse, validating quiescence, generation, agent ID,
    /// spawn-time `executable_sha256`, `runtime_fingerprint`, and `IDLE_TTL`.
    pub fn take_for_turn(
        &self,
        session_id: &SessionId,
        binding: Option<&AcpBinding>,
        agent_id: &AgentId,
        verified_sha256: &str,
        runtime_fingerprint: &str,
    ) -> TakeOutcome {
        let mut entries = self.entries.lock().expect("ACP warm pool lock");
        let Some(mut child) = entries.remove(session_id) else {
            return TakeOutcome::Absent;
        };
        let digest_mismatch = child.executable_sha256 != verified_sha256;
        let matches = child.quiescent
            && !digest_mismatch
            && child.agent_id == *agent_id
            && child.runtime_fingerprint == runtime_fingerprint
            && child.last_used.elapsed() < IDLE_TTL
            && binding.is_some_and(|b| b.generation == child.generation);
        if matches {
            child.quiescent = false;
            child.last_used = Instant::now();
            TakeOutcome::Warm(Box::new(child))
        } else {
            TakeOutcome::Retired {
                stale: Box::new(child),
                digest_mismatch,
            }
        }
    }

    /// Park a quiescent warm child and return any LRU/expired victims that must be terminated
    /// and have their `warm_owner` cleared in SQLite.
    pub fn park(&self, mut child: WarmChild) -> Vec<WarmChild> {
        child.last_used = Instant::now();
        let mut entries = self.entries.lock().expect("ACP warm pool lock");
        let mut evicted = Vec::new();
        if let Some(previous) = entries.insert(child.session_id.clone(), child) {
            evicted.push(previous);
        }
        // Evict any TTL-expired entries first.
        let expired_ids: Vec<SessionId> = entries
            .iter()
            .filter(|(_, entry)| entry.last_used.elapsed() >= IDLE_TTL)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired_ids {
            if let Some(victim) = entries.remove(&id) {
                evicted.push(victim);
            }
        }
        // Enforce MAX_WARM_SESSIONS via LRU eviction.
        while entries.len() > MAX_WARM_SESSIONS {
            let Some(oldest_id) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            if let Some(victim) = entries.remove(&oldest_id) {
                evicted.push(victim);
            }
        }
        evicted
    }

    /// Evict a session's warm child unconditionally (on `set_worktree`, agent switch, etc.).
    pub fn take_any(&self, session_id: &SessionId) -> Option<WarmChild> {
        self.entries
            .lock()
            .expect("ACP warm pool lock")
            .remove(session_id)
    }

    /// Evict only when the saved SQLite `generation` no longer matches the warm child's generation
    /// (called on `SessionInvalidated(session_id)`).
    pub fn take_if_generation_stale(
        &self,
        session_id: &SessionId,
        saved_generation: Option<u64>,
    ) -> Option<WarmChild> {
        let mut entries = self.entries.lock().expect("ACP warm pool lock");
        let stale = entries
            .get(session_id)
            .is_some_and(|warm| saved_generation != Some(warm.generation));
        if stale {
            entries.remove(session_id)
        } else {
            None
        }
    }

    /// Collect and remove all idle children past `IDLE_TTL`.
    pub fn take_expired(&self) -> Vec<WarmChild> {
        let mut entries = self.entries.lock().expect("ACP warm pool lock");
        let expired: Vec<SessionId> = entries
            .iter()
            .filter(|(_, entry)| entry.last_used.elapsed() >= IDLE_TTL)
            .map(|(id, _)| id.clone())
            .collect();
        expired
            .into_iter()
            .filter_map(|id| entries.remove(&id))
            .collect()
    }

    /// Drain all warm children (called on `Chat::close()`).
    pub fn drain_all(&self) -> Vec<WarmChild> {
        let mut entries = self.entries.lock().expect("ACP warm pool lock");
        let ids: Vec<SessionId> = entries.keys().cloned().collect();
        ids.into_iter()
            .filter_map(|id| entries.remove(&id))
            .collect()
    }
}
