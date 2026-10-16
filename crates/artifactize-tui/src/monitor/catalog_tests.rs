//! Where the Monitor starts, from the saved workspaces.
use super::*;
use crate::store;

fn workspace(path: &std::path::Path) -> Scope {
    Scope::Worktree(
        Repository::Workspace(path.to_path_buf()),
        path.to_path_buf(),
    )
}

#[tokio::test]
async fn starting_inside_saved_non_git_workspace_selects_its_runs() {
    let root = crate::test_os::tempdir();
    let repo = crate::test_os::canonical(root.path()).join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    let receipts = store::OwnedReceipts::open(&state, &repo).await.unwrap();
    let run: store::Run = serde_json::from_value(serde_json::json!({
        "id":"run-non-git",
        "repoPath":repo,
        "stateDir":state,
        "status":"GREEN",
        "createdAt":"2026-01-01T00:00:00Z",
        "completedAt":"2026-01-01T00:00:01Z",
        "selection":{"kind":"all"},
        "validation":null,
    }))
    .unwrap();
    receipts.create_run(&run, &[]).await.unwrap();
    let mut monitor = Monitor::new(state, Some(repo.join("sub")));
    monitor.refresh().await;
    assert_eq!(monitor.scope, workspace(&repo));
    assert_eq!(monitor.selected_run().unwrap().id.as_str(), "run-non-git");
}
