use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use artifactize_core::{
    runs, store,
    types::{RequestStatus, RunId},
};
use rusqlite::Connection;
use serde_json::{Value, json};
use support::os::{cat_program, echo_program, false_program, true_program};

#[path = "support/order.rs"]
mod order;

#[path = "support/run.rs"]
mod owned_run;
use owned_run::RunChild as Child;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

/// Bound fixture observation, not executor lifetimes or elapsed-time assertions.
const OBSERVATION_BOUND: Duration = Duration::from_secs(30);

struct Fixture {
    agent_endpoint: Option<String>,
    root: owned_run::Root,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        Self {
            agent_endpoint: None,
            state: root.path().join("state"),
            root,
        }
    }

    fn with_provider(provider: &support::FakeProvider) -> Self {
        let mut fixture = Self::new();
        fixture.agent_endpoint = Some(provider.openai_base());
        fixture
    }

    fn agent_environment(&self) -> Vec<(&str, &str)> {
        self.agent_endpoint
            .as_deref()
            .map_or_else(Vec::new, |endpoint| {
                vec![
                    ("OPENAI_API_KEY", "fake-openai-key"),
                    ("ARTIFACTIZE_OPENAI_BASE_URL", endpoint),
                ]
            })
    }

    fn agent_repo(&self, name: &str, model: &str) -> PathBuf {
        self.repo(
            name,
            json!({
                "name":"shared",
                "fingerprint":fingerprint("concurrent"),
                "evals":[{
                    "id":"check",
                    "title":"Review",
                    "profile":{"kind":"agent","backend":"openai","model":model},
                    "pass_schema":{"type":"object","properties":{"profile":{"type":"string"}},"required":["profile"]},
                    "payload":{"instruction":"Review."},
                }],
            }),
        )
    }

    fn repo(&self, name: &str, value: Value) -> PathBuf {
        let repo = self.root.path().join(name);
        write(&repo, "index.artf", value);
        repo
    }

    fn command(&self, repo: &Path, args: &[&str], code: i32) -> Value {
        self.root.track(&self.state, repo);
        let output = Command::new(support::os::cli())
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(&self.state)
            .env("ARTIFACTIZE_REMOTE", "off")
            .envs(self.agent_environment())
            .args(args)
            .arg("--json")
            .record_output()
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

    fn spawn(&self, repo: &Path, args: &[&str]) -> Child {
        self.root.track(&self.state, repo);
        let child = support::os::new_group(&mut Command::new(support::os::cli()))
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(&self.state)
            .env("ARTIFACTIZE_REMOTE", "off")
            .envs(self.agent_environment())
            .args(["verify", "--wait", "--all", "--json"])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Child::new(child, &self.state, repo)
    }

    fn announced(child: &mut Child) -> RunId {
        child.run()
    }

    fn context(&self, repo: &Path) -> runs::Context {
        self.root.track(&self.state, repo);
        runs::Context::new(repo, Some(&self.state), &support::os::cli()).unwrap()
    }

    /// Subscribe before the baseline and wait for this Run's ownership relation, not any
    /// transient QUEUED row in the whole fixture database.
    async fn observed_request(
        &self,
        repo: &Path,
        id: &RunId,
        ready: impl Fn(&store::Request) -> bool,
    ) -> store::Request {
        owned_run::subscribed(&self.state, async |changes| {
            let mut last = None;
            tokio::time::timeout(support::os::patience(OBSERVATION_BOUND), async {
                loop {
                    let view = store::read_run(&self.state, id).await.unwrap();
                    assert_eq!(view.run.repo_path, support::os::canonical(repo));
                    let request = &view.requests[0];
                    if ready(request) {
                        return store::Request::clone(request);
                    }
                    assert!(view.run.status().is_open(), "{view:?}");
                    last = Some(view);
                    changes.next_reconciled().await;
                }
            })
            .await
            .unwrap_or_else(|error| {
                panic!("cache ownership relation for {id} was not reached: {error}; {last:?}")
            })
        })
        .await
    }

    async fn completed(&self, repo: &Path, id: &RunId, child: Child, code: i32) -> Value {
        let ended = owned_run::terminal(&self.state, id).await;
        assert_eq!(ended.run.repo_path, support::os::canonical(repo));
        output(
            tokio::task::spawn_blocking(move || child.wait_with_output().unwrap())
                .await
                .unwrap(),
            code,
        )
    }

    /// Wait for a saved follower relation, not the owner's transient queued preparation.
    fn waiting_request(&self) -> Value {
        let mut request = Value::Null;
        wait_until(|| {
            let Ok(db) = Connection::open_with_flags(
                self.state.join("state.sqlite"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            ) else {
                return false;
            };
            let data = db
                .query_row::<String, _, _>(
                    "SELECT q.data FROM requests q JOIN executions e ON e.id=q.execution_id WHERE q.status='QUEUED' AND json_extract(e.data,'$.provenance.requestId')<>q.id LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();
            if let Some(data) = data {
                request = serde_json::from_str(&data).unwrap();
                true
            } else {
                false
            }
        });
        request
    }

    /// SQL observations cannot create or modify fixture state.
    fn observed(&self) -> Connection {
        Connection::open_with_flags(
            self.state.join("state.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }

    fn execution(&self, id: &str) -> Value {
        let data: String = Connection::open_with_flags(
            self.state.join("state.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
        .query_row("SELECT data FROM executions WHERE id=?", [id], |row| {
            row.get(0)
        })
        .unwrap();
        serde_json::from_str(&data).unwrap()
    }

    /// The same Artifact in its own repository: every shared repository has the same key. Its
    /// review runs `steps` from a file, where `STARTS` and `RELEASE` name the fixture's marker
    /// files.
    fn shared_repo(&self, name: &str, steps: &[&str]) -> PathBuf {
        let marker = |step: &&str| match *step {
            "STARTS" => support::os::path_text(&self.root.path().join("starts")),
            "RELEASE" => support::os::path_text(&self.root.path().join("release")),
            step => step.to_owned(),
        };
        let repo = self.repo(
            name,
            json!({
                "name":"shared",
                "fingerprint":fingerprint("concurrent"),
                "evals":[
                    {
                        "id":"check",
                        "title":"Review",
                        "profile":{
                            "kind":"runtime",
                            "command":support::os::steps(),
                            "args":["from", "review.txt"],
                        },
                        "payload":{"instruction":"Review."},
                    },
                ],
            }),
        );
        let lines: Vec<_> = steps.iter().map(marker).collect();
        fs::write(repo.join("review.txt"), lines.join("\n")).unwrap();
        repo
    }

    fn release(&self) {
        support::declaration::write(self.root.path().join("release"), "").unwrap();
    }

    /// Count completed marker records, not file creation or a partially written PID.
    fn starts(&self) -> usize {
        fs::read_to_string(self.root.path().join("starts")).map_or(0, |starts| {
            starts.bytes().filter(|byte| *byte == b'\n').count()
        })
    }

    /// Records of the key history.
    fn records(&self) -> u32 {
        self.count("executions WHERE completed_at IS NOT NULL")
    }

    fn count(&self, table: &str) -> u32 {
        Connection::open_with_flags(
            self.state.join("state.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    /// Seed records under the keys `seed-00000`, `seed-00001`, ... (each its own execution),
    /// least recently used first.
    fn seed_entries(&self, count: usize, bytes: i64) {
        let mut db = Connection::open(self.state.join("state.sqlite")).unwrap();
        let transaction = db.transaction().unwrap();
        for i in 0..count {
            let id = format!("seed-{i:05}");
            let key = format!("{i:064x}");
            transaction
                .execute(
                    "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,completed_at,bytes,last_used,data) SELECT ?1,?4,'seed',status,owner_pid,owner_start_time,?2,?3,?2,json_set(data,'$.id',?1,'$.key',?4) FROM executions LIMIT 1",
                    rusqlite::params![id, format!("2000-01-01T00:00:00.{i:09}Z"), bytes, key],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
    }

    /// Seed the LRU consumer's input after the real producer has exited. Publication order
    /// must not depend on sequential commands receiving different host clock ticks.
    fn last_used(&self, repo: &Path, run: &Value, at: &str) {
        let id = run["id"].as_str().unwrap().parse().unwrap();
        support::runs::finished_worker(&self.state, repo, &id, support::os::running);
        let at: artifactize_core::types::Timestamp = at.parse().unwrap();
        let db = Connection::open(self.state.join("state.sqlite")).unwrap();
        assert_eq!(
            db.execute(
                "UPDATE executions SET last_used=? WHERE id=? AND completed_at IS NOT NULL",
                rusqlite::params![at, run["requests"][0]["executionId"].as_str().unwrap()],
            )
            .unwrap(),
            1,
        );
    }

    /// Verify a new fingerprint from its own repository and return the reuse key; publishing
    /// the record runs LRU GC. Pin its subsequent collection position explicitly.
    fn publish(&self, name: &str, last_used: &str) -> String {
        let repo = self.repo(
            name,
            json!({
                "name":"publisher",
                "fingerprint":fingerprint(name),
                "evals":[eval("check", &[])],
            }),
        );
        let run = self.command(&repo, &["verify", "--wait", "--all"], 0);
        self.last_used(&repo, &run, last_used);
        key(&run, 0)
    }

    /// The key a running execution holds.
    fn running_key(&self) -> String {
        Connection::open_with_flags(
            self.state.join("state.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
        .query_row(
            "SELECT key FROM executions WHERE status='RUNNING'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn entries(&self) -> Vec<(String, String, i64, String, String)> {
        let db = Connection::open_with_flags(
            self.state.join("state.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut statement = db
            .prepare(
                "SELECT key,id,bytes,last_used,data FROM executions WHERE completed_at IS NOT NULL ORDER BY key,completed_at",
            )
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}

/// The provider receives the owner before the waiter attaches. Only an explicit release
/// supplies its verdict or non-retryable operational error; no child deadline orders it.
struct ClaimProvider {
    gate: support::Gate,
    entered: std::sync::mpsc::Receiver<()>,
    provider: support::FakeProvider,
}

impl ClaimProvider {
    fn new(owner_success: bool) -> Self {
        let gate = support::Gate::default();
        let (entered, receiving) = std::sync::mpsc::channel();
        let provider = support::FakeProvider::start({
            let gate = gate.clone();
            move |request| {
                if request.body["model"] == "owner" {
                    entered.send(()).unwrap();
                    gate.wait();
                    if !owner_success {
                        return support::Reply::Json(
                            404,
                            json!({"error":{"message":"The model does not exist","code":"model_not_found"}}),
                        );
                    }
                }
                support::openai::completed(
                    request,
                    vec![support::openai::message(
                        &json!({
                            "verdict":"GREEN", "profile":request.body["model"],
                        })
                        .to_string(),
                    )],
                    support::openai::usage(10, 2),
                )
            }
        });
        Self {
            gate,
            entered: receiving,
            provider,
        }
    }

    fn owner_started(&self) {
        self.entered
            .recv_timeout(support::os::patience(OBSERVATION_BOUND))
            .unwrap();
    }
}

/// Record the review's process, wait for the release and print a result.
const WAIT: &[&str] = &["pid", "STARTS", "wait", "RELEASE", "print", "original"];

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + support::os::patience(OBSERVATION_BOUND);
    while !condition() {
        assert!(Instant::now() < deadline, "condition was not reached");
        thread::sleep(Duration::from_millis(10));
    }
}

fn finish(mut child: Child, code: i32) -> Value {
    wait_until(|| child.try_wait().unwrap().is_some());
    output(child.wait_with_output().unwrap(), code)
}

fn output(output: Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// A suspended fixture worker is resumed on unwind before its Run owner cancels it.
struct Suspended(artifactize_core::process::ChildIdentity);
impl Suspended {
    fn new(pid: u32) -> Self {
        let suspended = Self(artifactize_core::process::ChildIdentity {
            pid: std::num::NonZeroU32::new(pid)
                .expect("owned worker PID")
                .into(),
            start_time: support::os::start_time(pid),
        });
        // Install ownership before the suspension call can unwind.
        support::os::suspend(pid);
        suspended
    }
}
impl Drop for Suspended {
    fn drop(&mut self) {
        if support::os::same_process_running(self.0.pid.get(), self.0.start_time) {
            support::os::resume(self.0.pid.get());
        }
    }
}

/// A deliberately killed worker cannot clean its runtime process group on unwind.
/// Keep its birth identity and observe exit before the fixture's inputs are removed.
fn orphan_group(fixture: &Fixture) -> owned_run::Processes {
    let pid = fs::read_to_string(fixture.root.path().join("starts"))
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mut processes = owned_run::Processes::default();
    processes.own(pid);
    processes
}

fn write(repo: &Path, path: &str, value: Value) {
    let path = repo.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    support::declaration::write(path, value.to_string()).unwrap();
}

fn eval(id: &str, steps: &[&str]) -> Value {
    json!({
        "id":id,
        "title":"Review",
        "profile":{"kind":"runtime","command":support::os::steps(),"args":steps},
        "payload":{"instruction":"Review."},
    })
}

fn fingerprint(key: &str) -> Value {
    json!({"script":{"command":echo_program(),"args":[key]}})
}

/// The reuse key of a Run's request.
fn key(run: &Value, index: usize) -> String {
    run["requests"][index]["key"].as_str().unwrap().to_owned()
}

fn request<'a>(view: &'a Value, id: &str) -> &'a Value {
    view["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["evalId"] == id)
        .unwrap()
}

#[test]
fn cross_repo_red_reuse_keeps_original_audit_and_blocks_gates_after_source_deletion() {
    let fixture = Fixture::new();
    // The same Artifact, under its own name, in another repository with another eval id.
    let source = fixture.repo(
        "source",
        json!({
            "name":"dependency",
            "fingerprint":fingerprint("shared:red"),
            "evals":[eval("first", &["print", "original", "exit", "7"])],
        }),
    );
    // The path artifactize records, taken before the source is deleted below.
    let source_path = support::os::canonical(&source);
    let original = fixture.command(&source, &["verify", "--wait", "--all"], 1);
    let original_request = &original["requests"][0];
    assert!(original_request["child"]["pid"].is_number());
    assert!(original_request["usage"].is_null());
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(fixture.records(), 1);
    fs::remove_dir_all(&source).unwrap();
    let target = fixture.repo("target", json!({"name":"root","basis":true}));
    let mut same = eval("second", &["print", "original", "exit", "7"]);
    same["title"] = json!("Renamed review");
    write(
        &target,
        "dependency/index.artf",
        json!({"name":"dependency","fingerprint":fingerprint("shared:red"),"evals":[same]}),
    );
    let mut consumer = eval("check", &["touch", "must-not-run"]);
    consumer["payload"]["instruction"] = json!("Check {dependency}.");
    write(
        &target,
        "consumer/index.artf",
        json!({"name":"consumer","evals":[consumer]}),
    );

    let before = fixture.entries();
    let status = fixture.command(&target, &["status", "consumer", "--recursive"], 1);
    let states = status["evals"].as_array().unwrap();
    assert_eq!(
        states
            .iter()
            .find(|r| r["id"] == "dependency/second")
            .unwrap()["action"],
        "reuse"
    );
    assert_eq!(
        states.iter().find(|r| r["id"] == "consumer/check").unwrap()["action"],
        "blocked"
    );
    assert_eq!(
        fixture.entries(),
        before,
        "status does not touch access times"
    );
    let reused = fixture.command(&target, &["verify", "--wait", "consumer", "--recursive"], 1);
    let hit = request(&reused, "dependency/second");
    for field in ["result", "profile", "executionId", "provenance", "usage"] {
        assert_eq!(hit[field], original_request[field], "{field}");
    }
    assert_eq!(hit["requestedProfile"], hit["profile"]);
    assert_eq!(
        hit["provenance"]["repoPath"],
        support::os::path_text(&source_path)
    );
    assert_eq!(hit["provenance"]["evalId"], "dependency/first");
    assert_eq!(
        hit["provenance"]["completedAt"],
        original_request["completedAt"]
    );
    for field in ["child", "argv", "runDir", "startedAt"] {
        assert!(hit[field].is_null(), "{field}");
    }
    assert_eq!(request(&reused, "consumer/check")["status"], "BLOCKED");
    assert_eq!(reused["validation"]["satisfied"], false);
    assert!(!target.join("consumer/must-not-run").exists());
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(
        fixture.command(
            &source,
            &["run", "show", original["id"].as_str().unwrap()],
            0
        ),
        original
    );
    assert_eq!(
        fixture.command(&target, &["run", "show", reused["id"].as_str().unwrap()], 0),
        reused
    );
    let nonrecursive = fixture.command(&target, &["verify", "--wait", "consumer"], 1);
    assert_eq!(nonrecursive["requests"].as_array().unwrap().len(), 1);
    assert_eq!(nonrecursive["requests"][0]["status"], "BLOCKED");
}

#[test]
fn distinct_evals_on_one_fingerprint_execute_and_status_reuses_each_definition() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({
            "name":"test",
            "fingerprint":fingerprint("shared"),
            "evals":[
                {
                    "id":"pass",
                    "title":"Pass",
                    "profile":{"kind":"runtime","command":true_program(),"args":[]},
                    "payload":{"instruction":"Review."},
                },
                {
                    "id":"fail",
                    "title":"Fail",
                    "profile":{"kind":"runtime","command":false_program(),"args":[]},
                    "payload":{"instruction":"Review."},
                },
            ],
        }),
    );
    let run = fixture.command(&repo, &["verify", "--wait", "--all", "--jobs", "1"], 1);
    let pass = &run["requests"][1];
    let fail = &run["requests"][0];
    assert_eq!(pass["status"], "GREEN");
    assert_eq!(fail["status"], "RED");
    assert_ne!(pass["executionId"], fail["executionId"]);
    assert_ne!(pass["evalDefHash"], fail["evalDefHash"]);
    assert_eq!(pass["evalDefHash"].as_str().unwrap().len(), 64);
    assert_eq!(pass["provenance"]["evalDefHash"], pass["evalDefHash"]);
    assert_eq!(fixture.count("executions"), 2);
    assert_eq!(fixture.records(), 2);
    let status = fixture.command(&repo, &["status"], 1);
    assert_eq!(status["evals"][1]["state"], "PASS");
    assert_eq!(status["evals"][0]["state"], "RED");
    assert_eq!(status["counts"]["reuse"], 2);
    let hit = fixture.command(&repo, &["verify", "--wait", "--all"], 1);
    for (new, original) in hit["requests"]
        .as_array()
        .unwrap()
        .iter()
        .zip(run["requests"].as_array().unwrap())
    {
        assert_eq!(new["executionId"], original["executionId"]);
        assert!(new["child"].is_null());
    }
    assert_eq!(fixture.count("executions"), 2);
    assert_eq!(
        fixture.command(&repo, &["run", "show", run["id"].as_str().unwrap()], 0),
        run
    );
    let entries = fixture.command(&repo, &["cache", "list"], 0);
    assert_eq!(entries.as_array().unwrap().len(), 2);
    // One fingerprint, two eval strategies: two keys.
    let (pass_key, fail_key) = (key(&run, 1), key(&run, 0));
    assert_ne!(pass_key, fail_key);
    let cached = fixture.command(&repo, &["cache", "show", &pass_key], 0);
    assert_eq!(cached["evalDefHash"], pass["evalDefHash"]);
    assert_eq!(cached["id"], pass["executionId"]);
    assert_eq!(cached["fingerprints"], json!({"test":"shared"}));
    assert_eq!(
        fixture.command(&repo, &["cache", "rm", &pass_key], 0),
        json!({"removed":true})
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "show", &pass_key], 4),
        Value::Null
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "show", &fail_key], 0)["id"],
        fail["executionId"]
    );
}

#[test]
fn fingerprint_false_executes_each_time_and_never_reads_or_publishes_cache() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({
            "name":"test",
            "fingerprint":fingerprint("cached"),
            "evals":[eval("check", &[])],
        }),
    );
    fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    let before = fixture.entries();
    write(
        &repo,
        "index.artf",
        json!({
            "name":"test",
            "fingerprint":false,
            "evals":[eval("check", &["append", "starts", "run", "exit", "8"])],
        }),
    );
    for _ in 0..2 {
        let run = fixture.command(&repo, &["verify", "--wait", "--all"], 1);
        assert!(run["requests"][0]["fingerprint"].is_null());
        assert!(run["requests"][0]["child"]["pid"].is_number());
    }
    assert_eq!(
        fs::read_to_string(repo.join("starts")).unwrap(),
        "run\nrun\n"
    );
    assert_eq!(fixture.entries(), before);
    assert_eq!(fixture.count("executions"), 3);
    let status = fixture.command(&repo, &["status"], 1);
    assert_eq!(status["evals"][0]["action"], "execute");
}

#[test]
fn errors_are_audited_but_never_published() {
    let fixture = Fixture::new();
    // A missing fixture program fails operationally on every OS without a real deadline.
    let (program, args, code) = support::os::runtime_failures()
        .into_iter()
        .find(|(_, _, code)| *code == "SPAWN_FAILED")
        .unwrap();
    let mut failed = eval("check", &[]);
    failed["profile"]["command"] = json!(program);
    failed["profile"]["args"] = json!(args);
    let repo = fixture.repo(
        "repo",
        json!({
            "name":"test",
            "fingerprint":fingerprint("retryable"),
            "evals":[failed],
        }),
    );
    for _ in 0..2 {
        let run = fixture.command(&repo, &["verify", "--wait", "--all"], 2);
        assert_eq!(run["requests"][0]["errorCode"], code);
        assert!(run["requests"][0]["result"].is_null());
        assert_eq!(fixture.records(), 0);
    }
    assert_eq!(fixture.count("executions"), 2);
    write(
        &repo,
        "index.artf",
        json!({
            "name":"test",
            "fingerprint":fingerprint("retryable"),
            "evals":[eval("check", &[])],
        }),
    );
    fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    assert_eq!(fixture.records(), 1);
}

#[test]
fn force_executes_and_adds_a_newer_record_that_later_runs_reuse_but_dependencies_reuse() {
    let fixture = Fixture::new();
    let check = eval("check", &["from", "review.txt"]);
    let repo = fixture.repo(
        "repo",
        json!({
            "name":"test",
            "fingerprint":fingerprint("forced"),
            "evals":[check],
        }),
    );
    fs::write(repo.join("review.txt"), "exit\n7\n").unwrap();
    let original = fixture.command(&repo, &["verify", "--wait", "--all"], 1);
    assert_eq!(fixture.entries().len(), 1);
    fs::write(repo.join("review.txt"), "append\nstarts\nforce\n").unwrap();
    let forced = fixture.command(&repo, &["verify", "--wait", "--all", "--force"], 0);
    assert_ne!(
        forced["requests"][0]["executionId"],
        original["requests"][0]["executionId"]
    );
    assert!(forced["requests"][0]["child"]["pid"].is_number());
    // Seed the record order: sequential executions can finish within one clock tick.
    for (run, completed) in [
        (&original, "2000-01-01T00:00:00.000000000Z"),
        (&forced, "2000-01-02T00:00:00.000000000Z"),
    ] {
        order::completed_at(&fixture.state, &repo, run, completed);
    }
    // The forced result joins the key's history next to the original.
    let after = fixture.entries();
    assert_eq!(after.len(), 2);
    assert!(after.iter().all(|entry| entry.0 == key(&original, 0)));
    let status = fixture.command(&repo, &["status", "--force"], 1);
    assert_eq!(status["evals"][0]["action"], "execute");
    assert_eq!(fixture.entries(), after);
    // The latest record wins: the next ordinary verify reuses the forced GREEN.
    let next = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    assert_eq!(
        next["requests"][0]["executionId"],
        forced["requests"][0]["executionId"]
    );
    assert_eq!(fs::read_to_string(repo.join("starts")).unwrap(), "force\n");
    let history = fixture.command(
        &repo,
        &["cache", "show", &key(&original, 0), "--history"],
        0,
    );
    let ids: Vec<_> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|record| record["id"].clone())
        .collect();
    assert_eq!(
        ids,
        [
            forced["requests"][0]["executionId"].clone(),
            original["requests"][0]["executionId"].clone()
        ]
    );
    // `cache list` shows the latest record per key; `--history` shows every record.
    let latest = fixture.command(&repo, &["cache", "list"], 0);
    assert_eq!(latest.as_array().unwrap().len(), 1);
    assert_eq!(
        latest[0]["executionId"],
        forced["requests"][0]["executionId"]
    );
    assert_eq!(latest[0]["verdict"], "GREEN");
    assert_eq!(latest[0]["records"], 2);
    let all = fixture.command(&repo, &["cache", "list", "--history"], 0);
    let verdicts: Vec<_> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["verdict"].as_str().unwrap())
        .collect();
    assert_eq!(verdicts, ["GREEN", "RED"]);

    let dependency = fixture.repo(
        "dep",
        json!({
            "name":"dep",
            "fingerprint":fingerprint("dependency"),
            "evals":[eval("check", &[])],
        }),
    );
    fixture.command(&dependency, &["verify", "--wait", "--all"], 0);
    write(
        &repo,
        "dependency/index.artf",
        json!({
            "name":"dep",
            "fingerprint":fingerprint("dependency"),
            "evals":[eval("check", &[])],
        }),
    );
    let forced = fixture.command(
        &repo,
        &["verify", "--wait", "test", "--recursive", "--force"],
        0,
    );
    assert_eq!(request(&forced, "test/check")["force"], true);
    assert_eq!(request(&forced, "dep/check")["force"], false);
    assert!(request(&forced, "dep/check")["child"].is_null());

    let absent = fixture.repo(
        "absent",
        json!({
            "name":"absent",
            "fingerprint":fingerprint("never-published"),
            "evals":[eval("check", &[])],
        }),
    );
    let before = fixture.entries().len();
    let forced = fixture.command(&absent, &["verify", "--wait", "--all", "--force"], 0);
    assert_eq!(fixture.entries().len(), before + 1);
    assert_eq!(
        fixture.command(&absent, &["verify", "--wait", "--all"], 0)["requests"][0]["executionId"],
        forced["requests"][0]["executionId"]
    );
}
#[test]
fn status_uses_current_fingerprint_and_only_prepares_the_selected_closure() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({
            "name":"test",
            "fingerprint":{"script":{"command":cat_program(),"args":["key"]}},
            "evals":[eval("check", &["touch", "executed"])],
        }),
    );
    fs::write(repo.join("key"), "first").unwrap();
    fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    fs::remove_file(repo.join("executed")).unwrap();
    assert_eq!(
        fixture.command(&repo, &["status"], 0)["evals"][0]["action"],
        "reuse"
    );
    fs::write(repo.join("key"), "changed").unwrap();
    assert_eq!(
        fixture.command(&repo, &["status"], 1)["evals"][0]["action"],
        "execute"
    );
    fs::write(repo.join("key"), "invalid key").unwrap();
    assert!(
        fixture.command(&repo, &["status"], 2)["error"]
            .as_str()
            .unwrap()
            .contains("Fingerprint script")
    );
    assert!(!repo.join("executed").exists());
    write(
        &repo,
        "selected/index.artf",
        json!({
            "name":"selected",
            "fingerprint":fingerprint("isolated"),
            "evals":[eval("check", &["touch", "must-not-run"])],
        }),
    );
    let selected = fixture.command(&repo, &["status", "selected"], 1);
    assert_eq!(selected["evals"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.count("runs"), 1);
    assert!(!repo.join("selected/must-not-run").exists());
}

#[test]
fn concurrent_repos_claim_once_and_poll_for_the_original_red_result() {
    let fixture = Fixture::new();
    let first = fixture.shared_repo("first", &[WAIT, &["exit", "7"]].concat());
    let second = fixture.shared_repo("second", &[WAIT, &["exit", "7"]].concat());
    let owner = fixture.spawn(&first, &[]);
    let waiter = fixture.spawn(&second, &[]);
    let waiting = fixture.waiting_request();
    wait_until(|| fixture.starts() > 0);
    assert_eq!(fixture.starts(), 1);
    let execution = fixture.execution(waiting["executionId"].as_str().unwrap());
    assert_eq!(execution["status"], "RUNNING");
    assert!(execution["completedAt"].is_null());
    assert!(execution["ownerStartTime"].as_u64().unwrap() > 0);
    assert_eq!(
        fixture.command(&second, &["status"], 1)["evals"][0]["action"],
        "wait"
    );
    assert_eq!(fixture.count("executions"), 1);
    fixture.release();
    let first = finish(owner, 1);
    let second = finish(waiter, 1);
    for field in ["executionId", "result", "profile", "provenance"] {
        assert_eq!(
            first["requests"][0][field], second["requests"][0][field],
            "{field}"
        );
    }
    assert_eq!(fixture.starts(), 1);
    assert_eq!(fixture.records(), 1);
    let follower = if first["requests"][0]["child"].is_null() {
        &first
    } else {
        &second
    };
    assert!(follower["requests"][0]["startedAt"].is_null());
    assert!(follower["requests"][0]["blockedReason"].is_null());
    let owner = if follower == &first { &second } else { &first };
    assert!(owner["requests"][0]["source"].is_null());
    assert_eq!(
        follower["requests"][0]["source"],
        json!({"runId":owner["id"],"requestId":owner["requests"][0]["id"],"kind":"joined"})
    );
}

#[test]
fn schema_seven_live_claim_upgrades_without_starting_a_duplicate() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    let target = fixture.shared_repo("target", WAIT);
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() == 1);
    // Pause the current binary while supplying the prior schema: only the upgrading
    // reader may query the database until its current columns have been restored.
    let suspended = Suspended::new(support::runs::worker_pid(&fixture.state, &source));
    // Supply the prior schema, including a genuinely live owner, at the upgrade seam.
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute_batch(&format!("BEGIN IMMEDIATE;
        {}
        DROP INDEX active_key;
        ALTER TABLE executions DROP COLUMN profile_identity;
        CREATE UNIQUE INDEX active_key ON executions(key) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN');
        PRAGMA user_version=7;
        COMMIT;", support::schema::EIGHT)).unwrap();
    assert_eq!(
        fixture.command(&target, &["status"], 1)["evals"][0]["action"],
        "wait"
    );
    drop(suspended);
    let waiter = fixture.spawn(&target, &[]);
    fixture.waiting_request();
    fixture.release();
    let first = finish(owner, 0);
    let second = finish(waiter, 0);
    assert_eq!(
        first["requests"][0]["executionId"],
        second["requests"][0]["executionId"]
    );
    assert_eq!(fixture.starts(), 1);
}

#[test]
fn killed_owner_is_reclaimed_by_a_waiting_verify() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    let target = fixture.shared_repo(
        "target",
        &["append", "STARTS", "replacement", "print", "recovered"],
    );
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let orphans = orphan_group(&fixture);
    let waiter = fixture.spawn(&target, &["--max-executions", "1"]);
    let waiting = fixture.waiting_request();
    let second_waiter = fixture.spawn(&target, &[]);
    wait_until(|| {
        fixture.count("requests WHERE status='QUEUED' AND execution_id IS NOT NULL") == 2
    });
    let worker = fixture.execution(waiting["executionId"].as_str().unwrap())["ownerPid"]
        .as_u64()
        .unwrap() as u32;
    support::os::kill(worker);
    finish(owner, 2);
    let recovered = finish(waiter, 0);
    let second = finish(second_waiter, 0);
    assert_eq!(
        second["requests"][0]["executionId"],
        recovered["requests"][0]["executionId"]
    );
    assert_eq!(
        second["executionsStarted"].as_u64().unwrap()
            + recovered["executionsStarted"].as_u64().unwrap(),
        1
    );
    let dead = fixture.execution(waiting["executionId"].as_str().unwrap());
    assert_eq!(dead["status"], "ERROR");
    assert_eq!(dead["errorCode"], "OWNER_DIED");
    assert!(dead["completedAt"].is_string());
    assert_eq!(recovered["requests"][0]["result"]["stdout"], "recovered");
    assert_ne!(
        recovered["requests"][0]["executionId"],
        waiting["executionId"]
    );
    assert_eq!(fixture.count("executions"), 2);
    assert_eq!(fixture.records(), 1);
    assert_eq!(fixture.starts(), 2);
    drop(orphans);
}

#[test]
fn joined_owner_error_is_shared_but_not_a_later_cache_hit() {
    let provider = ClaimProvider::new(false);
    let fixture = Fixture::with_provider(&provider.provider);
    let source = fixture.agent_repo("source", "owner");
    let target = fixture.agent_repo("target", "owner");
    let owner = fixture.spawn(&source, &[]);
    provider.owner_started();
    let waiter = fixture.spawn(&target, &[]);
    let waiting = fixture.waiting_request();
    assert!(
        waiting["blockedReason"]
            .as_str()
            .unwrap()
            .contains(waiting["executionId"].as_str().unwrap())
    );
    provider.gate.open();
    let failed = finish(owner, 2);
    let shared = finish(waiter, 2);
    for field in ["executionId", "error", "errorCode", "provenance"] {
        assert_eq!(
            shared["requests"][0][field], failed["requests"][0][field],
            "{field}"
        );
    }
    assert_eq!(shared["requests"][0]["errorCode"], "PROVIDER_ERROR");
    assert_eq!(shared["executionsStarted"], 0);
    assert_eq!(shared["requests"][0]["source"]["kind"], "joined");
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(fixture.records(), 0);
    assert_eq!(provider.provider.requests().len(), 1);
    let later = fixture.command(&target, &["verify", "--wait", "--all"], 2);
    assert_eq!(later["executionsStarted"], 1);
    assert_ne!(later["requests"][0]["executionId"], waiting["executionId"]);
}

#[test]
fn different_profiles_execute_independently_for_the_same_reuse_key() {
    let provider = ClaimProvider::new(true);
    let fixture = Fixture::with_provider(&provider.provider);
    let source = fixture.agent_repo("source", "owner");
    let target = fixture.agent_repo("target", "retry");
    let mut owner = fixture.spawn(&source, &[]);
    provider.owner_started();
    assert_eq!(
        fixture.command(&target, &["status"], 1)["evals"][0]["action"],
        "execute"
    );
    let other = finish(fixture.spawn(&target, &[]), 0);
    assert!(owner.try_wait().unwrap().is_none());
    assert_eq!(other["executionsStarted"], 1);
    assert_eq!(other["requests"][0]["profile"]["model"], "retry");
    assert_eq!(fixture.count("executions WHERE status='RUNNING'"), 1);
    provider.gate.open();
    let original = finish(owner, 0);
    assert_eq!(other["requests"][0]["key"], original["requests"][0]["key"]);
    assert_ne!(
        other["requests"][0]["executionId"],
        original["requests"][0]["executionId"]
    );
    assert_eq!(provider.provider.requests().len(), 2);
    assert_eq!(fixture.records(), 2);
}

#[tokio::test]
async fn cancelling_a_joined_run_leaves_its_owner_running_and_cancelling_the_owner_releases_the_claim()
 {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    let target = fixture.shared_repo("target", &["append", "STARTS", "retry", "print", "retried"]);
    // These live claims finish only through the test's cancellation and release stimuli.
    let mut owner = fixture.spawn(&source, &[]);
    let owner_id = Fixture::announced(&mut owner);
    let owned = fixture
        .observed_request(&source, &owner_id, |request| {
            request.status() == RequestStatus::Running && fixture.starts() == 1
        })
        .await;
    let mut waiter = fixture.spawn(&target, &[]);
    let waiter_id = Fixture::announced(&mut waiter);
    let waiting = fixture
        .observed_request(&target, &waiter_id, |request| {
            request.status() == RequestStatus::Queued
                && request.execution_id == owned.execution_id
                && request.child.is_none()
        })
        .await;
    runs::cancel(&fixture.context(&target), &waiter_id)
        .await
        .unwrap();
    let cancelled = fixture.completed(&target, &waiter_id, waiter, 2).await;
    assert_eq!(cancelled["requests"][0]["errorCode"], "CANCELLED");
    assert!(cancelled["requests"][0]["child"].is_null());
    assert!(owner.try_wait().unwrap().is_none());
    assert_eq!(
        fixture.execution(waiting.execution_id.as_deref().unwrap())["status"],
        "RUNNING"
    );
    assert_eq!(fixture.starts(), 1);
    let mut waiter = fixture.spawn(&target, &[]);
    let next_id = Fixture::announced(&mut waiter);
    fixture
        .observed_request(&target, &next_id, |request| {
            request.status() == RequestStatus::Queued
                && request.execution_id == owned.execution_id
                && request.child.is_none()
        })
        .await;
    runs::cancel(&fixture.context(&source), &owner_id)
        .await
        .unwrap();
    let cancelled = fixture.completed(&source, &owner_id, owner, 2).await;
    assert_eq!(cancelled["requests"][0]["errorCode"], "CANCELLED");
    let execution = fixture.execution(waiting.execution_id.as_deref().unwrap());
    assert_eq!(execution["errorCode"], "CANCELLED");
    let recovered = fixture.completed(&target, &next_id, waiter, 0).await;
    assert_eq!(recovered["requests"][0]["result"]["stdout"], "retried");
    assert_eq!(fixture.count("executions"), 2);
    let pid = cancelled["requests"][0]["child"]["pid"].as_u64().unwrap();
    assert!(support::os::gone(pid as u32));
}

#[test]
fn mismatched_start_time_is_reclaimed_but_status_and_saved_queries_do_not_reconcile() {
    let fixture = Fixture::new();
    let repo = fixture.shared_repo("repo", &["append", "STARTS", "attempt"]);
    let original = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    let id = original["requests"][0]["executionId"].as_str().unwrap();
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE executions SET completed_at=NULL,bytes=NULL,last_used=NULL",
        [],
    )
    .unwrap();
    db.execute(
        "UPDATE executions SET owner_pid=?,owner_start_time=0,status='RUNNING',data=json_set(data,'$.ownerPid',?,'$.ownerStartTime',0,'$.status','RUNNING','$.result',NULL,'$.completedAt',NULL,'$.provenance.completedAt',NULL) WHERE id=?",
        rusqlite::params![std::process::id(), std::process::id(), id],
    )
    .unwrap();
    assert_eq!(
        fixture.command(&repo, &["status"], 1)["evals"][0]["action"],
        "execute"
    );
    fixture.command(&repo, &["run", "show", original["id"].as_str().unwrap()], 0);
    assert_eq!(fixture.execution(id)["status"], "RUNNING");
    let recovered = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    assert_ne!(recovered["requests"][0]["executionId"], id);
    assert_eq!(fixture.execution(id)["errorCode"], "OWNER_DIED");
    assert_eq!(fixture.starts(), 2);
}

#[test]
fn joined_waiter_keeps_its_execution_when_a_forced_result_completes_first() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    let target = fixture.shared_repo("target", &["print", "forced"]);
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() == 1);
    let waiter = fixture.spawn(&target, &[]);
    let waiting = fixture.waiting_request();
    let forced = fixture.command(&target, &["verify", "--wait", "--all", "--force"], 0);
    assert_eq!(forced["requests"][0]["result"]["stdout"], "forced");
    fixture.release();
    let original = finish(owner, 0);
    let joined = finish(waiter, 0);
    assert_eq!(joined["requests"][0]["executionId"], waiting["executionId"]);
    assert_eq!(
        joined["requests"][0]["result"],
        original["requests"][0]["result"]
    );
    assert_eq!(joined["executionsStarted"], 0);
}

#[tokio::test]
async fn force_bypasses_a_live_claim_and_the_latest_completion_wins() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    let target = fixture.shared_repo("target", &["append", "STARTS", "forced", "print", "forced"]);
    let mut owner = fixture.spawn(&source, &[]);
    let owner_id = Fixture::announced(&mut owner);
    fixture
        .observed_request(&source, &owner_id, |request| {
            request.child.is_some() && fixture.starts() > 0
        })
        .await;
    let mut child = fixture.spawn(&target, &["--force"]);
    let forced_id = Fixture::announced(&mut child);
    let forced = fixture.completed(&target, &forced_id, child, 0).await;
    let execution = fixture.execution(forced["requests"][0]["executionId"].as_str().unwrap());
    assert_eq!(execution["key"], fixture.running_key().as_str());
    assert_eq!(fixture.records(), 1);
    assert_eq!(fixture.starts(), 2);
    fixture.release();
    let original = fixture.completed(&source, &owner_id, owner, 0).await;
    assert_eq!(fixture.records(), 2);
    // The owner is later in the seeded history, independent of host clock progression.
    order::completed_at(
        &fixture.state,
        &target,
        &forced,
        "2000-01-01T00:00:00.000000000Z",
    );
    order::completed_at(
        &fixture.state,
        &source,
        &original,
        "2000-01-02T00:00:00.000000000Z",
    );
    let reused = fixture.command(&target, &["verify", "--wait", "--all"], 0);
    assert_eq!(
        reused["requests"][0]["executionId"],
        original["requests"][0]["executionId"]
    );
    assert_eq!(fixture.starts(), 2);
}
#[test]
fn a_single_start_serves_concurrent_siblings_and_zero_budget_cache_hits() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("one-start"),"evals":[
            eval("first", &["append", "starts", "first", "wait", "release"]),
            eval("second", &["append", "starts", "first", "wait", "release"]),
            eval("third", &["append", "starts", "first", "wait", "release"])
        ]}),
    );
    let child = fixture.spawn(&repo, &["--jobs", "3", "--max-executions", "1"]);
    wait_until(|| {
        fs::read_to_string(repo.join("starts"))
            .is_ok_and(|contents| contents.replace("\r\n", "\n") == "first\n")
    });
    wait_until(|| fixture.count("requests WHERE execution_id IS NOT NULL") == 3);
    fs::write(repo.join("release"), "").unwrap();
    let run = finish(child, 0);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(fixture.count("executions"), 1);
    for request in run["requests"].as_array().unwrap() {
        assert_eq!(request["executionId"], run["requests"][0]["executionId"]);
    }
    let hit = fixture.command(
        &repo,
        &["verify", "--wait", "--all", "--max-executions", "0"],
        0,
    );
    assert_eq!(hit["executionsStarted"], 0);
    let forced = fixture.command(
        &repo,
        &[
            "verify",
            "--wait",
            "--all",
            "--max-executions",
            "0",
            "--force",
        ],
        4,
    );
    assert_eq!(forced["executionsStarted"], 0);
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(
        fs::read_to_string(repo.join("starts"))
            .unwrap()
            .replace("\r\n", "\n"),
        "first\n"
    );
}

