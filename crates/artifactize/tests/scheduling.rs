use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use serde_json::{Value, json};
use tempfile::TempDir;

#[path = "support/run.rs"]
mod owned_run;
use owned_run::RunChild as Child;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Fixture {
    _root: TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        owned_run::release_repo(&self.state, &self.repo);
    }
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
        let mut command = Command::new(support::os::cli());
        // Its own process group, so Ctrl-Break reaches it alone on Windows.
        support::os::new_group(&mut command)
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(["verify", "--wait", "--all", "--json"])
            .args(args);
        command
    }

    fn spawn(&self, args: &[&str]) -> Child {
        let child = self
            .command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Child::new(child, &self.state, &self.repo)
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
            "command":support::os::python_program(),
            "args":["-c",script,id],
        },
        "payload":{"instruction":"Check."},
    })
}

fn process_tree_eval(id: &str, record: &str) -> Value {
    let mut check = eval(id, support::os::process_tree_probe());
    check["profile"]["args"] = json!(["-c", support::os::process_tree_probe(), record]);
    check
}

// PID records are ready only after their complete newline-terminated pair is saved.
fn recorded_pids(path: &std::path::Path) -> Option<[u32; 2]> {
    let text = fs::read_to_string(path).ok()?;
    if !text.ends_with('\n') {
        return None;
    }
    let pids: Vec<u32> = text
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let pair: [u32; 2] = pids.try_into().ok()?;
    pair.iter().all(|pid| *pid > 0).then_some(pair)
}

