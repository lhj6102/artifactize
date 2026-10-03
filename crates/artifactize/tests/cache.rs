use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
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
