//! Human review: the facts and lifecycle jobs every interface shares. The terminal screens
//! live in `artifactize-tui`.

mod detail;
mod form;
mod runs;
pub mod search;

/// The Tool log every review interface shows; see [`log::Sink`].
pub use crate::tools::human::log;
pub use detail::{Stage, meta, outcome_text, shared, stage, technical};
pub use form::{Field, Form, Input, Key, Row, check as check_schema};
pub use runs::Runs;

use crate::{
    config::{HumanToolKind, ToolName},
    human,
    store::{self, HumanClaim, Request},
    tools::human::{CommandLine, Content, ToolResult},
    types::RequestId,
};
use serde_json::Value;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

/// The refusal for reviewing a request nobody claimed.
pub const CLAIM_FIRST: &str = "Claim this request before reviewing it.";
/// The refusal for releasing a claim that is not this reviewer's.
pub const NOT_CLAIMED: &str = "The request is not claimed.";

/// What `reviewer` may do with the request now: its id and whether nobody holds the claim
/// yet, or why it is read-only (loading, settled, claimed by someone else). Every interface
/// asks this, so they refuse alike.
pub fn actionable(
    view: Option<&crate::store::RequestView>,
    reviewer: &crate::types::ReviewerId,
) -> Result<(RequestId, bool), String> {
    let view = view.ok_or("The request is still loading.")?;
    if !view.request.status().is_human_wait() {
        return Err(format!(
            "The request is {}; there is nothing to review.",
            view.request.status()
        ));
    }
    match &view.claim {
        Some(claim) if &claim.reviewer != reviewer => Err(format!(
            "Claimed by {}; read-only for {}.",
            claim.reviewer, reviewer
        )),
        claim => Ok((view.request.id.clone(), claim.is_none())),
    }
}

/// The refusal for choosing a verdict, running a tool or submitting, as the reviewer sees it:
/// only the claimant reviews, and a claim is always the explicit one.
pub fn review_refusal(
    view: Option<&crate::store::RequestView>,
    reviewer: &crate::types::ReviewerId,
) -> Option<String> {
    match actionable(view, reviewer) {
        Ok((_, false)) => None,
        Ok((_, true)) | Err(_) => Some(CLAIM_FIRST.into()),
    }
}

/// The refusal for releasing: only a claim of this reviewer is released.
pub fn release_refusal(
    view: Option<&crate::store::RequestView>,
    reviewer: &crate::types::ReviewerId,
) -> Option<String> {
    match actionable(view, reviewer) {
        Ok((_, false)) => None,
        Ok((_, true)) => Some(NOT_CLAIMED.into()),
        Err(error) => Some(error),
    }
}

/// A Human submission has one verdict, parsed with its owner fields at the UI edge.
/// The private envelope prevents a separate display choice from contradicting the result;
/// the request's owner schema is still checked by the shared Human submission operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Submission(crate::agent::verdict::ValidatedResult);
impl TryFrom<Value> for Submission {
    type Error = String;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        serde_json::from_value(value)
            .map(Self)
            .map_err(|error| error.to_string())
    }
}
impl Submission {
    pub fn verdict(&self) -> crate::runtime::Verdict {
        self.0.verdict
    }
    pub fn to_json(&self) -> Value {
        self.0.to_json()
    }
}

/// Lifecycle work with owned inputs, so the screen keeps drawing while it runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    /// Explicit claim for an embedded single-request review.
    Claim { id: RequestId },
    /// Claim first when `claim`, then run the tool.
    Run {
        id: RequestId,
        tool: ToolName,
        claim: bool,
    },
    /// Claim first when `claim`, then submit and publish like `request submit`.
    Submit {
        id: RequestId,
        result: Submission,
        claim: bool,
    },
    /// Release the reviewer's remaining claims; quit afterwards when `quit`.
    Release { ids: Vec<RequestId>, quit: bool },
}

#[derive(Debug)]
pub enum Outcome {
    Claimed {
        result: Result<HumanClaim, String>,
    },
    /// Publishing failed after settlement; never offer to submit the verdict twice.
    SavedLocally {
        request: Box<Request>,
        error: String,
    },
    Ran {
        tool: ToolName,
        claimed: Option<HumanClaim>,
        result: Result<ToolResult, String>,
    },
    Submitted {
        id: RequestId,
        claimed: Option<HumanClaim>,
        result: Result<Box<Request>, String>,
    },
    Released {
        ids: Vec<RequestId>,
        quit: bool,
        /// Request IDs whose release failed, with the error.
        failed: Vec<(RequestId, String)>,
    },
}

