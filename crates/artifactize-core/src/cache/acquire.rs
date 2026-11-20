//! Acquire and recheck fingerprints from Artifact inputs and author-defined scripts.

use super::{IgnorePatterns, content, dependencies, eval_key, scratch};
use crate::{
    config::{ArtifactName, Eval, Fingerprint, RepoConfig},
    process, runtime, scope, workspace,
};
use futures_util::{StreamExt, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Keep retained per-file provenance small; the content digest remains complete
/// even when the optional explain-changes manifest is too large to store.
const MANIFEST_BYTES: usize = 64 * 1024;

/// Fingerprint scripts receive version 1's Artifact identity stdin envelope. Its
/// explicit format version lets owner scripts distinguish future envelopes, not DB schemas.
const FINGERPRINT_INPUT_VERSION: u32 = 1;
/// Keep per-file change explanations compact with 8 digest bytes (16 hex characters).
/// This diagnostic prefix is not identity: artifactsum/reuse keys retain full SHA-256.
const MANIFEST_DIGEST_PREFIX_BYTES: usize = 8;

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
    pub inputs: crate::types::Sha256Digest,
    /// Owner-relative path to the first 16 hex digits of its SHA-256.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<BTreeMap<scope::LogicalTarget, crate::types::DigestPrefix>>,
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
    pub fn new(limit: usize) -> Result<Self, String> {
        let limit = limit.max(1);
        if limit > Semaphore::MAX_PERMITS {
            return Err(format!(
                "fingerprint jobs must be at most {}.",
                Semaphore::MAX_PERMITS
            ));
        }
        Ok(Self {
            slots: Arc::new(Semaphore::new(limit)),
            limit,
        })
    }

    /// The default bound: the CPUs available to this process.
    pub fn available() -> usize {
        std::thread::available_parallelism().map_or(1, usize::from)
    }

    pub fn limit(&self) -> usize {
        self.limit
    }
}

/// Every file visible to a review must still be its declared regular target.
pub(crate) fn validate_file_inputs(config: &RepoConfig, eval: &Eval) -> Result<(), String> {
    for id in dependencies(config, eval) {
        scope::validate_file_target(&config.root, &config.artifacts[id])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Prepare only the selected dependency closure, once per Artifact, computing up to the
/// parallelism's limit of fingerprints at a time. The result does not depend on completion
/// order: a failure reports the first failing Artifact in input order, and fingerprints after
/// it are cancelled because they cannot change which failure that is.
pub async fn prepare<'a>(
    config: &RepoConfig,
    artifacts: impl IntoIterator<Item = &'a ArtifactName>,
    output_root: &Path,
    parallelism: &Parallelism,
    cancellation: CancellationToken,
) -> Result<BTreeMap<&'a ArtifactName, PreparedFingerprint>, String> {
    let artifacts = artifacts.into_iter().collect::<Vec<_>>();
    for id in &artifacts {
        let artifact = config
            .artifacts
            .get(*id)
            .ok_or_else(|| format!("Unknown Artifact: {id}"))?;
        scope::validate_file_target(&config.root, artifact).map_err(|error| error.to_string())?;
    }
    let mut prepared = BTreeMap::new();
    prepare_each(
        config,
        artifacts,
        output_root,
        parallelism,
        cancellation,
        |id, fingerprint| {
            prepared.insert(id, fingerprint);
        },
    )
    .await?;
    Ok(prepared)
}

