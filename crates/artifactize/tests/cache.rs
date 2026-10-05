use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    state: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        Self {
            state: root.path().join("state"),
            root,
        }
    }

    fn repo(&self, name: &str, value: Value) -> PathBuf {
        let repo = self.root.path().join(name);
        write(&repo, "artifactize.json", value);
        repo
    }

    fn command(&self, repo: &Path, args: &[&str], code: i32) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .arg("--json")
            .output()
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
        Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(["verify", "--all", "--json"])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn waiting_request(&self) -> Value {
        let mut request = Value::Null;
        wait_until(|| {
            let Ok(db) = Connection::open_with_flags(
                self.state.join("state.sqlite"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            ) else {
                return false;
            };
            let data = db.query_row::<String, _, _>(
                "SELECT data FROM requests WHERE status='QUEUED' AND execution_id IS NOT NULL LIMIT 1",
                [], |row| row.get(0),
            ).ok();
            if let Some(data) = data {
                request = serde_json::from_str(&data).unwrap();
                true
            } else {
                false
            }
        });
        request
    }

    fn execution(&self, id: &str) -> Value {
        let data: String = Connection::open(self.state.join("state.sqlite"))
            .unwrap()
            .query_row("SELECT data FROM executions WHERE id=?", [id], |row| {
                row.get(0)
            })
            .unwrap();
        serde_json::from_str(&data).unwrap()
    }

    fn shared_repo(&self, name: &str, script: &str) -> PathBuf {
        let repo = self.repo(name, json!({"name":name,"fingerprint":fingerprint("concurrent"),"evals":[{
            "id":"check","title":"Review","profile":{"kind":"runtime","command":"/bin/sh",
                "args":["review.sh",self.root.path().join("starts"),self.root.path().join("release")],"timeoutMs":10000},
            "payload":{"instruction":"Review."}
        }]}));
        fs::write(repo.join("review.sh"), script).unwrap();
        repo
    }

    fn release(&self) {
        fs::write(self.root.path().join("release"), "").unwrap();
    }

    /// Started review scripts; the shell creates the file before it writes the line.
    fn starts(&self) -> usize {
        fs::read_to_string(self.root.path().join("starts"))
            .map_or(0, |starts| starts.lines().count())
    }

    fn count(&self, table: &str) -> u32 {
        Connection::open(self.state.join("state.sqlite"))
            .unwrap()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn seed_entries(&self, count: usize, bytes: i64) {
        let mut db = Connection::open(self.state.join("state.sqlite")).unwrap();
        let transaction = db.transaction().unwrap();
        for i in 0..count {
            let fingerprint = format!("seed-{i:05}");
            transaction.execute(
                "INSERT INTO executions(id,fingerprint,eval_def_hash,owner_pid,owner_start_time,status,data) SELECT ?,?,eval_def_hash,owner_pid,owner_start_time,status,json_set(data,'$.id',?,'$.fingerprint',?) FROM executions LIMIT 1",
                rusqlite::params![fingerprint, fingerprint, fingerprint, fingerprint],
            ).unwrap();
            transaction.execute(
                "INSERT INTO cache_entries(fingerprint,eval_def_hash,execution_id,bytes,last_used) VALUES (?,(SELECT eval_def_hash FROM executions LIMIT 1),?,?,?)",
                rusqlite::params![fingerprint, fingerprint, bytes, format!("2000-01-01T00:00:00.{i:09}Z")],
            ).unwrap();
        }
        transaction.commit().unwrap();
    }

    /// Verify a new fingerprint from its own repository; publishing the entry runs LRU GC.
    fn publish(&self, key: &str) {
        let repo = self.repo(
            key,
            json!({"name":"publisher","fingerprint":fingerprint(key),"evals":[eval("check","exit 0")]}),
        );
        self.command(&repo, &["verify", "--all"], 0);
    }

    fn entries(&self) -> Vec<(String, String, i64, String, String)> {
        let db = Connection::open(self.state.join("state.sqlite")).unwrap();
        let mut statement = db.prepare("SELECT c.fingerprint,c.execution_id,c.bytes,c.last_used,e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id ORDER BY c.fingerprint").unwrap();
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

const WAIT_SCRIPT: &str = "echo $$ >> \"$1\"; i=0; while [ ! -e \"$2\" ] && [ $i -lt 200 ]; do sleep 0.05; i=$((i+1)); done; printf original";

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
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

fn signal(pid: u32, signal: &str) {
    assert!(
        Command::new("/bin/kill")
            .args([signal, &pid.to_string()])
            .status()
            .unwrap()
            .success()
    );
}

fn write(repo: &Path, path: &str, value: Value) {
    let path = repo.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, value.to_string()).unwrap();
}

fn eval(id: &str, script: &str) -> Value {
    json!({"id":id,"title":"Review","profile":{"kind":"runtime","command":"/bin/sh","args":["-c",script]},"payload":{"instruction":"Review."}})
}

fn fingerprint(key: &str) -> Value {
    json!({"script":{"command":"/bin/echo","args":[key]}})
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
    let source = fixture.repo("source", json!({"name":"original","fingerprint":fingerprint("shared:red"),"evals":[eval("first","printf original; exit 7")]}));
    let original = fixture.command(&source, &["verify", "--all"], 1);
    let original_request = &original["requests"][0];
    assert!(original_request["child"]["pid"].is_number());
    assert!(original_request["usage"].is_null());
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(fixture.count("cache_entries"), 1);
    fs::remove_dir_all(&source).unwrap();
    let target = fixture.repo("target", json!({"name":"root","basis":true}));
    let mut same = eval("second", "printf original; exit 7");
    same["title"] = json!("Renamed review");
    write(
        &target,
        "dependency/artifactize.json",
        json!({"name":"dependency","fingerprint":fingerprint("shared:red"),"evals":[same]}),
    );
    let mut consumer = eval("check", "touch must-not-run");
    consumer["payload"]["instruction"] = json!("Check {dependency}.");
    write(
        &target,
        "consumer/artifactize.json",
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
    let reused = fixture.command(&target, &["verify", "consumer", "--recursive"], 1);
    let hit = request(&reused, "dependency/second");
    for field in ["result", "profile", "executionId", "provenance", "usage"] {
        assert_eq!(hit[field], original_request[field], "{field}");
    }
    assert_eq!(hit["requestedProfile"], hit["profile"]);
    assert_eq!(
        hit["provenance"]["repoPath"],
        source.to_string_lossy().as_ref()
    );
    assert_eq!(hit["provenance"]["evalId"], "original/first");
    assert_eq!(
        hit["provenance"]["completedAt"],
        original_request["completedAt"]
    );
    for field in ["child", "argv", "runDir", "startedAt"] {
        assert!(hit[field].is_null(), "{field}");
    }
    assert_eq!(request(&reused, "consumer/check")["status"], "BLOCKED");
    assert_eq!(reused["validation"]["satisfied"], false);
    assert!(!target.join("dependency/must-not-run").exists());
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
    let nonrecursive = fixture.command(&target, &["verify", "consumer"], 1);
    assert_eq!(nonrecursive["requests"].as_array().unwrap().len(), 1);
    assert_eq!(nonrecursive["requests"][0]["status"], "BLOCKED");
}

#[test]
fn distinct_evals_on_one_fingerprint_execute_and_status_reuses_each_definition() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo", json!({"name":"test","fingerprint":fingerprint("shared"),"evals":[
        {"id":"pass","title":"Pass","profile":{"kind":"runtime","command":"/bin/true","args":[]},"payload":{"instruction":"Review."}},
        {"id":"fail","title":"Fail","profile":{"kind":"runtime","command":"/bin/false","args":[]},"payload":{"instruction":"Review."}}
    ]}));
    let run = fixture.command(&repo, &["verify", "--all", "--jobs", "1"], 1);
    let pass = &run["requests"][0];
    let fail = &run["requests"][1];
    assert_eq!(pass["status"], "GREEN");
    assert_eq!(fail["status"], "RED");
    assert_ne!(pass["executionId"], fail["executionId"]);
    assert_ne!(pass["evalDefHash"], fail["evalDefHash"]);
    assert_eq!(pass["evalDefHash"].as_str().unwrap().len(), 64);
    assert_eq!(pass["provenance"]["evalDefHash"], pass["evalDefHash"]);
    assert_eq!(fixture.count("executions"), 2);
    assert_eq!(fixture.count("cache_entries"), 2);
    let status = fixture.command(&repo, &["status"], 1);
    assert_eq!(status["evals"][0]["state"], "PASS");
    assert_eq!(status["evals"][1]["state"], "RED");
    assert_eq!(status["counts"]["reuse"], 2);
    let hit = fixture.command(&repo, &["verify", "--all"], 1);
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
    for command in ["show", "rm"] {
        assert!(
            fixture.command(&repo, &["cache", command, "shared"], 2)["error"]
                .as_str()
                .unwrap()
                .contains("multiple Eval definitions")
        );
    }
    let hash = pass["evalDefHash"].as_str().unwrap();
    let cached = fixture.command(&repo, &["cache", "show", "shared", hash], 0);
    assert_eq!(cached["evalDefHash"], hash);
    assert_eq!(cached["id"], pass["executionId"]);
    assert_eq!(
        fixture.command(&repo, &["cache", "rm", "shared", hash], 0),
        json!({"removed":true})
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "show", "shared"], 0)["id"],
        fail["executionId"]
    );
}

