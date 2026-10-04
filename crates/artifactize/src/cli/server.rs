use std::{
    io::{self, Write},
    net::SocketAddr,
    path::Path,
};

use clap::Subcommand;
use serde_json::json;

use super::{cancellation_listener, print_json};
use crate::server::{self, Scope, Store};

#[derive(Debug, Subcommand)]
pub enum ServerCommand {
    /// Serve the review store over plain HTTP; put a TLS proxy or tunnel in front.
    Run {
        /// Listen address (loopback by default).
        #[arg(long, value_name = "ADDR", default_value = server::DEFAULT_LISTEN)]
        listen: SocketAddr,
    },
    /// Create, list or revoke bearer tokens.
    Token {
        #[command(subcommand)]
        command: TokenCommand,
    },
    /// Remove stored entries for a fingerprint.
    Rm {
        fingerprint: String,
        /// Required when the fingerprint has multiple stored Eval definitions.
        eval_hash: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// Create a token and print it once; only its SHA-256 is stored.
    Add {
        name: String,
        /// Comma-separated scopes: read, publish, human.
        #[arg(
            long,
            value_name = "SCOPES",
            value_enum,
            value_delimiter = ',',
            required = true
        )]
        scopes: Vec<Scope>,
    },
    /// List token names, scopes and revocation times, never the tokens.
    List,
    /// Revoke a token at once; --purge also deletes the entries it published.
    Revoke {
        name: String,
        #[arg(long)]
        purge: bool,
    },
}

pub(super) async fn execute(
    state: &Path,
    command: ServerCommand,
    json: bool,
) -> Result<u8, String> {
    let store = Store::open(state).await?;
    match command {
        ServerCommand::Run { listen } => {
            if !listen.ip().is_loopback() {
                writeln!(io::stderr().lock(), "Serving plain HTTP on {listen}; clients require HTTPS, so put a TLS proxy or tunnel in front.")
                    .map_err(|e| e.to_string())?;
            }
            let (cancellation, listener) = cancellation_listener()?;
            let result = server::serve(
                store,
                listen,
                |address| {
                    if json {
                        print_json(&json!({"listen": address}))
                    } else {
                        writeln!(io::stdout().lock(), "Listening on http://{address}/")
                            .map_err(|e| e.to_string())
                    }
                },
                cancellation,
            )
            .await;
            listener.abort();
            result?;
        }
        ServerCommand::Token {
            command: TokenCommand::Add { name, scopes },
        } => {
            let token = store.add_token(&name, &scopes).await?;
            if json {
                print_json(&json!({"name": name, "scopes": scopes, "token": token}))?;
            } else {
                writeln!(
                    io::stdout().lock(),
                    "{token}\nStore this token now; it is not shown again."
                )
                .map_err(|e| e.to_string())?;
            }
        }
        ServerCommand::Token {
            command: TokenCommand::List,
        } => {
            let tokens = store.tokens().await?;
            if json {
                print_json(&tokens)?;
            } else {
                let mut out = io::stdout().lock();
                writeln!(out, "NAME\tSCOPES\tCREATED\tREVOKED").map_err(|e| e.to_string())?;
                for token in tokens {
                    let scopes: Vec<_> = token.scopes.iter().map(|scope| scope.name()).collect();
                    writeln!(
                        out,
                        "{}\t{}\t{}\t{}",
                        token.name,
                        scopes.join(","),
                        token.created_at,
                        token.revoked_at.as_deref().unwrap_or("-")
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
        }
        ServerCommand::Token {
            command: TokenCommand::Revoke { name, purge },
        } => print_json(&store.revoke(&name, purge).await?)?,
        ServerCommand::Rm {
            fingerprint,
            eval_hash,
        } => print_json(
            &json!({"removed": store.remove(&fingerprint, eval_hash.as_deref()).await?}),
        )?,
    }
    Ok(0)
}
