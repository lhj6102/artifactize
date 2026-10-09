//! Fingerprints, reuse keys, execution ownership, reuse, and end-of-review rechecks.

mod content;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use futures_util::{StreamExt, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

pub use crate::store::history::{Entry, list, remove, show};
pub(crate) use content::{HASH_BUFFER_BYTES, ignore_patterns};

use crate::{
    config::{Eval, EvalDeclaration, Fingerprint, Profile, RepoConfig},
    process, runtime, scope,
    store::{Execution, Request},
    workspace,
};

/// Keep retained per-file provenance small; the content digest remains complete
/// even when the optional explain-changes manifest is too large to store.
const MANIFEST_BYTES: usize = 64 * 1024;
/// Keep change explanations readable in CLI output; the complete path list remains in Changes.
const MAX_SUMMARY_PATHS: usize = 10;
/// Fingerprint scripts receive version 1's Artifact identity stdin envelope. Its
/// explicit format version lets owner scripts distinguish future envelopes, not DB schemas.
const FINGERPRINT_INPUT_VERSION: u32 = 1;
/// Keep per-file change explanations compact with 8 digest bytes (16 hex characters).
/// This diagnostic prefix is not identity: artifactsum/reuse keys retain full SHA-256.
const MANIFEST_DIGEST_PREFIX_BYTES: usize = 8;
/// Domain-separate and version the reuse-key hash input, including its terminating newline.
/// Changing these bytes invalidates existing reuse keys; this is not a config/session/DB version.
const REUSE_KEY_FORMAT_PREFIX: &str = "artifactize-key-v2\n";

/// Hash the eval strategy: what is asked and how the answer is judged, never how the eval
/// is executed. Execution options (backend, model, reasoning, limits, the profile variant),
/// the eval's id and title, and tool views stay out.
pub fn eval_definition_hash(eval: &EvalDeclaration) -> String {
    let kind = match &eval.profile {
        Profile::Agent { .. } => "agent",
        Profile::Human {} => "human",
        Profile::Dependency { .. } => "dependency",
        Profile::Runtime { .. } => "runtime",
    };
    let mut strategy = json!({
        "kind": kind,
        "payload": eval.payload,
        "passSchema": eval.pass_schema,
        "failSchema": eval.fail_schema,
    });
    if let Profile::Runtime { command, args, .. } = &eval.profile {
        strategy["command"] = json!(command);
        strategy["args"] = json!(args);
    }
    if let Profile::Dependency { depends_on } = &eval.profile {
        strategy["dependsOn"] = json!(depends_on);
    }
    strategy.sort_all_objects();
    content::hex(&Sha256::digest(
        serde_json::to_vec(&strategy).expect("eval strategy is JSON"),
    ))
}

/// A prepared fingerprint; artifactsum also carries its manifest.
#[derive(Debug, Clone)]
pub struct PreparedFingerprint {
    pub value: crate::types::Fingerprint,
    pub manifest: Option<Manifest>,
}

/// What artifactsum covered, saved with executions to explain later changes.
/// A file map that would exceed 64 KiB is omitted; the fingerprint still covers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// SHA-256 over every input path and file digest: the fingerprint without its prefix.
    pub inputs: String,
    /// Owner-relative path to the first 16 hex digits of its SHA-256.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<BTreeMap<String, String>>,
}

/// An eval's reuse key: `hash(eval strategy, sorted (name, fingerprint) of the Artifacts it
/// depends on)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub value: crate::types::ReuseKey,
    pub eval_def_hash: String,
    /// Each Artifact the eval depends on, its target included, with its fingerprint.
    pub fingerprints: BTreeMap<String, crate::types::Fingerprint>,
}

/// Why an eval has no reuse key: an Artifact it depends on declares `fingerprint: false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unkeyed {
    /// Dependency Evals are always derived, never cached.
    Derived,
    /// The eval's own target.
    Target,
    /// A mount, child or referenced Artifact.
    Dependency(String),
}

/// Why the current key differs from an earlier cached result for the same eval and Eval
/// definition.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub since_run_id: crate::types::RunId,
    /// The target's own files: `path` changed, `+path` added, `-path` removed; absent when
    /// not comparable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    /// Dependency Artifacts: `name` changed, `+name` added, `-name` removed; absent when none
    /// changed or not comparable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    pub summary: String,
}

