//! Route parsed commands to focused workflows; no business logic or rendering here.

use super::{
    Cli, Command, ConfigCommand, Invocation, Parsed, RunCommand, account, cache, chat_agent,
    config, maintenance, plugin, provider, remote, request, run, server, session, status, workflow,
};
use clap::CommandFactory;
use std::{io, path::PathBuf};

pub(super) struct Context {
    pub repo: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub json: bool,
}

pub(super) async fn execute(cli: Parsed) -> Result<u8, String> {
    let Parsed {
        state_dir,
        json,
        invocation,
    } = cli;
    match invocation {
        Invocation::Desktop(launch) => launch.run(state_dir).await,
        Invocation::ToolCheck { options, repo } => {
            maintenance::tools(
                Context {
                    repo,
                    state_dir,
                    json,
                },
                options,
            )
            .await
        }
        Invocation::Tool(tool) => Ok(artifactize_tools::cli::run(tool).await),
        Invocation::General(command, repo) => {
            execute_general(
                command,
                Context {
                    repo,
                    state_dir,
                    json,
                },
            )
            .await
        }
    }
}

async fn execute_general(command: Option<Command>, context: Context) -> Result<u8, String> {
    let executor = matches!(command, Some(Command::VerifyExecution { .. }));
    match command {
        Some(Command::ChatMcp) => {
            artifactize_core::acp::verify_bridge::stdio()?;
            Ok(0)
        }
        Some(Command::Mcp) => {
            let executable =
                artifactize_core::platform::current_executable().map_err(|e| e.to_string())?;
            // Without --repo each call's `project`, the client's roots or the working
            // directory name the project, as a plugin's server starts in the plugin's folder.
            let repo = context
                .repo
                .map(|repo| {
                    artifactize_core::runs::Context::new(
                        &repo,
                        context.state_dir.as_deref(),
                        &executable,
                    )
                })
                .transpose()?;
            let launch = artifactize_mcp::Launch {
                repo,
                state: context.state_dir,
                executable,
            };
            artifactize_mcp::serve(launch)
                .await
                .map_err(|error| error.to_string())?;
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
                &artifactize_core::platform::current_executable().map_err(|e| e.to_string())?,
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
        Some(Command::Login { provider }) => account::login(context, provider).await,
        Some(Command::Logout { provider }) => account::logout(context, provider).await,
        Some(Command::Provider { command }) => {
            provider::execute(
                context.state_dir.as_deref(),
                context.repo.as_deref(),
                command,
                context.json,
            )
            .await
        }
        Some(Command::ChatAgent { command }) => {
            chat_agent::execute(
                context.state_dir.as_deref(),
                context.repo.as_deref(),
                command,
                context.json,
            )
            .await
        }
        Some(Command::Plugin { command }) => plugin::execute(context, command),
        Some(Command::Doctor) => maintenance::doctor(context).await,
        Some(Command::Prune {
            older_than,
            dry_run,
        }) => maintenance::prune(context, older_than, dry_run),
        Some(Command::Limits { signoff_idle_grace }) => {
            maintenance::limits(context, signoff_idle_grace)
        }
        Some(Command::Models { provider }) => account::models(context, provider).await,
        Some(Command::Verify {
            selection,
            policy,
            jobs,
            max_executions,
            follow,
            request_key,
            comment,
            reuse_only,
            speculate,
            defer_human: None,
        }) => {
            workflow::verify(
                context,
                selection.resolve()?,
                policy,
                workflow::VerifyLimits {
                    modes: artifactize_core::runs::RunModes { speculate },
                    jobs,
                    max_executions,
                    follow,
                    request_key,
                    comment,
                    reuse_only,
                },
            )
            .await
        }
        Some(Command::Status { selection, policy }) => {
            status::execute(context, selection.resolve()?, policy).await
        }
        Some(Command::Graph { .. }) => Err(r#"graph moved to "artifactize config graph""#.into()),
        Some(Command::Config {
            command: ConfigCommand::Graph { artifact, impact },
        }) => config::graph(context, artifact, impact),
        Some(Command::Config {
            command: ConfigCommand::Rename { old, new },
        }) => config::rename(context, old, new),
        Some(Command::Config {
            command: ConfigCommand::Migrate,
        }) => config::migrate(context),
        Some(Command::Config {
            command: ConfigCommand::Expand { artifact },
        }) => config::expand(context, artifact),
        Some(Command::Config {
            command: ConfigCommand::Symbols,
        }) => config::symbols(context),
        Some(Command::Config {
            command: ConfigCommand::Check,
        }) => config::check(context),
        Some(Command::Run {
            command:
                RunCommand::List {
                    scope,
                    limit,
                    offset,
                },
        }) => run::list(context, scope, limit, offset).await,
        Some(Command::Run {
            command: RunCommand::Show { run_id, mode },
        }) => run::show(context, run_id, mode).await,
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
