use std::{fs, path::Path, process::Command, sync::mpsc, thread};

use artifactize::store::{
    DATABASE, Receipts, STATE_SCHEMA_VERSION, read_keyed_executions, read_latest_requests,
    read_request, read_requests, read_run, read_runs,
};
use rusqlite::Connection;

mod support;

/// A link to a directory: a symlink on Unix, and on Windows a junction, which needs no
/// privilege and which artifactize refuses just the same.
fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    support::os::symlink_dir(target, link).unwrap();
    #[cfg(windows)]
    support::os::junction(target, link);
}

#[tokio::test]
async fn missing_read_is_inert_and_future_schemas_are_not_modified() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    assert!(read_run(&state, "missing").await.is_err());
    assert!(!state.exists());
    let receipts = Receipts::open(&state, &repo).await.unwrap();
    drop(receipts);
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.pragma_update(None, "user_version", 99).unwrap();
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
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--state-dir")
            .arg(state)
            .args(args)
            .arg("--json")
            .output()
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
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let template = root.path().join("template");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let receipts = Receipts::open(&template, &repo).await.unwrap();
    let db = Connection::open(template.join(DATABASE)).unwrap();
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
    let initializer = thread::spawn(move || {
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
    });
    ready_rx.recv().unwrap();
    assert_empty_reads(&state, &repo).await;
    commit_tx.send(()).unwrap();
    initializer.join().unwrap();
    assert_empty_reads(&state, &repo).await;
    let db = Connection::open(state.join(DATABASE)).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        STATE_SCHEMA_VERSION
    );
}

#[tokio::test]
async fn repositories_share_one_state_database() {
    let root = tempfile::tempdir().unwrap();
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
    let mut saved = Vec::new();
    for explicit in [false, true] {
        for repo in [&first, &second] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
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
                .args(["verify", "--all", "--json"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let run: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(run["repoPath"], repo.to_string_lossy().as_ref());
            assert_eq!(run["stateDir"], state.to_string_lossy().as_ref());
            assert!(
                state
                    .join("runs")
                    .join(run["id"].as_str().unwrap())
                    .is_dir()
            );
            saved.push(run);
        }
    }
    let db = Connection::open(state.join(DATABASE)).unwrap();
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
            artifactize::query::run_output(&read, time::OffsetDateTime::now_utc()),
            run
        );
    }
    assert!(!root.path().join("unused").exists());
}

