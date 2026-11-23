//! Human waiting, claim locks, tools, and submission.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::verdict::{ValidatedResult, check_result},
    cache,
    config::{Eval, RepoConfig, read_workspace_config},
    remote::Session,
    scope,
    store::{self, HumanClaim, Receipts, Request},
    tools::human::{CommandLine, Registry, ToolResult},
};

/// Bound owner fields and serialized Human results consistently across CLI, editor
/// and submission so untrusted JSON cannot grow the review audit without limit.
pub const MAX_RESULT_BYTES: usize = 256_000;
/// Read one sentinel byte beyond the bound to detect oversized files without loading them.
pub const FIELDS_READ_BYTES: u64 = MAX_RESULT_BYTES as u64 + 1;

pub fn default_reviewer() -> Result<crate::types::ReviewerId, String> {
    crate::platform::user_name()
        .ok_or("Provide an explicit reviewer id; no default reviewer is available.")?
        .parse()
}

/// The saved request's Run repository and its receipts, where its Human actions are recorded.
pub async fn open(
    state: &Path,
    request: &crate::types::RequestId,
) -> Result<(Receipts, PathBuf), String> {
    let repo = store::read_request_repo_by_id(state, request.clone()).await?;
    Ok((Receipts::open(state, &repo).await?, repo))
}

pub async fn claim(
    receipts: &Receipts,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
) -> Result<HumanClaim, String> {
    claim_acquisition(receipts, request, reviewer)
        .await
        .map(|(claim, _)| claim)
}

/// The saved claim and whether this action created it, rather than finding an existing claim.
pub async fn claim_acquisition(
    receipts: &Receipts,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
) -> Result<(HumanClaim, bool), String> {
    receipts.claim_human(request, reviewer).await
}

/// Release this particular claim, returning false if it is gone or has been replaced.
pub async fn release_exact_claim(state: &Path, claim: &HumanClaim) -> Result<bool, String> {
    let (receipts, _) = open(state, &claim.request_id).await?;
    let result = receipts.release_exact_claim(claim.clone()).await;
    let closed = receipts.close().await;
    result.and_then(|released| closed.map(|()| released))
}

/// Release the reviewer lock without a verdict, so another reviewer can claim the request.
pub async fn unclaim(
    receipts: &Receipts,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
) -> Result<HumanClaim, String> {
    receipts.release_human(request, reviewer).await
}

/// Run `tool` for the reviewer who holds the claim, its output reaching `run`'s Tool log. A
/// refusal before it starts ends the run with the refusal.
pub async fn run_human_tool(
    receipts: &Receipts,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
    tool: &crate::config::ToolName,
    cancellation: CancellationToken,
    run: crate::tools::human::log::Run,
) -> Result<ToolResult, String> {
    let ready = async {
        let (mut request, _) = receipts.human_request(request, reviewer).await?;
        let config = reconnect(receipts, &request).await?;
        let registry = Registry::new(&config, &request.eval_id)?;
        if !registry.list().any(|entry| &entry.name == tool) {
            return Err::<_, String>("Unknown registered Human tool.".into());
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
        Ok((config, request.eval_id))
    };
    let (config, eval) = match ready.await {
        Ok(ready) => ready,
        Err(error) => {
            run.end(crate::tools::human::log::End::Failed(error.clone()));
            return Err(error);
        }
    };
    let registry = Registry::new(&config, &eval).expect("the registry was just built");
    Ok(registry.call_logged(tool, cancellation, run).await)
}

/// What every registered Human tool of a waiting request would run, by tool name; needs no
/// claim and runs nothing. A tool that cannot be resolved carries its error. A request whose
/// declarations changed ends here as it does before a tool run or a submission.
pub async fn tool_commands(
    receipts: &Receipts,
    request: &crate::types::RequestId,
) -> Result<std::collections::BTreeMap<crate::config::ToolName, Result<CommandLine, String>>, String>
{
    let request = receipts.waiting_human(request).await?;
    let config = reconnect(receipts, &request).await?;
    commands_for(&config, &request.eval_id)
}

/// Resolve display-only command lines without retiring a request or changing its claim.
pub async fn preview_tool_commands(
    request: std::sync::Arc<Request>,
) -> Result<std::collections::BTreeMap<crate::config::ToolName, Result<CommandLine, String>>, String>
{
    crate::task::blocking(move || {
        let config = declarations(&request).map_err(|mismatch| match mismatch {
            Mismatch::Unreadable(error) => error,
            Mismatch::Changed(reason) => reason.into(),
        })?;
        commands_for(&config, &request.eval_id)
    })
    .await?
}

fn commands_for(
    config: &RepoConfig,
    eval: &crate::types::EvalId,
) -> Result<std::collections::BTreeMap<crate::config::ToolName, Result<CommandLine, String>>, String>
{
    let registry = Registry::new(config, eval)?;
    Ok(registry
        .list()
        .filter(|tool| registry.is_command(&tool.name))
        .map(|tool| (tool.name.clone(), registry.command(&tool.name)))
        .collect())
}

/// A Human answer parsed at the caller's input boundary. The request's owner schema is
/// checked against these fields only once the saved request has been loaded.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Submission {
    verdict: crate::runtime::Verdict,
    #[serde(flatten)]
    fields: Map<String, Value>,
}