#[test]
fn no_fingerprint_executes_each_time_and_never_reads_or_publishes_cache() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("cached"),"evals":[eval("check","exit 0")]}),
    );
    fixture.command(&repo, &["verify", "--all"], 0);
    let before = fixture.entries();
    write(
        &repo,
        "artifactize.json",
        json!({"name":"test","evals":[eval("check","echo run >> starts; exit 8")]}),
    );
    for _ in 0..2 {
        let run = fixture.command(&repo, &["verify", "--all"], 1);
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
    let repo = fixture.repo("repo",json!({"name":"test","fingerprint":fingerprint("retryable"),"evals":[eval("check","kill -TERM $$")]}));
    for _ in 0..2 {
        let run = fixture.command(&repo, &["verify", "--all"], 2);
        assert_eq!(run["requests"][0]["errorCode"], "ABNORMAL_EXIT");
        assert!(run["requests"][0]["result"].is_null());
        assert_eq!(fixture.count("cache_entries"), 0);
    }
    assert_eq!(fixture.count("executions"), 2);
    write(
        &repo,
        "artifactize.json",
        json!({"name":"test","fingerprint":fingerprint("retryable"),"evals":[eval("check","exit 0")]}),
    );
    fixture.command(&repo, &["verify", "--all"], 0);
    assert_eq!(fixture.count("cache_entries"), 1);
}

