use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use support::os::shell;
use tempfile::TempDir;

mod support;

struct Fixture {
    _root: TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(evals: Vec<Value>) -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        support::declaration::write(
            repo.join("index.artf"),
            json!({"name":"test","fingerprint":false,"evals":evals}).to_string(),
        )
        .unwrap();
        Self {
            repo,
            state: root.path().join("state"),
            _root: root,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        // Its own process group, so Ctrl-Break reaches it alone on Windows.
        support::os::new_group(&mut command)
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(["verify", "--all", "--json"])
            .args(args);
        command
    }

    fn spawn(&self, args: &[&str]) -> Child {
        self.command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn run(&self, args: &[&str], code: i32) -> Value {
        finish(self.spawn(args), code)
    }

    fn starts(&self) -> Vec<String> {
        fs::read_to_string(self.repo.join("events"))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.strip_prefix("start ").map(str::to_owned))
            .collect()
    }
}

fn eval(id: &str, script: &str) -> Value {
    json!({
        "id":id,
        "title":"Check",
        "profile":{
            "kind":"runtime",
            "command":shell(),
            "args":["-c",script,"sh",id],
            "timeout_ms":10000,
        },
        "payload":{"instruction":"Check."},
    })
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(10));
    while !condition() {
        assert!(Instant::now() < deadline, "condition was not reached");
        thread::sleep(Duration::from_millis(10));
    }
}

