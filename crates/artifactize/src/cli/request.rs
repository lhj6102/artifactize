use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use clap::{Subcommand, builder::TypedValueParser, error::ErrorKind};
use serde_json::{Map, Value};

use super::{FAILURE_EXIT, cancellation_listener, print_json};
use artifactize_core::{
    human::{self, Submission},
    runtime::Verdict,
    store,
    tools::human::Content,
    types::{RequestId, ReviewerId, RunId},
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
    /// Submit a schema-valid Human verdict and owner fields.
    Submit {
        id: RequestId,
        #[arg(long, value_enum)]
        verdict: Verdict,
        /// Owner fields as a JSON object (default {}).
        #[arg(long, value_name = "JSON", conflicts_with = "fields_file", value_parser = OwnerFieldsParser)]
        fields: Option<OwnerFields>,
        /// Read owner fields from a regular JSON file.
        #[arg(long, value_name = "PATH")]
        fields_file: Option<PathBuf>,
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
        RequestCommand::List { run } => {
            if json {
                print_json(
                    &artifactize_core::inspect::request_list(
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
                &artifactize_core::inspect::request_show(
                    state,
                    &id,
                    time::OffsetDateTime::now_utc(),
                )
                .await?,
            )?;
        }
        RequestCommand::Logs { id, query } => {
            print_json(&artifactize_core::inspect::logs(state, &id, query.try_into()?).await?)?;
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
            return Ok(if result.is_error() { FAILURE_EXIT } else { 0 });
        }
        RequestCommand::Submit {
            id,
            verdict,
            fields,
            fields_file,
            reviewer,
        } => {
            let fields = match (fields, fields_file) {
                (None, None) => OwnerFields::default(),
                (Some(fields), None) => fields,
                (None, Some(path)) => OwnerFields::read_file(path)?,
                (Some(_), Some(_)) => {
                    unreachable!("clap makes --fields and --fields-file exclusive")
                }
            };
            let submission =
                Submission::new(verdict, fields.0).expect("OwnerFields excludes the verdict");
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (cancellation, listener) = cancellation_listener()?;
            let result =
                human::submit_and_publish(state, &id, &reviewer, &submission, cancellation).await;
            listener.abort();
            result?;
            print_json(
                &artifactize_core::inspect::request_show(
                    state,
                    &id,
                    time::OffsetDateTime::now_utc(),
                )
                .await?,
            )?;
        }
    }
    Ok(0)
}

/// A bounded JSON object without the verdict, supplied separately by `--verdict`.
#[derive(Clone, Debug, Default)]
pub struct OwnerFields(Map<String, Value>);

impl std::str::FromStr for OwnerFields {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() > human::MAX_RESULT_BYTES {
            return Err(format!(
                "Human fields exceed {} bytes.",
                human::MAX_RESULT_BYTES
            ));
        }
        let Value::Object(fields) = serde_json::from_str(text).map_err(|e| e.to_string())? else {
            return Err("Human fields must be a JSON object.".into());
        };
        if fields.contains_key("verdict") {
            return Err("Use --verdict, not a verdict inside --fields.".into());
        }
        Ok(Self(fields))
    }
}

#[derive(Clone)]
struct OwnerFieldsParser;

impl TypedValueParser for OwnerFieldsParser {
    type Value = OwnerFields;

    fn parse_ref(
        &self,
        command: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let text = value.to_str().ok_or_else(|| {
            clap::Error::raw(ErrorKind::InvalidUtf8, "Human fields must be UTF-8.")
                .with_cmd(command)
        })?;
        // Clap's default validation error includes the raw value; don't echo owner fields.
        text.parse().map_err(|error: String| {
            clap::Error::raw(ErrorKind::ValueValidation, error).with_cmd(command)
        })
    }
}

impl OwnerFields {
    fn read_file(path: PathBuf) -> Result<Self, String> {
        let file = artifactize_core::platform::open_regular_input(&path)
            .map_err(|e| e.to_string())?
            .ok_or("Human fields must be a regular JSON file.")?;
        let mut bytes = Vec::new();
        file.take(human::FIELDS_READ_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        std::str::from_utf8(&bytes)
            .map_err(|e| e.to_string())?
            .parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use artifactize_core::platform::path_text;
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
