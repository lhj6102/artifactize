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
        self.repo(name, json!({"name":name,"stale":identity("concurrent"),"evals":[{
            "id":"check","title":"Review","profile":{"kind":"runtime","command":"/bin/sh",
                "args":["-c",script,"sh",self.root.path().join("starts"),self.root.path().join("release")],"timeoutMs":10000},
            "payload":{"instruction":"Review."}
        }]}))
    }

    fn release(&self) {
        fs::write(self.root.path().join("release"), "").unwrap();
    }

    fn starts(&self) -> usize {
        fs::read_to_string(self.root.path().join("starts"))
            .unwrap()
            .lines()
            .count()
    }

    fn count(&self, table: &str) -> u32 {
        Connection::open(self.state.join("state.sqlite"))
            .unwrap()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn entries(&self) -> Vec<(String, String, i64, String, String)> {
        let db = Connection::open(self.state.join("state.sqlite")).unwrap();
        let mut statement = db.prepare("SELECT c.identity,c.execution_id,c.bytes,c.last_used,e.data FROM cache_entries c JOIN executions e ON e.id=c.execution_id ORDER BY c.identity").unwrap();
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

fn identity(key: &str) -> Value {
    json!({"kind":"identity","script":{"command":"/bin/echo","args":[key]}})
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
    let source = fixture.repo("source", json!({"name":"original","stale":identity("shared:red"),"evals":[eval("first","printf original; exit 7")]}));
    let original = fixture.command(&source, &["verify", "--all"], 1);
    let original_request = &original["requests"][0];
    assert!(original_request["child"]["pid"].is_number());
    assert!(original_request["usage"].is_null());
    assert_eq!(fixture.count("executions"), 1);
    assert_eq!(fixture.count("cache_entries"), 1);
    fs::remove_dir_all(&source).unwrap();
    let target = fixture.repo("target", json!({"name":"root","basis":true}));
    let mut different = eval("second", "touch must-not-run; exit 0");
    different["passSchema"] =
        json!({"properties":{"different":{"type":"string"}},"required":["different"]});
    write(
        &target,
        "dependency/artifactize.json",
        json!({"name":"dependency","stale":identity("shared:red"),"evals":[different]}),
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
    assert_ne!(hit["requestedProfile"], hit["profile"]);
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
fn green_is_reused_across_evals_profiles_and_unselected_sibling_obligations() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo", json!({"name":"test","stale":identity("shared:green"),"evals":[eval("first","printf first; touch first-ran"),eval("second","touch must-not-run; exit 9")]}));
    let run = fixture.command(&repo, &["verify", "--all"], 0);
    let first = &run["requests"][0];
    let second = &run["requests"][1];
    assert_eq!(first["result"], second["result"]);
    assert_eq!(first["executionId"], second["executionId"]);
    assert_eq!(second["profile"], first["profile"]);
    assert!(second["child"].is_null());
    assert!(!repo.join("must-not-run").exists());
    assert_eq!(fixture.count("executions"), 1);
    let selected = fixture.command(&repo, &["verify", "--eval", "test/second"], 0);
    assert_eq!(selected["requests"].as_array().unwrap().len(), 1);
    assert_eq!(selected["validation"]["obligations"], json!([]));
    assert_eq!(fixture.count("executions"), 1);
    let other = fixture.repo("other",json!({"name":"other","stale":identity("shared:green"),"evals":[{"id":"human","title":"Changed kind","profile":{"kind":"human"},"payload":{"instruction":"Other criteria"}}]}));
    let hit = fixture.command(&other, &["verify", "--all"], 0);
    assert_eq!(hit["requests"][0]["profile"], first["profile"]);
    assert_eq!(hit["requests"][0]["requestedProfile"]["kind"], "human");
    assert_eq!(fixture.count("executions"), 1);
    let agent_repo = fixture.repo("agent", json!({"name":"agent","stale":identity("shared:green"),"evals":[{"id":"review","title":"Agent","profile":{"kind":"agent","backend":"chatgpt","model":"not-called"},"payload":{"instruction":"Review."}}]}));
    let agent_hit = fixture.command(&agent_repo, &["verify", "--all"], 0);
    assert_eq!(
        agent_hit["requests"][0]["requestedProfile"]["kind"],
        "agent"
    );
    assert_eq!(agent_hit["requests"][0]["profile"], first["profile"]);
    assert_eq!(
        agent_hit["requests"][0]["executionId"],
        first["executionId"]
    );
    assert_eq!(agent_hit["requests"][0]["usage"], first["usage"]);
    assert_eq!(agent_hit["requests"][0]["toolCalls"], first["toolCalls"]);
    assert_eq!(fixture.count("executions"), 1);
}

#[test]
fn no_identity_executes_each_time_and_never_reads_or_publishes_cache() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","stale":identity("cached"),"evals":[eval("check","exit 0")]}),
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
        assert!(run["requests"][0]["identity"].is_null());
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
    let repo = fixture.repo("repo",json!({"name":"test","stale":identity("retryable"),"evals":[eval("check","kill -TERM $$")]}));
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
        json!({"name":"test","stale":identity("retryable"),"evals":[eval("check","exit 0")]}),
    );
    fixture.command(&repo, &["verify", "--all"], 0);
    assert_eq!(fixture.count("cache_entries"), 1);
}

#[test]
fn force_executes_without_reading_or_replacing_an_entry_but_dependencies_reuse() {
    let fixture = Fixture::new();
    let repo = fixture.repo(
        "repo",
        json!({"name":"test","stale":identity("forced"),"evals":[eval("check","exit 7")]}),
    );
    let original = fixture.command(&repo, &["verify", "--all"], 1);
    let before = fixture.entries();
    write(
        &repo,
        "artifactize.json",
        json!({"name":"test","stale":identity("forced"),"evals":[eval("check","echo force >> starts")]}),
    );
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
        json!({"name":"dep","stale":identity("dependency"),"evals":[eval("check","exit 0")]}),
    );
    fixture.command(&dependency, &["verify", "--all"], 0);
    write(
        &repo,
        "dependency/artifactize.json",
        json!({"name":"dep","stale":identity("dependency"),"evals":[eval("check","touch must-not-run; exit 1")]}),
    );
    let forced = fixture.command(&repo, &["verify", "test", "--recursive", "--force"], 0);
    assert_eq!(request(&forced, "test/check")["force"], true);
    assert_eq!(request(&forced, "dep/check")["force"], false);
    assert!(request(&forced, "dep/check")["child"].is_null());
    assert!(!repo.join("dependency/must-not-run").exists());

    let absent = fixture.repo("absent",json!({"name":"absent","stale":identity("never-published"),"evals":[eval("check","exit 0")]}));
    let entries = fixture.entries();
    fixture.command(&absent, &["verify", "--all", "--force"], 0);
    assert_eq!(fixture.entries(), entries);
}

