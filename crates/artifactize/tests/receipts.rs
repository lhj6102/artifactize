use std::{fs, process::Command, sync::mpsc, thread};

use artifactize_core::store::{
    DATABASE, OwnedReceipts, Receipts, STATE_SCHEMA_VERSION, read_keyed_executions,
    read_latest_requests, read_request, read_requests, read_run, read_runs,
};
use rusqlite::Connection;

#[path = "support/run.rs"]
mod owned_run;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

use support::os::link_dir;

/// Own the initializer across assertion failure or cancellation of its async test.
struct Initializer {
    commit: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Initializer {
    fn finish(&mut self) -> thread::Result<()> {
        if let Some(commit) = self.commit.take() {
            // The thread may already have failed; joining still owns that failure.
            let _ = commit.send(());
        }
        self.thread.take().map_or(Ok(()), |thread| thread.join())
    }
}
impl Drop for Initializer {
    fn drop(&mut self) {
        let joined = self.finish();
        if !thread::panicking() {
            joined.expect("schema initializer releases SQLite before temporary state");
        }
    }
}

#[tokio::test]
async fn missing_read_is_inert_and_future_schemas_are_not_modified() {
    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    assert!(read_run(&state, "missing").await.is_err());
    assert!(!state.exists());
    let receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    drop(receipts);
    // Future schema metadata is an explicit input; observations have no write capability.
    Connection::open(state.join(DATABASE))
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let db = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let before = db
        .query_row::<i64, _, _>("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert!(
        Receipts::open(&state, &repo)
            .await
            .err()
            .unwrap()
            .contains("Unsupported state schema")
    );
    assert!(
        read_run(&state, "missing")
            .await
            .unwrap_err()
            .contains("Unsupported state schema")
    );
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        99
    );
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
            .unwrap(),
        before
    );
}

async fn assert_empty_reads(state: &std::path::Path, repo: &std::path::Path) {
    assert!(read_requests(state, None).await.unwrap().is_empty());
    assert!(read_runs(state, None, 10, 0).await.unwrap().is_empty());
    assert!(read_latest_requests(state, repo).await.unwrap().is_empty());
    assert!(
        read_keyed_executions(
            state,
            &[
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    .parse()
                    .unwrap()
            ]
        )
        .await
        .unwrap()
        .is_empty()
    );
    assert!(
        read_run(state, "missing")
            .await
            .unwrap_err()
            .contains("Run not found")
    );
    assert!(
        read_request(state, "missing")
            .await
            .unwrap_err()
            .contains("Review request not found")
    );
    for args in [["request", "list"], ["run", "list"], ["cache", "list"]] {
        let output = Command::new(support::os::cli())
            .arg("--state-dir")
            .arg(state)
            .args(args)
            .arg("--json")
            .record_output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
            serde_json::json!([])
        );
    }
}

