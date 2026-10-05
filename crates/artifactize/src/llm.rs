//! Exact-model rig backends with explicit API-key credentials, and the loopback-only
//! test endpoint override for offline fake providers.

pub mod models;
#[cfg(test)]
pub(crate) mod tests;

use std::{
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures_util::StreamExt;
use rig_core::{
    Model,
    completion::{CompletionRequest, CompletionResponse, FinishReason},
    error::ProviderError,
    observe::{Action, AdapterContext, AdapterEvent, Observation, Subject, Witness},
    providers::{anthropic, openai},
    streaming::CompletionStream,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::config::Backend;

pub enum Client {
    Openai(Box<Model<openai::responses_api::wire::Responses>>),
    Anthropic(Box<Model<anthropic::wire::Messages>>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub turn: usize,
    pub attempt: usize,
    pub usage: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// Only counters cross this boundary; prompts, reasoning and raw responses stay transient.
#[derive(Default)]
struct ReportedUsage(Mutex<Map<String, Value>>);

impl Witness for ReportedUsage {
    fn observe(&self, observation: Observation) {
        if let Action::Adapter { observation } = observation.action
            && let AdapterEvent::Usage { usage } = observation.event
        {
            let mut counters = self.0.lock().unwrap();
            for (name, value) in [
                ("inputTokens", usage.input_tokens),
                ("outputTokens", usage.output_tokens),
                ("totalTokens", usage.total_tokens),
                ("cacheReadTokens", usage.cached_input_tokens),
                ("reasoningTokens", usage.reasoning_tokens),
            ] {
                insert_counter(&mut counters, name, value);
            }
        }
    }
}

fn insert_counter(counters: &mut Map<String, Value>, name: &str, value: Option<u64>) {
    if let Some(value) = value.filter(|value| *value <= 9_007_199_254_740_991) {
        counters.insert(name.into(), json!(value));
    }
}

impl Client {
    pub fn new(backend: Backend, model: &str) -> Result<Self, String> {
        let base = base_url(backend)?;
        let key = api_key(backend, "for this Agent backend")?;
        let http = rig_reqwest::ReqwestClient::from(http_client()?);
        Ok(match backend {
            Backend::Openai => Self::Openai(Box::new(
                openai::OpenAIConfig::new(key)
                    .with_base_url(base)
                    .connect(http)
                    .responses(model),
            )),
            Backend::Anthropic => Self::Anthropic(Box::new(
                anthropic::AnthropicConfig::new(key)
                    .with_base_url(base)
                    .connect(http)
                    .completion(model),
            )),
        })
    }

    pub fn parameters(backend: Backend, reasoning: Option<&str>) -> Result<Value, String> {
        if let Some(reasoning) = reasoning {
            backend.validate_reasoning(reasoning)?;
        }
        Ok(match backend {
            Backend::Openai => {
                let mut value = json!({"store":false, "parallel_tool_calls":false, "include":["reasoning.encrypted_content"]});
                if let Some(reasoning) = reasoning {
                    value["reasoning"] = json!({"effort":reasoning});
                }
                value
            }
            Backend::Anthropic => reasoning.map_or(
                json!({}),
                |effort| json!({"thinking":{"type":"adaptive"}, "output_config":{"effort":effort}}),
            ),
        })
    }

    async fn stream(
        &self,
        mut request: CompletionRequest,
        observed: AdapterContext,
    ) -> Result<CompletionStream, String> {
        match self {
            Self::Openai(model) => model.stream_observed(request, observed),
            Self::Anthropic(model) => {
                // Messages requires a per-turn output cap, unrelated to the review's maxTokens budget.
                request.max_tokens = Some(16_384);
                model.stream_observed(request, observed)
            }
        }
        .map_err(|error| diagnostic(&error))
    }

    pub async fn turn(
        &self,
        request: &CompletionRequest,
        context: Turn<'_>,
        attempts: &mut Vec<Attempt>,
    ) -> Result<CompletionResponse, String> {
        let mut emitted = context.prior_output;
        for attempt in 1..=3 {
            if context.cancellation.is_cancelled() {
                return Err("Agent review was cancelled.".into());
            }
            if Instant::now() >= context.deadline {
                return Err("Agent review timed out.".into());
            }
            let usage = Arc::new(ReportedUsage::default());
            let observed = AdapterContext::new(
                usage.clone(),
                Subject::default(),
                format!("turn-{}", context.number),
            );
            let mut stream = tokio::select! {
                biased;
                _ = context.cancellation.cancelled() => return Err("Agent review was cancelled.".into()),
                _ = tokio::time::sleep_until(context.deadline) => return Err("Agent review timed out.".into()),
                result = self.stream(request.clone(), observed) => result?,
            };
            let result = tokio::select! {
                biased;
                _ = context.cancellation.cancelled() => Err("Agent review was cancelled.".to_owned()),
                _ = tokio::time::sleep_until(context.deadline) => Err("Agent review timed out.".to_owned()),
                result = async {
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(_) => emitted = true,
                            Err(error) => return Err(error),
                        }
                    }
                    Ok(())
                } => Ok(result),
            };
            let partial = stream.partial();
            let mut counters = usage.0.lock().unwrap().clone();
            add_cache_usage(&mut counters, &partial);
            let result = match result {
                Ok(Ok(())) => stream
                    .finish()
                    .await
                    .map_err(|error| (diagnostic(&error), retryable(&error))),
                Ok(Err(error)) => Err((diagnostic(&error), retryable(&error))),
                Err(error) => Err((error, false)),
            };
            if let Ok(response) = &result {
                add_cache_usage(&mut counters, response);
            }
            let used_tokens = counters.values().any(|v| v.as_u64().is_some_and(|v| v > 0));
            attempts.push(Attempt {
                turn: context.number,
                attempt,
                usage: counters,
                error: result.as_ref().err().map(|(error, _)| error.clone()),
            });
            match result {
                Ok(response) => return Ok(response),
                Err((_error, transient))
                    if transient && !emitted && !used_tokens && attempt < 3 =>
                {
                    tokio::select! {
                        _ = context.cancellation.cancelled() => return Err("Agent review was cancelled.".into()),
                        _ = tokio::time::sleep_until(context.deadline) => return Err("Agent review timed out.".into()),
                        _ = tokio::time::sleep(Duration::from_millis(250 << (attempt - 1))) => {},
                    }
                }
                Err((error, _)) => return Err(error),
            }
        }
        unreachable!()
    }
}

pub struct Turn<'a> {
    pub number: usize,
    pub prior_output: bool,
    pub deadline: Instant,
    pub cancellation: &'a CancellationToken,
}

fn add_cache_usage(counters: &mut Map<String, Value>, response: &CompletionResponse) {
    insert_counter(
        counters,
        "cacheWriteTokens",
        response.usage.cache_creation_input_tokens,
    );
    insert_counter(
        counters,
        "cacheWrite1hTokens",
        response
            .raw
            .pointer("/usage/cache_creation/ephemeral_1h_input_tokens")
            .and_then(Value::as_u64),
    );
}

pub fn validate_response(response: &CompletionResponse, model: &str) -> Result<(), String> {
    if response
        .model
        .as_deref()
        .is_some_and(|actual| actual != model)
    {
        return Err(format!(
            "Provider model mismatch: requested {model:?}, received {:?}.",
            response.model
        ));
    }
    if !matches!(
        response.finish_reason(),
        Some(FinishReason::Stop | FinishReason::ToolCalls)
    ) {
        return Err(format!(
            "Provider response was incomplete: {:?}.",
            response.finish_reason()
        ));
    }
    Ok(())
}

fn retryable(error: &ProviderError) -> bool {
    let report = error.report();
    let text = format!(
        "{} {}",
        report.code.as_deref().unwrap_or_default(),
        diagnostic(error)
    )
    .to_lowercase();
    ![
        "quota",
        "billing",
        "credit",
        "authentication",
        "unauthorized",
        "permission",
        "usage limit",
    ]
    .iter()
    .any(|term| text.contains(term))
        && (error.is_retryable()
            || (report.http_status.is_none()
                && matches!(
                    report.code.as_deref(),
                    Some("overloaded_error" | "rate_limit_error" | "server_error")
                )))
}

fn diagnostic(error: &ProviderError) -> String {
    let message = error
        .provider_response()
        .and_then(|response| serde_json::from_str::<Value>(&response.body).ok())
        .and_then(|body| {
            body.pointer("/error/message")
                .or_else(|| body.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| error.to_string());
    clean_diagnostic(&message)
}

/// The environment variable that holds the backend's API key.
pub fn key_variable(backend: Backend) -> &'static str {
    match backend {
        Backend::Openai => "OPENAI_API_KEY",
        Backend::Anthropic => "ANTHROPIC_API_KEY",
    }
}

/// The environment variable that points the backend at a loopback test endpoint.
pub fn base_url_variable(backend: Backend) -> &'static str {
    match backend {
        Backend::Openai => "ARTIFACTIZE_OPENAI_BASE_URL",
        Backend::Anthropic => "ARTIFACTIZE_ANTHROPIC_BASE_URL",
    }
}

const BACKENDS: [Backend; 2] = [Backend::Openai, Backend::Anthropic];

/// The loopback test endpoint that replaces the backend's API root, if one is set.
/// An empty value is unset; any other value must be a loopback URL.
pub fn test_endpoint(backend: Backend) -> Result<Option<String>, String> {
    let variable = base_url_variable(backend);
    let Some(value) = std::env::var_os(variable).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    value
        .to_str()
        .ok_or_else(|| "it is not UTF-8".to_owned())
        .and_then(loopback_url)
        .map(Some)
        .map_err(|error| format!("{variable}: {error}."))
}

/// The first test endpoint variable that is set, valid or not.
pub fn active_test_endpoint() -> Option<&'static str> {
    BACKENDS
        .into_iter()
        .map(base_url_variable)
        .find(|variable| std::env::var_os(variable).is_some_and(|value| !value.is_empty()))
}

