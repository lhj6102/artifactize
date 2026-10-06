//! Exact-model rig backends with explicit API-key or Codex credentials, and the
//! loopback-only test endpoint override for offline fake providers.

pub mod codex;
pub mod models;
#[cfg(test)]
pub(crate) mod tests;

use std::{
    net::IpAddr,
    path::Path,
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

use crate::{
    agent::error::{Code, Failure},
    config::Backend,
};

/// Attempts per turn: the first and up to two retries.
const ATTEMPTS: usize = 3;

pub enum Client {
    Openai(Box<Model<openai::responses_api::wire::Responses>>),
    Anthropic(Box<Model<anthropic::wire::Messages>>),
    Codex(codex::Codex),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub turn: usize,
    pub attempt: usize,
    pub usage: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
}

/// A classified provider failure, with the wait the provider asked for, if any.
struct Classified {
    code: Code,
    message: String,
    retry_after: Option<Duration>,
}

impl From<Classified> for Failure {
    fn from(classified: Classified) -> Self {
        Self::new(classified.code, classified.message)
    }
}

impl From<Failure> for Classified {
    fn from(failure: Failure) -> Self {
        Self {
            code: failure.code,
            message: failure.message,
            retry_after: None,
        }
    }
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
    /// `state` and `repo` locate Codex credentials; the API-key backends ignore them.
    pub fn new(backend: Backend, model: &str, state: &Path, repo: &Path) -> Result<Self, Failure> {
        let base = base_url(backend)?;
        if backend == Backend::Codex {
            return Ok(Self::Codex(codex::Codex::new(model, base, state, repo)?));
        }
        let key = api_key(backend, "for this Agent backend")
            .map_err(|message| Failure::new(Code::Authentication, message))?;
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
            Backend::Codex => unreachable!("built above"),
        })
    }

    /// The provider parameters of every request of one review. The Responses backends key
    /// their prompt cache on the review's `session` id; Anthropic caches by prefix alone.
    pub fn parameters(
        backend: Backend,
        reasoning: Option<&str>,
        session: &str,
    ) -> Result<Value, String> {
        if let Some(reasoning) = reasoning {
            backend.validate_reasoning(reasoning)?;
        }
        Ok(match backend {
            Backend::Openai => {
                let mut value = json!({"store":false, "parallel_tool_calls":false, "include":["reasoning.encrypted_content"], "prompt_cache_key":session});
                if let Some(reasoning) = reasoning {
                    value["reasoning"] = json!({"effort":reasoning});
                }
                value
            }
            Backend::Anthropic => reasoning.map_or(
                json!({}),
                |effort| json!({"thinking":{"type":"adaptive"}, "output_config":{"effort":effort}}),
            ),
            // rig's Codex contract states store:false and encrypted reasoning itself. The
            // `session-id` header repeats the cache key (see `codex::envelope`).
            Backend::Codex => {
                let mut value = json!({"prompt_cache_key":session});
                if let Some(effort) = reasoning {
                    value["reasoning"] = json!({"effort":effort, "summary":"auto"});
                }
                value
            }
        })
    }

    async fn stream(
        &self,
        mut request: CompletionRequest,
        observed: AdapterContext,
    ) -> Result<CompletionStream, Classified> {
        match self {
            Self::Openai(model) => model.stream_observed(request, observed),
            Self::Anthropic(model) => {
                // Messages requires a per-turn output cap, unrelated to the review's maxTokens budget.
                request.max_tokens = Some(16_384);
                model.stream_observed(request, observed)
            }
            // Each turn reads the credentials afresh, refreshing them when due.
            Self::Codex(client) => {
                let model = client.model().await.map_err(|error| Classified {
                    code: if error.transient {
                        Code::Transient
                    } else {
                        Code::Authentication
                    },
                    message: error.message,
                    retry_after: None,
                })?;
                model.stream_observed(request, observed)
            }
        }
        .map_err(|error| self.classify(&error))
    }

    /// The error code of a provider failure, from its status, codes and message.
    fn classify(&self, error: &ProviderError) -> Classified {
        let message = match self {
            Self::Codex(_) => codex::diagnostic(error),
            _ => diagnostic(error),
        };
        let report = error.report();
        let status = report.http_status;
        let text =
            format!("{} {message}", report.code.as_deref().unwrap_or_default()).to_lowercase();
        let any = |terms: &[&str]| terms.iter().any(|term| text.contains(term));
        let code = if matches!(self, Self::Codex(_)) && codex::usage_limited(error)
            || any(&["quota", "billing", "credit balance", "usage limit"])
        {
            Code::Quota
        } else if matches!(status, Some(401 | 403))
            || any(&[
                "authentication",
                "unauthorized",
                "permission",
                "invalid_api_key",
                "api key",
            ])
        {
            Code::Authentication
        } else if status == Some(429) || any(&["rate_limit", "rate limit"]) {
            Code::RateLimit
        } else if error.is_retryable()
            || status.is_some_and(|status| status >= 500)
            || (status.is_none()
                && matches!(
                    report.code.as_deref(),
                    Some("overloaded_error" | "server_error" | "api_error")
                ))
        {
            Code::Transient
        } else {
            Code::ProviderError
        };
        Classified {
            code,
            message,
            retry_after: retry_after(error),
        }
    }

    /// One turn, retried while an attempt fails with a rate limit or a transient error
    /// before producing any output or usage: at any turn, since every request replays the
    /// whole conversation. A retry waits as long as the provider's `Retry-After` asks, or a
    /// short backoff, within the review's deadline.
    pub async fn turn(
        &self,
        request: &CompletionRequest,
        context: Turn<'_>,
        attempts: &mut Vec<Attempt>,
    ) -> Result<CompletionResponse, Failure> {
        for attempt in 1..=ATTEMPTS {
            if context.cancellation.is_cancelled() {
                return Err(Failure::cancelled());
            }
            if Instant::now() >= context.deadline {
                return Err(Failure::timeout());
            }
            let usage = Arc::new(ReportedUsage::default());
            let observed = AdapterContext::new(
                usage.clone(),
                Subject::default(),
                format!("turn-{}", context.number),
            );
            let mut emitted = false;
            let opened = tokio::select! {
                biased;
                _ = context.cancellation.cancelled() => return Err(Failure::cancelled()),
                _ = tokio::time::sleep_until(context.deadline) => return Err(Failure::timeout()),
                opened = self.stream(request.clone(), observed) => opened,
            };
            let (result, partial) = match opened {
                Err(failure) => (Err(failure), None),
                Ok(mut stream) => {
                    let read = tokio::select! {
                        biased;
                        _ = context.cancellation.cancelled() => Err(Failure::cancelled()),
                        _ = tokio::time::sleep_until(context.deadline) => Err(Failure::timeout()),
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
                    let result = match read {
                        Ok(Ok(())) => stream.finish().await.map_err(|error| self.classify(&error)),
                        Ok(Err(error)) => Err(self.classify(&error)),
                        Err(failure) => Err(Classified::from(failure)),
                    };
                    (result, Some(partial))
                }
            };
            let mut counters = usage.0.lock().unwrap().clone();
            if let Some(partial) = &partial {
                add_cache_usage(&mut counters, partial);
            }
            if let Ok(response) = &result {
                add_cache_usage(&mut counters, response);
            }
            let used_tokens = counters.values().any(|v| v.as_u64().is_some_and(|v| v > 0));
            attempts.push(Attempt {
                turn: context.number,
                attempt,
                usage: counters,
                error: result.as_ref().err().map(|failure| failure.message.clone()),
                error_code: result
                    .as_ref()
                    .err()
                    .map(|failure| failure.code.as_str().to_owned()),
            });
            let failure = match result {
                Ok(response) => return Ok(response),
                Err(failure) => failure,
            };
            if !failure.code.retryable() || emitted || used_tokens || attempt == ATTEMPTS {
                return Err(failure.into());
            }
            let wait = failure
                .retry_after
                .unwrap_or(Duration::from_millis(250 << (attempt - 1)));
            // A wait too long to add to now is past any deadline.
            if failure.retry_after.is_some()
                && Instant::now()
                    .checked_add(wait)
                    .is_none_or(|end| end >= context.deadline)
            {
                return Err(Failure::new(
                    failure.code,
                    format!(
                        "{}. The provider asked to retry after {:.1} s, past the review deadline.",
                        failure.message.trim_end_matches('.'),
                        wait.as_secs_f64()
                    ),
                ));
            }
            tokio::select! {
                _ = context.cancellation.cancelled() => return Err(Failure::cancelled()),
                _ = tokio::time::sleep_until(context.deadline) => return Err(Failure::timeout()),
                _ = tokio::time::sleep(wait) => {},
            }
        }
        unreachable!()
    }
}

