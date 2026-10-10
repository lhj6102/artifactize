//! Shared remote review store records: what leaves this machine and how it is mirrored back.

mod push;
mod session;
pub use push::{Push, push};
pub(crate) use session::REFRESH as REFRESH_INTERVAL;
pub use session::Session;

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::store::{Execution, ExecutionOptions, Origin, Producer, Provenance};

/// Record schema version: 2 since records carry the 0.5 reuse key.
pub const SCHEMA: u32 = 2;
/// Bound network/storage cost of reduced records, which omit large captured execution output.
pub const MAX_SUMMARY_BYTES: usize = 256 * 1024;
/// Full records must fit the same per-entry budget as the local reusable cache.
pub const MAX_FULL_BYTES: usize = crate::store::history::MAX_ENTRY_BYTES;

/// One record of a reuse key's history; the store keeps every record and returns the latest.
///
/// A summary carries no argv, captured output or repository path. A full record additionally
/// carries the saved `execution` as is; one an earlier artifactize published may still hold
/// its `toolCalls`, which readers ignore. The server stamps `publisher`
/// and `publishedAt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schema: u32,
    pub key: crate::types::ReuseKey,
    pub eval_def_hash: crate::types::DefinitionHash,
    /// Each Artifact the key covers, with its fingerprint.
    pub fingerprints: BTreeMap<crate::types::ArtifactName, crate::types::Fingerprint>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub artifact_kinds: BTreeMap<crate::types::ArtifactName, crate::config::ArtifactKind>,
    pub verdict: crate::types::ExecutionStatus,
    pub eval_id: crate::types::EvalId,
    pub run_id: crate::types::RunId,
    pub request_id: crate::types::RequestId,
    pub execution_id: crate::types::ExecutionId,
    pub profile: crate::config::StoredProfile,
    #[serde(default)]
    pub options: ExecutionOptions,
    /// An Agent result's tool `executionPaths` pins.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub execution_paths: crate::tools::pins::Pins,
    pub result: crate::store::ExecutionResult,
    pub usage: Option<Vec<crate::llm::Attempt>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<crate::types::ReviewerId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    pub started_at: crate::types::Timestamp,
    pub completed_at: crate::types::Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Box<Execution>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<crate::types::Timestamp>,
}

impl Record {
    /// Project a completed local GREEN/RED result with a key; `full` keeps the execution.
    pub fn new(execution: &Execution, full: bool) -> Result<Self, String> {
        let (Some(key), Some(_), Some(completed_at), Some(result)) = (
            &execution.key,
            execution.verdict(),
            &execution.completed_at,
            &execution.result,
        ) else {
            return Err("Only completed GREEN/RED results with a reuse key are shared.".into());
        };
        if execution.origin.is_some() {
            return Err("Mirrored results are never published again.".into());
        }
        let (result, usage) = if full {
            (result.clone(), execution.usage.clone())
        } else {
            (
                summary_result(&execution.profile, result),
                summary_usage(execution.usage.as_ref()),
            )
        };
        let record = Self {
            schema: SCHEMA,
            key: key.clone(),
            eval_def_hash: execution.eval_def_hash.clone(),
            fingerprints: execution.fingerprints.clone(),
            artifact_kinds: execution.artifact_kinds.clone(),
            verdict: execution.status,
            eval_id: execution.provenance.eval_id.clone(),
            run_id: execution.provenance.run_id.clone(),
            request_id: execution.provenance.request_id.clone(),
            execution_id: execution.id.clone(),
            profile: execution.profile.clone(),
            options: execution.options.clone(),
            execution_paths: execution.provenance.execution_paths.clone(),
            result,
            usage,
            reviewer: execution.reviewer.clone(),
            producer: execution.producer.clone(),
            started_at: execution.started_at,
            completed_at: *completed_at,
            execution: full.then(|| Box::new(execution.clone())),
            publisher: None,
            published_at: None,
        };
        record.validate()?;
        Ok(record)
    }