impl TryFrom<Value> for Submission {
    type Error = String;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value).map_err(|error| error.to_string())
    }
}

impl Submission {
    pub fn new(
        verdict: crate::runtime::Verdict,
        fields: Map<String, Value>,
    ) -> Result<Self, String> {
        if fields.contains_key("verdict") {
            return Err("Owner fields must not contain a verdict.".into());
        }
        Ok(Self { verdict, fields })
    }

    pub fn verdict(&self) -> crate::runtime::Verdict {
        self.verdict
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).expect("Human submission is JSON")
    }

    fn validate(&self, eval: &crate::config::EvalDeclaration) -> Result<ValidatedResult, String> {
        let value = self.to_json();
        if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > MAX_RESULT_BYTES {
            return Err(format!("Human result exceeds {MAX_RESULT_BYTES} bytes."));
        }
        check_result(eval, &value)?;
        Ok(ValidatedResult {
            verdict: self.verdict,
            fields: self.fields.clone(),
        })
    }
}

/// Invalid results remain correctable; only a valid submission performs the final fingerprint
/// check.
pub async fn submit(
    receipts: &Receipts,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
    submission: &Submission,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    submit_answer(
        receipts,
        request,
        reviewer,
        SubmittedAnswer::Explicit(submission),
        cancellation,
    )
    .await
}

enum SubmittedAnswer<'a> {
    Explicit(&'a Submission),
    PreparedExplicit(&'a Submission),
    SavedDraft,
    PreparedDraft(crate::review::Draft),
}

async fn submit_answer(
    receipts: &Receipts,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
    answer: SubmittedAnswer<'_>,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    let (mut request, _) = receipts.human_request(request, reviewer).await?;
    let config = reconnect(receipts, &request).await?;
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.id == request.eval_id)
        .expect("reconnected eval");
    let (answer, expected) = match answer {
        SubmittedAnswer::Explicit(submission) => (submission.validate(&eval.declaration)?, None),
        SubmittedAnswer::PreparedExplicit(submission) => (
            submission.validate(&eval.declaration)?,
            Some(crate::review::Draft::default()),
        ),
        other => {
            let draft = match other {
                SubmittedAnswer::SavedDraft => receipts
                    .read_review_draft(&request.id, reviewer)
                    .await?
                    .unwrap_or_default(),
                SubmittedAnswer::PreparedDraft(draft) => draft,
                SubmittedAnswer::Explicit(_) | SubmittedAnswer::PreparedExplicit(_) => {
                    unreachable!()
                }
            };
            (
                draft.submission(&request)?.validate(&eval.declaration)?,
                Some(draft),
            )
        }
    };
    recheck(receipts, &mut request, reviewer, &config, cancellation).await?;
    request.state = crate::store::RequestState::completed(answer.into(), crate::broker::now());
    receipts
        .settle_human_if_draft(&request, reviewer, expected)
        .await
}

/// `submit`, then publish the settled result to the configured remote review store.
pub async fn submit_and_publish(
    state: &Path,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
    submission: &Submission,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    publish_answer(
        state,
        request,
        reviewer,
        SubmittedAnswer::Explicit(submission),
        cancellation,
    )
    .await
}

pub(crate) async fn publish_explicit_without_draft(
    state: &Path,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
    submission: &Submission,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    publish_answer(
        state,
        request,
        reviewer,
        SubmittedAnswer::PreparedExplicit(submission),
        cancellation,
    )
    .await
}

/// Submit the saved answer, refusing to settle if another surface changes its verdict or
/// form while its inputs are checked. Notes are read at settlement in either case.
pub(crate) async fn publish_saved_draft(
    state: &Path,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
    draft: Option<crate::review::Draft>,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    let answer = draft.map_or(SubmittedAnswer::SavedDraft, SubmittedAnswer::PreparedDraft);
    publish_answer(state, request, reviewer, answer, cancellation).await
}

