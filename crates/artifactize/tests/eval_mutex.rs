//! Machine-wide mutex admission through real Run workers, commands and Agent reviews.

#[path = "support/run.rs"]
mod owned_run;
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

use std::{
    fs,
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use owned_run::RunChild;
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{FakeProvider, openai};

/// Failure bound only: all ordering uses provider gates, TCP barriers or saved request states.
const WAIT_BOUND: Duration = Duration::from_secs(30);

struct Fixture {
    root: owned_run::Root,
    provider: FakeProvider,
    gate: support::Gate,
    active: Arc<AtomicUsize>,
    listener: TcpListener,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.gate.open();
    }
}

impl Fixture {
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        fs::create_dir_all(root.path().join("state")).unwrap();
        let gate = support::Gate::default();
        let active = Arc::new(AtomicUsize::new(0));
        let provider = FakeProvider::start({
            let (gate, active) = (gate.clone(), active.clone());
            move |request| {
                active.fetch_add(1, Ordering::SeqCst);
                gate.wait();
                active.fetch_sub(1, Ordering::SeqCst);
                openai::completed(
                    request,
                    vec![openai::message(&json!({"verdict":"GREEN"}).to_string())],
                    openai::usage(10, 5),
                )
            }
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        Self {
            root,
            provider,
            gate,
            active,
            listener,
        }
    }

    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    fn repo(&self, name: &str, evals: Vec<Value>) -> PathBuf {
        let repo = self.root.path().join(name);
        fs::create_dir_all(&repo).unwrap();
        support::declaration::write(
            repo.join("index.artf"),
            json!({"name":name,"fingerprint":false,"evals":evals}).to_string(),
        )
        .unwrap();
        self.root.track(&self.state(), &repo);
        repo
    }

    fn runtime(&self, id: &str, tags: &[&str]) -> Value {
        let address = self.listener.local_addr().unwrap();
        json!({"id":id,"title":id,"mutex":tags,
            "profile":{"kind":"runtime","command":support::os::python_program(),
                "args":["-c", "import socket,sys\nwith socket.create_connection((sys.argv[1],int(sys.argv[2]))) as stream:\n    stream.sendall(sys.argv[3].encode())\n    assert stream.recv(1) == b'x'",address.ip().to_string(),address.port().to_string(),id]},
            "payload":{"instruction":"Wait for the fixture's release."}})
    }

    fn agent(&self, id: &str, tags: &[&str]) -> Value {
        json!({"id":id,"title":id,"mutex":tags,
            "profile":{"kind":"agent","backend":"openai","model":"fake-exact-model"},
            "payload":{"instruction":"Review."}})
    }

    fn spawn(&self, repo: &Path, extra: &[&str]) -> RunChild {
        let child = support::os::start(
            Command::new(support::os::cli())
                .arg("--repo")
                .arg(repo)
                .arg("--state-dir")
                .arg(self.state())
                .args(["verify", "--all", "--wait", "--json"])
                .args(extra)
                .envs(support::os::home_variables(self.root.path().join("home")))
                .env("ARTIFACTIZE_OPENAI_BASE_URL", self.provider.openai_base())
                .env("OPENAI_API_KEY", "fake-openai-key")
                .env("ARTIFACTIZE_REMOTE", "off"),
        );
        RunChild::new(child, &self.state(), repo)
    }

    fn observed(&self) -> Connection {
        Connection::open_with_flags(
            self.state().join("state.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }

    fn started(&self, expected: &str) -> TcpStream {
        let mut accepted = None;
        support::os::wait_until(WAIT_BOUND, || match self.listener.accept() {
            Ok((stream, _)) => {
                accepted = Some(stream);
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(error) => panic!("fixture accept: {error}"),
        });
        let mut stream = accepted.unwrap();
        stream
            .set_read_timeout(Some(support::os::patience(WAIT_BOUND)))
            .unwrap();
        let mut id = vec![0; expected.len()];
        stream.read_exact(&mut id).unwrap();
        assert_eq!(id, expected.as_bytes());
        stream
    }

    fn waiting(&self, run: &RunChild, eval: &str, holder: &str, holder_run: &RunChild) {
        let reason = format!(
            "Waiting for mutex tags held by eval {holder} in Run {}.",
            holder_run.run()
        );
        let db = self.observed();
        support::os::wait_until(WAIT_BOUND, || {
            db.query_row(
            "SELECT status,json_extract(data,'$.blockedReason') FROM requests WHERE run_id=?1 AND eval_id=?2",
            rusqlite::params![run.run(), eval],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        ).is_ok_and(|(status, saved)| status == "QUEUED" && saved.as_deref() == Some(&reason))
        });
    }
}

fn release(mut stream: TcpStream) {
    stream.write_all(b"x").unwrap();
}

fn finish(run: RunChild) -> Value {
    let output = run.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn two_runs_exclude_conflicting_runtime_and_agent_but_overlap_siblings_and_untagged() {
    let fixture = Fixture::new();
    let first_repo = fixture.repo("first", vec![fixture.runtime("hold", &["use-computer:db"])]);
    let first = fixture.spawn(&first_repo, &[]);
    let held = fixture.started("hold");
    let second_repo = fixture.repo(
        "second",
        vec![
            fixture.agent("conflict", &["use-computer:db:users"]),
            fixture.agent("sibling", &["use-computer:database"]),
            fixture.runtime("untagged", &[]),
        ],
    );
    let second = fixture.spawn(&second_repo, &["--max-executions", "3"]);
    let untagged = fixture.started("untagged");
    fixture.waiting(&second, "second/conflict", "first/hold", &first);
    support::os::wait_until(WAIT_BOUND, || fixture.active.load(Ordering::SeqCst) == 1);
    let started: i64 = fixture
        .observed()
        .query_row(
            "SELECT json_extract(data,'$.executionsStarted') FROM runs WHERE id=?",
            [second.run()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(started, 2, "waiting consumes no execution budget");
    release(held);
    support::os::wait_until(WAIT_BOUND, || fixture.active.load(Ordering::SeqCst) == 2);
    fixture.gate.open();
    release(untagged);
    assert_eq!(finish(first)["executionsStarted"], 1);
    assert_eq!(finish(second)["executionsStarted"], 3);
}

#[test]
fn all_tags_are_acquired_together_and_an_agent_holder_blocks_runtime_without_a_job_slot() {
    let fixture = Fixture::new();
    let first_repo = fixture.repo("first", vec![fixture.agent("hold", &["use-computer:db"])]);
    let first = fixture.spawn(&first_repo, &[]);
    support::os::wait_until(WAIT_BOUND, || fixture.active.load(Ordering::SeqCst) == 1);
    let second_repo = fixture.repo(
        "second",
        vec![
            fixture.runtime("a-wait", &["use-computer:browser", "use-computer:db"]),
            fixture.runtime("b-free", &["use-computer:browser"]),
        ],
    );
    let second = fixture.spawn(&second_repo, &["--jobs", "1"]);
    let free = fixture.started("b-free");
    fixture.waiting(&second, "second/a-wait", "first/hold", &first);
    fixture.gate.open();
    finish(first);
    release(free);
    release(fixture.started("a-wait"));
    let result = finish(second);
    assert_eq!(result["executionsStarted"], 2);
    assert!(
        result["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|request| request["blockedReason"].is_null())
    );
}

#[test]
fn root_equal_and_descendant_holders_exclude_conflicts_even_for_forced_runs() {
    for (held_tag, waiting_tag) in [
        ("use-computer", "use-computer:display"),
        ("use-computer:db:users", "use-computer:db"),
        ("use-computer:db", "use-computer:db"),
    ] {
        let fixture = Fixture::new();
        let first_repo = fixture.repo("first", vec![fixture.runtime("hold", &[held_tag])]);
        let second_repo = fixture.repo("second", vec![fixture.agent("wait", &[waiting_tag])]);
        let first = fixture.spawn(&first_repo, &["--force"]);
        let held = fixture.started("hold");
        let second = fixture.spawn(&second_repo, &["--force"]);
        fixture.waiting(&second, "second/wait", "first/hold", &first);
        assert_eq!(fixture.active.load(Ordering::SeqCst), 0);
        release(held);
        support::os::wait_until(WAIT_BOUND, || fixture.active.load(Ordering::SeqCst) == 1);
        fixture.gate.open();
        finish(first);
        finish(second);
    }
}

#[test]
fn a_crashed_worker_releases_its_tags_for_another_run() {
    let fixture = Fixture::new();
    let first_repo = fixture.repo("first", vec![fixture.runtime("hold", &["use-computer"])]);
    let second_repo = fixture.repo("second", vec![fixture.runtime("wait", &["use-computer"])]);
    let first = fixture.spawn(&first_repo, &[]);
    let _held = fixture.started("hold");
    let second = fixture.spawn(&second_repo, &[]);
    fixture.waiting(&second, "second/wait", "first/hold", &first);
    let control: String = fixture
        .observed()
        .query_row(
            "SELECT json_extract(data,'$.control') FROM runs WHERE id=?",
            [first.run()],
            |row| row.get(0),
        )
        .unwrap();
    let control: artifactize_core::runs::Control = serde_json::from_str(&control).unwrap();
    let artifactize_core::runs::Worker::Admitted { owner } = control.worker else {
        panic!("worker admitted before command starts")
    };
    support::os::kill_tree(owner.pid.get());
    release(fixture.started("wait"));
    finish(second);
    let output = first.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "crashed worker cannot report success"
    );
}

#[test]
fn cancellation_releases_tags_only_after_the_holder_stops() {
    let fixture = Fixture::new();
    let first_repo = fixture.repo("first", vec![fixture.runtime("hold", &["use-computer"])]);
    let second_repo = fixture.repo("second", vec![fixture.runtime("wait", &["use-computer"])]);
    let first = fixture.spawn(&first_repo, &[]);
    let _held = fixture.started("hold");
    let second = fixture.spawn(&second_repo, &[]);
    fixture.waiting(&second, "second/wait", "first/hold", &first);
    drop(first); // The fixture cancels and joins its owned Run, not only its watcher.
    release(fixture.started("wait"));
    finish(second);
}

#[test]
fn an_ended_run_cannot_keep_tags_even_when_the_recorded_process_is_alive() {
    let fixture = Fixture::new();
    let repo = fixture.repo("first", vec![fixture.runtime("hold", &["use-computer"])]);
    let first = fixture.spawn(&repo, &[]);
    release(fixture.started("hold"));
    finish(first);
    // Model an abandoned RUNNING lease whose Run already ended. The owner identity is
    // deliberately live, so process liveness alone cannot release this lease.
    Connection::open(fixture.state().join("state.sqlite")).unwrap().execute(
        "UPDATE executions SET status='RUNNING',owner_pid=?1,owner_start_time=?2,data=json_set(data,'$.status','RUNNING','$.result',NULL,'$.completedAt',NULL,'$.ownerPid',?1,'$.ownerStartTime',?2)",
        rusqlite::params![std::process::id(), support::os::start_time(std::process::id()) as i64],
    ).unwrap();
    let second = fixture.spawn(&repo, &["--force"]);
    release(fixture.started("hold"));
    finish(second);
}
