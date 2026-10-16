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
            ready_tx.send(()).unwrap();
            if commit_rx.recv().is_ok() {
                transaction.commit().unwrap();
            }
        })),
    };
    ready_rx.recv().unwrap();
    assert_empty_reads(&state, &repo).await;
    initializer.finish().unwrap();
    assert_empty_reads(&state, &repo).await;
    let db = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        STATE_SCHEMA_VERSION
    );
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
            repo.join("index.artf"),
            format!(r#"{{"name":"{name}","basis":true}}"#),
        )
        .unwrap();
    }
    root.track(&state, &first);
    root.track(&state, &second);
    let mut saved = Vec::new();
    for explicit in [false, true] {
        for repo in [&first, &second] {
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
            saved.push(run);
        }
    }
    let db = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        db.query_row::<u32, _, _>("SELECT count(DISTINCT repo) FROM runs", [], |r| r.get(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row::<u32, _, _>("SELECT count(*) FROM runs", [], |r| r.get(0))
            .unwrap(),
        4
    );
    fs::remove_dir_all(first).unwrap();
    fs::remove_dir_all(second).unwrap();
    for run in saved {
        let read = read_run(&state, run["id"].as_str().unwrap()).await.unwrap();
        assert_eq!(
            artifactize_core::query::run_output(&read, time::OffsetDateTime::now_utc()),
            run
        );
    }
    assert!(!root.path().join("unused").exists());
}

#[tokio::test]
async fn schema_allows_only_one_active_execution_per_key() {
    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let _receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    let db = Connection::open(state.join(DATABASE)).unwrap();
    let insert = "INSERT INTO executions(id,key,eval_def_hash,owner_pid,owner_start_time,status,data) VALUES (?, ?, 'hash', 1, 1, ?, '{}')";
    db.execute(insert, ["first", "shared", "RUNNING"]).unwrap();
    assert!(
        db.execute(insert, ["second", "shared", "WAITING_HUMAN"])
            .is_err()
    );
    db.execute(insert, ["different", "other", "WAITING_HUMAN"])
        .unwrap();
    db.execute(insert, ["completed", "shared", "GREEN"])
        .unwrap();
    db.execute("UPDATE executions SET status='ERROR' WHERE id='first'", [])
        .unwrap();
    db.execute(insert, ["second", "shared", "RUNNING"]).unwrap();
    db.execute(insert, [Some("uncached"), None, Some("RUNNING")])
        .unwrap();
    db.execute(insert, [Some("forced"), None, Some("RUNNING")])
        .unwrap();
}

#[test]
fn state_and_output_reject_reviewed_paths_and_symlink_ancestors() {
    let root = owned_run::Root::from(support::os::tempdir());
    let repo = root.path().join("repo");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(repo.join("index.artf"), r#"{"name":"basis","basis":true}"#)
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
    fs::write(&protected, "unchanged").unwrap();
    for suffix in ["", "-wal", "-shm"] {
        let file = state.join(format!("{DATABASE}{suffix}"));
        if support::os::symlink_file(&protected, &file).is_none() {
            continue;
        }
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
        assert_eq!(fs::read_to_string(&protected).unwrap(), "unchanged");
        fs::remove_file(file).unwrap();
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
        .create_run(&view.run, &view.requests)
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
    // Parsing is the observable rejection boundary. No write was attempted, so an
    // unchanged database would not independently demonstrate atomic write behavior.
    let db = Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        STATE_SCHEMA_VERSION
    );
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
