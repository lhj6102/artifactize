//! Agent turn retries, error codes and stopping a backend, against loopback fakes.

#[path = "support/run.rs"]
mod owned_run;
use owned_run::RunChild as Child;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::{Value, json};
use support::{FakeProvider, Reply, Request, anthropic, codex, openai};

/// A workspace of Artifacts, each with one Agent eval and its own fingerprint.
struct Project {
    // Retain inputs if releasing any durable worker fails, including on assertion unwind.
    _root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
}

impl Project {
    fn new(evals: &[(&str, Value)]) -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        for (name, profile) in evals {
            let folder = repo.join(name);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("notes.md"), format!("{name}: R1 holds.\n")).unwrap();
            support::declaration::write(
                folder.join("index.artf"),
                json!({"name":name,"fingerprint":{},
                    "views":{"agent_tools":{"read":{"builtin":"read"}}},
                    "evals":[{"id":"review","title":"Review","profile":profile,
                        "payload":{"instruction":format!("Review {{{name}}}.")}}]})
                .to_string(),
            )
            .unwrap();
        }
        let state = root.path().join("state");
        root.track(&state, &repo);
        let project = Self {
            state,
            repo,
            _root: root,
        };
        // A Codex sign-in, for the codex evals.
        let auth = project.state.join("auth");
        support::os::create_private_dir_all(&auth);
        let credentials = json!({
            "access_token":codex::jwt("account-1", codex::VALID_EXPIRY),
            "refresh_token":"refresh",
            "account_id":"account-1",
            "expires_at":codex::VALID_EXPIRY,
            "saved_at":1,
        });
        support::os::write_private_file(&auth.join("codex.json"), credentials.to_string());
        project
    }

    fn verify(&self, provider: &FakeProvider, args: &[&str], json: bool) -> Output {
        self.verify_with_cli(&support::os::cli(), provider, args, json)
    }

    fn verify_with_cli(
        &self,
        executable: &std::path::Path,
        provider: &FakeProvider,
        args: &[&str],
        json: bool,
    ) -> Output {
        let mut command = Command::new(executable);
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .arg("verify")
            .arg("--wait")
            .args(args)
            .env("ARTIFACTIZE_REMOTE", "off")
            .env("OPENAI_API_KEY", "fake-openai-key")
            .env("ANTHROPIC_API_KEY", "fake-anthropic-key")
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
            .env("ARTIFACTIZE_ANTHROPIC_BASE_URL", provider.anthropic_base())
            .env("ARTIFACTIZE_CODEX_BASE_URL", provider.codex_base())
            .env("ARTIFACTIZE_CODEX_AUTH_URL", &provider.url)
            .env_remove("ARTIFACTIZE_CODEX_AUTH_FILE");
        if json {
            command.arg("--json");
        }
        command.record_output().unwrap()
    }

    fn run(&self, provider: &FakeProvider, args: &[&str], code: i32) -> Value {
        let output = self.verify(provider, args, true);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

fn agent(backend: &str) -> Value {
    json!({"kind":"agent","backend":backend,"model":format!("{backend}-model"),"timeout_ms":20000})
}

fn request<'a>(run: &'a Value, eval: &str) -> &'a Value {
    let requests = run["requests"].as_array().unwrap();
    requests
        .iter()
        .find(|request| request["evalId"] == eval)
        .unwrap()
}

fn attempts(request: &Value) -> Vec<(u64, u64, Value)> {
    request["usage"]
        .as_array()
        .unwrap()
        .iter()
        .map(|attempt| {
            (
                attempt["turn"].as_u64().unwrap(),
                attempt["attempt"].as_u64().unwrap(),
                attempt["errorCode"].clone(),
            )
        })
        .collect()
}

fn replayed(request: &Request) -> bool {
    request.body["input"].as_array().is_some_and(|input| {
        input
            .iter()
            .any(|item| item["type"] == "function_call_output")
    }) || request.body["messages"]
        .as_array()
        .is_some_and(|messages| messages.len() > 1)
}

