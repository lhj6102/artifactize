//! The `codex` backend: the Codex Responses endpoint with ChatGPT/Codex sign-in
//! credentials, the way Pi's `openai-codex` provider calls it
//! (pi-mono `packages/ai/src/api/openai-codex-responses.ts`).
//!
//! rig's ChatGPT dialect speaks the Codex request contract: an always-streamed
//! Responses turn with `store:false`, encrypted reasoning replay, function tools, all
//! system messages as top-level `instructions`, and an event stream that may name no
//! content type. artifactize adds the headers Pi sends (`OpenAI-Beta`, an event-stream
//! `accept`, its own `originator` and user agent, the ChatGPT account), and drops
//! rig's default instructions so the review's own system prompt is the only one.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rig_core::{
    Model,
    completion::CompletionRequest,
    error::ProviderError,
    http_client::Builder,
    providers::{
        chatgpt,
        openai::{
            self,
            wire::{CallerIdentity, DialectHooks},
        },
    },
};
use serde_json::Value;

use crate::auth::codex::{self as auth, ORIGINATOR};

/// The Codex API root; requests go to `<root>/responses` and `<root>/models`.
pub const BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

/// The Codex backend filters its model catalog by client version. artifactize asks
/// for the catalog of the Codex release whose Responses protocol it follows.
pub const CLIENT_VERSION: &str = "0.160.0";

static HOOKS: DialectHooks = DialectHooks {
    default_endpoint: None,
    model_route: None,
    completion_envelope: Some(envelope),
    modality_envelope: None,
};

fn envelope(_: &openai::OpenAIConfig, _: &CompletionRequest, builder: Builder) -> Builder {
    builder
        .header("OpenAI-Beta", "responses=experimental")
        .header("accept", "text/event-stream")
}

/// `artifactize/<version> (<os> <arch>; artifactize)`, the shape rig gives gateways.
pub fn user_agent() -> String {
    format!(
        "artifactize/{} ({} {}; {ORIGINATOR})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

pub struct Codex {
    model: String,
    base: String,
    state: PathBuf,
    repo: PathBuf,
    http: reqwest::Client,
}

impl Codex {
    pub(super) fn new(
        model: &str,
        base: String,
        state: &Path,
        repo: &Path,
    ) -> Result<Self, String> {
        Ok(Self {
            model: model.into(),
            base,
            state: state.into(),
            repo: repo.into(),
            http: super::http_client()?,
        })
    }

    /// The Responses model for one turn, with credentials read (and refreshed) now.
    /// Callers must never print or log the token it carries.
    pub(super) async fn model(
        &self,
    ) -> Result<Model<openai::responses_api::wire::Responses>, auth::TokenError> {
        let token = auth::access_token(Some(&self.state), Some(&self.repo)).await?;
        let mut dialect = chatgpt::DIALECT;
        dialect.quirks.hooks = Some(&HOOKS);
        let mut config = openai::OpenAIConfig::with_key(&dialect, token.access_token)
            .with_base_url(&self.base)
            .with_account_id(token.account_id);
        config.instructions = None;
        config.identity = Some(CallerIdentity {
            originator: ORIGINATOR.into(),
            user_agent: user_agent(),
        });
        Ok(config
            .connect(rig_reqwest::ReqwestClient::from(self.http.clone()))
            .responses(&self.model))
    }
}

/// The account's Codex models that the Codex picker lists (`visibility: "list"`), in
/// server order, from `GET <root>/models?client_version=…` as the Codex CLI asks.
pub(super) async fn models(
    base: &str,
    state: Option<&Path>,
    repo: Option<&Path>,
) -> Result<Vec<super::models::ListedModel>, String> {
    let token = auth::access_token(state, repo)
        .await
        .map_err(|error| error.message)?;
    let response = super::http_client()?
        .get(format!("{base}/models?client_version={CLIENT_VERSION}"))
        .bearer_auth(&token.access_token)
        .header("ChatGPT-Account-Id", &token.account_id)
        .header("originator", ORIGINATOR)
        .header(reqwest::header::USER_AGENT, user_agent())
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| "Codex model listing failed; check your connection.".to_owned())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(describe(
            Some(status.as_u16()),
            &body,
            "Codex model listing failed.",
        ));
    }
    let invalid = || "Invalid Codex models response.".to_owned();
    body["models"]
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .filter(|model| model["visibility"] == "list")
        .map(|model| {
            let slug = model["slug"].as_str().filter(|slug| !slug.is_empty());
            let slug = slug.ok_or_else(invalid)?;
            Ok(super::models::ListedModel {
                slug: slug.into(),
                display_name: model["display_name"].as_str().unwrap_or(slug).into(),
            })
        })
        .collect()
}

/// The error object of an HTTP error body or a `response.failed` event.
fn error_object(body: &Value) -> &Value {
    body.pointer("/response/error")
        .filter(|error| error.is_object())
        .or_else(|| body.get("error").filter(|error| error.is_object()))
        .unwrap_or(body)
}

fn error_code(body: &Value) -> Option<&str> {
    let error = error_object(body);
    error["code"]
        .as_str()
        .or_else(|| error["type"].as_str())
        .filter(|code| !code.is_empty())
}

/// A plan usage limit: a usage code, or a 429 that says when the limit resets. A plain
/// 429 is a rate limit.
fn usage_limit(status: Option<u16>, body: &Value) -> bool {
    error_code(body)
        .is_some_and(|code| matches!(code, "usage_limit_reached" | "usage_not_included"))
        || (status == Some(429) && error_object(body)["resets_at"].is_u64())
}

/// A plan usage limit, which no retry within a review outlasts.
pub(super) fn usage_limited(error: &ProviderError) -> bool {
    usage_limit(error.report().http_status, &body(error))
}

fn body(error: &ProviderError) -> Value {
    error
        .provider_response()
        .and_then(|response| serde_json::from_str(&response.body).ok())
        .unwrap_or(Value::Null)
}

pub(super) fn diagnostic(error: &ProviderError) -> String {
    let body = body(error);
    if body.is_null() {
        return super::diagnostic(error);
    }
    describe(error.report().http_status, &body, &super::diagnostic(error))
}

/// A bounded message with the provider's code, an HTTP status, and what to do next:
/// usage limits name the plan and when they reset, as Pi words them, and rejected
/// credentials say how to sign in again.
pub(super) fn describe(status: Option<u16>, body: &Value, fallback: &str) -> String {
    let error = error_object(body);
    let code = error_code(body);
    let mut message = if usage_limit(status, body) {
        let plan = error["plan_type"]
            .as_str()
            .map(|plan| format!(" ({} plan)", plan.to_lowercase()))
            .unwrap_or_default();
        let reset = error["resets_at"]
            .as_u64()
            .zip(auth::now().ok())
            .map(|(at, now)| {
                format!(
                    " Try again in ~{} min.",
                    at.saturating_sub(now).div_ceil(60)
                )
            })
            .unwrap_or_default();
        format!("You have hit your ChatGPT usage limit{plan}.{reset}")
    } else {
        error["message"]
            .as_str()
            .or_else(|| body["detail"].as_str())
            .unwrap_or(fallback)
            .to_owned()
    };
    if let Some(code) = code {
        message = format!("{code}: {message}");
    }
    if let Some(status) = status {
        message.push_str(&format!(" (HTTP {status})"));
    }
    if matches!(status, Some(401 | 403)) {
        message.push_str(" Sign in again with `artifactize login codex`, or with Codex when ARTIFACTIZE_CODEX_AUTH_FILE is set.");
    }
    super::clean_diagnostic(&message)
}

#[cfg(test)]
mod tests;