#[test]
fn force_executes_without_reading_or_replacing_an_entry_but_dependencies_reuse() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("forced"),"evals":[eval("check","sh review.sh")]}),
    );
    fs::write(repo.join("review.sh"), "exit 7").unwrap();
    let original = fixture.command(&repo, &["verify", "--all"], 1);
    let before = fixture.entries();
    fs::write(repo.join("review.sh"), "echo force >> starts").unwrap();
    let forced = fixture.command(&repo, &["verify", "--all", "--force"], 0);
    assert_ne!(
        forced["requests"][0]["executionId"],
        original["requests"][0]["executionId"]
    );
    assert!(forced["requests"][0]["child"]["pid"].is_number());
    assert_eq!(fixture.entries(), before);
    let status = fixture.command(&repo, &["status", "--force"], 1);
    assert_eq!(status["evals"][0]["action"], "execute");
    assert_eq!(fixture.entries(), before);
    assert_eq!(
        fixture.command(&repo, &["verify", "--all"], 1)["requests"][0]["result"],
        original["requests"][0]["result"]
    );
    assert_eq!(fs::read_to_string(repo.join("starts")).unwrap(), "force\n");

    let dependency = fixture.repo(
        "dep",
        json!({"name":"dep","fingerprint":fingerprint("dependency"),"evals":[eval("check","exit 0")]}),
    );
    fixture.command(&dependency, &["verify", "--all"], 0);
    write(
        &repo,
        "dependency/artifactize.json",
        json!({"name":"dep","fingerprint":fingerprint("dependency"),"evals":[eval("check","exit 0")]}),
    );
    let forced = fixture.command(&repo, &["verify", "test", "--recursive", "--force"], 0);
    assert_eq!(request(&forced, "test/check")["force"], true);
    assert_eq!(request(&forced, "dep/check")["force"], false);
    assert!(request(&forced, "dep/check")["child"].is_null());
    assert!(!repo.join("dependency/must-not-run").exists());

    let absent = fixture.repo("absent",json!({"name":"absent","fingerprint":fingerprint("never-published"),"evals":[eval("check","exit 0")]}));
    let entries = fixture.entries();
    fixture.command(&absent, &["verify", "--all", "--force"], 0);
    assert_eq!(fixture.entries(), entries);
}

#[test]
fn status_uses_current_fingerprint_and_only_prepares_the_selected_closure() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo",json!({"name":"test","fingerprint":{"script":{"command":"/bin/cat","args":["key"]}},"evals":[eval("check","touch executed")]}));
    fs::write(repo.join("key"), "first").unwrap();
    fixture.command(&repo, &["verify", "--all"], 0);
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
        "selected/artifactize.json",
        json!({"name":"selected","fingerprint":fingerprint("isolated"),"evals":[eval("check","touch must-not-run")]}),
    );
    let selected = fixture.command(&repo, &["status", "selected"], 1);
    assert_eq!(selected["evals"].as_array().unwrap().len(), 1);
    assert_eq!(fixture.count("runs"), 1);
    assert!(!repo.join("selected/must-not-run").exists());
}

#[test]
fn concurrent_repos_claim_once_and_poll_for_the_original_red_result() {
    let fixture = Fixture::new();
    let first = fixture.shared_repo("first", &format!("{WAIT_SCRIPT}; exit 7"));
    let second = fixture.shared_repo("second", &format!("{WAIT_SCRIPT}; exit 7"));
    let owner = fixture.spawn(&first, &[]);
    let waiter = fixture.spawn(&second, &[]);
    let waiting = fixture.waiting_request();
    wait_until(|| fixture.starts() > 0);
    assert_eq!(fixture.starts(), 1);
    let execution = fixture.execution(waiting["executionId"].as_str().unwrap());
    assert_eq!(execution["status"], "RUNNING");
    assert!(execution["completedAt"].is_null());
    assert!(execution["ownerStartTime"].as_u64().unwrap() > 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.busy_timeout(Duration::from_millis(100)).unwrap();
    db.execute_batch("BEGIN IMMEDIATE; COMMIT;").unwrap();
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
    assert_eq!(fixture.count("cache_entries"), 1);
    let follower = if first["requests"][0]["child"].is_null() {
        &first
    } else {
        &second
    };
    assert!(follower["requests"][0]["startedAt"].is_null());
    assert!(follower["requests"][0]["blockedReason"].is_null());
}

#[test]
fn killed_owner_is_reclaimed_by_a_waiting_verify() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT_SCRIPT);
    let target = fixture.shared_repo("target", "echo replacement >> \"$1\"; printf recovered");
    let mut owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let waiter = fixture.spawn(&target, &["--max-executions", "1"]);
    let waiting = fixture.waiting_request();
    owner.kill().unwrap();
    owner.wait().unwrap();
    let recovered = finish(waiter, 0);
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
    assert_eq!(fixture.count("cache_entries"), 1);
    assert_eq!(fixture.starts(), 2);
    assert_eq!(recovered["executionsStarted"], 1);
    // SIGKILL cannot run foreground cleanup; stop this fixture's orphaned group.
    let pid = fs::read_to_string(fixture.root.path().join("starts")).unwrap();
    assert!(
        Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", pid.lines().next().unwrap())])
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn owner_error_releases_claim_and_waiter_uses_its_own_profile() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", &format!("{WAIT_SCRIPT}; kill -TERM $$"));
    let target = fixture.shared_repo("target", "echo retry >> \"$1\"; printf retried");
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let waiter = fixture.spawn(&target, &[]);
    fixture.waiting_request();
    fixture.release();
    let failed = finish(owner, 2);
    let recovered = finish(waiter, 0);
    assert_eq!(failed["requests"][0]["errorCode"], "ABNORMAL_EXIT");
    assert_eq!(recovered["requests"][0]["result"]["stdout"], "retried");
    assert_eq!(
        recovered["requests"][0]["profile"],
        recovered["requests"][0]["requestedProfile"]
    );
    assert_eq!(fixture.count("executions"), 2);
    assert_eq!(fixture.count("cache_entries"), 1);
    assert_eq!(fixture.starts(), 2);
}

