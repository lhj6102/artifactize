use std::{
    io::{self, BufRead, IsTerminal, Read, Write},
    path::Path,
};

use clap::Subcommand;
use serde_json::json;

use super::print_json;
use crate::auth::remote::{self, Share};

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
                print_json(
                    &json!({"url": remote.url, "share": share, "principal": principal.principal, "scopes": principal.scopes}),
                )?;
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
                writeln!(out, "Signed out of the remote review store. The server keeps the token valid until `artifactize server token revoke`.")
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

/// Turns terminal echo off until dropped, so a pasted token is not shown.
struct HiddenInput(libc::termios);

impl HiddenInput {
    fn new() -> Result<Self, String> {
        let mut termios = std::mem::MaybeUninit::uninit();
        // SAFETY: tcgetattr fills the struct on success; it is read only then.
        let saved = unsafe {
            if libc::tcgetattr(libc::STDIN_FILENO, termios.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            termios.assume_init()
        };
        let mut hidden = saved;
        hidden.c_lflag &= !libc::ECHO;
        // SAFETY: a valid termios for the same descriptor.
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &hidden) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(Self(saved))
    }
}

impl Drop for HiddenInput {
    fn drop(&mut self) {
        // SAFETY: restores the attributes read in `new`.
        unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0) };
    }
}

fn read_token() -> Result<String, String> {
    let stdin = io::stdin().lock();
    let hidden = if stdin.is_terminal() {
        // Echo goes off before the prompt invites a paste.
        let hidden = HiddenInput::new()?;
        write!(
            io::stderr().lock(),
            "Paste the remote token, then press Enter: "
        )
        .map_err(|e| e.to_string())?;
        Some(hidden)
    } else {
        None
    };
    let mut token = String::new();
    let read = stdin.take(8192).read_line(&mut token);
    if hidden.is_some() {
        drop(hidden);
        // The Enter key was not echoed either.
        let _ = writeln!(io::stderr().lock());
    }
    read.map_err(|_| "Cannot read the remote token from stdin.".to_owned())?;
    Ok(token.trim().to_owned())
}
