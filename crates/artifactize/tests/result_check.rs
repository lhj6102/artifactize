//! An Agent eval's `resultCheck` end to end, against the loopback fake provider.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::{Value, json};
use support::{FakeProvider, Request, openai};

/// Reads the eval's input on stdin, records it, and checks the result against the spec
/// and the tool-call audit. The mode argument makes it crash, hang or misbehave.
const CHECK: &str = r#"
import json, os, sys, time
capture, spec, mode = sys.argv[1], sys.argv[2], sys.argv[3]
data = json.load(sys.stdin)
with open(capture, "a") as out:
    out.write(json.dumps({"input": data, "cwd": os.getcwd(), "home": os.environ.get("HOME"),
                          "secret": os.environ.get("RESULT_CHECK_SECRET")}) + "\n")
if mode == "crash":
    print("boom", file=sys.stderr)
    sys.exit(3)
if mode == "hang":
    time.sleep(30)
if mode == "garbage":
    print("not json")
    sys.exit(0)
text = open(spec).read()
errors = []
read = [call for call in data["toolCalls"] if call["name"] == "read_spec" and not call["isError"]]
if data["result"]["verdict"] == "GREEN" and not read:
    errors.append("A GREEN verdict needs spec.md read with read_spec.")
for requirement in data["result"].get("covered", []):
    if requirement + ":" not in text:
        errors.append(requirement + " does not appear in spec.md.")
print(json.dumps({"errors": errors}))
"#;

struct Project {
    root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Project {
    fn new(mode: &str, timeout_ms: u32) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("spec")).unwrap();
        fs::write(repo.join("spec/spec.md"), "R1: the spec covers R1.\n").unwrap();
        fs::write(repo.join("spec/check.py"), CHECK).unwrap();
        let project = Self {
            state: root.path().join("state"),
            repo,
            root,
        };
        project.declare(mode, timeout_ms);
        project
    }

    fn capture(&self) -> PathBuf {
        self.root.path().join("capture.jsonl")
    }

    fn declare(&self, mode: &str, timeout_ms: u32) {
        self.declare_with(mode, timeout_ms, &[]);
    }

    /// `extra` args follow the ones the check reads; it ignores them.
    fn declare_with(&self, mode: &str, timeout_ms: u32, extra: &[&str]) {
        let mut args = json!(["check.py", self.capture(), "{spec}/spec.md", mode]);
        args.as_array_mut()
            .unwrap()
            .extend(extra.iter().map(|arg| json!(arg)));
        let check = json!({"command":"python3","args":args,"timeoutMs":timeout_ms});
        fs::write(
            self.repo.join("spec/artifactize.json"),
            json!({"name":"spec","fingerprint":{},
                "views":{"agentTools":{"read":{"builtin":"read"}}},
                "evals":[{"id":"review","title":"Review",
                    "profile":{"kind":"agent","backend":"openai","model":"fake-model"},
                    "payload":{"instruction":"Review {spec}."},
                    "passSchema":{"type":"object","properties":{"covered":{"type":"array","items":{"type":"string"}}},"required":["covered"]},
                    "resultCheck":check}]})
            .to_string(),
        )
        .unwrap();
    }

    fn command(&self, provider: &FakeProvider, args: &[&str]) -> Command {
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
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
            .env("RESULT_CHECK_SECRET", "must-not-leak");
        command
    }

    fn verify(&self, provider: &FakeProvider, args: &[&str], code: i32) -> Value {
        let output = self
            .command(provider, &[&["verify", "--all"][..], args].concat())
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

    /// What the check received, one record per run.
    fn captured(&self) -> Vec<Value> {
        fs::read_to_string(self.capture())
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn last_prompt(request: &Request) -> String {
    let input = request.body["input"].as_array().unwrap();
    input.last().unwrap()["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Reads spec.md, then answers `first`; the repair turn answers `repaired`.
fn reviewer(first: &'static [&'static str], repaired: &'static [&'static str]) -> FakeProvider {
    FakeProvider::start(move |request| {
        let input = request.body["input"].as_array().unwrap();
        let tool_output = input
            .iter()
            .any(|item| item["type"] == "function_call_output");
        let output = if last_prompt(request).starts_with("Your final response did not") {
            openai::message(&json!({"verdict":"GREEN","covered":repaired}).to_string())
        } else if tool_output {
            openai::message(&json!({"verdict":"GREEN","covered":first}).to_string())
        } else {
            openai::function_call("call_1", "read_spec", &json!({"path":"spec.md"}))
        };
        openai::completed(request, vec![output], openai::usage(10, 2))
    })
}

fn review(run: &Value) -> &Value {
    &run["requests"][0]
}

#[test]
fn a_passing_check_sees_the_result_and_audit_in_an_isolated_process() {
    let project = Project::new("check", 10_000);
    let provider = reviewer(&["R1"], &[]);
    let run = project.verify(&provider, &[], 0);
    assert_eq!(review(&run)["status"], "GREEN", "{run}");
    assert_eq!(
        review(&run)["result"],
        json!({"verdict":"GREEN","covered":["R1"]})
    );
    assert_eq!(provider.requests().len(), 2, "no repair turn");
    let captured = project.captured();
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0]["input"],
        json!({"version":1,"artifactId":"spec","family":null,
            "result":{"verdict":"GREEN","covered":["R1"]},
            "toolCalls":[{"name":"read_spec","arguments":{"path":"spec.md"},"isError":false}]})
    );
    // The target's folder is the cwd, with a private home and no inherited environment.
    let cwd = fs::canonicalize(project.repo.join("spec")).unwrap();
    assert_eq!(Path::new(captured[0]["cwd"].as_str().unwrap()), cwd);
    assert!(!captured[0]["home"].as_str().unwrap().is_empty());
    assert_ne!(captured[0]["home"], json!(std::env::var("HOME").ok()));
    assert!(captured[0]["secret"].is_null());
}

