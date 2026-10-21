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
}
