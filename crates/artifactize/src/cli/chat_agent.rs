//! Manage machine-local ACP Chat agents, official ACP Registry installs, and Workspace Trust.

use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::{Path, PathBuf},
};

use clap::Subcommand;
use serde_json::json;

use artifactize_core::acp::{
    AgentId,
    catalog::{self, AgentAvailability, CustomAgent, SecretEnvValue},
    trust::{self, LauncherFingerprint, TrustScope},
};

use super::print_json;

#[derive(Debug, Subcommand)]
pub enum ChatAgentCommand {
    /// List built-in ACP presets, Registry-installed, and custom ACP Chat agents.
    List,
    /// Probe an ACP agent in an empty private tempdir and show its models and configOptions.
    Probe {
        id: AgentId,
        /// Bypass the short probe cache and launch a fresh probe.
        #[arg(long)]
        refresh: bool,
    },
    /// Save or replace a machine-local custom ACP Chat agent.
    Add {
        id: AgentId,
        /// Display name shown in Settings and Chat (defaults to id).
        #[arg(long)]
        name: Option<String>,
        /// Executable name on PATH or absolute path.
        #[arg(long)]
        command: String,
        /// Repeatable literal argument passed to the executable.
        #[arg(long = "arg")]
        args: Vec<String>,
        /// Repeatable non-secret environment pair (KEY=VALUE).
        #[arg(long = "env", value_parser = parse_env_pair)]
        env: Vec<(String, String)>,
        /// Repeatable secret env variable name whose value is read from stdin (never argv).
        #[arg(long = "secret-env")]
        secret_env: Vec<String>,
        /// Disable Artifactize's one-turn Verify MCP bridge for this agent.
        #[arg(long)]
        no_mcp_stdio: bool,
    },
    /// Remove a custom or Registry-installed ACP Chat agent and its stored secrets.
    Remove { id: AgentId },
    /// Browse entries from the official ACP Registry (discovery-only; never executes anything).
    Registry,
    /// Install a pinned entry from the official ACP Registry after inspecting its exact spec.
    Install {
        id: AgentId,
        /// Confirm installation of the displayed pinned version, source, and digest policy.
        #[arg(long)]
        yes: bool,
        /// Allow package lifecycle build scripts (disabled by default).
        #[arg(long)]
        allow_build_scripts: bool,
        /// Allow installing an archive/binary entry without a published SHA-256 digest.
        #[arg(long)]
        allow_unsigned: bool,
    },
    /// Inspect, grant, or revoke machine-local Workspace Trust for ACP Chat agents.
    Trust {
        #[command(subcommand)]
        command: TrustCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum TrustCommand {
    /// List saved Workspace Trust records.
    List,
    /// Trust an ACP agent to run in a worktree (or its shared Git repository).
    Grant {
        agent: AgentId,
        /// Worktree directory to trust (defaults to --repo or the current directory).
        path: Option<PathBuf>,
    },
    /// Revoke a saved Workspace Trust record.
    Revoke {
        agent: AgentId,
        /// Worktree directory whose trust scope should be revoked (defaults to --repo or cwd).
        path: Option<PathBuf>,
    },
}

fn parse_env_pair(raw: &str) -> Result<(String, String), String> {
    let (key, value) = raw
        .split_once('=')
        .ok_or_else(|| format!("Environment pair {raw:?} must use KEY=VALUE syntax."))?;
    let key = key.trim();
    if key.is_empty() {
        return Err("Environment variable name cannot be empty.".into());
    }
    Ok((key.to_owned(), value.to_owned()))
}

pub(super) async fn execute(
    state: Option<&Path>,
    repo: Option<&Path>,
    command: ChatAgentCommand,
    json: bool,
) -> Result<u8, String> {
    match command {
        ChatAgentCommand::List => {
            let entries = catalog::list_catalog(state, repo)?;
            if json {
                print_json(&entries)?;
            } else {
                let mut out = io::stdout().lock();
                for entry in entries {
                    let tier = match entry.tier {
                        catalog::AgentTier::Preset => "preset",
                        catalog::AgentTier::Registry => "registry",
                        catalog::AgentTier::Custom => "custom",
                    };
                    writeln!(
                        out,
                        "{}\t{tier}\t{}\t{}",
                        entry.id, entry.name, entry.command
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        ChatAgentCommand::Probe { id, refresh } => {
            let availability = catalog::probe_agent(&id, state, repo, refresh).await;
            if json {
                print_json(&availability)?;
            } else {
                let mut out = io::stdout().lock();
                match &availability {
                    AgentAvailability::Ready {
                        executable,
                        version,
                        config_options,
                        ..
                    } => {
                        writeln!(
                            out,
                            "{id}\tready\t{}\t{}",
                            artifactize_core::platform::path_text(executable),
                            version.as_deref().unwrap_or("unknown")
                        )
                        .map_err(|e| e.to_string())?;
                        for opt in config_options {
                            writeln!(out, "  {} = {}", opt.id, opt.current_value)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    AgentAvailability::Missing { reason } => {
                        writeln!(out, "{id}\tmissing\t{reason}").map_err(|e| e.to_string())?;
                    }
                    AgentAvailability::TooOld {
                        version,
                        required_version,
                        ..
                    } => {
                        writeln!(out, "{id}\ttoo-old\t{version} < {required_version}")
                            .map_err(|e| e.to_string())?;
                    }
                    AgentAvailability::NotSetUp { reason, .. } => {
                        writeln!(out, "{id}\tnot-set-up\t{reason}").map_err(|e| e.to_string())?;
                    }
                    AgentAvailability::Failed { error, .. } => {
                        writeln!(out, "{id}\tfailed\t{error}").map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        ChatAgentCommand::Add {
            id,
            name,
            command,
            args,
            env,
            secret_env,
            no_mcp_stdio,
        } => {
            let mut secrets = BTreeMap::new();
            for key in secret_env {
                let key = key.trim().to_owned();
                if key.is_empty() {
                    return Err("Secret environment variable name cannot be empty.".into());
                }
                let raw = super::secret::read(&format!("secret env {key}"))?;
                secrets.insert(key, SecretEnvValue::parse(&raw)?);
            }
            let agent = CustomAgent {
                name: name.unwrap_or_else(|| id.as_str().to_owned()),
                id: id.clone(),
                command,
                args,
                env: env.into_iter().collect(),
                secret_env_keys: Vec::new(),
                mcp_stdio: !no_mcp_stdio,
                registry: None,
                entrypoint_sha256: None,
            };
            let saved = catalog::save_custom_agent(state, repo, agent, secrets, &[]).await?;
            if json {
                print_json(&saved)?;
            } else {
                writeln!(
                    io::stdout().lock(),
                    "Saved custom ACP Chat agent {}.",
                    saved.id
                )
                .map_err(|e| e.to_string())?;
            }
        }
        ChatAgentCommand::Remove { id } => {
            let removed = catalog::remove_custom_agent(state, repo, &id).await?;
            if !removed {
                return Err(format!("No custom ACP Chat agent named {id:?} is saved."));
            }
            if json {
                print_json(&json!({ "id": id, "removed": true }))?;
            } else {
                writeln!(io::stdout().lock(), "Removed custom ACP Chat agent {id}.")
                    .map_err(|e| e.to_string())?;
            }
        }
        ChatAgentCommand::Registry => {
            let entries = catalog::discover_registry().await?;
            if json {
                print_json(&entries)?;
            } else {
                let mut out = io::stdout().lock();
                for entry in entries {
                    writeln!(
                        out,
                        "{}\t{}\t{}\t{}",
                        entry.id, entry.version, entry.name, entry.spec.package_or_url
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        ChatAgentCommand::Install {
            id,
            yes,
            allow_build_scripts,
            allow_unsigned,
        } => {
            let entries = catalog::discover_registry().await?;
            let mut entry = entries
                .into_iter()
                .find(|entry| entry.id == id)
                .ok_or_else(|| format!("No ACP Registry entry found for {id:?}."))?;
            entry.spec.build_scripts_allowed = allow_build_scripts;
            entry.spec.allow_unsigned_digest = allow_unsigned;
            if !yes {
                if json {
                    print_json(&json!({
                        "installed": false,
                        "confirmationRequired": true,
                        "spec": entry.spec,
                    }))?;
                } else {
                    let mut out = io::stdout().lock();
                    writeln!(
                        out,
                        "Registry entry {} ({}) v{}\n  source: {}\n  platform: {}\n  sha256: {}\nRe-run with --yes to install this pinned version.",
                        entry.spec.registry_id,
                        entry.spec.display_name,
                        entry.spec.exact_version,
                        entry.spec.package_or_url,
                        entry.spec.platform,
                        entry.spec.artifact_sha256.as_deref().unwrap_or("none (requires --allow-unsigned)"),
                    )
                    .map_err(|e| e.to_string())?;
                }
                return Ok(2);
            }
            let installed = catalog::install_registry_agent(state, repo, entry.spec).await?;
            if json {
                print_json(&installed)?;
            } else {
                writeln!(
                    io::stdout().lock(),
                    "Installed ACP Chat agent {} at {}.",
                    installed.id,
                    installed.command
                )
                .map_err(|e| e.to_string())?;
            }
        }
        ChatAgentCommand::Trust { command } => match command {
            TrustCommand::List => {
                let records = trust::list_trust(state, repo)?;
                if json {
                    print_json(&records)?;
                } else {
                    let mut out = io::stdout().lock();
                    for record in records {
                        writeln!(
                            out,
                            "{}\t{}\t{}\t{}",
                            record.agent_id,
                            record.scope.display_label(),
                            artifactize_core::platform::path_text(
                                &record.fingerprint.resolved_executable
                            ),
                            record.fingerprint.executable_sha256
                        )
                        .map_err(|e| e.to_string())?;
                    }
                }
            }
            TrustCommand::Grant { agent, path } => {
                let worktree = resolve_trust_target(path.as_deref().or(repo))?;
                let identity = artifactize_core::repository::identify(&worktree);
                let spec = catalog::resolve_spec(&agent, state, Some(&worktree))?;
                let resolved = trust::resolve_executable(
                    &spec.program,
                    Some(&worktree),
                    spec.reject_worktree_executable,
                )?;
                let fingerprint = LauncherFingerprint::compute(&resolved, &spec.args)?;
                let scope = TrustScope::for_worktree(&worktree, &identity)?;
                let record = trust::grant_trust(state, None, spec.id, scope, fingerprint).await?;
                if json {
                    print_json(&record)?;
                } else {
                    writeln!(
                        io::stdout().lock(),
                        "Trusted {} for {} (sha256 {}).",
                        record.agent_id,
                        record.scope.display_label(),
                        record.fingerprint.executable_sha256
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            TrustCommand::Revoke { agent, path } => {
                let worktree = resolve_trust_target(path.as_deref().or(repo))?;
                let identity = artifactize_core::repository::identify(&worktree);
                let scope = TrustScope::for_worktree(&worktree, &identity)?;
                let removed = trust::revoke_trust(state, None, &agent, &scope).await?;
                if json {
                    print_json(&json!({
                        "agentId": agent,
                        "scope": scope,
                        "revoked": removed,
                    }))?;
                } else if removed {
                    writeln!(
                        io::stdout().lock(),
                        "Revoked trust for {agent} on {}.",
                        scope.display_label()
                    )
                    .map_err(|e| e.to_string())?;
                } else {
                    writeln!(
                        io::stdout().lock(),
                        "No saved trust record for {agent} on {}.",
                        scope.display_label()
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        },
    }
    Ok(0)
}

fn resolve_trust_target(explicit: Option<&Path>) -> Result<PathBuf, String> {
    let target = match explicit {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    artifactize_core::workspace::canonical_target(&target).map_err(|e| e.to_string())
}
