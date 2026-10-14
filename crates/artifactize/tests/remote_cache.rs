//! Read-through/write-through of the remote review store: two state directories ("machines")
//! and one real `artifactize server` on loopback.

#[path = "support/run.rs"]
mod owned_run;
use owned_run::{RunChild, Watcher};

use std::{
    fs,
    io::{BufRead, BufReader},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{ChildStdout, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use support::os::{cat_program, shell, true_program};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Server {
    state: PathBuf,
    child: Watcher,
    _stdout: BufReader<ChildStdout>,
    url: String,
}

impl Server {
    fn start(root: &Path) -> Self {
        let state = root.join("server");
        let child = Command::new(support::os::cli())
            .arg("--state-dir")
            .arg(&state)
            .args(["server", "run", "--listen", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut child = Watcher::new(child);
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let url = line
            .trim()
            .strip_prefix("Listening on ")
            .unwrap()
            .to_owned();
        Self {
            state,
            child,
            _stdout: stdout,
            url,
        }
    }

    /// A freshly generated token; only the server's SHA-256 of it is stored.
    fn token(&self, name: &str, scopes: &str) -> String {
        let output = Command::new(support::os::cli())
            .arg("--state-dir")
            .arg(&self.state)
            .args(["server", "token", "add", name, "--scopes", scopes, "--json"])
            .record_output()
            .unwrap();
        assert!(output.status.success());
        let added: Value = serde_json::from_slice(&output.stdout).unwrap();
        added["token"].as_str().unwrap().to_owned()
    }

    /// Stored (publisher, record) pairs.
    fn entries(&self) -> Vec<(String, Value)> {
        let db = Connection::open_with_flags(
            self.state.join("review-store.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut statement = db
            .prepare("SELECT publisher,data FROM entries ORDER BY publisher,key,completed_at")
            .unwrap();
        statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    serde_json::from_str(&row.get::<_, String>(1)?).unwrap(),
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// The last use of each stored record, by execution ID; a lookup updates the record it
    /// returns.
    fn last_used(&self) -> Vec<(String, String)> {
        let db = Connection::open_with_flags(
            self.state.join("review-store.sqlite"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let mut statement = db
            .prepare("SELECT execution_id,last_used FROM entries ORDER BY execution_id")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// Seed completion ordering, not elapsed wall-clock time, for a stored fixture.
    fn completed_at(&self, execution: &str, at: &str) {
        let db = Connection::open(self.state.join("review-store.sqlite")).unwrap();
        let data: String = db
            .query_row(
                "SELECT data FROM entries WHERE execution_id=?",
                [execution],
                |row| row.get(0),
            )
            .unwrap();
        let data = completion_fixture(&data, at);
        assert_eq!(
            db.execute(
                "UPDATE entries SET completed_at=?,bytes=?,data=? WHERE execution_id=?",
                rusqlite::params![at, data.len() as i64, data, execution]
            )
            .unwrap(),
            1
        );
    }

    fn stop(&mut self) {
        support::os::kill(self.child.id());
        let _ = self.child.wait();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// One machine: its own state directory, user name and remote token.
struct Machine {
    state: PathBuf,
    user: &'static str,
    url: String,
    token: String,
}

impl Machine {
    fn new(root: &Path, user: &'static str, url: &str, token: &str) -> Self {
        Self {
            state: root.join(format!("state-{user}")),
            user,
            url: url.into(),
            token: token.into(),
        }
    }

    fn command(&self, repo: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(support::os::cli());
        command
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .env("ARTIFACTIZE_REMOTE", &self.url)
            .env("ARTIFACTIZE_REMOTE_TOKEN", &self.token)
            .env_remove("ARTIFACTIZE_REMOTE_SHARE");
        support::os::sign_in_as(&mut command, self.user);
        command
    }

    /// Text stdout and stderr; neither ever contains the token.
    fn run(&self, repo: &Path, args: &[&str], code: i32) -> (String, String) {
        checked(
            self.command(repo, args).record_output().unwrap(),
            code,
            &self.token,
        )
    }

    fn json(&self, repo: &Path, args: &[&str], code: i32) -> Value {
        let (stdout, _) = self.run(repo, &[args, &["--json"]].concat(), code);
        serde_json::from_str(&stdout).unwrap()
    }

    /// Keep cache metadata and its saved execution wire timestamp in agreement.
    fn completed_at(&self, execution: &str, at: &str) {
        let db = Connection::open(self.state.join("state.sqlite")).unwrap();
        let data: String = db
            .query_row(
                "SELECT data FROM executions WHERE id=?",
                [execution],
                |row| row.get(0),
            )
            .unwrap();
        let data = completion_fixture(&data, at);
        assert_eq!(
            db.execute(
                "UPDATE executions SET completed_at=?,bytes=?,data=? WHERE id=?",
                rusqlite::params![at, data.len() as i64, data, execution]
            )
            .unwrap(),
            1
        );
    }

    /// Claim and submit the Human request that `run` recorded; returns stderr.
    fn sign(&self, repo: &Path, run: &Value) -> String {
        let id = run["requests"][0]["id"].as_str().unwrap();
        self.json(repo, &["request", "claim", id, "--reviewer", "alice"], 0);
        let submit = [
            "request",
            "submit",
            id,
            "--verdict",
            "GREEN",
            "--reviewer",
            "alice",
        ];
        self.run(repo, &[&submit[..], &["--json"]].concat(), 0).1
    }
}

/// Deterministic runtime fixture times; no verdict, approval or execution content changes.
fn completion_fixture(data: &str, at: &str) -> String {
    fn stamp(record: &mut Value, at: &str) {
        record["startedAt"] = json!("2000-01-01T00:00:00.000000000Z");
        record["completedAt"] = json!(at);
        if let Some(provenance) = record.get_mut("provenance") {
            provenance["completedAt"] = json!(at);
        }
        if let Some(execution) = record
            .get_mut("execution")
            .filter(|value| value.is_object())
        {
            stamp(execution, at);
        }
    }
    let mut record: Value = serde_json::from_str(data).unwrap();
    stamp(&mut record, at);
    serde_json::to_string(&record).unwrap()
}

/// Text stdout and stderr, without verify's `Started RUN_ID` announcement.
fn checked(output: Output, code: i32, token: &str) -> (String, String) {
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stdout.contains(token) && !stderr.contains(token));
    assert_eq!(output.status.code(), Some(code), "{stdout}{stderr}");
    assert!(stderr.matches("Started run-").count() <= 1, "{stderr}");
    let stderr = stderr
        .lines()
        .filter(|line| !line.starts_with("Started run-"))
        .map(|line| format!("{line}\n"))
        .collect();
    (stdout, stderr)
}

/// Artifacts `app` and `docs`, each with a runtime eval and a script fingerprint from `version`.
fn runtime_repo(root: &Path, name: &str, docs: &str) -> PathBuf {
    let repo = root.join(name);
    write(&repo, json!({"name":"root","basis":true}));
    for (artifact, version) in [("app", "v1"), ("docs", docs)] {
        let folder = repo.join(artifact);
        write(
            &folder,
            json!({
                "name":artifact,
                "fingerprint":{"script":{"command":cat_program(),"args":["version"]}},
                "evals":[
                    {
                        "id":"check",
                        "title":"Check",
                        "profile":{"kind":"runtime","command":true_program(),"args":[]},
                        "payload":{"instruction":"Check."},
                    },
                ],
            }),
        );
        fs::write(folder.join("version"), format!("{artifact}-{version}\n")).unwrap();
    }
    repo
}

fn human_repo(root: &Path, name: &str) -> PathBuf {
    let repo = root.join(name);
    write(
        &repo,
        json!({
            "name":"brand",
            "fingerprint":{"script":{"command":cat_program(),"args":["version"]}},
            "evals":[
                {
                    "id":"signoff",
                    "title":"Sign off",
                    "profile":{"kind":"human"},
                    "payload":{"instruction":"Sign off."},
                },
            ],
        }),
    );
    fs::write(repo.join("version"), "brand-v1\n").unwrap();
    repo
}

fn write(folder: &Path, declaration: Value) {
    fs::create_dir_all(folder).unwrap();
    support::declaration::write(folder.join("index.artf"), declaration.to_string()).unwrap();
}

fn line<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.lines()
        .find(|line| line.trim_start().starts_with(prefix))
        .unwrap_or_else(|| panic!("no {prefix} line in:\n{text}"))
}

const OFFLINE: &str = "continuing without the remote review store";

#[test]
fn another_machine_reuses_a_published_verdict_and_status_predicts_it() {
    let root = support::os::tempdir();
    let server = Server::start(root.path());
    let alice = Machine::new(
        root.path(),
        "alice",
        &server.url,
        &server.token("alice-laptop", "read,publish"),
    );
    let bob = Machine::new(
        root.path(),
        "bob",
        &server.url,
        &server.token("bob-ci", "read"),
    );
    let repo_a = runtime_repo(root.path(), "a", "v1");
    let repo_b = runtime_repo(root.path(), "b", "v1");

    let (text, stderr) = alice.run(&repo_a, &["verify", "--wait", "--all"], 0);
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 2 (runtime 2, agent 0, human 0), reused 0 (runtime 0, agent 0, human 0)"
    );
    assert!(stderr.is_empty(), "{stderr}");
    let entries = server.entries();
    assert_eq!(entries.len(), 2);
    for (publisher, record) in &entries {
        assert_eq!(publisher, "alice-laptop");
        assert!(
            record["producer"]["name"]
                .as_str()
                .unwrap()
                .starts_with("alice@")
        );
        // A summary carries no captured output or local paths.
        assert!(record.get("execution").is_none());
        assert!(
            !record
                .to_string()
                .contains(&support::os::path_text(root.path()))
        );
    }

    // Status reads the remote without mirroring it.
    let status = bob.json(&repo_b, &["status"], 0);
    assert_eq!(status["counts"]["reuse"], 2);
    assert!(
        status["evals"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("result in the remote review store, from alice@"),
        "{status}"
    );
    assert!(bob.json(&repo_b, &["cache", "list"], 0) == json!([]));

    let (text, _) = bob.run(&repo_b, &["verify", "--wait", "--all"], 0);
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 0 (runtime 0, agent 0, human 0), reused 2 (runtime 2, agent 0, human 0)"
    );
    let source = entries[0].1["runId"].as_str().unwrap();
    let marker = line(&text, "app/check");
    assert!(
        marker.contains("]: GREEN (reused from remote: alice@"),
        "{marker}"
    );
    assert!(marker.ends_with(&format!(", {source})")), "{marker}");
    let id = line(&text, "Run:").strip_prefix("Run: ").unwrap();
    let run = bob.json(&repo_b, &["run", "show", id], 0);
    assert_eq!(run["executionsStarted"], 0);
    assert_eq!(run["requests"][0]["origin"]["publisher"], "alice-laptop");
    assert_eq!(
        run["requests"][0]["source"],
        json!({
            "runId":source,
            "requestId":run["requests"][0]["provenance"]["requestId"],
            "kind":"remote",
        })
    );
    let key = run["requests"][0]["key"].as_str().unwrap();
    let shown = bob.json(&repo_b, &["cache", "show", key], 0);
    assert_eq!(shown["origin"]["store"], server.url.as_str());
    assert!(
        shown["producer"]["name"]
            .as_str()
            .unwrap()
            .starts_with("alice@")
    );

    // Bob changes docs: it executes locally, and his read-only token publishes nothing.
    fs::write(repo_b.join("docs/version"), "docs-v2\n").unwrap();
    let (text, stderr) = bob.run(&repo_b, &["verify", "--wait", "--all"], 0);
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 1 (runtime 1, agent 0, human 0), reused 1 (runtime 1, agent 0, human 0)"
    );
    assert!(stderr.is_empty(), "{stderr}");
    assert_eq!(server.entries().len(), 2);
}

#[test]
fn an_unreachable_store_falls_back_to_the_local_latest_with_one_warning() {
    let root = support::os::tempdir();
    let mut server = Server::start(root.path());
    let bob = Machine::new(
        root.path(),
        "bob",
        &server.url,
        &server.token("bob", "read,publish"),
    );
    server.stop();
    let repo = runtime_repo(root.path(), "repo", "v1");

    let (text, stderr) = bob.run(&repo, &["verify", "--wait", "--all"], 0);
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 2 (runtime 2, agent 0, human 0), reused 0 (runtime 0, agent 0, human 0)"
    );
    assert_eq!(stderr.matches(OFFLINE).count(), 1, "{stderr}");
    assert!(stderr.contains("unreachable"), "{stderr}");
    // The store is asked again next time; while it is down, the local latest is reused.
    let (text, stderr) = bob.run(&repo, &["verify", "--wait", "--all"], 0);
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 0 (runtime 0, agent 0, human 0), reused 2 (runtime 2, agent 0, human 0)"
    );
    assert_eq!(stderr.matches(OFFLINE).count(), 1, "{stderr}");
    let status = bob.json(&repo, &["status"], 0);
    assert_eq!(status["counts"]["reuse"], 2);
}
#[test]
fn status_force_makes_no_remote_call_and_a_rejected_token_fails_closed() {
    let root = support::os::tempdir();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let machine = Machine::new(root.path(), "bob", &url, "azt_unused");
    let repo = runtime_repo(root.path(), "repo", "v1");

    let (_, stderr) = machine.run(&repo, &["status", "--force"], 1);
    assert!(stderr.is_empty(), "{stderr}");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );

    let server = Server::start(root.path());
    server.token("bob", "read,publish");
    let rejected = Machine::new(root.path(), "mallory", &server.url, "azt_unknown");
    let (_, stderr) = rejected.run(&repo, &["verify", "--wait", "--all"], 2);
    assert!(stderr.contains("rejected the token (HTTP 401)"), "{stderr}");
    assert_eq!(
        rejected.json(&repo, &["run", "list"], 0)[0]["status"],
        "ERROR"
    );
}
#[test]
fn human_signoffs_publish_only_with_the_human_scope_and_settle_a_waiting_verify() {
    let root = support::os::tempdir();
    let server = Server::start(root.path());
    let laptop = Machine::new(
        root.path(),
        "alice",
        &server.url,
        &server.token("alice-laptop", "read,publish"),
    );
    let signer = Machine::new(
        root.path(),
        "signer",
        &server.url,
        &server.token("alice-signoff", "read,publish,human"),
    );
    let bob = Machine::new(
        root.path(),
        "bob",
        &server.url,
        &server.token("bob-ci", "read"),
    );
    let repo = human_repo(root.path(), "repo");

    // Bob waits for a sign-off that does not exist anywhere yet.
    let waiting = bob
        .command(&repo, &["verify", "--wait", "--all"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let waiting = RunChild::new(waiting, &bob.state, &repo);

    // A token without the human scope keeps the sign-off local. Each signer's own verify
    // records the request and stops waiting at once.
    let record = ["verify", "--all"];
    let run = laptop.json(&repo, &record, 0);
    let stderr = laptop.sign(&repo, &run);
    assert!(
        stderr.contains("stays local: remote token alice-laptop lacks the human scope"),
        "{stderr}"
    );
    assert_eq!(server.entries(), []);

    let run = signer.json(&repo, &record, 0);
    assert!(signer.sign(&repo, &run).is_empty());
    let entries = server.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "alice-signoff");
    assert_eq!(entries[0].1["reviewer"], "alice");

    // Bob's wait poll finds the remote sign-off and settles his waiting request from it.
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(20));
    let mut waiting = waiting;
    while waiting.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "the waiting verify did not settle"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let (text, _) = checked(waiting.wait_with_output().unwrap(), 0, &bob.token);
    let marker = line(&text, "brand/signoff");
    assert!(
        marker.contains(
            "]: GREEN (reused from remote: Human sign-off by alice, published by alice-signoff, "
        ),
        "{marker}"
    );
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 0 (runtime 0, agent 0, human 0), reused 1 (runtime 0, agent 0, human 1)"
    );

    // A fresh machine, such as CI, reuses the sign-off from the store under --reuse-only.
    let carol = Machine::new(root.path(), "carol", &server.url, &bob.token);
    let (text, _) = carol.run(
        &repo,
        &["verify", "--wait", "--all", "--reuse-only", "human"],
        0,
    );
    assert!(line(&text, "brand/signoff").contains("Human sign-off by alice"));
}

#[test]
fn remote_push_publishes_results_produced_offline_once() {
    let root = support::os::tempdir();
    let server = Server::start(root.path());
    let token = server.token("alice-laptop", "read,publish");
    let offline = Machine::new(root.path(), "alice", "off", &token);
    let alice = Machine::new(root.path(), "alice", &server.url, &token);
    let runtime = runtime_repo(root.path(), "runtime", "v1");
    let human = human_repo(root.path(), "human");
    offline.run(&runtime, &["verify", "--wait", "--all"], 0);
    let run = offline.json(&human, &["verify", "--all"], 0);
    offline.sign(&human, &run);
    assert_eq!(server.entries(), []);

    let push = |machine: &Machine, args: &[&str]| {
        machine.json(&runtime, &[&["remote", "push"][..], args].concat(), 0)
    };
    assert_eq!(
        push(&alice, &["--dry-run"]),
        json!({"dryRun":true,"pushed":2,"existing":0,"skipped":1})
    );
    assert_eq!(server.entries(), []);
    let (stdout, stderr) = alice.run(&runtime, &["remote", "push"], 0);
    assert_eq!(stdout, "Pushed 2, already in the store 0, skipped 1.\n");
    assert!(
        stderr.contains("(brand/signoff): remote token alice-laptop lacks the human scope"),
        "{stderr}"
    );
    assert_eq!(server.entries().len(), 2);
    assert_eq!(
        push(&alice, &[]),
        json!({"dryRun":false,"pushed":0,"existing":2,"skipped":1})
    );

    // Another machine reuses the pushed results; its mirrors are never pushed again.
    let bob = Machine::new(
        root.path(),
        "bob",
        &server.url,
        &server.token("bob-laptop", "read,publish"),
    );
    assert_eq!(
        bob.json(&runtime, &["verify", "--wait", "--all"], 0)["executionsStarted"],
        0
    );
    assert_eq!(
        push(&bob, &[]),
        json!({"dryRun":false,"pushed":0,"existing":0,"skipped":0})
    );

    let ci = Machine::new(root.path(), "ci", &server.url, &server.token("ci", "read"));
    let (_, stderr) = ci.run(&runtime, &["remote", "push"], 2);
    assert!(
        stderr.contains("Remote token ci lacks the publish scope."),
        "{stderr}"
    );
}

#[test]
fn the_newer_record_wins_across_local_history_and_the_store() {
    // Explicitly ordered fixture timestamps make clock resolution and scheduling irrelevant.
    const FIRST: &str = "2000-01-01T00:00:01.000000000Z";
    const SECOND: &str = "2000-01-01T00:00:02.000000000Z";
    const THIRD: &str = "2000-01-01T00:00:03.000000000Z";
    let root = support::os::tempdir();
    let server = Server::start(root.path());
    let alice_token = server.token("alice-laptop", "read,publish");
    let alice = Machine::new(root.path(), "alice", &server.url, &alice_token);
    let offline = Machine::new(root.path(), "alice", "off", &alice_token);
    let bob = Machine::new(
        root.path(),
        "bob",
        &server.url,
        &server.token("bob-laptop", "read,publish"),
    );
    let carol = Machine::new(
        root.path(),
        "carol",
        &server.url,
        &server.token("carol-ci", "read"),
    );
    // The eval fails when a `broken` file exists, which the fingerprint does not cover.
    let repo = |name: &str| {
        let repo = root.path().join(name);
        write(
            &repo,
            json!({
                "name":"app",
                "fingerprint":{"script":{"command":cat_program(),"args":["version"]}},
                "evals":[
                    {
                        "id":"check",
                        "title":"Check",
                        "profile":{
                            "kind":"runtime",
                            "command":shell(),
                            "args":["-c","test ! -e broken"],
                        },
                        "payload":{"instruction":"Check."},
                    },
                ],
            }),
        );
        fs::write(repo.join("version"), "app-v1\n").unwrap();
        repo
    };
    let (repo_a, repo_b, repo_c) = (repo("a"), repo("b"), repo("c"));
    let first = alice.json(&repo_a, &["verify", "--wait", "--all"], 0);
    let alice_record = first["requests"][0]["executionId"].as_str().unwrap();
    alice.completed_at(alice_record, FIRST);
    server.completed_at(alice_record, FIRST);
    let key = first["requests"][0]["key"].as_str().unwrap().to_owned();
    assert_eq!(server.entries().len(), 1);
    let used = server.last_used();

    // Bob reviews again by force: it reads nothing from the store, so no record's last use
    // changes, and its RED is published as a newer record without `remote push`.
    fs::write(repo_b.join("broken"), "").unwrap();
    let forced = bob.json(&repo_b, &["verify", "--wait", "--all", "--force"], 1);
    let bob_record = forced["requests"][0]["executionId"].as_str().unwrap();
    bob.completed_at(bob_record, SECOND);
    server.completed_at(bob_record, SECOND);
    assert_eq!(forced["requests"][0]["key"], key.as_str());
    let entries = server.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].0, "bob-laptop");
    assert_eq!(entries[1].1["verdict"], "RED");
    let last_use = |used: &[(String, String)]| {
        used.iter()
            .find(|(id, _)| id == alice_record)
            .unwrap()
            .1
            .clone()
    };
    assert_eq!(last_use(&server.last_used()), last_use(&used));
    assert_eq!(
        bob.json(&repo_b, &["remote", "push"], 0),
        json!({"dryRun":false,"pushed":0,"existing":1,"skipped":0})
    );

    // A machine without a local record reuses the store's latest: bob's RED.
    let reused = carol.json(&repo_c, &["verify", "--wait", "--all"], 1);
    assert_eq!(reused["executionsStarted"], 0);
    assert_eq!(reused["requests"][0]["provenance"]["runId"], forced["id"]);

    // A newer store record beats an older local one. status predicts it read-only.
    let status = alice.json(&repo_a, &["status"], 1);
    assert_eq!(status["evals"][0]["action"], "reuse");
    assert_eq!(status["evals"][0]["state"], "RED");
    assert!(
        status["evals"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("result in the remote review store, from bob@"),
        "{status}"
    );
    let history = || alice.json(&repo_a, &["cache", "show", &key, "--history"], 0);
    assert_eq!(history().as_array().unwrap().len(), 1);
    let newer = alice.json(&repo_a, &["verify", "--wait", "--all"], 1);
    assert_eq!(newer["requests"][0]["provenance"]["runId"], forced["id"]);
    assert_eq!(newer["requests"][0]["origin"]["publisher"], "bob-laptop");
    assert_eq!(history().as_array().unwrap().len(), 2);

    // A newer local record beats an older store one: alice reviews again offline.
    let local = offline.json(&repo_a, &["verify", "--wait", "--all", "--force"], 0);
    offline.completed_at(local["requests"][0]["executionId"].as_str().unwrap(), THIRD);
    assert_eq!(server.entries().len(), 2);
    let status = alice.json(&repo_a, &["status"], 0);
    assert_eq!(status["evals"][0]["state"], "PASS");
    let again = alice.json(&repo_a, &["verify", "--wait", "--all"], 0);
    assert_eq!(
        again["requests"][0]["executionId"],
        local["requests"][0]["executionId"]
    );
    assert!(again["requests"][0]["origin"].is_null());
    assert_eq!(history().as_array().unwrap().len(), 3);
}

/// A store that accepts the token, then answers a lookup with a record whose only field is the
/// token: decoding it fails, and the failure names the field. Neither verify's output nor its
/// warnings may hold the token or a piece of it.
#[test]
fn an_undecodable_record_naming_the_token_never_gets_it_printed() {
    let root = support::os::tempdir();
    let token = format!("azt_lookup{}", "0123456789abcdef".repeat(2));
    let echoed = token.clone();
    let store = support::FakeProvider::start(move |request| match request.path.as_str() {
        "/v1/whoami" => {
            support::Reply::Json(200, json!({"principal":"bob-laptop","scopes":["read"]}))
        }
        "/v1/lookup" => support::Reply::Json(
            200,
            json!({"entries":[{ echoed.clone(): null }, { format!("{}\n{}", &echoed[..14], &echoed[14..]): 1 }]}),
        ),
        _ => support::Reply::Json(404, json!({"error":"not found"})),
    });
    let bob = Machine::new(root.path(), "bob", &format!("{}/", store.url), &token);
    let repo = runtime_repo(root.path(), "repo", "v1");
    let (stdout, stderr) = bob.run(&repo, &["verify", "--wait", "--all", "--json"], 0);
    for piece in [&token[10..22], &token[token.len() - 12..]] {
        assert!(
            !stdout.contains(piece) && !stderr.contains(piece),
            "{stdout}{stderr}"
        );
    }
    assert!(
        stderr.contains("Ignoring an invalid remote review record: [withheld"),
        "{stderr}"
    );
    assert!(
        store
            .requests()
            .iter()
            .any(|request| request.path == "/v1/lookup")
    );
}

#[test]
fn tool_session_lifecycle_pins_round_trip_through_the_remote_store() {
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    let log = root.path().join("log");
    fs::create_dir(&log).unwrap();
    let mut views = support::tool_sessions::views(&log, "start", "stop", "act");
    for command in ["start", "stop"] {
        views["agent_sessions"]["app"][command]["execution_paths"] = json!(["harness.py"]);
    }
    write(
        &repo,
        json!({"name":"app", "fingerprint":{}, "views":views,
        "evals":[{"id":"check","title":"Review","profile":{"kind":"agent","backend":"openai","model":"fake-model"},"payload":{"instruction":"Review."}}]}),
    );
    fs::write(repo.join("harness.py"), support::tool_sessions::script()).unwrap();
    let server = Server::start(root.path());
    let token = server.token("alice", "read,publish");
    let consumer = Machine::new(root.path(), "bob", &server.url, &token);
    let state = root.path().join("fixture-state");
    let config = artifactize_core::config::read_workspace_config(&repo).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // A synthetic remote protocol fixture, not a fake-provider verdict: provider
    // test endpoint isolation remains intact, and no Agent is asked to publish it.
    let pins = runtime.block_on(async {
        let fingerprints = artifactize_core::cache::prepare(
            &config,
            [&config.artifacts["app"].name],
            &state,
            &artifactize_core::cache::Parallelism::new(2),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        let keys = artifactize_core::cache::eval_keys(&config, &fingerprints);
        let eval = &config.evals[0];
        let key = &keys[&eval.id];
        let pins = artifactize_core::tools::pins::execution_paths(&config, &eval.id)
            .await
            .unwrap();
        let record = artifactize_core::remote::Record {
            schema: artifactize_core::remote::SCHEMA,
            key: key.value.clone(),
            eval_def_hash: key.eval_def_hash.clone(),
            fingerprints: key.fingerprints.clone(),
            artifact_kinds: key.artifact_kinds.clone(),
            verdict: artifactize_core::types::ExecutionStatus::Green,
            eval_id: eval.id.clone(),
            run_id: "run-lifecycle-fixture".parse().unwrap(),
            request_id: "run-lifecycle-fixture-1".parse().unwrap(),
            execution_id: "execution-run-lifecycle-fixture-1".parse().unwrap(),
            profile: artifactize_core::config::StoredProfile::from(eval.declaration.profile()),
            options: artifactize_core::store::ExecutionOptions::new(
                eval.declaration.profile(),
                None,
            ),
            execution_paths: pins.clone(),
            result: json!({"verdict":"GREEN"}).try_into().unwrap(),
            usage: None,
            reviewer: None,
            producer: None,
            started_at: "2026-10-04T00:00:00Z".parse().unwrap(),
            completed_at: "2026-10-04T00:00:01Z".parse().unwrap(),
            execution: None,
            publisher: None,
            published_at: None,
        };
        record.validate().unwrap();
        let (remote, _) = artifactize_core::auth::remote::login(
            Some(&state),
            Some(&repo),
            artifactize_core::auth::remote::parse_url(&server.url).unwrap(),
            artifactize_core::auth::remote::Share::Summary,
            token.clone(),
        )
        .await
        .unwrap();
        assert!(remote.publish(&record).await.unwrap());
        let fetched = remote
            .lookup(std::slice::from_ref(&record.key))
            .await
            .unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].as_ref().unwrap().execution_paths, pins);
        serde_json::to_value(pins).unwrap()
    });
    for key in ["app_app.start", "app_app.stop"] {
        assert_eq!(pins[key]["harness.py"].as_str().unwrap().len(), 64);
    }
    let entries = server.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1["executionPaths"], pins);
    let reused = consumer.json(&repo, &["verify", "--wait", "--all"], 0);
    assert_eq!(reused["executionsStarted"], 0);
    assert_eq!(reused["requests"][0]["provenance"]["executionPaths"], pins);
    assert!(support::tool_sessions::directories(&consumer.state).is_empty());
    assert!(support::tool_sessions::records(&log, "start").is_empty());
}
