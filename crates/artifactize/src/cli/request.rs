//! Parse and route request commands.

mod actions;
mod inspect;
mod issues;

use std::path::Path;

use artifactize_core::{
    runtime::Verdict,
    types::{RequestId, ReviewerId, RunId},
};
use clap::Subcommand;
use issues::IssuesSource;

#[derive(Debug, Subcommand)]
pub enum RequestCommand {
    /// List saved waiting, claimed, and settled requests.
    List {
        /// Only requests from this Run.
        #[arg(long, value_name = "RUN_ID")]
        run: Option<RunId>,
    },
    /// Read full saved audit, claim and summary as JSON.
    Show { id: RequestId },
    /// Read bounded runtime evidence without loading the full request audit.
    Logs {
        id: RequestId,
        #[command(flatten)]
        query: artifactize_core::inspect::LogOptions,
    },
    /// Acquire the Human reviewer lock.
    Claim {
        id: RequestId,
        /// Reviewer name (defaults to the signed-in user).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<ReviewerId>,
    },
    /// Release your Human reviewer lock while the request still waits.
    Unclaim {
        id: RequestId,
        /// Reviewer name (defaults to the signed-in user).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<ReviewerId>,
    },
    /// Run a predefined Human tool as the claimant.
    Tool {
        id: RequestId,
        tool: artifactize_core::config::ToolName,
        /// Reviewer name (defaults to the signed-in user).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<ReviewerId>,
    },
    /// Submit a person's verdict: GREEN with no issue, or RED with the issues to change.
    Submit {
        id: RequestId,
        #[arg(long, value_enum)]
        verdict: Verdict,
        #[command(flatten)]
        issues: IssuesSource,
        /// Reviewer name (defaults to the signed-in user).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<ReviewerId>,
    },
}

pub(super) async fn execute(
    state: &Path,
    command: RequestCommand,
    json: bool,
) -> Result<u8, String> {
    match command {
        RequestCommand::List { run } => inspect::list(state, run, json).await?,
        RequestCommand::Show { id } => inspect::show(state, &id).await?,
        RequestCommand::Logs { id, query } => inspect::logs(state, id, query).await?,
        RequestCommand::Claim { id, reviewer } => actions::claim(state, id, reviewer).await?,
        RequestCommand::Unclaim { id, reviewer } => actions::unclaim(state, id, reviewer).await?,
        RequestCommand::Tool { id, tool, reviewer } => {
            return actions::tool(state, id, tool, reviewer, json).await;
        }
        RequestCommand::Submit {
            id,
            verdict,
            issues,
            reviewer,
        } => {
            actions::submit(state, id, verdict, issues, reviewer).await?;
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use artifactize_core::{platform::path_text, store};
    use serde_json::json;

    #[tokio::test]
    async fn refused_human_actions_release_the_database_before_returning_the_error() {
        let root = crate::test_os::tempdir();
        let repo = root.path().join("repo");
        let state = root.path().join("state");
        std::fs::create_dir(&repo).unwrap();
        let receipts = store::OwnedReceipts::open(&state, &repo).await.unwrap();
        let run: store::Run = serde_json::from_value(json!({
            "id":"run-1", "repoPath":path_text(&repo), "stateDir":path_text(&state), "status":"GREEN",
            "createdAt":"2026-01-01T00:00:00Z", "completedAt":"2026-01-01T00:00:01Z",
            "selection":{"kind":"all"}, "validation":null, "definitions":null
        }))
        .unwrap();
        let request =
            crate::test_request::request("app/check", "GREEN", json!({"cwd":path_text(&repo)}))
                .request
                .as_ref()
                .clone();
        let id = request.id.clone();
        receipts.create_run(&run, &[request]).await.unwrap();
        store::Receipts::clone(&receipts).close().await.unwrap();
        let reviewer = Some("alice".parse().unwrap());
        for command in [
            RequestCommand::Claim {
                id: id.clone(),
                reviewer: reviewer.clone(),
            },
            RequestCommand::Unclaim {
                id: id.clone(),
                reviewer: reviewer.clone(),
            },
            RequestCommand::Tool {
                id,
                reviewer,
                tool: "inspect".parse().unwrap(),
            },
        ] {
            let error = execute(&state, command, true).await.unwrap_err();
            assert!(error.contains("not waiting for a Human review"), "{error}");
            assert!(crate::test_os::database_released(&state.join(store::DATABASE)).unwrap());
        }
        root.close().unwrap();
    }
}
