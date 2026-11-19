//! Machine-wide Agent backend capacity from `$STATE/limits.json`, shared by every `verify`
//! process.

#[path = "support/run.rs"]
mod owned_run;
use owned_run::RunChild as Child;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::os::Launch;
use support::runs::RecordOutput;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use artifactize_core::{
    store::{QueueReason, RequestState},
    types::RequestStatus,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{FakeProvider, openai};

struct Fixture {
    // Release every registered Run before its temporary state or provider is dropped.
    root: owned_run::Root,
    provider: FakeProvider,
    /// Reviews in flight at the provider, and the most at once.
    active: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
    gate: support::Gate,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.gate.open();
    }
}

impl Fixture {
    /// A fake provider whose response is released explicitly by the test.
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        fs::create_dir_all(root.path().join("home")).unwrap();
        fs::create_dir_all(root.path().join("state")).unwrap();
        let (active, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let gate = support::Gate::default();
        gate.open();
        let provider = FakeProvider::start({
            let gate = gate.clone();
            let (active, most) = (active.clone(), most.clone());
            move |request| {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                gate.wait();
                active.fetch_sub(1, Ordering::SeqCst);
                openai::completed(
                    request,
                    vec![openai::message(&json!({"verdict":"GREEN"}).to_string())],
                    openai::usage(10, 5),
                )
            }
        });
        Self {
            root,
            provider,
            active,
            most,
            gate,
        }
    }

    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    /// A repository of `count` Agent-reviewed Artifacts named `{prefix}0`, `{prefix}1`, ...
    fn repo(&self, prefix: &str, count: usize) -> PathBuf {
        let repo = self.root.path().join(prefix);
        for index in 0..count {
            let name = format!("{prefix}{index}");
            let folder = repo.join(&name);
            fs::create_dir_all(&folder).unwrap();
            support::declaration::write(
                &(repo),
                folder.join("index.artf"),
                json!({"name":name,"fingerprint":false,"evals":[{"id":"review","title":"Review",
                    "profile":{"kind":"agent","backend":"openai","model":"fake-exact-model"},
                    "payload":{"instruction":"Review."}}]})
                .to_string(),
            )
            .unwrap();
        }
        self.root.track(&self.state(), &repo);
        repo
    }

    fn limits(&self, limits: Value) {
        fs::write(
            support::declaration::source_path(self.state().join("limits.json")),
            limits.to_string(),
        )
        .unwrap();
    }

    fn command(&self, repo: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(support::os::cli());
        command
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(self.state())
            .args(args)
            .envs(support::os::home_variables(self.root.path().join("home")))
            .env("ARTIFACTIZE_OPENAI_BASE_URL", self.provider.openai_base())
            .env("OPENAI_API_KEY", "fake-openai-key")
            .env("ARTIFACTIZE_REMOTE", "off");
        command
    }

    fn spawn(&self, repo: &Path, args: &[&str]) -> Child {
        let child = self
            .command(
                repo,
                &[&["verify", "--wait", "--all", "--json"][..], args].concat(),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .launch();
        Child::new(child, &self.state(), repo)
    }

    fn stimulus(&self) -> Connection {
        Connection::open(self.state().join("state.sqlite")).unwrap()
    }

    /// A fresh review must reach the provider while the one-slot limit is still in force.
    fn assert_slot_released(&self) {
        let probe = self.repo("probe", 1);
        let before = self.provider.requests().len();
        let run = self.spawn(&probe, &["--force"]);
        wait_until(|| self.provider.requests().len() > before);
        finish(run, 0);
        assert_eq!(self.provider.requests().len(), before + 1);
    }

    /// Hold an openai slot with an execution of this live process (the test).
    fn hold(&self) {
        let pid = support::os::current_pid();
        self.stimulus().execute(
            "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,backend,data) SELECT 'held',NULL,eval_def_hash,'RUNNING',?1,?2,'openai',json_set(data,'$.id','held','$.key',NULL,'$.status','RUNNING','$.result',NULL,'$.error',NULL,'$.errorCode',NULL,'$.completedAt',NULL,'$.ownerPid',?1,'$.ownerStartTime',?2) FROM executions LIMIT 1",
            rusqlite::params![pid, support::os::start_time(pid) as i64],
        )
        .unwrap();
    }
}

fn finish(child: Child, code: i32) -> Value {
    let output = child.wait_with_output().unwrap();
    checked(&output, code)
}

fn checked(output: &Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(20));
    while !condition() {
        assert!(Instant::now() < deadline, "condition was not reached");
        thread::sleep(Duration::from_millis(20));
    }
}

