//! Git display identity, separate from the artifactize workspace used for execution.

use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub common_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// Git, found through the shared program lookup; `None` when it is not installed.
fn git_command() -> Option<Command> {
    let cwd = std::env::current_dir().ok()?;
    artifactize_tools::program::resolve(std::ffi::OsStr::new("git"), &cwd)
        .ok()
        .map(Command::new)
}

fn git(path: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = git_command()?
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn git_path(path: &Path, arg: &str) -> Option<PathBuf> {
    let output = git(path, &["rev-parse", "--path-format=absolute", arg])?;
    let text = String::from_utf8(output).ok()?;
    // rev-parse emits one trailing newline; embedded newlines belong to the path.
    let path = PathBuf::from(
        text.strip_suffix("\r\n")
            .or_else(|| text.strip_suffix('\n'))
            .unwrap_or(&text),
    );
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

/// NUL-delimited porcelain does not quote spaces, tabs or newlines in paths.
pub fn worktrees(common_dir: &Path) -> Vec<Worktree> {
    let Some(mut git) = git_command() else {
        return Vec::new();
    };
    let output = git
        .arg("--git-dir")
        .arg(common_dir)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output();
    match output {
        Ok(output) if output.status.success() => parse_worktrees(&output.stdout),
        _ => Vec::new(),
    }
}

fn parse_worktrees(bytes: &[u8]) -> Vec<Worktree> {
    let mut trees: Vec<Worktree> = Vec::new();
    for field in bytes.split(|byte| *byte == 0) {
        if let Some(path) = field.strip_prefix(b"worktree ") {
            let path = crate::platform::path_from_bytes(path);
            trees.push(Worktree { path, branch: None });
        } else if let Some(branch) = field.strip_prefix(b"branch refs/heads/")
            && let Some(tree) = trees.last_mut()
        {
            tree.branch = Some(String::from_utf8_lossy(branch).into_owned());
        }
    }
    trees
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn porcelain_paths_are_not_split_on_whitespace() {
        let trees = parse_worktrees(
            b"worktree /a space\nline\0HEAD abc\0branch refs/heads/main\0\0worktree /detached\0HEAD def\0detached\0\0",
        );
        assert_eq!(trees[0].path, PathBuf::from("/a space\nline"));
        assert_eq!(trees[0].branch.as_deref(), Some("main"));
        assert_eq!(trees[1].branch, None);
    }
}