#[test]
fn source_names_the_execution_a_result_came_from_and_how() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("sources"),"evals":[
            eval("first", &["touch", "started", "wait", "release"]),
            eval("second", &["touch", "started", "wait", "release"])
        ]}),
    );
    // Siblings share a reuse key: the second joins the first's live execution.
    let child = fixture.spawn(&repo, &["--jobs", "2"]);
    wait_until(|| repo.join("started").exists());
    fixture.waiting_request();
    fs::write(repo.join("release"), "").unwrap();
    let run = finish(child, 0);
    let (owner, joined) = (&run["requests"][0], &run["requests"][1]);
    assert!(owner["source"].is_null(), "{run}");
    let source = |kind| json!({"runId":run["id"],"requestId":owner["id"],"kind":kind});
    assert_eq!(joined["source"], source("joined"));
    assert!(joined.get("joined").is_none());
    assert_eq!(run["summary"]["reused"]["total"], 1);
    let shown = fixture.command(&repo, &["run", "show", run["id"].as_str().unwrap()], 0);
    assert_eq!(shown["requests"][0]["source"], Value::Null);
    assert_eq!(shown["requests"][1]["source"], source("joined"));
    let id = joined["id"].as_str().unwrap();
    let request = fixture.command(&repo, &["request", "show", id], 0);
    assert_eq!(request["source"], source("joined"));

    // A later Run finds the completed record.
    let hit = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    for request in hit["requests"].as_array().unwrap() {
        assert_eq!(request["source"], source("cache"));
    }
    assert_eq!(hit["summary"]["reused"]["total"], 2);
}

