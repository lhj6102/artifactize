//! Execution records and their compatible wire format.
use crate::{
    config::Profile,
    process,
    runtime::Verdict,
    store,
    types::{ExecutionStatus, Fingerprint, RequestId, ReuseKey, RunId},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    #[serde(with = "crate::platform::path_serde")]
    pub repo_path: PathBuf,
    pub run_id: RunId,
    pub request_id: RequestId,
    pub eval_id: crate::types::EvalId,
    pub eval_def_hash: crate::types::DefinitionHash,
    pub completed_at: Option<crate::types::Timestamp>,
    /// For Agent results: the SHA-256 of each tool's `executionPaths` when the review started,
    /// by tool name and declared path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub execution_paths: crate::tools::pins::Pins,
}

/// One execution and, once GREEN or RED with a key, one record of its key's history. A local
/// Agent execution holds one of its backend's machine-wide slots while RUNNING.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "ExecutionWire", into = "ExecutionWire")]
pub struct Execution {
    pub state: store::ExecutionState,
    pub id: crate::types::ExecutionId,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    pub key: Option<ReuseKey>,
    /// The target Artifact's fingerprint.
    pub fingerprint: Option<Fingerprint>,
    /// Each Artifact the key covers, the target included, with its fingerprint.
    pub fingerprints: BTreeMap<crate::types::ArtifactName, Fingerprint>,
    /// Kinds of exactly the Artifacts covered by a new-format reuse key.
    pub artifact_kinds: BTreeMap<crate::types::ArtifactName, crate::config::ArtifactKind>,
    pub eval_def_hash: crate::types::DefinitionHash,
    /// The local process running this execution; absent for one mirrored from a remote
    /// review store, which no local process owns.
    pub owner: Option<process::ChildIdentity>,
    pub profile: crate::config::StoredProfile,
    /// How this result was produced; never part of the key.
    pub options: ExecutionOptions,
    pub usage: Option<Vec<crate::llm::Attempt>>,
    pub provenance: Provenance,
    pub started_at: crate::types::Timestamp,
    pub producer: Option<Producer>,
    /// The Human claimant who submitted this result.
    pub reviewer: Option<crate::types::ReviewerId>,
    /// Set only on executions mirrored from a remote review store.
    pub origin: Option<Origin>,
    pub manifest: Option<crate::cache::Manifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecutionWire {
    pub id: crate::types::ExecutionId,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    #[serde(default)]
    pub key: Option<ReuseKey>,
    /// The target Artifact's fingerprint.
    pub fingerprint: Option<Fingerprint>,
    /// Each Artifact the key covers, the target included, with its fingerprint.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fingerprints: BTreeMap<crate::types::ArtifactName, Fingerprint>,
    /// Kinds of exactly the Artifacts covered by a new-format reuse key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub artifact_kinds: BTreeMap<crate::types::ArtifactName, crate::config::ArtifactKind>,
    /// Partition Live names out of the old strict enum map for schema-10 readers.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub live_artifacts: std::collections::BTreeSet<crate::types::ArtifactName>,
    pub eval_def_hash: crate::types::DefinitionHash,
    pub owner_pid: u32,
    pub owner_start_time: u64,
    pub status: ExecutionStatus,
    pub result: Option<store::ExecutionResult>,
    pub error: Option<String>,
    pub error_code: Option<crate::types::FailureCode>,
    pub profile: crate::config::StoredProfile,
    /// How this result was produced; never part of the key.
    #[serde(default)]
    pub options: ExecutionOptions,
    pub usage: Option<Vec<crate::llm::Attempt>>,
    pub provenance: Provenance,
    pub started_at: crate::types::Timestamp,
    pub completed_at: Option<crate::types::Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    /// The Human claimant who submitted this result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<crate::types::ReviewerId>,
    /// Set only on executions mirrored from a remote review store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<crate::cache::Manifest>,
}

impl TryFrom<ExecutionWire> for Execution {
    type Error = String;
    fn try_from(wire: ExecutionWire) -> Result<Self, Self::Error> {
        if wire
            .live_artifacts
            .iter()
            .any(|name| wire.artifact_kinds.contains_key(name))
        {
            return Err(
                "A saved execution gives one Artifact both a filesystem and Live kind.".into(),
            );
        }
        let state = store::ExecutionState::from_parts(
            wire.status,
            wire.result,
            wire.error,
            wire.error_code,
            wire.completed_at,
        )?;
        Ok(Self {
            state,
            id: wire.id,
            key: wire.key,
            fingerprint: wire.fingerprint,
            fingerprints: wire.fingerprints,
            artifact_kinds: wire
                .artifact_kinds
                .into_iter()
                .chain(
                    wire.live_artifacts
                        .into_iter()
                        .map(|name| (name, crate::config::ArtifactKind::Live)),
                )
                .collect(),
            eval_def_hash: wire.eval_def_hash,
            owner: std::num::NonZeroU32::new(wire.owner_pid).map(|pid| process::ChildIdentity {
                pid: process::ProcessId::from(pid),
                start_time: wire.owner_start_time,
            }),
            profile: wire.profile,
            options: wire.options,
            usage: wire.usage,
            provenance: wire.provenance,
            started_at: wire.started_at,
            producer: wire.producer,
            reviewer: wire.reviewer,
            origin: wire.origin,
            manifest: wire.manifest,
        })
    }
}
impl From<Execution> for ExecutionWire {
    fn from(record: Execution) -> Self {
        let owner_pid = record.owner_pid();
        let owner_start_time = record.owner_start_time();
        Self {
            status: record.status(),
            result: record.result().cloned(),
            error: record.error().map(str::to_owned),
            error_code: record.error_code(),
            completed_at: record.completed_at(),
            id: record.id,
            key: record.key,
            fingerprint: record.fingerprint,
            fingerprints: record.fingerprints,
            live_artifacts: record
                .artifact_kinds
                .iter()
                .filter(|(_, kind)| **kind == crate::config::ArtifactKind::Live)
                .map(|(name, _)| name.clone())
                .collect(),
            artifact_kinds: record
                .artifact_kinds
                .into_iter()
                .filter(|(_, kind)| *kind != crate::config::ArtifactKind::Live)
                .collect(),
            eval_def_hash: record.eval_def_hash,
            owner_pid,
            owner_start_time,
            profile: record.profile,
            options: record.options,
            usage: record.usage,
            provenance: record.provenance,
            started_at: record.started_at,
            producer: record.producer,
            reviewer: record.reviewer,
            origin: record.origin,
            manifest: record.manifest,
        }
    }
}
impl Execution {
    pub fn status(&self) -> crate::types::ExecutionStatus {
        self.state.status()
    }
    pub fn result(&self) -> Option<&store::ExecutionResult> {
        self.state.result()
    }
    pub fn error(&self) -> Option<&str> {
        self.state.error()
    }
    pub fn error_code(&self) -> Option<crate::types::FailureCode> {
        self.state.error_code()
    }
    pub fn completed_at(&self) -> Option<crate::types::Timestamp> {
        self.state.completed_at()
    }
    /// The owning process's pid, or 0 for a remote-mirrored execution with no local owner:
    /// the `owner_pid` SQL column and wire field's sentinel for "no owner", confined to
    /// this persistence boundary.
    pub fn owner_pid(&self) -> u32 {
        self.owner.map_or(0, |owner| owner.pid.get())
    }
    /// The owning process's start time, or 0 alongside [`Self::owner_pid`]'s sentinel.
    pub fn owner_start_time(&self) -> u64 {
        self.owner.map_or(0, |owner| owner.start_time)
    }
}

/// The execution options of a result: stored next to it, never part of its key, so results
/// from different profiles for the same key reuse each other.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionOptions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mutex: Vec<crate::config::MutexTag>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<crate::config::Backend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<crate::config::ModelId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<crate::config::Reasoning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "crate::config::validation::milliseconds")]
    pub timeout_ms: Option<std::time::Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    /// The named profile a Run chose instead of the eval's own, which only executions saved
    /// before profiles chose their settings from the environment record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<crate::config::ProfileName>,
}