pub struct Turn<'a> {
    pub number: usize,
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

/// The wait a provider asked for: `retry-after-ms`, or `Retry-After` in seconds or as an
/// HTTP date.
fn retry_after(error: &ProviderError) -> Option<Duration> {
    wait_header(
        error.provider_response_headers()?,
        time::OffsetDateTime::now_utc(),
    )
}

fn wait_header(
    headers: &rig_core::http_client::HeaderMap,
    now: time::OffsetDateTime,
) -> Option<Duration> {
    let header = |name: &str| headers.get(name)?.to_str().ok().map(str::trim);
    let seconds = |value: &str| {
        value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .and_then(|value| Duration::try_from_secs_f64(value).ok())
    };
    if let Some(wait) = header("retry-after-ms").and_then(seconds) {
        return Some(wait / 1000);
    }
    let value = header("retry-after")?;
    seconds(value).or_else(|| {
        let at = time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc2822)
            .ok()?;
        Some(Duration::try_from(at - now).unwrap_or_default())
    })
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

/// The environment variable that holds an API-key backend's key; `None` for codex.
pub fn key_variable(backend: Backend) -> Option<&'static str> {
    match backend {
        Backend::Openai => Some("OPENAI_API_KEY"),
        Backend::Anthropic => Some("ANTHROPIC_API_KEY"),
        Backend::Codex => None,
    }
}

