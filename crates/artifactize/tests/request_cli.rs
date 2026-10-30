use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

#[path = "support/run.rs"]
mod owned_run;
use owned_run::RunChild as Child;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

// The fields-file input boundary; the oversized fixture is one byte beyond it.
const FIELDS_BYTES: usize = 256_000;

struct Fixture {
    root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(fingerprint: bool) -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        let child = repo.join("child");
        fs::create_dir_all(&child).unwrap();
        // Beside the child rather than around it, so that the child is no part of it.
        fs::create_dir_all(repo.join("parent")).unwrap();
        support::declaration::write(
            repo.join("parent/index.artf"),
            json!({
                "name":"parent","fingerprint":false,"evals":[{"id":"check","title":"Dependent",
                    "profile":{"kind":"runtime","command":support::os::true_program(),"args":[]},
                    "payload":{"instruction":"Check {child}."}}]
            })
            .to_string(),
        )
        .unwrap();
        let mut declaration = json!({
            "name":"child",
            "views":{
                "human_tools":{
                    "inspect":{
                        "description":"Inspect",
                        "kind":"output",
                        "command":support::os::cat_program(),
                        "args":["fingerprint"],
                    },
                    "open":{"description":"Launch","kind":"launch","command":support::os::true_program(),"args":[]},
                },
            },
            "evals":[
                {
                    "id":"review",
                    "title":"Human review",
                    "profile":{"kind":"human"},
                    "payload":{"instruction":"Review."},
                    "pass_schema":{
                        "type":"object",
                        "properties":{"approved":{"const":true}},
                        "required":["approved"],
                        "additionalProperties":false,
                    },
                    "fail_schema":{
                        "type":"object",
                        "properties":{"reason":{"type":"string"}},
                        "required":["reason"],
                        "additionalProperties":false,
                    },
                },
            ],
        });
        if fingerprint {
            declaration["fingerprint"] =
                json!({"script":{"command":support::os::cat_program(),"args":["fingerprint"]}});
        } else {
            declaration["fingerprint"] = json!(false);
        }
        fs::write(child.join("fingerprint"), "review-v1\n").unwrap();
        support::declaration::write(child.join("index.artf"), declaration.to_string()).unwrap();
        let state = root.path().join("state");
        root.track(&state, &repo);
        Self { repo, state, root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(support::os::cli());
        // Its own process group, so Ctrl-Break reaches it alone on Windows; the declarations
        // name `cat` and `true` for PATH to find.
        support::os::new_group(&mut command)
            .env("PATH", support::os::path())
            .current_dir(self.root.path())
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state);
        support::os::sign_in_as(&mut command, "alice");
        command
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self
            .command()
            .args(args)
            .arg("--json")
            .record_output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        parse(&output)
    }

    fn start(&self) -> Child {
        let child =
            support::os::start(self.command().args(["verify", "--wait", "--all", "--json"]));
        Child::new(child, &self.state, &self.repo)
    }

    fn request_state(&self, run: &str, eval: &str, expected: &str) {
        let run: artifactize_core::types::RunId = run.parse().unwrap();
        thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    runtime.block_on(async {
                        owned_run::subscribed(&self.state, async |changes| {
                            tokio::time::timeout(
                                support::os::patience(Duration::from_secs(15)),
                                async {
                                    loop {
                                        let view =
                                            artifactize_core::store::read_run(&self.state, &run)
                                                .await
                                                .unwrap();
                                        if view.requests.iter().any(|request| {
                                            request.eval_id == eval
                                                && request.status().as_str() == expected
                                        }) {
                                            return;
                                        }
                                        assert!(view.run.status().is_open(), "{view:?}");
                                        changes.next_reconciled().await;
                                    }
                                },
                            )
                            .await
                            .expect("saved request reaches the asserted state");
                        })
                        .await;
                    });
                })
                .join()
                .unwrap();
        });
    }

    fn finished(&self, id: &str) {
        let id = id.parse().unwrap();
        thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    runtime.block_on(owned_run::terminal(&self.state, &id));
                })
                .join()
                .unwrap();
        });
        support::runs::finished_worker(&self.state, &self.repo, &id, support::os::running);
    }

    fn waiting(&self) -> Value {
        let deadline = Instant::now() + support::os::patience(Duration::from_secs(8));
        loop {
            let list = self.json(&["request", "list"], 0);
            if let Some(request) = list
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["status"] == "DEFERRED")
            {
                return request.clone();
            }
            assert!(Instant::now() < deadline, "Human request did not appear");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn parse(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn finish(mut child: Child, code: i32) -> Value {
    support::os::wait_child(&mut child, "verify");
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse(&output)
}

#[test]
fn fingerprint_cli_claim_tool_correctable_submission_and_next_verify() {
    let fixture = Fixture::new(true);
    assert_eq!(fixture.json(&["request", "list"], 0), json!([]));
    assert!(!fixture.state.exists());
    // The Run remains waiting; the next verify reuses the submitted result.
    let run = fixture.json(&["verify", "--all"], 0);
    let list = fixture.json(
        &["request", "list", "--run", run["id"].as_str().unwrap()],
        0,
    );
    let waiting = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["status"] == "DEFERRED")
        .unwrap();
    let id = waiting["id"].as_str().unwrap();
    assert!(waiting["humanDefinition"]["eval"].is_object());
    assert!(
        fixture
            .json(&["request", "list", "--run", "unknown"], 0)
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture.json(&["request", "claim", id], 0)["reviewer"],
        "alice"
    );
    let show = fixture.json(&["request", "show", id], 0);
    assert_eq!(show["claim"]["reviewer"], "alice");
    for args in [
        vec!["request", "claim", id, "--reviewer", "bob"],
        vec!["request", "tool", id, "inspect_child", "--reviewer", "bob"],
        vec![
            "request",
            "submit",
            id,
            "--verdict",
            "GREEN",
            "--fields",
            r#"{"approved":true}"#,
            "--reviewer",
            "bob",
        ],
    ] {
        assert!(
            fixture.json(&args, 2)["error"]
                .as_str()
                .unwrap()
                .contains("claimant")
        );
    }
    let tool = fixture
        .command()
        .args(["request", "tool", id, "inspect_child"])
        .record_output()
        .unwrap();
    assert!(tool.status.success());
    assert!(String::from_utf8_lossy(&tool.stdout).contains("review-v1"));
    let launch = fixture
        .command()
        .args(["request", "tool", id, "open_child"])
        .record_output()
        .unwrap();
    assert!(launch.status.success());
    assert!(String::from_utf8_lossy(&launch.stdout).contains("launched"));
    for fields in [
        "{}",
        "[]",
        "invalid",
        r#"{"verdict":"RED","approved":true}"#,
    ] {
        fixture.json(
            &[
                "request",
                "submit",
                id,
                "--verdict",
                "GREEN",
                "--fields",
                fields,
            ],
            2,
        );
        assert_eq!(
            fixture.json(&["request", "show", id], 0)["status"],
            "DEFERRED"
        );
    }
    let fields = fixture.root.path().join("fields.json");
    fs::write(&fields, " ".repeat(FIELDS_BYTES + 1)).unwrap();
    fixture.json(
        &[
            "request",
            "submit",
            id,
            "--verdict",
            "GREEN",
            "--fields-file",
            fields.to_str().unwrap(),
        ],
        2,
    );
    fs::write(&fields, r#"{"approved":true}"#).unwrap();
    let submitted = fixture.json(
        &[
            "request",
            "submit",
            id,
            "--verdict",
            "GREEN",
            "--fields-file",
            fields.to_str().unwrap(),
        ],
        0,
    );
    assert_eq!(submitted["status"], "GREEN");
    assert!(submitted["claim"].is_null());
    assert!(submitted["summary"].get("toolCalls").is_none());
    assert_eq!(submitted["summary"]["executorStarts"], 0);
    assert!(submitted["summary"]["wallMs"].is_u64());
    fixture.json(
        &[
            "request",
            "submit",
            id,
            "--verdict",
            "GREEN",
            "--fields",
            r#"{"approved":true}"#,
        ],
        2,
    );
    let completed = fixture.json(&["verify", "--wait", "--all"], 0);
    assert_ne!(completed["id"], run["id"]);
    assert_eq!(completed["summary"]["executorStarts"], 1);
    assert_eq!(completed["summary"]["counts"]["GREEN"], 2);
    assert!(completed["summary"].get("toolCalls").is_none());
    fixture.finished(run["id"].as_str().unwrap());
    fs::remove_dir_all(&fixture.repo).unwrap();
    let shown = fixture
        .command()
        .args(["request", "show", id])
        .record_output()
        .unwrap();
    assert!(shown.status.success());
    assert_eq!(parse(&shown), submitted);
    let text = fixture
        .command()
        .args(["request", "list"])
        .record_output()
        .unwrap();
    assert!(text.status.success());
    assert!(String::from_utf8_lossy(&text.stdout).contains(id));
    assert_eq!(
        fixture.json(&["run", "show", completed["id"].as_str().unwrap()], 0),
        completed
    );
}

#[test]
fn unclaim_releases_the_claimants_lock_for_another_reviewer() {
    let fixture = Fixture::new(true);
    let run = fixture.json(&["verify", "--all"], 0);
    let id = run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["status"] == "DEFERRED")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let error = |args: &[&str]| fixture.json(args, 2)["error"].as_str().unwrap().to_owned();
    assert!(error(&["request", "unclaim", id]).contains("claimant"));
    let claim = fixture.json(&["request", "claim", id], 0);
    assert!(error(&["request", "unclaim", id, "--reviewer", "bob"]).contains("claimant"));
    assert_eq!(fixture.json(&["request", "unclaim", id], 0), claim);
    assert!(fixture.json(&["request", "show", id], 0)["claim"].is_null());
    assert_eq!(
        fixture.json(&["request", "claim", id, "--reviewer", "bob"], 0)["reviewer"],
        "bob"
    );
    fixture.json(
        &[
            "request",
            "submit",
            id,
            "--reviewer",
            "bob",
            "--verdict",
            "RED",
            "--fields",
            r#"{"reason":"Needs work"}"#,
        ],
        0,
    );
    assert!(
        error(&["request", "unclaim", id, "--reviewer", "bob"])
            .contains("not waiting for a Human review")
    );
    let text = fixture
        .command()
        .args(["request", "unclaim", "missing"])
        .record_output()
        .unwrap();
    assert_eq!(text.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&text.stderr).contains("not found"));
}