#[test]
fn waiter_ctrl_c_leaves_owner_running_and_owner_ctrl_c_releases_the_claim() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT_SCRIPT);
    let target = fixture.shared_repo("target", "echo retry >> \"$1\"; printf retried");
    let mut owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let waiter = fixture.spawn(&target, &[]);
    let waiting = fixture.waiting_request();
    signal(waiter.id(), "-INT");
    let cancelled = finish(waiter, 2);
    assert_eq!(cancelled["requests"][0]["errorCode"], "CANCELLED");
    assert!(cancelled["requests"][0]["child"].is_null());
    assert!(owner.try_wait().unwrap().is_none());
    assert_eq!(
        fixture.execution(waiting["executionId"].as_str().unwrap())["status"],
        "RUNNING"
    );
    assert_eq!(fixture.starts(), 1);
    let waiter = fixture.spawn(&target, &[]);
    fixture.waiting_request();
    signal(owner.id(), "-INT");
    let cancelled = finish(owner, 2);
    assert_eq!(cancelled["requests"][0]["errorCode"], "CANCELLED");
    let execution = fixture.execution(waiting["executionId"].as_str().unwrap());
    assert_eq!(execution["errorCode"], "CANCELLED");
    let recovered = finish(waiter, 0);
    assert_eq!(recovered["requests"][0]["result"]["stdout"], "retried");
    assert_eq!(fixture.count("executions"), 2);
    let pid = cancelled["requests"][0]["child"]["pid"].as_u64().unwrap();
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
}

#[test]
fn mismatched_start_time_is_reclaimed_but_status_and_saved_queries_do_not_reconcile() {
    let fixture = Fixture::new();
    let repo = fixture.shared_repo("repo", "echo attempt >> \"$1\"");
    let original = fixture.command(&repo, &["verify", "--all"], 0);
    let id = original["requests"][0]["executionId"].as_str().unwrap();
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute("DELETE FROM cache_entries", []).unwrap();
    db.execute(
        "UPDATE executions SET owner_pid=?,owner_start_time=0,status='RUNNING',data=json_set(data,'$.ownerPid',?,'$.ownerStartTime',0,'$.status','RUNNING','$.result',NULL,'$.completedAt',NULL,'$.provenance.completedAt',NULL) WHERE id=?",
        rusqlite::params![std::process::id(), std::process::id(), id],
    ).unwrap();
    assert_eq!(
        fixture.command(&repo, &["status"], 1)["evals"][0]["action"],
        "execute"
    );
    fixture.command(&repo, &["run", "show", original["id"].as_str().unwrap()], 0);
    assert_eq!(fixture.execution(id)["status"], "RUNNING");
    let recovered = fixture.command(&repo, &["verify", "--all"], 0);
    assert_ne!(recovered["requests"][0]["executionId"], id);
    assert_eq!(fixture.execution(id)["errorCode"], "OWNER_DIED");
    assert_eq!(fixture.starts(), 2);
}

#[test]
fn force_bypasses_a_live_claim_and_never_publishes() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT_SCRIPT);
    let target = fixture.shared_repo("target", "echo forced >> \"$1\"; printf forced");
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let forced = finish(fixture.spawn(&target, &["--force"]), 0);
    let execution = fixture.execution(forced["requests"][0]["executionId"].as_str().unwrap());
    assert!(execution["fingerprint"].is_null());
    assert_eq!(fixture.count("cache_entries"), 0);
    assert_eq!(fixture.starts(), 2);
    fixture.release();
    let original = finish(owner, 0);
    let reused = fixture.command(&target, &["verify", "--all"], 0);
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
            eval("first", "echo first >> starts; sleep 0.1"),
            eval("second", "echo first >> starts; sleep 0.1"),
            eval("third", "echo first >> starts; sleep 0.1")
        ]}),
    );
    let run = fixture.command(
        &repo,
        &["verify", "--all", "--jobs", "3", "--max-executions", "1"],
        0,
    );
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(fixture.count("executions"), 1);
    for request in run["requests"].as_array().unwrap() {
        assert_eq!(request["executionId"], run["requests"][0]["executionId"]);
    }
    let hit = fixture.command(&repo, &["verify", "--all", "--max-executions", "0"], 0);
    assert_eq!(hit["executionsStarted"], 0);
    let forced = fixture.command(
        &repo,
        &["verify", "--all", "--max-executions", "0", "--force"],
        4,
    );
    assert_eq!(forced["executionsStarted"], 0);
    assert_eq!(fixture.count("executions"), 1);
    assert!(!repo.join("must-not-run").exists());
}

#[test]
fn zero_budget_can_join_an_owner_but_cannot_replace_it_after_failure() {
    for success in [true, false] {
        let fixture = Fixture::new();
        let script = if success {
            WAIT_SCRIPT.to_owned()
        } else {
            format!("{WAIT_SCRIPT}; kill -TERM $$")
        };
        let source = fixture.shared_repo("source", &script);
        let target = fixture.shared_repo("target", "echo replacement >> \"$1\"");
        let owner = fixture.spawn(&source, &[]);
        wait_until(|| fixture.starts() > 0);
        let waiter = fixture.spawn(&target, &["--max-executions", "0"]);
        fixture.waiting_request();
        fixture.release();
        let original = finish(owner, if success { 0 } else { 2 });
        let joined = finish(waiter, if success { 0 } else { 4 });
        assert_eq!(original["executionsStarted"], 1);
        assert_eq!(joined["executionsStarted"], 0);
        assert_eq!(fixture.starts(), 1);
        assert_eq!(fixture.count("executions"), 1);
        if success {
            assert_eq!(
                joined["requests"][0]["executionId"],
                original["requests"][0]["executionId"]
            );
        } else {
            assert_eq!(joined["requests"][0]["status"], "BUDGET_EXHAUSTED");
        }
    }
}

