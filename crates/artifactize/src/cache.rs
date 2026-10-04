//! Stale key preparation, execution ownership, reuse, and end-of-review rechecks.

mod content;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

pub use crate::store::cache_entries::{Entry, list, remove, show};
pub(crate) use content::ignore_patterns;

use crate::{
    config::{EvalDeclaration, RepoConfig, StaleKey},
    process, runtime, scope,
    store::{Execution, Request},
    workspace,
};

const MANIFEST_BYTES: usize = 64 * 1024;

/// Hash the effective declaration after profile selection, without file fingerprints.
pub fn eval_definition_hash(eval: &EvalDeclaration) -> String {
    let mut definition = json!({
        "profile": eval.profile,
        "payload": eval.payload,
        "passSchema": eval.pass_schema,
        "failSchema": eval.fail_schema,
    });
    definition.sort_all_objects();
    content::hex(&Sha256::digest(
        serde_json::to_vec(&definition).expect("eval definition is JSON"),
    ))
}

/// A prepared stale key; a content stale key also carries its manifest.
#[derive(Debug, Clone)]
pub struct PreparedKey {
    pub value: String,
    pub manifest: Option<Manifest>,
}

/// What a content stale key covered, saved with executions to explain later changes.
/// Maps that would exceed 64 KiB are omitted; the stale key still covers them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// SHA-256 over every input path and file digest.
    pub inputs: String,
    /// Owner-relative path to the first 16 hex digits of its SHA-256.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files: Option<BTreeMap<String, String>>,
    /// Dependency Artifact to its stale key script value or content digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<BTreeMap<String, String>>,
}

/// Why the current stale key differs from an earlier cached result for the same Eval definition.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub since_run_id: String,
    /// `path` changed, `+path` added, `-path` removed; absent when not comparable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    /// `id` changed, `+id` added, `-id` removed; absent when not comparable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    pub summary: String,
}

/// Prepare only the selected dependency closure, once per Artifact.
pub async fn prepare<'a>(
    config: &RepoConfig,
    artifacts: impl IntoIterator<Item = &'a str>,
    output_root: &Path,
    cancellation: CancellationToken,
) -> Result<BTreeMap<&'a str, PreparedKey>, String> {
    let mut locals = BTreeMap::new();
    let mut keys = BTreeMap::new();
    for id in artifacts {
        if config.artifacts[id].stale_key.is_some() {
            keys.insert(
                id,
                compute(config, id, output_root, &cancellation, &mut locals).await?,
            );
        }
    }
    Ok(keys)
}

pub fn reuse(request: &mut Request, execution: &Execution, completed_at: String) {
    request.status = execution.status.clone();
    request.execution_id = Some(execution.id.clone());
    request.result = execution.result.clone();
    request.profile = execution.profile.clone();
    request.provenance = Some(execution.provenance.clone());
    request.usage = None;
    request.reused_usage = execution.usage.clone();
    request.producer = execution.producer.clone();
    request.reviewer = execution.reviewer.clone();
    request.origin = execution.origin.clone();
    request.tool_calls = execution.tool_calls.clone();
    request.completed_at = Some(completed_at);
    request.blocked_reason = None;
}

/// The returned value is the Artifact's whole stale key, shared by all of its Evals.
pub async fn stale_key(
    config: &RepoConfig,
    artifact_id: &str,
    output_root: &Path,
    cancellation: CancellationToken,
) -> Result<String, String> {
    compute(
        config,
        artifact_id,
        output_root,
        &cancellation,
        &mut BTreeMap::new(),
    )
    .await
    .map(|key| key.value)
}

/// `locals` memoizes each Artifact's own contribution within one preparation.
async fn compute(
    config: &RepoConfig,
    id: &str,
    output_root: &Path,
    cancellation: &CancellationToken,
    locals: &mut BTreeMap<String, String>,
) -> Result<PreparedKey, String> {
    match &config.artifacts[id].stale_key {
        Some(StaleKey::Content { .. }) => content(config, id, output_root, cancellation, locals)
            .await
            .map_err(|error| format!("Content stale key for Artifact {id} failed: {error}")),
        Some(StaleKey::Script { .. }) => Ok(PreparedKey {
            value: local(config, id, output_root, cancellation, locals).await?,
            manifest: None,
        }),
        None => Err(format!("No stale key declared for Artifact {id}.")),
    }
}

/// Own input files plus each dependency's own contribution, never a dependency's dependencies.
async fn content(
    config: &RepoConfig,
    id: &str,
    output_root: &Path,
    cancellation: &CancellationToken,
    locals: &mut BTreeMap<String, String>,
) -> Result<PreparedKey, String> {
    let Some(StaleKey::Content {
        inputs,
        dependencies: scope,
        ignore,
    }) = &config.artifacts[id].stale_key
    else {
        unreachable!("content stale key")
    };
    let files = content::files(config, id, inputs, ignore, cancellation).await?;
    locals.insert(id.to_owned(), files.digest.clone());
    let mut dependencies = BTreeMap::new();
    for dependency in content::dependencies(config, id, *scope) {
        let value = local(config, dependency, output_root, cancellation, locals).await?;
        dependencies.insert(dependency.to_owned(), value);
    }
    let mut digest = Sha256::new();
    digest.update(format!(
        "artifactize-content-v1\nartifact {id}\nfiles {}\n",
        files.digest
    ));
    for (dependency, value) in &dependencies {
        digest.update(format!("dependency {dependency} {value}\n"));
    }
    let mut manifest = Manifest {
        inputs: files.digest,
        files: Some(
            files
                .files
                .iter()
                .map(|(path, file)| (path.clone(), content::hex(&file[..8])))
                .collect(),
        ),
        dependencies: Some(dependencies),
    };
    let size = |manifest: &Manifest| {
        serde_json::to_vec(manifest)
            .expect("manifest is JSON")
            .len()
    };
    if size(&manifest) > MANIFEST_BYTES {
        manifest.files = None;
    }
    if size(&manifest) > MANIFEST_BYTES {
        manifest.dependencies = None;
    }
    Ok(PreparedKey {
        value: format!("content:{}", content::hex(&digest.finalize())),
        manifest: Some(manifest),
    })
}