impl Outcome {
    /// What the review tells the reviewer about the outcome, and whether it is an error.
    pub fn notice(&self) -> (String, bool) {
        match self {
            Self::SavedLocally { error, .. } => (error.clone(), true),
            Self::Claimed { result: Ok(_) } => (
                "Claimed. Fill GREEN or RED fields, then Submit.".into(),
                false,
            ),
            Self::Claimed { result: Err(error) } => (error.clone(), true),
            Self::Ran { tool, result, .. } => {
                let launched = Content::Launch { launched: true };
                match result {
                    Ok(result) if result.is_error() => (
                        format!("{tool} failed; a tool error is not a verdict."),
                        true,
                    ),
                    Ok(result) if result.content().contains(&launched) => {
                        (format!("{tool} launched."), false)
                    }
                    Ok(_) => (format!("{tool} finished."), false),
                    Err(_) => (format!("{tool} did not run."), true),
                }
            }
            Self::Submitted {
                result: Ok(request),
                ..
            } => (
                format!(
                    "Submitted {} for {} ({}).",
                    request.status(),
                    request.eval_id,
                    request.id
                ),
                false,
            ),
            Self::Submitted {
                result: Err(error), ..
            } => (error.clone(), true),
            Self::Released { failed, .. } if failed.is_empty() => ("Claim released.".into(), false),
            Self::Released { failed, .. } => {
                let lines = failed.iter().map(|(id, error)| format!("{id}: {error}"));
                (lines.collect::<Vec<_>>().join("\n"), true)
            }
        }
    }
}

/// A declared Human tool as the saved definition records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub name: ToolName,
    /// The Artifact that declares the tool.
    pub artifact: crate::types::ArtifactName,
    pub kind: HumanToolKind,
    pub description: String,
    pub declared: Runs,
}

impl Tool {
    /// Its name in the Artifact that declares it: `open` of `notes` is `open_notes`.
    pub fn operation(&self) -> &str {
        Self::operation_of(&self.name, &self.artifact)
    }

    /// The name tool `name` has in `artifact`, which declares it.
    pub fn operation_of<'a>(name: &'a ToolName, artifact: &crate::types::ArtifactName) -> &'a str {
        name.as_str()
            .strip_suffix(&format!("_{artifact}"))
            .unwrap_or(name.as_str())
    }

    /// What running the tool does, as the Tools pane shows it: a command's resolved line once
    /// `resolved` is known, else its declaration; a builtin's action on its logical target.
    pub fn action(&self, resolved: Option<&Result<CommandLine, String>>) -> String {
        match (&self.declared, resolved) {
            (Runs::Command(_), Some(Ok(command))) => {
                let program = crate::platform::path_text(std::path::Path::new(&command.program));
                let words = std::iter::once(program).chain(command.args.iter().cloned());
                let words: Vec<_> = words.collect();
                format!("$ {}", shell(words.iter().map(String::as_str)))
            }
            (declared, _) => declared.line(),
        }
    }
}