#[test]
fn a_fingerprint_waiter_occupies_a_job_slot_without_consuming_execution_budget() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT_SCRIPT);
    let target = fixture.shared_repo("a-target", "touch must-not-run");
    write(
        &target,
        "independent/artifactize.json",
        json!({"name":"z-independent","evals":[eval("check", "touch ran")]}),
    );
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let waiter = fixture.spawn(
        &target,
        &["--jobs", "1", "--max-executions", "1", "--ignore-gates"],
    );
    fixture.waiting_request();
    thread::sleep(Duration::from_millis(250));
    assert!(!target.join("independent/ran").exists());
    fixture.release();
    finish(owner, 0);
    let run = finish(waiter, 0);
    assert_eq!(run["executionsStarted"], 1);
    assert_eq!(fixture.starts(), 1);
    assert!(target.join("independent/ran").exists());
}

#[test]
fn cache_commands_are_inert_for_missing_and_empty_state() {
    let fixture = Fixture::new();
    let missing_repo = fixture.root.path().join("missing-repo");
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
            fixture.command(&missing_repo, &["cache", "show", "missing"], 4),
            Value::Null
        );
        assert_eq!(
            fixture.command(&missing_repo, &["cache", "rm", "missing"], 0),
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
    let repo = fixture.repo("repo", json!({"name":"original","fingerprint":fingerprint("entry"),"evals":[eval("check","printf original; exit 7")]}));
    let run = fixture.command(&repo, &["verify", "--all"], 1);
    fs::remove_dir_all(&repo).unwrap();
    let before = fixture.entries();
    let entries = fixture.command(&repo, &["cache", "list"], 0);
    assert_eq!(entries.as_array().unwrap().len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["fingerprint"], "entry");
    assert_eq!(entry["verdict"], "RED");
    assert_eq!(entry["repoPath"], run["repoPath"]);
    assert_eq!(entry["evalId"], "original/check");
    assert_eq!(entry["bytes"], before[0].2);
    assert_eq!(entry["lastUsed"], before[0].3);
    let saved = fixture.command(&repo, &["cache", "show", "entry"], 0);
    assert_eq!(
        saved,
        fixture.execution(run["requests"][0]["executionId"].as_str().unwrap())
    );
    for field in ["result", "profile", "provenance", "usage"] {
        assert_eq!(saved[field], run["requests"][0][field]);
    }
    for args in [vec!["cache", "list"], vec!["cache", "show", "entry"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--state-dir")
            .arg(&fixture.state)
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success());
        if args[1] == "list" {
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains("FINGERPRINT\tEVAL HASH\tVERDICT\tREPO\tEVAL\tBYTES\tLAST USED"));
            assert!(text.contains(&format!(
                "entry\t{}\tRED",
                entry["evalDefHash"].as_str().unwrap()
            )));
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
        fixture.command(&repo, &["cache", "rm", "entry"], 0),
        json!({"removed":true})
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "rm", "entry"], 0),
        json!({"removed":false})
    );
    assert_eq!(
        fixture.command(&repo, &["cache", "show", "entry"], 4),
        Value::Null
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
        json!({"name":"test","fingerprint":fingerprint("seed-00000"),"evals":[eval("check","exit 0")]}),
    );
    let original = fixture.command(&repo, &["verify", "--all"], 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute("DELETE FROM cache_entries", []).unwrap();
    fixture.seed_entries(10_000, 1);
    let hit = fixture.command(&repo, &["verify", "--all"], 0);
    assert_eq!(hit["requests"][0]["executionId"], "seed-00000");
    let used: String = db
        .query_row(
            "SELECT last_used FROM cache_entries WHERE fingerprint='seed-00000'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(used.as_str() > "2000");
    write(
        &repo,
        "artifactize.json",
        json!({"name":"test","fingerprint":fingerprint("new"),"evals":[eval("check","exit 0")]}),
    );
    fixture.command(&repo, &["verify", "--all"], 0);
    assert_eq!(fixture.count("cache_entries"), 10_000);
    assert_eq!(
        fixture.command(&repo, &["cache", "show", "seed-00001"], 4),
        Value::Null
    );
    assert!(
        fixture
            .command(&repo, &["cache", "show", "seed-00000"], 0)
            .is_object()
    );
    assert!(
        fixture
            .command(&repo, &["cache", "show", "new"], 0)
            .is_object()
    );
    assert_eq!(fixture.count("executions"), 10_002);
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
        json!({"name":"test","fingerprint":fingerprint("original"),"evals":[eval("check","exit 0")]}),
    );
    let original = fixture.command(&repo, &["verify", "--all"], 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute("DELETE FROM cache_entries", []).unwrap();
    fixture.seed_entries(65, 16 * MIB);
    db.execute("INSERT INTO executions(id,fingerprint,eval_def_hash,owner_pid,owner_start_time,status,data) SELECT 'active','seed-00000',eval_def_hash,1,1,'WAITING_HUMAN','{}' FROM executions LIMIT 1", []).unwrap();
    db.execute("INSERT INTO requests(id,run_id,execution_id,status,data) VALUES ('waiter',?,'seed-00001','QUEUED','{}')", [original["id"].as_str().unwrap()]).unwrap();
    for key in ["seed-00000", "seed-00001"] {
        assert!(
            fixture.command(&repo, &["cache", "rm", key], 2)["error"]
                .as_str()
                .unwrap()
                .contains("in use")
        );
    }
    // The new entry takes 65 seeded 16 MiB entries over 1 GiB.
    fixture.publish("first");
    assert_eq!(fixture.count("cache_entries"), 64);
    assert!(
        db.query_row::<i64, _, _>("SELECT sum(bytes) FROM cache_entries", [], |row| row.get(0))
            .unwrap()
            <= 1024 * MIB
    );
    for key in ["seed-00000", "seed-00001", "seed-00004", "first"] {
        assert!(
            fixture
                .command(&repo, &["cache", "show", key], 0)
                .is_object()
        );
    }
    for key in ["seed-00002", "seed-00003"] {
        assert_eq!(
            fixture.command(&repo, &["cache", "show", key], 4),
            Value::Null
        );
    }
    assert_eq!(fixture.count("executions"), 68);
    assert_eq!(fixture.count("requests"), 3);
    assert_eq!(
        db.query_row::<String, _, _>(
            "SELECT status FROM executions WHERE id='active'",
            [],
            |row| row.get(0)
        )
        .unwrap(),
        "WAITING_HUMAN"
    );
    db.execute("UPDATE cache_entries SET bytes=?", [16 * MIB + 1])
        .unwrap();
    fixture.publish("second");
    let keys = || {
        fixture
            .entries()
            .into_iter()
            .map(|entry| entry.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        keys(),
        ["second", "seed-00000", "seed-00001"],
        "protected rows survive even when oversized"
    );
    db.execute("UPDATE executions SET status='ERROR' WHERE id='active'", [])
        .unwrap();
    db.execute("UPDATE requests SET status='GREEN' WHERE id='waiter'", [])
        .unwrap();
    fixture.publish("third");
    assert_eq!(keys(), ["second", "third"]);
    assert_eq!(fixture.count("executions"), 70);
}

#[test]
fn rm_refuses_an_active_fingerprint_even_without_an_entry() {
    let fixture = Fixture::new();
    let repo = fixture.shared_repo("repo", WAIT_SCRIPT);
    let mut owner = fixture.spawn(&repo, &[]);
    wait_until(|| fixture.starts() > 0);
    assert!(
        fixture.command(&repo, &["cache", "rm", "concurrent"], 2)["error"]
            .as_str()
            .unwrap()
            .contains("active execution")
    );
    fixture.publish("other");
    assert_eq!(fixture.count("executions"), 2);
    assert!(owner.try_wait().unwrap().is_none());
    fixture.release();
    finish(owner, 0);
}

#[test]
fn waiter_receives_its_original_execution_after_entry_eviction_and_replacement() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT_SCRIPT);
    let target = fixture.shared_repo("target", "echo unexpected >> \"$1\"; printf unexpected");
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let waiter = fixture.spawn(&target, &["--max-executions", "0"]);
    let waiting = fixture.waiting_request();
    signal(waiter.id(), "-STOP");
    fixture.release();
    let original = finish(owner, 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute("DELETE FROM cache_entries", []).unwrap();
    db.execute("INSERT INTO executions(id,fingerprint,eval_def_hash,owner_pid,owner_start_time,status,data) SELECT 'replacement',fingerprint,eval_def_hash,owner_pid,owner_start_time,status,json_set(data,'$.id','replacement','$.result.stdout','replacement') FROM executions WHERE id=?", [waiting["executionId"].as_str().unwrap()]).unwrap();
    db.execute("INSERT INTO cache_entries(fingerprint,eval_def_hash,execution_id,bytes,last_used) SELECT 'concurrent',eval_def_hash,'replacement',1,'2000-01-01T00:00:00Z' FROM executions WHERE id='replacement'", []).unwrap();
    signal(waiter.id(), "-CONT");
    let joined = finish(waiter, 0);
    assert_eq!(
        joined["requests"][0]["result"],
        original["requests"][0]["result"]
    );
    assert_eq!(joined["requests"][0]["executionId"], waiting["executionId"]);
    assert_eq!(fixture.starts(), 1);
    assert_eq!(
        fixture.command(&target, &["cache", "list"], 0)[0]["lastUsed"],
        "2000-01-01T00:00:00Z"
    );
}

#[tokio::test]
async fn oversized_completion_is_delivered_to_owner_and_waiter_but_not_retained() {
    use artifactize::store::{Execution, Receipts, Request};
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT_SCRIPT);
    let target = fixture.shared_repo("target", "echo unexpected >> \"$1\"; printf unexpected");
    let mut owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.starts() > 0);
    let waiter = fixture.spawn(&target, &["--max-executions", "0"]);
    let waiting = fixture.waiting_request();
    signal(waiter.id(), "-STOP");
    let mut execution: Execution =
        serde_json::from_value(fixture.execution(waiting["executionId"].as_str().unwrap()))
            .unwrap();
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    let data: String = db
        .query_row(
            "SELECT data FROM requests WHERE id=?",
            [&execution.provenance.request_id],
            |row| row.get(0),
        )
        .unwrap();
    let mut request: Request = serde_json::from_str(&data).unwrap();
    owner.kill().unwrap();
    owner.wait().unwrap();
    let pid = fs::read_to_string(fixture.root.path().join("starts")).unwrap();
    assert!(
        Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", pid.trim())])
            .status()
            .unwrap()
            .success()
    );
    let receipts = Receipts::open(&fixture.state, &source).await.unwrap();
    execution.status = "GREEN".into();
    execution.result = Some(json!({"verdict":"GREEN", "large":"x".repeat(16 * 1024 * 1024)}));
    execution.completed_at = Some("2026-10-04T00:00:00Z".into());
    execution.provenance.completed_at = execution.completed_at.clone();
    request.status = execution.status.clone();
    request.result = execution.result.clone();
    request.completed_at = execution.completed_at.clone();
    request.provenance = Some(execution.provenance.clone());
    receipts
        .complete_execution(&execution, &request)
        .await
        .unwrap();
    assert_eq!(fixture.count("cache_entries"), 0);
    assert_eq!(
        artifactize::store::read_run(&fixture.state, &request.run_id)
            .await
            .unwrap()
            .requests[0]
            .result,
        request.result
    );
    signal(waiter.id(), "-CONT");
    // Drain stdout while the large result is written, rather than waiting on a full pipe.
    let joined = output(waiter.wait_with_output().unwrap(), 0);
    assert_eq!(
        joined["requests"][0]["result"],
        serde_json::to_value(execution.result).unwrap()
    );
    assert_eq!(joined["requests"][0]["executionId"], execution.id);
    assert_eq!(joined["executionsStarted"], 0);
    assert_eq!(fixture.count("cache_entries"), 0);
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(fixture.starts(), 1);
    let later = fixture.command(&target, &["verify", "--all"], 0);
    assert_ne!(later["requests"][0]["executionId"], execution.id);
    assert_eq!(later["requests"][0]["result"]["stdout"], "unexpected");
}