/// Pass after one `read` tool call, in the requested backend's wire format.
fn pass(request: &Request) -> Reply {
    let verdict = r#"{"verdict":"GREEN"}"#;
    match request.path.as_str() {
        "/v1/messages" => anthropic::text(request, verdict, 10, 2),
        path => {
            let output = if replayed(request) {
                vec![openai::message(verdict)]
            } else {
                let artifact = request.body["tools"][0]["name"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                vec![openai::function_call(
                    "call_1",
                    &artifact,
                    &json!({"path":"notes.md"}),
                )]
            };
            if path.starts_with("/backend-api/codex") {
                codex::completed(request, output, openai::usage(10, 2))
            } else {
                openai::completed(request, output, openai::usage(10, 2))
            }
        }
    }
}

fn error(status: u16, headers: Vec<(&'static str, String)>, body: Value) -> Reply {
    let mut headers = headers;
    headers.push(("content-type", "application/json".into()));
    Reply::Raw(status, headers, body.to_string())
}

/// Fail the first `failures` model requests that `fails` selects, then answer `pass`.
fn flaky(
    fails: impl Fn(&Request) -> bool + Send + Sync + 'static,
    failures: Vec<Reply>,
) -> FakeProvider {
    let failures = std::sync::Mutex::new(failures.into_iter());
    FakeProvider::start(move |request| {
        if fails(request)
            && let Some(failure) = failures.lock().unwrap().next()
        {
            return failure;
        }
        pass(request)
    })
}

#[test]
fn a_later_turn_retries_transient_failures_and_replays_the_same_conversation() {
    let project = Project::new(&[("notes", agent("openai"))]);
    let provider = flaky(
        replayed,
        vec![
            error(
                503,
                vec![],
                json!({"error":{"message":"Service unavailable","type":"server_error"}}),
            ),
            error(
                429,
                vec![("retry-after-ms", "700".into())],
                json!({
                    "error":{
                        "message":"Rate limit reached",
                        "type":"requests",
                        "code":"rate_limit_exceeded",
                    },
                }),
            ),
        ],
    );
    let run = project.run(&provider, &["--all"], 0);
    let review = request(&run, "notes/review");
    assert_eq!(review["status"], "GREEN", "{run}");
    assert_eq!(
        attempts(review),
        [
            (1, 1, Value::Null),
            (2, 1, json!("TRANSIENT")),
            (2, 2, json!("RATE_LIMIT")),
            (2, 3, Value::Null)
        ]
    );
    let calls = provider.requests();
    assert_eq!(calls.len(), 4);
    // Each retry replays the same conversation; virtual-clock coverage below checks the wait.
    assert_eq!(calls[1].body, calls[3].body);
}

#[test]
fn responses_stream_overload_retries_without_output_and_keeps_the_same_request() {
    for backend in ["codex", "openai"] {
        let project = Project::new(&[("notes", agent(backend))]);
        // The real error arrives in SSE under HTTP 200, not an HTTP 5xx response.
        let events = vec![json!({
            "type":"error",
            "code":"server_is_overloaded",
            "message":"Our servers are currently overloaded. Please try again later.",
            "param":null,
            "sequence_number":0,
        })];
        let failure = if backend == "codex" {
            codex::stream(events)
        } else {
            Reply::Events(events)
        };
        let provider = flaky(|_| true, vec![failure]);
        let run = project.run(&provider, &["--all"], 0);
        let review = request(&run, "notes/review");
        assert_eq!(review["status"], "GREEN");
        assert_eq!(
            attempts(review),
            [
                (1, 1, json!("TRANSIENT")),
                (1, 2, Value::Null),
                (2, 1, Value::Null)
            ]
        );
        let calls = provider.requests();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].body, calls[1].body);
        assert!(replayed(&calls[2]));
    }
}

#[test]
fn every_backend_retries_and_reports_exhausted_failures_by_code() {
    // Anthropic overloads, then passes; codex fails at its second turn, then passes.
    let project = Project::new(&[("claude", agent("anthropic")), ("luna", agent("codex"))]);
    let failed = std::sync::Mutex::new(std::collections::BTreeSet::new());
    let provider = FakeProvider::start(move |request| {
        let mut failed = failed.lock().unwrap();
        if request.path == "/v1/messages" && failed.insert("anthropic") {
            return error(
                529,
                vec![],
                json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}),
            );
        }
        if request.path.starts_with("/backend-api/codex")
            && replayed(request)
            && failed.insert("codex")
        {
            return error(
                500,
                vec![],
                json!({"error":{"message":"Internal error","code":"server_error"}}),
            );
        }
        pass(request)
    });
    let run = project.run(&provider, &["--all", "--jobs", "1"], 0);
    assert_eq!(
        attempts(request(&run, "claude/review")),
        [(1, 1, json!("TRANSIENT")), (1, 2, Value::Null)]
    );
    assert_eq!(
        attempts(request(&run, "luna/review")),
        [
            (1, 1, Value::Null),
            (2, 1, json!("TRANSIENT")),
            (2, 2, Value::Null)
        ]
    );

    // Three rate limits without Retry-After exhaust the attempts.
    let limited = FakeProvider::start(|_| {
        error(
            429,
            vec![],
            json!({"error":{"message":"Rate limit reached","code":"rate_limit_exceeded"}}),
        )
    });
    let run = project.run(&limited, &["--all", "--force", "--jobs", "1"], 2);
    for eval in ["claude/review", "luna/review"] {
        let review = request(&run, eval);
        assert_eq!(review["errorCode"], "RATE_LIMIT", "{run}");
        assert_eq!(review["usage"].as_array().unwrap().len(), 3);
    }
}

#[test]
fn a_retry_after_past_the_deadline_fails_at_once() {
    let mut profile = agent("openai");
    profile["timeout_ms"] = json!(5000);
    let project = Project::new(&[("notes", profile)]);
    let provider = FakeProvider::start(|_| {
        error(
            429,
            vec![("retry-after", "120".into())],
            json!({"error":{"message":"Rate limit reached","code":"rate_limit_exceeded"}}),
        )
    });
    let run = project.run(&provider, &["--all"], 2);
    let review = request(&run, "notes/review");
    assert_eq!(review["errorCode"], "RATE_LIMIT");
    assert!(
        review["error"]
            .as_str()
            .unwrap()
            .ends_with("The provider asked to retry after 120.0 s, past the review deadline."),
        "{review}"
    );
    assert_eq!(provider.requests().len(), 1);
}