fn finish(mut child: Child, code: i32) -> Value {
    wait_until(|| child.try_wait().unwrap().is_some());
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// Interrupt or terminate a child, named as the Unix signals that do it; Windows has
/// Ctrl-Break for both.
fn signal(child: &Child, signal: &str) {
    match signal {
        "-INT" => support::os::interrupt(child.id()),
        "-TERM" => support::os::terminate(child.id()),
        _ => unreachable!("{signal}"),
    }
}

#[test]
fn ipc_budget_decision_is_idempotent_while_an_execution_remains_running() {
    let fixture = Fixture::new(vec![eval("slow", SLOW), eval("zz-over-budget", "true")]);
    let child = fixture.spawn(&["--jobs", "2", "--max-executions", "1"]);
    wait_until(|| fixture.starts().len() == 1 && fixture.state.join("state.sqlite").exists());
    let database = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    database.busy_timeout(Duration::from_secs(5)).unwrap();
    database
        .execute_batch(
            "CREATE TABLE fixture_update_count(count INTEGER); INSERT INTO fixture_update_count VALUES(0); CREATE TRIGGER fixture_budget_updates AFTER UPDATE ON requests WHEN NEW.status='BUDGET_EXHAUSTED' BEGIN UPDATE fixture_update_count SET count=count+1; END;",
        )
        .unwrap();
    fs::write(fixture.repo.join("release"), "finish").unwrap();
    let run = finish(child, 4);
    // Every rewrite while the execution ran is counted by the time the Run has finished.
    let count: i64 = database
        .query_row("SELECT count FROM fixture_update_count", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(
        count <= 2,
        "self-invalidations repeatedly rewrote a budget decision: {count}"
    );
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["requests"][1]["status"], "BUDGET_EXHAUSTED");
}

#[test]
fn expired_human_deadline_still_drains_parallel_runtime_without_rewriting_waiter() {
    let fixture = Fixture::new(vec![
        json!({
            "id":"human",
            "title":"Human",
            "profile":{"kind":"human"},
            "payload":{"instruction":"Review."},
        }),
        eval("slow", SLOW),
    ]);
    let child = fixture.spawn(&["--jobs", "2", "--timeout-ms", "10"]);
    wait_until(|| fixture.starts().len() == 1);
    fs::write(fixture.repo.join("release"), "finish").unwrap();
    let run = finish(child, 3);
    assert_eq!(run["waitTimedOut"], true);
    assert_eq!(run["requests"][0]["status"], "WAITING_HUMAN");
    assert_eq!(run["requests"][1]["status"], "GREEN");
}

const SLOW: &str = "printf 'start %s\n' \"$1\" >> events; while [ ! -e release ]; do sleep 0.02; done; printf 'end %s\n' \"$1\" >> events";

#[test]
fn independent_evals_fill_jobs_without_exceeding_them_and_default_to_four() {
    for (args, jobs) in [(vec!["--jobs", "2"], 2), (vec![], 4)] {
        let fixture = Fixture::new((0..6).map(|id| eval(&format!("e{id}"), SLOW)).collect());
        let child = fixture.spawn(&args);
        wait_until(|| fixture.starts().len() >= jobs);
        let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
        db.busy_timeout(support::os::patience(Duration::from_millis(100)))
            .unwrap();
        db.execute_batch("BEGIN IMMEDIATE; COMMIT;").unwrap();
        let data: String = db
            .query_row("SELECT data FROM runs", [], |row| row.get(0))
            .unwrap();
        let run: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(run["executionsStarted"], jobs);
        fs::write(fixture.repo.join("release"), "").unwrap();
        let run = finish(child, 0);
        assert_eq!(run["jobs"], jobs);
        assert_eq!(run["executionsStarted"], 6);
        let mut active = 0;
        let mut peak = 0;
        for event in fs::read_to_string(fixture.repo.join("events"))
            .unwrap()
            .lines()
        {
            if event.starts_with("start ") {
                active += 1;
            } else {
                active -= 1;
            }
            peak = peak.max(active);
            assert!(active <= jobs);
        }
        assert_eq!(active, 0);
        assert_eq!(peak, jobs);
        let requests = run["requests"].as_array().unwrap();
        let starts = fixture.starts();
        assert_eq!(starts.len(), requests.len());
    }
}

#[test]
fn completion_releases_a_dependent_while_an_independent_eval_is_still_running() {
    let fixture = Fixture::new(vec![]);
    support::declaration::write(
        fixture.repo.join("index.artf"),
        r#"{"name":"root","basis":true}"#,
    )
    .unwrap();
    for (name, script, instruction) in [
        (
            "a-dependent",
            "test -e ../b-input/done; touch started",
            "Check {b-input}.",
        ),
        (
            "b-input",
            "while [ ! -e ../c-slow/started ]; do sleep 0.01; done; touch done",
            "Check.",
        ),
        (
            "c-slow",
            "touch started; while [ ! -e release ]; do sleep 0.02; done",
            "Check.",
        ),
    ] {
        let folder = fixture.repo.join(name);
        fs::create_dir(&folder).unwrap();
        let mut check = eval("check", script);
        check["payload"]["instruction"] = json!(instruction);
        support::declaration::write(
            folder.join("index.artf"),
            json!({"name":name,"fingerprint":false,"evals":[check]}).to_string(),
        )
        .unwrap();
    }
    let mut child = fixture.spawn(&["--jobs", "2"]);
    wait_until(|| fixture.repo.join("a-dependent/started").exists());
    assert!(child.try_wait().unwrap().is_none());
    assert!(fixture.repo.join("c-slow/started").exists());
    fs::write(fixture.repo.join("c-slow/release"), "").unwrap();
    let run = finish(child, 0);
    let requests = run["requests"].as_array().unwrap();
    assert_eq!(requests[0]["status"], "GREEN");
    assert_eq!(requests[1]["status"], "GREEN");
    assert_eq!(run["executionsStarted"], 3);
}

#[test]
fn budget_is_shared_across_selected_evals_and_stops_new_starts() {
    let fixture = Fixture::new(
        (0..5)
            .map(|id| eval(&format!("e{id}"), "printf 'start %s\n' \"$1\" >> events"))
            .collect(),
    );
    let run = fixture.run(&["--jobs", "4", "--max-executions", "2"], 4);
    assert_eq!(run["status"], "INCOMPLETE");
    assert_eq!(run["maxExecutions"], 2);
    assert_eq!(run["executionsStarted"], 2);
    assert!(
        run["error"]
            .as_str()
            .unwrap()
            .contains("maxExecutions budget exhausted")
    );
    assert_eq!(fixture.starts().len(), 2);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][1]["status"], "GREEN");
    for request in run["requests"].as_array().unwrap().iter().skip(2) {
        assert_eq!(request["status"], "BUDGET_EXHAUSTED");
        assert!(request["startedAt"].is_null());
        assert!(request["executionId"].is_null());
        assert!(
            request["blockedReason"]
                .as_str()
                .unwrap()
                .contains("2 of 2")
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--state-dir")
        .arg(&fixture.state)
        .args(["run", "show", run["id"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        run
    );
    let zero = fixture.run(&["--max-executions", "0"], 4);
    assert_eq!(zero["executionsStarted"], 0);
    assert_eq!(fixture.starts().len(), 2);
}

#[test]
fn preparation_failures_do_not_consume_a_start_and_finished_runs_at_the_cap_succeed() {
    let mut invalid = eval("bad", "exit 0");
    invalid["profile"]["args"] = json!(["{test}/missing"]);
    let fixture = Fixture::new(vec![
        invalid,
        eval("good", "printf 'start good\n' >> events"),
    ]);
    let run = fixture.run(&["--max-executions", "1"], 2);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["requests"][0]["errorCode"], "PREPARATION_FAILED");
    assert!(run["requests"][0]["startedAt"].is_null());
    assert_eq!(run["requests"][1]["status"], "GREEN");
    let fixture = Fixture::new(vec![eval("good", "exit 0")]);
    assert_eq!(
        fixture.run(&["--max-executions", "1"], 0)["executionsStarted"],
        1
    );
}

#[test]
fn cancellation_kills_all_owned_groups_and_marks_queued_requests_cancelled() {
    for interrupt in ["-INT", "-TERM"] {
        let fixture = Fixture::new(
            (0..5)
                .map(|id| {
                    eval(
                        &format!("e{id}"),
                        "sleep 30 & printf '%s %s\n' \"$$\" \"$!\" > \"$1.pids\"; wait",
                    )
                })
                .collect(),
        );
        let child = fixture.spawn(&["--jobs", "3"]);
        wait_until(|| (0..3).all(|id| fixture.repo.join(format!("e{id}.pids")).exists()));
        signal(&child, interrupt);
        let run = finish(child, 2);
        assert_eq!(run["status"], "ERROR");
        assert_eq!(run["executionsStarted"], 3);
        for request in run["requests"].as_array().unwrap() {
            assert_eq!(request["status"], "ERROR");
            assert_eq!(request["errorCode"], "CANCELLED");
            assert!(request["result"].is_null());
        }
        for id in 0..3 {
            for pid in fs::read_to_string(fixture.repo.join(format!("e{id}.pids")))
                .unwrap()
                .split_whitespace()
            {
                assert!(
                    !support::os::running(pid.parse().unwrap()),
                    "process {pid} still running"
                );
            }
        }
        assert!(!fixture.repo.join("e3.pids").exists());
    }
}

#[test]
fn invalid_limits_fail_without_creating_state() {
    let fixture = Fixture::new(vec![eval("check", "exit 0")]);
    for args in [
        ["--jobs", "0"],
        ["--jobs", "-1"],
        ["--max-executions", "-1"],
    ] {
        let run = fixture.run(&args, 2);
        assert!(run["error"].is_string());
        assert!(!fixture.state.exists());
    }
}

#[test]
fn budget_is_rechecked_when_a_dependency_makes_an_eval_ready() {
    let fixture = Fixture::new(vec![]);
    support::declaration::write(
        fixture.repo.join("index.artf"),
        r#"{"name":"root","basis":true}"#,
    )
    .unwrap();
    for name in ["dependency", "consumer"] {
        let folder = fixture.repo.join(name);
        fs::create_dir(&folder).unwrap();
        let mut check = eval("check", "touch ran");
        if name == "consumer" {
            check["payload"]["instruction"] = json!("Check {dependency}.");
        }
        support::declaration::write(
            folder.join("index.artf"),
            json!({"name":name,"fingerprint":false,"evals":[check]}).to_string(),
        )
        .unwrap();
    }
    let run = fixture.run(&["--jobs", "2", "--max-executions", "1"], 4);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["requests"][0]["status"], "BUDGET_EXHAUSTED");
    assert_eq!(run["requests"][1]["status"], "GREEN");
    assert!(!fixture.repo.join("consumer/ran").exists());
    assert!(fixture.repo.join("dependency/ran").exists());
}

#[test]
fn cancellation_keeps_already_committed_evidence() {
    let fixture = Fixture::new(vec![
        eval("completed", "exit 0"),
        eval(
            "running",
            "touch started; while [ ! -e release ]; do sleep 0.02; done",
        ),
    ]);
    let child = fixture.spawn(&["--jobs", "1"]);
    wait_until(|| fixture.repo.join("started").exists());
    signal(&child, "-INT");
    let run = finish(child, 2);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][0]["result"]["verdict"], "GREEN");
    assert_eq!(run["requests"][1]["errorCode"], "CANCELLED");
}