    /// Check shape, the key against its fingerprints, and size limits; never trusts the
    /// record's verdict.
    pub fn validate(&self) -> Result<(), String> {
        let valid = self.schema == SCHEMA
            && valid_hash(&self.key)
            && valid_hash(&self.eval_def_hash)
            && !self.fingerprints.is_empty()
            && self.fingerprints.iter().all(|(name, fingerprint)| {
                crate::config::identifier(name, "Artifact name").is_ok()
                    && valid_fingerprint(fingerprint)
            })
            && (self.artifact_kinds.is_empty()
                || self.artifact_kinds.keys().eq(self.fingerprints.keys()))
            && crate::cache::key_with_kinds(
                &self.eval_def_hash,
                &self.fingerprints,
                &self.artifact_kinds,
            ) == self.key
            && matches!(
                self.verdict,
                crate::types::ExecutionStatus::Green | crate::types::ExecutionStatus::Red
            )
            && self.result.verdict().as_str() == self.verdict.as_str()
            && self.execution_id.valid_wire()
            && valid_session(self.producer.as_ref())
            && self.execution.as_ref().is_none_or(|execution| {
                valid_session(execution.producer.as_ref())
                    && execution.key.as_ref() == Some(&self.key)
                    && execution.eval_def_hash == self.eval_def_hash
                    && execution.fingerprints == self.fingerprints
                    && execution.artifact_kinds == self.artifact_kinds
                    && execution.status == self.verdict
                    && execution.profile == self.profile
                    && execution.origin.is_none()
            });
        if !valid {
            return Err("Invalid remote review record.".into());
        }
        let limit = if self.execution.is_some() {
            MAX_FULL_BYTES
        } else {
            MAX_SUMMARY_BYTES
        };
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > limit {
            return Err(format!("Remote review record exceeds {limit} bytes."));
        }
        Ok(())
    }

    /// Rebuild a self-contained execution for the local cache, marked with its origin.
    pub fn mirror(self, store: &str) -> Result<Execution, String> {
        self.validate()?;
        let (Some(publisher), Some(published_at)) = (self.publisher, self.published_at) else {
            return Err("Remote review record has no server publisher.".into());
        };
        let id = self.execution_id.remote_mirror()?;
        let mut execution = match self.execution {
            Some(execution) => *execution,
            None => Execution {
                id: id.clone(),
                key: Some(self.key),
                // The target is the eval id's Artifact part.
                fingerprint: self
                    .eval_id
                    .rsplit_once('/')
                    .and_then(|(target, _)| self.fingerprints.get(target))
                    .cloned(),
                fingerprints: self.fingerprints,
                artifact_kinds: self.artifact_kinds,
                eval_def_hash: self.eval_def_hash.clone(),
                owner_pid: 0,
                owner_start_time: 0,
                status: self.verdict,
                result: Some(self.result),
                error: None,
                error_code: None,
                profile: self.profile,
                options: self.options,
                usage: self.usage,
                provenance: Provenance {
                    repo_path: PathBuf::new(),
                    run_id: self.run_id,
                    request_id: self.request_id,
                    eval_id: self.eval_id,
                    eval_def_hash: self.eval_def_hash,
                    completed_at: Some(self.completed_at),
                    execution_paths: self.execution_paths,
                },
                started_at: self.started_at,
                completed_at: Some(self.completed_at),
                producer: self.producer,
                reviewer: self.reviewer,
                origin: None,
                manifest: None,
            },
        };
        execution.id = id;
        execution.origin = Some(Origin {
            store: store.parse()?,
            publisher,
            published_at,
        });
        Ok(execution)
    }
}

fn summary_result(
    profile: &crate::config::StoredProfile,
    result: &crate::store::ExecutionResult,
) -> crate::store::ExecutionResult {
    if profile.kind() == crate::config::ProfileKind::Runtime
        && let crate::store::ExecutionResult::Runtime(runtime) = result
    {
        return crate::store::ExecutionResult::Runtime(runtime.summary());
    }
    result.clone()
}

fn summary_usage(usage: Option<&Vec<crate::llm::Attempt>>) -> Option<Vec<crate::llm::Attempt>> {
    let attempts = usage?;
    Some(
        attempts
            .iter()
            .map(|attempt| crate::llm::Attempt {
                turn: attempt.turn,
                attempt: attempt.attempt,
                usage: attempt.usage.clone(),
                error: None,
                error_code: None,
            })
            .collect(),
    )
}

pub(crate) fn valid_fingerprint(value: &str) -> bool {
    value.parse::<crate::types::Fingerprint>().is_ok()
}

pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == crate::types::SHA256_HEX_BYTES
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
#[test]
fn sha256_hash_wire_format_keeps_exact_width_and_lowercase() {
    assert!(valid_hash(&"a".repeat(crate::types::SHA256_HEX_BYTES)));
    for hash in [
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        assert!(!valid_hash(&hash), "{hash}");
    }
}

/// A producer's session reference, if any, names its conversation in bounded, printable
/// fields. It is a pointer back to the producing machine; no conversation is ever published.
fn valid_session(producer: Option<&Producer>) -> bool {
    producer
        .and_then(|producer| producer.session.as_ref())
        .is_none_or(crate::agent::session::SessionRef::valid)
}
