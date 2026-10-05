use std::time::Duration;

use rig_core::providers::{anthropic, openai};
use serde::Serialize;

use crate::config::Backend;

#[derive(Serialize)]
pub struct ListedModel {
    pub slug: String,
    pub display_name: String,
}

#[derive(Serialize)]
pub struct Listing {
    pub backend: Backend,
    pub models: Vec<ListedModel>,
}

pub async fn list(backend: Backend) -> Result<Listing, String> {
    let base = super::base_url(backend)?;
    let key = super::api_key(backend, "to list models")?;
    Ok(Listing {
        backend,
        models: list_at(backend, &key, &base).await?,
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
    use crate::llm::tests::Server;

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
        assert!(requests[0].body.is_null());

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
