//! The review conversation against a scripted Claude Code on the other end of the channel:
//! what artifactize writes to Claude Code, and the verdict, failures, attempts and saved
//! conversation it makes of what Claude Code writes back.

use std::{path::PathBuf, time::Duration};

use rig_core::message::AssistantContent;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

use super::*;
use crate::{config::read_workspace_config, llm::claude_agent::channel::Channel};

const SESSION: &str = "7d1c8e2a-claude-agent-session";
const MODEL: &str = "claude-opus-5-5";
/// Claude Code's version in a successful `initialize`.
const VERSION: &str = "2.1.322";

struct Fixture {
    _directory: TempDir,
    config: RepoConfig,
    output: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = crate::test_os::tempdir();
        let repo = directory.path().join("repo");
        let output = directory.path().join("output");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        crate::test_declaration::write(
            repo.join("index.artf"),
            json!({
                "name": "a",
                "evals": [{
                    "id": "review",
                    "title": "Review",
                    "profile": {"kind": "agent", "backend": "claude-agent", "model": MODEL, "reasoning": "high"},
                    "payload": {"instruction": "Check {a}"},
                    "pass_schema": {
                        "type": "object",
                        "properties": {"reason": {"type": "string"}},
                        "required": ["reason"],
                    },
                }],
                "views": {"agent_tools": {"read": {"builtin": "read"}}},
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(repo.join("notes.txt"), "the evidence line\n").unwrap();
        let config = read_workspace_config(&repo).unwrap();
        Self {
            state: directory.path().join("state"),
            _directory: directory,
            config,
            output,
        }
    }
}

/// Claude Code's end of the channel: what artifactize wrote, and what Claude Code writes.
struct Claude {
    input: Lines<BufReader<ReadHalf<DuplexStream>>>,
    output: WriteHalf<DuplexStream>,
}

/// One line holds at most a tool result here.
const PIPE: usize = 1024 * 1024;

fn channel() -> (
    Channel<ReadHalf<DuplexStream>, WriteHalf<DuplexStream>>,
    Claude,
) {
    let (ours, theirs) = tokio::io::duplex(PIPE);
    let (our_reader, our_writer) = tokio::io::split(ours);
    let (their_reader, their_writer) = tokio::io::split(theirs);
    (
        Channel::new(our_reader, our_writer),
        Claude {
            input: BufReader::new(their_reader).lines(),
            output: their_writer,
        },
    )
}

impl Claude {
    async fn read(&mut self) -> Value {
        let line = self.input.next_line().await.unwrap().expect("a line");
        serde_json::from_str(&line).unwrap()
    }

    async fn write(&mut self, line: Value) {
        let mut bytes = line.to_string().into_bytes();
        bytes.push(b'\n');
        self.output.write_all(&bytes).await.unwrap();
    }

    /// Send an MCP message for artifactize's server and return its JSON-RPC reply.
    async fn mcp(&mut self, message: Value) -> Value {
        self.write(json!({
            "type": "control_request",
            "request_id": "mcp",
            "request": {"subtype": "mcp_message", "server_name": "artifactize", "message": message},
        }))
        .await;
        let reply = self.read().await;
        assert_eq!(reply["response"]["request_id"], "mcp");
        reply["response"]["response"]["mcp_response"].clone()
    }

    /// Set up the MCP server, answer `initialize` as Claude Code `version`, and return the
    /// offered tools' names.
    async fn start(&mut self, version: &str) -> Vec<String> {
        let initialize = self.read().await;
        assert_eq!(initialize["request"]["subtype"], "initialize");
        assert_eq!(
            initialize["request"]["sdkMcpServers"],
            json!(["artifactize"])
        );
        let reply = self
            .mcp(json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}}}))
            .await;
        assert_eq!(reply["result"]["protocolVersion"], "2025-11-25");
        self.mcp(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        let tools = self
            .mcp(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .await;
        self.write(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": initialize["request_id"], "response": {"claude_code_version": version, "models": []}},
        }))
        .await;
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// Read the next user message artifactize sends and return its text.
    async fn prompt(&mut self) -> String {
        let user = self.read().await;
        assert_eq!(user["type"], "user");
        assert_eq!(user["client_composed"], true, "sent verbatim");
        user["message"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn event(&mut self, event: Value) {
        self.write(json!({"type": "stream_event", "event": event}))
            .await;
    }

    /// One response `id` of `blocks`, streamed and then delivered whole, with usage.
    async fn respond(&mut self, id: &str, blocks: Vec<Value>) {
        self.event(json!({"type": "message_start", "message": {"id": id, "model": MODEL, "usage": {"input_tokens": 10, "output_tokens": 1}}}))
            .await;
        for (index, block) in blocks.iter().enumerate() {
            if block["type"] == "text" {
                self.event(json!({"type": "content_block_start", "index": index, "content_block": {"type": "text", "text": ""}}))
                    .await;
                self.event(json!({"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": block["text"]}}))
                    .await;
                self.event(json!({"type": "content_block_stop", "index": index}))
                    .await;
            }
            self.write(json!({"type": "assistant", "message": {"id": id, "model": MODEL, "content": [block]}, "parent_tool_use_id": null}))
                .await;
        }
    }

    async fn stop(&mut self) {
        self.event(
            json!({"type": "message_delta", "usage": {"input_tokens": 10, "output_tokens": 30}}),
        )
        .await;
        self.event(json!({"type": "message_stop"})).await;
    }

    /// Call artifactize's tool `name` and return the reply's result.
    async fn call(&mut self, id: &str, name: &str, arguments: Value) -> Value {
        self.respond(
            &format!("msg-{id}"),
            vec![json!({"type": "tool_use", "id": id, "name": format!("mcp__artifactize__{name}"), "input": arguments})],
        )
        .await;
        let reply = self
            .mcp(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": name, "arguments": arguments}}))
            .await;
        reply["result"].clone()
    }

    /// Report a tool's result back as Claude Code does, then end the response.
    async fn answered(&mut self, id: &str, result: &Value) {
        self.write(json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": result["content"], "is_error": result["isError"]}]},
            "parent_tool_use_id": null,
        }))
        .await;
        self.stop().await;
    }

    async fn verdict(&mut self, id: &str, verdict: Value) {
        self.respond(
            id,
            vec![json!({"type": "tool_use", "id": format!("{id}-out"), "name": "StructuredOutput", "input": verdict})],
        )
        .await;
        self.stop().await;
        self.write(json!({"type": "result", "subtype": "success", "is_error": false, "result": "", "structured_output": verdict}))
            .await;
    }
}