impl ExecutionOptions {
    /// The declared options of an effective profile, as declared (absent means the default).
    pub fn new(profile: &Profile) -> Self {
        let mut options = Self::default();
        match profile {
            Profile::Agent {
                backend,
                model,
                reasoning,
                timeout_ms,
                max_tool_calls,
                max_tokens,
            } => {
                options.backend = Some(backend.clone());
                options.model = Some(model.clone());
                options.reasoning = *reasoning;
                options.timeout_ms = *timeout_ms;
                options.max_tool_calls = *max_tool_calls;
                options.max_tokens = *max_tokens;
            }
            Profile::Runtime { timeout_ms, .. } => options.timeout_ms = *timeout_ms,
            Profile::Human {} => {}
        }
        options
    }
}

/// Who produced an execution: display metadata, never authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Producer {
    /// `user@host`.
    pub name: crate::types::ProducerId,
    /// The producing artifactize version.
    pub version: String,
    /// Where an Agent review's saved conversation lives. It travels with remote records,
    /// which older clients read without it; the conversation itself never leaves the machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<crate::agent::session::SessionRef>,
}

/// The name every producer without a user or host gets.
const UNKNOWN_PRODUCER: &str = "unknown@unknown";

impl Producer {
    pub fn current() -> Self {
        let user = crate::platform::user_name();
        let host = crate::platform::host_name();
        let name = format!(
            "{}@{}",
            user.as_deref().unwrap_or("unknown"),
            host.as_deref()
                .map(str::trim)
                .filter(|host| !host.is_empty())
                .unwrap_or("unknown")
        );
        // A name is printable and bounded; whatever the system reports is cut to fit.
        let mut printable: String = name.chars().filter(|c| !c.is_control()).collect();
        while printable.len() > crate::types::MAX_ID_BYTES {
            printable.pop();
        }
        Self {
            name: printable
                .parse()
                .unwrap_or_else(|_| UNKNOWN_PRODUCER.parse().expect("a valid producer name")),
            version: env!("CARGO_PKG_VERSION").into(),
            session: None,
        }
    }
}

/// The remote store, authenticated publisher and server clock of a mirrored execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    pub store: crate::types::StoreUrl,
    pub publisher: crate::types::TokenName,
    pub published_at: crate::types::Timestamp,
}

impl Execution {
    /// The backend whose machine-wide slot this execution holds while RUNNING: a local Agent
    /// review's. A mirrored record ran elsewhere.
    pub(super) fn backend(&self) -> Option<&str> {
        self.options
            .backend
            .as_ref()
            .map(|backend| backend.as_str())
            .filter(|_| self.origin.is_none())
    }

    /// Whether this record completed after `other`, comparing `completedAt` as instants.
    pub fn completed_after(&self, other: &Execution) -> bool {
        self.completed_at() > other.completed_at()
    }

    pub fn verdict(&self) -> Option<Verdict> {
        match self.status() {
            ExecutionStatus::Green => Some(Verdict::Green),
            ExecutionStatus::Red => Some(Verdict::Red),
            _ => None,
        }
    }
}
