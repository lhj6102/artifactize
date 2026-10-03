//! Identity commands and end-of-review rechecks. Reuse and claims are added separately.

use std::path::Path;

use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    config::{RepoConfig, Stale},
    process, runtime, scope, workspace,
};

/// The returned owner value is the whole identity, shared by all of its Evals.
pub async fn identity(
    config: &RepoConfig,
    artifact_id: &str,
    output_root: &Path,
    cancellation: CancellationToken,
) -> Result<String, String> {
    compute(config, artifact_id, output_root, cancellation)
        .await
        .map_err(|error| format!("Identity script for Artifact {artifact_id} failed: {error}"))
}

async fn compute(
    config: &RepoConfig,
    artifact_id: &str,
    output_root: &Path,
    cancellation: CancellationToken,
) -> Result<String, String> {
    if cancellation.is_cancelled() {
        return Err(process::Error::Cancelled.to_string());
    }
    let artifact = &config.artifacts[artifact_id];
    let Some(Stale::Identity {
        script,
        inputs,
        timeout_ms,
    }) = &artifact.stale
    else {
        return Err("No identity command declared.".into());
    };
    let cwd = scope::scoped_path(&config.root, &artifact.path).map_err(|e| e.to_string())?;
    for input in inputs
        .iter()
        .chain(artifact.family.iter().flat_map(|family| &family.material))
    {
        scope::scoped_path(&cwd, Path::new(input)).map_err(|e| e.to_string())?;
    }
    let program = if !Path::new(&script.command).is_absolute() && script.command.contains('/') {
        let relative = script.command.strip_prefix("./").unwrap_or(&script.command);
        let program = scope::scoped_path(&cwd, Path::new(relative)).map_err(|e| e.to_string())?;
        if !program.is_file() {
            return Err("Identity executable must be a regular file.".into());
        }
        program.into_os_string()
    } else {
        script.command.clone().into()
    };
    let scope = scope::artifact_scope(config, &[artifact_id]).map_err(|e| e.to_string())?;
    let args = scope::resolve_argv(config, &scope, artifact_id, &script.args)
        .map_err(|e| e.to_string())?;
    let mut input = json!({"version":1,"artifactId":artifact_id});
    if let Some(family) = &artifact.family {
        input["family"] = json!({"name":family.name,"material":family.material});
    }
    let output_root =
        workspace::prepare_directory(output_root, &config.root).map_err(|e| e.to_string())?;
    let disposable = tempfile::Builder::new()
        .prefix("identity-")
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
            return Err(format!("Identity command exited with {}.", output.status));
        }
        if output.truncated {
            return Err("Identity command output exceeded its limit.".into());
        }
        validate_output(&output.stdout)
    }
    .await;
    let cleaned = disposable.close();
    if cancellation.is_cancelled() {
        return Err(process::Error::Cancelled.to_string());
    }
    let value = result?;
    cleaned.map_err(|_| "Identity validation failed.".to_owned())?;
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
    Ok(String::from_utf8(value.to_vec()).expect("validated ASCII identity"))
}