async fn publish_answer(
    state: &Path,
    request: &crate::types::RequestId,
    reviewer: &crate::types::ReviewerId,
    answer: SubmittedAnswer<'_>,
    cancellation: CancellationToken,
) -> Result<Request, String> {
    let (receipts, repo) = open(state, request).await?;
    let result = async {
        let remote = Session::open(Some(state), Some(&repo))?;
        let request = submit_answer(&receipts, request, reviewer, answer, cancellation).await?;
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
    .await;
    let closed = receipts.close().await;
    result.and_then(|request| closed.map(|()| request))
}

pub(crate) fn definition(
    config: &RepoConfig,
    eval: &Eval,
) -> Result<crate::store::HumanDefinition, String> {
    let scope = scope::eval_scope(config, eval).map_err(|e| e.to_string())?;
    Ok(crate::store::HumanDefinition::authored(
        &config.root,
        eval,
        &scope,
    ))
}

/// Why the live declarations cannot serve a waiting request.
enum Mismatch {
    /// They could not be read: the request keeps waiting until they load again.
    Unreadable(String),
    /// They differ from those the request recorded, so it can no longer be answered as recorded.
    Changed(&'static str),
}

/// The live declarations of the request's eval. When they no longer match those it recorded,
/// the request ends ERROR (INPUT_CHANGED) with the requests joined to it, whoever holds it, as
/// a changed fingerprint ends it: the next verify asks again with the new declarations.
async fn reconnect(receipts: &Receipts, request: &Request) -> Result<RepoConfig, String> {
    match declarations(request) {
        Ok(config) => Ok(config),
        Err(Mismatch::Unreadable(error)) => Err(error),
        Err(Mismatch::Changed(error)) => {
            receipts
                .retire_human(&request.id, crate::types::FailureCode::InputChanged, error)
                .await?;
            Err(error.into())
        }
    }
}

/// The live declarations of the request's eval, compared with those it was recorded with. Only
/// the declarations of the eval's scope are read, however large the workspace is; the whole
/// workspace only when the scope alone cannot answer, as when an Artifact of it moved.
fn declarations(request: &Request) -> Result<RepoConfig, Mismatch> {
    let repo = &request
        .provenance
        .as_ref()
        .ok_or_else(|| Mismatch::Unreadable("Human request has no provenance.".into()))?
        .repo_path;
    let sites = request
        .human_definition
        .as_ref()
        .and_then(|definition| definition.artifacts.value())
        .and_then(|artifacts| {
            artifacts
                .iter()
                .map(|(name, artifact)| {
                    Some((
                        name.clone(),
                        crate::config::Site {
                            path: artifact.path.value()?.clone(),
                            definition: artifact.definition.value()?.clone(),
                        },
                    ))
                })
                .collect::<Option<std::collections::BTreeMap<_, _>>>()
        });
    let scoped = sites.and_then(|sites| {
        crate::config::read_eval_scope(repo, &request.eval_id, &sites)
            .ok()
            .flatten()
    });
    let config = match scoped {
        Some(config) => config,
        None => read_workspace_config(repo).map_err(|e| Mismatch::Unreadable(e.to_string()))?,
    };
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.id == request.eval_id)
        .ok_or(Mismatch::Changed("Recorded Human eval no longer exists."))?;
    if Some(definition(&config, eval).map_err(Mismatch::Unreadable)?) != request.human_definition {
        return Err(Mismatch::Changed(
            "Human Artifact scope or eval declarations changed since the request was recorded.",
        ));
    }
    Ok(config)
}

async fn recheck(
    receipts: &Receipts,
    request: &mut Request,
    reviewer: &crate::types::ReviewerId,
    config: &RepoConfig,
    cancellation: CancellationToken,
) -> Result<(), String> {
    if cancellation.is_cancelled() {
        return Err("Human action was cancelled.".into());
    }
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.id == request.eval_id)
        .ok_or("Recorded Human eval no longer exists.")?;
    if request.key.is_none() && cache::live_input(config, eval).is_none() {
        return Ok(());
    }
    let output = request
        .run_dir
        .as_deref()
        .ok_or("Human request has no output directory.")?;
    let parallelism = cache::Parallelism::new(cache::Parallelism::available())?;
    let (code, error) = match if let Some(expected) = &request.key {
        cache::recheck(config, eval, output, &parallelism, cancellation.clone())
            .await
            .map(|actual| actual.as_ref() == Some(expected))
    } else {
        cache::recheck_observations(
            config,
            &request.fingerprints,
            output,
            &parallelism,
            cancellation.clone(),
        )
        .await
    } {
        Ok(true) => return Ok(()),
        Ok(false) => (
            crate::types::FailureCode::InputChanged,
            "Fingerprint changed during review.".into(),
        ),
        Err(error) if cancellation.is_cancelled() => return Err(error),
        Err(error) => (crate::types::FailureCode::FingerprintRecheckFailed, error),
    };
    request.state =
        crate::store::RequestState::failed(error.clone(), Some(code), crate::broker::now());
    receipts.settle_human(request, reviewer).await?;
    Err(error)
}
