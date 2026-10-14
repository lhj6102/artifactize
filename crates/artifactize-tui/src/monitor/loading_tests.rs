use super::*;
use crate::{
    monitor::{Action, Pane},
    store::{Receipts, Run},
};
use crossterm::event::{KeyCode, KeyEvent};
use serde_json::json;
use std::{fs, sync::Arc};

async fn fixture(count: usize) -> (tempfile::TempDir, Receipts, Monitor) {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    let receipts = Receipts::open(&state, &repo).await.unwrap();
    for n in 0..count {
        let saved = json!({
            "id":format!("run-{n}"),"repoPath":repo,"stateDir":state,"status":"GREEN",
            "createdAt":"2026-01-01T00:00:00Z","completedAt":"2026-01-01T00:00:01Z",
            "selection":{"kind":"all"},"definitions":{"artifacts":{"app":{"path":"app"}}},"validation":null
        });
        let run: Run = serde_json::from_value(saved).unwrap();
        receipts.create_run(&run, &[]).await.unwrap();
    }
    (root, receipts, Monitor::new(state, None))
}
#[tokio::test]
async fn delayed_worker_leaves_navigation_and_quit_available_and_rejects_obsolete_selection() {
    let (_root, _receipts, mut monitor) = fixture(2).await;
    monitor.refresh().await;
    assert_eq!(monitor.open.as_deref(), Some("run-1"));
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let job = monitor
        .start_refresh_after(true, false, move || {
            started.send(()).unwrap();
            gate.recv().unwrap();
        })
        .unwrap();
    ready.await.unwrap();
    // No timers: the worker cannot proceed until after the input assertions finish.
    assert!(monitor.start_refresh(true, false).is_none());
    assert_eq!(monitor.key(KeyEvent::from(KeyCode::Down)), Action::Refresh);
    assert_eq!(monitor.open.as_deref(), Some("run-0"));
    assert_eq!(
        monitor.key(KeyEvent::from(KeyCode::Char('q'))),
        Action::Quit
    );
    release.send(()).unwrap();
    assert!(
        !monitor
            .finish_refresh(crate::task::joined(job.await).unwrap())
            .await
    );
    assert_eq!(monitor.open.as_deref(), Some("run-0"));
    monitor.refresh().await;
    assert_eq!(monitor.run.as_ref().unwrap().0.run.id.as_str(), "run-0");
}
#[tokio::test]
async fn page_edges_remain_bounded_and_the_selected_run_is_pinned_without_prefix_growth() {
    let (_root, receipts, mut monitor) = fixture(9).await;
    monitor.limit = 2;
    monitor.refresh().await;
    assert_eq!(
        monitor
            .runs
            .iter()
            .map(|run| run.id.as_str())
            .collect::<Vec<_>>(),
        ["run-8", "run-7"]
    );
    monitor.key(KeyEvent::from(KeyCode::Down));
    monitor.key(KeyEvent::from(KeyCode::Down));
    monitor.refresh().await;
    assert_eq!(monitor.offset, 2);
    assert_eq!(monitor.runs.len(), 2);
    assert_eq!(monitor.open.as_deref(), Some("run-6"));
    monitor.key(KeyEvent::from(KeyCode::Up));
    monitor.refresh().await;
    assert_eq!(monitor.offset, 0);
    assert_eq!(monitor.open.as_deref(), Some("run-7"));
    let mut newest = monitor.run.as_ref().unwrap().0.run.clone();
    newest.id = "run-new".parse().unwrap();
    receipts.create_run(&newest, &[]).await.unwrap();
    monitor.refresh().await;
    assert_eq!(monitor.limit, 2);
    assert_eq!(monitor.runs.len(), 3);
    assert_eq!(monitor.open.as_deref(), Some("run-7"));
    for _ in 0..4 {
        monitor.focus = Pane::Runs;
        monitor.select_run(monitor.runs.len() - 1);
        monitor.key(KeyEvent::from(KeyCode::Down));
        monitor.refresh().await;
        assert!(monitor.runs.len() <= 3);
    }
}
#[tokio::test]
async fn unrelated_writes_keep_the_selected_tree_and_detail_and_resync_forces_a_new_baseline() {
    let (_root, receipts, mut monitor) = fixture(2).await;
    monitor.refresh().await;
    monitor.open_detail().await;
    let nodes = monitor.nodes();
    let detail = monitor
        .detail
        .as_ref()
        .unwrap()
        .detail
        .field("Path")
        .map(str::to_owned);
    let mut unrelated = monitor.run.as_ref().unwrap().0.run.clone();
    unrelated.id = "run-0".parse().unwrap();
    unrelated.jobs = Some(unrelated.jobs.unwrap_or(0) + 1);
    receipts.save_run(&unrelated).await.unwrap();
    monitor.refresh().await;
    assert!(Arc::ptr_eq(&nodes, &monitor.nodes()));
    assert_eq!(
        monitor.detail.as_ref().unwrap().detail.field("Path"),
        detail.as_deref()
    );
    let job = monitor.start_refresh(true, true).unwrap();
    assert!(
        monitor
            .finish_refresh(crate::task::joined(job.await).unwrap())
            .await
    );
    assert!(!Arc::ptr_eq(&nodes, &monitor.nodes()));
}