#[test]
fn gc_failure_does_not_replace_a_completed_result() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("first"),"evals":[eval("check","exit 0")]}),
    );
    fixture.command(&repo, &["verify", "--all"], 0);
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute("UPDATE cache_entries SET bytes=1073741824", [])
        .unwrap();
    db.execute_batch("CREATE TRIGGER fail_gc BEFORE DELETE ON cache_entries BEGIN SELECT RAISE(FAIL,'GC unavailable'); END;").unwrap();
    write(
        &repo,
        "artifactize.json",
        json!({"name":"test","fingerprint":fingerprint("second"),"evals":[eval("check","exit 0")]}),
    );
    let verified = fixture.spawn(&repo, &[]).wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&verified.stderr).into_owned();
    assert!(
        stderr.contains("Cache GC failed") && stderr.contains("GC unavailable"),
        "{stderr}"
    );
    let run = output(verified, 0);
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert!(
        fixture
            .command(&repo, &["cache", "show", "second"], 0)
            .is_object()
    );
    db.execute_batch("DROP TRIGGER fail_gc").unwrap();
    fixture.publish("third");
    assert_eq!(
        fixture.command(&repo, &["cache", "show", "first"], 4),
        Value::Null,
        "the next publication retries collection"
    );
    assert!(
        fixture
            .command(&repo, &["cache", "show", "second"], 0)
            .is_object()
    );
}