#[test]
fn zero_budget_can_join_an_owner_and_receive_its_success_or_error() {
    for success in [true, false] {
        let provider = ClaimProvider::new(success);
        let fixture = Fixture::with_provider(&provider.provider);
        let source = fixture.agent_repo("source", "owner");
        let target = fixture.agent_repo("target", "owner");
        let owner = fixture.spawn(&source, &[]);
        provider.owner_started();
        let waiter = fixture.spawn(&target, &["--max-executions", "0"]);
        let waiting = fixture.waiting_request();
        assert_eq!(fixture.count("executions"), 1);
        provider.gate.open();
        let original = finish(owner, if success { 0 } else { 2 });
        let joined = finish(waiter, if success { 0 } else { 2 });
        assert_eq!(original["executionsStarted"], 1);
        assert_eq!(joined["executionsStarted"], 0);
        assert_eq!(fixture.count("executions"), 1);
        let requests = provider.provider.requests();
        assert_eq!(
            requests.len(),
            1,
            "zero budget shares the owner without starting"
        );
        assert_eq!(requests[0].body["model"], "owner");
        if success {
            assert_eq!(original["requests"][0]["status"], "GREEN");
            assert_eq!(joined["requests"][0]["status"], "GREEN");
            assert_eq!(
                joined["requests"][0]["executionId"],
                original["requests"][0]["executionId"]
            );
            assert_eq!(
                joined["requests"][0]["result"],
                original["requests"][0]["result"]
            );
            assert_eq!(fixture.records(), 1);
        } else {
            assert_eq!(original["requests"][0]["status"], "ERROR");
            assert_eq!(original["requests"][0]["errorCode"], "PROVIDER_ERROR");
            assert!(original["requests"][0]["result"].is_null());
            assert_eq!(
                original["requests"][0]["executionId"],
                waiting["executionId"]
            );
            assert_eq!(joined["requests"][0]["status"], "ERROR");
            assert_eq!(joined["requests"][0]["errorCode"], "PROVIDER_ERROR");
            assert_eq!(fixture.records(), 0);
        }
    }
}