#[tokio::test]
async fn readers_observe_empty_state_until_schema_commits() {
    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    let template = root.path().join("template");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let receipts = OwnedReceipts::open(&template, &repo).await.unwrap();
    let db = Connection::open_with_flags(
        template.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let schema = db
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    drop(db);
    drop(receipts);
    fs::create_dir(&state).unwrap();
    drop(Connection::open(state.join(DATABASE)).unwrap());
    assert_empty_reads(&state, &repo).await;

    let (ready_tx, ready_rx) = mpsc::channel();
    let (commit_tx, commit_rx) = mpsc::channel();
    let path = state.join(DATABASE);
    let id = "run-initialized";
    let repo_text = support::os::path_text(&repo);
    let saved = serde_json::json!({
        "id": id, "repoPath": repo_text, "stateDir": support::os::path_text(&state),
        "status":"GREEN", "createdAt":"2026-01-01T00:00:00Z", "completedAt":"2026-01-01T00:00:01Z",
        "selection":{"kind":"all"}, "validation":null, "definitions":null
    })
    .to_string();
    let mut initializer = Initializer {
        commit: Some(commit_tx),
        thread: Some(thread::spawn(move || {
            let mut db = Connection::open(path).unwrap();
            db.pragma_update(None, "journal_mode", "WAL").unwrap();
            let transaction = db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                .unwrap();
            for sql in schema {
                transaction.execute_batch(&sql).unwrap();
            }
            transaction
                .pragma_update(None, "user_version", STATE_SCHEMA_VERSION)
                .unwrap();
            transaction
                .execute(
                    "INSERT INTO runs(id,repo,status,data) VALUES (?,?,?,?)",
                    rusqlite::params![id, repo_text, "GREEN", saved],
                )
                .unwrap();
            ready_tx.send(()).unwrap();
            if commit_rx.recv().is_ok() {
                transaction.commit().unwrap();
            }
        })),
    };
    ready_rx.recv().unwrap();
    assert_empty_reads(&state, &repo).await;
    assert!(
        read_run(&state, id)
            .await
            .unwrap_err()
            .contains("Run not found")
    );
    initializer.finish().unwrap();
    let saved = read_run(&state, id).await.unwrap();
    assert_eq!(saved.run.id.as_str(), id);
    assert_eq!(
        saved.run.status(),
        artifactize_core::types::RunStatus::Green
    );
    assert!(saved.requests.is_empty());
}

#[tokio::test]
async fn repositories_share_one_state_database() {
    let root = owned_run::Root::from(support::os::tempdir());
    // The long, plain paths artifactize records, whatever form TEMP takes.
    let base = support::os::canonical(root.path());
    let first = base.join("first");
    let second = base.join("second");
    let state = base.join("state");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    for (repo, name) in [(&first, "first"), (&second, "second")] {
        support::declaration::write(
            repo,
            repo.join("index.artf"),
            format!(r#"{{"name":"{name}","basis":true}}"#),
        )
        .unwrap();
    }
    root.track(&state, &first);
    root.track(&state, &second);
    let mut saved = Vec::new();
    for explicit in [false, true] {
        for (repo, name) in [(&first, "first"), (&second, "second")] {
            let mut command = Command::new(support::os::cli());
            command
                .env("ARTIFACTIZE_STATE_HOME", &state)
                .arg("--repo")
                .arg(repo);
            if explicit {
                command
                    .env("ARTIFACTIZE_STATE_HOME", root.path().join("unused"))
                    .arg("--state-dir")
                    .arg(&state);
            }
            let output = command
                .args(["verify", "--wait", "--all", "--json"])
                .record_output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let run: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(run["repoPath"], support::os::path_text(repo));
            assert_eq!(run["stateDir"], support::os::path_text(&state));
            assert!(
                state
                    .join("runs")
                    .join(run["id"].as_str().unwrap())
                    .is_dir()
            );
            let id = run["id"].as_str().unwrap().to_owned();
            saved.push((id, repo.clone(), name));
        }
    }
    let listed = read_runs(&state, None, 10, 0).await.unwrap();
    let actual = listed
        .iter()
        .map(|run| (run.id.as_str(), run.repo_path.as_path()))
        .collect::<std::collections::BTreeSet<_>>();
    let expected = saved
        .iter()
        .map(|(id, repo, _)| (id.as_str(), repo.as_path()))
        .collect();
    assert_eq!(actual, expected);
    fs::remove_dir_all(&first).unwrap();
    fs::remove_dir_all(&second).unwrap();
    for (id, repo, name) in saved {
        let read = read_run(&state, &id).await.unwrap();
        assert_eq!(read.run.id.as_str(), id);
        assert_eq!(read.run.repo_path, repo);
        assert_eq!(read.run.state_dir, state);
        assert_eq!(read.run.status(), artifactize_core::types::RunStatus::Green);
        assert!(read.run.completed_at().is_some());
        assert!(matches!(
            read.run.selection,
            artifactize_core::project::selection::Selection::All
        ));
        assert_eq!(
            read.run.validation.snapshot().unwrap().satisfied.value(),
            Some(&true)
        );
        assert!(read.requests.is_empty());
        assert!(read.unreadable.is_empty());
        let graph = read.run.definitions.graph().unwrap();
        assert_eq!(graph.artifacts().count(), 1);
        assert_eq!(graph.artifact(name).unwrap().basis.value(), Some(&true));
        assert!(graph.evals().is_empty());
    }
    assert!(!root.path().join("unused").exists());
}

#[tokio::test]
async fn concurrent_runs_share_a_claim_without_preventing_independent_executions() {
    use artifactize_core::types::{RequestStatus, RunId};
    use serde_json::{Value, json};
    use std::time::Duration;

    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    root.track(&state, &repo);
    let release = owned_run::FileGate::new(repo.join("release"));
    support::declaration::write(&repo, repo.join("index.artf"), json!({
        "name":"app", "fingerprint":{"files":["input"]},
        "evals":[{"id":"check", "title":"Check", "payload":{"instruction":"Check."},
            "profile":{"kind":"runtime","command":support::os::steps(),"args":["wait","release"]}}]
    }).to_string()).unwrap();
    fs::write(repo.join("input"), "stable").unwrap();
    let submit = |args: &[&str]| -> RunId {
        let output = support::os::run(
            Command::new(support::os::cli())
                .arg("--repo")
                .arg(&repo)
                .arg("--state-dir")
                .arg(&state)
                .env("ARTIFACTIZE_REMOTE", "off")
                .args(["verify", "--all", "--json"])
                .args(args),
        );
        assert!(output.status.success(), "{output:?}");
        let accepted: Value = serde_json::from_slice(&output.stdout).unwrap();
        accepted["run"].as_str().unwrap().parse().unwrap()
    };
    let pending = async |id: &RunId, status: RequestStatus| {
        owned_run::subscribed(&state, async |changes| {
            const WATCHDOG: Duration = Duration::from_secs(30);
            tokio::time::timeout(support::os::patience(WATCHDOG), async {
                loop {
                    let view = read_run(&state, id).await.unwrap();
                    if let Some(request) = view.requests.first()
                        && request.status() == status
                        && (request.execution_id.is_some() || request.child.is_some())
                    {
                        return (request.key.clone(), request.execution_id.clone());
                    }
                    assert!(view.run.status().is_open(), "{view:?}");
                    changes.next_reconciled().await;
                }
            })
            .await
            .unwrap_or_else(|error| panic!("Run {id} did not reach {status}: {error}"))
        })
        .await
    };
    let first = submit(&[]);
    let main = pending(&first, RequestStatus::Running).await;
    assert!(main.0.is_some() && main.1.is_some());
    let second = submit(&[]);
    assert_eq!(pending(&second, RequestStatus::Queued).await, main);
    let forced = submit(&["--force"]);
    // Forced work has its own child before its execution ID is saved with the outcome.
    assert!(pending(&forced, RequestStatus::Running).await.1.is_none());
    release.release().unwrap();
    for (id, relation) in [(first, 1), (second, 0), (forced, 1)] {
        let completed = owned_run::terminal(&state, &id).await;
        assert_eq!(completed.requests[0].status(), RequestStatus::Green);
        assert_eq!(completed.run.executions_started, relation);
        if completed.run.force {
            assert_ne!(completed.requests[0].execution_id, main.1);
            assert!(completed.requests[0].execution_id.is_some());
        }
    }
}

#[test]
fn state_and_output_reject_reviewed_paths_and_symlink_ancestors() {
    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(
        &(repo),
        repo.join("index.artf"),
        r#"{"name":"basis","basis":true}"#,
    )
    .unwrap();
    link_dir(&repo, &root.path().join("alias"));
    let inner = repo.join("inner");
    fs::create_dir(&inner).unwrap();
    link_dir(&inner, &root.path().join("nested-alias"));
    for state in support::os::reviewed_state_paths(root.path(), &repo) {
        for explicit in [false, true] {
            let mut command = Command::new(support::os::cli());
            command
                .env("ARTIFACTIZE_STATE_HOME", &state)
                .arg("--repo")
                .arg(&repo);
            if explicit {
                command.arg("--state-dir").arg(&state);
            }
            let output = command
                .args(["verify", "--wait", "--all", "--json"])
                .record_output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2));
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("outside the reviewed repository")
            );
        }
    }
    assert!(!repo.join("state").exists());
    let state = root.path().join("state");
    root.track(&state, &repo);
    fs::create_dir(&state).unwrap();
    link_dir(&repo, &state.join("runs"));
    let output = Command::new(support::os::cli())
        .env("ARTIFACTIZE_STATE_HOME", root.path().join("home"))
        .arg("--repo")
        .arg(&repo)
        .arg("--state-dir")
        .arg(&state)
        .args(["verify", "--wait", "--all"])
        .record_output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside the reviewed repository"));
    support::os::remove_link_dir(&state.join("runs"));
    let output = Command::new(support::os::cli())
        .env("ARTIFACTIZE_STATE_HOME", repo.join("home"))
        .arg("--repo")
        .arg(&repo)
        .arg("--state-dir")
        .arg(&state)
        .args(["verify", "--wait", "--all"])
        .record_output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!repo.join("home").exists());
    assert!(state.join(DATABASE).is_file());
}

