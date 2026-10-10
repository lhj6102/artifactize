//! Read-through/write-through of the remote review store: two state directories ("machines")
//! and one real `artifactize server` on loopback.

use std::{
    fs,
    io::{BufRead, BufReader},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use support::os::{cat_program, shell, true_program};

mod support;

struct Server {
    state: PathBuf,
    child: Child,
    _stdout: BufReader<ChildStdout>,
    url: String,
}

impl Server {
    fn start(root: &Path) -> Self {
        let state = root.join("server");
        let mut child = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--state-dir")
            .arg(&state)
            .args(["server", "run", "--listen", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
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
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--state-dir")
            .arg(&self.state)
            .args(["server", "token", "add", name, "--scopes", scopes, "--json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let added: Value = serde_json::from_slice(&output.stdout).unwrap();
        added["token"].as_str().unwrap().to_owned()
    }

    /// Stored (publisher, record) pairs.
    fn entries(&self) -> Vec<(String, Value)> {
        let db = Connection::open(self.state.join("review-store.sqlite")).unwrap();
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
        let db = Connection::open(self.state.join("review-store.sqlite")).unwrap();
        let mut statement = db
            .prepare("SELECT execution_id,last_used FROM entries ORDER BY execution_id")
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .env("USER", self.user)
            .env("ARTIFACTIZE_REMOTE", &self.url)
            .env("ARTIFACTIZE_REMOTE_TOKEN", &self.token)
            .env_remove("ARTIFACTIZE_REMOTE_SHARE");
        command
    }

    /// Text stdout and stderr; neither ever contains the token.
    fn run(&self, repo: &Path, args: &[&str], code: i32) -> (String, String) {
        checked(
            self.command(repo, args).output().unwrap(),
            code,
            &self.token,
        )
    }

    fn json(&self, repo: &Path, args: &[&str], code: i32) -> Value {
        let (stdout, _) = self.run(repo, &[args, &["--json"]].concat(), code);
        serde_json::from_str(&stdout).unwrap()
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

    let (text, stderr) = alice.run(&repo_a, &["verify", "--all"], 0);
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
        assert!(!record.to_string().contains(root.path().to_str().unwrap()));
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

    let (text, _) = bob.run(&repo_b, &["verify", "--all"], 0);
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
    let (text, stderr) = bob.run(&repo_b, &["verify", "--all"], 0);
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

    let (text, stderr) = bob.run(&repo, &["verify", "--all"], 0);
    assert_eq!(
        line(&text, "Summary:"),
        "Summary: executed 2 (runtime 2, agent 0, human 0), reused 0 (runtime 0, agent 0, human 0)"
    );
    assert_eq!(stderr.matches(OFFLINE).count(), 1, "{stderr}");
    assert!(stderr.contains("unreachable"), "{stderr}");
    // The store is asked again next time; while it is down, the local latest is reused.
    let (text, stderr) = bob.run(&repo, &["verify", "--all"], 0);
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
    let (_, stderr) = rejected.run(&repo, &["verify", "--all"], 2);
    assert!(stderr.contains("rejected the token (HTTP 401)"), "{stderr}");
    assert_eq!(rejected.json(&repo, &["run", "list"], 0), json!([]));
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
        .command(&repo, &["verify", "--all", "--timeout-ms", "30000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // A token without the human scope keeps the sign-off local. Each signer's own verify
    // records the request and stops waiting at once.
    let record = ["verify", "--all", "--timeout-ms", "1"];
    let run = laptop.json(&repo, &record, 3);
    let stderr = laptop.sign(&repo, &run);
    assert!(
        stderr.contains("stays local: remote token alice-laptop lacks the human scope"),
        "{stderr}"
    );
    assert_eq!(server.entries(), []);

    let run = signer.json(&repo, &record, 3);
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
    let (text, _) = carol.run(&repo, &["verify", "--all", "--reuse-only", "human"], 0);
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
    offline.run(&runtime, &["verify", "--all"], 0);
    let run = offline.json(&human, &["verify", "--all", "--timeout-ms", "1"], 3);
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
        bob.json(&runtime, &["verify", "--all"], 0)["executionsStarted"],
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
    let first = alice.json(&repo_a, &["verify", "--all"], 0);
    let key = first["requests"][0]["key"].as_str().unwrap().to_owned();
    assert_eq!(server.entries().len(), 1);
    let used = server.last_used();

    // Bob reviews again by force: it reads nothing from the store, so no record's last use
    // changes, and its RED is published as a newer record without `remote push`.
    fs::write(repo_b.join("broken"), "").unwrap();
    let forced = bob.json(&repo_b, &["verify", "--all", "--force"], 1);
    assert_eq!(forced["requests"][0]["key"], key.as_str());
    let entries = server.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].0, "bob-laptop");
    assert_eq!(entries[1].1["verdict"], "RED");
    let alice_record = first["requests"][0]["executionId"].as_str().unwrap();
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
    let reused = carol.json(&repo_c, &["verify", "--all"], 1);
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
    let newer = alice.json(&repo_a, &["verify", "--all"], 1);
    assert_eq!(newer["requests"][0]["provenance"]["runId"], forced["id"]);
    assert_eq!(newer["requests"][0]["origin"]["publisher"], "bob-laptop");
    assert_eq!(history().as_array().unwrap().len(), 2);

    // A newer local record beats an older store one: alice reviews again offline.
    let local = offline.json(&repo_a, &["verify", "--all", "--force"], 0);
    assert_eq!(server.entries().len(), 2);
    let status = alice.json(&repo_a, &["status"], 0);
    assert_eq!(status["evals"][0]["state"], "PASS");
    let again = alice.json(&repo_a, &["verify", "--all"], 0);
    assert_eq!(
        again["requests"][0]["executionId"],
        local["requests"][0]["executionId"]
    );
    assert!(again["requests"][0]["origin"].is_null());
    assert_eq!(history().as_array().unwrap().len(), 3);
}
