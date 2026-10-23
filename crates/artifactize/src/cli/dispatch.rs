//! Route parsed commands to focused workflows; no business logic or rendering here.

use super::{
    Cli, Command, ConfigCommand, RunCommand, Screens, ToolsCommand, cache, interactive,
    maintenance, provider, remote, request, run, server, session, workflow,
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
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
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
            artifactize_mcp::serve(launch).await?;
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
        Some(Command::Login { provider }) => maintenance::login(context, provider).await,
        Some(Command::Logout { provider }) => maintenance::logout(context, provider).await,
        Some(Command::Provider { command }) => {
            provider::execute(
                context.state_dir.as_deref(),
                context.repo.as_deref(),
                command,
                context.json,
            )
            .await
        }
        Some(Command::Doctor) => maintenance::doctor(context).await,
        Some(Command::Prune {
            older_than,
            dry_run,
        }) => maintenance::prune(context, older_than, dry_run).await,
        Some(Command::Limits { signoff_idle_grace }) => {
            maintenance::limits(context, signoff_idle_grace)
        }
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
            if defer_human {
                return Err("--defer-human was removed in 0.16.0: every verify lets machine work go on while sign-offs wait; remove the flag.".into());
            }
            workflow::verify(
                context,
                selection,
                policy,
                workflow::VerifyLimits {
                    modes: artifactize_core::runs::RunModes { speculate },
                    jobs,
                    max_executions,
                    follow: workflow::Follow::from_flags(wait, watch),
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