fn wait_until(condition: impl FnMut() -> bool) {
    // Files and child exit are the barriers; shared polling only bounds a stalled fixture.
    support::os::wait_until(Duration::from_secs(10), condition);
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

async fn observed(
    fixture: &Fixture,
    id: &artifactize_core::types::RunId,
    ready: impl Fn(&artifactize_core::store::RunView) -> bool,
) -> artifactize_core::store::RunView {
    let mut changes = artifactize_core::changes::Subscription::new(&fixture.state).await;
    // A fixture observation watchdog, not an execution lifetime or timing assertion.
    const OBSERVATION_BOUND: Duration = Duration::from_secs(30);
    tokio::time::timeout(support::os::patience(OBSERVATION_BOUND), async {
        loop {
            let view = artifactize_core::store::read_run(&fixture.state, id)
                .await
                .unwrap();
            if ready(&view) {
                return view;
            }
            assert_eq!(
                view.run.status(),
                artifactize_core::types::RunStatus::Running
            );
            changes.next_reconciled().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn ipc_budget_decision_is_idempotent_while_an_execution_remains_running() {
    let fixture = Fixture::new(vec![eval("slow", SLOW), eval("zz-over-budget", "pass")]);
    let child = fixture.spawn(&["--jobs", "2", "--max-executions", "1"]);
    let id = child.run();
    let before = observed(&fixture, &id, |view| {
        view.run.executions_started == 1
            && view.requests[0].status() == artifactize_core::types::RequestStatus::Running
            && view.requests[1].status() == artifactize_core::types::RequestStatus::BudgetExhausted
    })
    .await;
    let mut notifications = artifactize_core::changes::Subscription::new(&fixture.state).await;
    notifications.next_state().await;
    // Another real Run changes the shared state; observing it must not consume another
    // start or change the original Run's already-visible budget decision.
    let other = fixture._root.path().join("other");
    fs::create_dir(&other).unwrap();
    support::declaration::write(
        other.join("index.artf"),
        json!({
            "name":"other", "evals":[{"id":"check","title":"Check","profile":{
                "kind":"runtime","command":support::os::true_program(),"args":[]},
                "payload":{"instruction":"Check."}}]
        })
        .to_string(),
    )
    .unwrap();
    let _other_runs = owned_run::RepoLease::new(&fixture.state, &other);
    let output = Command::new(support::os::cli())
        .args(["verify", "--wait", "--all", "--json"])
        .arg("--repo")
        .arg(&other)
        .arg("--state-dir")
        .arg(&fixture.state)
        .record_output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    notifications.next_state().await;
    let after = observed(&fixture, &id, |view| view.run.executions_started == 1).await;
    assert_eq!(
        serde_json::to_value(&after.requests[1]).unwrap(),
        serde_json::to_value(&before.requests[1]).unwrap(),
        "the visible budget audit is unchanged"
    );
    assert_eq!(
        after.requests[1].execution_id,
        before.requests[1].execution_id
    );
    assert_eq!(fixture.starts(), ["slow"]);
    fs::write(fixture.repo.join("release"), "finish").unwrap();
    let run = finish(child, 4);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["requests"][1]["status"], "BUDGET_EXHAUSTED");
}

const SLOW: &str = r#"import pathlib, sys, time
with pathlib.Path('events').open('a', encoding='utf-8') as events:
    events.write('start ' + sys.argv[1] + '\n')
while not pathlib.Path('release').exists():
    time.sleep(0.01)
with pathlib.Path('events').open('a', encoding='utf-8') as events:
    events.write('end ' + sys.argv[1] + '\n')
"#;

#[tokio::test]
async fn ready_evals_are_unlimited_unless_jobs_is_explicit() {
    for (args, cap) in [(vec!["--jobs", "2"], Some(2)), (vec![], None)] {
        let jobs = cap.unwrap_or(6);
        let fixture = Fixture::new((0..6).map(|id| eval(&format!("e{id}"), SLOW)).collect());
        let child = fixture.spawn(&args);
        wait_until(|| fixture.starts().len() >= jobs);
        let view = observed(&fixture, &child.run(), |view| {
            view.run.executions_started == jobs as u64
                && view
                    .requests
                    .iter()
                    .filter(|request| {
                        request.status() == artifactize_core::types::RequestStatus::Running
                            && request.child.is_some()
                    })
                    .count()
                    == jobs
        })
        .await;
        assert_eq!(view.run.executions_started, jobs as u64);
        fs::write(fixture.repo.join("release"), "").unwrap();
        let run = finish(child, 0);
        match cap {
            Some(limit) => assert_eq!(run["jobs"], limit),
            None => assert!(
                run.get("jobs").is_none(),
                "unlimited is an absent saved field"
            ),
        }
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

#[tokio::test]
async fn completion_releases_a_dependent_while_an_independent_eval_is_still_running() {
    let fixture = Fixture::new(vec![]);
    support::declaration::write(
        fixture.repo.join("index.artf"),
        r#"{"name":"root","basis":true}"#,
    )
    .unwrap();
    for (name, script, instruction) in [
        (
            "a-dependent",
            "import pathlib; assert pathlib.Path('../b-input/done').exists(); pathlib.Path('started').touch()",
            "Check {b-input}.",
        ),
        (
            "b-input",
            "import pathlib,time\nwhile not pathlib.Path('../c-slow/started').exists(): time.sleep(0.01)\npathlib.Path('done').touch()",
            "Check.",
        ),
        (
            "c-slow",
            "import pathlib,time\npathlib.Path('started').touch()\nwhile not pathlib.Path('release').exists(): time.sleep(0.01)",
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
    // These deliberately empty markers are the script's complete start/done signals.
    wait_until(|| fixture.repo.join("a-dependent/started").exists());
    observed(&fixture, &child.run(), |view| {
        view.requests[0].status() == artifactize_core::types::RequestStatus::Green
            && view.requests[1].status() == artifactize_core::types::RequestStatus::Green
            && view.requests[2].status() == artifactize_core::types::RequestStatus::Running
    })
    .await;
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
            .map(|id| eval(&format!("e{id}"), "import pathlib,sys\nwith pathlib.Path('events').open('a', encoding='utf-8') as events: events.write('start ' + sys.argv[1] + '\\n')"))
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
    let output = Command::new(support::os::cli())
        .arg("--state-dir")
        .arg(&fixture.state)
        .args(["run", "show", run["id"].as_str().unwrap()])
        .record_output()
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
    let mut invalid = eval("bad", "pass");
    invalid["profile"]["args"] = json!(["{test}/missing"]);
    let fixture = Fixture::new(vec![
        invalid,
        eval(
            "good",
            "import pathlib\nwith pathlib.Path('events').open('a', encoding='utf-8') as events: events.write('start good\\n')",
        ),
    ]);
    let run = fixture.run(&["--max-executions", "1"], 2);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["requests"][0]["errorCode"], "PREPARATION_FAILED");
    assert!(run["requests"][0]["startedAt"].is_null());
    assert_eq!(run["requests"][1]["status"], "GREEN");
    let fixture = Fixture::new(vec![eval("good", "pass")]);
    assert_eq!(
        fixture.run(&["--max-executions", "1"], 0)["executionsStarted"],
        1
    );
}

#[tokio::test]
async fn cancellation_kills_all_owned_groups_and_marks_queued_requests_cancelled() {
    {
        let fixture = Fixture::new(
            (0..5)
                .map(|id| process_tree_eval(&format!("e{id}"), &format!("e{id}.pids")))
                .collect(),
        );
        let child = fixture.spawn(&["--jobs", "3"]);
        wait_until(|| {
            (0..3).all(|id| recorded_pids(&fixture.repo.join(format!("e{id}.pids"))).is_some())
        });
        observed(&fixture, &child.run(), |view| {
            view.requests.iter().take(3).all(|request| {
                request.status() == artifactize_core::types::RequestStatus::Running
                    && request.child.is_some()
            })
        })
        .await;
        support::runs::cancel_active(&fixture.state, &fixture.repo);
        let run = finish(child, 2);
        assert_eq!(run["status"], "ERROR");
        assert_eq!(run["executionsStarted"], 3);
        for request in run["requests"].as_array().unwrap() {
            assert_eq!(request["status"], "ERROR");
            assert_eq!(request["errorCode"], "CANCELLED");
            assert!(request["result"].is_null());
        }
        for id in 0..3 {
            for pid in recorded_pids(&fixture.repo.join(format!("e{id}.pids"))).unwrap() {
                assert!(!support::os::running(pid), "process {pid} still running");
            }
        }
        assert!(!fixture.repo.join("e3.pids").exists());
    }
}

#[tokio::test]
async fn owned_run_drop_and_unwind_release_the_worker_and_groups_before_fixture_state() {
    for unwind in [false, true] {
        let fixture = Fixture::new(vec![process_tree_eval("controlled", "pids")]);
        let child = fixture.spawn(&[]);
        let id = child.run();
        let watcher = child.id();
        wait_until(|| recorded_pids(&fixture.repo.join("pids")).is_some());
        let before = observed(&fixture, &id, |view| {
            view.requests[0].status() == artifactize_core::types::RequestStatus::Running
                && view.requests[0].child.is_some()
        })
        .await;
        let artifactize_core::runs::Worker::Admitted { owner } = before.run.control.unwrap().worker
        else {
            panic!("controlled execution has an admitted worker");
        };
        let pids = recorded_pids(&fixture.repo.join("pids")).unwrap();
        // The move owns the guard on every exit, including assertion unwind.
        let result = std::panic::catch_unwind(move || {
            let _owned = child;
            assert!(!unwind, "fixture unwind probe");
        });
        assert_eq!(result.is_err(), unwind);
        let after = artifactize_core::store::read_run(&fixture.state, &id)
            .await
            .unwrap();
        assert_eq!(
            after.requests[0].error_code(),
            Some(artifactize_core::types::FailureCode::Cancelled)
        );
        assert!(
            !support::os::running(owner.pid.get()),
            "durable worker exited"
        );
        assert!(!support::os::running(watcher), "watcher was reaped");
        assert!(pids.into_iter().all(|pid| !support::os::running(pid)));
        assert!(
            fixture.state.exists(),
            "cleanup finishes before temporary state drops"
        );
    }
}

#[test]
fn run_guard_constructor_reaps_the_caller_if_setup_unwinds_before_announcement() {
    let fixture = Fixture::new(vec![]);
    // Hold a caller on its input pipe without submitting a durable Run. Missing stderr
    // deliberately exercises constructor unwind before announcement parsing can begin.
    let caller = Command::new(support::os::python_program())
        .args(["-c", "import sys; sys.stdin.read()"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = caller.id();
    let result = std::panic::catch_unwind(|| Child::new(caller, &fixture.state, &fixture.repo));
    assert!(result.is_err());
    assert!(
        !support::os::running(pid),
        "constructor guard kills and reaps its caller"
    );
    assert!(!fixture.state.exists(), "no Run was submitted");
}

#[test]
fn invalid_limits_fail_without_creating_state() {
    let fixture = Fixture::new(vec![eval("check", "pass")]);
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
        let mut check = eval("check", "import pathlib; pathlib.Path('ran').touch()");
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

#[tokio::test]
async fn cancellation_keeps_already_committed_evidence() {
    let fixture = Fixture::new(vec![
        eval("completed", "pass"),
        eval(
            "running",
            "import pathlib,time\npathlib.Path('started').touch()\nwhile not pathlib.Path('release').exists(): time.sleep(0.01)",
        ),
    ]);
    let child = fixture.spawn(&["--jobs", "1"]);
    // The empty start marker is intentional; saved request states are the cancellation barrier.
    wait_until(|| fixture.repo.join("started").exists());
    observed(&fixture, &child.run(), |view| {
        view.requests[0].status() == artifactize_core::types::RequestStatus::Green
            && view.requests[1].status() == artifactize_core::types::RequestStatus::Running
            && view.requests[1].child.is_some()
    })
    .await;
    support::runs::cancel_active(&fixture.state, &fixture.repo);
    let run = finish(child, 2);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert_eq!(run["requests"][0]["result"]["verdict"], "GREEN");
    assert_eq!(run["requests"][1]["errorCode"], "CANCELLED");
}
