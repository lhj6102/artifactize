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
        support::declaration::write(
            repo.join("spec/index.artf"),
            json!({
                "name":"spec",
                "views":{"agent_tools":{"read":{"builtin":"read"}}},
                "evals":[
                    {
                        "id":"review",
                        "title":"Review",
                        "profile":profile,
                        "payload":{"instruction":"Review {spec}."},
                        "pass_schema":{
                            "type":"object",
                            "properties":{"covered":{"type":"array","items":{"type":"string"}}},
                            "required":["covered"],
                        },
                    },
                ],
            })
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
    let project = Project::new(json!({
        "kind":"agent",
        "backend":"openai",
        "model":"fake-model",
        "reasoning":"max",
        "max_tool_calls":2,
    }));
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
    // The review's tool calls live in its saved session, not in the request.
    assert!(review.get("toolCalls").is_none());
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

/// The text of the last user message in a Responses request.
fn last_prompt(request: &support::Request) -> String {
    let input = request.body["input"].as_array().unwrap();
    input.last().unwrap()["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn the_repair_turn_names_the_failing_schema_paths() {
    let project = Project::new(json!({"kind":"agent","backend":"openai","model":"fake-model"}));
    let reply = |request: &support::Request, verdict: Value| {
        openai::completed(
            request,
            vec![openai::message(&verdict.to_string())],
            openai::usage(10, 2),
        )
    };
    let repaired = FakeProvider::start(move |request| {
        if last_prompt(request).starts_with("Your final response did not match") {
            reply(request, json!({"verdict":"GREEN","covered":["R1"]}))
        } else {
            reply(request, json!({"verdict":"GREEN","covered":"R1"}))
        }
    });
    let run = parsed(
        project
            .command(&["verify", "--all"])
            .env("ARTIFACTIZE_OPENAI_BASE_URL", repaired.openai_base())
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(
        run["requests"][0]["result"],
        json!({"verdict":"GREEN","covered":["R1"]})
    );
    let calls = repaired.requests();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        last_prompt(&calls[1]),
        "Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema\n- instancePath \"/covered\": \"R1\" is not of type \"array\"\nReturn only one JSON object matching the schema."
    );

    // A second invalid result ends the review; the saved error stays the bare code.
    let stubborn = FakeProvider::start(move |request| {
        reply(request, json!({"verdict":"GREEN","covered":"R1"}))
    });
    let run = parsed(
        project
            .command(&["verify", "--all", "--force"])
            .env("ARTIFACTIZE_OPENAI_BASE_URL", stubborn.openai_base())
            .output()
            .unwrap(),
        2,
    );
    assert_eq!(
        run["requests"][0]["error"],
        "Invalid final Agent result after one format repair: schema_mismatch: result must match the selected verdict's owner schema."
    );
    assert_eq!(stubborn.requests().len(), 2);
}

#[test]
fn fixed_builtin_tools_have_closed_schemas_and_return_content_in_offline_agent_turns() {
    let project = Project::new(
        json!({"kind":"agent","backend":"openai","model":"fake-model","max_tool_calls":6}),
    );
    fs::write(
        project.repo.join("spec/spec.md"),
        "# First\nbody\n## Child\nchild\n# Next\nlast\n",
    )
    .unwrap();
    let stub = project.repo.join("spec/help-stub");
    fs::write(
        &stub,
        "#!/bin/sh\nprintf 'stub documentation: %s\\n' \"$*\"\n",
    )
    .unwrap();
    support::os::make_executable(&stub);
    let mut declaration =
        support::declaration::read(fs::read(project.repo.join("spec/index.artf")).unwrap())
            .unwrap();
    declaration["views"]["agent_tools"] = json!({
        "fixed":{"builtin":"read","args":["spec.md"]},
        "section":{"builtin":"section","args":["spec.md"]},
        "fixed_section":{"builtin":"section","args":["spec.md","First"]},
        "help":{"builtin":"help","args":["./help-stub","sub"]}
    });
    support::declaration::write(
        project.repo.join("spec/index.artf"),
        declaration.to_string(),
    )
    .unwrap();
    let provider = FakeProvider::start(|request| {
        let outputs: Vec<_> = request.body["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .collect();
        let turn = outputs.len();
        let call = match turn {
            0 => openai::function_call("fixed", "fixed_spec", &json!({})),
            1 => openai::function_call("section", "section_spec", &json!({"heading":"First"})),
            2 => openai::function_call("missing", "section_spec", &json!({"heading":"Absent"})),
            3 => openai::function_call("help", "help_spec", &json!({})),
            _ => {
                return openai::completed(
                    request,
                    vec![openai::message(
                        &json!({"verdict":"GREEN","covered":["R1"]}).to_string(),
                    )],
                    openai::usage(30, 5),
                );
            }
        };
        openai::completed(request, vec![call], openai::usage(30, 5))
    });
    let run = parsed(
        project
            .command(&["verify", "--all"])
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(run["requests"][0]["status"], "GREEN");
    let requests = provider.requests();
    let tools = requests[0].body["tools"].as_array().unwrap();
    for name in ["fixed_spec", "fixed_section_spec", "help_spec"] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["parameters"]["properties"], json!({}));
    }
    let section = tools
        .iter()
        .find(|tool| tool["name"] == "section_spec")
        .unwrap();
    assert_eq!(section["parameters"]["required"], json!(["heading"]));
    assert_eq!(
        section["parameters"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["heading"]
    );
    let outputs: Vec<_> = requests.last().unwrap().body["input"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "function_call_output")
        .collect();
    assert_eq!(outputs.len(), 4);
    let result = |index: usize| outputs[index]["output"].as_str().unwrap();
    assert_eq!(result(0), "# First\nbody\n## Child\nchild\n# Next\nlast\n");
    assert_eq!(result(1), "# First\nbody\n## Child\nchild\n");
    assert!(result(2).contains("No matching heading: Absent"));
    assert!(result(2).contains("First\nChild\nNext"));
    assert_eq!(result(3), "stub documentation: sub --help\n");
}