#[tokio::test]
async fn sqlite_files_cannot_redirect_writes_through_links() {
    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    fs::create_dir(&state).unwrap();
    let protected = repo.join("must-not-write");
    fs::write(support::declaration::source_path(&protected), "unchanged").unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let file = state.join(format!("{DATABASE}{suffix}"));
        support::os::file_link(&protected, &file);
        assert!(
            Receipts::open(&state, &repo)
                .await
                .err()
                .unwrap()
                .contains("regular files")
        );
        assert!(
            read_run(&state, "missing")
                .await
                .unwrap_err()
                .contains("regular files")
        );
        assert_eq!(
            support::declaration::read_text(&protected).unwrap(),
            "unchanged"
        );
        support::os::remove_file_link(&file);
    }
}

#[tokio::test]
async fn schema_five_profiles_survive_typed_reads_and_invalid_states_fail_at_parse_boundary() {
    use artifactize_core::store::RunView;
    use serde_json::json;
    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    // Literal schema-5 fixture: optional profile fields were serialized as null.
    let view: RunView = serde_json::from_value(json!({
        "id":"run-old",
        "repoPath":support::os::path_text(&repo),
        "stateDir":support::os::path_text(&state),
        "status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z",
        "completedAt":null,
        "selection":{"kind":"all"},
        "profile":null,
        "validation":null,
        "error":null,
        "requests":[
            {
                "id":"run-old-1",
                "runId":"run-old",
                "evalId":"app/check",
                "target":"app",
                "title":"Check",
                "profile":{
                    "kind":"agent",
                    "backend":"openai",
                    "model":"fixture",
                    "reasoning":null,
                    "timeoutMs":null,
                    "maxToolCalls":null,
                    "maxTokens":null,
                },
                "requestedProfile":{
                    "kind":"agent",
                    "backend":"openai",
                    "model":"fixture",
                    "reasoning":null,
                    "timeoutMs":null,
                    "maxToolCalls":null,
                    "maxTokens":null,
                },
                "evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                "executionId":null,
                "provenance":null,
                "usage":null,
                "payload":{},
                "references":{},
                "deps":[],
                "status":"QUEUED",
                "createdAt":"2026-01-01T00:00:00Z",
                "startedAt":null,
                "completedAt":null,
                "cwd":support::os::path_text(&repo),
                "runDir":null,
                "argv":null,
                "child":null,
                "result":null,
                "error":null,
                "errorCode":null,
                "blockedReason":null,
            },
        ],
    }))
    .unwrap();
    receipts
        .create_run(
            &view.run,
            &view
                .requests
                .iter()
                .map(|request| artifactize_core::store::Request::clone(request))
                .collect::<Vec<_>>(),
        )
        .await
        .unwrap();
    let saved = serde_json::to_value(read_run(&state, "run-old").await.unwrap()).unwrap();
    assert_eq!(
        saved["requests"][0]["profile"],
        json!({
            "kind":"agent",
            "backend":"openai",
            "model":"fixture",
            "reasoning":null,
            "timeoutMs":null,
            "maxToolCalls":null,
            "maxTokens":null,
        })
    );
    let good = serde_json::to_value(&view.requests[0]).unwrap();
    for (status, result, error) in [
        ("GREEN", serde_json::Value::Null, serde_json::Value::Null),
        ("GREEN", json!({"verdict":"GREEN"}), json!("contradiction")),
        ("ERROR", json!({"verdict":"GREEN"}), json!("contradiction")),
    ] {
        let mut bad = good.clone();
        bad["status"] = json!(status);
        bad["result"] = result;
        bad["error"] = error;
        bad["completedAt"] = json!("2026-01-01T00:00:01Z");
        assert!(serde_json::from_value::<artifactize_core::store::Request>(bad).is_err());
    }
    let mut wire = serde_json::to_value(&view.run).unwrap();
    wire["status"] = json!("GREEN");
    assert!(serde_json::from_value::<artifactize_core::store::Run>(wire).is_err());
    // Valid older writes still read unchanged; contradictory lifecycle fields are
    // corruption and fail only that record at the parse boundary.
    Connection::open(state.join(DATABASE))
        .unwrap()
        .execute(
            "UPDATE requests SET status='GREEN',data=json_set(data,'$.status','GREEN')",
            [],
        )
        .unwrap();
    let saved = read_run(&state, "run-old").await.unwrap();
    assert!(saved.requests.is_empty());
    assert_eq!(saved.unreadable[0].id, "run-old-1");
    assert!(saved.unreadable[0].reason.contains("matching result"));
}
