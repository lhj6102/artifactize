//! Saved Run and request representations and their wire conversions.
use crate::store::{self as store, ExecutionOptions, Origin, Producer, Provenance};
use crate::{
    config::ProfileKind,
    types::{
        Fingerprint, RequestId, RequestStatus, ReuseKey, RunId, RunStatus, SessionId, Timestamp,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RunWire", into = "RunWire")]
pub struct Run {
    pub id: RunId,
    pub repo_path: PathBuf,
    /// Optional display metadata; schema 5 Runs written before this remain readable.
    pub repository: crate::repository::Identity,
    pub state_dir: PathBuf,
    pub state: store::RunState,
    pub modes: crate::runs::RunModes,
    pub control: Option<crate::runs::Control>,
    pub request_key: Option<crate::types::RequestKey>,
    /// What the requester said it is working on; for people only.
    pub comment: Option<crate::types::RunComment>,
    pub created_at: Timestamp,
    pub selection: crate::project::selection::Selection,
    pub profile: Option<crate::project::selection::ProfileSelection>,
    pub definitions: store::definitions::Definitions,
    pub recursive: bool,
    pub force: bool,
    pub jobs: Option<usize>,
    /// Fingerprints computed at once; absent in Runs saved before 0.5.
    pub fingerprint_jobs: Option<usize>,
    pub max_executions: Option<u64>,
    pub executions_started: u64,
    pub wait_timeout_ms: Option<Duration>,
    pub wait_timed_out: bool,
    /// Kinds whose evals only reuse a result; one with nothing to reuse is not executed.
    pub reuse_only: BTreeSet<ProfileKind>,
    pub validation: store::Validation,
    /// Agent backends this Run stopped admitting reviews on, in the order they stopped.
    pub stopped_backends: Vec<StoppedBackend>,
    /// Verdicts the Run took from saved results for evals it has no request for, such as the
    /// dependencies of a partial Run, so readers judge gates on the Run's own evidence.
    pub evidence: BTreeMap<crate::types::EvalId, crate::types::RequestStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunWire {
    pub id: RunId,
    #[serde(with = "crate::platform::path_serde")]
    pub repo_path: PathBuf,
    /// Optional display metadata; schema 5 Runs written before this remain readable.
    #[serde(flatten)]
    pub repository: crate::repository::Identity,
    #[serde(with = "crate::platform::path_serde")]
    pub state_dir: PathBuf,
    #[serde(default, skip_serializing_if = "crate::runs::RunModes::is_default")]
    pub modes: crate::runs::RunModes,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<crate::runs::Control>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_key: Option<crate::types::RequestKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<crate::types::RunComment>,
    pub status: RunStatus,
    pub created_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    pub selection: crate::project::selection::Selection,
    #[serde(default)]
    pub profile: Option<crate::project::selection::ProfileSelection>,
    #[serde(default)]
    pub definitions: store::definitions::Definitions,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub force: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jobs: Option<usize>,
    /// Fingerprints computed at once; absent in Runs saved before 0.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint_jobs: Option<usize>,
    #[serde(default)]
    pub max_executions: Option<u64>,
    #[serde(default)]
    pub executions_started: u64,
    #[serde(default, with = "store::wait_timeout")]
    pub wait_timeout_ms: Option<Duration>,
    #[serde(default)]
    pub wait_timed_out: bool,
    /// Kinds whose evals only reuse a result; one with nothing to reuse is not executed.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub reuse_only: BTreeSet<ProfileKind>,
    pub validation: store::Validation,
    pub error: Option<String>,
    /// Agent backends this Run stopped admitting reviews on, in the order they stopped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped_backends: Vec<StoppedBackend>,
    /// Verdicts the Run took from saved results for evals it has no request for, such as the
    /// dependencies of a partial Run, so readers judge gates on the Run's own evidence.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub evidence: BTreeMap<crate::types::EvalId, crate::types::RequestStatus>,
}

impl TryFrom<RunWire> for Run {
    type Error = String;
    fn try_from(wire: RunWire) -> Result<Self, Self::Error> {
        let state = store::RunState::from_parts(wire.status, wire.completed_at, wire.error)?;
        Ok(Self {
            state,
            modes: wire.modes,
            control: wire.control,
            request_key: wire.request_key,
            comment: wire.comment,
            id: wire.id,
            repo_path: wire.repo_path,
            repository: wire.repository,
            state_dir: wire.state_dir,
            created_at: wire.created_at,
            selection: wire.selection,
            profile: wire.profile,
            definitions: wire.definitions,
            recursive: wire.recursive,
            force: wire.force,
            jobs: wire.jobs,
            fingerprint_jobs: wire.fingerprint_jobs,
            max_executions: wire.max_executions,
            executions_started: wire.executions_started,
            wait_timeout_ms: wire.wait_timeout_ms,
            wait_timed_out: wire.wait_timed_out,
            reuse_only: wire.reuse_only,
            validation: wire.validation,
            stopped_backends: wire.stopped_backends,
            evidence: wire.evidence,
        })
    }
}
impl From<Run> for RunWire {
    fn from(record: Run) -> Self {
        Self {
            modes: record.modes.clone(),
            control: record.control.clone(),
            request_key: record.request_key.clone(),
            comment: record.comment.clone(),
            status: record.status(),
            completed_at: record.completed_at(),
            error: record.error().map(str::to_owned),
            id: record.id,
            repo_path: record.repo_path,
            repository: record.repository,
            state_dir: record.state_dir,
            created_at: record.created_at,
            selection: record.selection,
            profile: record.profile,
            definitions: record.definitions,
            recursive: record.recursive,
            force: record.force,
            jobs: record.jobs,
            fingerprint_jobs: record.fingerprint_jobs,
            max_executions: record.max_executions,
            executions_started: record.executions_started,
            wait_timeout_ms: record.wait_timeout_ms,
            wait_timed_out: record.wait_timed_out,
            reuse_only: record.reuse_only,
            validation: record.validation,
            stopped_backends: record.stopped_backends,
            evidence: record.evidence,
        }
    }
}
impl Run {
    pub fn status(&self) -> RunStatus {
        self.state.status()
    }
    /// Show the Run WAITING_HUMAN while one of its requests, saved with `statuses`, is an
    /// unsubmitted sign-off, and RUNNING otherwise while it is open.
    pub(in crate::store) fn show_signoffs(
        &mut self,
        mut statuses: impl Iterator<Item = crate::types::RequestStatus>,
    ) {
        let open = statuses.any(crate::types::RequestStatus::is_human_wait);
        self.state = self.state.clone().with_signoffs(open);
    }
    pub fn completed_at(&self) -> Option<Timestamp> {
        self.state.completed_at()
    }
    pub fn error(&self) -> Option<&str> {
        self.state.error()
    }
}

/// An Agent backend a Run stopped after an AUTHENTICATION or QUOTA failure, which every
/// later review on it would repeat.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedBackend {
    pub backend: crate::config::Backend,
    pub error_code: crate::types::BackendStopCode,
    pub eval_id: crate::types::EvalId,
    pub request_id: RequestId,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RequestWire", into = "RequestWire")]
pub struct Request {
    pub state: store::RequestState,
    pub id: RequestId,
    pub run_id: RunId,
    pub eval_id: crate::types::EvalId,
    pub target: crate::types::ArtifactName,
    pub title: String,
    pub profile: crate::config::StoredProfile,
    pub requested_profile: crate::config::StoredProfile,
    pub eval_def_hash: crate::types::DefinitionHash,
    /// The requested execution options, or a reused result's.
    pub options: ExecutionOptions,
    pub execution_id: Option<crate::types::ExecutionId>,
    pub provenance: Option<Provenance>,
    /// Usage spent by this request; a reused request spent none.
    pub usage: Option<Vec<crate::llm::Attempt>>,
    /// The reused execution's original usage, never counted as spent.
    pub reused_usage: Option<Vec<crate::llm::Attempt>>,
    /// The reused result came from the live execution this request waited for, not a
    /// completed record. Saved only; output reports it as `source.kind` ([`crate::query::source`]).
    pub joined: bool,
    /// Who produced a reused result.
    pub producer: Option<Producer>,
    /// The Human claimant who submitted a reused result.
    pub reviewer: Option<crate::types::ReviewerId>,
    /// The remote store, publisher and publication time of a result reused from a mirror.
    pub origin: Option<Origin>,
    /// The Agent review's session id ([`crate::agent::session_id`]), which every request of
    /// the review sends as its prompt-cache identity. Absent when no Agent review ran, as on
    /// reuse, and on requests saved before it existed.
    pub session_id: Option<SessionId>,
    /// Where the saved conversation behind this request's result lives: its own review's,
    /// or for a reused result the producing review's. Absent when none was saved.
    pub session: Option<crate::agent::session::SessionRef>,
    pub human_definition: Option<store::HumanDefinition>,
    pub payload: crate::config::StoredPayload,
    pub references: BTreeMap<crate::config::ReferenceName, crate::types::ArtifactName>,
    pub deps: Vec<crate::types::ArtifactName>,
    pub force: bool,
    /// The target Artifact's fingerprint.
    pub fingerprint: Option<Fingerprint>,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    pub key: Option<ReuseKey>,
    /// Each Artifact the key covers, with its fingerprint.
    pub fingerprints: std::collections::BTreeMap<crate::types::ArtifactName, Fingerprint>,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub cwd: PathBuf,
    pub run_dir: Option<PathBuf>,
    pub argv: Option<Vec<String>>,
    pub child: Option<store::ChildIdentity>,
    pub blocked_reason: Option<String>,
    /// Unfulfilled Evals behind this request’s dependency gates.
    pub blocked_by: Vec<store::Blocker>,
    /// What a queued request waits for besides a job; `blocked_reason` says it in words.
    pub queue: Option<QueueReason>,
}

/// What a queued request waits for besides a job slot of its Run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QueueReason {
    /// A free machine-wide slot of this backend (`limits.json`).
    Slot { backend: crate::config::Backend },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestWire {
    pub id: RequestId,
    pub run_id: RunId,
    pub eval_id: crate::types::EvalId,
    pub target: crate::types::ArtifactName,
    pub title: String,
    pub profile: crate::config::StoredProfile,
    pub requested_profile: crate::config::StoredProfile,
    pub eval_def_hash: crate::types::DefinitionHash,
    /// The requested execution options, or a reused result's.
    #[serde(default)]
    pub options: ExecutionOptions,
    pub execution_id: Option<crate::types::ExecutionId>,
    pub provenance: Option<Provenance>,
    /// Usage spent by this request; a reused request spent none.
    pub usage: Option<Vec<crate::llm::Attempt>>,
    /// The reused execution's original usage, never counted as spent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_usage: Option<Vec<crate::llm::Attempt>>,
    /// The reused result came from the live execution this request waited for, not a
    /// completed record. Saved only; output reports it as `source.kind` ([`crate::query::source`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub joined: bool,
    /// Who produced a reused result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    /// The Human claimant who submitted a reused result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<crate::types::ReviewerId>,
    /// The remote store, publisher and publication time of a result reused from a mirror.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// The Agent review's session id ([`crate::agent::session_id`]), which every request of
    /// the review sends as its prompt-cache identity. Absent when no Agent review ran, as on
    /// reuse, and on requests saved before it existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// Where the saved conversation behind this request's result lives: its own review's,
    /// or for a reused result the producing review's. Absent when none was saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<crate::agent::session::SessionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_definition: Option<store::HumanDefinition>,
    pub payload: crate::config::StoredPayload,
    pub references: BTreeMap<crate::config::ReferenceName, crate::types::ArtifactName>,
    pub deps: Vec<crate::types::ArtifactName>,
    #[serde(default)]
    pub force: bool,
    /// The target Artifact's fingerprint.
    #[serde(default)]
    pub fingerprint: Option<Fingerprint>,
    /// The reuse key; absent without a fingerprint on every Artifact the eval depends on.
    #[serde(default)]
    pub key: Option<ReuseKey>,
    /// Each Artifact the key covers, with its fingerprint.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub fingerprints: std::collections::BTreeMap<crate::types::ArtifactName, Fingerprint>,
    pub status: RequestStatus,
    pub created_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub completed_at: Option<Timestamp>,
    #[serde(with = "crate::platform::path_serde")]
    pub cwd: PathBuf,
    #[serde(default, with = "crate::platform::path_serde::option")]
    pub run_dir: Option<PathBuf>,
    pub argv: Option<Vec<String>>,
    pub child: Option<store::ChildIdentity>,
    pub result: Option<store::ExecutionResult>,
    pub error: Option<String>,
    pub error_code: Option<crate::types::FailureCode>,
    pub blocked_reason: Option<String>,
    /// Unfulfilled Evals behind this request’s dependency gates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<store::Blocker>,
    /// Absent from requests saved before it existed, which read as waiting for a job.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue: Option<QueueReason>,
}

impl TryFrom<RequestWire> for Request {
    type Error = String;
    fn try_from(wire: RequestWire) -> Result<Self, Self::Error> {
        let state = store::RequestState::from_parts(
            wire.status,
            wire.result,
            wire.error,
            wire.error_code,
            wire.completed_at,
        )?;
        Ok(Self {
            state,
            id: wire.id,
            run_id: wire.run_id,
            eval_id: wire.eval_id,
            target: wire.target,
            title: wire.title,
            profile: wire.profile,
            requested_profile: wire.requested_profile,
            eval_def_hash: wire.eval_def_hash,
            options: wire.options,
            execution_id: wire.execution_id,
            provenance: wire.provenance,
            usage: wire.usage,
            reused_usage: wire.reused_usage,
            joined: wire.joined,
            producer: wire.producer,
            reviewer: wire.reviewer,
            origin: wire.origin,
            session_id: wire.session_id,
            session: wire.session,
            human_definition: wire.human_definition,
            payload: wire.payload,
            references: wire.references,
            deps: wire.deps,
            force: wire.force,
            fingerprint: wire.fingerprint,
            key: wire.key,
            fingerprints: wire.fingerprints,
            created_at: wire.created_at,
            started_at: wire.started_at,
            cwd: wire.cwd,
            run_dir: wire.run_dir,
            argv: wire.argv,
            child: wire.child,
            blocked_reason: wire.blocked_reason,
            blocked_by: wire.blocked_by,
            queue: wire.queue,
        })
    }
}
impl From<Request> for RequestWire {
    fn from(record: Request) -> Self {
        Self {
            status: record.status(),
            result: record.result().cloned(),
            error: record.error().map(str::to_owned),
            error_code: record.error_code(),
            completed_at: record.completed_at(),
            id: record.id,
            run_id: record.run_id,
            eval_id: record.eval_id,
            target: record.target,
            title: record.title,
            profile: record.profile,
            requested_profile: record.requested_profile,
            eval_def_hash: record.eval_def_hash,
            options: record.options,
            execution_id: record.execution_id,
            provenance: record.provenance,
            usage: record.usage,
            reused_usage: record.reused_usage,
            joined: record.joined,
            producer: record.producer,
            reviewer: record.reviewer,
            origin: record.origin,
            session_id: record.session_id,
            session: record.session,
            human_definition: record.human_definition,
            payload: record.payload,
            references: record.references,
            deps: record.deps,
            force: record.force,
            fingerprint: record.fingerprint,
            key: record.key,
            fingerprints: record.fingerprints,
            created_at: record.created_at,
            started_at: record.started_at,
            cwd: record.cwd,
            run_dir: record.run_dir,
            argv: record.argv,
            child: record.child,
            blocked_reason: record.blocked_reason,
            blocked_by: record.blocked_by,
            queue: record.queue,
        }
    }
}
impl Request {
    pub fn status(&self) -> crate::types::RequestStatus {
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunView {
    #[serde(flatten)]
    pub run: Run,
    pub requests: Vec<std::sync::Arc<Request>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<store::Unreadable>,
}
