//! Repository-bound inspection and lossless detail paging, with no CLI duplication.
use super::{RUN_LIST_LIMIT, encode, input::*, page, parse};
use artifactize_core::{project::VerifyOptions, runs};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

pub(super) async fn inspect(
    context: &runs::Context,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let repo = context.repo();
    let state = Some(context.state());

    let cursor = arguments
        .get("cursor")
        .map(|value| {
            value
                .as_str()
                .ok_or("cursor must be a string".to_owned())?
                .parse::<page::Cursor>()
        })
        .transpose()?;
    let as_of = cursor
        .as_ref()
        .map_or_else(time::OffsetDateTime::now_utc, |cursor| cursor.as_of);
    let mut query = arguments.clone();
    if let Some(object) = query.as_object_mut() {
        object.remove("cursor");
    }
    let arguments = query.clone();
    let output = match name {
        "status" => {
            let args: Status = parse(arguments)?;
            encode(
                artifactize_core::project::status(
                    repo,
                    state,
                    &args.scope.0,
                    &VerifyOptions::default(),
                    CancellationToken::new(),
                )
                .await?,
            )?
        }
        "config_graph" => {
            let args: GraphInput = parse(arguments)?;
            let config =
                artifactize_core::config::read_workspace_config(repo).map_err(|e| e.to_string())?;
            let selection = if args.impact {
                artifactize_core::query::impact(&config, &args.scope.0)?
            } else {
                args.scope.0
            };
            encode(artifactize_core::query::graph(&config, &selection)?)?
        }
        "request_logs" => {
            let args: LogInput = parse(arguments)?;
            if artifactize_core::store::read_request_repo(context.state(), &args.id)
                .await?
                .1
                != *repo
            {
                return Err("Request not found in this repository.".into());
            }
            encode(artifactize_core::inspect::logs(context.state(), &args.id, args.query).await?)?
        }
        "config_check" => {
            let _: Empty = parse(arguments)?;
            encode(artifactize_core::inspect::config_check(repo)?)?
        }
        "run_list" => {
            let args: Runs = parse(arguments)?;
            let limit = args.limit.unwrap_or(RUN_LIST_LIMIT);
            if limit > RUN_LIST_LIMIT {
                return Err("limit must be at most 20; use offset for older Runs.".into());
            }
            encode(
                artifactize_core::store::read_runs(
                    context.state(),
                    Some(repo),
                    limit,
                    args.offset.unwrap_or(0),
                )
                .await?,
            )?
        }
        "run_show" => {
            let args: RunDetails = parse(arguments)?;
            artifactize_core::query::run_output(&runs::view(context, &args.run).await?, as_of)
        }
        "request_list" => {
            let args: Requests = parse(arguments)?;
            if let Some(run) = &args.run {
                runs::view(context, run).await?;
            }
            encode(
                artifactize_core::inspect::request_list(
                    context.state(),
                    args.run.as_ref(),
                    Some(repo),
                    as_of,
                )
                .await?,
            )?
        }
        "request_show" => {
            let args: super::input::Request = parse(arguments)?;
            let view = artifactize_core::store::read_request(context.state(), &args.id).await?;
            if artifactize_core::store::read_request_repo(context.state(), &view.request.id)
                .await?
                .1
                != repo
            {
                return Err("Request not found in this repository.".into());
            }
            artifactize_core::inspect::request_show(context.state(), &args.id, as_of).await?
        }
        "session_show" => {
            let args: Session = parse(arguments)?;
            let located =
                artifactize_core::inspect::session::locate(context.state(), &args.reference)
                    .await?;
            runs::view(context, &located.request.request.run_id).await?;
            artifactize_core::inspect::session_show(context.state(), &args.reference, args.summary)
                .await?
        }
        "tools_check" => {
            let args: Tools = parse(arguments)?;
            let options = artifactize_core::diagnostics::ToolCheckOptions {
                eval: args.eval,
                artifact: args.artifact,
                tool: args.tool,
                audience: args.audience.map(Into::into),
                ..Default::default()
            };
            encode(
                artifactize_core::diagnostics::check_tools(
                    repo,
                    state,
                    &options,
                    CancellationToken::new(),
                )
                .await?,
            )?
        }
        "doctor" => {
            let _: Empty = parse(arguments)?;
            encode(artifactize_core::diagnostics::doctor(state, Some(repo)).await?)?
        }
        "models" => {
            let args: Models = parse(arguments)?;
            encode(artifactize_core::llm::models::list(args.provider, state, Some(repo)).await?)?
        }
        "remote_status" => {
            let _: Empty = parse(arguments)?;
            artifactize_core::inspect::remote_status(state, Some(repo)).await?
        }
        "cache_show" => {
            let args: Cache = parse(arguments)?;
            let mut records =
                artifactize_core::cache::show(context.state(), &args.key, args.history).await?;
            if args.history {
                encode(records)?
            } else {
                encode(records.pop())?
            }
        }
        _ => return Err(format!("Unknown tool: {name}")),
    };
    page::project(context, name, &query, output, cursor, as_of)
}
