//! Initial selection never changes how legacy workspaces are grouped.
use super::*;
use crate::{store::CatalogRun, types::RunStatus};

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