/// The Artifacts an eval depends on: its target, the target's mounts and child Artifacts, and
/// the Artifacts the eval names in its instruction or runtime args. Connections further away
/// count only through how the developer defines fingerprints.
pub fn dependencies<'a>(config: &'a RepoConfig, eval: &'a Eval) -> BTreeSet<&'a str> {
    let target = &config.artifacts[&eval.target];
    std::iter::once(eval.target.as_str())
        .chain(target.mounts.values().map(String::as_str))
        .chain(target.children.values().map(String::as_str))
        .chain(eval.deps.iter().map(String::as_str))
        .collect()
}

/// Only executable Evals need fingerprints; a derived verdict never runs a fingerprint
/// script solely for its own request. Restrict preparation to the required key inputs.
pub(crate) fn fingerprint_targets<'a>(
    config: &'a RepoConfig,
    required: &BTreeSet<&'a str>,
) -> BTreeSet<&'a str> {
    let dependency_scope: BTreeSet<_> = config
        .evals
        .iter()
        .filter(|eval| {
            required.contains(eval.target.as_str())
                && matches!(eval.declaration.profile, Profile::Dependency { .. })
        })
        .flat_map(|eval| {
            std::iter::once(eval.target.as_str()).chain(eval.deps.iter().map(String::as_str))
        })
        .collect();
    required
        .iter()
        .copied()
        .filter(|id| !dependency_scope.contains(id))
        .chain(
            config
                .evals
                .iter()
                .filter(|eval| {
                    required.contains(eval.target.as_str())
                        && !matches!(eval.declaration.profile, Profile::Dependency { .. })
                })
                .flat_map(|eval| dependencies(config, eval)),
        )
        .collect()
}

/// The reuse key of an Eval definition hash over these fingerprints.
pub fn key(
    eval_def_hash: &str,
    fingerprints: &BTreeMap<String, crate::types::Fingerprint>,
) -> crate::types::ReuseKey {
    let mut digest = Sha256::new();
    digest.update(format!("{REUSE_KEY_FORMAT_PREFIX}eval {eval_def_hash}\n"));
    // Names are identifiers and fingerprints never contain spaces or line breaks.
    for (name, fingerprint) in fingerprints {
        digest.update(format!("artifact {name} {fingerprint}\n"));
    }
    content::hex(&digest.finalize())
        .parse()
        .expect("SHA-256 is a reuse key")
}

/// The eval's reuse key from prepared fingerprints. Without a fingerprint on every Artifact
/// it depends on, the eval has no key, so it is never reused or published.
pub fn eval_key(
    config: &RepoConfig,
    eval: &Eval,
    fingerprints: &BTreeMap<&str, PreparedFingerprint>,
) -> Result<Key, Unkeyed> {
    if matches!(eval.declaration.profile, Profile::Dependency { .. }) {
        return Err(Unkeyed::Derived);
    }
    if !fingerprints.contains_key(eval.target.as_str()) {
        return Err(Unkeyed::Target);
    }
    let mut values = BTreeMap::new();
    for id in dependencies(config, eval) {
        let fingerprint = fingerprints
            .get(id)
            .ok_or_else(|| Unkeyed::Dependency(id.to_owned()))?;
        values.insert(id.to_owned(), fingerprint.value.clone());
    }
    let eval_def_hash = eval_definition_hash(&eval.declaration);
    Ok(Key {
        value: key(&eval_def_hash, &values),
        eval_def_hash,
        fingerprints: values,
    })
}

/// Keys of every eval whose dependency Artifacts were all prepared, by eval id.
pub fn eval_keys<'a>(
    config: &'a RepoConfig,
    fingerprints: &BTreeMap<&str, PreparedFingerprint>,
) -> BTreeMap<&'a str, Key> {
    config
        .evals
        .iter()
        .filter_map(|eval| Some((eval.id.as_str(), eval_key(config, eval, fingerprints).ok()?)))
        .collect()
}

