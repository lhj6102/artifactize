use std::{
    ffi::OsStr,
    io::{self, Write},
    path::Path,
};

use clap::{Subcommand, builder::TypedValueParser, error::ErrorKind};
use serde_json::json;
use url::Url;

use super::print_json;
use artifactize_core::auth::remote::{self, Share, TokenSource};

#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
    /// Verify a token read from stdin (never argv) and save it with remote.json.
    Login {
        /// Store URL: https://, or http:// only on loopback.
        #[arg(value_parser = LoginUrl)]
        url: Url,
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

/// Parse the remote's URL without letting clap echo a rejected URL that may contain credentials.
#[derive(Clone)]
struct LoginUrl;

impl TypedValueParser for LoginUrl {
    type Value = Url;

    fn parse_ref(
        &self,
        command: &clap::Command,
        _argument: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        value
            .to_str()
            .ok_or_else(|| "Invalid remote URL.".to_owned())
            .and_then(remote::parse_url)
            .map_err(|message| {
                clap::Error::raw(ErrorKind::ValueValidation, message).format(&mut command.clone())
            })
    }
}

/// Limit bearer credentials as the remote login API does, without logging their values.
const MAX_TOKEN_BYTES: usize = 4096;

struct BearerToken(String);

impl TryFrom<String> for BearerToken {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if (1..=MAX_TOKEN_BYTES).contains(&value.len())
            && value.bytes().all(|byte| byte.is_ascii_graphic())
        {
            Ok(Self(value))
        } else {
            Err(format!(
                "A remote token is 1–{MAX_TOKEN_BYTES} printable ASCII characters without spaces."
            ))
        }
    }
}

async fn login(
    state: Option<&Path>,
    repo: Option<&Path>,
    url: Url,
    share: Share,
    token: BearerToken,
) -> Result<(remote::Remote, remote::Principal), String> {
    // The core API still takes an owned string and also validates tokens from non-CLI callers.
    remote::login(state, repo, url, share, token.0).await
}

fn share_text(share: Share) -> &'static str {
    match share {
        Share::Summary => "summary",
        Share::Full => "full",
    }
}

fn token_source_text(source: TokenSource) -> &'static str {
    match source {
        TokenSource::Env => "env",
        TokenSource::File => "file",
        TokenSource::None => "none",
    }
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
            let token = super::secret::read("remote token")?.try_into()?;
            let (remote, principal) = login(state, repo, url, share, token).await?;
            if json {
                print_json(&json!({
                    "url": remote.url(),
                    "share": share,
                    "principal": principal.principal,
                    "scopes": principal.scopes,
                }))?;
            } else {
                writeln!(
                    out,
                    "Signed in to {} as {} (scopes: {}).",
                    remote.url(),
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
                    share_text(status.share),
                    token_source_text(status.token_source),
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