/// An http(s) URL on localhost, 127.0.0.0/8 or [::1], without credentials, a query or a
/// fragment, and without a trailing slash. A test endpoint never leaves the machine.
fn loopback_url(value: &str) -> Result<String, String> {
    let url = url::Url::parse(value).map_err(|_| "it is not a URL".to_owned())?;
    let loopback = match url.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        None => false,
    };
    if !matches!(url.scheme(), "http" | "https") || !loopback {
        return Err("a test endpoint must be http(s) on localhost, 127.0.0.0/8 or [::1]".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("a test endpoint has no credentials, query or fragment".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

/// The backend's API root: its loopback test endpoint, else the provider's.
fn base_url(backend: Backend) -> Result<String, String> {
    Ok(test_endpoint(backend)?.unwrap_or_else(|| {
        match backend {
            Backend::Openai => "https://api.openai.com/v1",
            Backend::Anthropic => "https://api.anthropic.com",
        }
        .into()
    }))
}

/// The backend's API key from its environment variable; never printed or logged.
fn api_key(backend: Backend, purpose: &str) -> Result<String, String> {
    let variable = key_variable(backend);
    std::env::var(variable)
        .ok()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| format!("{variable} is required {purpose}."))
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())
}

fn clean_diagnostic(message: &str) -> String {
    String::from_utf8(crate::runtime::clean_output(message.as_bytes()))
        .expect("clean output is UTF-8")
        .chars()
        .take(4096)
        .collect()
}