/// How many fingerprints a process computes at once: one bound shared by a Run's
/// preparation and its end-of-review rechecks, or by `status`.
#[derive(Debug, Clone)]
pub struct Parallelism {
    slots: Arc<Semaphore>,
    limit: usize,
}

impl Parallelism {
    /// At most `limit` fingerprints at a time; at least one.
    pub fn new(limit: usize) -> Self {
        let limit = limit.max(1);
        Self {
            slots: Arc::new(Semaphore::new(limit)),
            limit,
        }
    }

    /// The default bound: the CPUs available to this process.
    pub fn available() -> usize {
        std::thread::available_parallelism().map_or(1, usize::from)
    }

    pub fn limit(&self) -> usize {
        self.limit
    }
}

/// Prepare only the selected dependency closure, once per Artifact, computing up to the
/// parallelism's limit of fingerprints at a time. The result does not depend on completion
/// order: a failure reports the first failing Artifact in input order, and fingerprints after
/// it are cancelled because they cannot change which failure that is.
pub async fn prepare<'a>(
    config: &RepoConfig,
    artifacts: impl IntoIterator<Item = &'a str>,
    output_root: &Path,
    parallelism: &Parallelism,
    cancellation: CancellationToken,
) -> Result<BTreeMap<&'a str, PreparedFingerprint>, String> {
    let mut seen = BTreeSet::new();
    let ids: Vec<&'a str> = artifacts
        .into_iter()
        .filter(|id| config.artifacts[*id].fingerprint.is_some() && seen.insert(*id))
        .collect();
    let tokens: Vec<_> = ids.iter().map(|_| cancellation.child_token()).collect();
    let mut running: FuturesUnordered<_> = ids
        .iter()
        .zip(&tokens)
        .enumerate()
        .map(|(index, (id, token))| async move {
            let result = match parallelism.slots.acquire().await {
                // A slot can be waited for long; a cancelled fingerprint never starts.
                Ok(_slot) if !token.is_cancelled() => {
                    compute(config, id, output_root, token.clone()).await
                }
                _ => Err(process::Error::Cancelled.to_string()),
            };
            (index, result)
        })
        .collect();
    let mut prepared = BTreeMap::new();
    let mut failure: Option<(usize, String)> = None;
    while let Some((index, result)) = running.next().await {
        match result {
            Ok(fingerprint) => {
                prepared.insert(ids[index], fingerprint);
            }
            Err(error) if failure.as_ref().is_none_or(|(first, _)| index < *first) => {
                for token in &tokens[index + 1..] {
                    token.cancel();
                }
                failure = Some((index, error));
            }
            Err(_) => {}
        }
    }
    match failure {
        Some((_, error)) => Err(error),
        None => Ok(prepared),
    }
}

/// Give a request another execution's result. A request that waited for this very
/// execution (a live owner it joined, or the Human wait it followed) joined it; any other
/// found a completed record.
pub fn reuse(request: &mut Request, execution: &Execution, completed_at: String) {
    request.joined = request.execution_id.as_ref() == Some(&execution.id);
    request.status = execution.status.into();
    request.execution_id = Some(execution.id.clone());
    request.result = execution.result.clone();
    request.profile = execution.profile.clone();
    request.options = execution.options.clone();
    request.provenance = Some(execution.provenance.clone());
    request.usage = None;
    request.reused_usage = execution.usage.clone();
    request.producer = execution.producer.clone();
    request.session = execution
        .producer
        .as_ref()
        .and_then(|producer| producer.session.clone());
    request.reviewer = execution.reviewer.clone();
    request.origin = execution.origin.clone();
    request.completed_at = Some(completed_at);
    request.blocked_reason = None;
}

/// Recompute the eval's key after a review: every Artifact it depends on is fingerprinted
/// again, so a change to any of them during the review is detected.
pub async fn recheck(
    config: &RepoConfig,
    eval: &Eval,
    output_root: &Path,
    parallelism: &Parallelism,
    cancellation: CancellationToken,
) -> Result<Option<crate::types::ReuseKey>, String> {
    let fingerprints = prepare(
        config,
        dependencies(config, eval),
        output_root,
        parallelism,
        cancellation,
    )
    .await?;
    Ok(eval_key(config, eval, &fingerprints)
        .ok()
        .map(|key| key.value))
}

