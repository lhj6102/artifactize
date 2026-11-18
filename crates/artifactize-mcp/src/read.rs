//! Repository-bound inspection and lossless detail paging, with no CLI duplication.
use super::{
    error::Error,
    input::{Inspection, Read},
    page::Page,
};
use artifactize_core::{
    cache, config, diagnostics, inspect, llm, platform, project, query, runs, store,
    types::RequestId,
};
use serde_json::to_string;
use tokio_util::sync::CancellationToken;

pub(super) async fn inspect(
    context: &runs::Context,
    Inspection { read, page }: Inspection,
) -> Result<Page, Error> {
    let repo = context.repo();
    let state = Some(context.state());
    let as_of = page.as_of();
    let text = match read {
        Read::Status(args) => to_string(
            &project::status(
                repo,
                state,
                args.scope.selection(),
                &project::VerifyOptions::default(),
                CancellationToken::new(),
            )
            .await?,
        )?,
        Read::Graph(args) => {
            let config = config::read_workspace_config(repo).map_err(Error::Config)?;
            let selection = args.scope.selection();
            if args.impact {
                to_string(&query::graph(&config, &query::impact(&config, selection)?)?)?
            } else {
                to_string(&query::graph(&config, selection)?)?
            }
        }
        Read::Logs(args) => {
            verify_request_project(context, &args.id).await?;
            to_string(&inspect::logs(context.state(), &args.id, args.query).await?)?
        }
        Read::Config => to_string(&inspect::config_check(repo)?)?,
        Read::Runs(args) => to_string(
            &store::read_runs(
                context.state(),
                Some(repo),
                args.limit(),
                args.offset.unwrap_or_default(),
            )
            .await?,
        )?,
        Read::Run(run) => to_string(&query::run_output(&runs::view(context, &run).await?, as_of))?,
        Read::Requests(args) => {
            if let Some(run) = &args.run {
                runs::view(context, run).await?;
            }
            to_string(
                &inspect::request_list(context.state(), args.run.as_ref(), Some(repo), as_of)
                    .await?,
            )?
        }
        Read::Request(id) => {
            verify_request_project(context, &id).await?;
            to_string(&inspect::request_show(context.state(), &id, as_of).await?)?
        }
        Read::Session(args) => {
            let located = inspect::session::locate(context.state(), &args.reference).await?;
            runs::view(context, &located.request.request.run_id).await?;
            to_string(
                &inspect::session_show(context.state(), &args.reference, args.summary).await?,
            )?
        }
        Read::Tools(options) => to_string(
            &diagnostics::check_tools(repo, state, &options, CancellationToken::new()).await?,
        )?,
        Read::Doctor => to_string(&diagnostics::doctor(state, Some(repo)).await?)?,
        Read::Models(provider) => {
            to_string(&llm::models::list(provider, state, Some(repo)).await?)?
        }
        Read::Remote => to_string(&inspect::remote_status(state, Some(repo)).await?)?,
        Read::Cache(args) => {
            let records = cache::show(context.state(), &args.key, args.history).await?;
            if args.history {
                to_string(&records)?
            } else {
                to_string(&records.into_iter().next_back())?
            }
        }
    };
    page.project(context, &text)
}

async fn verify_request_project(context: &runs::Context, request: &RequestId) -> Result<(), Error> {
    let repo = store::read_request_repo(context.state(), request).await?;
    if !platform::paths_equal(&repo, context.repo()) {
        return Err(Error::RequestOutsideProject);
    }
    Ok(())
}