#[test]
fn changed_schema_profile_args_and_payload_require_new_executions() {
    let fixture = Fixture::new();
    let base = json!({"id":"check","title":"Review","profile":{"kind":"runtime","command":"/bin/true","args":[]},"payload":{"instruction":"Review."}});
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("unchanged"),"evals":[base]}),
    );
    let original = fixture.command(&repo, &["verify", "--all"], 0);
    for (pointer, value, code) in [
        (
            "/passSchema",
            json!({"type":"object","properties":{"extra":{"type":"string"}}}),
            0,
        ),
        (
            "/failSchema",
            json!({"type":"object","properties":{"reason":{"type":"string"}}}),
            0,
        ),
        ("/profile/command", json!("/bin/false"), 1),
        ("/profile/args", json!(["unused"]), 0),
        ("/profile/timeoutMs", json!(1000), 0),
        ("/payload/instruction", json!("Different criteria."), 0),
        ("/payload/extra", json!({"criteria":[1,2]}), 0),
    ] {
        let mut changed = base.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        changed.pointer_mut(parent).unwrap()[key] = value;
        write(
            &repo,
            "artifactize.json",
            json!({"name":"test","fingerprint":fingerprint("unchanged"),"evals":[changed]}),
        );
        assert_eq!(
            fixture.command(&repo, &["status"], 1)["evals"][0]["action"],
            "execute",
            "{pointer}"
        );
        let run = fixture.command(&repo, &["verify", "--all"], code);
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
    assert_eq!(fixture.count("executions"), 8);
    assert_eq!(fixture.count("cache_entries"), 8);
}