async fn compute(
    config: &RepoConfig,
    id: &str,
    output_root: &Path,
    cancellation: CancellationToken,
) -> Result<PreparedFingerprint, String> {
    match &config.artifacts[id].fingerprint {
        Some(Fingerprint::Artifactsum { files, ignore }) => {
            content(config, id, files, ignore, &cancellation)
                .await
                .map_err(|error| format!("Artifactsum for Artifact {id} failed: {error}"))
        }
        Some(Fingerprint::Script { .. }) => Ok(PreparedFingerprint {
            value: script(config, id, output_root, cancellation)
                .await
                .map_err(|error| format!("Fingerprint script for Artifact {id} failed: {error}"))?,
            manifest: None,
        }),
        None => Err(format!("Artifact {id} declares fingerprint: false.")),
    }
}

/// The built-in hash of the Artifact's own input files, nothing else.
async fn content(
    config: &RepoConfig,
    id: &str,
    inputs: &[String],
    ignore: &[String],
    cancellation: &CancellationToken,
) -> Result<PreparedFingerprint, String> {
    let files = content::files(config, id, inputs, ignore, cancellation).await?;
    let mut manifest = Manifest {
        inputs: files.digest.clone(),
        files: Some(
            files
                .files
                .iter()
                .map(|(path, file)| {
                    (
                        path.clone(),
                        content::hex(&file[..MANIFEST_DIGEST_PREFIX_BYTES]),
                    )
                })
                .collect(),
        ),
    };
    if serde_json::to_vec(&manifest)
        .expect("manifest is JSON")
        .len()
        > MANIFEST_BYTES
    {
        manifest.files = None;
    }
    Ok(PreparedFingerprint {
        value: format!("artifactsum:{}", files.digest)
            .parse()
            .expect("artifactsum digest is a fingerprint"),
        manifest: Some(manifest),
    })
}

/// Explain a changed key against an earlier cached execution of the same Eval definition:
/// which of the target's files changed, and which dependency Artifacts' fingerprints changed.
pub fn changes(
    previous: &Execution,
    target: &str,
    current: &Key,
    manifest: Option<&Manifest>,
) -> Changes {
    let mut parts = Vec::new();
    let mut files = None;
    if previous.fingerprints.get(target) != current.fingerprints.get(target) {
        match previous.manifest.as_ref().zip(manifest) {
            Some((old, new)) if old.inputs == new.inputs => files = Some(Vec::new()),
            Some((old, new)) => {
                files = old
                    .files
                    .as_ref()
                    .zip(new.files.as_ref())
                    .map(|(old, new)| diff(old, new));
                match &files {
                    Some(files) if !files.is_empty() => {
                        let shown = files
                            .iter()
                            .take(MAX_SUMMARY_PATHS)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        parts.push(match files.len() {
                            0..=MAX_SUMMARY_PATHS => format!("changed: {shown}"),
                            total => {
                                format!("changed: {shown} and {} more", total - MAX_SUMMARY_PATHS)
                            }
                        });
                    }
                    _ => parts.push("inputs changed".to_owned()),
                }
            }
            None => parts.push("fingerprint changed".to_owned()),
        }
    }
    let dependencies = (!previous.fingerprints.is_empty())
        .then(|| {
            let others = |map: &BTreeMap<String, crate::types::Fingerprint>| {
                map.iter()
                    .filter(|(name, _)| *name != target)
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<BTreeMap<_, _>>()
            };
            diff(
                &others(&previous.fingerprints),
                &others(&current.fingerprints),
            )
        })
        .filter(|dependencies| !dependencies.is_empty());
    for dependency in dependencies.iter().flatten() {
        parts.push(if let Some(id) = dependency.strip_prefix('+') {
            format!("dependency {id} added")
        } else if let Some(id) = dependency.strip_prefix('-') {
            format!("dependency {id} removed")
        } else {
            format!("dependency {dependency} changed")
        });
    }
    if parts.is_empty() {
        parts.push("fingerprint changed".to_owned());
    }
    Changes {
        since_run_id: previous.provenance.run_id.clone(),
        files,
        dependencies,
        summary: parts.join("; "),
    }
}