#[test]
fn failures_carry_specific_error_codes() {
    let project = Project::new(&[("notes", agent("openai"))]);
    for (reply, code) in [
        (
            error(
                401,
                vec![],
                json!({"error":{"message":"Incorrect API key provided","code":"invalid_api_key"}}),
            ),
            "AUTHENTICATION",
        ),
        (
            error(
                429,
                vec![],
                json!({
                    "error":{
                        "message":"You exceeded your current quota",
                        "code":"insufficient_quota",
                    },
                }),
            ),
            "QUOTA",
        ),
        (
            error(
                404,
                vec![],
                json!({"error":{"message":"The model does not exist","code":"model_not_found"}}),
            ),
            "PROVIDER_ERROR",
        ),
    ] {
        let reply = Arc::new(std::sync::Mutex::new(Some(reply)));
        let provider = FakeProvider::start(move |_| {
            reply.lock().unwrap().take().unwrap_or_else(|| {
                error(500, vec![], json!({"error":{"message":"unexpected retry"}}))
            })
        });
        let run = project.run(&provider, &["--all", "--force"], 2);
        let review = request(&run, "notes/review");
        assert_eq!(review["errorCode"], code, "{review}");
        assert_eq!(provider.requests().len(), 1, "{code} is never retried");
    }
    // Invalid output after the repair, and a missing key, have their own codes.
    let invalid = FakeProvider::start(|request| {
        openai::completed(
            request,
            vec![openai::message("not json")],
            openai::usage(1, 1),
        )
    });
    let run = project.run(&invalid, &["--all", "--force"], 2);
    assert_eq!(request(&run, "notes/review")["errorCode"], "INVALID_RESULT");
    let output = Command::new(support::os::cli())
        .arg("--repo")
        .arg(&project.repo)
        .arg("--state-dir")
        .arg(&project.state)
        .args(["verify", "--wait", "--all", "--force", "--json"])
        .env("ARTIFACTIZE_REMOTE", "off")
        .env("ARTIFACTIZE_OPENAI_BASE_URL", invalid.openai_base())
        .env_remove("OPENAI_API_KEY")
        .record_output()
        .unwrap();
    let run: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(request(&run, "notes/review")["errorCode"], "AUTHENTICATION");
}

/// The three backends the project reaches, one eval each.
const CAPPED: [(&str, &str); 3] = [
    ("gpt", "openai"),
    ("luna", "codex"),
    ("claude", "anthropic"),
];