/// [`prepare`], handing each fingerprint to `ready` as soon as it is computed.
pub(crate) async fn prepare_each<'a>(
    config: &RepoConfig,
    artifacts: impl IntoIterator<Item = &'a ArtifactName>,
    output_root: &Path,
    parallelism: &Parallelism,
    cancellation: CancellationToken,
    mut ready: impl FnMut(&'a ArtifactName, PreparedFingerprint),
) -> Result<(), String> {
    // Missing or wrong-kind targets are per-eval preparation failures, not a global
    // admission failure. They have no key; the broker reports the precise path.
    let artifacts: Vec<_> = artifacts
        .into_iter()
        .filter(|id| scope::validate_file_target(&config.root, &config.artifacts[*id]).is_ok())
        .collect();
    let mut seen = BTreeSet::new();
    let ids: Vec<&'a ArtifactName> = artifacts
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
    let mut failure: Option<(usize, String)> = None;
    while let Some((index, result)) = running.next().await {
        match result {
            Ok(fingerprint) => ready(ids[index], fingerprint),
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
        None => Ok(()),
    }
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
    validate_file_inputs(config, eval)?;
    let fingerprints = prepare(
        config,
        dependencies(config, eval),
        output_root,
        parallelism,
        cancellation,
    )
    .await?;
    validate_file_inputs(config, eval)?;
    Ok(eval_key(config, eval, &fingerprints)
        .ok()
        .map(|key| key.value))
}

/// Keyless Live reviews retain and compare every available admitted snapshot too.
pub(crate) async fn recheck_observations(
    config: &RepoConfig,
    expected: &BTreeMap<ArtifactName, crate::types::Fingerprint>,
    output: &Path,
    parallelism: &Parallelism,
    cancellation: CancellationToken,
) -> Result<bool, String> {
    let current = prepare(config, expected.keys(), output, parallelism, cancellation).await?;
    Ok(expected.iter().all(|(name, value)| {
        current
            .get(name)
            .is_some_and(|current| current.value == *value)
    }))
}

async fn compute(
    config: &RepoConfig,
    id: &ArtifactName,
    output_root: &Path,
    cancellation: CancellationToken,
) -> Result<PreparedFingerprint, String> {
    scope::validate_file_target(&config.root, &config.artifacts[id])
        .map_err(|error| error.to_string())?;
    let result = match &config.artifacts[id].fingerprint {
        Some(Fingerprint::Graph) => Ok(PreparedFingerprint {
            value: crate::graph_structure::read(&config.root)?.fingerprint(),
            manifest: None,
        }),
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
    }?;
    scope::validate_file_target(&config.root, &config.artifacts[id])
        .map_err(|error| error.to_string())?;
    Ok(result)
}

/// The built-in hash of the Artifact's own input files, nothing else.
async fn content(
    config: &RepoConfig,
    id: &ArtifactName,
    inputs: &[crate::config::LogicalPath],
    ignore: &IgnorePatterns,
    cancellation: &CancellationToken,
) -> Result<PreparedFingerprint, String> {
    let files = content::files(config, id, inputs, ignore, cancellation).await?;
    Ok(artifactsum(files))
}

fn artifactsum(files: content::Files) -> PreparedFingerprint {
    let mut manifest = Manifest {
        inputs: files.digest.clone(),
        files: Some(
            files
                .files
                .iter()
                .map(|(path, file)| {
                    (
                        path.clone(),
                        content::hex(&file.as_bytes()[..MANIFEST_DIGEST_PREFIX_BYTES])
                            .parse()
                            .expect("eight-byte SHA-256 prefix"),
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
    PreparedFingerprint {
        value: format!("artifactsum:{}", files.digest)
            .parse()
            .expect("artifactsum digest is a fingerprint"),
        manifest: Some(manifest),
    }
}

async fn script(
    config: &RepoConfig,
    artifact_id: &ArtifactName,
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
    let cwd = scope::scoped_path(&config.root, artifact.folder()).map_err(|e| e.to_string())?;
    for input in files {
        scope::scoped_path(&cwd, Path::new(input)).map_err(|e| e.to_string())?;
    }
    let scope = scope::argv_scope(config, artifact_id, args).map_err(|e| e.to_string())?;
    let program = scope::executable(&config.root, &scope, artifact_id, command)
        .map_err(|error| format!("Fingerprint executable could not be spawned: {error}"))?;
    let args = scope::resolve_argv(config, &scope, artifact_id, args).map_err(|e| e.to_string())?;
    let input = json!({"version":FINGERPRINT_INPUT_VERSION,"artifactId":artifact_id});
    let output_root =
        workspace::prepare_directory(output_root, &config.root).map_err(|e| e.to_string())?;
    let disposable = scratch::Scratch::create(&output_root).map_err(|e| e.to_string())?;
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
    let (value, endings) = crate::platform::fingerprint_line_ending(stdout);
    if !(1..=crate::types::MAX_FINGERPRINT_BYTES).contains(&value.len())
        || !value
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err(format!(
            "stdout must contain 1–128 characters from [A-Za-z0-9._:-], with at most one trailing {endings}."
        ));
    }
    String::from_utf8(value.to_vec())
        .map_err(|error| error.to_string())?
        .parse()
}
