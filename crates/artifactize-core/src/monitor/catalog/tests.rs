//! Initial selection and refreshed Git discovery through the shared snapshot loader.
use super::*;
use crate::{
    monitor::{Input, Loader, Page, Snapshot},
    store::{self, CatalogRun},
    types::RunStatus,
};

fn input(state: &Path, repo: &Path, shown: Option<&Snapshot>, full: bool) -> Input {
    Input {
        state: state.to_path_buf(),
        initial: Some(repo.to_path_buf()),
        catalog: shown.map_or_else(Catalog::default, |shown| shown.catalog.clone()),
        initialized: shown.is_some(),
        page: Page {
            scope: shown.map_or(Scope::All, |shown| shown.scope.clone()),
            open: shown.and_then(|shown| shown.selected_id.clone()),
            offset: 0,
            tail: false,
            limit: crate::monitor::PAGE,
        },
        full,
        displayed: shown.and_then(|shown| shown.selected_id.clone()),
        changed: Default::default(),
        runs: shown.map(|shown| shown.runs.clone()),
    }
}

#[tokio::test]
async fn full_refresh_rereads_git_metadata() {
    for saved in [false, true] {
        let root = crate::test_os::tempdir();
        let base = crate::test_os::canonical(root.path());
        let repo = base.join("repo");
        let state = base.join("state");
        let before = base.join("empty-before");
        let after = base.join("empty-after");
        std::fs::create_dir(&repo).unwrap();
        crate::test_os::git(&repo, &["init", "--quiet"]);
        crate::test_os::git(&repo, &["checkout", "--quiet", "-b", "before-refresh"]);
        crate::test_os::git(
            &repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        );
        crate::test_os::git(
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "empty-before",
                &crate::platform::path_text(&before),
            ],
        );
        if saved {
            let receipts = store::OwnedReceipts::open(&state, &repo).await.unwrap();
            let run: store::Run = serde_json::from_value(serde_json::json!({
                "id":"run-refresh", "repoPath":repo, "stateDir":state, "status":"GREEN",
                "createdAt":"2026-01-01T00:00:00Z", "completedAt":"2026-01-01T00:00:01Z",
                "selection":{"kind":"all"}, "definitions":{"artifacts":{},"evals":[]},
                "validation":null,
            }))
            .unwrap();
            receipts.create_run(&run, &[]).await.unwrap();
        }
        let repository = Repository::Git(crate::test_os::canonical(&repo.join(".git")));
        let scope = Scope::Worktree(repository.clone(), repo.clone());
        let before_scope = Scope::Worktree(repository.clone(), before.clone());
        let after_scope = Scope::Worktree(repository.clone(), after.clone());
        let mut loader = Loader::default();
        let mut shown = loader.load(input(&state, &repo, None, true)).unwrap();
        shown.catalog.toggle(&repository);
        shown = loader
            .load(input(&state, &repo, Some(&shown), false))
            .unwrap();
        assert_eq!(shown.scope, scope);
        assert_eq!(shown.catalog.label(&scope), "before-refresh · repo");
        assert!(
            shown
                .catalog
                .rows
                .iter()
                .any(|row| row.scope == before_scope)
        );
        let run_ids: Vec<_> = shown.runs.iter().map(|run| run.id.clone()).collect();
        assert_eq!(run_ids.len(), usize::from(saved));

        // Only Git changes: neither a database write nor a state hint is needed.
        crate::test_os::git(&repo, &["branch", "-m", "after-refresh"]);
        crate::test_os::git(
            &repo,
            &["worktree", "remove", &crate::platform::path_text(&before)],
        );
        crate::test_os::git(
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "empty-after",
                &crate::platform::path_text(&after),
            ],
        );
        // Both navigation and ordinary full database refreshes keep Git discovery cached.
        for full in [false, true] {
            shown = loader
                .load(input(&state, &repo, Some(&shown), full))
                .unwrap();
            assert_eq!(shown.catalog.label(&scope), "before-refresh · repo");
            assert!(
                shown
                    .catalog
                    .rows
                    .iter()
                    .any(|row| row.scope == before_scope)
            );
            assert!(
                !shown
                    .catalog
                    .rows
                    .iter()
                    .any(|row| row.scope == after_scope)
            );
        }
        loader.rescan();
        shown = loader
            .load(input(&state, &repo, Some(&shown), true))
            .unwrap();
        assert_eq!(shown.scope, scope);
        assert_eq!(shown.catalog.label(&scope), "after-refresh · repo");
        assert!(
            !shown
                .catalog
                .rows
                .iter()
                .any(|row| row.scope == before_scope)
        );
        assert!(
            shown
                .catalog
                .rows
                .iter()
                .any(|row| row.scope == after_scope)
        );
        assert_eq!(
            shown
                .runs
                .iter()
                .map(|run| run.id.clone())
                .collect::<Vec<_>>(),
            run_ids
        );

        // A failed read must not consume the reset while the caller retains its old catalog.
        crate::test_os::git(&repo, &["branch", "-m", "after-retry"]);
        let broken = base.join("not-a-state-directory");
        std::fs::write(&broken, "not a directory").unwrap();
        loader.rescan();
        assert!(
            loader
                .load(input(&broken, &repo, Some(&shown), true))
                .is_err()
        );
        shown = loader
            .load(input(&state, &repo, Some(&shown), true))
            .unwrap();
        assert_eq!(shown.catalog.label(&scope), "after-retry · repo");
        assert!(
            shown
                .catalog
                .rows
                .iter()
                .any(|row| row.scope == after_scope)
        );
    }
}

fn run(path: &Path) -> CatalogRun {
    CatalogRun {
        id: "run-1".parse().unwrap(),
        created_at: crate::types::Timestamp::now(),
        completed_at: None,
        repo_path: path.to_path_buf(),
        repository: Identity::default(),
        status: RunStatus::Green,
        comment: None,
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
    let root = crate::test_os::tempdir();
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
