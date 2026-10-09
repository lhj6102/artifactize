//! One prompt-cache identity per Agent review, against loopback fakes.

mod support;

use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use support::{FakeProvider, Reply, Request, anthropic, codex, openai};

/// A workspace of Artifacts, each with one Agent eval whose result must list `covered`.
struct Project {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Project {
    fn new(evals: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        for (name, backend) in evals {
            let folder = repo.join(name);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("notes.md"), format!("{name}: R1 holds.\n")).unwrap();
            let profile = json!({"kind":"agent","backend":backend,
                "model":format!("{backend}-model"),"timeout_ms":20000});
            support::declaration::write(
                folder.join("index.artf"),
                json!({"name":name,"views":{"agent_tools":{"read":{"builtin":"read"}}},
                    "evals":[{"id":"review","title":"Review","profile":profile,
                        "payload":{"instruction":format!("Review {{{name}}}.")},
                        "pass_schema":{"type":"object","required":["covered"],
                            "properties":{"covered":{"type":"array","items":{"type":"string"}}}}}]})
                .to_string(),
            )
            .unwrap();
        }
        let state = root.path().join("state");
        let auth = state.join("auth");
        support::os::create_private_dir_all(&auth);
        let expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let credentials = json!({"access_token":codex::jwt("account-1", expires),
            "refresh_token":"refresh","account_id":"account-1","expires_at":expires,"saved_at":1});
        support::os::write_private_file(&auth.join("codex.json"), credentials.to_string());
        Self {
            _root: root,
            repo,
            state,
        }
    }

    fn json(&self, provider: &FakeProvider, args: &[&str], code: i32) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .arg("--json")
            .env("ARTIFACTIZE_REMOTE", "off")
            .env("OPENAI_API_KEY", "fake-openai-key")
            .env("ANTHROPIC_API_KEY", "fake-anthropic-key")
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
            .env("ARTIFACTIZE_ANTHROPIC_BASE_URL", provider.anthropic_base())
            .env("ARTIFACTIZE_CODEX_BASE_URL", provider.codex_base())
            .env("ARTIFACTIZE_CODEX_AUTH_URL", &provider.url)
            .env_remove("ARTIFACTIZE_CODEX_AUTH_FILE")
            .output()
            .unwrap();
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

fn replayed(request: &Request) -> bool {
    request.body["input"].as_array().is_some_and(|input| {
        input
            .iter()
            .any(|item| item["type"] == "function_call_output")
    })
}

fn repairing(request: &Request) -> bool {
    request.body["input"]
        .as_array()
        .and_then(|input| input.last()?["content"][0]["text"].as_str())
        .is_some_and(|text| text.starts_with("Your final response did not match"))
}

/// Read the notes, answer without `covered`, then repair; Anthropic passes at once.
fn reply(request: &Request) -> Reply {
    if request.path == "/v1/messages" {
        return anthropic::text(request, r#"{"verdict":"GREEN","covered":["R1"]}"#, 10, 2);
    }
    let output = if repairing(request) {
        vec![openai::message(r#"{"verdict":"GREEN","covered":["R1"]}"#)]
    } else if replayed(request) {
        vec![openai::message(r#"{"verdict":"GREEN"}"#)]
    } else {
        let tool = request.body["tools"][0]["name"].as_str().unwrap();
        vec![openai::function_call(
            "call_1",
            tool,
            &json!({"path":"notes.md"}),
        )]
    };
    if request.path.starts_with("/backend-api/codex") {
        codex::completed(request, output, openai::usage(10, 2))
    } else {
        openai::completed(request, output, openai::usage(10, 2))
    }
}

fn attempts(review: &Value) -> Vec<(u64, u64, Value)> {
    review["usage"]
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

#[test]
fn every_request_of_a_review_shares_the_saved_session_id() {
    for backend in ["openai", "codex"] {
        let project = Project::new(&[("notes", backend)]);
        // The second turn fails once before any output, and is retried.
        let failed = Mutex::new(false);
        let provider = FakeProvider::start(move |request| {
            if replayed(request)
                && !repairing(request)
                && !std::mem::replace(&mut *failed.lock().unwrap(), true)
            {
                return Reply::Json(
                    503,
                    json!({"error":{"message":"Service unavailable","code":"server_error"}}),
                );
            }
            reply(request)
        });
        let run = project.json(&provider, &["verify", "--all"], 0);
        let review = &run["requests"][0];
        assert_eq!(review["status"], "GREEN", "{run}");
        assert_eq!(
            attempts(review),
            [
                (1, 1, Value::Null),
                (2, 1, json!("TRANSIENT")),
                (2, 2, Value::Null),
                (3, 1, Value::Null)
            ],
            "{backend}"
        );
        let session = review["sessionId"].as_str().unwrap();
        assert_eq!(session.len(), 36, "{session}");

        // The tool turn, the failed attempt, its retry and the repair turn.
        let calls = provider.requests();
        assert_eq!(calls.len(), 4, "{backend}");
        assert!(repairing(&calls[3]));
        for call in &calls {
            assert_eq!(call.body["prompt_cache_key"], session, "{backend}");
            if backend == "codex" {
                assert_eq!(call.headers["session-id"], session);
                assert!(
                    !call.headers.contains_key("session_id"),
                    "{:?}",
                    call.headers
                );
            } else {
                assert!(!call.headers.contains_key("session-id"));
            }
        }

        // The saved request shows it.
        let id = review["id"].as_str().unwrap();
        let shown = project.json(&provider, &["run", "show", run["id"].as_str().unwrap()], 0);
        assert_eq!(shown["requests"][0]["sessionId"], session);
        let shown = project.json(&provider, &["request", "show", id], 0);
        assert_eq!(shown["sessionId"], session);
    }
}

#[test]
fn each_review_has_its_own_session_id() {
    let project = Project::new(&[
        ("a", "codex"),
        ("b", "codex"),
        ("c", "openai"),
        ("d", "anthropic"),
    ]);
    let provider = FakeProvider::start(reply);
    let run = project.json(&provider, &["verify", "--all"], 0);
    let sessions: Vec<_> = run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .map(|review| {
            assert_eq!(review["status"], "GREEN", "{run}");
            review["sessionId"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(
        sessions
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
        "{sessions:?}"
    );

    // Each Responses request carries its own review's session; Anthropic gets none.
    let calls = provider.requests();
    let session_of = |artifact: &str| {
        let review = run["requests"]
            .as_array()
            .unwrap()
            .iter()
            .find(|review| review["evalId"] == format!("{artifact}/review"))
            .unwrap();
        review["sessionId"].as_str().unwrap().to_owned()
    };
    for call in &calls {
        if call.path == "/v1/messages" {
            assert!(call.body.get("prompt_cache_key").is_none());
            assert!(!call.headers.contains_key("session-id"));
            continue;
        }
        // Its tool, offered or called earlier, names the review's Artifact.
        let body = call.body.to_string();
        let artifact = ["a", "b", "c"]
            .into_iter()
            .find(|name| body.contains(&format!("\"read_{name}\"")))
            .unwrap_or_else(|| panic!("{body}"));
        let session = session_of(artifact);
        assert_eq!(call.body["prompt_cache_key"], session);
        if call.path.starts_with("/backend-api/codex") {
            assert_eq!(call.headers["session-id"], session);
        }
    }

    // A forced re-review starts a new session.
    let rerun = project.json(&provider, &["verify", "--all", "--force"], 0);
    let rerun = rerun["requests"].as_array().unwrap();
    for review in rerun {
        assert!(!sessions.contains(&review["sessionId"].as_str().unwrap().to_owned()));
    }
}