#[test]
fn check_errors_feed_the_one_repair_turn() {
    let project = Project::new("check", 10_000);
    let provider = reviewer(&["R9"], &["R1"]);
    let run = project.verify(&provider, &[], 0);
    assert_eq!(
        review(&run)["result"],
        json!({"verdict":"GREEN","covered":["R1"]})
    );
    let calls = provider.requests();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        last_prompt(&calls[2]),
        "Your final response did not pass the project's result check:\n- R9 does not appear in spec.md.\nReturn only one JSON object matching the schema."
    );
    // The repair turn has no tools; the check saw both results.
    assert!(calls[2].body["tools"].as_array().is_none_or(Vec::is_empty));
    let covered: Vec<_> = project
        .captured()
        .iter()
        .map(|record| record["input"]["result"]["covered"].clone())
        .collect();
    assert_eq!(covered, [json!(["R9"]), json!(["R1"])]);
}

#[test]
fn check_errors_after_the_repair_end_the_review() {
    let project = Project::new("check", 10_000);
    let provider = reviewer(&["R9"], &["R7"]);
    let run = project.verify(&provider, &[], 2);
    assert_eq!(review(&run)["errorCode"], "INVALID_RESULT", "{run}");
    assert_eq!(
        review(&run)["error"],
        "Invalid final Agent result after one format repair: resultCheck: R7 does not appear in spec.md."
    );
    assert_eq!(provider.requests().len(), 3, "a single repair turn");
    assert!(review(&run)["result"].is_null());
}

#[test]
fn a_crashing_hanging_or_misbehaving_check_ends_the_review() {
    for (mode, timeout_ms, expected) in [
        (
            "crash",
            10_000,
            "resultCheck ended with exit status 3: boom",
        ),
        ("hang", 300, "resultCheck timed out after 300 ms."),
        ("garbage", 10_000, "resultCheck must print one JSON object"),
    ] {
        let project = Project::new(mode, timeout_ms);
        let provider = reviewer(&["R1"], &["R1"]);
        let run = project.verify(&provider, &[], 2);
        let failed = review(&run);
        assert_eq!(failed["errorCode"], "RESULT_CHECK_FAILED", "{mode}: {run}");
        assert!(
            failed["error"].as_str().unwrap().starts_with(expected),
            "{mode}: {failed}"
        );
        // No repair turn for a broken check, and no result.
        assert_eq!(provider.requests().len(), 2, "{mode}");
        assert!(failed["result"].is_null());
    }
}

