//! The custom providers of this machine: names an Agent profile's `backend` may use.

use std::{
    io::{self, Write},
    path::Path,
};

use clap::{ArgGroup, Subcommand};
use serde_json::json;

use super::print_json;
use artifactize_core::{
    auth::providers::{self, Custom, CustomView, EndpointUrl, KeyChange},
    config::{Backend, Provider},
    llm::EndpointApi,
};

#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// Show the built-in providers' keys (never their values) and the custom providers.
    List,
    /// Add a custom provider, or replace the one with that name.
    #[command(group(ArgGroup::new("target").args(["alias", "api"]).required(true)))]
    Add {
        /// The name evals use in `backend`.
        name: Backend,
        /// Reach a built-in provider, with its sign-in or key, under this name.
        #[arg(long, value_enum, value_name = "PROVIDER", conflicts_with_all = ["url", "key"])]
        alias: Option<Provider>,
        /// The API the endpoint speaks.
        #[arg(long, value_enum, requires = "url")]
        api: Option<EndpointApi>,
        /// The endpoint's API root, such as https://api.openai.com/v1.
        #[arg(long, value_parser = parse_url)]
        url: Option<EndpointUrl>,
        /// Read the endpoint's key from stdin (never argv); without it the endpoint has no key.
        #[arg(long)]
        key: bool,
    },
    /// Remove a custom provider.
    Remove { name: Backend },
}

fn parse_url(value: &str) -> Result<EndpointUrl, String> {
    value.parse()
}

pub(super) async fn execute(
    state: Option<&Path>,
    repo: Option<&Path>,
    command: ProviderCommand,
    json: bool,
) -> Result<u8, String> {
    let mut out = io::stdout().lock();
    match command {
        ProviderCommand::List => {
            let list = providers::list(state, repo)?;
            if json {
                print_json(&list)?;
                return Ok(0);
            }
            for key in &list.keys {
                let source = match key.source {
                    providers::KeySource::Environment => format!("key from {}", key.variable),
                    providers::KeySource::Saved => "saved key".to_owned(),
                    providers::KeySource::Absent => "no key".to_owned(),
                };
                writeln!(out, "{}\t{source}", key.provider).map_err(|e| e.to_string())?;
            }
            for (name, custom) in &list.custom {
                let target = match custom {
                    CustomView::Alias { provider } => format!("alias of {provider}"),
                    CustomView::Endpoint { api, url, key } => format!(
                        "{} at {}{}",
                        json!(api).as_str().unwrap_or_default(),
                        url.as_str(),
                        if *key { ", saved key" } else { "" }
                    ),
                };
                writeln!(out, "{name}\t{target}").map_err(|e| e.to_string())?;
            }
        }
        ProviderCommand::Add {
            name,
            alias,
            api,
            url,
            key,
        } => {
            let (custom, change) = match (alias, api, url) {
                (Some(provider), _, _) => (Custom::Alias { provider }, KeyChange::None),
                (None, Some(api), Some(url)) => (
                    Custom::Endpoint {
                        api,
                        url,
                        key: None,
                    },
                    if key {
                        KeyChange::Set(super::secret::read("endpoint key")?.parse()?)
                    } else {
                        KeyChange::None
                    },
                ),
                _ => unreachable!("clap requires --alias, or --api with --url"),
            };
            providers::save_custom(state, repo, name.clone(), custom, change).await?;
            if json {
                print_json(&json!({ "provider": name, "saved": true }))?;
            } else {
                writeln!(out, "Saved custom provider {name}.").map_err(|e| e.to_string())?;
            }
        }
        ProviderCommand::Remove { name } => {
            providers::remove_custom(state, repo, &name).await?;
            if json {
                print_json(&json!({ "provider": name, "saved": false }))?;
            } else {
                writeln!(out, "Removed custom provider {name}.").map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(0)
}