#[test]
fn fingerprint_false_submission_from_another_process_continues_the_same_run() {
    let fixture = Fixture::new(false);
    let sibling = fixture.repo.join("sibling");
    fs::create_dir(&sibling).unwrap();
    support::declaration::write(
        sibling.join("index.artf"),
        json!({
            "name":"sibling","fingerprint":false,"evals":[{"id":"check","title":"Once",
            "profile":{"kind":"runtime","command":support::os::true_program(),"args":[]},
            "payload":{"instruction":"Run before the Human finishes."}}]
        })
        .to_string(),
    )
    .unwrap();
    let child = support::os::start(fixture.command().args([
        "verify",
        "--wait",
        "--all",
        "--max-executions",
        "2",
        "--json",
    ]));
    let child = Child::new(child, &fixture.state, &fixture.repo);
    let waiting = fixture.waiting();
    let id = waiting["id"].as_str().unwrap();
    let run_id = waiting["runId"].as_str().unwrap();
    assert_eq!(
        fixture.json(&["run", "show", run_id], 0)["status"],
        "WAITING_HUMAN"
    );
    // The sibling's saved result, not merely Human admission, orders the submission.
    fixture.request_state(run_id, "sibling/check", "GREEN");
    fixture.json(&["request", "claim", id, "--reviewer", "custom"], 0);
    fixture.json(
        &[
            "request",
            "submit",
            id,
            "--reviewer",
            "custom",
            "--verdict",
            "GREEN",
            "--fields",
            r#"{"approved":true}"#,
        ],
        0,
    );
    let completed = finish(child, 0);
    assert_eq!(completed["id"], run_id);
    assert_eq!(completed["executionsStarted"], 2);
    assert!(
        completed["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "GREEN")
    );
    let runs = fixture.json(&["run", "list"], 0);
    assert_eq!(runs.as_array().unwrap().len(), 1);
    assert_eq!(runs[0]["id"], run_id);
    assert_eq!(fixture.json(&["cache", "list", "--history"], 0), json!([]));
    assert!(
        completed["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|request| { request["key"].is_null() })
    );
}

#[test]
fn status_predicts_deferral_only_for_a_selected_signoff() {
    let fixture = Fixture::new(false);
    let action = |args: &[&str], eval: &str| {
        let output = support::os::run(fixture.command().args(["status", "--json"]).args(args));
        let status = parse(&output);
        status["evals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == eval)
            .unwrap_or_else(|| panic!("{eval} in {status}"))["action"]
            .clone()
    };
    // Outside the selection, the sign-off is not asked for, so what relies on it waits.
    assert_eq!(action(&["parent"], "parent/check"), "wait");
    // Selected, it is asked for and deferred, so what relies on it runs at once.
    assert_eq!(
        action(&["parent", "--recursive"], "parent/check"),
        "execute"
    );
    assert_eq!(
        action(&["parent", "--recursive"], "child/review"),
        "execute"
    );
}

#[test]
fn submission_leaves_human_claim_and_submission_available() {
    let fixture = Fixture::new(false);
    let run = fixture.json(&["verify", "--all"], 0);
    assert_eq!(run["status"], "WAITING_HUMAN");
    assert_eq!(run["waitTimedOut"], false);
    let waiting = fixture.waiting();
    let id = waiting["id"].as_str().unwrap();
    fixture.json(&["request", "claim", id], 0);
    fixture.json(
        &[
            "request",
            "submit",
            id,
            "--verdict",
            "GREEN",
            "--fields",
            r#"{"approved":true}"#,
        ],
        0,
    );
    assert_eq!(fixture.json(&["request", "show", id], 0)["status"], "GREEN");
    for args in [
        vec!["verify", "--all", "--timeout-ms", "0"],
        vec!["verify", "--all", "--timeout-ms", "2147483648"],
    ] {
        fixture.json(&args, 2);
    }
}

#[test]
fn wait_observes_red_and_fingerprint_errors_and_explicit_cancellation_stops_cleanly() {
    let fixture = Fixture::new(false);
    let child = fixture.start();
    let waiting = fixture.waiting();
    let id = waiting["id"].as_str().unwrap();
    // The sign-off holds up nothing: the dependent check runs before anyone signs off.
    fixture.request_state(
        waiting["runId"].as_str().unwrap(),
        "parent/check",
        "PROVISIONAL",
    );
    fixture.json(&["request", "claim", id], 0);
    fixture.json(
        &[
            "request",
            "submit",
            id,
            "--verdict",
            "RED",
            "--fields",
            r#"{"reason":"Needs work"}"#,
        ],
        0,
    );
    let run = finish(child, 1);
    assert_eq!(run["executionsStarted"], 1);
    assert!(
        run["requests"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["status"] == "BLOCKED")
    );

    let fixture = Fixture::new(true);
    let child = fixture.start();
    let waiting = fixture.waiting();
    let id = waiting["id"].as_str().unwrap();
    // The sign-off holds up nothing: the dependent check runs before anyone signs off.
    fixture.request_state(
        waiting["runId"].as_str().unwrap(),
        "parent/check",
        "PROVISIONAL",
    );
    fixture.json(&["request", "claim", id], 0);
    fs::write(fixture.repo.join("child/fingerprint"), "changed\n").unwrap();
    fixture.json(
        &[
            "request",
            "submit",
            id,
            "--verdict",
            "GREEN",
            "--fields",
            r#"{"approved":true}"#,
        ],
        2,
    );
    let run = finish(child, 2);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["status"], "ERROR");

    let fixture = Fixture::new(false);
    let child = fixture.start();
    let waiting = fixture.waiting();
    fixture.request_state(
        waiting["runId"].as_str().unwrap(),
        "parent/check",
        "PROVISIONAL",
    );
    support::runs::cancel_active(&fixture.state, &fixture.repo);
    let run = finish(child, 2);
    assert_eq!(run["status"], "ERROR");
    assert_eq!(run["waitTimedOut"], false);
    assert_eq!(run["error"], "Run was cancelled.");
    assert_eq!(run["executionsStarted"], 1);
}
