//! The loader's snapshots of a large Run: listing, opening, and updating the shown Run
//! when saved state changes.
use super::*;
use crate::{
    changes::Change,
    store::{DATABASE, OwnedReceipts, Run},
};
use serde_json::json;
use std::path::Path;

/// As many Artifacts, evals and requests as the largest Run saved so far.
const LARGE: usize = 16_000;

fn run(id: &str, repo: &Path, state: &Path, artifacts: usize) -> (Run, Vec<store::Request>) {
    let names: Vec<_> = (0..artifacts).map(|n| format!("a{n}")).collect();
    let definitions = json!({
        "artifacts": names.iter().map(|name| (name.clone(), json!({"path": name}))).collect::<serde_json::Map<_, _>>(),
        "evals": names.iter().map(|name| json!({"id": format!("{name}/check"), "target": name, "deps": [], "declaration": {"title": "Check"}})).collect::<Vec<_>>(),
    });
    let run = serde_json::from_value(json!({
        "id":id,"repoPath":repo,"stateDir":state,"status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z","completedAt":null,
        "selection":{"kind":"all"},"definitions":definitions,"validation":null,"evidence":{}
    }))
    .unwrap();
    let requests = names
        .iter()
        .map(|name| {
            crate::test_request::request(
                &format!("{name}/check"),
                "QUEUED",
                json!({"id":format!("{id}-{name}"),"runId":id,"cwd":repo}),
            )
            .request
            .as_ref()
            .clone()
        })
        .collect();
    (run, requests)
}

fn input(state: &Path, repo: &Path, open: Option<&str>, changed: Changed) -> Input {
    Input {
        state: state.to_path_buf(),
        launch: Some(repo.to_path_buf()),
        open: open.map(|id| id.parse().unwrap()),
        force: false,
        rescan: false,
        changed,
    }
}

fn hinted(id: &str) -> Changed {
    let mut changed = Changed::default();
    changed.add(&Change::RunInvalidated(id.parse().unwrap()));
    changed
}

/// Fail the request `id` as a writer would.
async fn fail(receipts: &OwnedReceipts, requests: &[store::Request], id: &str) {
    let mut request = requests
        .iter()
        .find(|request| request.id.as_str() == id)
        .unwrap()
        .clone();
    request.state = store::RequestState::failed("stopped".into(), None, crate::broker::now());
    receipts.save_request(&request).await.unwrap();
}

fn loaded(snapshot: Snapshot) -> Opened {
    let OpenRun::Loaded(run) = snapshot.open_run else {
        panic!("snapshot should have loaded the Run");
    };
    *run
}

#[tokio::test]
async fn a_cold_reference_reads_only_the_named_run_even_with_unrelated_history() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir(&repo).unwrap();
    let receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    std::fs::create_dir(repo.join(".artifactize")).unwrap();
    std::fs::write(
        repo.join(".artifactize/root.artf"),
        "path = './'\nbasis = true\n",
    )
    .unwrap();
    let (selected, requests) = run("run-selected", &repo, &state, 1);
    receipts.create_run(&selected, &requests).await.unwrap();
    let read = || {
        let mut loader = Loader::default();
        loader.reader.traced_statements();
        let reference = loader.open(&state, &selected.id).unwrap();
        assert_eq!(reference.opened.unwrap().1.len(), 1);
        assert!(reference.branch.is_some());
        loader.reader.traced_statements()
    };
    let before = read();
    for index in 0..40 {
        let (old, requests) = run(&format!("run-other{index}"), &repo, &state, 10);
        receipts.create_run(&old, &requests).await.unwrap();
    }
    let after = read();
    assert_eq!(before, after);
    for sql in &after {
        if sql.contains("FROM runs") || sql.contains("FROM requests") {
            assert!(
                sql.contains("'run-selected'"),
                "unscoped history read: {sql}"
            );
        }
    }
}

