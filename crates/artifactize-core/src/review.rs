//! Human review: the facts and lifecycle jobs every interface shares. The terminal screens
//! live in `artifactize-tui`.

mod detail;
mod form;
mod runs;
pub mod search;

pub use detail::{Stage, meta, outcome_text, shared, stage, technical};
pub use form::{Field, Form, Input, Key, boundary, template};
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

/// The last tool result or failure, shown in the output pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub title: String,
    pub text: String,
    pub error: bool,
}

impl Job {
    /// Carry out the job through the `human` lifecycle API, as `request` does; every UI runs
    /// its Human actions through here.
    pub async fn run(
        self,
        state: PathBuf,
        reviewer: crate::types::ReviewerId,
        cancel: CancellationToken,
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
                let result = async {
                    let (receipts, _) = human::open(&state, &id).await?;
                    let result = async {
                        if first {
                            claimed = Some(human::claim(&receipts, &id, &reviewer).await?);
                        }
                        human::run_human_tool(&receipts, &id, &reviewer, &tool, cancel).await
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
    use crate::store::definitions::HumanTool;
    let Some(artifacts) = request
        .human_definition
        .as_ref()
        .and_then(|definition| definition.artifacts.value())
    else {
        return Vec::new();
    };
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

/// The output pane's view of a tool run: its text, or why it failed.
pub fn output(tool: &str, result: Result<ToolResult, String>) -> Output {
    match result {
        Ok(result) => Output {
            title: if result.is_error() {
                format!("{tool} · tool error")
            } else {
                tool.to_owned()
            },
            text: result
                .content()
                .iter()
                .map(|content| match content {
                    Content::Text { text } => text.trim_end().to_owned(),
                    Content::Launch { launched: true } => "launched".into(),
                    Content::Launch { launched: false } => "not launched".into(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            error: result.is_error(),
        },
        Err(error) => Output {
            title: format!("{tool} · failed"),
            text: error,
            error: true,
        },
    }
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