impl Job {
    /// Carry out the job through the `human` lifecycle API, as `request` does; every UI runs
    /// its Human actions through here. A tool run logs to `log` from its start line to its end,
    /// failures included; a launched program's output keeps arriving after the job returns.
    pub async fn run(
        self,
        state: PathBuf,
        reviewer: crate::types::ReviewerId,
        cancel: CancellationToken,
        log: log::Sink,
    ) -> Outcome {
        let mut claimed = None;
        match self {
            Job::Claim { id } => {
                let result = async {
                    let (receipts, _) = human::open(&state, &id).await?;
                    let result = human::claim(&receipts, &id, &reviewer).await;
                    let closed = receipts.close().await;
                    result.and_then(|claim| closed.map(|()| claim))
                }
                .await;
                Outcome::Claimed { result }
            }
            Job::Run {
                id,
                tool,
                claim: first,
            } => {
                // The entry names the tool as the saved request declares it.
                let declared = store::read_request(&state, &id)
                    .await
                    .ok()
                    .and_then(|view| {
                        tools(&view.request)
                            .into_iter()
                            .find(|declared| declared.name == tool)
                    });
                let run = match &declared {
                    Some(declared) => {
                        log.start(&id, declared.operation(), declared.artifact.as_str())
                    }
                    None => log.start(&id, tool.as_str(), ""),
                };
                let result = async {
                    let (receipts, _) = match human::open(&state, &id).await {
                        Ok(opened) => opened,
                        Err(error) => {
                            run.end(log::End::Failed(error.clone()));
                            return Err(error);
                        }
                    };
                    let result = async {
                        if first {
                            match human::claim(&receipts, &id, &reviewer).await {
                                Ok(claim) => claimed = Some(claim),
                                Err(error) => {
                                    run.end(log::End::Failed(error.clone()));
                                    return Err(error);
                                }
                            }
                        }
                        human::run_human_tool(&receipts, &id, &reviewer, &tool, cancel, run).await
                    }
                    .await;
                    let closed = receipts.close().await;
                    result.and_then(|result| closed.map(|()| result))
                }
                .await;
                Outcome::Ran {
                    tool,
                    claimed,
                    result,
                }
            }
            Job::Submit {
                id,
                result,
                claim: first,
            } => {
                let result = async {
                    if first {
                        let (receipts, _) = human::open(&state, &id).await?;
                        let result = human::claim(&receipts, &id, &reviewer).await;
                        let closed = receipts.close().await;
                        claimed = Some(result.and_then(|claim| closed.map(|()| claim))?);
                    }
                    human::submit_and_publish(&state, &id, &reviewer, &result.to_json(), cancel)
                        .await
                        .map(Box::new)
                }
                .await;
                if let Err(error) = &result
                    && let Ok(view) = store::read_request(&state, &id).await
                    && matches!(
                        view.request.status(),
                        crate::types::RequestStatus::Green | crate::types::RequestStatus::Red
                    )
                    && view.execution.as_ref().is_some_and(|execution| {
                        execution.reviewer.as_deref() == Some(reviewer.as_str())
                    })
                {
                    return Outcome::SavedLocally {
                        request: Box::new(view.request),
                        error: error.clone(),
                    };
                }
                Outcome::Submitted {
                    id,
                    claimed,
                    result,
                }
            }
            Job::Release { ids, quit } => {
                let mut failed = Vec::new();
                for id in &ids {
                    let release = async {
                        // Settled requests and claims released elsewhere leave nothing to release.
                        let view = store::read_request(&state, id).await?;
                        if view.claim.is_none_or(|claim| claim.reviewer != reviewer) {
                            return Ok(());
                        }
                        let (receipts, _) = human::open(&state, id).await?;
                        let result = human::unclaim(&receipts, id, &reviewer).await.map(drop);
                        let closed = receipts.close().await;
                        result.and(closed)
                    };
                    if let Err(error) = release.await {
                        failed.push((id.clone(), error));
                    }
                }
                Outcome::Released { ids, quit, failed }
            }
        }
    }
}

/// Declared Human tools in the eval scope, named like the registry.
pub fn tools(request: &Request) -> Vec<Tool> {
    request
        .human_definition
        .as_ref()
        .and_then(|definition| definition.artifacts.value())
        .map(declared)
        .unwrap_or_default()
}

/// The command line a tool would run, or why it cannot be resolved.
pub type Resolved = Result<CommandLine, String>;

/// The Human tools Artifact `artifact` itself declares in the workspace at `repo` as it is now,
/// named like the registry, each with the command line it would run (a builtin has none).
/// Listing reads the declarations only: no request, claim or Run is involved.
pub fn artifact_tools(
    repo: &std::path::Path,
    artifact: &crate::types::ArtifactName,
) -> Result<Vec<(Tool, Option<Resolved>)>, String> {
    let config = crate::config::read_workspace_config(repo).map_err(|error| error.to_string())?;
    let registry = crate::tools::human::Registry::for_artifact(&config, artifact.as_str())?;
    let scope =
        crate::scope::artifact_scope(&config, &[artifact]).map_err(|error| error.to_string())?;
    // The scope's declarations in the saved definitions' shape, as a request records them.
    let artifacts: std::collections::BTreeMap<_, crate::store::definitions::Artifact> =
        serde_json::to_value(&scope.artifacts)
            .and_then(serde_json::from_value)
            .map_err(|error| error.to_string())?;
    Ok(declared(&artifacts)
        .into_iter()
        .filter(|tool| &tool.artifact == artifact)
        .map(|tool| {
            let command = registry
                .is_command(&tool.name)
                .then(|| registry.command(&tool.name));
            (tool, command)
        })
        .collect())
}