#[test]
fn zero_budget_waiter_does_not_replace_a_crashed_owner() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    let target = fixture.shared_repo("target", WAIT);
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() == 1);
    let orphans = orphan_group(&fixture);
    let waiter = fixture.spawn(&target, &["--max-executions", "0"]);
    let waiting = fixture.waiting_request();
    let worker = fixture.execution(waiting["executionId"].as_str().unwrap())["ownerPid"]
        .as_u64()
        .unwrap() as u32;
    support::os::kill(worker);
    finish(owner, 2);
    let ended = finish(waiter, 4);
    assert_eq!(ended["requests"][0]["status"], "BUDGET_EXHAUSTED");
    assert_eq!(ended["executionsStarted"], 0);
    assert_eq!(fixture.starts(), 1);
    drop(orphans);
}

#[test]
fn a_fingerprint_waiter_occupies_a_job_slot_without_consuming_execution_budget() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    // The shared Artifact and an unrelated sibling, without a parent that would own both.
    let target = fixture.root.path().join("a-target");
    let shared = fixture.shared_repo("a-target/a-shared", &["touch", "must-not-run"]);
    write(
        &target,
        "independent/index.artf",
        json!({"name":"z-independent","fingerprint":false,"evals":[eval("check", &["touch", "ran"])]}),
    );
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let waiter = fixture.spawn(&target, &["--jobs", "1", "--max-executions", "1"]);
    fixture.waiting_request();
    assert!(!target.join("independent/ran").exists());
    fixture.release();
    finish(owner, 0);
    let run = finish(waiter, 0);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(fixture.starts(), 1);
    assert!(target.join("independent/ran").exists());
    assert!(!shared.join("must-not-run").exists());
}

