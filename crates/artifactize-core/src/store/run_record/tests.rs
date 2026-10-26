use crate::store::{self, DATABASE, OwnedReceipts};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::Path;

fn run(id: &str, repo: &Path, state: &Path, definitions: Value) -> store::Run {
    serde_json::from_value(json!({
        "id":id,"repoPath":repo,"stateDir":state,"status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z","completedAt":null,
        "selection":{"kind":"all"},"definitions":definitions,"validation":null,"evidence":{}
    }))
    .unwrap()
}

fn request(run: &str, artifact: &str, repo: &Path) -> store::Request {
    crate::test_request::request(
        &format!("{artifact}/check"),
        "QUEUED",
        json!({"id":format!("{run}-{artifact}"),"runId":run,"cwd":repo}),
    )
    .request
    .as_ref()
    .clone()
}

const DEFINED: &str = r#"{"artifacts":{"app":{"path":"app"}},"evals":[{"id":"app/check","target":"app","deps":[],"declaration":{"title":"Check"}}]}"#;

async fn fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    OwnedReceipts,
    store::Run,
) {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir(&repo).unwrap();
    let receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    let saved = run(
        "run-1",
        &repo,
        &state,
        serde_json::from_str(DEFINED).unwrap(),
    );
    receipts
        .create_run(
            &saved,
            &[
                request("run-1", "app", &repo),
                request("run-1", "web", &repo),
            ],
        )
        .await
        .unwrap();
    (root, state, receipts, saved)
}

fn row<T: rusqlite::types::FromSql>(state: &Path, sql: &str) -> T {
    Connection::open_with_flags(
        state.join(DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row(sql, [], |row| row.get(0))
    .unwrap()
}

#[tokio::test]
async fn a_run_saves_its_definitions_once_and_its_state_without_them() {
    let (_root, state, receipts, mut saved) = fixture().await;
    let written: String = row(
        &state,
        "SELECT data FROM run_definitions WHERE run_id='run-1'",
    );
    assert_eq!(
        serde_json::from_str::<Value>(&written).unwrap(),
        serde_json::from_str::<Value>(DEFINED).unwrap()
    );
    // Saving the Run again writes its state; the definitions it holds are not written again.
    saved.jobs = Some(7);
    saved.definitions =
        serde_json::from_value(json!({"artifacts":{"web":{"path":"web"}},"evals":[]})).unwrap();
    receipts.save_run(&saved).await.unwrap();
    let state_text: String = row(&state, "SELECT data FROM runs WHERE id='run-1'");
    let state_value: Value = serde_json::from_str(&state_text).unwrap();
    assert_eq!(state_value["jobs"], 7);
    assert!(state_value["definitions"].is_null());
    assert_eq!(
        row::<String>(
            &state,
            "SELECT data FROM run_definitions WHERE run_id='run-1'"
        ),
        written
    );
    let read = store::read_run(&state, "run-1").await.unwrap();
    assert_eq!(read.run.jobs, Some(7));
    let artifacts = read
        .run
        .definitions
        .graph()
        .unwrap()
        .artifacts
        .value()
        .unwrap();
    assert!(artifacts.contains_key("app") && !artifacts.contains_key("web"));
}

#[tokio::test]
async fn an_update_raises_the_revision_of_only_the_row_it_changes() {
    let (_root, state, receipts, saved) = fixture().await;
    let revisions = |state: &Path| -> (i64, i64, i64) {
        (
            row(state, "SELECT revision FROM runs WHERE id='run-1'"),
            row(state, "SELECT revision FROM requests WHERE id='run-1-app'"),
            row(state, "SELECT revision FROM requests WHERE id='run-1-web'"),
        )
    };
    assert_eq!(revisions(&state), (0, 0, 0));
    let mut app = request("run-1", "app", saved.repo_path.as_path());
    app.state = store::RequestState::failed("stopped".into(), None, crate::broker::now());
    receipts.save_request(&app).await.unwrap();
    receipts.save_run(&saved).await.unwrap();
    receipts.save_run(&saved).await.unwrap();
    assert_eq!(revisions(&state), (2, 1, 0));
    // A writer that bypasses the store is numbered too.
    Connection::open(state.join(DATABASE))
        .unwrap()
        .execute(
            "UPDATE requests SET status='ERROR' WHERE id='run-1-web'",
            [],
        )
        .unwrap();
    assert_eq!(revisions(&state), (2, 1, 1));
}