#[tokio::test]
async fn schema_allows_only_one_active_execution_per_key() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let _receipts = Receipts::open(&state, &repo).await.unwrap();
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
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    fs::create_dir(&repo).unwrap();
    support::declaration::write(repo.join("index.artf"), r#"{"name":"basis","basis":true}"#)
        .unwrap();
    link_dir(&repo, &root.path().join("alias"));
    let inner = repo.join("inner");
    fs::create_dir(&inner).unwrap();
    link_dir(&inner, &root.path().join("nested-alias"));
    for state in [
        repo.join("state"),
        // Windows opens a name in any case: this is the repository too.
        #[cfg(windows)]
        support::os::other_case(&repo).join("state"),
        root.path().join("alias/state"),
        // Windows drops `..` from a path before following any link, so there this names
        // root/state, outside the repository.
        #[cfg(unix)]
        root.path().join("nested-alias/../state"),
    ] {
        for explicit in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
            command
                .env("ARTIFACTIZE_STATE_HOME", &state)
                .arg("--repo")
                .arg(&repo);
            if explicit {
                command.arg("--state-dir").arg(&state);
            }
            let output = command
                .args(["verify", "--all", "--json"])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2));
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("outside the reviewed repository")
            );
        }
    }
    assert!(!repo.join("state").exists());
    let state = root.path().join("state");
    fs::create_dir(&state).unwrap();
    link_dir(&repo, &state.join("runs"));
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("ARTIFACTIZE_STATE_HOME", root.path().join("home"))
        .arg("--repo")
        .arg(&repo)
        .arg("--state-dir")
        .arg(&state)
        .args(["verify", "--all"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside the reviewed repository"));
    // Windows removes a directory link as a directory.
    #[cfg(unix)]
    fs::remove_file(state.join("runs")).unwrap();
    #[cfg(windows)]
    fs::remove_dir(state.join("runs")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .env("ARTIFACTIZE_STATE_HOME", repo.join("home"))
        .arg("--repo")
        .arg(&repo)
        .arg("--state-dir")
        .arg(&state)
        .args(["verify", "--all"])
        .output()
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
    let root = tempfile::tempdir().unwrap();
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
async fn schema_five_profiles_and_statuses_survive_typed_reads_and_invalid_writes_are_atomic() {
    use artifactize::{
        store::RunView,
        types::{RequestStatus, RunStatus},
    };
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let receipts = Receipts::open(&state, &repo).await.unwrap();
    // Literal schema-5 fixture: optional profile fields were serialized as null.
    let view: RunView = serde_json::from_value(json!({
        "id":"run-old","repoPath":repo,"stateDir":state,"status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z","completedAt":null,"selection":{"kind":"all"},"profile":null,
        "validation":null,"error":null,"requests":[{
            "id":"run-old-1","runId":"run-old","evalId":"app/check","target":"app","title":"Check",
            "profile":{"kind":"agent","backend":"openai","model":"fixture","reasoning":null,"timeoutMs":null,"maxToolCalls":null,"maxTokens":null},
            "requestedProfile":{"kind":"agent","backend":"openai","model":"fixture","reasoning":null,"timeoutMs":null,"maxToolCalls":null,"maxTokens":null},
            "evalDefHash":"fixture","executionId":null,"provenance":null,"usage":null,
            "payload":{},"references":{},"deps":[],"status":"QUEUED","createdAt":"2026-01-01T00:00:00Z",
            "startedAt":null,"completedAt":null,"cwd":repo,"runDir":null,"argv":null,"child":null,"result":null,"error":null,"errorCode":null,"blockedReason":null
        }]
    }))
    .unwrap();
    receipts
        .create_run(&view.run, &view.requests)
        .await
        .unwrap();
    let before = serde_json::to_value(read_run(&state, "run-old").await.unwrap()).unwrap();
    assert_eq!(
        before["requests"][0]["profile"],
        json!({"kind":"agent","backend":"openai","model":"fixture","reasoning":null,"timeoutMs":null,"maxToolCalls":null,"maxTokens":null})
    );
    let mut bad = view.requests[0].clone();
    bad.status = RequestStatus::Green;
    bad.completed_at = Some("2026-01-01T00:00:01Z".into());
    assert!(receipts.save_request(&bad).await.is_err());
    bad.result = Some(json!({"verdict":"GREEN"}));
    bad.error = Some("contradiction".into());
    assert!(receipts.save_request(&bad).await.is_err());
    bad.status = RequestStatus::Error;
    assert!(receipts.save_request(&bad).await.is_err());
    let mut run = view.run.clone();
    run.status = RunStatus::Green;
    assert!(receipts.save_run(&run).await.is_err());
    assert_eq!(
        serde_json::to_value(read_run(&state, "run-old").await.unwrap()).unwrap(),
        before
    );
    let db = Connection::open(state.join(DATABASE)).unwrap();
    assert_eq!(
        db.pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        STATE_SCHEMA_VERSION
    );
    // Earlier schema-5 writes remain readable, even if their lifecycle fields contradict:
    // validation gates new writes, not a migration of saved data.
    db.execute(
        "UPDATE requests SET status='GREEN',data=json_set(data,'$.status','GREEN')",
        [],
    )
    .unwrap();
    assert_eq!(
        read_run(&state, "run-old").await.unwrap().requests[0].status,
        RequestStatus::Green
    );
}