#[test]
fn cache_commands_are_inert_for_missing_and_empty_state() {
    let fixture = Fixture::new();
    let missing_repo = fixture.root.path().join("missing-repo");
    let missing_key = "f".repeat(64);
    for empty in [false, true] {
        if empty {
            fs::create_dir(&fixture.state).unwrap();
            fs::write(fixture.state.join("state.sqlite"), []).unwrap();
        }
        assert_eq!(
            fixture.command(&missing_repo, &["cache", "list"], 0),
            json!([])
        );
        assert_eq!(
            fixture.command(&missing_repo, &["cache", "show", &missing_key], 4),
            Value::Null
        );
        assert_eq!(
            fixture.command(&missing_repo, &["cache", "rm", &missing_key], 0),
            json!({"removed":false})
        );
        if empty {
            assert_eq!(fs::read_dir(&fixture.state).unwrap().count(), 1);
            assert_eq!(
                fs::metadata(fixture.state.join("state.sqlite"))
                    .unwrap()
                    .len(),
                0
            );
        } else {
            assert!(!fixture.state.exists());
        }
    }
    assert!(!missing_repo.exists());
}

#[test]
fn cache_list_show_and_rm_are_repository_independent_and_preserve_audit() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({
            "name":"original",
            "fingerprint":fingerprint("entry"),
            "evals":[eval("check", &["print", "original", "exit", "7"])],
        }),
    );
    let run = fixture.command(&repo, &["verify", "--wait", "--all"], 1);
    fs::remove_dir_all(&repo).unwrap();
    let before = fixture.entries();
    let entries = fixture.command(&repo, &["cache", "list"], 0);
    assert_eq!(entries.as_array().unwrap().len(), 1);
    let entry = &entries[0];
    let key = key(&run, 0);
    assert_eq!(entry["key"], key.as_str());
    assert_eq!(entry["fingerprint"], "entry");
    assert_eq!(entry["records"], 1);
    assert!(entry["producer"].as_str().unwrap().contains('@'));
    assert_eq!(entry["verdict"], "RED");
    assert_eq!(entry["repoPath"], run["repoPath"]);
    assert_eq!(entry["evalId"], "original/check");
    assert_eq!(entry["bytes"], before[0].2);
    assert_eq!(entry["lastUsed"], before[0].3);
    let saved = fixture.command(&repo, &["cache", "show", &key], 0);
    assert_eq!(
        saved,
        fixture.execution(run["requests"][0]["executionId"].as_str().unwrap())
    );
    for field in ["result", "profile", "provenance", "usage"] {
        assert_eq!(saved[field], run["requests"][0][field]);
    }
    for args in [vec!["cache", "list"], vec!["cache", "show", &key]] {
        let output = Command::new(support::os::cli())
            .arg("--state-dir")
            .arg(&fixture.state)
            .env("ARTIFACTIZE_REMOTE", "off")
            .args(&args)
            .record_output()
            .unwrap();
        assert!(output.status.success());
        if args[1] == "list" {
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains(
                "KEY\tEVAL\tVERDICT\tCOMPLETED\tPRODUCER\tSOURCE\tRECORDS\tBYTES\tLAST USED"
            ));
            assert!(text.contains(&format!("{key}\toriginal/check\tRED\t")));
        } else {
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stdout).unwrap(),
                saved
            );
        }
    }
    assert_eq!(
        fixture.entries(),
        before,
        "queries do not touch LRU metadata"
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "rm", &key], 0),
        json!({"removed":true})
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "rm", &key], 0),
        json!({"removed":false})
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "show", &key], 4),
        Value::Null
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "show", &key, "--history"], 4),
        json!([])
    );
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(
        fixture.command(&repo, &["run", "show", run["id"].as_str().unwrap()], 0),
        run
    );
}

