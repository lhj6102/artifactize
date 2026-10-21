use super::*;
use crate::{
    repository::Identity,
    store::CatalogRun,
    types::{RunStatus, Timestamp},
};
use std::path::Path;

/// A saved Run's catalog fact: Run `id` of the workspace `repo`, started `day` days after a
/// fixed time.
fn run(id: &str, repo: &Path, identity: Identity, day: i64, status: RunStatus) -> CatalogRun {
    let start = time::OffsetDateTime::UNIX_EPOCH + time::Duration::days(10_000 + day);
    CatalogRun {
        id: id.parse().unwrap(),
        created_at: Timestamp::new(start).unwrap(),
        completed_at: None,
        repo_path: repo.to_path_buf(),
        repository: identity,
        status,
        red: 0,
        error: 0,
        waiting: Vec::new(),
    }
}

/// Git with a fixed identity, so commits never depend on the machine's configuration.
fn git(path: &Path, arguments: &[&str]) {
    let mut all = vec![
        "-c",
        "user.name=artifactize",
        "-c",
        "user.email=artifactize@example.invalid",
    ];
    all.extend_from_slice(arguments);
    crate::test_os::git(path, &all);
}

/// A repository `main` on branch `main` with one commit, inside `root`.
fn repository(root: &Path) -> std::path::PathBuf {
    let main = root.join("project");
    std::fs::create_dir(&main).unwrap();
    git(&main, &["init", "--quiet", "--initial-branch=main"]);
    git(
        &main,
        &["commit", "--quiet", "--allow-empty", "-m", "first"],
    );
    crate::test_os::canonical(&main)
}

fn names(listing: &Listing) -> Vec<(String, String)> {
    let mut names: Vec<_> = listing
        .branches
        .iter()
        .map(|branch| (branch.project.clone(), branch.name.clone()))
        .collect();
    names.sort();
    names
}

#[test]
fn a_project_lists_each_worktree_that_exists_with_its_branch_or_detached_folder() {
    let root = crate::test_os::tempdir();
    let main = repository(root.path());
    let feature = root.path().join("feature");
    let loose = root.path().join("loose");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feat/x",
            &crate::platform::path_text(&feature),
        ],
    );
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            &crate::platform::path_text(&loose),
        ],
    );
    let identity = repository::identify(&main);
    let runs = [run("run-1", &main, identity, 0, RunStatus::Green)];
    let mut discovery = Discovery::default();
    let listing = branches(&runs, None, &mut discovery);
    assert_eq!(
        names(&listing),
        [
            ("project".into(), "feat/x".into()),
            ("project".into(), "loose".into()),
            ("project".into(), "main".into()),
        ]
    );
    // A removed worktree goes once Git is asked again; until then the listing is cached.
    std::fs::remove_dir_all(&feature).unwrap();
    assert_eq!(branches(&runs, None, &mut discovery).branches.len(), 3);
    discovery.rescan();
    assert_eq!(
        names(&branches(&runs, None, &mut discovery)),
        [
            ("project".into(), "loose".into()),
            ("project".into(), "main".into()),
        ]
    );
}

#[test]
fn a_branch_has_its_projects_runs_on_that_branch_from_whichever_folder_they_ran_in() {
    let root = crate::test_os::tempdir();
    let main = repository(root.path());
    let common = repository::identify(&main).common_dir.unwrap();
    let feature = root.path().join("feature");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feat/x",
            &crate::platform::path_text(&feature),
        ],
    );
    let feature = crate::test_os::canonical(&feature);
    // A Run of feat/x from a worktree removed since, and one from the worktree it has now.
    let gone = root.path().join("gone");
    let on = |tree: &Path| Identity {
        common_dir: Some(common.clone()),
        worktree_path: Some(tree.to_path_buf()),
        branch: Some("feat/x".into()),
    };
    let runs = [
        run("run-2", &feature, on(&feature), 2, RunStatus::Running),
        run("run-1", &gone, on(&gone), 1, RunStatus::Red),
        run(
            "run-0",
            &main,
            repository::identify(&main),
            0,
            RunStatus::Green,
        ),
    ];
    let listing = branches(&runs, None, &mut Discovery::default());
    let [first, second] = listing.branches.as_slice() else {
        panic!("two branches: {:?}", listing.branches);
    };
    // The most recent Run first.
    assert_eq!(first.name, "feat/x");
    let ids: Vec<_> = first.runs.iter().map(|chip| chip.id.to_string()).collect();
    assert_eq!(ids, ["run-1", "run-2"], "oldest first");
    assert_eq!(
        first.running().map(|chip| chip.id.to_string()).as_deref(),
        Some("run-2")
    );
    assert_eq!(first.workspace, feature);
    assert_eq!(second.name, "main");
    assert_eq!(second.running(), None);
    // The removed folder's Run keeps its label, for a review that still waits on it.
    assert_eq!(
        listing.labels["run-1"],
        Label {
            project: "project".into(),
            branch: "feat/x".into()
        }
    );
}