#[test]
fn a_changed_check_command_misses_the_cache_but_its_timeout_does_not() {
    let project = Project::new("check", 10_000);
    let provider = reviewer(&["R1"], &[]);
    let first = project.verify(&provider, &[], 0);
    // The check's timeoutMs is an execution option, recorded with the result.
    assert_eq!(review(&first)["options"]["resultCheckTimeoutMs"], 10_000);
    let again = project.verify(&provider, &[], 0);
    assert_eq!(again["executionsStarted"], 0);
    assert_eq!(review(&again)["executionId"], review(&first)["executionId"]);
    assert_eq!(provider.requests().len(), 2);

    // A new timeoutMs alone reuses the result, which keeps the options it was made with.
    project.declare("check", 20_000);
    let status = project.command(&provider, &["status"]).output().unwrap();
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["evals"][0]["action"], "reuse", "{status}");
    let longer = project.verify(&provider, &[], 0);
    assert_eq!(longer["executionsStarted"], 0);
    assert_eq!(
        review(&longer)["executionId"],
        review(&first)["executionId"]
    );
    assert_eq!(
        review(&longer)["evalDefHash"],
        review(&first)["evalDefHash"]
    );
    assert_eq!(review(&longer)["options"]["resultCheckTimeoutMs"], 10_000);
    assert_eq!(project.captured().len(), 1);

    // The command and args are the eval's strategy: changing them reviews again.
    project.declare_with("check", 20_000, &["strict"]);
    let status = project.command(&provider, &["status"]).output().unwrap();
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["evals"][0]["action"], "execute", "{status}");
    let changed = project.verify(&provider, &[], 0);
    assert_eq!(changed["executionsStarted"], 1);
    assert_ne!(
        review(&changed)["evalDefHash"],
        review(&first)["evalDefHash"]
    );
    assert_eq!(review(&changed)["options"]["resultCheckTimeoutMs"], 20_000);
    assert_eq!(provider.requests().len(), 4);
    assert_eq!(project.captured().len(), 2);
}

#[test]
fn an_artifact_the_check_names_is_a_dependency_of_the_eval() {
    let project = Project::new("check", 10_000);
    fs::create_dir_all(project.repo.join("reqs")).unwrap();
    fs::write(
        project.repo.join("reqs/artifactize.json"),
        json!({"name":"reqs","basis":true,"fingerprint":{}}).to_string(),
    )
    .unwrap();
    fs::write(project.repo.join("reqs/list.txt"), "R1\n").unwrap();
    let path = project.repo.join("spec/artifactize.json");
    let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    declaration["evals"][0]["resultCheck"]["args"]
        .as_array_mut()
        .unwrap()
        .push(json!("{reqs}/list.txt"));
    fs::write(&path, declaration.to_string()).unwrap();

    let provider = reviewer(&["R1"], &[]);
    let graph = project
        .command(&provider, &["config", "graph", "spec"])
        .output()
        .unwrap();
    let graph: Value = serde_json::from_slice(&graph.stdout).unwrap();
    assert!(
        graph.to_string().contains(r#""kind":"argv""#) && graph.to_string().contains("list.txt"),
        "{graph}"
    );
    let first = project.verify(&provider, &[], 0);
    assert!(
        review(&first)["fingerprints"]["reqs"].is_string(),
        "{first}"
    );
    // The check reads reqs, so a change there reviews again.
    fs::write(project.repo.join("reqs/list.txt"), "R1\nR2\n").unwrap();
    let changed = project.verify(&provider, &[], 0);
    assert_eq!(changed["executionsStarted"], 1);
    assert_eq!(project.captured().len(), 2);
}