#[test]
fn automatic_gc_enforces_the_entry_cap_in_lru_order_and_touches_hits() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("seed"),"evals":[eval("check", &[])]}),
    );
    let original = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE executions SET completed_at=NULL,bytes=NULL,last_used=NULL",
        [],
    )
    .unwrap();
    // Every seed is a record of the repository's one eval: the newest stays as what its next
    // verify reuses, and the cap bounds the other 10,000.
    fixture.seed_entries(10_001, 1);
    // The oldest seed holds the repository's key.
    let current = key(&original, 0);
    db.execute(
        "UPDATE executions SET key=? WHERE key='0000000000000000000000000000000000000000000000000000000000000000'",
        [&current],
    )
    .unwrap();
    let previous_use: String = fixture
        .observed()
        .query_row(
            "SELECT last_used FROM executions WHERE id='seed-00000'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let hit = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    assert_eq!(hit["requests"][0]["executionId"], "seed-00000");
    let used: String = fixture
        .observed()
        .query_row(
            "SELECT last_used FROM executions WHERE id='seed-00000'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(
        used, previous_use,
        "the cache hit touches its fixed seeded LRU timestamp"
    );
    write(
        &repo,
        "index.artf",
        json!({"name":"test","fingerprint":fingerprint("new"),"evals":[eval("check", &[])]}),
    );
    let new = key(
        &fixture.command(&repo, &["verify", "--wait", "--all"], 0),
        0,
    );
    // The new record is the eval's newest now; of the rest, the least recently used left.
    assert_eq!(fixture.records(), 10_001);
    assert_eq!(
        fixture.command(
            &repo,
            &[
                "cache",
                "show",
                "0000000000000000000000000000000000000000000000000000000000000001"
            ],
            4
        ),
        Value::Null
    );
    for key in [&current, &new] {
        assert!(
            fixture
                .command(&repo, &["cache", "show", key], 0)
                .is_object()
        );
    }
    assert_eq!(fixture.count("executions"), 10_003);
    assert_eq!(
        fixture.command(&repo, &["run", "show", original["id"].as_str().unwrap()], 0),
        original
    );
}
#[test]
fn automatic_gc_enforces_bytes_and_preserves_active_executions_and_waiters() {
    const MIB: i64 = 1024 * 1024;
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({
            "name":"test",
            "fingerprint":fingerprint("original"),
            "evals":[eval("check", &[])],
        }),
    );
    let original = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE executions SET completed_at=NULL,bytes=NULL,last_used=NULL",
        [],
    )
    .unwrap();
    // The newest seed stays as what the repository's next verify reuses; seeds 0 and 1 are
    // held below; the cap bounds the other 66.
    fixture.seed_entries(69, 16 * MIB);
    db.execute(
        "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,data) VALUES ('active','0000000000000000000000000000000000000000000000000000000000000000','seed','WAITING_HUMAN',1,1,'{}')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO requests(id,run_id,eval_id,ordinal,execution_id,status,data) VALUES ('waiter',?,'waiter',1,'seed-00001','QUEUED','{}')",
        [original["id"].as_str().unwrap()],
    )
    .unwrap();
    for key in [
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000001",
    ] {
        assert!(
            fixture.command(&repo, &["cache", "rm", key], 2)["error"]
                .as_str()
                .unwrap()
                .contains("in use")
        );
    }
    // Another checkout's Run ends: its own newest record stays, and 66 seeded 16 MiB records
    // are over 1 GiB, so the two least recently used leave.
    let first = fixture.publish("first", "2000-01-02T00:00:00Z");
    assert_eq!(fixture.records(), 68);
    for key in [
        "0000000000000000000000000000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000001",
        "0000000000000000000000000000000000000000000000000000000000000004",
        &first,
    ] {
        assert!(
            fixture
                .command(&repo, &["cache", "show", key], 0)
                .is_object()
        );
    }
    for key in [
        "0000000000000000000000000000000000000000000000000000000000000002",
        "0000000000000000000000000000000000000000000000000000000000000003",
    ] {
        assert_eq!(
            fixture.command(&repo, &["cache", "show", key], 4),
            Value::Null
        );
    }
    assert_eq!(fixture.count("executions"), 72);
    assert_eq!(fixture.count("requests"), 3);
    assert_eq!(
        fixture
            .observed()
            .query_row::<String, _, _>(
                "SELECT status FROM executions WHERE id='active'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "WAITING_HUMAN"
    );
    db.execute(
        "UPDATE executions SET bytes=? WHERE completed_at IS NOT NULL",
        [16 * MIB + 1],
    )
    .unwrap();
    let second = fixture.publish("second", "2000-01-03T00:00:00Z");
    let keys = || {
        fixture
            .entries()
            .into_iter()
            .map(|entry| entry.0)
            .collect::<Vec<_>>()
    };
    let sorted = |mut keys: Vec<String>| {
        keys.sort();
        keys
    };
    assert_eq!(
        keys(),
        sorted(vec![
            second.clone(),
            "0000000000000000000000000000000000000000000000000000000000000000".into(),
            "0000000000000000000000000000000000000000000000000000000000000001".into()
        ]),
        "protected rows survive even when oversized"
    );
    db.execute("UPDATE executions SET status='ERROR' WHERE id='active'", [])
        .unwrap();
    db.execute("UPDATE requests SET status='GREEN' WHERE id='waiter'", [])
        .unwrap();
    let third = fixture.publish("third", "2000-01-04T00:00:00Z");
    assert_eq!(keys(), sorted(vec![second, third]));
    assert_eq!(fixture.count("executions"), 74);
}