/// Run tool `tool` of Artifact `artifact` at once in the workspace at `repo` as it is now, for
/// no review: nobody claims anything and nothing is recorded. Its output reaches `run`'s Tool
/// log; a launched program's keeps arriving until it exits. A refusal ends the run with it.
pub async fn run_artifact_tool(
    repo: &std::path::Path,
    artifact: &crate::types::ArtifactName,
    tool: &ToolName,
    cancel: CancellationToken,
    run: log::Run,
) -> Result<ToolResult, String> {
    let config = match crate::config::read_workspace_config(repo) {
        Ok(config) => config,
        Err(error) => {
            run.end(log::End::Failed(error.to_string()));
            return Err(error.to_string());
        }
    };
    let registry = match crate::tools::human::Registry::for_artifact(&config, artifact.as_str()) {
        Ok(registry) => registry,
        Err(error) => {
            run.end(log::End::Failed(error.clone()));
            return Err(error);
        }
    };
    // Only the Artifact's own tools: one of a child it includes belongs to that child's Detail.
    if !registry
        .list()
        .any(|entry| &entry.name == tool && &entry.artifact_id == artifact)
    {
        let error = format!("{artifact} declares no Human tool {tool}.");
        run.end(log::End::Failed(error.clone()));
        return Err(error);
    }
    Ok(registry.call_logged(tool, cancel, run).await)
}

/// The Human tools `artifacts` declare, named like the registry, sorted by name.
fn declared(
    artifacts: &std::collections::BTreeMap<
        crate::types::ArtifactName,
        crate::store::definitions::Artifact,
    >,
) -> Vec<Tool> {
    use crate::store::definitions::HumanTool;
    let mut tools: Vec<_> = artifacts
        .iter()
        .flat_map(|(id, artifact)| {
            artifact
                .views
                .value()
                .and_then(|views| views.human_tools.value())
                .into_iter()
                .flatten()
                // A saved name that is not a valid tool name has no tool to run; the
                // registry, built from the same declarations, refuses it as well.
                .filter_map(move |(operation, tool)| {
                    let (kind, description) = match tool {
                        HumanTool::Command(tool) => (tool.kind, &tool.description),
                        HumanTool::Builtin(tool) => (tool.kind, &tool.description),
                    };
                    Some(Tool {
                        name: format!("{operation}_{id}").parse().ok()?,
                        artifact: id.clone(),
                        kind,
                        description: description.replace("{artifactName}", id.as_str()),
                        declared: Runs::parse(artifacts, id, tool),
                    })
                })
        })
        .collect();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

/// The owner schema of `verdict` recorded with the request: `None` when it declares none.
/// Without a recorded Human definition the schemas are unknown, not empty: an empty form
/// would submit a verdict the schema then rejects.
pub fn schema(
    request: &Request,
    verdict: crate::runtime::Verdict,
) -> Result<Option<Value>, String> {
    let definition = request.human_definition.as_ref().ok_or(
        "This request records no Human definition; open the original request its actions go to.",
    )?;
    let declaration = definition.eval.declaration.value();
    let schema = declaration.and_then(|declaration| match verdict {
        crate::runtime::Verdict::Green => declaration.pass_schema.value(),
        crate::runtime::Verdict::Red => declaration.fail_schema.value(),
    });
    Ok(schema.map(|schema| Value::Object(schema.clone())))
}

/// Words joined for an on-screen preview, double-quoted when a word has whitespace or a
/// quote of its own. This is a reading aid, not a shell command: it claims no single
/// shell's escaping rules, which differ between Unix shells and Windows's.
pub fn shell<'a>(words: impl IntoIterator<Item = &'a str>) -> String {
    let quote = |word: &str| {
        let plain = !word.is_empty()
            && word
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%{}".contains(c));
        if plain {
            word.to_owned()
        } else {
            format!("\"{}\"", word.replace('"', "\"\""))
        }
    };
    words.into_iter().map(quote).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod submission_tests {
    use super::Submission;
    use crate::runtime::Verdict;
    use serde_json::json;

    #[test]
    fn submission_parses_one_verdict_and_keeps_its_owner_fields() {
        for (value, verdict) in [
            (json!({"verdict":"GREEN","approved":true}), Verdict::Green),
            (json!({"verdict":"RED","reason":"needs work"}), Verdict::Red),
        ] {
            let submitted = Submission::try_from(value.clone()).unwrap();
            assert_eq!(submitted.verdict(), verdict);
            assert_eq!(submitted.to_json(), value);
        }
        assert!(Submission::try_from(json!({"approved":true})).is_err());
        assert!(Submission::try_from(json!({"verdict":"ERROR"})).is_err());
    }
}