/// The environment variable that points the backend at a loopback test endpoint.
pub fn base_url_variable(backend: Backend) -> &'static str {
    match backend {
        Backend::Openai => "ARTIFACTIZE_OPENAI_BASE_URL",
        Backend::Anthropic => "ARTIFACTIZE_ANTHROPIC_BASE_URL",
        Backend::Codex => "ARTIFACTIZE_CODEX_BASE_URL",
    }
}

/// Every test endpoint variable: the backends' API roots and the Codex sign-in root.
const TEST_ENDPOINTS: [&str; 4] = [
    "ARTIFACTIZE_OPENAI_BASE_URL",
    "ARTIFACTIZE_ANTHROPIC_BASE_URL",
    "ARTIFACTIZE_CODEX_BASE_URL",
    crate::auth::codex::AUTH_URL_VARIABLE,
];

/// The loopback test endpoint that replaces the backend's API root, if one is set.
pub fn test_endpoint(backend: Backend) -> Result<Option<String>, String> {
    variable_endpoint(base_url_variable(backend))
}

/// The loopback test endpoint in `variable`, if set. An empty value is unset; any other
/// value must be a loopback URL.
pub fn variable_endpoint(variable: &str) -> Result<Option<String>, String> {
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
    TEST_ENDPOINTS
        .into_iter()
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
            Backend::Codex => codex::BASE_URL,
        }
        .into()
    }))
}

/// The backend's API key from its environment variable; never printed or logged.
fn api_key(backend: Backend, purpose: &str) -> Result<String, String> {
    let variable = key_variable(backend).expect("an API-key backend");
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