#[tokio::test]
async fn scope_navigation_restores_the_same_cached_run_after_clearing_the_display() {
    let (_root, _receipts, mut monitor) = fixture(2).await;
    monitor.refresh().await;
    let id = monitor.open.clone().unwrap();
    let index = monitor
        .catalog
        .rows
        .iter()
        .position(|row| matches!(row.scope, Scope::Repository(_)))
        .unwrap();
    assert_eq!(monitor.select_scope(index), Action::Refresh);
    assert!(monitor.run.is_none());
    let job = monitor.start_refresh(false, false).unwrap();
    assert!(
        monitor
            .finish_refresh(crate::task::joined(job.await).unwrap())
            .await
    );
    assert_eq!(monitor.run.as_ref().unwrap().0.run.id, id);
    // Catalog rebuilding on navigation retains its global badges and the selected scope.
    assert_eq!(monitor.catalog.selected.as_ref(), Some(&monitor.scope));
}

#[tokio::test]
async fn run_navigation_reuses_catalog_and_page_but_detects_a_commit_without_any_hint() {
    let (_root, receipts, mut monitor) = fixture(3).await;
    monitor.refresh().await;
    monitor.select_run(1);
    let job = monitor.start_refresh(false, false).unwrap();
    let completed = crate::task::joined(job.await).unwrap();
    assert_eq!(
        completed.queries.len(),
        2,
        "Only the selected Run and joined requests are read"
    );
    assert!(monitor.finish_refresh(completed).await);
    assert_eq!(monitor.run.as_ref().unwrap().0.run.id.as_str(), "run-1");
    let mut inserted = monitor.run.as_ref().unwrap().0.run.clone();
    inserted.id = "run-new".parse().unwrap();
    receipts.create_run(&inserted, &[]).await.unwrap();
    // No subscription/hint is involved here; data_version belongs to this exact reader.
    monitor.select_run(2);
    let job = monitor.start_refresh(false, false).unwrap();
    let completed = crate::task::joined(job.await).unwrap();
    assert_eq!(completed.queries.len(), 7);
    assert!(monitor.finish_refresh(completed).await);
    assert!(monitor.runs.iter().any(|run| run.id.as_str() == "run-new"));
    assert_eq!(monitor.run.as_ref().unwrap().0.run.id.as_str(), "run-0");
}

#[tokio::test]
async fn different_state_identity_resets_details_even_when_run_ids_match() {
    let (_root, _receipts, mut monitor) = fixture(1).await;
    let (_other, _other_receipts, replacement) = fixture(1).await;
    monitor.refresh().await;
    monitor.open_detail().await;
    let previous = monitor.origin.identity().cloned().unwrap();
    assert!(monitor.detail.is_some());
    monitor.state = replacement.state;
    monitor.refresh().await;
    assert_ne!(monitor.origin.identity(), Some(&previous));
    assert!(monitor.detail.is_none() && monitor.peek.is_none() && monitor.reviews.is_empty());
    assert_eq!(monitor.run.as_ref().unwrap().0.run.id.as_str(), "run-0");
}
