//! Route parsed commands to focused workflows; no business logic or rendering here.

use super::{
    AuthProvider, Cli, Command, ConfigCommand, RunCommand, Screens, ToolsCommand, cache,
    interactive, maintenance, remote, request, run, server, session, workflow,
};
use clap::CommandFactory;
use std::{io, path::PathBuf};

pub(super) struct Context {
    pub repo: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub json: bool,
}

pub(super) async fn execute(cli: Cli, screens: Screens) -> Result<u8, String> {
    let context = Context {
        repo: cli.repo,
        state_dir: cli.state_dir,
        json: cli.json,
    };
    let executor = matches!(cli.command, Some(Command::VerifyExecution { .. }));
    match cli.command {
        Some(Command::Mcp) => {
            let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
            let runs = artifactize_core::runs::Context::new(
                &repo,
                context.state_dir.as_deref(),
                &std::env::current_exe().map_err(|e| e.to_string())?,
            )?;
            artifactize_mcp::serve(runs).await?;
            Ok(0)
        }
        None => {
            Cli::command()
                .write_help(&mut io::stdout().lock())
                .map_err(|error| error.to_string())?;
            Ok(0)
        }
        Some(Command::VerifyWorker { run_id }) | Some(Command::VerifyExecution { run_id }) => {
            let runs = artifactize_core::runs::Context::new(
                context.repo.as_deref().unwrap_or(std::path::Path::new(".")),
                context.state_dir.as_deref(),
                &std::env::current_exe().map_err(|e| e.to_string())?,
            )?;
            if executor {
                artifactize_core::runs::execute(&runs, &run_id).await?;
            } else {
                artifactize_core::runs::supervise(&runs, &run_id).await?;
            }
            Ok(0)
        }
        Some(Command::Run {
            command: RunCommand::Watch { run_id },
        }) => run::watch(context, run_id).await,
        Some(Command::Run {
            command: RunCommand::Cancel { run_id },
        }) => run::cancel(context, run_id).await,
        Some(Command::Tools {
            command: ToolsCommand::Check(options),
        }) => maintenance::tools(context, options.into()).await,
        Some(Command::Tools {
            command: ToolsCommand::Builtin(tool),
        }) => Ok(artifactize_tools::cli::run(tool).await),
        Some(Command::Login {
            provider: AuthProvider::Codex,
        }) => maintenance::login(context).await,
        Some(Command::Logout {
            provider: AuthProvider::Codex,
        }) => maintenance::logout(context).await,
        Some(Command::Doctor) => maintenance::doctor(context).await,
        Some(Command::Prune {
            older_than,
            dry_run,
        }) => maintenance::prune(context, older_than, dry_run).await,
        Some(Command::Models { provider }) => maintenance::models(context, provider).await,
        Some(Command::Verify {
            selection,
            policy,
            jobs,
            max_executions,
            wait,
            watch,
            request_key,
            reuse_only,
            speculate,
            defer_human,
        }) => {
            workflow::verify(
                context,
                selection,
                policy,
                workflow::VerifyLimits {
                    modes: artifactize_core::runs::RunModes {
                        speculate,
                        defer_human,
                    },
                    jobs,
                    max_executions,
                    wait,
                    watch,
                    request_key,
                    reuse_only,
                },
            )
            .await
        }
        Some(Command::Status { selection, policy }) => {
            workflow::status(context, selection, policy).await
        }
        Some(Command::Graph { .. }) => Err(r#"graph moved to "artifactize config graph""#.into()),
        Some(Command::Config {
            command: ConfigCommand::Graph { artifact, impact },
        }) => workflow::graph(context, artifact, impact).await,
        Some(Command::Config {
            command: ConfigCommand::Check,
        }) => workflow::check(context).await,
        Some(Command::Run {
            command: RunCommand::List {
                all, limit, offset, ..
            },
        }) => run::list(context, all, limit, offset).await,
        Some(Command::Run {
            command:
                RunCommand::Show {
                    run_id,
                    wait,
                    timeout_ms,
                },
        }) => run::show(context, run_id, wait, timeout_ms).await,
        Some(Command::Monitor { all, path }) => {
            interactive::monitor(context, all, path, screens).await
        }
        Some(Command::Review {
            request,
            all,
            reviewer,
        }) => interactive::review(context, request, all, reviewer, screens).await,
        Some(Command::Cache { command }) => cache::execute(context, command).await,
        Some(Command::Remote { command }) => {
            remote::execute(
                context.state_dir.as_deref(),
                context.repo.as_deref(),
                command,
                context.json,
            )
            .await
        }
        Some(Command::Server { command }) => {
            let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
            server::execute(&state, command, context.json).await
        }
        Some(Command::Request { command }) => {
            let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
            request::execute(&state, command, context.json).await
        }
        Some(Command::Session { command }) => {
            let state = artifactize_core::store::state_dir(context.state_dir.as_deref())?;
            session::execute(&state, context.repo.as_deref(), command, context.json).await
        }
    }
}
