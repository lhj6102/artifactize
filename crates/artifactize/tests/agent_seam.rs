//! The loopback test endpoints that point Agent backends at a fake provider.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};
use support::{FakeProvider, Reply, anthropic, openai};

struct Project {
    _root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Project {
    /// One `spec` Artifact with a built-in `read` tool and one Agent eval.
    fn new(profile: Value) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("spec")).unwrap();
        fs::write(repo.join("spec/spec.md"), "R1: the spec covers R1.\n").unwrap();
        fs::write(
            repo.join("spec/artifactize.json"),
            json!({"name":"spec","views":{"agentTools":{"read":{"builtin":"read"}}},
                "evals":[{"id":"review","title":"Review","profile":profile,
                    "payload":{"instruction":"Review {spec}."},
                    "passSchema":{"type":"object","properties":{"covered":{"type":"array","items":{"type":"string"}}},"required":["covered"]}}]})
            .to_string(),
        )
        .unwrap();
        Self {
            state: root.path().join("state"),
            repo,
            _root: root,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .arg("--json")
            .env("ARTIFACTIZE_REMOTE", "off")
            .env("OPENAI_API_KEY", "fake-openai-key")
            .env("ANTHROPIC_API_KEY", "fake-anthropic-key")
            .env_remove("ARTIFACTIZE_OPENAI_BASE_URL")
            .env_remove("ARTIFACTIZE_ANTHROPIC_BASE_URL");
        command
    }
}

fn parsed(output: Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn check<'a>(report: &'a Value, name: &str) -> &'a Value {
    let checks = report["checks"].as_array().unwrap();
    checks.iter().find(|check| check["name"] == name).unwrap()
}

