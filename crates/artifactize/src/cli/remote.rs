use std::{
    io::{self, Write},
    path::Path,
};

use clap::Subcommand;
use serde_json::json;

use super::print_json;
use artifactize_core::auth::remote::{self, Share};

#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
    /// Verify a token read from stdin (never argv) and save it with remote.json.
    Login {
        /// Store URL: https://, or http:// only on loopback.
        url: String,
        /// What published records contain.
        #[arg(long, value_enum, default_value = "summary")]
        share: Share,
    },
    /// Forget the stored token and remote.json; the server keeps the token until revoked.
    Logout,
    /// Show the URL, share level, token source, reachability, principal and scopes.
    Status,
    /// Re-send local GREEN/RED results; keys the store already has are no-ops.
    Push {
        /// Report what would be sent without publishing.
        #[arg(long)]
        dry_run: bool,
    },
}

pub(super) async fn execute(
    state: Option<&Path>,
    repo: Option<&Path>,
    command: RemoteCommand,
    json: bool,
) -> Result<u8, String> {
    let mut out = io::stdout();
    match command {
        RemoteCommand::Login { url, share } => {
            let url = remote::parse_url(&url)?;
            let token = super::secret::read("remote token")?;
            let (remote, principal) = remote::login(state, repo, url, share, token).await?;
            if json {
                print_json(&json!({
                    "url": remote.url,
                    "share": share,
                    "principal": principal.principal,
                    "scopes": principal.scopes,
                }))?;
            } else {
                writeln!(
                    out,
                    "Signed in to {} as {} (scopes: {}).",
                    remote.url,
                    principal.principal,
                    principal
                        .scopes
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(0)
        }
        RemoteCommand::Logout => {
            remote::logout(state, repo)?;
            if json {
                print_json(&json!({"signedIn": false}))?;
            } else {
                writeln!(
                    out,
                    "Signed out of the remote review store. The server keeps the token valid until `artifactize server token revoke`."
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(0)
        }
        RemoteCommand::Push { dry_run } => {
            let report = artifactize_core::remote::push(state, repo, dry_run).await?;
            if json {
                print_json(&report)?;
            } else {
                writeln!(
                    out,
                    "{} {}, already in the store {}, skipped {}.",
                    if dry_run { "Would push" } else { "Pushed" },
                    report.pushed,
                    report.existing,
                    report.skipped
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(0)
        }
        RemoteCommand::Status => {
            let Some(status) = remote::status(state, repo).await? else {
                if json {
                    print_json(&artifactize_core::inspect::remote_output(None))?;
                } else {
                    writeln!(out, "No remote review store is configured.")
                        .map_err(|e| e.to_string())?;
                }
                return Ok(1);
            };
            if json {
                print_json(&artifactize_core::inspect::remote_output(Some(&status)))?;
            } else {
                writeln!(
                    out,
                    "Remote: {}\nShare: {}\nToken: {}\nReachable: {}",
                    status.url,
                    json!(status.share).as_str().unwrap_or_default(),
                    json!(status.token_source).as_str().unwrap_or_default(),
                    if status.reachable() { "yes" } else { "no" }
                )
                .map_err(|e| e.to_string())?;
                match &status.connection {
                    remote::Connection::SignedIn { principal, scopes } => writeln!(
                        out,
                        "Principal: {} (scopes: {})",
                        principal,
                        scopes
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    remote::Connection::Refused { error }
                    | remote::Connection::Unreachable { error } => writeln!(out, "Error: {error}"),
                }
                .map_err(|e| e.to_string())?;
            }
            Ok(u8::from(!status.signed_in()))
        }
    }
}
