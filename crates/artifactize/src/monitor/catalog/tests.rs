//! Initial selection never changes how legacy workspaces are grouped.
use super::*;
use crate::{
    store::{self, CatalogRun},
    types::RunStatus,
};

fn run(path: &Path) -> CatalogRun {
    CatalogRun {
        repo_path: path.to_path_buf(),
        repository: Identity::default(),
        status: RunStatus::Green,
        red: 0,
        error: 0,
        waiting: Vec::new(),
    }
}
fn workspace(path: &Path) -> Scope {
    Scope::Worktree(
        Repository::Workspace(path.to_path_buf()),
        path.to_path_buf(),
    )
}

#[test]
fn non_git_initial_selection_uses_nearest_component_ancestor_without_grouping_legacy_paths() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let nested = repo.join("nested");
    let sibling = root.path().join("repo2");
    let unrelated = root.path().join("other");
    for path in [&repo, &nested, &sibling, &unrelated] {
        std::fs::create_dir_all(path.join("sub")).unwrap();
    }
    let runs = vec![run(&repo), run(&nested)];
    let mut catalog = Catalog::default();
    catalog.update(&runs, Some(&nested.join("sub")));
    assert_eq!(
        catalog.initial(Some(&nested.join("sub"))),
        workspace(&nested)
    );
    assert_eq!(catalog.initial(Some(&repo.join("sub"))), workspace(&repo));
    assert_eq!(catalog.initial(Some(&repo)), workspace(&repo));
    assert_eq!(
        catalog.initial(Some(&sibling.join("sub"))),
        workspace(&sibling.join("sub"))
    );
    assert_eq!(catalog.initial(Some(&unrelated)), workspace(&unrelated));
    assert_eq!(catalog.initial(None), Scope::All);
    assert_eq!(catalog.paths(&workspace(&repo)), Some(vec![repo.clone()]));
    assert_eq!(
        catalog.paths(&workspace(&nested)),
        Some(vec![nested.clone()])
    );
    assert!(
        catalog
            .rows
            .iter()
            .any(|row| row.scope == Scope::Repository(Repository::Workspace(repo.clone())))
    );
    assert!(
        catalog
            .rows
            .iter()
            .any(|row| row.scope == Scope::Repository(Repository::Workspace(nested.clone())))
    );
}

#[tokio::test]
async fn starting_inside_saved_non_git_workspace_selects_its_runs() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    std::fs::create_dir_all(repo.join("sub")).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
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
    let mut monitor = super::super::Monitor::new(state, Some(repo.join("sub")));
    monitor.refresh().await;
    assert_eq!(monitor.scope, workspace(&repo));
    assert_eq!(monitor.selected_run().unwrap().id.as_str(), "run-non-git");
}
