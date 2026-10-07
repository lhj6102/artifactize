//! Human waiting, claim locks, tools, and submission.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::verdict::validate_result,
    cache,
    config::{Eval, RepoConfig, read_workspace_config},
    remote::Session,
    scope,
    store::{self, HumanClaim, Receipts, Request},
    tools::human::{CommandLine, Registry, ToolResult},
};

/// Keep reviewer labels bounded in audit records and terminal layouts while allowing
/// printable Unicode user names rather than restricting them to path identities.
const MAX_REVIEWER_BYTES: usize = 200;
/// Bound owner fields and serialized Human results consistently across CLI, editor
/// and submission so untrusted JSON cannot grow the review audit without limit.
pub(crate) const MAX_RESULT_BYTES: usize = 256_000;
/// Read one sentinel byte beyond the bound to detect oversized files without loading them.
pub(crate) const FIELDS_READ_BYTES: u64 = MAX_RESULT_BYTES as u64 + 1;

pub fn default_reviewer() -> Result<String, String> {
    // Windows names the signed-in user in USERNAME and sets no USER.
    let reviewer = std::env::var("USER")
        .or_else(|error| {
            if cfg!(windows) {
                std::env::var("USERNAME")
            } else {
                Err(error)
            }
        })
        .map_err(|_| "Set USER or provide a reviewer id.")?;
    validate_reviewer(&reviewer)?;
    Ok(reviewer)
}

pub(crate) fn validate_reviewer(reviewer: &str) -> Result<(), String> {
    if reviewer.trim().is_empty()
        || reviewer.len() > MAX_REVIEWER_BYTES
        || reviewer.chars().any(char::is_control)
    {
        return Err("A reviewer id of 1–200 bytes without control characters is required.".into());
    }
    Ok(())
}

/// The saved request's Run repository and its receipts, where its Human actions are recorded.
pub async fn open(state: &Path, request: &str) -> Result<(Receipts, PathBuf), String> {
    let view = store::read_request(state, request).await?;
    let repo = store::read_run(state, &view.request.run_id)
        .await?
        .run
        .repo_path;
    Ok((Receipts::open(state, &repo).await?, repo))
}

pub async fn claim(
    receipts: &Receipts,
    request: &str,
    reviewer: &str,
) -> Result<HumanClaim, String> {
    validate_reviewer(reviewer)?;
    receipts.claim_human(request, reviewer).await
}

/// Release the reviewer lock without a verdict, so another reviewer can claim the request.
pub async fn unclaim(
    receipts: &Receipts,
    request: &str,
    reviewer: &str,
) -> Result<HumanClaim, String> {
    validate_reviewer(reviewer)?;
    receipts.release_human(request, reviewer).await
}

pub async fn run_human_tool(
    receipts: &Receipts,
    request: &str,
    reviewer: &str,
    tool: &str,
    cancellation: CancellationToken,
) -> Result<ToolResult, String> {
    validate_reviewer(reviewer)?;
    let (mut request, _) = receipts.human_request(request, reviewer).await?;
    let config = reconnect(&request)?;
    let registry = Registry::new(&config, &request.eval_id)?;
    if !registry.list().any(|entry| entry.name == tool) {
        return Err("Unknown registered Human tool.".into());
    }
    recheck(
        receipts,
        &mut request,
        reviewer,
        &config,
        cancellation.clone(),
    )
    .await?;
    receipts.human_request(&request.id, reviewer).await?;
    Ok(registry.call(tool, cancellation).await)
}

/// What a registered Human tool of a waiting request would run; needs no claim and runs nothing.
pub async fn tool_command(
    receipts: &Receipts,
    request: &str,
    tool: &str,
) -> Result<CommandLine, String> {
    let request = receipts.waiting_human(request).await?;
    let config = reconnect(&request)?;
    Registry::new(&config, &request.eval_id)?.command(tool)
}

/// Invalid results remain correctable; only a valid submission performs the final fingerprint check.
pub async fn submit(
    receipts: &Receipts,
    request: &str,
    reviewer: &str,
    result: &Value,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    validate_reviewer(reviewer)?;
    let (mut request, _) = receipts.human_request(request, reviewer).await?;
    let config = reconnect(&request)?;
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.id == request.eval_id)
        .expect("reconnected eval");
    if serde_json::to_vec(result).map_err(|e| e.to_string())?.len() > MAX_RESULT_BYTES {
        return Err("Human result exceeds 256000 bytes.".into());
    }
    let result = validate_result(&eval.declaration, result)?;
    recheck(receipts, &mut request, reviewer, &config, cancellation).await?;
    request.status = result["verdict"]
        .as_str()
        .ok_or("Validated result has no verdict.")?
        .parse()?;
    request.result = Some(result);
    receipts.settle_human(&request, reviewer).await
}

/// `submit`, then publish the settled result to the configured remote review store.
pub async fn submit_and_publish(
    state: &Path,
    request: &str,
    reviewer: &str,
    result: &Value,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    let (receipts, repo) = open(state, request).await?;
    let remote = Session::open(Some(state), Some(&repo))?;
    let request = submit(&receipts, request, reviewer, result, cancellation).await?;
    if let Some(remote) = remote {
        remote
            .publish_request(&receipts, &request)
            .await
            .map_err(|error| {
                format!("The Human result is saved locally, but publishing it failed: {error}")
            })?;
    }
    Ok(request)
}

pub(crate) fn definition(config: &RepoConfig, eval: &Eval) -> Result<Value, String> {
    let scope = scope::eval_scope(config, eval).map_err(|e| e.to_string())?;
    Ok(json!({"repo":config.root,"eval":eval,"artifacts":scope.artifacts}))
}

fn reconnect(request: &Request) -> Result<RepoConfig, String> {
    let repo = &request
        .provenance
        .as_ref()
        .ok_or("Human request has no provenance.")?
        .repo_path;
    let config = read_workspace_config(repo).map_err(|e| e.to_string())?;
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.id == request.eval_id)
        .ok_or("Recorded Human eval no longer exists.")?;
    if Some(definition(&config, eval)?) != request.human_definition {
        return Err(
            "Human Artifact scope or eval declarations changed since the request was recorded."
                .into(),
        );
    }
    Ok(config)
}

async fn recheck(
    receipts: &Receipts,
    request: &mut Request,
    reviewer: &str,
    config: &RepoConfig,
    cancellation: CancellationToken,
) -> Result<(), String> {
    if cancellation.is_cancelled() {
        return Err("Human action was cancelled.".into());
    }
    let Some(expected) = &request.key else {
        return Ok(());
    };
    let output = request
        .run_dir
        .as_deref()
        .ok_or("Human request has no output directory.")?;
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.id == request.eval_id)
        .ok_or("Recorded Human eval no longer exists.")?;
    let parallelism = cache::Parallelism::new(cache::Parallelism::available());
    let (code, error) =
        match cache::recheck(config, eval, output, &parallelism, cancellation.clone()).await {
            Ok(Some(actual)) if &actual == expected => return Ok(()),
            Ok(_) => ("INPUT_CHANGED", "Fingerprint changed during review.".into()),
            Err(error) if cancellation.is_cancelled() => return Err(error),
            Err(error) => ("FINGERPRINT_RECHECK_FAILED", error),
        };
    request.status = crate::types::RequestStatus::Error;
    request.error = Some(error.clone());
    request.error_code = Some(code.into());
    request.result = None;
    receipts.settle_human(request, reviewer).await?;
    Err(error)
}
