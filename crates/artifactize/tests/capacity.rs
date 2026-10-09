//! Machine-wide Agent backend capacity from `$STATE/limits.json`, shared by every `verify`
//! process through the RUNNING executions of each backend in `state.sqlite`.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use support::{FakeProvider, openai};

struct Fixture {
    root: tempfile::TempDir,
    provider: FakeProvider,
    /// Reviews in flight at the provider, and the most at once.
    active: Arc<AtomicUsize>,
    most: Arc<AtomicUsize>,
}

impl Fixture {
    /// A fake OpenAI provider whose every review takes 300 ms and passes.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("home")).unwrap();
        fs::create_dir_all(root.path().join("state")).unwrap();
        let (active, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let provider = FakeProvider::start({
            let (active, most) = (active.clone(), most.clone());
            move |request| {
                let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(300));
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
                folder.join("index.artf"),
                json!({"name":name,"fingerprint":false,"evals":[{"id":"review","title":"Review",
                    "profile":{"kind":"agent","backend":"openai","model":"fake-exact-model"},
                    "payload":{"instruction":"Review."}}]})
                .to_string(),
            )
            .unwrap();
        }
        repo
    }

    fn limits(&self, limits: Value) {
        fs::write(self.state().join("limits.json"), limits.to_string()).unwrap();
    }

    fn command(&self, repo: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(self.state())
            .args(args)
            .env("HOME", self.root.path().join("home"))
            .env("ARTIFACTIZE_OPENAI_BASE_URL", self.provider.openai_base())
            .env("OPENAI_API_KEY", "fake-openai-key")
            .env("ARTIFACTIZE_REMOTE", "off");
        command
    }