#[test]
fn openai_review_calls_tools_against_the_fake_and_passes_reasoning_exactly() {
    let project = Project::new(
        json!({"kind":"agent","backend":"openai","model":"fake-model","reasoning":"max","maxToolCalls":2}),
    );
    let provider = FakeProvider::start(|request| {
        let replayed = request.body["input"].as_array().is_some_and(|input| {
            input
                .iter()
                .any(|item| item["type"] == "function_call_output")
        });
        if replayed {
            let verdict = json!({"verdict":"GREEN","covered":["R1"]});
            openai::completed(
                request,
                vec![openai::message(&verdict.to_string())],
                openai::usage(40, 8),
            )
        } else {
            let call = openai::function_call("call_1", "read_spec", &json!({"path":"spec.md"}));
            openai::completed(request, vec![call], openai::usage(30, 5))
        }
    });
    let run = parsed(
        project
            .command(&["verify", "--all"])
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
            .output()
            .unwrap(),
        0,
    );
    let review = &run["requests"][0];
    assert_eq!(review["status"], "GREEN", "{run}");
    assert_eq!(
        review["result"],
        json!({"verdict":"GREEN","covered":["R1"]})
    );
    assert_eq!(review["toolCalls"][0]["name"], "read_spec");
    assert_eq!(review["toolCalls"][0]["isError"], false);
    let attempts = review["usage"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[1]["usage"]["inputTokens"], 40);

    let calls = provider.requests();
    assert_eq!(calls.len(), 2);
    for call in &calls {
        assert_eq!(
            (call.method.as_str(), call.path.as_str()),
            ("POST", "/v1/responses")
        );
        assert_eq!(call.headers["authorization"], "Bearer fake-openai-key");
        assert_eq!(call.body["model"], "fake-model");
        assert_eq!(call.body["reasoning"], json!({"effort":"max"}));
        assert_eq!(call.body["stream"], true);
        assert_eq!(call.body["tools"][0]["name"], "read_spec");
    }
    let output = calls[1].body["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "function_call_output")
        .unwrap();
    assert_eq!(output["call_id"], "call_1");
    assert!(
        output.to_string().contains("the spec covers R1"),
        "{output}"
    );
}

#[test]
fn anthropic_review_and_model_listings_use_their_test_endpoints() {
    let project = Project::new(
        json!({"kind":"agent","backend":"anthropic","model":"fake-claude","reasoning":"high"}),
    );
    let provider =
        FakeProvider::start(
            |request| match (request.method.as_str(), request.path.as_str()) {
                ("POST", "/v1/messages") => {
                    anthropic::text(request, &json!({"verdict":"RED"}).to_string(), 12, 3)
                }
                ("GET", "/v1/models") if request.headers.contains_key("x-api-key") => {
                    anthropic::models(&["fake-claude"])
                }
                ("GET", "/v1/models") => openai::models(&["fake-model"]),
                _ => Reply::Json(404, json!({"error":{"message":"not found"}})),
            },
        );
    let run = parsed(
        project
            .command(&["verify", "--all"])
            .env("ARTIFACTIZE_ANTHROPIC_BASE_URL", provider.anthropic_base())
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(run["requests"][0]["status"], "RED", "{run}");
    let review = &provider.requests()[0];
    assert_eq!(review.headers["x-api-key"], "fake-anthropic-key");
    assert!(!review.headers.contains_key("authorization"));
    assert_eq!(review.body["output_config"], json!({"effort":"high"}));

    for (backend, variable, base, model) in [
        (
            "anthropic",
            "ARTIFACTIZE_ANTHROPIC_BASE_URL",
            provider.anthropic_base(),
            "fake-claude",
        ),
        // The anthropic root also accepts the /v1 that API docs show.
        (
            "anthropic",
            "ARTIFACTIZE_ANTHROPIC_BASE_URL",
            format!("{}/v1/", provider.url),
            "fake-claude",
        ),
        (
            "openai",
            "ARTIFACTIZE_OPENAI_BASE_URL",
            provider.openai_base(),
            "fake-model",
        ),
    ] {
        let listing = parsed(
            project
                .command(&["models", backend])
                .env(variable, base)
                .output()
                .unwrap(),
            0,
        );
        assert_eq!(
            listing,
            json!({"backend":backend,"models":[{"slug":model,"display_name":model}]})
        );
    }
    let paths: Vec<_> = provider.requests().iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        paths,
        ["/v1/messages", "/v1/models", "/v1/models", "/v1/models"]
    );
}

#[test]
fn test_endpoints_must_stay_on_loopback() {
    let project = Project::new(json!({"kind":"agent","backend":"openai","model":"fake-model"}));
    // TEST-NET-1 is never routed; the endpoint is refused before any connection.
    for remote in [
        "http://192.0.2.1/v1",
        "https://api.openai.com/v1",
        "http://user:pw@127.0.0.1/v1",
    ] {
        let failure = parsed(
            project
                .command(&["models", "openai"])
                .env("ARTIFACTIZE_OPENAI_BASE_URL", remote)
                .output()
                .unwrap(),
            2,
        );
        let error = failure["error"].as_str().unwrap();
        assert!(
            error.starts_with("ARTIFACTIZE_OPENAI_BASE_URL: a test endpoint"),
            "{error}"
        );
        assert!(!error.contains("pw"), "{error}");

        let run = parsed(
            project
                .command(&["verify", "--all"])
                .env("ARTIFACTIZE_OPENAI_BASE_URL", remote)
                .output()
                .unwrap(),
            2,
        );
        assert_eq!(run["requests"][0]["status"], "ERROR");
        assert!(
            run["requests"][0]["error"]
                .as_str()
                .unwrap()
                .starts_with("ARTIFACTIZE_OPENAI_BASE_URL: ")
        );
        assert_eq!(run["requests"][0]["usage"], json!([]));

        let report = parsed(
            project
                .command(&["doctor"])
                .env("ARTIFACTIZE_OPENAI_BASE_URL", remote)
                .output()
                .unwrap(),
            1,
        );
        assert_eq!(check(&report, "openai")["status"], "FAIL");
    }
}

#[test]
fn doctor_warns_about_an_active_test_endpoint() {
    let project = Project::new(json!({"kind":"agent","backend":"openai","model":"fake-model"}));
    let report = parsed(project.command(&["doctor"]).output().unwrap(), 0);
    assert_eq!(check(&report, "openai")["status"], "PASS");
    assert_eq!(check(&report, "openai")["details"], json!({"present":true}));
    let report = parsed(
        project
            .command(&["doctor"])
            .env("ARTIFACTIZE_OPENAI_BASE_URL", "http://127.0.0.1:9/v1/")
            .output()
            .unwrap(),
        0,
    );
    let openai = check(&report, "openai");
    assert_eq!(openai["status"], "WARN");
    assert_eq!(
        openai["details"],
        json!({"present":true,"testEndpoint":"http://127.0.0.1:9/v1"})
    );
    assert!(
        openai["message"]
            .as_str()
            .unwrap()
            .contains("ARTIFACTIZE_OPENAI_BASE_URL")
    );
    assert!(!report.to_string().contains("fake-openai-key"));
    assert_eq!(check(&report, "anthropic")["status"], "PASS");
}

#[test]
fn fake_results_never_reach_a_review_store() {
    let project = Project::new(json!({"kind":"agent","backend":"openai","model":"fake-model"}));
    let provider = FakeProvider::script(Vec::new());
    let output = project
        .command(&["verify", "--all"])
        .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
        // Resolving the store is offline; the guard refuses before any lookup.
        .env("ARTIFACTIZE_REMOTE", "http://127.0.0.1:9/")
        .env("ARTIFACTIZE_REMOTE_TOKEN", "team-token")
        .output()
        .unwrap();
    let failure = parsed(output, 2);
    let error = failure["error"].as_str().unwrap();
    assert!(error.contains("ARTIFACTIZE_OPENAI_BASE_URL"), "{error}");
    assert!(error.contains("ARTIFACTIZE_REMOTE=off"), "{error}");
    assert!(provider.requests().is_empty());
    assert!(!state_has_runs(&project.state));
}

fn state_has_runs(state: &Path) -> bool {
    fs::read_dir(state.join("runs")).is_ok_and(|mut entries| entries.next().is_some())
}