#[test]
fn rm_refuses_an_active_key_even_without_an_entry() {
    let fixture = Fixture::new();
    let repo = fixture.shared_repo("repo", WAIT);
    let mut owner = fixture.spawn(&repo, &[]);
    wait_until(|| fixture.starts() > 0);
    assert!(
        fixture.command(&repo, &["cache", "rm", &fixture.running_key()], 2)["error"]
            .as_str()
            .unwrap()
            .contains("active execution")
    );
    fixture.publish("other", "2000-01-02T00:00:00Z");
    assert_eq!(fixture.count("executions"), 2);
    assert!(owner.try_wait().unwrap().is_none());
    fixture.release();
    finish(owner, 0);
}
#[test]
fn waiter_receives_its_original_execution_after_entry_eviction_and_replacement() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    let target = fixture.shared_repo(
        "target",
        &["append", "STARTS", "unexpected", "print", "unexpected"],
    );
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let waiter = fixture.spawn(&target, &["--max-executions", "0"]);
    let waiting = fixture.waiting_request();
    let suspended = Suspended::new(support::runs::worker_pid(&fixture.state, &target));
    fixture.release();
    let original = finish(owner, 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE executions SET completed_at=NULL,bytes=NULL,last_used=NULL",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,completed_at,bytes,last_used,data) SELECT 'replacement',key,eval_def_hash,status,owner_pid,owner_start_time,'2999-01-01T00:00:00.000000000Z',1,'2000-01-01T00:00:00.000000000Z',json_set(data,'$.id','replacement','$.result.stdout','replacement') FROM executions WHERE id=?",
        [waiting["executionId"].as_str().unwrap()],
    )
    .unwrap();
    drop(suspended);
    let joined = finish(waiter, 0);
    assert_eq!(
        joined["requests"][0]["result"],
        original["requests"][0]["result"]
    );
    assert_eq!(joined["requests"][0]["executionId"], waiting["executionId"]);
    assert_eq!(fixture.starts(), 1);
    assert_eq!(
        fixture.command(&target, &["cache", "list"], 0)[0]["lastUsed"],
        "2000-01-01T00:00:00.000000000Z"
    );
}

#[tokio::test]
async fn oversized_completion_is_delivered_to_owner_and_waiter_but_not_retained() {
    use artifactize_core::store::{Execution, OwnedReceipts as Receipts, Request};
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT);
    let target = fixture.shared_repo(
        "target",
        &["append", "STARTS", "unexpected", "print", "unexpected"],
    );
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let orphans = orphan_group(&fixture);
    let waiter = fixture.spawn(&target, &["--max-executions", "0"]);
    let waiting = fixture.waiting_request();
    let suspended = Suspended::new(support::runs::worker_pid(&fixture.state, &target));
    let mut execution: Execution =
        serde_json::from_value(fixture.execution(waiting["executionId"].as_str().unwrap()))
            .unwrap();
    let db = fixture.observed();
    let data: String = db
        .query_row(
            "SELECT data FROM requests WHERE id=?",
            [&execution.provenance.request_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut request: Request = serde_json::from_str(&data).unwrap();
    // Keep the publisher alive while injecting its completion. The frozen waiter may
    // have an in-flight read snapshot from before publication; killing the publisher
    // would make that snapshot legitimately exhaust its zero execution budget.
    let _publisher_suspended = Suspended::new(support::runs::worker_pid(&fixture.state, &source));
    let receipts = Receipts::open(&fixture.state, &source).await.unwrap();
    execution.state = artifactize_core::store::ExecutionState::Completed {
        result: json!({"verdict":"GREEN", "large":"x".repeat(16 * 1024 * 1024)})
            .try_into()
            .unwrap(),
        at: "2026-10-04T00:00:00Z".parse().unwrap(),
    };
    execution.provenance.completed_at = execution.completed_at();
    request.state = execution.state.clone().into();
    request.provenance = Some(execution.provenance.clone());
    receipts
        .complete_execution(&execution, &request)
        .await
        .unwrap();
    assert_eq!(fixture.records(), 0);
    assert_eq!(
        artifactize_core::store::read_run(&fixture.state, &request.run_id)
            .await
            .unwrap()
            .requests[0]
            .result(),
        request.result()
    );
    drop(suspended);
    // Drain stdout while the large result is written, rather than waiting on a full pipe.
    let joined = output(waiter.wait_with_output().unwrap(), 0);
    assert_eq!(
        joined["requests"][0]["result"],
        serde_json::to_value(execution.result()).unwrap()
    );
    assert_eq!(joined["requests"][0]["executionId"], execution.id.as_str());
    assert_eq!(joined["executionsStarted"], 0);
    let worker = fixture.execution(waiting["executionId"].as_str().unwrap())["ownerPid"]
        .as_u64()
        .unwrap() as u32;
    support::os::kill(worker);
    output(owner.wait_with_output().unwrap(), 2);
    drop(orphans);
    assert_eq!(fixture.records(), 0);
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(fixture.starts(), 1);
    let later = fixture.command(&target, &["verify", "--wait", "--all"], 0);
    assert_ne!(later["requests"][0]["executionId"], execution.id.as_str());
    assert_eq!(later["requests"][0]["result"]["stdout"], "unexpected");
}

#[test]
fn gc_failure_does_not_replace_a_completed_result() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("first"),"evals":[eval("check", &[])]}),
    );
    let first_run = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    fixture.last_used(&repo, &first_run, "2000-01-01T00:00:00Z");
    let first = key(&first_run, 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute(
        "UPDATE executions SET bytes=1073741824 WHERE completed_at IS NOT NULL",
        [],
    )
    .unwrap();
    db.execute_batch(
        "CREATE TRIGGER fail_gc BEFORE UPDATE OF completed_at ON executions WHEN NEW.completed_at IS NULL BEGIN SELECT RAISE(FAIL,'GC unavailable'); END;",
    )
    .unwrap();
    write(
        &repo,
        "index.artf",
        json!({
            "name":"test",
            "fingerprint":fingerprint("second"),
            "evals":[eval("check", &[])],
        }),
    );
    let verified = fixture.spawn(&repo, &[]).wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&verified.stderr).into_owned();
    assert!(
        stderr.contains("Cache GC failed") && stderr.contains("GC unavailable"),
        "{stderr}"
    );
    let run = output(verified, 0);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    let second = key(&run, 0);
    fixture.last_used(&repo, &run, "2000-01-02T00:00:00Z");
    assert!(
        fixture
            .command(&repo, &["cache", "show", &second], 0)
            .is_object()
    );
    db.execute_batch("DROP TRIGGER fail_gc").unwrap();
    fixture.publish("third", "2000-01-04T00:00:00Z");
    assert_eq!(
        fixture.command(&repo, &["cache", "show", &first], 4),
        Value::Null,
        "the next publication retries collection"
    );
    assert!(
        fixture
            .command(&repo, &["cache", "show", &second], 0)
            .is_object()
    );
}