#[test]
fn only_the_effective_profile_variant_partitions_reuse() {
    let fixture = Fixture::new();
    let mut declaration = eval("check", "exit 0");
    declaration["profileVariants"] = json!({
        "fail":{"kind":"runtime","command":"/bin/false","args":[]},
        "same":{"kind":"runtime","command":"/bin/sh","args":["-c","exit 0"]}
    });
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","fingerprint":fingerprint("variants"),"evals":[declaration]}),
    );
    let original = fixture.command(&repo, &["verify", "--all"], 0);
    let same = fixture.command(&repo, &["verify", "--all", "--profile", "same"], 0);
    assert_eq!(
        same["requests"][0]["executionId"],
        original["requests"][0]["executionId"]
    );
    assert_eq!(
        fixture.command(&repo, &["status", "--profile", "fail"], 1)["evals"][0]["action"],
        "execute"
    );
    let different = fixture.command(&repo, &["verify", "--all", "--profile", "fail"], 1);
    assert_eq!(different["executionsStarted"], 1);
    assert_ne!(
        different["requests"][0]["evalDefHash"],
        original["requests"][0]["evalDefHash"]
    );
    declaration["profileVariants"]["fail"]["args"] = json!(["changed unused variant"]);
    write(
        &repo,
        "artifactize.json",
        json!({"name":"test","fingerprint":fingerprint("variants"),"evals":[declaration]}),
    );
    assert_eq!(
        fixture.command(&repo, &["verify", "--all"], 0)["requests"][0]["executionId"],
        original["requests"][0]["executionId"]
    );
    assert_eq!(fixture.count("executions"), 2);
}

#[test]
fn canonical_definition_hash_includes_all_agent_settings_but_not_names() {
    use artifactize::{cache::eval_definition_hash, config::EvalDeclaration};
    let first: EvalDeclaration = serde_json::from_str(r#"{"id":"one","title":"First","profile":{"kind":"agent","backend":"anthropic","model":"model","reasoning":"high","maxToolCalls":3,"maxTokens":10,"timeoutMs":1000},"payload":{"instruction":"Review.","nested":{"b":2,"a":1}},"passSchema":{"type":"object","properties":{"b":{"type":"number"},"a":{"type":"string"}}}}"#).unwrap();
    let second: EvalDeclaration = serde_json::from_str(r#"{"title":"Second","id":"two","payload":{"nested":{"a":1,"b":2},"instruction":"Review."},"passSchema":{"properties":{"a":{"type":"string"},"b":{"type":"number"}},"type":"object"},"profile":{"maxTokens":10,"maxToolCalls":3,"timeoutMs":1000,"reasoning":"high","model":"model","backend":"anthropic","kind":"agent"}}"#).unwrap();
    let hash = eval_definition_hash(&first);
    assert_eq!(hash, eval_definition_hash(&second));
    for (key, value) in [
        ("backend", json!("openai")),
        ("model", json!("other")),
        ("reasoning", json!("low")),
        ("maxToolCalls", json!(4)),
        ("maxTokens", json!(11)),
        ("timeoutMs", json!(2000)),
    ] {
        let mut changed = serde_json::to_value(&first).unwrap();
        changed["profile"][key] = value;
        changed.as_object_mut().unwrap().remove("failSchema");
        assert_ne!(
            hash,
            eval_definition_hash(&serde_json::from_value(changed).unwrap()),
            "{key}"
        );
    }
    let mut human = first.clone();
    human.profile = artifactize::config::Profile::Human {};
    assert_ne!(hash, eval_definition_hash(&human));
}

#[test]
fn concurrent_claims_dedupe_each_definition_without_blocking_another() {
    let fixture = Fixture::new();
    let script = format!("{WAIT_SCRIPT}; [ \"$3\" = pass ]");
    let mut repos = Vec::new();
    for (name, verdict) in [
        ("pass-one", "pass"),
        ("pass-two", "pass"),
        ("fail-one", "fail"),
        ("fail-two", "fail"),
    ] {
        let repo = fixture.shared_repo(name, &script);
        let path = repo.join("artifactize.json");
        let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        declaration["evals"][0]["profile"]["args"]
            .as_array_mut()
            .unwrap()
            .push(json!(verdict));
        fs::write(path, declaration.to_string()).unwrap();
        repos.push(repo);
    }
    let children: Vec<_> = repos.iter().map(|repo| fixture.spawn(repo, &[])).collect();
    wait_until(|| fixture.starts() == 2);
    wait_until(|| {
        let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
        db.query_row::<u32, _, _>(
            "SELECT count(*) FROM requests WHERE status='QUEUED' AND execution_id IS NOT NULL",
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
    assert_eq!(fixture.count("cache_entries"), 2);
}

#[test]
fn gc_and_removal_protect_only_the_matching_definition() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo", json!({"name":"test","fingerprint":fingerprint("shared"),"evals":[eval("pass","exit 0"),eval("fail","exit 1")]}));
    let run = fixture.command(&repo, &["verify", "--all"], 1);
    let hash = run["requests"][0]["evalDefHash"].as_str().unwrap();
    let db = Connection::open(fixture.state.join("state.sqlite")).unwrap();
    db.execute("INSERT INTO executions(id,fingerprint,eval_def_hash,owner_pid,owner_start_time,status,data) VALUES ('active','shared',?,1,1,'WAITING_HUMAN','{}')", [hash]).unwrap();
    let other = run["requests"][1]["evalDefHash"].as_str().unwrap();
    assert_eq!(
        fixture.command(&repo, &["cache", "rm", "shared", other], 0),
        json!({"removed":true})
    );
    assert!(
        fixture.command(&repo, &["cache", "rm", "shared", hash], 2)["error"]
            .as_str()
            .unwrap()
            .contains("in use")
    );
    fixture.command(&repo, &["verify", "--eval", "test/fail"], 1);
    db.execute("UPDATE cache_entries SET bytes=16777217", [])
        .unwrap();
    fixture.publish("other");
    assert_eq!(fixture.count("cache_entries"), 2);
    assert_eq!(
        fixture.command(&repo, &["cache", "show", "shared", other], 4),
        Value::Null
    );
    assert!(
        fixture
            .command(&repo, &["cache", "show", "shared", hash], 0)
            .is_object()
    );
}