/// The review's budgets in a test: a minute, and `max_tool_calls` calls.
fn limits(max_tool_calls: Option<u64>) -> Limits {
    Limits {
        deadline: Instant::now() + Duration::from_secs(60),
        max_tokens: None,
        max_tool_calls,
    }
}

struct Outcome {
    result: Result<ValidatedResult, Failure>,
    attempts: Vec<Attempt>,
    fixture: Fixture,
}

impl Outcome {
    fn saved(&self) -> session::Conversation {
        let path = session::path(&self.fixture.state, SESSION).unwrap();
        session::Conversation::load(&path).unwrap().unwrap()
    }
}

/// Run one review against Claude Code playing `script`.
async fn review<F, Fut>(limits: Limits, script: F) -> Outcome
where
    F: FnOnce(Claude) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let fixture = Fixture::new();
    let eval = &fixture.config.evals[0];
    let registry = Registry::new(&fixture.config, &eval.id).unwrap();
    let verdict = verdict::VerdictSchema::new(
        eval.declaration.pass_schema(),
        eval.declaration.fail_schema(),
    )
    .unwrap();
    let structured = Structured::new(&verdict.schema, Api::ClaudeAgent);
    let server = Server::new(registry.list());
    let mut recorder = Recorder::test(&fixture.state, SESSION);
    recorder.start(session::Header::default());
    let mut attempts = Vec::new();
    let cancellation = CancellationToken::new();
    let (ours, theirs) = channel();
    let mut conversation = Conversation::new(
        Session {
            server: &server,
            registry: &registry,
            output: &fixture.output,
            model: MODEL,
            limits,
            cancellation: &cancellation,
            recorder: &mut recorder,
            attempts: &mut attempts,
        },
        ours,
    );
    let (result, ()) = tokio::join!(
        async {
            let result = match conversation.initialize().await {
                Ok(()) => judge(&mut conversation, "Review a.", &verdict, &structured).await,
                Err(failure) => Err(failure),
            };
            if let Err(failure) = &result {
                conversation.fail(failure);
            }
            result
        },
        script(theirs),
    );
    drop(conversation);
    drop(recorder);
    Outcome {
        result,
        attempts,
        fixture,
    }
}

#[tokio::test]
async fn a_review_runs_its_tools_in_artifactize_and_returns_the_validated_verdict() {
    let outcome = review(limits(None), |mut claude| async move {
        assert_eq!(claude.start(VERSION).await, ["read_a"]);
        assert!(claude.prompt().await.starts_with("Review a."));
        let result = claude
            .call("toolu_1", "read_a", json!({"path": "notes.txt"}))
            .await;
        assert_eq!(result["isError"], false);
        assert!(
            result["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("the evidence line"),
            "{result}"
        );
        claude.answered("toolu_1", &result).await;
        claude
            .respond(
                "msg-text",
                vec![json!({"type": "text", "text": "It holds."})],
            )
            .await;
        claude
            .verdict(
                "msg-verdict",
                json!({"verdict": "GREEN", "verdictFields": {"reason": "Read it."}}),
            )
            .await;
    })
    .await;
    let result = outcome.result.as_ref().unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({"verdict": "GREEN", "reason": "Read it."})
    );
    // One attempt per response, with the highest usage Claude Code reported for it.
    let turns: Vec<_> = outcome
        .attempts
        .iter()
        .map(|attempt| attempt.turn)
        .collect();
    assert_eq!(turns, [1, 2, 3]);
    assert_eq!(outcome.attempts[0].usage["outputTokens"], 30);
    let saved = outcome.saved();
    let history = saved.history().unwrap();
    let call = history.iter().find_map(|message| match message {
        Message::Assistant { content, .. } => content.iter().find_map(|part| match part {
            AssistantContent::ToolCall(call) => Some(call.function.name.as_str().to_owned()),
            _ => None,
        }),
        _ => None,
    });
    assert_eq!(call.as_deref(), Some("read_a"));
    let delivered = saved
        .wire_events
        .iter()
        .any(|event| event["kind"] == "delivery" && event["text"] == "It holds.");
    assert!(delivered, "the text reached the live view");
}

