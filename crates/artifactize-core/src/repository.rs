//! Git display identity, separate from the artifactize workspace used for execution.

use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::platform::path_serde::option"
    )]
    pub common_dir: Option<PathBuf>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::platform::path_serde::option"
    )]
    pub worktree_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

fn git(path: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let args = [OsStr::new("-C"), path.as_os_str()]
        .into_iter()
        .chain(args.iter().map(OsStr::new));
    crate::process::tool_output("git", args)
}

fn git_path(path: &Path, arg: &str) -> Option<PathBuf> {
    let output = git(path, &["rev-parse", "--path-format=absolute", arg])?;
    // Only the native output terminator is removed; path bytes remain the platform's own.
    let path = crate::platform::path_from_line(&output);
    crate::platform::canonicalize(&path).ok()
}

/// Missing paths and non-Git workspaces have no inferred Git identity.
pub fn identify(path: &Path) -> Identity {
    let common_dir = git_path(path, "--git-common-dir");
    let worktree_path = git_path(path, "--show-toplevel");
    if common_dir.is_none() || worktree_path.is_none() {
        return Identity::default();
    }
    let branch = git(path, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.trim_end_matches(['\r', '\n']).to_owned());
    Identity {
        common_dir,
        worktree_path,
        branch,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// The worktrees Git lists for `common_dir` that exist now: one Git marks prunable (its folder
/// was removed) or whose folder is gone is not a worktree any more. NUL-delimited porcelain does
/// not quote spaces, tabs or newlines in paths.
pub fn worktrees(common_dir: &Path) -> Vec<Worktree> {
    let args = [OsStr::new("--git-dir"), common_dir.as_os_str()]
        .into_iter()
        .chain(["worktree", "list", "--porcelain", "-z"].map(OsStr::new));
    crate::process::tool_output("git", args)
        .map(|output| parse_worktrees(&output))
        .unwrap_or_default()
        .into_iter()
        .filter(|tree| tree.path.try_exists().unwrap_or(false))
        .collect()
}

fn parse_worktrees(bytes: &[u8]) -> Vec<Worktree> {
    let mut trees: Vec<(Worktree, bool)> = Vec::new();
    for field in bytes.split(|byte| *byte == 0) {
        if let Some(path) = field.strip_prefix(b"worktree ") {
            let path = crate::platform::path_from_bytes(path);
            trees.push((Worktree { path, branch: None }, false));
        } else if let Some((tree, _)) = trees.last_mut()
            && let Some(branch) = field.strip_prefix(b"branch refs/heads/")
        {
            tree.branch = Some(String::from_utf8_lossy(branch).into_owned());
        } else if let Some((_, prunable)) = trees.last_mut()
            && (field == b"prunable" || field.starts_with(b"prunable "))
        {
            *prunable = true;
        }
    }
    trees
        .into_iter()
        .filter_map(|(tree, prunable)| (!prunable).then_some(tree))
        .collect()
}

/// A commit in Git's topological history, including every parent and decorating ref.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub refs: Vec<String>,
    pub summary: String,
    /// Unix seconds, represented as text so JavaScript never rounds an identifier or time.
    pub time: String,
}

pub fn history(worktree: &Path) -> Result<Vec<Commit>, String> {
    let bytes = git(
        worktree,
        &[
            "log",
            // --all includes HEAD when it exists, without requiring an unborn HEAD.
            "--all",
            "--topo-order",
            "--format=%H%x00%P%x00%D%x00%s%x00%ct%x00",
        ],
    )
    .ok_or_else(|| "Cannot read Git history".to_owned())?;
    let text = String::from_utf8_lossy(&bytes);
    let fields: Vec<_> = text.split('\0').collect();
    Ok(fields
        .as_chunks::<5>()
        .0
        .iter()
        .map(|row| Commit {
            id: row[0].trim_start_matches(['\r', '\n']).to_owned(),
            parents: row[1].split_whitespace().map(str::to_owned).collect(),
            refs: row[2]
                .split(", ")
                .filter(|r| !r.is_empty())
                .map(str::to_owned)
                .collect(),
            summary: row[3].to_owned(),
            time: row[4].to_owned(),
        })
        .collect())
}

/// A worktree's Git administrative directory is independent of its current branch.
/// The primary checkout uses its common directory; linked worktrees have their own git-dir.
pub fn worktree_id(path: &Path) -> Result<PathBuf, String> {
    git_path(path, "--git-dir").ok_or_else(|| "Not a Git worktree".to_owned())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CreatedWorktree {
    #[serde(with = "crate::platform::path_serde")]
    #[cfg_attr(feature = "desktop-types", specta(type = String))]
    pub worktree: PathBuf,
    pub branch: String,
}

pub fn create_worktree(worktree: &Path, commit: &str) -> Result<CreatedWorktree, String> {
    let identity = identify(worktree);
    let common = identity.common_dir.ok_or("Not a Git worktree")?;
    let primary = worktrees(&common)
        .into_iter()
        .next()
        .ok_or("No primary checkout")?;
    let parent = primary
        .path
        .parent()
        .ok_or("The primary checkout has no parent")?;
    // Git resolves the revision before it is used as a positional argument to worktree add.
    let revision = git(
        worktree,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{commit}^{{commit}}"),
        ],
    )
    .ok_or("Cannot resolve the selected commit")?;
    let revision = String::from_utf8_lossy(&revision);
    loop {
        let random = getrandom::u64().map_err(|e| e.to_string())?;
        let branch = format!("worktree-{random:016x}");
        let path = parent.join(&branch);
        if path.exists()
            || git(
                worktree,
                &["show-ref", "--verify", &format!("refs/heads/{branch}")],
            )
            .is_some()
        {
            continue;
        }
        let args = [
            OsStr::new("-C"),
            worktree.as_os_str(),
            OsStr::new("worktree"),
            OsStr::new("add"),
            OsStr::new("-b"),
            OsStr::new(&branch),
            OsStr::new("--"),
            path.as_os_str(),
            OsStr::new(revision.trim()),
        ];
        crate::process::tool_output("git", args).ok_or("Git could not create the worktree")?;
        return Ok(CreatedWorktree {
            worktree: path,
            branch,
        });
    }
}

/// Git refuses a primary, locked or dirty checkout; removal never silently discards work.
pub fn remove_worktree(worktree: &Path) -> Result<(), String> {
    let identity = identify(worktree);
    let common = identity.common_dir.ok_or("Not a Git worktree")?;
    crate::process::tool_output(
        "git",
        [
            OsStr::new("--git-dir"),
            common.as_os_str(),
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--"),
            worktree.as_os_str(),
        ],
    )
    .ok_or_else(|| {
        "Git refused to remove the worktree (it may be primary, locked or dirty)".to_owned()
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_identity_preserves_supported_path_characters() {
        let root = crate::test_os::tempdir();
        for path in crate::test_os::git_path_cases(root.path()) {
            crate::test_os::git(&path, &["init", "--quiet"]);
            let identity = identify(&path);
            assert_eq!(
                identity.common_dir,
                Some(crate::test_os::canonical(&path.join(".git"))),
                "{path:?}"
            );
            assert_eq!(
                identity.worktree_path,
                Some(crate::test_os::canonical(&path)),
                "{path:?}"
            );
        }
    }

    #[test]
    fn porcelain_paths_are_not_split_on_whitespace() {
        let root = crate::test_os::tempdir();
        // Parser input only: no directory with an embedded newline is created.
        let named = root.path().join("a space\nline");
        let detached = root.path().join("detached");
        let wire = format!(
            "worktree {}\0HEAD abc\0branch refs/heads/main\0\0worktree {}\0HEAD def\0detached\0\0",
            crate::platform::path_text(&named),
            crate::platform::path_text(&detached),
        );
        let trees = parse_worktrees(wire.as_bytes());
        assert_eq!(trees[0].path, named);
        assert_eq!(trees[0].branch.as_deref(), Some("main"));
        assert_eq!(trees[1].path, detached);
        assert_eq!(trees[1].branch, None);
    }

    #[test]
    fn prunable_worktrees_are_not_listed() {
        let wire = "worktree /work/main\0HEAD abc\0branch refs/heads/main\0\0worktree /work/gone\0HEAD def\0branch refs/heads/gone\0prunable gitdir file points to non-existent location\0\0";
        let trees = parse_worktrees(wire.as_bytes());
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].branch.as_deref(), Some("main"));
    }

    #[test]
    fn a_removed_worktree_folder_is_not_listed() {
        let root = crate::test_os::tempdir();
        let main = root.path().join("main");
        std::fs::create_dir(&main).unwrap();
        crate::test_os::git(&main, &["init", "--quiet", "--initial-branch=main"]);
        crate::test_os::git(
            &main,
            &[
                "-c",
                "user.name=artifactize",
                "-c",
                "user.email=artifactize@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "first",
            ],
        );
        let other = root.path().join("other");
        crate::test_os::git(
            &main,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "other",
                &crate::platform::path_text(&other),
            ],
        );
        let common = crate::test_os::canonical(&main.join(".git"));
        let branches = |trees: Vec<Worktree>| -> Vec<Option<String>> {
            trees.into_iter().map(|tree| tree.branch).collect()
        };
        assert_eq!(
            branches(worktrees(&common)),
            [Some("main".into()), Some("other".into())]
        );
        std::fs::remove_dir_all(&other).unwrap();
        assert_eq!(branches(worktrees(&common)), [Some("main".into())]);
    }

    /// Git with a fixed identity, so commits never depend on the machine's configuration.
    fn committing(path: &Path, arguments: &[&str]) {
        let mut all = vec![
            "-c",
            "user.name=artifactize",
            "-c",
            "user.email=artifactize@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ];
        all.extend_from_slice(arguments);
        crate::test_os::git(path, &all);
    }

    fn commit(path: &Path, message: &str) -> String {
        committing(path, &["commit", "--quiet", "--allow-empty", "-m", message]);
        head(path)
    }

    /// Read-only observations of a checkout through Git itself.
    fn head(path: &Path) -> String {
        output(path, &["rev-parse", "HEAD"])
    }
    fn output(path: &Path, arguments: &[&str]) -> String {
        let bytes = git(path, arguments).expect("Git answers");
        String::from_utf8(bytes).unwrap().trim_end().to_owned()
    }

    /// The primary checkout `root/project` on `main`, with a first and a second commit.
    fn project(root: &Path) -> (PathBuf, String, String) {
        let main = root.join("project");
        std::fs::create_dir(&main).unwrap();
        crate::test_os::git(&main, &["init", "--quiet", "--initial-branch=main"]);
        let first = commit(&main, "first");
        let second = commit(&main, "second");
        (main, first, second)
    }

    #[test]
    fn a_worktree_from_a_commit_lands_beside_the_primary_on_a_new_branch_and_removal_removes_it() {
        let root = crate::test_os::tempdir();
        let (main, first, _) = project(root.path());
        let common = crate::test_os::canonical(&main.join(".git"));
        let created = create_worktree(&main, &first).unwrap();
        assert_eq!(
            created.worktree.parent(),
            Some(crate::test_os::canonical(root.path()).as_path())
        );
        assert_eq!(head(&created.worktree), first);
        assert_eq!(
            output(&created.worktree, &["symbolic-ref", "--short", "HEAD"]),
            created.branch
        );
        assert_ne!(created.branch, "main");
        // The same commit again never reuses the folder or the branch of the first one.
        let again = create_worktree(&main, &first).unwrap();
        assert_ne!(again.worktree, created.worktree);
        assert_ne!(again.branch, created.branch);
        let listed = |trees: Vec<Worktree>| -> Vec<PathBuf> {
            trees.into_iter().map(|tree| tree.path).collect()
        };
        assert!(listed(worktrees(&common)).contains(&created.worktree));

        remove_worktree(&created.worktree).unwrap();
        assert!(!created.worktree.exists());
        assert!(!listed(worktrees(&common)).contains(&created.worktree));
        assert!(listed(worktrees(&common)).contains(&again.worktree));
    }

    #[test]
    fn removal_refuses_the_primary_checkout_and_uncommitted_work_and_keeps_them() {
        let root = crate::test_os::tempdir();
        let (main, first, second) = project(root.path());
        let created = create_worktree(&main, &first).unwrap();
        let work = created.worktree.join("unsaved.txt");
        std::fs::write(&work, "work in progress").unwrap();
        assert!(remove_worktree(&created.worktree).is_err());
        assert_eq!(std::fs::read_to_string(&work).unwrap(), "work in progress");
        assert!(remove_worktree(&main).is_err());
        assert_eq!(head(&main), second);
    }

    #[test]
    fn a_commit_that_is_not_in_the_repository_creates_no_worktree() {
        let root = crate::test_os::tempdir();
        let (main, _, _) = project(root.path());
        let before: Vec<_> = std::fs::read_dir(root.path()).unwrap().collect();
        assert!(create_worktree(&main, &"0".repeat(40)).is_err());
        assert!(create_worktree(&main, "--orphan").is_err());
        let after: Vec<_> = std::fs::read_dir(root.path()).unwrap().collect();
        assert_eq!(before.len(), after.len());
    }

    #[test]
    fn history_lists_every_branch_with_parents_and_refs_children_first() {
        let root = crate::test_os::tempdir();
        let (main, first, second) = project(root.path());
        crate::test_os::git(&main, &["switch", "--quiet", "-c", "side", &first]);
        let third = commit(&main, "third");
        crate::test_os::git(&main, &["switch", "--quiet", "main"]);
        committing(
            &main,
            &["merge", "--quiet", "--no-ff", "-m", "merge side", "side"],
        );
        let merge = head(&main);
        let commits = history(&main).unwrap();
        let find = |id: &str| -> (usize, &Commit) {
            commits
                .iter()
                .enumerate()
                .find(|(_, commit)| commit.id == id)
                .unwrap_or_else(|| panic!("{id} is listed"))
        };
        let (merged_at, merged) = find(&merge);
        let (second_at, second_commit) = find(&second);
        let (third_at, third_commit) = find(&third);
        let (first_at, first_commit) = find(&first);
        assert_eq!(commits.len(), 4);
        assert_eq!(merged.parents, [second.as_str(), third.as_str()]);
        assert_eq!(merged.summary, "merge side");
        assert!(merged.refs.contains(&"HEAD -> main".to_owned()));
        assert_eq!(second_commit.parents, [first.as_str()]);
        assert_eq!(third_commit.parents, [first.as_str()]);
        assert!(third_commit.refs.contains(&"side".to_owned()));
        assert!(first_commit.parents.is_empty());
        assert!(merged_at < second_at.min(third_at));
        assert!(second_at.max(third_at) < first_at);
    }
}