#[test]
fn changed_strategy_requires_a_new_execution_but_execution_options_reuse() {
    let fixture = Fixture::new();
    let base = json!({
        "id":"check",
        "title":"Review",
        "profile":{"kind":"runtime","command":true_program(),"args":[]},
        "payload":{"instruction":"Review."},
    });
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("unchanged"),"evals":[base]}),
    );
    let original = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    let change = |pointer: &str, value: Value| {
        let mut changed = base.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        changed.pointer_mut(parent).unwrap()[key] = value;
        write(
            &repo,
            "index.artf",
            json!({"name":"test","fingerprint":fingerprint("unchanged"),"evals":[changed]}),
        );
    };
    // Execution options and names are not part of the key.
    for (pointer, value) in [
        ("/profile/timeout_ms", json!(1000)),
        ("/title", json!("Renamed")),
    ] {
        change(pointer, value);
        assert_eq!(
            fixture.command(&repo, &["status"], 0)["evals"][0]["action"],
            "reuse",
            "{pointer}"
        );
        let run = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
        assert_eq!(run["executionsStarted"], 0, "{pointer}");
        assert_eq!(run["requests"][0]["key"], original["requests"][0]["key"]);
    }
    for (pointer, value, code) in [
        (
            "/pass_schema",
            json!({"type":"object","properties":{"extra":{"type":"string"}}}),
            0,
        ),
        (
            "/fail_schema",
            json!({"type":"object","properties":{"reason":{"type":"string"}}}),
            0,
        ),
        ("/profile/command", json!(false_program()), 1),
        ("/profile/args", json!(["unused"]), 0),
        ("/payload/instruction", json!("Different criteria."), 0),
        ("/payload/extra", json!({"criteria":[1,2]}), 0),
    ] {
        change(pointer, value);
        assert_eq!(
            fixture.command(&repo, &["status"], 1)["evals"][0]["action"],
            "execute",
            "{pointer}"
        );
        let run = fixture.command(&repo, &["verify", "--wait", "--all"], code);
        assert_eq!(run["executionsStarted"], 1, "{pointer}");
        assert_ne!(
            run["requests"][0]["evalDefHash"], original["requests"][0]["evalDefHash"],
            "{pointer}"
        );
        assert_eq!(
            fixture.command(&repo, &["status"], code)["evals"][0]["action"],
            "reuse"
        );
    }
    assert_eq!(fixture.count("executions"), 7);
    assert_eq!(fixture.records(), 7);
}
#[test]
fn profile_variants_share_a_result_unless_they_change_the_strategy() {
    let fixture = Fixture::new();
    let mut declaration = eval("check", &[]);
    declaration["profile"]["timeout_ms"] = json!(5000);
    declaration["profile_variants"] = json!({
        "fail":{"kind":"runtime","command":false_program(),"args":[]},
        "patient":{
            "kind":"runtime",
            "command":support::os::steps(),
            "args":[],
            "timeout_ms":60000,
        },
    });
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("variants"),"evals":[declaration]}),
    );
    // The patient variant produces the result; the default profile reuses it.
    let patient = fixture.command(
        &repo,
        &["verify", "--wait", "--all", "--profile", "patient"],
        0,
    );
    assert_eq!(
        patient["requests"][0]["options"],
        json!({"timeoutMs":60000,"variant":"patient"})
    );
    let default = fixture.command(&repo, &["verify", "--wait", "--all"], 0);
    assert_eq!(default["executionsStarted"], 0);
    let reused = &default["requests"][0];
    assert_eq!(reused["executionId"], patient["requests"][0]["executionId"]);
    // The request and the record show which profile produced the result.
    assert_eq!(
        reused["options"],
        json!({"timeoutMs":60000,"variant":"patient"})
    );
    assert_eq!(reused["profile"]["timeoutMs"], 60000);
    assert_eq!(reused["requestedProfile"]["timeoutMs"], 5000);
    let record = fixture.command(&repo, &["cache", "show", &key(&default, 0)], 0);
    assert_eq!(record["options"]["variant"], "patient");
    let text = Command::new(support::os::cli())
        .arg("--repo")
        .arg(&repo)
        .arg("--state-dir")
        .arg(&fixture.state)
        .env("ARTIFACTIZE_REMOTE", "off")
        .args(["verify", "--wait", "--all"])
        .record_output()
        .unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains(", profile patient)"), "{text}");
    assert!(
        text.contains("reused 1 (runtime 1, agent 0, human 0); 1 produced by another profile"),
        "{text}"
    );
    let status = fixture.command(&repo, &["status"], 0);
    assert!(
        status["evals"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("produced by profile patient"),
        "{status}"
    );

    assert_eq!(
        fixture.command(&repo, &["status", "--profile", "fail"], 1)["evals"][0]["action"],
        "execute"
    );
    let different = fixture.command(
        &repo,
        &["verify", "--wait", "--all", "--profile", "fail"],
        1,
    );
    assert_eq!(different["executionsStarted"], 1);
    assert_ne!(
        different["requests"][0]["evalDefHash"],
        default["requests"][0]["evalDefHash"]
    );
    declaration["profile_variants"]["fail"]["args"] = json!(["changed unused variant"]);
    write(
        &repo,
        "index.artf",
        json!({"name":"test","fingerprint":fingerprint("variants"),"evals":[declaration]}),
    );
    assert_eq!(
        fixture.command(&repo, &["verify", "--wait", "--all"], 0)["requests"][0]["executionId"],
        patient["requests"][0]["executionId"]
    );
    assert_eq!(fixture.count("executions"), 2);
}
#[test]
fn canonical_definition_hash_covers_the_strategy_but_no_agent_setting_or_name() {
    use artifactize_core::{cache::eval_definition_hash, config::parse_declaration};
    let parse = |value| {
        parse_declaration(
            &support::declaration::to_toml(json!({"name":"app","evals":[value]})).unwrap(),
        )
        .unwrap()
        .evals
        .remove(0)
    };
    let first_value = json!({
        "id":"one",
        "title":"First",
        "profile":{
            "kind":"agent",
            "backend":"anthropic",
            "model":"model",
            "reasoning":"high",
            "max_tool_calls":3,
            "max_tokens":10,
            "timeout_ms":1000,
        },
        "payload":{"instruction":"Review.","nested":{"b":2,"a":1}},
        "pass_schema":{"type":"object","properties":{"b":{"type":"number"},"a":{"type":"string"}}},
    });
    let first = parse(first_value.clone());
    let second = parse(json!({
        "title":"Second",
        "id":"two",
        "payload":{"nested":{"a":1,"b":2},"instruction":"Review."},
        "pass_schema":{
            "properties":{"a":{"type":"string"},"b":{"type":"number"}},
            "type":"object",
        },
        "profile":{
            "max_tokens":10,
            "max_tool_calls":3,
            "timeout_ms":1000,
            "reasoning":"high",
            "model":"model",
            "backend":"anthropic",
            "kind":"agent",
        },
    }));
    let hash = eval_definition_hash(&first);
    assert_eq!(hash, eval_definition_hash(&second));
    for (key, value) in [
        ("backend", json!("openai")),
        ("model", json!("other")),
        ("reasoning", json!("low")),
        ("max_tool_calls", json!(4)),
        ("max_tokens", json!(11)),
        ("timeout_ms", json!(2000)),
    ] {
        let mut changed = first_value.clone();
        changed["profile"][key] = value;
        assert_eq!(hash, eval_definition_hash(&parse(changed)), "{key}");
    }
    let mut human_value = first_value.clone();
    human_value["profile"] = json!({"kind":"human"});
    let human = parse(human_value);
    assert_ne!(hash, eval_definition_hash(&human));
    let mut nested = first_value;
    nested["payload"]["nested"]["a"] = json!(3);
    assert_ne!(hash, eval_definition_hash(&parse(nested)));
}
#[test]
fn concurrent_claims_dedupe_each_definition_without_blocking_another() {
    let fixture = Fixture::new();
    // The verdict each repository adds to its arguments is compared with `pass`.
    let script = [WAIT, &["equals", "pass"]].concat();
    let mut repos = Vec::new();
    for (name, verdict) in [
        ("pass-one", "pass"),
        ("pass-two", "pass"),
        ("fail-one", "fail"),
        ("fail-two", "fail"),
    ] {
        let repo = fixture.shared_repo(name, &script);
        let path = repo.join("index.artf");
        let mut declaration: Value = support::declaration::read(fs::read(&path).unwrap()).unwrap();
        declaration["evals"][0]["profile"]["args"]
            .as_array_mut()
            .unwrap()
            .push(json!(verdict));
        support::declaration::write(path, declaration.to_string()).unwrap();
        repos.push(repo);
    }
    let children: Vec<_> = repos.iter().map(|repo| fixture.spawn(repo, &[])).collect();
    wait_until(|| fixture.starts() == 2);
    wait_until(|| {
        let db = Connection::open_with_flags(
            fixture.state.join("state.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row::<u32, _, _>(
            "SELECT count(*) FROM requests q JOIN executions e ON e.id=q.execution_id WHERE q.status='QUEUED' AND json_extract(e.data,'$.provenance.requestId')<>q.id",
            [],
            |row| row.get(0),
        )
        .unwrap()
            == 2
    });
    assert_eq!(fixture.count("executions"), 2);
    fixture.release();
    let runs: Vec<_> = children
        .into_iter()
        .enumerate()
        .map(|(index, child)| finish(child, if index < 2 { 0 } else { 1 }))
        .collect();
    assert_eq!(
        runs[0]["requests"][0]["executionId"],
        runs[1]["requests"][0]["executionId"]
    );
    assert_eq!(
        runs[2]["requests"][0]["executionId"],
        runs[3]["requests"][0]["executionId"]
    );
    assert_ne!(
        runs[0]["requests"][0]["executionId"],
        runs[2]["requests"][0]["executionId"]
    );
    assert_eq!(fixture.starts(), 2);
    assert_eq!(fixture.records(), 2);
}

#[test]
fn gc_and_removal_protect_only_the_matching_key() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({
            "name":"test",
            "fingerprint":fingerprint("shared"),
            "evals":[eval("pass", &[]),eval("fail", &["exit", "1"])],
        }),
    );
    let run = fixture.command(&repo, &["verify", "--wait", "--all"], 1);
    let (active, other) = (key(&run, 1), key(&run, 0));
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute(
        "INSERT INTO executions(id,key,eval_def_hash,status,owner_pid,owner_start_time,data) VALUES ('active',?,'active','WAITING_HUMAN',1,1,'{}')",
        [&active],
    )
    .unwrap();
    assert_eq!(
        fixture.command(&repo, &["cache", "rm", &other], 0),
        json!({"removed":true})
    );
    assert!(
        fixture.command(&repo, &["cache", "rm", &active], 2)["error"]
            .as_str()
            .unwrap()
            .contains("in use")
    );
    fixture.command(&repo, &["verify", "--wait", "--eval", "test/fail"], 1);
    db.execute(
        "UPDATE executions SET bytes=16777217 WHERE completed_at IS NOT NULL",
        [],
    )
    .unwrap();
    fixture.publish("other", "2000-01-02T00:00:00Z");
    assert_eq!(fixture.records(), 2);
    assert_eq!(
        fixture.command(&repo, &["cache", "show", &other], 4),
        Value::Null
    );
    assert!(
        fixture
            .command(&repo, &["cache", "show", &active], 0)
            .is_object()
    );
}