#[tokio::test]
async fn a_tool_call_past_the_budget_is_refused_and_ends_the_review() {
    let outcome = review(limits(Some(0)), |mut claude| async move {
        claude.start(VERSION).await;
        claude.prompt().await;
        let result = claude
            .call("toolu_1", "read_a", json!({"path": "notes.txt"}))
            .await;
        assert_eq!(result["isError"], true);
    })
    .await;
    let failure = outcome.result.unwrap_err();
    assert_eq!(failure.code, Code::ProviderBudgetExceeded);
    assert_eq!(
        outcome.attempts.last().unwrap().error_code,
        Some(Code::ProviderBudgetExceeded)
    );
}

#[tokio::test]
async fn a_reached_subscription_limit_is_a_quota_failure_with_its_reset() {
    let outcome = review(limits(None), |mut claude| async move {
        claude.start(VERSION).await;
        claude.prompt().await;
        claude
            .write(json!({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "resetsAt": 1794531600, "rateLimitType": "seven_day"}}))
            .await;
    })
    .await;
    let failure = outcome.result.unwrap_err();
    assert_eq!(failure.code, Code::Quota);
    assert_eq!(
        failure.message,
        "Claude Code subscription limit reached in the weekly window; it resets at 2026-11-13T01:00:00Z."
    );
}

#[tokio::test]
async fn a_signed_out_claude_code_is_an_authentication_failure() {
    let outcome = review(limits(None), |mut claude| async move {
        claude.start(VERSION).await;
        claude.prompt().await;
        claude
            .write(json!({
                "type": "assistant",
                "message": {"id": "synthetic", "model": "<synthetic>", "content": [{"type": "text", "text": "Not logged in"}]},
                "error": "authentication_failed",
            }))
            .await;
    })
    .await;
    let failure = outcome.result.unwrap_err();
    assert_eq!(failure.code, Code::Authentication);
    assert!(failure.message.contains("claude auth login"), "{failure}");
}

#[tokio::test]
async fn an_old_claude_code_is_refused_before_the_review() {
    let outcome = review(limits(None), |mut claude| async move {
        claude.start("2.1.200").await;
    })
    .await;
    let failure = outcome.result.unwrap_err();
    assert_eq!(failure.code, Code::AgentError);
    assert!(failure.message.contains("claude update"), "{failure}");
}

#[tokio::test]
async fn an_invalid_verdict_gets_one_repair_without_tools() {
    let outcome = review(limits(None), |mut claude| async move {
        claude.start(VERSION).await;
        claude.prompt().await;
        claude
            .verdict("msg-1", json!({"verdict": "GREEN", "verdictFields": {}}))
            .await;
        let repair = claude.prompt().await;
        assert!(
            repair.contains("did not match the required schema"),
            "{repair}"
        );
        let result = claude
            .call("toolu_2", "read_a", json!({"path": "notes.txt"}))
            .await;
        assert_eq!(result["isError"], true);
    })
    .await;
    let failure = outcome.result.unwrap_err();
    assert_eq!(failure.code, Code::InvalidResult);
    assert!(failure.message.contains("tools are disabled"), "{failure}");
}

#[tokio::test]
async fn a_repaired_verdict_completes_the_review() {
    let outcome = review(limits(None), |mut claude| async move {
        claude.start(VERSION).await;
        claude.prompt().await;
        claude
            .verdict("msg-1", json!({"verdict": "GREEN", "verdictFields": {}}))
            .await;
        claude.prompt().await;
        claude
            .verdict(
                "msg-2",
                json!({"verdict": "GREEN", "verdictFields": {"reason": "Fixed."}}),
            )
            .await;
    })
    .await;
    assert_eq!(
        serde_json::to_value(outcome.result.unwrap()).unwrap(),
        json!({"verdict": "GREEN", "reason": "Fixed."})
    );
}

#[tokio::test]
async fn an_answer_from_another_model_fails_the_review() {
    let outcome = review(limits(None), |mut claude| async move {
        claude.start(VERSION).await;
        claude.prompt().await;
        claude
            .event(json!({"type": "message_start", "message": {"id": "m", "model": "claude-haiku-5-5"}}))
            .await;
    })
    .await;
    let failure = outcome.result.unwrap_err();
    assert_eq!(failure.code, Code::ProviderError);
    assert!(failure.message.contains("model mismatch"), "{failure}");
}