/// Observe the queued request in the Run that is actually waiting for the slot.
fn waits_for_openai(run: &Child) -> bool {
    run.saved().requests.iter().any(|request| {
        matches!(&request.state, RequestState::Queued {
            reason: Some(QueueReason::Slot { backend }), ..
        } if backend.as_str() == "openai")
    })
}

#[test]
fn verify_processes_on_one_machine_share_a_backend_limit() {
    let fixture = Fixture::new();
    let (first, second) = (fixture.repo("a", 3), fixture.repo("b", 3));
    fixture.limits(json!({"backends":{"openai":1,"anthropic":2}}));
    fixture.gate.close();
    let runs = [fixture.spawn(&first, &[]), fixture.spawn(&second, &[])];
    wait_until(|| runs.iter().any(waits_for_openai));
    fixture.gate.open();
    for run in runs {
        let run = finish(run, 0);
        assert_eq!(run["executionsStarted"], 3);
    }
    assert_eq!(fixture.most.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.provider.requests().len(), 6);
    fixture.assert_slot_released();

    // Without a limit the same Runs review in parallel.
    support::declaration::remove_file(fixture.state().join("limits.json")).unwrap();
    fixture.most.store(0, Ordering::SeqCst);
    fixture.gate.close();
    let runs = [fixture.spawn(&first, &[]), fixture.spawn(&second, &[])];
    wait_until(|| fixture.active.load(Ordering::SeqCst) > 1);
    fixture.gate.open();
    for run in runs {
        finish(run, 0);
    }
    assert!(fixture.most.load(Ordering::SeqCst) > 1);
    assert_eq!(fixture.active.load(Ordering::SeqCst), 0);
}

#[test]
fn a_waiting_request_consumes_no_budget_and_a_dead_owner_frees_its_slot() {
    let fixture = Fixture::new();
    let repo = fixture.repo("solo", 1);
    fixture.limits(json!({"backends":{"openai":1}}));
    // A first Run creates the state database; then a live process (this test) holds the slot.
    finish(fixture.spawn(&repo, &[]), 0);
    fixture.hold();
    let calls = fixture.provider.requests().len();
    let waiting = fixture.spawn(&repo, &["--force", "--max-executions", "1"]);
    wait_until(|| waits_for_openai(&waiting));
    // Waiting: no provider call and no executor start.
    assert_eq!(fixture.provider.requests().len(), calls);
    assert_eq!(waiting.saved().run.executions_started, 0);

    // The holder "crashes": the execution of an owner that is gone ends, and frees its slot.
    fixture
        .stimulus()
        .execute(
            "UPDATE executions SET owner_start_time=0 WHERE id='held'",
            [],
        )
        .unwrap();
    let run = finish(waiting, 0);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert!(run["requests"][0]["blockedReason"].is_null());
    assert_eq!(fixture.provider.requests().len(), calls + 1);
    assert_eq!(run["requests"][0]["options"]["backend"], "openai");
    fixture.assert_slot_released();
}

