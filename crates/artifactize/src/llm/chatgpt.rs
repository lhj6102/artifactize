use std::path::{Path, PathBuf};

use rig_core::{Model, providers::openai};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const API_BASE: &str = "https://api.openai.com/v1";

pub struct Chatgpt {
    model: String,
    state: PathBuf,
    repo: PathBuf,
    http: reqwest::Client,
    base_url: String,
}

impl Chatgpt {
    pub(super) fn new(model: &str, state: &Path, repo: &Path) -> Result<Self, String> {
        Ok(Self {
            model: model.into(),
            state: state.into(),
            repo: repo.into(),
            http: super::http_client()?,
            base_url: API_BASE.into(),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_base_url(mut self, base: String) -> Self {
        self.base_url = base;
        self
    }

    pub(super) async fn model(
        &self,
    ) -> Result<Model<openai::responses_api::wire::Responses>, String> {
        let token = crate::auth::chatgpt_access_token(Some(&self.state), Some(&self.repo)).await?;
        Ok(openai::OpenAIConfig::new(token)
            .with_base_url(&self.base_url)
            .with_system_instructions_placement(
                openai::responses_api::SystemInstructionsPlacement::AllInstructions,
            )
            .connect(rig_reqwest::ReqwestClient::from(self.http.clone()))
            .responses(&self.model))
    }
}

#[derive(Deserialize, Serialize)]
pub struct ListedModel {
    pub slug: String,
    pub display_name: String,
}

#[derive(Deserialize)]
struct Models {
    models: Vec<Value>,
}

pub async fn models(state: Option<&Path>, repo: Option<&Path>) -> Result<Vec<ListedModel>, String> {
    models_at(state, repo, API_BASE).await
}

async fn models_at(
    state: Option<&Path>,
    repo: Option<&Path>,
    base: &str,
) -> Result<Vec<ListedModel>, String> {
    let token = crate::auth::chatgpt_access_token(state, repo).await?;
    let response = super::http_client()?
        .get(format!("{base}/models"))
        .bearer_auth(token)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| "ChatGPT model listing failed; check your connection.".to_owned())?;
    let status = response.status();
    let body = response.json::<Value>().await;
    if !status.is_success() {
        return Err(diagnostic(
            Some(status.as_u16()),
            &body.unwrap_or(Value::Null),
            "ChatGPT model listing failed.",
        ));
    }
    let models: Models =
        serde_json::from_value(body.map_err(|_| "Invalid ChatGPT models response.".to_owned())?)
            .map_err(|_| "Invalid ChatGPT models response.".to_owned())?;
    models
        .models
        .into_iter()
        .filter(|model| model["visibility"] == "list")
        .map(|model| {
            serde_json::from_value(model).map_err(|_| "Invalid listed ChatGPT model.".to_owned())
        })
        .collect()
}

fn error_object(body: &Value) -> &Value {
    body.pointer("/response/error")
        .filter(|error| error.is_object())
        .or_else(|| body.get("error").filter(|error| error.is_object()))
        .unwrap_or(body)
}

pub(super) fn error_code(body: &Value) -> Option<&str> {
    error_object(body)["code"]
        .as_str()
        .filter(|code| !code.is_empty())
}

pub(super) fn diagnostic(status: Option<u16>, body: &Value, fallback: &str) -> String {
    let code = error_code(body);
    let message = error_object(body)["message"]
        .as_str()
        .or_else(|| body["detail"].as_str())
        .unwrap_or(fallback);
    let mut message = match code {
        Some(code) => format!("{code}: {message}"),
        None => message.to_owned(),
    };
    if let Some(status) = status {
        message.push_str(&format!(" (HTTP {status})"));
    }
    if status == Some(401)
        || matches!(
            code,
            Some(
                "subscription_sharing_invalid_user"
                    | "chatpass_v2_scope_not_authorized"
                    | "chatpass_v2_invalid_authorization_context"
                    | "invalid_api_key"
                    | "invalid_token"
            )
        )
    {
        message.push_str(" Check your ChatGPT account and grant; run `artifactize login chatgpt`.");
    } else if code == Some("subscription_sharing_usage_limit_exceeded") {
        message.push_str(" Check ChatGPT Settings > Usage before retrying.");
    }
    super::clean_diagnostic(&message)
}

#[cfg(test)]
pub(super) mod tests;
