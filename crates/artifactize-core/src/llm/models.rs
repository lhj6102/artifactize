use std::{path::Path, time::Duration};

use rig_core::providers::{anthropic, gemini, openai};
use serde::{Deserialize, Serialize};

use super::Api;
use crate::config::Backend;

/// Model discovery is interactive metadata, not a review; stop a stalled listing
/// after thirty seconds for both API-key backends and the Codex account endpoint.
pub(super) const LIST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedModel {
    pub slug: crate::config::ModelId,
    pub display_name: String,
    #[serde(default)]
    pub efforts: Vec<String>,
    #[serde(default)]
    pub default_effort: Option<String>,
}

impl ListedModel {
    pub(crate) fn new(slug: crate::config::ModelId, display_name: String) -> Self {
        Self {
            slug,
            display_name,
            efforts: Vec::new(),
            default_effort: None,
        }
    }

    pub(crate) fn with_efforts(
        mut self,
        efforts: impl IntoIterator<Item = String>,
        default: Option<String>,
    ) -> Self {
        for effort in efforts {
            if !effort.is_empty() && !self.efforts.contains(&effort) {
                self.efforts.push(effort);
            }
        }
        // The picker and new sessions share one order: a provider's declared default
        // first, otherwise the first advertised level is the starting choice.
        if let Some(index) =
            default.and_then(|default| self.efforts.iter().position(|effort| *effort == default))
        {
            let default = self.efforts.remove(index);
            self.efforts.insert(0, default);
        }
        self.default_effort = self.efforts.first().cloned();
        self
    }
}

#[derive(Serialize)]
pub struct Listing {
    pub backend: Backend,
    pub models: Vec<ListedModel>,
}

/// The result a model menu renders. Absence of setup is not a failed listing.
pub enum Availability {
    Models(Vec<ListedModel>),
    NotSetUp {
        reason: String,
    },
    Failed {
        error: String,
    },
    CliMissing {
        reason: String,
    },
    CliTooOld {
        version: semver::Version,
        required_version: semver::Version,
        error: String,
    },
}

impl Availability {
    fn listed(listing: Result<Vec<ListedModel>, String>) -> Self {
        match listing {
            Ok(models) => Self::Models(models),
            Err(error) => Self::Failed { error },
        }
    }
}

