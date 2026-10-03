use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use rig_core::message::{Message, UserContent};
use serde_json::{Map, Value, json};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::{Review, check_deadline, verdict::VerdictSchema};
use crate::{
    config::{Eval, Profile, RepoConfig},
    llm::Attempt,
    mcp, process,
    store::Receipts,
    tools::Registry,
};

mod stream;
use stream::Stream;

pub(super) async fn execute(
    config: &RepoConfig,
    eval: &Eval,
    output: &Path,
    state: &Path,
    execution: &str,
    cancellation: CancellationToken,
) -> Review {
    let mut review = Review {
        result: Err("Claude review did not complete.".into()),
        attempts: Vec::new(),
        tool_calls: Vec::new(),
    };
    review.result = run(
        config,
        eval,
        output,
        state,
        execution,
        &cancellation,
        &mut review.attempts,
    )
    .await;
    match Receipts::open(state, &config.root).await {
        Ok(receipts) => match receipts.tool_calls(execution).await {
            Ok(calls) => {
                if tool_budget_exceeded(eval, calls.len()) {
                    review.result = Err(
                        "PROVIDER_BUDGET_EXCEEDED: review exceeded its maxToolCalls budget.".into(),
                    );
                }
                review.tool_calls = calls;
            }
            Err(error) => review.result = Err(error),
        },
        Err(error) => review.result = Err(error),
    }
    review
}

