use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::JoinHandle,
    time::Duration,
};

use serde_json::{Value, json};

#[path = "support/run.rs"]
mod owned_run;
use owned_run::RunChild as Child;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

// One fixture-owned writer records acknowledged events. Concurrent Python text-file
// appends can overwrite each other on Windows; those lost records are not scheduler bugs.
struct EventLog {
    address: SocketAddr,
    task: Option<JoinHandle<io::Result<()>>>,
}

impl EventLog {
    fn new(repo: &Path) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let mut file = fs::File::create(repo.join("events")).unwrap();
        let task = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream?;
                let mut event = String::new();
                BufReader::new(&mut stream).read_line(&mut event)?;
                if event == "stop\n" {
                    return Ok(());
                }
                // A killed fixture can disconnect before publishing its next event.
                if event.is_empty() {
                    continue;
                }
                file.write_all(event.as_bytes())?;
                stream.write_all(b"ok\n")?;
            }
            Ok(())
        });
        Self {
            address,
            task: Some(task),
        }
    }
}

impl Drop for EventLog {
    fn drop(&mut self) {
        let stopped =
            TcpStream::connect(self.address).and_then(|mut stream| stream.write_all(b"stop\n"));
        let joined = self.task.take().unwrap().join();
        if !std::thread::panicking() {
            joined.expect("join the fixture event writer").unwrap();
            stopped.expect("stop the fixture event writer");
        }
    }
}

struct Fixture {
    // The writer is joined before its temporary files are removed, including on unwind.
    events: Option<EventLog>,
    _root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let released = self._root.release(&self.state, &self.repo);
        if !std::thread::panicking() {
            released.expect("release the Run before joining its fixture event writer");
        }
    }
}

impl Fixture {
    fn new(evals: Vec<Value>) -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        fs::create_dir(&repo).unwrap();
        support::declaration::write(
            repo.join("index.artf"),
            json!({"name":"test","fingerprint":false,"evals":evals}).to_string(),
        )
        .unwrap();
        let state = root.path().join("state");
        root.track(&state, &repo);
        Self {
            events: None,
            repo,
            state,
            _root: root,
        }
    }

    fn recorded(evals: impl FnOnce(SocketAddr) -> Vec<Value>) -> Self {
        let mut fixture = Self::new(vec![]);
        let events = EventLog::new(&fixture.repo);
        let evals = evals(events.address);
        fixture.events = Some(events);
        support::declaration::write(
            fixture.repo.join("index.artf"),
            json!({"name":"test","fingerprint":false,"evals":evals}).to_string(),
        )
        .unwrap();
        fixture
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
            .split_inclusive('\n')
            .filter_map(|line| {
                line.strip_suffix('\n')?
                    .strip_prefix("start ")
                    .map(str::to_owned)
            })
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
    owned_run::subscribed(&fixture.state, async |changes| {
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
                assert!(view.run.status().is_open());
                changes.next_reconciled().await;
            }
        })
        .await
        .unwrap()
    })
    .await
}

#[tokio::test]
async fn ipc_budget_decision_is_idempotent_while_an_execution_remains_running() {
    let fixture = Fixture::recorded(|address| {
        vec![slow_eval("slow", address), eval("zz-over-budget", "pass")]
    });
    let child = fixture.spawn(&["--jobs", "2", "--max-executions", "1"]);
    let id = child.run();
    let before = observed(&fixture, &id, |view| {
        view.run.executions_started == 1
            && view.requests[0].status() == artifactize_core::types::RequestStatus::Running
            && view.requests[1].status() == artifactize_core::types::RequestStatus::BudgetExhausted
    })
    .await;
    owned_run::subscribed(&fixture.state, async |notifications| {
        // Bound stalled fixture notifications without assigning the Run an execution lifetime.
        const NOTIFICATION_BOUND: Duration = Duration::from_secs(30);
        tokio::time::timeout(support::os::patience(NOTIFICATION_BOUND), async {
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
            fixture._root.track(&fixture.state, &other);
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
            // RUNNING is saved before Python publishes its start event; wait for that event itself.
            wait_until(|| fixture.starts().iter().any(|id| id == "slow"));
            assert_eq!(fixture.starts(), ["slow"]);
            fs::write(fixture.repo.join("release"), "finish").unwrap();
            let run = finish(child, 4);
            assert_eq!(run["executionsStarted"], 1);
            assert_eq!(run["requests"][1]["status"], "BUDGET_EXHAUSTED");
        })
        .await
        .expect("fixture notification states were observed");
    })
    .await;
}

