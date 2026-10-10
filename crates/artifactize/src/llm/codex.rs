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
//!
//! Every request of one review carries the review's session id, as `prompt_cache_key`
//! and as the `session-id` header the backend keys its prompt cache on. rig 0.43 sends a
//! fresh `session_id` per request instead (0xPlaygrounds/rig#2719), which artifactize
//! turns off.

use std::path::{Path, PathBuf};

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
mod errors;

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

fn envelope(_: &openai::OpenAIConfig, request: &CompletionRequest, builder: Builder) -> Builder {
    let builder = builder
        .header("OpenAI-Beta", "responses=experimental")
        .header("accept", "text/event-stream");
    // TODO: once rig ships `OpenAIConfig::with_session_id` (0xPlaygrounds/rig#2722), pin
    // the session there and drop this header and the `session_ids` override in `model`.
    match session(request) {
        Some(session) => builder.header("session-id", session),
        None => builder,
    }
}

/// The review's session id: the request's `prompt_cache_key` ([`super::Client::parameters`]).
fn session(request: &CompletionRequest) -> Option<&str> {
    request
        .additional_params
        .as_ref()?
        .get("prompt_cache_key")?
        .as_str()
}

/// `artifactize/<version> (<os> <arch>; artifactize)`, the shape rig gives gateways.
pub fn user_agent() -> String {
    format!(
        "artifactize/{} ({}; {ORIGINATOR})",
        env!("CARGO_PKG_VERSION"),
        crate::platform::label(),
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
        // No fresh `session_id` per request, which the backend's cache ignores; the
        // envelope sends the review's `session-id` instead.
        if let Some(identity) = &mut dialect.quirks.identity {
            identity.session_ids = false;
        }
        let mut config = openai::OpenAIConfig::with_key(&dialect, token.access_token)
            .with_base_url(&self.base)
            .with_account_id(token.account_id.to_string());
        config.instructions = None;
        config.identity = Some(CallerIdentity {
            originator: ORIGINATOR.into(),
            user_agent: user_agent(),
        });
        Ok(config
            .connect(super::delivery::Tap::new(self.http.clone()))
            .responses(&self.model))
    }
}

/// The known model catalog fields. Maps retain last duplicate values, while absent/null
/// or wrong-typed optional fields preserve the picker's existing skip/default behavior.
#[derive(Default)]
struct ModelCatalog {
    models: crate::json::Optional<Vec<crate::json::Optional<crate::json::Object<PickerModel>>>>,
}
#[derive(Default)]
struct PickerModel {
    visibility: crate::json::Optional<String>,
    slug: crate::json::Optional<String>,
    display_name: crate::json::Optional<String>,
}
impl<'de> serde::Deserialize<'de> for ModelCatalog {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CatalogVisitor;
        impl<'de> serde::de::Visitor<'de> for CatalogVisitor {
            type Value = ModelCatalog;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a model catalog object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut catalog = ModelCatalog::default();
                while let Some(field) = map.next_key::<String>()? {
                    if field == "models" {
                        catalog.models = map.next_value()?;
                    } else {
                        map.next_value::<crate::json::Ignored>()?;
                    }
                }
                Ok(catalog)
            }
        }
        deserializer.deserialize_map(CatalogVisitor)
    }
}
impl<'de> serde::Deserialize<'de> for PickerModel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ModelVisitor;
        impl<'de> serde::de::Visitor<'de> for ModelVisitor {
            type Value = PickerModel;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a picker model object")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut model = PickerModel::default();
                while let Some(field) = map.next_key::<String>()? {
                    match field.as_str() {
                        "visibility" => model.visibility = map.next_value()?,
                        "slug" => model.slug = map.next_value()?,
                        "display_name" => model.display_name = map.next_value()?,
                        _ => {
                            map.next_value::<crate::json::Ignored>()?;
                        }
                    }
                }
                Ok(model)
            }
        }
        deserializer.deserialize_map(ModelVisitor)
    }
}
fn picker_models(data: &[u8]) -> Result<Vec<super::models::ListedModel>, String> {
    let invalid = || "Invalid Codex models response.".to_owned();
    let crate::json::Object(catalog): crate::json::Object<ModelCatalog> =
        serde_json::from_slice(data).map_err(|_| invalid())?;
    catalog
        .models
        .0
        .ok_or_else(invalid)?
        .into_iter()
        .filter_map(|model| model.0.map(|crate::json::Object(model)| model))
        .filter(|model| model.visibility.0.as_deref() == Some("list"))
        .map(|model| {
            let slug = model
                .slug
                .0
                .filter(|slug| !slug.is_empty())
                .ok_or_else(invalid)?;
            Ok(super::models::ListedModel {
                display_name: model.display_name.0.unwrap_or_else(|| slug.clone()),
                slug,
            })
        })
        .collect()
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
        .header("ChatGPT-Account-Id", token.account_id.as_str())
        .header("originator", ORIGINATOR)
        .header(reqwest::header::USER_AGENT, user_agent())
        .timeout(super::models::LIST_TIMEOUT)
        .send()
        .await
        .map_err(|_| "Codex model listing failed; check your connection.".to_owned())?;
    let status = response.status();
    let data = response.bytes().await.unwrap_or_default();
    if !status.is_success() {
        // Provider error payloads vary by endpoint; the success catalog has a known shape.
        let body = errors::Error::parse(&data);
        return Err(describe(
            Some(status.as_u16()),
            body.as_ref(),
            "Codex model listing failed.",
            auth::now().ok(),
        ));
    }
    picker_models(&data)
}

/// A plan usage limit, which no retry within a review outlasts.
pub(super) fn usage_limited(error: &ProviderError) -> bool {
    body(error).is_some_and(|body| body.usage_limit(error.report().http_status))
}
fn body(error: &ProviderError) -> Option<errors::Error> {
    error
        .provider_response()
        .and_then(|response| errors::Error::parse(response.body.as_bytes()))
}

pub(super) fn diagnostic(error: &ProviderError) -> String {
    let body = body(error);
    if body.is_none() {
        return super::diagnostic(error);
    }
    describe(
        error.report().http_status,
        body.as_ref(),
        &super::diagnostic(error),
        auth::now().ok(),
    )
}

/// A bounded message with the provider's code, an HTTP status, and what to do next:
/// usage limits name the plan and when they reset, as Pi words them, and rejected
/// credentials say how to sign in again.
fn describe(
    status: Option<u16>,
    body: Option<&errors::Error>,
    fallback: &str,
    now: Option<u64>,
) -> String {
    let code = body.and_then(errors::Error::code);
    let mut message = if body.is_some_and(|body| body.usage_limit(status)) {
        let plan = body
            .and_then(|body| body.fields.plan_type.0.as_deref())
            .map(|plan| format!(" ({} plan)", plan.to_lowercase()))
            .unwrap_or_default();
        let reset = body
            .and_then(|body| body.fields.resets_at.0)
            .zip(now)
            .map(|(at, now)| {
                format!(
                    " Try again in ~{} min.",
                    at.saturating_sub(now).div_ceil(60)
                )
            })
            .unwrap_or_default();
        format!("You have hit your ChatGPT usage limit{plan}.{reset}")
    } else {
        body.and_then(|body| body.fields.message.0.as_deref().or(body.detail.as_deref()))
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
        message.push_str(
            " Sign in again with `artifactize login codex`, or with Codex when ARTIFACTIZE_CODEX_AUTH_FILE is set.",
        );
    }
    super::clean_diagnostic(&message)
}

#[cfg(test)]
mod tests;
