//! Agent turn retries, error codes and stopping a backend, against loopback fakes.

mod support;

use std::{
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use support::{FakeProvider, Reply, Request, anthropic, codex, openai};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A workspace of Artifacts, each with one Agent eval and its own fingerprint.
struct Project {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Project {
    fn new(evals: &[(&str, Value)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        for (name, profile) in evals {
            let folder = repo.join(name);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("notes.md"), format!("{name}: R1 holds.\n")).unwrap();
            fs::write(
                folder.join("artifactize.json"),
                json!({"name":name,"fingerprint":{},
                    "views":{"agentTools":{"read":{"builtin":"read"}}},
                    "evals":[{"id":"review","title":"Review","profile":profile,
                        "payload":{"instruction":format!("Review {{{name}}}.")}}]})
                .to_string(),
            )
            .unwrap();
        }
        let project = Self {
            state: root.path().join("state"),
            repo,
            _root: root,
        };
        // A Codex sign-in, for the codex evals.
        let auth = project.state.join("auth");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&auth)
            .unwrap();
        let credentials = json!({"access_token":codex::jwt("account-1", now() + 3600),
            "refresh_token":"refresh","account_id":"account-1","expires_at":now() + 3600,"saved_at":1});
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(auth.join("codex.json"))
            .unwrap();
        fs::write(auth.join("codex.json"), credentials.to_string()).unwrap();
        project
    }

    fn verify(&self, provider: &FakeProvider, args: &[&str], json: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .arg("verify")
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
        command.output().unwrap()
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
    json!({"kind":"agent","backend":backend,"model":format!("{backend}-model"),"timeoutMs":20000})
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
fn a_later_turn_retries_transient_failures_and_honours_retry_after() {
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
                json!({"error":{"message":"Rate limit reached","type":"requests","code":"rate_limit_exceeded"}}),
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
    // Each retry replays the same conversation; the last waited as Retry-After asked.
    assert_eq!(calls[1].body, calls[3].body);
    let waited = calls[3].received - calls[2].received;
    assert!(waited >= Duration::from_millis(700), "{waited:?}");
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
    profile["timeoutMs"] = json!(5000);
    let project = Project::new(&[("notes", profile)]);
    let provider = FakeProvider::start(|_| {
        error(
            429,
            vec![("retry-after", "120".into())],
            json!({"error":{"message":"Rate limit reached","code":"rate_limit_exceeded"}}),
        )
    });
    let started = Instant::now();
    let run = project.run(&provider, &["--all"], 2);
    assert!(started.elapsed() < Duration::from_secs(5));
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
                json!({"error":{"message":"You exceeded your current quota","code":"insufficient_quota"}}),
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
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--repo")
        .arg(&project.repo)
        .arg("--state-dir")
        .arg(&project.state)
        .args(["verify", "--all", "--force", "--json"])
        .env("ARTIFACTIZE_REMOTE", "off")
        .env("ARTIFACTIZE_OPENAI_BASE_URL", invalid.openai_base())
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    let run: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(request(&run, "notes/review")["errorCode"], "AUTHENTICATION");
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
        skipped["error"]
            .as_str()
            .unwrap()
            .starts_with("Not started: this Run stopped admitting openai reviews after AUTHENTICATION in a/review: Incorrect API key provided"),
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
    let shown = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--state-dir")
        .arg(&project.state)
        .args(["run", "show", run["id"].as_str().unwrap()])
        .output()
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

#[test]
fn a_codex_usage_limit_stops_codex_for_the_run() {
    let project = Project::new(&[("a", agent("codex")), ("b", agent("codex"))]);
    let provider = FakeProvider::start(|request| {
        if request.path.starts_with("/backend-api/codex") {
            return error(
                429,
                vec![],
                json!({"error":{"type":"usage_limit_reached","message":"The usage limit has been reached","plan_type":"plus","resets_at":now() + 3600}}),
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
    let release = Arc::new(AtomicBool::new(false));
    let models = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider::start({
        let (release, models) = (release.clone(), models.clone());
        move |request| {
            if request.path != "/v1/responses" {
                return pass(request);
            }
            models.fetch_add(1, Ordering::SeqCst);
            // Hold the only slot until the test has seen b wait for it.
            let deadline = Instant::now() + Duration::from_secs(20);
            while !release.load(Ordering::SeqCst) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            error(
                401,
                vec![],
                json!({"error":{"message":"Incorrect API key provided","code":"invalid_api_key"}}),
            )
        }
    });
    let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
    command
        .arg("--repo")
        .arg(&project.repo)
        .arg("--state-dir")
        .arg(&project.state)
        .args(["verify", "--all", "--jobs", "3", "--json"])
        .env("ARTIFACTIZE_REMOTE", "off")
        .env("OPENAI_API_KEY", "fake-openai-key")
        .env("ANTHROPIC_API_KEY", "fake-anthropic-key")
        .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
        .env("ARTIFACTIZE_ANTHROPIC_BASE_URL", provider.anthropic_base())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().unwrap();
    let database = || rusqlite::Connection::open(project.state.join("state.sqlite")).ok();
    let waiting_for_slot = || {
        database().is_some_and(|db| {
            db.query_row(
                "SELECT count(*) FROM requests WHERE json_extract(data,'$.evalId')='b/review' AND json_extract(data,'$.blockedReason') LIKE 'Waiting for a free openai slot%'",
                [],
                |row| row.get::<_, u32>(0),
            )
            .is_ok_and(|count| count == 1)
        })
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while !waiting_for_slot() {
        assert!(
            Instant::now() < deadline,
            "b never waited for the openai slot"
        );
        thread::sleep(Duration::from_millis(20));
    }
    release.store(true, Ordering::SeqCst);
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
    let slots: u32 = database()
        .unwrap()
        .query_row("SELECT count(*) FROM backend_slots", [], |row| row.get(0))
        .unwrap();
    assert_eq!(slots, 0);
}
