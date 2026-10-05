//! Shared remote review store records: what leaves this machine and how it is mirrored back.

mod push;
mod session;
pub use push::{Push, push};
pub use session::Session;

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::store::{Execution, ExecutionOptions, Origin, Producer, Provenance};

/// Record schema version: 2 since records carry the 0.5 reuse key.
pub const SCHEMA: u32 = 2;
/// JSON byte limit of a summary record.
pub const MAX_SUMMARY_BYTES: usize = 256 * 1024;
/// JSON byte limit of a full record, the local per-entry cache limit.
pub const MAX_FULL_BYTES: usize = crate::store::cache_entries::MAX_ENTRY_BYTES;

/// One record of a reuse key's history; the store keeps every record and returns the latest.
///
/// A summary carries no argv, captured output, tool-call audit or repository path. A full
/// record additionally carries the saved `execution` as is. The server stamps `publisher`
/// and `publishedAt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schema: u32,
    pub key: String,
    pub eval_def_hash: String,
    /// Each Artifact the key covers, with its fingerprint.
    pub fingerprints: BTreeMap<String, String>,
    pub verdict: String,
    pub eval_id: String,
    pub run_id: String,
    pub request_id: String,
    pub execution_id: String,
    pub profile: Value,
    #[serde(default)]
    pub options: ExecutionOptions,
    /// An Agent result's tool `executionPaths` pins.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub execution_paths: crate::tools::pins::Pins,
    pub result: Value,
    pub usage: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<Producer>,
    pub started_at: String,
    pub completed_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Box<Execution>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
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
            verdict: execution.status.clone(),
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
            started_at: execution.started_at.clone(),
            completed_at: completed_at.clone(),
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
            && crate::cache::key(&self.eval_def_hash, &self.fingerprints) == self.key
            && matches!(self.verdict.as_str(), "GREEN" | "RED")
            && self.result["verdict"] == self.verdict.as_str()
            && matches!(
                self.profile["kind"].as_str(),
                Some("runtime" | "agent" | "human")
            )
            && valid_id(&self.execution_id)
            && crate::broker::sortable(&self.completed_at).is_some()
            && self.execution.as_ref().is_none_or(|execution| {
                execution.key.as_ref() == Some(&self.key)
                    && execution.eval_def_hash == self.eval_def_hash
                    && execution.fingerprints == self.fingerprints
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
        let id = format!("remote-{}", self.execution_id);
        let mut execution = match self.execution {
            Some(execution) => *execution,
            None => Execution {
                id: String::new(),
                key: Some(self.key),
                // The target is the eval id's Artifact part.
                fingerprint: self
                    .eval_id
                    .rsplit_once('/')
                    .and_then(|(target, _)| self.fingerprints.get(target))
                    .cloned(),
                fingerprints: self.fingerprints,
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
                tool_calls: Vec::new(),
                provenance: Provenance {
                    repo_path: PathBuf::new(),
                    run_id: self.run_id,
                    request_id: self.request_id,
                    eval_id: self.eval_id,
                    eval_def_hash: self.eval_def_hash,
                    completed_at: Some(self.completed_at.clone()),
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
            store: store.into(),
            publisher,
            published_at,
        });
        Ok(execution)
    }
}

fn summary_result(profile: &Value, result: &Value) -> Value {
    if profile["kind"] != "runtime" {
        // Agent and Human results are the schema-validated owner fields.
        return result.clone();
    }
    json!({
        "verdict": result["verdict"],
        "exitCode": result["exitCode"],
        "durationMs": result["durationMs"],
        "truncated": result["truncated"],
    })
}

fn summary_usage(usage: Option<&Value>) -> Option<Value> {
    let attempts = usage?.as_array()?;
    Some(
        attempts
            .iter()
            .map(|attempt| {
                json!({"turn": attempt["turn"], "attempt": attempt["attempt"], "usage": attempt["usage"]})
            })
            .collect(),
    )
}

pub(crate) fn valid_fingerprint(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_id(value: &str) -> bool {
    (1..=200).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