/// Discover models and classify setup failures at the native provider boundary.
pub async fn availability(
    backend: Backend,
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Availability {
    use super::Route;
    use crate::auth::{claude_agent, codex};
    use Availability::*;
    if backend.as_str() == "antigravity" {
        let id: crate::acp::AgentId = "antigravity".parse().expect("antigravity agent id");
        return match crate::acp::catalog::probe_agent(&id, state, repo, false).await {
            crate::acp::catalog::AgentAvailability::Ready { models, .. } => Models(models),
            crate::acp::catalog::AgentAvailability::Missing { reason } => CliMissing { reason },
            crate::acp::catalog::AgentAvailability::NotSetUp { reason, .. } => NotSetUp { reason },
            crate::acp::catalog::AgentAvailability::TooOld {
                version,
                required_version,
                reason,
                ..
            } => CliTooOld {
                version: version
                    .parse()
                    .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
                required_version: required_version
                    .parse()
                    .unwrap_or(crate::acp::catalog::ANTIGRAVITY_MINIMUM),
                error: reason,
            },
            crate::acp::catalog::AgentAvailability::Failed { error, .. } => Failed { error },
        };
    }
    let route = match super::route(&backend, state, repo, "to list models") {
        Ok(route) => route,
        Err(failure) if failure.code == crate::agent::error::Code::Authentication => {
            return NotSetUp {
                reason: failure.message,
            };
        }
        Err(failure) => {
            return Failed {
                error: failure.message,
            };
        }
    };
    match route {
        Route::ClaudeAgent => {
            let minimum = super::claude_agent::MINIMUM_VERSION;
            match claude_agent::cli_version().await {
                Ok(None) => {
                    return CliMissing {
                        reason: "Install Claude Code (claude).".into(),
                    };
                }
                Ok(Some(version)) if version < minimum => {
                    return CliTooOld {
                        error: format!("Update Claude Code to {minimum} or newer."),
                        version,
                        required_version: minimum,
                    };
                }
                Err(error) => return Failed { error },
                Ok(Some(_)) => {}
            }
            match claude_agent::status().await {
                claude_agent::Status::Missing => CliMissing {
                    reason: "Install Claude Code (claude).".into(),
                },
                claude_agent::Status::SignedOut => NotSetUp {
                    reason: "Sign in to Claude Code.".into(),
                },
                claude_agent::Status::Failed { error } => Failed { error },
                claude_agent::Status::SignedIn { .. } => {
                    Availability::listed(super::claude_agent::models().await)
                }
            }
        }
        Route::Http(super::HttpRoute {
            api: Api::Codex,
            base,
            ..
        }) => match codex::status(state, repo) {
            Ok(codex::Status::Absent) => NotSetUp {
                reason: "Sign in to Codex.".into(),
            },
            Ok(codex::Status::Refused { reason }) => Failed { error: reason },
            Err(error) => Failed { error },
            Ok(_) => Availability::listed(super::codex::models(&base, state, repo).await),
        },
        Route::Http(super::HttpRoute { api, base, key }) => {
            let key = key.as_ref().map_or("", |key| key.expose());
            Availability::listed(list_at(api, key, super::adapter_base(&base)).await)
        }
    }
}

/// `state` holds saved keys, custom providers and Codex credentials; `repo` keeps them
/// outside the reviewed repository.
pub async fn list(
    backend: Backend,
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Result<Listing, String> {
    if backend.as_str() == "antigravity" {
        let models = crate::auth::antigravity::models().await?;
        return Ok(Listing { backend, models });
    }
    let route =
        super::route(&backend, state, repo, "to list models").map_err(|failure| failure.message)?;
    let models = match route {
        super::Route::ClaudeAgent => super::claude_agent::models().await?,
        super::Route::Http(super::HttpRoute {
            api: Api::Codex,
            base,
            ..
        }) => super::codex::models(&base, state, repo).await?,
        super::Route::Http(super::HttpRoute { api, base, key }) => {
            let key = key.as_ref().map_or("", |key| key.expose());
            list_at(api, key, super::adapter_base(&base)).await?
        }
    };
    Ok(Listing { backend, models })
}

async fn list_at(api: Api, key: &str, base: &str) -> Result<Vec<ListedModel>, String> {
    let http = rig_reqwest::ReqwestClient::from(super::http_client()?);
    let request = async {
        match api {
            Api::OpenaiResponses | Api::OpenaiChat => {
                openai::OpenAIConfig::new(key.to_owned())
                    .with_base_url(base)
                    .connect(http)
                    .list_models()
                    .await
            }
            Api::Anthropic => {
                anthropic::AnthropicConfig::new(key.to_owned())
                    .with_base_url(base)
                    .connect(http)
                    .list_models()
                    .await
            }
            Api::Gemini => {
                gemini::GeminiConfig::new(key.to_owned())
                    .with_base_url(base)
                    .connect(http)
                    .list_models()
                    .await
            }
            Api::Codex | Api::ClaudeAgent => {
                unreachable!("they list their own models")
            }
        }
    };
    let models = tokio::time::timeout(LIST_TIMEOUT, request)
        .await
        .map_err(|_| "Model listing timed out.".to_owned())?
        .map_err(|error| {
            let sent: Vec<&str> = Some(key)
                .filter(|key| !key.is_empty())
                .into_iter()
                .collect();
            let text = super::diagnostic_hiding(&error, &sent);
            if text == crate::secret::WITHHELD {
                format!("Model listing failed: {text}")
            } else {
                text
            }
        })?;
    // A listed model without a usable id cannot be selected, so it is left out.
    Ok(models
        .into_iter()
        .filter_map(|model| {
            let slug = model.id.parse().ok()?;
            let (efforts, default) = super::api::model_efforts(api, &model.id);
            Some(
                ListedModel::new(slug, model.name.unwrap_or(model.id)).with_efforts(
                    efforts.iter().map(|effort| (*effort).to_owned()),
                    default.map(str::to_owned),
                ),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use rig_core::test_utils::MockHttpResponse;
    use serde_json::json;

    use super::*;
    use crate::llm::tests::Server;

    #[tokio::test]
    async fn api_key_models_use_provider_headers_and_follow_anthropic_pages() {
        Server::run(
            vec![MockHttpResponse::success(
                json!({"data":[
                    {"id":"z-model"}, {"id":"a-model", "name":"A model"}
                ]})
                .to_string(),
            )],
            async |openai| {
                let models = list_at(Api::OpenaiResponses, "openai-key", &openai.base)
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(models).unwrap(),
                    json!([
                        {"slug":"z-model","display_name":"z-model","efforts":[],"default_effort":null},
                        {"slug":"a-model","display_name":"A model","efforts":[],"default_effort":null}
                    ])
                );
                let requests = openai.requests();
                assert_eq!(requests[0].line, "GET /v1/models HTTP/1.1");
                assert_eq!(requests[0].headers["authorization"], "Bearer openai-key");
                assert!(!requests[0].headers.contains_key("x-api-key"));
                assert!(requests[0].body.is_null());
            },
        )
        .await;

        Server::run(
            vec![
                MockHttpResponse::success(
                    json!({
                        "data":[{"id":"first", "display_name":"First","efforts":[],"default_effort":null}],
                        "has_more":true,
                        "last_id":"first",
                    })
                    .to_string(),
                ),
                MockHttpResponse::success(
                    json!({"data":[{"id":"second", "display_name":"Second","efforts":[],"default_effort":null}],"has_more":false})
                        .to_string(),
                ),
            ],
            async |anthropic| {
                let models = list_at(Api::Anthropic, "anthropic-key", &anthropic.base)
                    .await
                    .unwrap();
                assert_eq!(
                    serde_json::to_value(models).unwrap(),
                    json!([
                        {"slug":"first","display_name":"First","efforts":[],"default_effort":null},
                        {"slug":"second","display_name":"Second","efforts":[],"default_effort":null}
                    ])
                );
                let requests = anthropic.requests();
                assert_eq!(requests[0].line, "GET /v1/models HTTP/1.1");
                assert_eq!(requests[1].line, "GET /v1/models?after_id=first HTTP/1.1");
                assert_eq!(requests[0].headers["x-api-key"], "anthropic-key");
                assert!(requests[0].headers.contains_key("anthropic-version"));
                assert!(!requests[0].headers.contains_key("authorization"));
            },
        )
        .await;
    }

    #[tokio::test]
    async fn model_listing_errors_do_not_expose_api_keys() {
        for backend in [Api::OpenaiResponses, Api::Anthropic] {
            Server::run(
                vec![MockHttpResponse::error(
                rig_core::http_client::StatusCode::UNAUTHORIZED,
                json!({"error":{"message":"Invalid secret-key", "type":"authentication_error"}})
                    .to_string(),
            )],
                async |server| {
                    let error = list_at(backend, "secret-key", &server.base)
                        .await
                        .err()
                        .unwrap();
                    // The provider's text held the key: withheld whole, as every server text is.
                    assert_eq!(
                        error,
                        format!("Model listing failed: {}", crate::secret::WITHHELD)
                    );
                },
            )
            .await;
        }
        // Text without the key still explains the failure.
        Server::run(
            vec![MockHttpResponse::error(
                rig_core::http_client::StatusCode::UNAUTHORIZED,
                json!({"error":{"message":"Invalid API key\n", "type":"authentication_error"}})
                    .to_string(),
            )],
            async |server| {
                let error = list_at(Api::OpenaiResponses, "secret-key-value", &server.base)
                    .await
                    .err()
                    .unwrap();
                assert_eq!(error, "Invalid API key");
            },
        )
        .await;
    }
}
