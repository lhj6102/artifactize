use std::{fs, os::unix::fs::symlink, process::Command, sync::mpsc, thread};

use artifactize::store::{
    DATABASE, Receipts, STATE_SCHEMA_VERSION, read_fingerprint_executions, read_latest_requests,
    read_request, read_requests, read_run, read_runs,
};
use rusqlite::Connection;

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
        read_fingerprint_executions(state, &[("missing".into(), "definition".into())])
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
    let first = root.path().join("first");
    let second = root.path().join("second");
    let state = root.path().join("state");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    for (repo, name) in [(&first, "first"), (&second, "second")] {
        fs::write(
            repo.join("artifactize.json"),
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
        assert_eq!(artifactize::query::run_output(&read), run);
    }
    assert!(!root.path().join("unused").exists());
}

#[tokio::test]
async fn schema_allows_only_one_active_execution_per_fingerprint_and_definition() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let _receipts = Receipts::open(&state, &repo).await.unwrap();
    let db = Connection::open(state.join(DATABASE)).unwrap();
    let insert = "INSERT INTO executions(id,fingerprint,eval_def_hash,owner_pid,owner_start_time,status,data) VALUES (?, ?, 'definition', 1, 1, ?, '{}')";
    db.execute(insert, ["first", "shared", "RUNNING"]).unwrap();
    assert!(
        db.execute(insert, ["second", "shared", "WAITING_HUMAN"])
            .is_err()
    );
    db.execute("INSERT INTO executions(id,fingerprint,eval_def_hash,owner_pid,owner_start_time,status,data) VALUES ('different','shared','other',1,1,'WAITING_HUMAN','{}')", []).unwrap();
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
    fs::write(
        repo.join("artifactize.json"),
        r#"{"name":"basis","basis":true}"#,
    )
    .unwrap();
    symlink(&repo, root.path().join("alias")).unwrap();
    let inner = repo.join("inner");
    fs::create_dir(&inner).unwrap();
    symlink(&inner, root.path().join("nested-alias")).unwrap();
    for state in [
        repo.join("state"),
        root.path().join("alias/state"),
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
    symlink(&repo, state.join("runs")).unwrap();
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
    fs::remove_file(state.join("runs")).unwrap();
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
        symlink(&protected, &file).unwrap();
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