#[test]
fn status_uses_current_identity_and_only_prepares_the_selected_closure() {
    let fixture = Fixture::new();
    let repo = fixture.repo("repo",json!({"name":"test","stale":{"kind":"identity","script":{"command":"/bin/cat","args":["key"]}},"evals":[eval("check","touch executed")]}));
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
            .contains("Identity script")
    );
    assert!(!repo.join("executed").exists());
    write(
        &repo,
        "selected/artifactize.json",
        json!({"name":"selected","stale":identity("isolated"),"evals":[eval("check","touch must-not-run")]}),
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
    wait_until(|| fixture.root.path().join("starts").exists());
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
    wait_until(|| fixture.root.path().join("starts").exists());
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
    wait_until(|| fixture.root.path().join("starts").exists());
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
    wait_until(|| fixture.root.path().join("starts").exists());
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
    wait_until(|| fixture.root.path().join("starts").exists());
    let forced = finish(fixture.spawn(&target, &["--force"]), 0);
    let execution = fixture.execution(forced["requests"][0]["executionId"].as_str().unwrap());
    assert!(execution["identity"].is_null());
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
        json!({"name":"test","stale":identity("one-start"),"evals":[
            eval("first", "echo first >> starts; sleep 0.1"),
            eval("second", "touch must-not-run"),
            eval("third", "touch must-not-run")
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
        wait_until(|| fixture.root.path().join("starts").exists());
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
fn an_identity_waiter_occupies_a_job_slot_without_consuming_execution_budget() {
    let fixture = Fixture::new();
    let source = fixture.shared_repo("source", WAIT_SCRIPT);
    let target = fixture.shared_repo("a-target", "touch must-not-run");
    write(
        &target,
        "independent/artifactize.json",
        json!({"name":"z-independent","evals":[eval("check", "touch ran")]}),
    );
    let owner = fixture.spawn(&source, &[]);
    wait_until(|| fixture.root.path().join("starts").exists());
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