#[tokio::test]
async fn a_large_run_is_listed_opened_and_refreshed_with_its_saved_definitions() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir(&repo).unwrap();
    let receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    let (large, large_requests) = run("run-large", &repo, &state, LARGE);
    let (small, small_requests) = run("run-small", &repo, &state, 1);
    let (broken, broken_requests) = run("run-broken", &repo, &state, 1);
    receipts.create_run(&large, &large_requests).await.unwrap();
    receipts.create_run(&small, &small_requests).await.unwrap();
    receipts
        .create_run(&broken, &broken_requests)
        .await
        .unwrap();
    // Definitions that do not decode: only opening their Run reads them.
    rusqlite::Connection::open(state.join(DATABASE))
        .unwrap()
        .execute(
            "UPDATE run_definitions SET data='{\"artifacts\":7}' WHERE run_id='run-broken'",
            [],
        )
        .unwrap();
    let mut loader = Loader::default();

    // Starting lists every Run and reads no definitions.
    let start = loader
        .load(input(&state, &repo, None, Changed::Nothing))
        .unwrap();
    assert!(start.unreadable.is_empty(), "{:?}", start.unreadable);
    assert_eq!(start.labels.len(), 3);

    // Opening a Run decodes its definitions, and a broken one says why it does not open.
    let broken = loader
        .load(input(&state, &repo, Some("run-broken"), Changed::Nothing))
        .unwrap();
    assert!(matches!(broken.open_run, OpenRun::Absent));
    assert!(
        broken
            .unreadable
            .iter()
            .any(|record| record.id == "run-broken"),
        "{:?}",
        broken.unreadable
    );
    let mut open = input(&state, &repo, Some("run-large"), Changed::Nothing);
    open.force = true;
    let (_, before, _, _) = loaded(loader.load(open).unwrap());
    assert_eq!(before.len(), LARGE);
    assert_eq!(
        before[7]
            .definition
            .as_ref()
            .unwrap()
            .eval()
            .declaration
            .value()
            .and_then(|declaration| declaration.title.value())
            .map(String::as_str),
        Some("Check")
    );

    // The shown snapshot keeps the original definitions while reflecting changed requests.
    rusqlite::Connection::open(state.join(DATABASE))
        .unwrap()
        .execute(
            "UPDATE run_definitions SET data='{\"artifacts\":7}' WHERE run_id='run-large'",
            [],
        )
        .unwrap();
    fail(&receipts, &large_requests, "run-large-a7").await;
    let (_, requests, _, _) = loaded(
        loader
            .load(input(&state, &repo, Some("run-large"), hinted("run-large")))
            .unwrap(),
    );
    let failed = requests
        .iter()
        .find(|view| view.request.id.as_str() == "run-large-a7")
        .unwrap();
    assert_eq!(failed.request.status(), crate::types::RequestStatus::Error);
    assert_eq!(
        failed
            .definition
            .as_ref()
            .unwrap()
            .eval()
            .declaration
            .value()
            .and_then(|declaration| declaration.title.value())
            .map(String::as_str),
        Some("Check")
    );

    // A write to another Run leaves the open Run unchanged.
    fail(&receipts, &small_requests, "run-small-a0").await;
    let other = loader
        .load(input(&state, &repo, Some("run-large"), hinted("run-small")))
        .unwrap();
    assert!(matches!(other.open_run, OpenRun::Unchanged));
    let unopened = Loader::default()
        .load(input(&state, &repo, Some("run-large"), Changed::Nothing))
        .unwrap();
    assert!(matches!(unopened.open_run, OpenRun::Absent));
    assert!(
        unopened
            .unreadable
            .iter()
            .any(|record| record.id == "run-large")
    );
}

#[test]
fn a_hinted_runs_row_replaces_its_own_and_a_new_run_is_listed_last() {
    let row = |id: &str, status: crate::types::RunStatus| CatalogRun {
        id: id.parse().unwrap(),
        created_at: "2026-01-01T00:00:00Z".parse().unwrap(),
        completed_at: None,
        repo_path: "/repo".into(),
        repository: Default::default(),
        status,
        comment: None,
        red: 0,
        error: 0,
        waiting: Vec::new(),
    };
    use crate::types::RunStatus::{Green, Red, Running};
    let runs = [row("run-1", Running), row("run-2", Running)];
    let ids: Vec<RunId> = ["run-1", "run-3"].map(|id| id.parse().unwrap()).into();
    let rows = merged(&runs, &ids, vec![row("run-1", Green), row("run-3", Red)]);
    assert_eq!(
        rows.iter()
            .map(|run| (run.id.as_str(), run.status))
            .collect::<Vec<_>>(),
        [("run-1", Green), ("run-2", Running), ("run-3", Red)]
    );
    // A hinted Run without a row is gone.
    let ids: Vec<RunId> = vec!["run-2".parse().unwrap()];
    let rows = merged(&runs, &ids, Vec::new());
    assert_eq!(
        rows.iter().map(|run| run.id.as_str()).collect::<Vec<_>>(),
        ["run-1"]
    );
}

#[tokio::test]
async fn a_run_no_longer_shown_gives_its_memory_back_once_its_last_share_goes() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir(&repo).unwrap();
    let receipts = OwnedReceipts::open(&state, &repo).await.unwrap();
    let (first, first_requests) = run("run-first", &repo, &state, 3);
    let (second, second_requests) = run("run-second", &repo, &state, 3);
    receipts.create_run(&first, &first_requests).await.unwrap();
    receipts
        .create_run(&second, &second_requests)
        .await
        .unwrap();
    let mut loader = Loader::default();
    let shown = loaded(
        loader
            .load(input(&state, &repo, Some("run-first"), Changed::Nothing))
            .unwrap(),
    );
    let first_graph = std::sync::Arc::downgrade(&shown.0.run.definitions.share().unwrap());

    // The old snapshot keeps its graph while it is displayed, even after a different Run opens.
    let next = loaded(
        loader
            .load(input(&state, &repo, Some("run-second"), Changed::Nothing))
            .unwrap(),
    );
    let second_graph = std::sync::Arc::downgrade(&next.0.run.definitions.share().unwrap());
    assert!(first_graph.upgrade().is_some());
    drop(shown);
    assert!(
        first_graph.upgrade().is_none(),
        "the old graph has no owners"
    );

    drop(next);
    assert!(
        second_graph.upgrade().is_some(),
        "the loader still holds the open graph"
    );
    let closed = loader
        .load(input(&state, &repo, None, Changed::Nothing))
        .unwrap();
    assert!(matches!(closed.open_run, OpenRun::Absent));
    assert!(
        second_graph.upgrade().is_none(),
        "closing releases the second graph"
    );

    let reopened = loaded(
        loader
            .load(input(&state, &repo, Some("run-second"), Changed::Nothing))
            .unwrap(),
    );
    assert_eq!(reopened.1.len(), 3);
}
