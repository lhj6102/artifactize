use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use clap::Subcommand;
use serde_json::{Value, json};

use super::{cancellation_listener, print_json};
use crate::{
    human, store,
    tools::human::Content,
    types::{RequestId, RunId},
};

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
    /// Acquire the Human reviewer lock.
    Claim {
        id: RequestId,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<crate::types::ReviewerId>,
    },
    /// Release your Human reviewer lock while the request still waits.
    Unclaim {
        id: RequestId,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<crate::types::ReviewerId>,
    },
    /// Run a predefined Human tool as the claimant.
    Tool {
        id: RequestId,
        tool: crate::config::ToolName,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<crate::types::ReviewerId>,
    },
    /// Submit a schema-valid Human verdict and owner fields.
    Submit {
        id: RequestId,
        #[arg(long, value_enum)]
        verdict: crate::runtime::Verdict,
        /// Owner fields as a JSON object (default {}).
        #[arg(long, value_name = "JSON", conflicts_with = "fields_file")]
        fields: Option<String>,
        /// Read owner fields from a regular JSON file.
        #[arg(long, value_name = "PATH")]
        fields_file: Option<PathBuf>,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<crate::types::ReviewerId>,
    },
}

pub(super) async fn execute(
    state: &Path,
    command: RequestCommand,
    json: bool,
) -> Result<u8, String> {
    match command {
        RequestCommand::List { run } => {
            if json {
                print_json(
                    &crate::inspect::request_list(
                        state,
                        run.as_ref(),
                        None,
                        time::OffsetDateTime::now_utc(),
                    )
                    .await?,
                )?;
            } else {
                let views = store::read_requests(state, run.as_deref()).await?;
                let mut out = io::stdout().lock();
                writeln!(out, "REQUEST\tRUN\tEVAL\tSTATUS\tREVIEWER").map_err(|e| e.to_string())?;
                for view in views {
                    writeln!(
                        out,
                        "{}\t{}\t{}\t{}\t{}",
                        view.request.id,
                        view.request.run_id,
                        view.request.eval_id,
                        view.request.status(),
                        view.claim
                            .as_ref()
                            .map_or("-", |claim| claim.reviewer.as_str())
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        RequestCommand::Show { id } => {
            print_json(
                &crate::inspect::request_show(state, &id, time::OffsetDateTime::now_utc()).await?,
            )?;
        }
        RequestCommand::Claim { id, reviewer } => {
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (receipts, _) = human::open(state, &id).await?;
            let result = human::claim(&receipts, &id, &reviewer).await;
            let closed = receipts.close().await;
            print_json(&result.and_then(|claim| closed.map(|()| claim))?)?;
        }
        RequestCommand::Unclaim { id, reviewer } => {
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (receipts, _) = human::open(state, &id).await?;
            let result = human::unclaim(&receipts, &id, &reviewer).await;
            let closed = receipts.close().await;
            print_json(&result.and_then(|claim| closed.map(|()| claim))?)?;
        }
        RequestCommand::Tool { id, tool, reviewer } => {
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (receipts, _) = human::open(state, &id).await?;
            let result = async {
                let (cancellation, listener) = cancellation_listener()?;
                let result =
                    human::run_human_tool(&receipts, &id, &reviewer, &tool, cancellation).await;
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
            return Ok(if result.is_error() { 2 } else { 0 });
        }
        RequestCommand::Submit {
            id,
            verdict,
            fields,
            fields_file,
            reviewer,
        } => {
            let result = submission(verdict, fields, fields_file)?;
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (cancellation, listener) = cancellation_listener()?;
            let result =
                human::submit_and_publish(state, &id, &reviewer, &result, cancellation).await;
            listener.abort();
            result?;
            print_json(
                &crate::inspect::request_show(state, &id, time::OffsetDateTime::now_utc()).await?,
            )?;
        }
    }
    Ok(0)
}

fn submission(
    verdict: crate::runtime::Verdict,
    fields: Option<String>,
    file: Option<PathBuf>,
) -> Result<Value, String> {
    let fields = if let Some(path) = file {
        let file = crate::platform::open_nonblocking(&path).map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Human fields must be a regular JSON file.".into());
        }
        let mut bytes = Vec::new();
        file.take(human::FIELDS_READ_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())?
    } else {
        fields.unwrap_or_else(|| "{}".into())
    };
    if fields.len() > human::MAX_RESULT_BYTES {
        return Err("Human fields exceed 256000 bytes.".into());
    }
    let mut result: Value = serde_json::from_str(&fields).map_err(|e| e.to_string())?;
    let object = result
        .as_object_mut()
        .ok_or("Human fields must be a JSON object.")?;
    if object.contains_key("verdict") {
        return Err("Use --verdict, not a verdict inside --fields.".into());
    }
    object.insert("verdict".into(), json!(verdict));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refused_human_actions_release_the_database_before_returning_the_error() {
        let root = crate::test_os::tempdir();
        let repo = root.path().join("repo");
        let state = root.path().join("state");
        std::fs::create_dir(&repo).unwrap();
        let receipts = store::Receipts::open(&state, &repo).await.unwrap();
        let run: store::Run = serde_json::from_value(json!({
            "id":"run-1", "repoPath":repo, "stateDir":state, "status":"GREEN",
            "createdAt":"2026-01-01T00:00:00Z", "completedAt":"2026-01-01T00:00:01Z",
            "selection":{"kind":"all"}, "validation":null, "definitions":null
        }))
        .unwrap();
        let request =
            crate::test_request::request("app/check", "GREEN", json!({"cwd":repo})).request;
        let id = request.id.clone();
        receipts.create_run(&run, &[request]).await.unwrap();
        receipts.close().await.unwrap();
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