/// A streamed answer of `deltas` that stops at the output cap after `output` tokens, in the
/// requested backend's wire format.
fn capped(request: &Request, deltas: &[String], output: u64) -> Reply {
    if request.path == "/v1/messages" {
        let mut events = vec![
            json!({"type":"message_start","message":{"id":"msg_fake","type":"message",
                "role":"assistant","model":request.body["model"],"content":[],
                "stop_reason":null,"stop_sequence":null,
                "usage":{"input_tokens":10,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        ];
        events.extend(deltas.iter().map(|delta| {
            json!({"type":"content_block_delta","index":0,
                "delta":{"type":"text_delta","text":delta}})
        }));
        events.extend([
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"max_tokens","stop_sequence":null},
                "usage":{"output_tokens":output}}),
            json!({"type":"message_stop"}),
        ]);
        return Reply::Events(events);
    }
    let mut message = openai::message(&deltas.concat());
    message["status"] = json!("incomplete");
    let mut events = vec![json!({"type":"response.output_item.added","output_index":0,
        "item":{"type":"message","id":"msg_fake","role":"assistant","status":"in_progress",
            "content":[]}})];
    events.extend(deltas.iter().map(|delta| {
        json!({"type":"response.output_text.delta","item_id":"msg_fake","output_index":0,
            "content_index":0,"delta":delta})
    }));
    events.push(
        json!({"type":"response.incomplete","response":{"id":"resp_fake",
        "object":"response","created_at":1,"model":request.body["model"],"status":"incomplete",
        "incomplete_details":{"reason":"max_output_tokens"},"output":[message],
        "usage":openai::usage(10, output)}}),
    );
    for (index, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = json!(index);
    }
    if request.path.starts_with("/backend-api/codex") {
        codex::stream(events)
    } else {
        Reply::Events(events)
    }
}

/// The saved session's `attempt` events of `review`.
fn attempt_events(project: &Project, review: &Value) -> Vec<Value> {
    let session = review["sessionId"].as_str().unwrap();
    fs::read_to_string(
        project
            .state
            .join("agent-sessions")
            .join(format!("{session}.jsonl")),
    )
    .unwrap()
    .lines()
    .map(|line| serde_json::from_str::<Value>(line).unwrap())
    .filter(|event| event["kind"] == "attempt")
    .collect()
}

/// Each attempt's `dispatch`: `None` for the first dispatch, which is not saved.
fn dispatches(request: &Value) -> Vec<Option<u64>> {
    request["usage"]
        .as_array()
        .unwrap()
        .iter()
        .map(|attempt| attempt["dispatch"].as_u64())
        .collect()
}

/// The saved session's `attempt` events of the session `id`.
fn session_attempts(project: &Project, id: &Value) -> Vec<Value> {
    attempt_events(project, &json!({ "sessionId": id }))
}

#[test]
fn a_response_rejected_after_delivery_fails_its_billed_attempt_with_its_counters() {
    let project = Project::new(&CAPPED.map(|(name, backend)| (name, agent(backend))));
    let provider =
        FakeProvider::start(|request| capped(request, &[r#"{"verdict":"GR"#.to_owned()], 128));
    let run = project.run(&provider, &["--all"], 2);
    for (name, backend) in CAPPED {
        let review = request(&run, &format!("{name}/review"));
        assert_eq!(review["errorCode"], "PROVIDER_ERROR", "{backend}: {review}");
        // Billed output is not retried within its turn; the review is dispatched once more.
        let usage = review["usage"].as_array().unwrap();
        assert_eq!(usage.len(), 2, "{backend}: {review}");
        assert_eq!(dispatches(review), [None, Some(2)], "{backend}");
        for attempt in usage {
            assert_eq!(
                attempt["errorCode"], "PROVIDER_ERROR",
                "{backend}: {attempt}"
            );
            assert_eq!(attempt["error"], review["error"], "{backend}");
            assert_eq!(
                attempt["usage"]["outputTokens"], 128,
                "{backend}: {attempt}"
            );
        }
        let redispatches = review["redispatches"].as_array().unwrap();
        assert_eq!(redispatches.len(), 1, "{backend}: {review}");
        assert_eq!(redispatches[0]["errorCode"], "PROVIDER_ERROR", "{backend}");
        assert_eq!(redispatches[0]["error"], review["error"], "{backend}");
        assert_ne!(
            redispatches[0]["sessionId"], review["sessionId"],
            "{backend}"
        );
        for (events, attempt) in [
            (
                session_attempts(&project, &redispatches[0]["sessionId"]),
                &usage[0],
            ),
            (attempt_events(&project, review), &usage[1]),
        ] {
            assert_eq!(events.len(), 1, "{backend}");
            assert_eq!(events[0]["errorCode"], "PROVIDER_ERROR", "{backend}");
            assert_eq!(events[0]["usage"], attempt["usage"], "{backend}");
        }
    }
    assert_eq!(provider.requests().len(), 2 * CAPPED.len());
}

/// An unfinished result, then whitespace in deltas that pass the stop point together.
fn degenerate_deltas() -> Vec<String> {
    let mut deltas = vec![r#"{"verdict":"GREEN","findings":["#.to_owned()];
    deltas.extend(std::iter::repeat_n(" \n\t ".repeat(256), 5));
    deltas
}

#[test]
fn a_response_that_degenerates_into_whitespace_stops_with_its_own_code() {
    let project = Project::new(&CAPPED.map(|(name, backend)| (name, agent(backend))));
    let deltas = degenerate_deltas();
    let provider = FakeProvider::start(move |request| capped(request, &deltas, 128_000));
    let run = project.run(&provider, &["--all"], 2);
    for (name, backend) in CAPPED {
        let review = request(&run, &format!("{name}/review"));
        assert_eq!(
            review["errorCode"], "DEGENERATE_OUTPUT",
            "{backend}: {review}"
        );
        assert!(
            review["error"].as_str().unwrap().contains("whitespace"),
            "{backend}: {review}"
        );
        assert_eq!(
            attempts(review),
            [
                (1, 1, json!("DEGENERATE_OUTPUT")),
                (1, 1, json!("DEGENERATE_OUTPUT"))
            ],
            "{backend}: one attempt in each of two dispatches"
        );
        let events = attempt_events(&project, review);
        assert_eq!(events.len(), 1, "{backend}");
        assert_eq!(events[0]["errorCode"], "DEGENERATE_OUTPUT", "{backend}");
    }
    assert_eq!(provider.requests().len(), 2 * CAPPED.len());
}

/// A streamed answer that breaks off with an overload after its first text.
fn interrupted(request: &Request) -> Reply {
    let mut events = vec![
        json!({"type":"response.output_item.added","output_index":0,
            "item":{"type":"message","id":"msg_fake","role":"assistant","status":"in_progress",
                "content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"msg_fake","output_index":0,
            "content_index":0,"delta":r#"{"verdict":"#}),
        json!({"type":"error","code":"server_is_overloaded",
            "message":"Our servers are currently overloaded. Please try again later.",
            "param":null}),
    ];
    for (index, event) in events.iter_mut().enumerate() {
        event["sequence_number"] = json!(index);
    }
    if request.path.starts_with("/backend-api/codex") {
        codex::stream(events)
    } else {
        Reply::Events(events)
    }
}

#[test]
fn a_failed_provider_output_is_dispatched_again_in_a_fresh_session() {
    let project = Project::new(&[("notes", agent("openai"))]);
    type Failure = fn(&Request) -> Reply;
    let failures: [(Failure, &str); 3] = [
        (
            |request| capped(request, &degenerate_deltas(), 128_000),
            "DEGENERATE_OUTPUT",
        ),
        (
            |request| capped(request, &[r#"{"verdict":"GR"#.to_owned()], 128),
            "PROVIDER_ERROR",
        ),
        (interrupted, "TRANSIENT"),
    ];
    for (failure, code) in failures {
        let failed = std::sync::Mutex::new(false);
        let provider = FakeProvider::start(move |request| {
            if !std::mem::replace(&mut *failed.lock().unwrap(), true) {
                return failure(request);
            }
            pass(request)
        });
        let run = project.run(&provider, &["--all", "--force"], 0);
        let review = request(&run, "notes/review");
        assert_eq!(review["status"], "GREEN", "{code}: {run}");
        assert_eq!(
            attempts(review),
            [
                (1, 1, json!(code)),
                (1, 1, Value::Null),
                (2, 1, Value::Null)
            ],
            "{code}: the failed dispatch's attempt is kept"
        );
        assert_eq!(dispatches(review), [None, Some(2), Some(2)], "{code}");
        let redispatches = review["redispatches"].as_array().unwrap();
        assert_eq!(redispatches.len(), 1, "{code}: {review}");
        assert_eq!(redispatches[0]["errorCode"], code);
        assert_ne!(redispatches[0]["sessionId"], review["sessionId"], "{code}");
        assert_eq!(
            session_attempts(&project, &redispatches[0]["sessionId"]).len(),
            1,
            "{code}"
        );
        let calls = provider.requests();
        assert_eq!(calls.len(), 3, "{code}");
        // The new dispatch starts the conversation over rather than continuing it.
        assert!(!replayed(&calls[1]), "{code}");
        assert_eq!(calls[0].body["input"], calls[1].body["input"], "{code}");
    }

    // With no redispatches, the failed output ends the review as before.
    fs::write(
        project.state.join("limits.json"),
        r#"{"agentRedispatches":0}"#,
    )
    .unwrap();
    let failed = std::sync::Mutex::new(false);
    let provider = FakeProvider::start(move |request| {
        if !std::mem::replace(&mut *failed.lock().unwrap(), true) {
            return capped(request, &degenerate_deltas(), 128_000);
        }
        pass(request)
    });
    let run = project.run(&provider, &["--all", "--force"], 2);
    let review = request(&run, "notes/review");
    assert_eq!(review["errorCode"], "DEGENERATE_OUTPUT", "{review}");
    assert_eq!(attempts(review), [(1, 1, json!("DEGENERATE_OUTPUT"))]);
    assert!(review.get("redispatches").is_none(), "{review}");
    assert_eq!(provider.requests().len(), 1);
}

#[test]
fn an_authentication_failure_stops_new_reviews_on_that_backend_only() {
    let project = Project::new(&[
        ("a", agent("openai")),
        ("b", agent("openai")),
        ("c", agent("openai")),
        ("d", agent("anthropic")),
    ]);
    // c has a result from an earlier Run, which stays reusable.
    let working = FakeProvider::start(pass);
    project.run(&working, &["c"], 0);

    let models = Arc::new(AtomicUsize::new(0));
    let counted = models.clone();
    let provider = FakeProvider::start(move |request| {
        if request.path == "/v1/responses" {
            counted.fetch_add(1, Ordering::SeqCst);
            return error(
                401,
                vec![],
                json!({"error":{"message":"Incorrect API key provided","code":"invalid_api_key"}}),
            );
        }
        pass(request)
    });
    let run = project.run(&provider, &["--all", "--jobs", "1"], 2);
    assert_eq!(
        models.load(Ordering::SeqCst),
        1,
        "only the first openai review ran"
    );
    assert_eq!(request(&run, "a/review")["errorCode"], "AUTHENTICATION");
    let skipped = request(&run, "b/review");
    assert_eq!(skipped["status"], "ERROR");
    assert_eq!(skipped["errorCode"], "BACKEND_STOPPED");
    assert!(
        skipped["error"].as_str().unwrap().starts_with(
            "Not started: this Run stopped admitting openai reviews after AUTHENTICATION in a/review: Incorrect API key provided"
        ),
        "{skipped}"
    );
    assert!(skipped["executionId"].is_null());
    assert_eq!(request(&run, "c/review")["status"], "GREEN", "reused");
    assert_eq!(
        request(&run, "d/review")["status"],
        "GREEN",
        "anthropic still runs"
    );
    assert_eq!(
        run["stoppedBackends"],
        json!([{"backend":"openai","errorCode":"AUTHENTICATION","evalId":"a/review",
            "requestId":request(&run, "a/review")["id"],"error":"Incorrect API key provided"}])
    );
    let shown = Command::new(support::os::cli())
        .arg("--state-dir")
        .arg(&project.state)
        .args(["run", "show", run["id"].as_str().unwrap()])
        .record_output()
        .unwrap();
    let shown: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(shown["stoppedBackends"], run["stoppedBackends"]);
    assert_eq!(request(&shown, "b/review")["errorCode"], "BACKEND_STOPPED");

    // The text report names the stop; a new Run admits the backend again.
    let text = project.verify(&provider, &["--all", "--jobs", "1"], false);
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(
        text.contains(
            "Stopped backend openai after AUTHENTICATION in a/review: 1 review not started."
        ),
        "{text}"
    );
    assert_eq!(models.load(Ordering::SeqCst), 2);
}

/// Cargo's published path can be replaced while another fixture's caller and worker run.
/// A private candidate must keep serving later Runs, including their durable self-spawn.
#[test]
fn an_owned_cli_keeps_verifying_when_its_published_source_is_replaced() {
    let root = support::os::tempdir();
    let source = root.path().join(support::os::cli().file_name().unwrap());
    fs::copy(support::os::cli(), &source).unwrap();
    let candidate = support::os::OwnedExecutable::copy_from(&source).unwrap();
    let project = Project::new(&[("notes", agent("openai"))]);
    let release = support::Gate::default();
    let (arrived, admitted) = std::sync::mpsc::sync_channel(1);
    let provider = FakeProvider::start({
        let release = release.clone();
        move |request| {
            if !replayed(request) {
                arrived.send(()).unwrap();
                release.wait();
            }
            pass(request)
        }
    });
    thread::scope(|scope| {
        let running =
            scope.spawn(|| project.verify_with_cli(candidate.path(), &provider, &["--all"], true));
        // The provider request proves the real durable worker is alive, not just admitted.
        // Release on assertion unwind as well, before the scoped caller is joined.
        struct ReleaseOnExit(support::Gate);
        impl Drop for ReleaseOnExit {
            fn drop(&mut self) {
                self.0.open();
            }
        }
        let release_on_exit = ReleaseOnExit(release.clone());
        admitted
            .recv_timeout(support::os::patience(Duration::from_secs(20)))
            .expect("the candidate's durable worker reached the provider");
        fs::remove_file(&source).unwrap();
        fs::write(&source, "replacement is deliberately not the declared CLI").unwrap();
        release.open();
        drop(release_on_exit);
        let output = running.join().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let run: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(request(&run, "notes/review")["status"], "GREEN");
    });
    let working = FakeProvider::start(pass);
    let output = project.verify_with_cli(candidate.path(), &working, &["--all", "--force"], true);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let run: Value = serde_json::from_slice(&output.stdout).unwrap();
    let review = request(&run, "notes/review");
    assert_eq!(review["status"], "GREEN");
    assert!(review["executionId"].is_string(), "a new worker executed");
    let directory = candidate.path().parent().unwrap().to_owned();
    // A terminal receipt can precede worker teardown; release the Run owner before its
    // executable, including on Windows where a running executable cannot be unlinked.
    drop(project);
    drop(candidate);
    assert!(!directory.exists(), "the scoped candidate was cleaned up");
}

#[test]
fn the_process_owned_cli_is_removed_on_success_and_test_unwind() {
    const RECORD: &str = "ARTIFACTIZE_TEST_CLI_DIRECTORY_RECORD";
    const PANIC: &str = "ARTIFACTIZE_TEST_CLI_DIRECTORY_PANIC";
    let root = support::os::tempdir();
    for mode in ["success", "unwind", "exit"] {
        let record = root.path().join(format!("candidate-{mode}"));
        let mut command = support::os::test_command(
            "owned_cli_exit_probe",
            &[
                (RECORD, record.as_os_str().to_owned()),
                (PANIC, mode.into()),
            ],
        );
        command.arg("--ignored");
        let output = support::os::finish(support::os::start(&mut command), "owned CLI exit");
        assert_eq!(
            output.status.success(),
            mode != "unwind",
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let directory = PathBuf::from(fs::read_to_string(record).unwrap());
        assert!(!directory.exists(), "the process-owned CLI was cleaned up");
    }
}

#[test]
#[ignore = "launched by the owned CLI exit lifecycle test"]
fn owned_cli_exit_probe() {
    let executable = support::os::cli();
    assert!(
        support::os::run(Command::new(&executable).arg("--version"))
            .status
            .success(),
        "the published private CLI is executable"
    );
    fs::write(
        support::os::environment_variable("ARTIFACTIZE_TEST_CLI_DIRECTORY_RECORD").unwrap(),
        support::os::path_text(executable.parent().unwrap()),
    )
    .unwrap();
    match support::os::environment_variable("ARTIFACTIZE_TEST_CLI_DIRECTORY_PANIC")
        .unwrap()
        .into_string()
        .unwrap()
        .as_str()
    {
        "success" => {}
        "unwind" => panic!("scripted test unwind"),
        "exit" => std::process::exit(0),
        mode => panic!("unknown CLI exit mode: {mode}"),
    }
}

#[test]
fn a_codex_usage_limit_stops_codex_for_the_run() {
    let project = Project::new(&[("a", agent("codex")), ("b", agent("codex"))]);
    let provider = FakeProvider::start(|request| {
        if request.path.starts_with("/backend-api/codex") {
            return error(
                429,
                vec![],
                json!({
                    "error":{
                        "type":"usage_limit_reached",
                        "message":"The usage limit has been reached",
                        "plan_type":"plus",
                        "resets_at":codex::VALID_EXPIRY,
                    },
                }),
            );
        }
        pass(request)
    });
    let run = project.run(&provider, &["--all", "--jobs", "1"], 2);
    assert_eq!(request(&run, "a/review")["errorCode"], "QUOTA");
    assert_eq!(request(&run, "b/review")["errorCode"], "BACKEND_STOPPED");
    assert_eq!(run["stoppedBackends"][0]["backend"], "codex");
    assert_eq!(run["stoppedBackends"][0]["errorCode"], "QUOTA");
    assert_eq!(provider.requests().len(), 1);
}

#[test]
fn a_slot_waiter_on_a_stopped_backend_is_not_started_and_takes_no_slot() {
    let project = Project::new(&[
        ("a", agent("openai")),
        ("b", agent("openai")),
        ("c", agent("anthropic")),
    ]);
    // One openai review at a time on this machine (limits.json).
    fs::write(
        project.state.join("limits.json"),
        json!({"backends":{"openai":1}}).to_string(),
    )
    .unwrap();
    let release = support::Gate::default();
    let models = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider::start({
        let (release, models) = (release.clone(), models.clone());
        move |request| {
            if request.path != "/v1/responses" {
                return pass(request);
            }
            models.fetch_add(1, Ordering::SeqCst);
            // Hold the only slot until the test has seen b wait for it.
            release.wait();
            error(
                401,
                vec![],
                json!({"error":{"message":"Incorrect API key provided","code":"invalid_api_key"}}),
            )
        }
    });
    let mut command = Command::new(support::os::cli());
    command
        .arg("--repo")
        .arg(&project.repo)
        .arg("--state-dir")
        .arg(&project.state)
        .args(["verify", "--wait", "--all", "--jobs", "3", "--json"])
        .env("ARTIFACTIZE_REMOTE", "off")
        .env("OPENAI_API_KEY", "fake-openai-key")
        .env("ANTHROPIC_API_KEY", "fake-anthropic-key")
        .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
        .env("ARTIFACTIZE_ANTHROPIC_BASE_URL", provider.anthropic_base());
    let child = Child::new(
        support::os::start(&mut command),
        &project.state,
        &project.repo,
    );
    let database = || {
        rusqlite::Connection::open_with_flags(
            project.state.join("state.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .ok()
    };
    let waiting_for_slot = || {
        database().is_some_and(|db| {
            db.query_row(
                "SELECT json_extract(data,'$.blockedReason') FROM requests WHERE run_id=? AND eval_id='b/review'",
                [child.run()],
                |row| row.get::<_, Option<String>>(0),
            )
            .is_ok_and(|reason| reason.is_some_and(|reason| reason.starts_with("Waiting for a free openai slot")))
        })
    };
    support::os::wait_until(Duration::from_secs(20), waiting_for_slot);
    release.open();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(request(&run, "a/review")["errorCode"], "AUTHENTICATION");
    // The waiter stops waiting once openai stops, without ever taking the slot: a records
    // the stop before it frees the slot, whatever the scheduler is doing at that moment.
    let skipped = request(&run, "b/review");
    assert_eq!(skipped["errorCode"], "BACKEND_STOPPED", "{run}");
    assert!(skipped["blockedReason"].is_null());
    assert!(skipped["executionId"].is_null());
    assert_eq!(request(&run, "c/review")["status"], "GREEN");
    assert_eq!(models.load(Ordering::SeqCst), 1);
    // A fresh Run can admit both openai reviews under the same one-slot limit. This proves
    // capacity was released through the public execution seam, not private row counts.
    let working = FakeProvider::start(pass);
    let next = project.run(&working, &["--all", "--force"], 0);
    for eval in ["a/review", "b/review", "c/review"] {
        assert_eq!(request(&next, eval)["status"], "GREEN", "{next}");
    }
}

#[tokio::test(start_paused = true)]
async fn retry_after_waits_for_the_declared_virtual_deadline_before_replaying() {
    use artifactize_core::llm::{Client, Turn};
    use rig_core::{
        completion::CompletionRequest,
        message::Message,
        providers::openai::OpenAIConfig,
        test_utils::{MockHttpResponse, SequencedHttpClient},
    };
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("retry-after-ms", "700".parse().unwrap());
    let transport = SequencedHttpClient::new([
        MockHttpResponse::error_with_headers(
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"Rate limit reached","code":"rate_limit_exceeded"}}"#,
            headers,
        ),
        MockHttpResponse::error(
            reqwest::StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"Invalid key","code":"invalid_api_key"}}"#,
        ),
    ]);
    let captured = transport.clone();
    let client = Client::Openai(
        Box::new(
            OpenAIConfig::new("fixture")
                .connect(transport)
                .responses("fixture-model"),
        ),
        "fixture".into(),
    );
    let mut running = Box::pin(async move {
        let cancellation = tokio_util::sync::CancellationToken::new();
        let mut attempts = Vec::new();
        let result = client
            .turn(
                &CompletionRequest::new(Message::user("literal replay")),
                Turn {
                    number: 1,
                    model: "fixture-model",
                    deadline: tokio::time::Instant::now() + Duration::from_secs(5),
                    cancellation: &cancellation,
                },
                &mut attempts,
            )
            .await;
        (result, attempts)
    });
    // Poll the public turn future to its actual pending retry, not a scheduler yield count.
    assert!(futures_util::poll!(&mut running).is_pending());
    assert_eq!(captured.requests().len(), 1);
    tokio::time::advance(Duration::from_millis(699)).await;
    assert!(futures_util::poll!(&mut running).is_pending());
    assert_eq!(captured.requests().len(), 1, "retry ran before Retry-After");
    tokio::time::advance(Duration::from_millis(1)).await;
    let std::task::Poll::Ready((_, attempts)) = futures_util::poll!(&mut running) else {
        panic!("retry did not finish at the declared virtual deadline");
    };
    assert_eq!(attempts.len(), 2);
    let calls = captured.requests();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].body, calls[1].body);
}

#[tokio::test(start_paused = true)]
async fn retry_after_past_deadline_finishes_without_advancing_the_clock() {
    use artifactize_core::llm::{Client, Turn};
    use rig_core::{
        completion::CompletionRequest,
        message::Message,
        providers::openai::OpenAIConfig,
        test_utils::{MockHttpResponse, SequencedHttpClient},
    };
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("retry-after", "120".parse().unwrap());
    let transport = SequencedHttpClient::new([MockHttpResponse::error_with_headers(
        reqwest::StatusCode::TOO_MANY_REQUESTS,
        r#"{"error":{"message":"Rate limit reached","code":"rate_limit_exceeded"}}"#,
        headers,
    )]);
    let captured = transport.clone();
    let client = Client::Openai(
        Box::new(
            OpenAIConfig::new("fixture")
                .connect(transport)
                .responses("fixture-model"),
        ),
        "fixture".into(),
    );
    let mut running = Box::pin(async move {
        let cancellation = tokio_util::sync::CancellationToken::new();
        client
            .turn(
                &CompletionRequest::new(Message::user("fixture")),
                Turn {
                    number: 1,
                    model: "fixture-model",
                    deadline: tokio::time::Instant::now() + Duration::from_secs(5),
                    cancellation: &cancellation,
                },
                &mut Vec::new(),
            )
            .await
    });
    let std::task::Poll::Ready(result) = futures_util::poll!(&mut running) else {
        panic!("an impossible retry waited instead of failing immediately");
    };
    let error = result.unwrap_err();
    assert!(
        error.message.contains("past the review deadline"),
        "{error:?}"
    );
    assert_eq!(captured.requests().len(), 1);
}

/// Every piece of `secret` worth checking for: the whole and its eight-character windows.
fn pieces(secret: &str) -> Vec<&str> {
    (0..=secret.len().saturating_sub(8))
        .step_by(4)
        .map(|start| &secret[start..(start + 8).min(secret.len())])
        .collect()
}

/// Files below `directory`, recursively.
fn files(directory: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                files(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

/// A provider that echoes the credential it was sent (the Authorization header, or
/// Anthropic's x-api-key) in its error: at the request's open (HTTP 400) or inside the
/// stream. The error leaves a review through the shared rule for server text: withheld from
/// verify's output, its attempt records and the saved Agent sessions.
#[test]
fn an_agent_error_echoing_the_credential_withholds_it_everywhere() {
    let access = codex::jwt("account-1", codex::VALID_EXPIRY);
    let credentials = [
        ("openai", "fake-openai-key".to_owned()),
        ("anthropic", "fake-anthropic-key".to_owned()),
        ("codex", access.clone()),
    ];
    for in_stream in [false, true] {
        let project = Project::new(&[
            ("open", agent("openai")),
            ("claude", agent("anthropic")),
            ("luna", agent("codex")),
        ]);
        let provider = FakeProvider::start(move |request| {
            let sent = request
                .headers
                .get("x-api-key")
                .or_else(|| request.headers.get("authorization"))
                .cloned()
                .unwrap_or_default();
            // Split by a newline, which dropping control characters would rejoin.
            let (head, tail) = sent.split_at(sent.len() / 2);
            let message = format!("Bad credential {head}\n{tail}");
            let anthropic = request.path == "/v1/messages";
            if !in_stream {
                let body = if anthropic {
                    json!({"type":"error","error":{"type":"invalid_request_error","message":message}})
                } else {
                    json!({"error":{"message":message,"code":"bad_request"}})
                };
                return error(400, vec![], body);
            }
            if anthropic {
                Reply::Events(vec![json!({
                    "type":"error",
                    "error":{"type":"invalid_request_error","message":message},
                })])
            } else {
                let failed = json!({"type":"error","code":"bad_request","message":message,"param":null,"sequence_number":0});
                if request.path.starts_with("/backend-api/codex") {
                    codex::stream(vec![failed])
                } else {
                    Reply::Events(vec![failed])
                }
            }
        });
        let output = project.verify(&provider, &["--all", "--jobs", "1"], true);
        let shown = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let saved: String = files(&project.state.join("agent-sessions"))
            .iter()
            .map(|path| String::from_utf8_lossy(&fs::read(path).unwrap()).into_owned())
            .collect();
        assert!(!saved.is_empty(), "the Agent sessions were saved");
        for (backend, credential) in &credentials {
            for piece in pieces(credential) {
                assert!(
                    !shown.contains(piece),
                    "{backend} credential in output (in stream: {in_stream}): {shown}"
                );
                assert!(
                    !saved.contains(piece),
                    "{backend} credential in a saved session (in stream: {in_stream})"
                );
            }
        }
        let run: Value = serde_json::from_slice(&output.stdout).unwrap();
        for eval in ["open/review", "claude/review", "luna/review"] {
            let review = request(&run, eval);
            assert!(
                review["error"]
                    .as_str()
                    .is_some_and(|error| error.contains("[withheld")),
                "{eval} (in stream: {in_stream}): {review}"
            );
            for attempt in review["usage"].as_array().unwrap() {
                assert!(
                    attempt["error"]
                        .as_str()
                        .is_some_and(|error| error.contains("[withheld")),
                    "{eval}: {attempt}"
                );
            }
        }
    }
}