fn slow_eval(id: &str, address: SocketAddr) -> Value {
    let mut check = eval(id, SLOW);
    check["profile"]["args"] = json!(["-c", SLOW, id, address.to_string()]);
    check
}

const SLOW: &str = r#"import pathlib, socket, sys, time
host, port = sys.argv[2].rsplit(':', 1)
def record(kind):
    with socket.create_connection((host, int(port))) as stream:
        stream.sendall((kind + ' ' + sys.argv[1] + '\n').encode())
        with stream.makefile('rb') as response:
            assert response.readline() == b'ok\n'
record('start')
while not pathlib.Path('release').exists():
    time.sleep(0.01)
record('end')
"#;

#[tokio::test]
async fn ready_evals_are_unlimited_unless_jobs_is_explicit() {
    for (args, cap) in [(vec!["--jobs", "2"], Some(2)), (vec![], None)] {
        let jobs = cap.unwrap_or(6);
        let fixture = Fixture::recorded(|address| {
            (0..6)
                .map(|id| slow_eval(&format!("e{id}"), address))
                .collect()
        });
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

#[tokio::test]
async fn a_settled_block_names_every_red_that_blocks_it_and_nothing_else() {
    let fixture = Fixture::new(vec![]);
    support::declaration::write(
        fixture.repo.join("index.artf"),
        r#"{"name":"root","basis":true}"#,
    )
    .unwrap();
    for (name, script, instruction) in [
        ("consumer", "pass", "Check {early} and {late}."),
        ("early", "raise SystemExit(1)", "Check."),
        (
            "late",
            "import pathlib,time\nwhile not pathlib.Path('release').exists(): time.sleep(0.01)\nraise SystemExit(1)",
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
    let child = fixture.spawn(&["--speculate"]);
    let status = |view: &artifactize_core::store::RunView, eval: &str| {
        let request = view
            .requests
            .iter()
            .find(|request| request.eval_id == eval)
            .unwrap();
        (request.status(), request.blocked_reason.clone())
    };
    // The block is saved while late/check still runs: only the RED known then is named.
    let blocked = observed(&fixture, &child.run(), |view| {
        status(view, "consumer/check").0 == artifactize_core::types::RequestStatus::Blocked
            && status(view, "late/check").0 == artifactize_core::types::RequestStatus::Running
    })
    .await;
    assert_eq!(
        status(&blocked, "consumer/check").1.as_deref(),
        Some("Dependency verdict RED: early/check")
    );
    fs::write(fixture.repo.join("late/release"), "").unwrap();
    let run = finish(child, 1);
    let consumer = run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["evalId"] == "consumer/check")
        .unwrap();
    assert_eq!(consumer["status"], "BLOCKED");
    assert_eq!(
        consumer["blockedReason"],
        "Dependency verdict RED: early/check, late/check"
    );
    assert_eq!(consumer["blockedBy"], json!(["early/check", "late/check"]));
}

#[test]
fn budget_is_shared_across_selected_evals_and_stops_new_starts() {
    let fixture = Fixture::recorded(|address| {
        (0..5)
            .map(|id| slow_eval(&format!("e{id}"), address))
            .collect()
    });
    // Let admitted executions finish immediately; their acknowledged events still share
    // one writer, so concurrent starts cannot overwrite each other's records on Windows.
    fs::write(fixture.repo.join("release"), "finish").unwrap();
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
        let control = before.run.control.unwrap();
        let artifactize_core::runs::Worker::Admitted { owner } = control.worker else {
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
        for owner in std::iter::once(owner).chain(control.supervisor) {
            assert!(
                !support::os::same_process_running(owner.pid.get(), owner.start_time),
                "durable worker and supervisor exited"
            );
        }
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
