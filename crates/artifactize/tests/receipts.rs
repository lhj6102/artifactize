use std::{fs, os::unix::fs::symlink, path::Path, process::Command};

use artifactize::store::{DATABASE, Receipts, read_run};
use rusqlite::Connection;

#[tokio::test]
async fn missing_read_is_inert_and_future_schemas_are_not_modified() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    assert!(read_run(&state, None, "missing").await.is_err());
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
            .contains("Unsupported receipts schema")
    );
    assert!(
        read_run(&state, None, "missing")
            .await
            .unwrap_err()
            .contains("Unsupported receipts schema")
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

#[tokio::test]
async fn explicit_receipts_cannot_be_rebound_to_another_repository() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    let state = root.path().join("state");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    let receipts = Receipts::open(&state, &first).await.unwrap();
    assert!(
        Receipts::open(&state, &second)
            .await
            .err()
            .unwrap()
            .contains("different repository")
    );
    assert!(
        read_run(&state, Some(&second), "missing")
            .await
            .unwrap_err()
            .contains("different repository")
    );
    drop(receipts);
    let db = Connection::open(state.join(DATABASE)).unwrap();
    let repo: String = db
        .query_row(
            "SELECT value FROM metadata WHERE key='repo_path'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(Path::new(&repo), first);
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
        let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .env("ARTIFACTIZE_STATE_HOME", root.path().join("home"))
            .arg("--repo")
            .arg(&repo)
            .arg("--state-dir")
            .arg(state)
            .args(["verify", "--all", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("outside the reviewed repository")
        );
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
    assert_eq!(output.status.code(), Some(2));
    assert!(!repo.join("home").exists());
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
    symlink(&protected, state.join(DATABASE)).unwrap();
    assert!(
        Receipts::open(&state, &repo)
            .await
            .err()
            .unwrap()
            .contains("regular files")
    );
    assert_eq!(fs::read_to_string(&protected).unwrap(), "unchanged");
}
