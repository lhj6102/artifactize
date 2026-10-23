//! Saved lifecycle corruption is isolated at the Monitor boundary.

use std::fs;

use artifactize_core::{
    project::{self, VerifyOptions, selection::Selection},
    store::{self, DATABASE},
};
use rusqlite::Connection;
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

async fn verify_at(
    repo: &std::path::Path,
    state: &std::path::Path,
    created_at: &str,
) -> store::RunView {
    let mut view = project::verify(
        repo,
        Some(state),
        &Selection::All,
        &VerifyOptions::default(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    // The Monitor's initial selection must not depend on verify speed or clock resolution.
    view.run.created_at = created_at.parse().unwrap();
    store::OwnedReceipts::open(state, repo)
        .await
        .unwrap()
        .save_run(&view.run)
        .await
        .unwrap();
    view
}

#[tokio::test]
async fn contradictory_runs_do_not_abort_the_monitor() {
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let _lease = support::runs::RepoLease::new(root, &state, &repo);
    support::declaration::write(repo.join("index.artf"), json!({
        "name":"app", "evals":[{"id":"check","title":"Check","payload":{"instruction":"Check."},"profile":{"kind":"runtime","command":support::os::echo_program(),"args":["ok"]}}]
    }).to_string()).unwrap();
    verify_at(&repo, &state, "2026-01-01T00:00:00Z").await;
    // Corrupt the newest Run: the Monitor initially selects it and must still refresh.
    let bad = verify_at(&repo, &state, "2026-01-01T00:01:00Z").await;
    let db = Connection::open(state.join(DATABASE)).unwrap();
    db.execute(
        "UPDATE runs SET status='RUNNING',data=json_set(data,'$.status','RUNNING') WHERE id=?",
        [&bad.run.id],
    )
    .unwrap();
    let mut monitor = artifactize_tui::monitor::Monitor::new(state.clone(), Some(repo.clone()));
    monitor.refresh().await;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(180, 40)).unwrap();
    let frame = terminal.draw(|frame| monitor.draw(frame)).unwrap();
    let text: String = frame
        .buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    // The deliberately malformed receipt is setup, not a worker's real lifecycle. Restore
    // it after rendering so the fixture can observe terminal worker exit during teardown.
    db.execute(
        "UPDATE runs SET status=?1,data=json_set(data,'$.status',?1) WHERE id=?2",
        [bad.run.status().as_str(), bad.run.id.as_str()],
    )
    .unwrap();
    assert!(text.contains(bad.run.id.as_str()), "{text}");
    assert!(text.contains("An open Run cannot be completed"), "{text}");
    assert!(!text.contains("read failed"), "{text}");
}