#[test]
fn a_folder_outside_git_lives_while_it_exists_and_holds_declarations() {
    let root = crate::test_os::tempdir();
    let folder = root.path().join("plain");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("index.artf"), "name = \"plain\"\n").unwrap();
    let empty = root.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let removed = root.path().join("removed");
    let runs = [
        run("run-1", &folder, Identity::default(), 0, RunStatus::Green),
        run("run-2", &empty, Identity::default(), 1, RunStatus::Green),
        run("run-3", &removed, Identity::default(), 2, RunStatus::Green),
    ];
    let listing = branches(&runs, None, &mut Discovery::default());
    assert_eq!(names(&listing), [("plain".into(), String::new())]);
    assert_eq!(listing.branches[0].place, Place::Folder(folder.clone()));
}

#[test]
fn the_launch_folder_is_listed_before_its_project_has_a_run() {
    let root = crate::test_os::tempdir();
    let main = repository(root.path());
    let nested = main.join("docs");
    std::fs::create_dir(&nested).unwrap();
    let listing = branches(&[], Some(&nested), &mut Discovery::default());
    assert_eq!(names(&listing), [("project".into(), "main".into())]);
    assert_eq!(listing.launch.as_ref(), Some(&listing.branches[0].place));
    assert_eq!(
        listing.branches[0].workspace, nested,
        "scanned where it was started"
    );
    assert!(listing.branches[0].runs.is_empty());
}

fn branch(project: &str, name: &str, latest: Option<i64>) -> Branch {
    Branch {
        place: Place::Folder(format!("/{project}/{name}").into()),
        project: project.into(),
        name: name.into(),
        workspace: "/".into(),
        runs: latest
            .map(|day| {
                let chip = run(
                    "run-1",
                    Path::new("/"),
                    Identity::default(),
                    day,
                    RunStatus::Green,
                );
                vec![Chip {
                    id: chip.id,
                    status: chip.status,
                    created_at: chip.created_at,
                    completed_at: None,
                }]
            })
            .unwrap_or_default(),
    }
}

fn shown(query: &str, branches: &[Branch]) -> Vec<String> {
    let query: Query = query.parse().unwrap();
    show(&query, branches, &[])
        .0
        .into_iter()
        .map(|branch| format!("{} {}", branch.project, branch.name))
        .collect()
}

#[test]
fn the_search_narrows_by_project_and_branch_and_sorts_by_recency_or_name() {
    let branches = [
        branch("beta", "main", Some(1)),
        branch("alpha", "feat/search", None),
        branch("alpha", "main", Some(3)),
        branch("beta", "fix/alpha", Some(2)),
    ];
    assert_eq!(
        shown(DEFAULT, &branches),
        [
            "alpha main",
            "beta fix/alpha",
            "beta main",
            "alpha feat/search"
        ],
        "the most recent Run first, branches without Runs last"
    );
    assert_eq!(
        shown("sort:name", &branches),
        [
            "alpha feat/search",
            "alpha main",
            "beta fix/alpha",
            "beta main"
        ]
    );
    // Free text matches part of a project or a branch; repo: and branch: one field each.
    assert_eq!(
        shown("alpha sort:name", &branches),
        ["alpha feat/search", "alpha main", "beta fix/alpha"]
    );
    assert_eq!(
        shown("repo:alp sort:name", &branches),
        ["alpha feat/search", "alpha main"]
    );
    assert_eq!(shown("branch:ALPHA", &branches), ["beta fix/alpha"]);
    assert_eq!(shown("repo:beta main", &branches), ["beta main"]);
    for invalid in ["kind:human", "repo:", "sort:size", "sort:name sort:recent"] {
        assert!(invalid.parse::<Query>().is_err(), "{invalid}");
    }
    assert_eq!(
        shown("\"repo:beta\"", &branches),
        Vec::<String>::new(),
        "quoted is text"
    );
}

#[test]
fn the_search_completes_keys_projects_branches_and_orders() {
    let branches = [
        branch("alpha", "main", None),
        branch("beta", "feat/x", None),
    ];
    let candidates = |text: &str| complete(text, &branches, &[]).candidates;
    assert_eq!(candidates("sort:recent b"), ["branch:"]);
    assert_eq!(candidates("re"), ["repo:"]);
    assert_eq!(candidates("repo:"), ["repo:alpha", "repo:beta"]);
    assert_eq!(candidates("branch:fe"), ["branch:feat/x"]);
    assert_eq!(candidates("sort:"), ["sort:name", "sort:recent"]);
    assert_eq!(
        complete("sort:recent b", &branches, &[]).keep,
        "sort:recent "
    );
}