fn diff<T: PartialEq>(old: &BTreeMap<String, T>, new: &BTreeMap<String, T>) -> Vec<String> {
    old.keys()
        .chain(new.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|key| match (old.get(key), new.get(key)) {
            (Some(old), Some(new)) if old != new => Some(key.clone()),
            (None, Some(_)) => Some(format!("+{key}")),
            (Some(_), None) => Some(format!("-{key}")),
            _ => None,
        })
        .collect()
}

async fn script(
    config: &RepoConfig,
    artifact_id: &str,
    output_root: &Path,
    cancellation: CancellationToken,
) -> Result<crate::types::Fingerprint, String> {
    if cancellation.is_cancelled() {
        return Err(process::Error::Cancelled.to_string());
    }
    let artifact = &config.artifacts[artifact_id];
    let Some(Fingerprint::Script {
        command,
        args,
        files,
        timeout_ms,
    }) = &artifact.fingerprint
    else {
        unreachable!("fingerprint script")
    };
    let cwd = scope::scoped_path(&config.root, &artifact.path).map_err(|e| e.to_string())?;
    for input in files {
        scope::scoped_path(&cwd, Path::new(input)).map_err(|e| e.to_string())?;
    }
    let program = if !Path::new(command).is_absolute() && command.contains('/') {
        let relative = command.strip_prefix("./").unwrap_or(command);
        let program = scope::scoped_path(&cwd, Path::new(relative)).map_err(|e| e.to_string())?;
        if !program.is_file() {
            return Err("Fingerprint executable must be a regular file.".into());
        }
        program.into_os_string()
    } else {
        command.clone().into()
    };
    let scope = scope::argv_scope(config, artifact_id, args).map_err(|e| e.to_string())?;
    let args = scope::resolve_argv(config, &scope, artifact_id, args).map_err(|e| e.to_string())?;
    let input = json!({"version":FINGERPRINT_INPUT_VERSION,"artifactId":artifact_id});
    let output_root =
        workspace::prepare_directory(output_root, &config.root).map_err(|e| e.to_string())?;
    let disposable = tempfile::Builder::new()
        .prefix("fingerprint-")
        .tempdir_in(output_root)
        .map_err(|e| e.to_string())?;
    let result = async {
        let mut command = runtime::Command::prepare(
            program,
            args.iter().map(Into::into).collect(),
            &config.root,
            disposable.path(),
            *timeout_ms,
        )
        .map_err(|e| e.to_string())?;
        command.cwd = cwd;
        let output = command
            .output(input.to_string().into_bytes(), cancellation.clone())
            .await
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "Fingerprint command exited with {}.",
                output.status
            ));
        }
        if output.truncated {
            return Err("Fingerprint command output exceeded its limit.".into());
        }
        validate_output(&output.stdout)
    }
    .await;
    let cleaned = disposable.close();
    if cancellation.is_cancelled() {
        return Err(process::Error::Cancelled.to_string());
    }
    let value = result?;
    cleaned.map_err(|_| "Fingerprint validation failed.".to_owned())?;
    Ok(value)
}

fn validate_output(stdout: &[u8]) -> Result<crate::types::Fingerprint, String> {
    // Windows programs end a line with CRLF, as Python's print does there; the value is the
    // same as from an LF-ending Unix script.
    let value = match stdout.strip_suffix(b"\r\n") {
        Some(value) if cfg!(windows) => value,
        _ => stdout.strip_suffix(b"\n").unwrap_or(stdout),
    };
    if !(1..=crate::types::MAX_FINGERPRINT_BYTES).contains(&value.len())
        || !value
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(if cfg!(windows) {
            "stdout must contain 1–128 characters from [A-Za-z0-9._:-], with at most one trailing LF or CRLF.".into()
        } else {
            "stdout must contain 1–128 characters from [A-Za-z0-9._:-], with at most one trailing LF.".into()
        });
    }
    String::from_utf8(value.to_vec())
        .map_err(|error| error.to_string())?
        .parse()
}

#[cfg(test)]
mod tests;