    fn spawn(&self, repo: &Path, args: &[&str]) -> Child {
        self.command(repo, &[&["verify", "--all", "--json"][..], args].concat())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn database(&self) -> Connection {
        Connection::open(self.state().join("state.sqlite")).unwrap()
    }

    /// Slots held: RUNNING executions on a backend.
    fn slots(&self) -> u32 {
        self.database()
            .query_row(
                "SELECT count(*) FROM executions WHERE backend IS NOT NULL AND status='RUNNING'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// Hold an openai slot with an execution of this live process (the test).
    fn hold(&self) {
        self.database().execute(
            "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,backend,data) VALUES ('held',NULL,'held','RUNNING',?,?,'openai','{}')",
            rusqlite::params![std::process::id(), own_start_time() as i64],
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

/// The waiting reason a request shows while its backend has no free slot.
fn capacity_waiters(db: &Connection) -> u32 {
    db.query_row(
        "SELECT count(*) FROM requests WHERE status='QUEUED' AND json_extract(data,'$.blockedReason') LIKE 'Waiting for a free openai slot%'",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

#[test]
fn verify_processes_on_one_machine_share_a_backend_limit() {
    let fixture = Fixture::new();
    let (first, second) = (fixture.repo("a", 3), fixture.repo("b", 3));
    fixture.limits(json!({"backends":{"openai":1,"anthropic":2}}));
    let runs = [
        fixture.spawn(&first, &["--jobs", "3"]),
        fixture.spawn(&second, &["--jobs", "3"]),
    ];
    let mut seen_waiting = false;
    wait_until(|| {
        seen_waiting |= Connection::open(fixture.state().join("state.sqlite"))
            .is_ok_and(|db| capacity_waiters(&db) > 0);
        seen_waiting
    });
    for run in runs {
        let run = finish(run, 0);
        assert_eq!(run["executionsStarted"], 3);
    }
    assert_eq!(fixture.most.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.provider.requests().len(), 6);
    assert_eq!(fixture.slots(), 0, "every slot is released");

    // Without a limit the same Runs review in parallel.
    fs::remove_file(fixture.state().join("limits.json")).unwrap();
    fixture.most.store(0, Ordering::SeqCst);
    let runs = [
        fixture.spawn(&first, &["--jobs", "3"]),
        fixture.spawn(&second, &["--jobs", "3"]),
    ];
    for run in runs {
        finish(run, 0);
    }
    assert!(fixture.most.load(Ordering::SeqCst) > 1);
    assert_eq!(fixture.active.load(Ordering::SeqCst), 0);
}

/// This process's start time, as artifactize records owners.
fn own_start_time() -> u64 {
    support::os::start_time(std::process::id())
}

#[test]
fn a_waiting_request_consumes_no_budget_and_a_dead_owner_frees_its_slot() {
    let fixture = Fixture::new();
    let repo = fixture.repo("solo", 1);
    fixture.limits(json!({"backends":{"openai":1}}));
    // A first Run creates the state database; then a live process (this test) holds the slot.
    finish(fixture.spawn(&repo, &[]), 0);
    let db = fixture.database();
    fixture.hold();
    let calls = fixture.provider.requests().len();
    let waiting = fixture.spawn(&repo, &["--force", "--max-executions", "1"]);
    wait_until(|| capacity_waiters(&db) == 1);
    thread::sleep(Duration::from_millis(500));
    // Still waiting: no provider call and no executor start.
    assert_eq!(capacity_waiters(&db), 1);
    assert_eq!(fixture.provider.requests().len(), calls);
    let started: i64 = db
        .query_row(
            "SELECT json_extract(data,'$.executionsStarted') FROM runs WHERE status='RUNNING'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(started, 0);

    // The holder "crashes": the execution of an owner that is gone ends, and frees its slot.
    db.execute(
        "UPDATE executions SET owner_start_time=0 WHERE id='held'",
        [],
    )
    .unwrap();
    let run = finish(waiting, 0);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert!(run["requests"][0]["blockedReason"].is_null());
    assert_eq!(fixture.provider.requests().len(), calls + 1);
    assert_eq!(fixture.slots(), 0);
    let held: (String, String) = db
        .query_row(
            "SELECT status,json_extract(data,'$.errorCode') FROM executions WHERE id='held'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(held, ("ERROR".to_owned(), "OWNER_DIED".to_owned()));
    // Every execution names the backend whose slot it held.
    let backends: Vec<Option<String>> = db
        .prepare("SELECT DISTINCT backend FROM executions WHERE id!='held'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(backends, [Some("openai".to_owned())]);
}

#[test]
fn invalid_limits_fail_verify_and_doctor_before_any_run() {
    let fixture = Fixture::new();
    let repo = fixture.repo("solo", 1);
    for (limits, message) in [
        (json!({"backends":{"openai":0}}), "between 1 and 100000"),
        (json!({"backends":{"claude":2}}), "was removed in 0.5.0"),
        (json!({"backends":{"opneai":2}}), "unknown variant"),
        (json!({"backend":{"openai":2}}), "unknown field"),
    ] {
        fixture.limits(limits);
        let output = fixture
            .command(&repo, &["verify", "--all"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("limits.json") && stderr.contains(message),
            "{stderr}"
        );
        let doctor = fixture
            .command(&repo, &["doctor", "--json"])
            .output()
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
        .output()
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
        check.join("index.artf"),
        json!({"name":"check","evals":[{"id":"run","title":"Run",
            "profile":{"kind":"runtime","command":"true","args":[]},"payload":{"instruction":"Run."}}]})
        .to_string(),
    )
    .unwrap();
    fixture.limits(json!({"backends":{"openai":1}}));
    finish(fixture.spawn(&repo, &[]), 0);
    let db = fixture.database();
    fixture.hold();
    // With one job, the Agent review waits for its slot and the runtime eval still runs.
    let run = fixture.spawn(&repo, &["--force", "--jobs", "1"]);
    wait_until(|| capacity_waiters(&db) == 1);
    wait_until(|| {
        db.query_row(
            "SELECT count(*) FROM requests q JOIN runs r ON r.id=q.run_id WHERE r.status='RUNNING' AND json_extract(q.data,'$.evalId')='check/run' AND q.status='GREEN'",
            [],
            |row| row.get::<_, u32>(0),
        )
        .unwrap()
            == 1
    });
    db.execute("UPDATE executions SET status='ERROR' WHERE id='held'", [])
        .unwrap();
    let run = finish(run, 0);
    assert_eq!(run["executionsStarted"], 2);
}
