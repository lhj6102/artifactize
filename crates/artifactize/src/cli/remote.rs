use std::{
    io::{self, BufRead, IsTerminal, Read, Write},
    path::Path,
};

use clap::Subcommand;
use serde_json::json;

use super::print_json;
use crate::{
    auth::remote::{self, Share},
    platform::HiddenInput,
};

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
            remote::parse_url(&url)?;
            let token = read_token()?;
            let (remote, principal) = remote::login(state, repo, &url, share, token).await?;
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
                    principal.scopes.join(", ")
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
            let report = crate::remote::push(state, repo, dry_run).await?;
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
            let Some(remote) = remote::remote(state, repo)? else {
                if json {
                    print_json(&json!({"configured": false}))?;
                } else {
                    writeln!(out, "No remote review store is configured.")
                        .map_err(|e| e.to_string())?;
                }
                return Ok(1);
            };
            let (principal, failure) = match remote.whoami().await {
                Ok(principal) => (Some(principal), None),
                Err(failure) => (None, Some(failure)),
            };
            let reachable =
                principal.is_some() || failure.as_ref().is_some_and(|failure| failure.responded);
            if json {
                print_json(&json!({
                    "configured": true,
                    "url": remote.url,
                    "share": remote.share,
                    "tokenSource": remote.token_source,
                    "reachable": reachable,
                    "principal": principal.as_ref().map(|principal| &principal.principal),
                    "scopes": principal.as_ref().map(|principal| &principal.scopes),
                    "error": failure.as_ref().map(|failure| &failure.message),
                }))?;
            } else {
                writeln!(
                    out,
                    "Remote: {}\nShare: {}\nToken: {}\nReachable: {}",
                    remote.url,
                    json!(remote.share).as_str().unwrap_or_default(),
                    json!(remote.token_source).as_str().unwrap_or_default(),
                    if reachable { "yes" } else { "no" }
                )
                .map_err(|e| e.to_string())?;
                match (&principal, &failure) {
                    (Some(principal), _) => writeln!(
                        out,
                        "Principal: {} (scopes: {})",
                        principal.principal,
                        principal.scopes.join(", ")
                    ),
                    (_, Some(failure)) => writeln!(out, "Error: {}", failure.message),
                    _ => Ok(()),
                }
                .map_err(|e| e.to_string())?;
            }
            Ok(u8::from(principal.is_none()))
        }
    }
}

/// Bound stdin allocation before bearer validation, with room beyond the 4096-byte
/// token limit for surrounding whitespace and line endings; oversized tokens stay invalid.
const TOKEN_STDIN_BYTES: u64 = 8192;

fn read_token() -> Result<String, String> {
    let stdin = io::stdin().lock();
    let hidden = if stdin.is_terminal() {
        // Echo goes off before the prompt invites a paste.
        let hidden = match HiddenInput::new() {
            Ok(hidden) => Some(hidden),
            // mintty (Git Bash) hands Windows programs a pipe that only looks like a terminal,
            // with no console echo to turn off: read the token visibly rather than not at all.
            Err(error) if cfg!(windows) => {
                writeln!(
                    io::stderr().lock(),
                    "Warning: this terminal cannot hide input ({error}); the token will be visible."
                )
                .map_err(|e| e.to_string())?;
                None
            }
            Err(error) => return Err(error.to_string()),
        };
        write!(
            io::stderr().lock(),
            "Paste the remote token, then press Enter: "
        )
        .map_err(|e| e.to_string())?;
        hidden
    } else {
        None
    };
    let mut token = String::new();
    let read = stdin.take(TOKEN_STDIN_BYTES).read_line(&mut token);
    if hidden.is_some() {
        drop(hidden);
        // The Enter key was not echoed either.
        let _ = writeln!(io::stderr().lock());
    }
    read.map_err(|_| "Cannot read the remote token from stdin.".to_owned())?;
    Ok(token.trim().to_owned())
}
