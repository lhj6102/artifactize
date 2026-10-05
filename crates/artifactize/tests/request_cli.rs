use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

mod support;

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(fingerprint: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let child = repo.join("child");
        fs::create_dir_all(&child).unwrap();
        fs::write(
            repo.join("artifactize.json"),
            json!({
                "name":"parent","evals":[{"id":"check","title":"Dependent",
                    "profile":{"kind":"runtime","command":"true","args":[]},
                    "payload":{"instruction":"Check child."}}]
            })
            .to_string(),
        )
        .unwrap();
        let mut declaration = json!({
            "name":"child","views":{"humanTools":{
                "inspect":{"description":"Inspect","kind":"output","command":"cat","args":["fingerprint"]},
                "open":{"description":"Launch","kind":"launch","command":"true","args":[]}
            }},
            "evals":[{"id":"review","title":"Human review","profile":{"kind":"human"},
                "payload":{"instruction":"Review."},
                "passSchema":{"type":"object","properties":{"approved":{"const":true}},"required":["approved"],"additionalProperties":false},
                "failSchema":{"type":"object","properties":{"reason":{"type":"string"}},"required":["reason"],"additionalProperties":false}}]
        });
        if fingerprint {
            declaration["fingerprint"] = json!({"script":{"command":"cat","args":["fingerprint"]}});
        }
        fs::write(child.join("fingerprint"), "review-v1\n").unwrap();
        fs::write(child.join("artifactize.json"), declaration.to_string()).unwrap();
        Self {
            repo,
            state: root.path().join("state"),
            root,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        // Its own process group, so Ctrl-Break reaches it alone on Windows; the declarations
        // name `cat` and `true` for PATH to find.
        support::os::new_group(&mut command)
            .env("PATH", support::os::path())
            .current_dir(self.root.path())
            .env("USER", "alice")
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state);
        command
    }

    fn json(&self, args: &[&str], code: i32) -> Value {
        let output = self.command().args(args).arg("--json").output().unwrap();
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
        self.command()
            .args([
                "verify",
                "--all",
                "--wait",
                "--timeout-ms",
                &support::os::slow(10000).to_string(),
                "--json",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn waiting(&self) -> Value {
        let deadline = Instant::now() + support::os::patience(Duration::from_secs(8));
        loop {
            let list = self.json(&["request", "list"], 0);
            if let Some(request) = list
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["status"] == "WAITING_HUMAN")
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
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(15));
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("verify did not finish");
        }
        thread::sleep(Duration::from_millis(20));
    }
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
    let run = fixture.json(&["verify", "--all"], 4);
    let list = fixture.json(
        &["request", "list", "--run", run["id"].as_str().unwrap()],
        0,
    );
    let waiting = list
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["status"] == "WAITING_HUMAN")
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
        .output()
        .unwrap();
    assert!(tool.status.success());
    assert!(String::from_utf8_lossy(&tool.stdout).contains("review-v1"));
    let launch = fixture
        .command()
        .args(["request", "tool", id, "open_child"])
        .output()
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
            "WAITING_HUMAN"
        );
    }
    let fields = fixture.root.path().join("fields.json");
    fs::write(&fields, " ".repeat(256_001)).unwrap();
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
    assert_eq!(submitted["summary"]["toolCalls"]["inspect_child"], 1);
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
    let completed = fixture.json(&["verify", "--all", "--wait"], 0);
    assert_ne!(completed["id"], run["id"]);
    assert_eq!(completed["summary"]["executorStarts"], 1);
    assert_eq!(completed["summary"]["counts"]["GREEN"], 2);
    assert_eq!(
        completed["summary"]["toolCalls"],
        json!({}),
        "reused tools are not counted twice"
    );
    fs::remove_dir_all(&fixture.repo).unwrap();
    let shown = fixture
        .command()
        .args(["request", "show", id])
        .output()
        .unwrap();
    assert!(shown.status.success());
    assert_eq!(parse(&shown), submitted);
    let text = fixture
        .command()
        .args(["request", "list"])
        .output()
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
    let run = fixture.json(&["verify", "--all"], 4);
    let id = run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["status"] == "WAITING_HUMAN")
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
        .output()
        .unwrap();
    assert_eq!(text.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&text.stderr).contains("not found"));
}

#[test]
fn no_fingerprint_submission_from_another_process_continues_the_same_run() {
    let fixture = Fixture::new(false);
    let sibling = fixture.repo.join("sibling");
    fs::create_dir(&sibling).unwrap();
    fs::write(
        sibling.join("artifactize.json"),
        json!({
            "name":"sibling","evals":[{"id":"check","title":"Once",
            "profile":{"kind":"runtime","command":"true","args":[]},
            "payload":{"instruction":"Run before the Human finishes."}}]
        })
        .to_string(),
    )
    .unwrap();
    let child = fixture
        .command()
        .args([
            "verify",
            "--all",
            "--wait",
            "--max-executions",
            "2",
            "--timeout-ms",
            &support::os::slow(10000).to_string(),
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let waiting = fixture.waiting();
    let id = waiting["id"].as_str().unwrap();
    let run_id = waiting["runId"].as_str().unwrap();
    assert_eq!(
        fixture.json(&["run", "show", run_id], 0)["status"],
        "RUNNING"
    );
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
    let db = rusqlite::Connection::open(fixture.state.join("state.sqlite")).unwrap();
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM runs", [], |r| r.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM cache_entries", [], |r| r.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn wait_timeout_leaves_human_claim_and_submission_available() {
    let fixture = Fixture::new(false);
    let run = fixture.json(&["verify", "--all", "--wait", "--timeout-ms", "40"], 3);
    assert_eq!(run["status"], "INCOMPLETE");
    assert_eq!(run["waitTimedOut"], true);
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
        vec!["verify", "--all", "--timeout-ms", "10"],
        vec!["verify", "--all", "--wait", "--timeout-ms", "0"],
        vec!["verify", "--all", "--wait", "--timeout-ms", "2147483648"],
    ] {
        fixture.json(&args, 2);
    }
}

#[test]
fn wait_observes_red_and_fingerprint_errors_and_ctrl_c_stops_cleanly() {
    let fixture = Fixture::new(false);
    let child = fixture.start();
    let waiting = fixture.waiting();
    let id = waiting["id"].as_str().unwrap();
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
    assert_eq!(run["executionsStarted"], 0);
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
    assert_eq!(run["executionsStarted"], 0);
    assert_eq!(run["status"], "ERROR");

    let fixture = Fixture::new(false);
    let child = fixture.start();
    fixture.waiting();
    support::os::interrupt(child.id());
    let run = finish(child, 2);
    assert_eq!(run["status"], "ERROR");
    assert_eq!(run["waitTimedOut"], false);
    assert_eq!(run["error"], "Run was cancelled.");
    assert_eq!(run["executionsStarted"], 0);
}