#[tokio::test]
async fn a_slot_waiter_finishes_by_reusing_a_forced_producers_completed_result() {
    let fixture = Fixture::new();
    let repo = fixture.repo("solo", 1);
    // Keep the waiter's one job occupied until its Agent request has reused the cache.
    // It cannot race through claim/reuse instead of the scheduler's cache-read path.
    let barrier = support::os::tcp::nonblocking_loopback_listener();
    let address = barrier.local_addr().unwrap();
    // A stable key lets the waiting Run reuse the forced producer's eventual result.
    support::declaration::write(
        &(repo),
        repo.join("solo0/index.artf"),
        json!({"name":"solo0","evals":[
            {"id":"review","title":"Review",
                "profile":{"kind":"agent","backend":"openai","model":"fake-exact-model"},
                "payload":{"instruction":"Review."}},
            {"id":"zz-hold","title":"Hold the job",
                "profile":{"kind":"runtime","command":support::os::python_program(),
                    "args":support::os::tcp::socket_gate_args(address, None)},
                "payload":{"instruction":"Wait for the fixture's release."}}
        ]})
        .to_string(),
    )
    .unwrap();
    fixture.limits(json!({"backends":{"openai":1}}));
    fixture.gate.close();
    // A forced execution holds the backend slot but does not claim its reuse key.
    let child = fixture
        .command(
            &repo,
            &[
                "verify",
                "--wait",
                "--eval",
                "solo0/review",
                "--force",
                "--json",
            ],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .launch();
    let producer = Child::new(child, &fixture.state(), &repo);
    wait_until(|| fixture.active.load(Ordering::SeqCst) == 1);
    let waiter = fixture.spawn(&repo, &["--jobs", "1"]);
    wait_until(|| waits_for_openai(&waiter));
    let mut held = None;
    wait_until(|| {
        held = barrier.accept().ok().map(|(stream, _)| stream);
        held.is_some()
    });
    let mut held = held.unwrap();

    fixture.gate.open();
    // Only the Agent eval was selected, so this producer's Artifact still has an obligation.
    let produced = finish(producer, 4);
    let waiter_id = waiter.run();
    wait_until(|| {
        waiter.saved().requests.iter().any(|request| {
            request.eval_id.as_str() == "solo0/review" && request.status() == RequestStatus::Green
        })
    });
    held.write_all(b"x").unwrap();
    drop(held);
    // A GREEN request alone is not completion: the Run itself must settle without cancel.
    owned_run::terminal(&fixture.state(), &waiter_id).await;
    let reused = finish(waiter, 0);
    assert_eq!(reused["status"], "GREEN");
    assert_eq!(
        reused["executionsStarted"], 1,
        "only the runtime job starts"
    );
    assert_eq!(reused["requests"][0]["status"], "GREEN");
    assert_eq!(reused["requests"][0]["provenance"]["runId"], produced["id"]);
}

#[test]
fn invalid_limits_fail_verify_and_doctor_before_any_run() {
    let fixture = Fixture::new();
    let repo = fixture.repo("solo", 1);
    for (limits, message) in [
        (json!({"backends":{"openai":0}}), "between 1 and 100000"),
        (json!({"backends":{"claude":2}}), "was removed in 0.5.0"),
        (json!({"backends":{"my provider":2}}), "backend must match"),
        (json!({"backend":{"openai":2}}), "unknown field"),
    ] {
        fixture.limits(limits);
        let output = fixture
            .command(&repo, &["verify", "--wait", "--all"])
            .record_output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("limits.json") && stderr.contains(message),
            "{stderr}"
        );
        let doctor = fixture
            .command(&repo, &["doctor", "--json"])
            .record_output()
            .unwrap();
        assert_eq!(doctor.status.code(), Some(1));
        let report: Value = serde_json::from_slice(&doctor.stdout).unwrap();
        let check = report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["name"] == "limits")
            .unwrap();
        assert_eq!(check["status"], "FAIL");
    }
    assert!(!fixture.state().join("state.sqlite").exists());
    fixture.limits(json!({"backends":{"codex":4,"openai":8}}));
    let doctor = fixture
        .command(&repo, &["doctor", "--json"])
        .record_output()
        .unwrap();
    let report: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    let check = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "limits")
        .unwrap()
        .clone();
    assert_eq!(check["status"], "PASS");
    assert_eq!(check["details"], json!({"backends":{"codex":4,"openai":8}}));
}

#[test]
fn a_request_waiting_for_a_slot_holds_no_job_slot() {
    let fixture = Fixture::new();
    let repo = fixture.repo("mixed", 1);
    let check = repo.join("check");
    fs::create_dir_all(&check).unwrap();
    support::declaration::write(
        &(repo),
        check.join("index.artf"),
        json!({
            "name":"check",
            "evals":[
                {
                    "id":"run",
                    "title":"Run",
                    "profile":{"kind":"runtime","command":support::os::true_program(),"args":[]},
                    "payload":{"instruction":"Run."},
                },
            ],
        })
        .to_string(),
    )
    .unwrap();
    fixture.limits(json!({"backends":{"openai":1}}));
    finish(fixture.spawn(&repo, &[]), 0);
    fixture.hold();
    // With one job, the Agent review waits for its slot and the runtime eval still runs.
    let run = fixture.spawn(&repo, &["--force", "--jobs", "1"]);
    wait_until(|| waits_for_openai(&run));
    wait_until(|| {
        run.saved().requests.iter().any(|request| {
            request.eval_id.as_str() == "check/run" && request.status() == RequestStatus::Green
        })
    });
    fixture
        .stimulus()
        .execute("UPDATE executions SET status='ERROR' WHERE id='held'", [])
        .unwrap();
    let run = finish(run, 0);
    assert_eq!(run["executionsStarted"], 2);
}
