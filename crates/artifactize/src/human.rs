//! Human waiting, claim locks, tools, and submission.

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::verdict::validate_result,
    cache,
    config::{Eval, RepoConfig, read_workspace_config},
    scope,
    store::{HumanClaim, Receipts, Request},
    tools::human::{Registry, ToolResult},
};

pub fn default_reviewer() -> Result<String, String> {
    let reviewer = std::env::var("USER").map_err(|_| "Set USER or provide a reviewer id.")?;
    validate_reviewer(&reviewer)?;
    Ok(reviewer)
}

fn validate_reviewer(reviewer: &str) -> Result<(), String> {
    if reviewer.trim().is_empty() || reviewer.len() > 200 || reviewer.chars().any(char::is_control)
    {
        return Err("A reviewer id of 1–200 bytes without control characters is required.".into());
    }
    Ok(())
}

pub async fn claim(
    receipts: &Receipts,
    request: &str,
    reviewer: &str,
) -> Result<HumanClaim, String> {
    validate_reviewer(reviewer)?;
    receipts.claim_human(request, reviewer).await
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
    let result = registry.call(tool, cancellation).await;
    receipts
        .record_human_tool(
            &request.id,
            reviewer,
            json!({"name":tool,"isError":result.is_error}),
        )
        .await?;
    Ok(result)
}

/// Invalid results remain correctable; only a valid submission performs the final identity check.
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
    if serde_json::to_vec(result).map_err(|e| e.to_string())?.len() > 256_000 {
        return Err("Human result exceeds 256000 bytes.".into());
    }
    let result = validate_result(&eval.declaration, result)?;
    recheck(receipts, &mut request, reviewer, &config, cancellation).await?;
    request.status = result["verdict"]
        .as_str()
        .expect("validated verdict")
        .into();
    request.result = Some(result);
    receipts.settle_human(&request, reviewer).await
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
    let Some(expected) = &request.identity else {
        return Ok(());
    };
    let output = request
        .run_dir
        .as_deref()
        .ok_or("Human request has no output directory.")?;
    let (code, error) =
        match cache::identity(config, &request.target, output, cancellation.clone()).await {
            Ok(actual) if &actual == expected => return Ok(()),
            Ok(_) => (
                "INPUT_CHANGED",
                "Artifact input changed during review (identity differs).".into(),
            ),
            Err(error) if cancellation.is_cancelled() => return Err(error),
            Err(error) => ("IDENTITY_RECHECK_FAILED", error),
        };
    request.status = "ERROR".into();
    request.error = Some(error.clone());
    request.error_code = Some(code.into());
    request.result = None;
    receipts.settle_human(request, reviewer).await?;
    Err(error)
}
