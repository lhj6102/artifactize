use crate::store::{self, OwnedReceipts};
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

#[tokio::test]
async fn a_saved_run_keeps_its_first_definitions_when_saved_again() {
    let (_root, state, receipts, mut saved) = fixture().await;
    let first = store::read_run(&state, "run-1").await.unwrap();
    // Saving the Run again saves its state; the definitions it was started with stay.
    saved.jobs = Some(7);
    saved.definitions =
        serde_json::from_value(json!({"artifacts":{"web":{"path":"web"}},"evals":[]})).unwrap();
    receipts.save_run(&saved).await.unwrap();
    let read = store::read_run(&state, "run-1").await.unwrap();
    assert_eq!(read.run.jobs, Some(7));
    assert_eq!(
        serde_json::to_value(read.run.definitions.graph().unwrap()).unwrap(),
        serde_json::to_value(first.run.definitions.graph().unwrap()).unwrap()
    );
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
async fn an_existing_reader_sees_the_updated_run_and_request() {
    let (_root, state, receipts, mut saved) = fixture().await;
    let id = "run-1".parse().unwrap();
    let mut reader = store::reader::Reader::default();
    let snapshot = |reader: &mut store::reader::Reader| {
        reader
            .snapshot(&state, |db, cache, state| {
                cache.selected(db, state, &id, false)
            })
            .unwrap()
            .unwrap()
            .0
    };
    let before = snapshot(&mut reader);
    let mut app = request("run-1", "app", saved.repo_path.as_path());
    app.state = store::RequestState::failed("stopped".into(), None, crate::broker::now());
    receipts.save_request(&app).await.unwrap();
    let updated = snapshot(&mut reader);
    assert_eq!(updated.requests[0].error(), Some("stopped"));
    assert_eq!(updated.requests[1].status(), before.requests[1].status());
    assert_eq!(before.requests[0].error(), None);
    saved.jobs = Some(7);
    receipts.save_run(&saved).await.unwrap();
    let settled = snapshot(&mut reader);
    assert_eq!(settled.run.jobs, Some(7));
    assert_eq!(settled.requests[0].error(), Some("stopped"));
    assert_eq!(settled.requests[1].status(), before.requests[1].status());
}