/// A dependency's contribution: its stale key script value, or the digest of its own
/// content inputs (`.` without a declared stale key).
async fn local(
    config: &RepoConfig,
    id: &str,
    output_root: &Path,
    cancellation: &CancellationToken,
    locals: &mut BTreeMap<String, String>,
) -> Result<String, String> {
    if let Some(value) = locals.get(id) {
        return Ok(value.clone());
    }
    let value = match &config.artifacts[id].stale_key {
        Some(StaleKey::Script { .. }) => script(config, id, output_root, cancellation.clone())
            .await
            .map_err(|error| format!("Stale key script for Artifact {id} failed: {error}"))?,
        stale_key => {
            let (inputs, ignore) = match stale_key {
                Some(StaleKey::Content { inputs, ignore, .. }) => (inputs.clone(), ignore.clone()),
                _ => (vec![".".to_owned()], Vec::new()),
            };
            content::files(config, id, &inputs, &ignore, cancellation)
                .await
                .map_err(|error| format!("Content of Artifact {id} failed: {error}"))?
                .digest
        }
    };
    locals.insert(id.to_owned(), value.clone());
    Ok(value)
}

/// Explain a changed stale key against the manifest of an earlier cached execution.
pub fn changes(previous: &Execution, current: &PreparedKey) -> Changes {
    let compared = previous.manifest.as_ref().zip(current.manifest.as_ref());
    let files = compared.and_then(|(old, new)| {
        if old.inputs == new.inputs {
            Some(Vec::new())
        } else {
            old.files
                .as_ref()
                .zip(new.files.as_ref())
                .map(|(old, new)| diff(old, new))
        }
    });
    let dependencies = compared.and_then(|(old, new)| {
        old.dependencies
            .as_ref()
            .zip(new.dependencies.as_ref())
            .map(|(old, new)| diff(old, new))
    });
    let mut parts = Vec::new();
    match &files {
        Some(files) if !files.is_empty() => {
            let shown = files
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(match files.len() {
                0..=10 => format!("changed: {shown}"),
                total => format!("changed: {shown} and {} more", total - 10),
            });
        }
        None if compared.is_some() => parts.push("inputs changed".to_owned()),
        _ => {}
    }
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
        parts.push("stale key changed".to_owned());
    }
    Changes {
        since_run_id: previous.provenance.run_id.clone(),
        files,
        dependencies,
        summary: parts.join("; "),
    }
}

fn diff(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> Vec<String> {
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
) -> Result<String, String> {
    if cancellation.is_cancelled() {
        return Err(process::Error::Cancelled.to_string());
    }
    let artifact = &config.artifacts[artifact_id];
    let Some(StaleKey::Script {
        command,
        args,
        inputs,
        timeout_ms,
    }) = &artifact.stale_key
    else {
        unreachable!("stale key script")
    };
    let cwd = scope::scoped_path(&config.root, &artifact.path).map_err(|e| e.to_string())?;
    for input in inputs
        .iter()
        .chain(artifact.family.iter().flat_map(|family| &family.material))
    {
        scope::scoped_path(&cwd, Path::new(input)).map_err(|e| e.to_string())?;
    }
    let program = if !Path::new(command).is_absolute() && command.contains('/') {
        let relative = command.strip_prefix("./").unwrap_or(command);
        let program = scope::scoped_path(&cwd, Path::new(relative)).map_err(|e| e.to_string())?;
        if !program.is_file() {
            return Err("Stale key executable must be a regular file.".into());
        }
        program.into_os_string()
    } else {
        command.clone().into()
    };
    let scope = scope::argv_scope(config, artifact_id, args).map_err(|e| e.to_string())?;
    let args = scope::resolve_argv(config, &scope, artifact_id, args).map_err(|e| e.to_string())?;
    let mut input = json!({"version":1,"artifactId":artifact_id});
    if let Some(family) = &artifact.family {
        input["family"] = json!({"name":family.name,"material":family.material});
    }
    let output_root =
        workspace::prepare_directory(output_root, &config.root).map_err(|e| e.to_string())?;
    let disposable = tempfile::Builder::new()
        .prefix("stale-key-")
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
            return Err(format!("Stale key command exited with {}.", output.status));
        }
        if output.truncated {
            return Err("Stale key command output exceeded its limit.".into());
        }
        validate_output(&output.stdout)
    }
    .await;
    let cleaned = disposable.close();
    if cancellation.is_cancelled() {
        return Err(process::Error::Cancelled.to_string());
    }
    let value = result?;
    cleaned.map_err(|_| "Stale key validation failed.".to_owned())?;
    Ok(value)
}

fn validate_output(stdout: &[u8]) -> Result<String, String> {
    let value = stdout.strip_suffix(b"\n").unwrap_or(stdout);
    if !(1..=128).contains(&value.len())
        || !value
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err("stdout must contain 1–128 characters from [A-Za-z0-9._:-], with at most one trailing LF.".into());
    }
    Ok(String::from_utf8(value.to_vec()).expect("validated ASCII stale key"))
}

#[cfg(test)]
mod tests;
