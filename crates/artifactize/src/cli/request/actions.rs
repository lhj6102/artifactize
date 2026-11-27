//! Perform Human review actions and finish their SQLite writes before returning.

use std::{
    io::{self, Write},
    path::Path,
};

use artifactize_core::{
    config::ToolName,
    human,
    runtime::Verdict,
    tools::human::Content,
    types::{RequestId, ReviewerId},
};

use super::super::{FAILURE_EXIT, cancellation_listener, print_json};
use super::{inspect, issues::IssuesSource};

pub(super) async fn claim(
    state: &Path,
    id: RequestId,
    reviewer: Option<ReviewerId>,
) -> Result<(), String> {
    let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
    let (receipts, _) = human::open(state, &id).await?;
    let result = human::claim(&receipts, &id, &reviewer).await;
    let closed = receipts.close().await;
    print_json(&result.and_then(|claim| closed.map(|()| claim))?)
}

pub(super) async fn unclaim(
    state: &Path,
    id: RequestId,
    reviewer: Option<ReviewerId>,
) -> Result<(), String> {
    let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
    let (receipts, _) = human::open(state, &id).await?;
    let result = human::unclaim(&receipts, &id, &reviewer).await;
    let closed = receipts.close().await;
    print_json(&result.and_then(|claim| closed.map(|()| claim))?)
}

pub(super) async fn tool(
    state: &Path,
    id: RequestId,
    tool: ToolName,
    reviewer: Option<ReviewerId>,
    json: bool,
) -> Result<u8, String> {
    let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
    let (receipts, _) = human::open(state, &id).await?;
    let result = async {
        let (cancellation, listener) = cancellation_listener()?;
        let result = human::run_human_tool(
            &receipts,
            &id,
            &reviewer,
            &tool,
            cancellation,
            artifactize_core::review::log::Run::unwatched(),
        )
        .await;
        listener.abort();
        result
    }
    .await;
    let closed = receipts.close().await;
    let result = result.and_then(|result| closed.map(|()| result))?;
    if json {
        print_json(&result)?;
    } else {
        let mut out = io::stdout().lock();
        for content in result.content() {
            match content {
                Content::Text { text } => writeln!(out, "{text}"),
                Content::Viewer { artifact, tool } => {
                    writeln!(out, "Open {artifact} with {tool} in the Artifactize desktop.")
                }
                Content::Launch { launched } => writeln!(
                    out,
                    "Tool {tool}: {}",
                    if *launched {
                        "launched"
                    } else {
                        "not launched"
                    }
                ),
            }
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(if result.is_error() { FAILURE_EXIT } else { 0 })
}

pub(super) async fn submit(
    state: &Path,
    id: RequestId,
    verdict: Verdict,
    issues: IssuesSource,
    reviewer: Option<ReviewerId>,
) -> Result<(), String> {
    let submission = issues.submission(verdict)?;
    let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
    let (cancellation, listener) = cancellation_listener()?;
    let result = human::submit_and_publish(state, &id, &reviewer, &submission, cancellation).await;
    listener.abort();
    result?;
    inspect::show(state, &id).await
}