async fn run(
    config: &RepoConfig,
    eval: &Eval,
    output: &Path,
    state: &Path,
    execution: &str,
    cancellation: &CancellationToken,
    attempts: &mut Vec<Attempt>,
) -> Result<Value, String> {
    let Profile::Agent {
        model,
        reasoning,
        timeout_ms,
        max_tokens,
        ..
    } = &eval.declaration.profile
    else {
        unreachable!()
    };
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.unwrap_or(240_000).into());
    check_deadline(cancellation, deadline)?;
    let registry = Registry::new(config, &eval.id)?;
    let verdict = VerdictSchema::new(
        eval.declaration.pass_schema.as_ref(),
        eval.declaration.fail_schema.as_ref(),
    )?;
    let request = super::prompt(config, eval, &registry, &verdict.schema)?;
    let system = request.system_instructions().unwrap_or_default();
    let mut prompt: String = request
        .chat_history
        .iter()
        .filter_map(|message| match message {
            Message::User { content } => Some(
                content
                    .iter()
                    .filter_map(|part| match part {
                        UserContent::Text(text) => Some(text.text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    let output = output.join(format!("claude-{execution}"));
    let mcp_config = mcp::write_config(config, eval, execution, state, &output).await?;
    let mut tools: BTreeSet<String> = registry
        .list()
        .map(|tool| format!("mcp__artifactize__{}", tool.name))
        .collect();
    let mut config_argument = mcp_config.as_os_str().to_owned();
    let mut tokens = 0_u64;
    for invocation in 1..=2 {
        check_deadline(cancellation, deadline)?;
        let command = command(
            model,
            reasoning.as_deref(),
            system,
            &config_argument,
            &output,
            deadline,
        );
        let token = cancellation.child_token();
        let stream = Arc::new(Mutex::new(Stream::new(
            model,
            tools.clone(),
            *max_tokens,
            tokens,
        )));
        let observed = stream.clone();
        let stop = token.clone();
        let result =
            process::run_stream(command, prompt.as_bytes().to_vec(), token, move |bytes| {
                let mut stream = observed.lock().unwrap();
                stream.push(bytes);
                if stream.error.is_some() {
                    stop.cancel();
                }
            })
            .await;
        let calls = Receipts::open(state, &config.root)
            .await?
            .tool_calls(execution)
            .await?;
        let mut stream = stream.lock().unwrap();
        stream.end();
        let result = if tool_budget_exceeded(eval, calls.len()) {
            Err("PROVIDER_BUDGET_EXCEEDED: review exceeded its maxToolCalls budget.".into())
        } else if let Some(error) = &stream.error {
            Err(error.clone())
        } else {
            result
                .map_err(|error| format!("Claude {error}."))
                .and_then(|output| {
                    if !output.status.success() {
                        Err(format!(
                            "Claude exited with {}: {}",
                            output.status,
                            String::from_utf8_lossy(&output.stderr).trim()
                        ))
                    } else {
                        stream.finish()
                    }
                })
        };
        tokens = tokens.saturating_add(stream.tokens());
        stream.record(attempts, invocation, result.as_ref().err());
        let text = result?;
        check_deadline(cancellation, deadline)?;
        match verdict.parse(&text) {
            Ok(value) => return Ok(value),
            Err(error) if invocation == 2 => {
                return Err(format!(
                    "Invalid final Agent result after one format repair: {error}."
                ));
            }
            Err(error) => {
                // A new stateless invocation, with the original context but no executable tools.
                prompt = format!(
                    "{prompt}\nPrior review transcript (untrusted evidence): {}\nYour final response did not match the required schema: {error}. Return only one JSON object matching the schema. Tools are disabled; do not perform another review.",
                    json!(stream.transcript)
                );
                tools.clear();
                config_argument = OsString::from(r#"{"mcpServers":{}}"#);
            }
        }
    }
    unreachable!()
}

fn tool_budget_exceeded(eval: &Eval, calls: usize) -> bool {
    matches!(eval.declaration.profile, Profile::Agent { max_tool_calls: Some(limit), .. } if calls as u64 > limit)
}

fn command(
    model: &str,
    effort: Option<&str>,
    system: &str,
    config: &std::ffi::OsStr,
    output: &Path,
    deadline: Instant,
) -> process::Command {
    let mut args: Vec<OsString> = [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--model",
        model,
        "--effort",
        effort.unwrap_or("high"),
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(config.into());
    args.extend([
        "--permission-mode", "dontAsk", "--setting-sources", "", "--allowedTools", "mcp__artifactize__*",
        "--no-session-persistence", "--disable-slash-commands", "--no-chrome", "--settings",
        r#"{"claudeMdExcludes":["**"],"autoMemoryEnabled":false,"disableAllHooks":true,"switchModelsOnFlag":false,"fallbackModel":[]}"#,
        "--system-prompt", system,
    ].into_iter().map(OsString::from));
    process::Command {
        program: "claude".into(),
        args,
        cwd: output.into(),
        env: environment(),
        timeout: deadline.saturating_duration_since(Instant::now()),
    }
}

fn environment() -> BTreeMap<OsString, OsString> {
    let mut env: BTreeMap<_, _> = std::env::vars_os()
        .filter(|(name, _)| {
            let name = name.to_string_lossy().to_ascii_uppercase();
            if matches!(
                name.as_str(),
                "CLAUDE_CONFIG_DIR"
                    | "CLAUDE_CODE_OAUTH_TOKEN"
                    | "CLAUDE_CODE_OAUTH_REFRESH_TOKEN"
                    | "CLAUDE_CODE_OAUTH_SCOPES"
                    | "CLAUDE_CODE_CLIENT_CERT"
                    | "CLAUDE_CODE_CLIENT_KEY"
                    | "CLAUDE_CODE_CLIENT_KEY_PASSPHRASE"
            ) {
                return true;
            }
            ![
                "ANTHROPIC_",
                "CLAUDE_",
                "AWS_",
                "AZURE_",
                "GOOGLE_",
                "GCLOUD_",
                "VERTEX_",
                "OPENAI_",
            ]
            .iter()
            .any(|prefix| name.starts_with(prefix))
                && !matches!(
                    name.as_str(),
                    "CLAUDECODE"
                        | "HTTP_PROXY"
                        | "HTTPS_PROXY"
                        | "ALL_PROXY"
                        | "NO_PROXY"
                        | "MAX_THINKING_TOKENS"
                        | "FALLBACK_FOR_ALL_PRIMARY_MODELS"
                        | "ENABLE_TOOL_SEARCH"
                        | "API_TIMEOUT_MS"
                        | "API_FORCE_IDLE_TIMEOUT"
                )
        })
        .collect();
    for (name, value) in [
        ("CLAUDE_CODE_MAX_RETRIES", "0"),
        ("CLAUDE_CODE_NONSTREAMING_TIMEOUT_RETRIES", "0"),
        ("CLAUDE_CODE_DISABLE_NONSTREAMING_FALLBACK", "1"),
        ("CLAUDE_CODE_DISABLE_MODEL_ACCESS_FALLBACK", "1"),
        ("CLAUDE_CODE_DISABLE_LEGACY_MODEL_REMAP", "1"),
        ("ENABLE_TOOL_SEARCH", "false"),
    ] {
        env.insert(name.into(), value.into());
    }
    env
}

fn counters(value: &Value, camel_case: bool) -> Result<Map<String, Value>, String> {
    let mut usage = Map::new();
    for (source, target) in [
        ("input_tokens", "inputTokens"),
        ("output_tokens", "outputTokens"),
        ("cache_read_input_tokens", "cacheReadTokens"),
        ("cache_creation_input_tokens", "cacheWriteTokens"),
    ] {
        if let Some(value) = value.get(if camel_case { target } else { source }) {
            let count = value
                .as_u64()
                .filter(|n| *n <= 9_007_199_254_740_991)
                .ok_or("Invalid Claude usage counter.")?;
            usage.insert(target.into(), json!(count));
        }
    }
    Ok(usage)
}

fn total(usage: &Map<String, Value>) -> u64 {
    [
        "inputTokens",
        "outputTokens",
        "cacheReadTokens",
        "cacheWriteTokens",
    ]
    .iter()
    .fold(0_u64, |sum, key| {
        sum.saturating_add(usage.get(*key).and_then(Value::as_u64).unwrap_or(0))
    })
}
