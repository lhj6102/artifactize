use std::{path::Path, time::Duration};

use rig_core::providers::{anthropic, openai};
use serde::Serialize;

use crate::config::Backend;

pub use super::chatgpt::ListedModel;

#[derive(Serialize)]
pub struct Listing {
    pub backend: Backend,
    pub models: Vec<ListedModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'static str>,
}

pub async fn list(
    backend: Backend,
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Result<Listing, String> {
    let models = match backend {
        Backend::Chatgpt => super::chatgpt_models(state, repo).await?,
        Backend::Claude => {
            return Ok(Listing {
                backend,
                models: Vec::new(),
                note: Some(
                    "The Claude CLI selects models by name via --model; it has no model-listing API. Use an explicit model name or a CLI-supported alias.",
                ),
            });
        }
        Backend::Openai | Backend::Anthropic => {
            let (variable, base) = match backend {
                Backend::Openai => ("OPENAI_API_KEY", "https://api.openai.com/v1"),
                _ => ("ANTHROPIC_API_KEY", "https://api.anthropic.com"),
            };
            let key = std::env::var(variable)
                .ok()
                .filter(|key| !key.trim().is_empty())
                .ok_or_else(|| format!("{variable} is required to list models."))?;
            list_at(backend, &key, base).await?
        }
    };
    Ok(Listing {
        backend,
        models,
        note: None,
    })
}

async fn list_at(backend: Backend, key: &str, base: &str) -> Result<Vec<ListedModel>, String> {
    let http = rig_reqwest::ReqwestClient::from(super::http_client()?);
    let request = async {
        match backend {
            Backend::Openai => {
                openai::OpenAIConfig::new(key.to_owned())
                    .with_base_url(base)
                    .connect(http)
                    .list_models()
                    .await
            }
            Backend::Anthropic => {
                anthropic::AnthropicConfig::new(key.to_owned())
                    .with_base_url(base)
                    .connect(http)
                    .list_models()
                    .await
            }
            _ => unreachable!("API-key backend required"),
        }
    };
    let models = tokio::time::timeout(Duration::from_secs(30), request)
        .await
        .map_err(|_| "Model listing timed out.".to_owned())?
        .map_err(|error| super::diagnostic(&error).replace(key, "[redacted]"))?;
    Ok(models
        .into_iter()
        .map(|model| ListedModel {
            display_name: model.name.unwrap_or_else(|| model.id.clone()),
            slug: model.id,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use rig_core::test_utils::MockHttpResponse;
    use serde_json::json;

    use super::*;
    use crate::llm::Server;

    #[tokio::test]
    async fn api_key_models_use_provider_headers_and_follow_anthropic_pages() {
        let openai = Server::new(vec![MockHttpResponse::success(
            json!({"data":[
                {"id":"z-model"}, {"id":"a-model", "name":"A model"}
            ]})
            .to_string(),
        )])
        .await;
        let models = list_at(Backend::Openai, "openai-key", &openai.base)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(models).unwrap(),
            json!([
                {"slug":"z-model","display_name":"z-model"},
                {"slug":"a-model","display_name":"A model"}
            ])
        );
        let requests = openai.requests();
        assert_eq!(requests[0].line, "GET /v1/models HTTP/1.1");
        assert_eq!(requests[0].headers["authorization"], "Bearer openai-key");
        assert!(!requests[0].headers.contains_key("x-api-key"));

        let anthropic = Server::new(vec![
            MockHttpResponse::success(json!({"data":[{"id":"first", "display_name":"First"}], "has_more":true,"last_id":"first"}).to_string()),
            MockHttpResponse::success(json!({"data":[{"id":"second", "display_name":"Second"}],"has_more":false}).to_string()),
        ]).await;
        let models = list_at(Backend::Anthropic, "anthropic-key", &anthropic.base)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(models).unwrap(),
            json!([
                {"slug":"first","display_name":"First"},
                {"slug":"second","display_name":"Second"}
            ])
        );
        let requests = anthropic.requests();
        assert_eq!(requests[0].line, "GET /v1/models HTTP/1.1");
        assert_eq!(requests[1].line, "GET /v1/models?after_id=first HTTP/1.1");
        assert_eq!(requests[0].headers["x-api-key"], "anthropic-key");
        assert!(requests[0].headers.contains_key("anthropic-version"));
        assert!(!requests[0].headers.contains_key("authorization"));
    }

    #[tokio::test]
    async fn model_listing_errors_do_not_expose_api_keys() {
        for backend in [Backend::Openai, Backend::Anthropic] {
            let server = Server::new(vec![MockHttpResponse::error(
                rig_core::http_client::StatusCode::UNAUTHORIZED,
                json!({"error":{"message":"Invalid secret-key", "type":"authentication_error"}})
                    .to_string(),
            )])
            .await;
            let error = list_at(backend, "secret-key", &server.base)
                .await
                .err()
                .unwrap();
            assert!(error.contains("Invalid"), "{error}");
            assert!(!error.contains("secret-key"), "{error}");
        }
    }
}
