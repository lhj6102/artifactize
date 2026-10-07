use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use clap::Subcommand;
use serde_json::{Value, json};

use super::{cancellation_listener, print_json};
use crate::{human, query, store, tools::human::Content};

#[derive(Debug, Subcommand)]
pub enum RequestCommand {
    /// List saved waiting, claimed, and settled requests.
    List {
        /// Only requests from this Run.
        #[arg(long, value_name = "RUN_ID")]
        run: Option<String>,
    },
    /// Read full saved audit, claim and summary as JSON.
    Show { id: String },
    /// Acquire the Human reviewer lock.
    Claim {
        id: String,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<String>,
    },
    /// Release your Human reviewer lock while the request still waits.
    Unclaim {
        id: String,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<String>,
    },
    /// Run a predefined Human tool as the claimant.
    Tool {
        id: String,
        tool: String,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<String>,
    },
    /// Submit a schema-valid Human verdict and owner fields.
    Submit {
        id: String,
        #[arg(long, value_parser = ["GREEN", "RED"])]
        verdict: String,
        /// Owner fields as a JSON object (default {}).
        #[arg(long, value_name = "JSON", conflicts_with = "fields_file")]
        fields: Option<String>,
        /// Read owner fields from a regular JSON file.
        #[arg(long, value_name = "PATH")]
        fields_file: Option<PathBuf>,
        /// Reviewer name (defaults to USER).
        #[arg(long, value_name = "NAME")]
        reviewer: Option<String>,
    },
}

pub(super) async fn execute(
    state: &Path,
    command: RequestCommand,
    json: bool,
) -> Result<u8, String> {
    match command {
        RequestCommand::List { run } => {
            let views = store::read_requests(state, run.as_deref()).await?;
            if json {
                print_json(
                    &views
                        .iter()
                        .map(|view| query::request_output(view, time::OffsetDateTime::now_utc()))
                        .collect::<Vec<_>>(),
                )?;
            } else {
                let mut out = io::stdout().lock();
                writeln!(out, "REQUEST\tRUN\tEVAL\tSTATUS\tREVIEWER").map_err(|e| e.to_string())?;
                for view in views {
                    writeln!(
                        out,
                        "{}\t{}\t{}\t{}\t{}",
                        view.request.id,
                        view.request.run_id,
                        view.request.eval_id,
                        view.request.status,
                        view.claim
                            .as_ref()
                            .map_or("-", |claim| claim.reviewer.as_str())
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        RequestCommand::Show { id } => {
            print_json(&query::request_output(
                &store::read_request(state, &id).await?,
                time::OffsetDateTime::now_utc(),
            ))?;
        }
        RequestCommand::Claim { id, reviewer } => {
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (receipts, _) = human::open(state, &id).await?;
            print_json(&human::claim(&receipts, &id, &reviewer).await?)?;
        }
        RequestCommand::Unclaim { id, reviewer } => {
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (receipts, _) = human::open(state, &id).await?;
            print_json(&human::unclaim(&receipts, &id, &reviewer).await?)?;
        }
        RequestCommand::Tool { id, tool, reviewer } => {
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (receipts, _) = human::open(state, &id).await?;
            let (cancellation, listener) = cancellation_listener()?;
            let result =
                human::run_human_tool(&receipts, &id, &reviewer, &tool, cancellation).await;
            listener.abort();
            let result = result?;
            if json {
                print_json(&result)?;
            } else {
                let mut out = io::stdout().lock();
                for content in &result.content {
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
            return Ok(if result.is_error { 2 } else { 0 });
        }
        RequestCommand::Submit {
            id,
            verdict,
            fields,
            fields_file,
            reviewer,
        } => {
            let result = submission(&verdict, fields, fields_file)?;
            let reviewer = reviewer.map_or_else(human::default_reviewer, Ok)?;
            let (cancellation, listener) = cancellation_listener()?;
            let result =
                human::submit_and_publish(state, &id, &reviewer, &result, cancellation).await;
            listener.abort();
            result?;
            print_json(&query::request_output(
                &store::read_request(state, &id).await?,
                time::OffsetDateTime::now_utc(),
            ))?;
        }
    }
    Ok(0)
}

fn submission(
    verdict: &str,
    fields: Option<String>,
    file: Option<PathBuf>,
) -> Result<Value, String> {
    let fields = if let Some(path) = file {
        let file = crate::platform::open_nonblocking(&path).map_err(|e| e.to_string())?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Human fields must be a regular JSON file.".into());
        }
        let mut bytes = Vec::new();
        file.take(256_001)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())?
    } else {
        fields.unwrap_or_else(|| "{}".into())
    };
    if fields.len() > 256_000 {
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
