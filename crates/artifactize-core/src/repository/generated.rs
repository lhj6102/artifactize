//! Cleanup belongs only to the temporary branch this worktree was created with. A name
//! pattern alone cannot establish ownership: another tool may generate the same kind of name.
use crate::platform;
use std::{
    ffi::OsStr,
    io::{Read, Write},
    path::Path,
};

/// Kept in the worktree's Git administrative directory, never in its tracked working files.
const ORIGINAL_BRANCH: &str = "artifactize-created-branch";

pub(super) fn record(worktree: &Path, branch: &str) -> Result<(), String> {
    let directory = super::worktree_id(worktree)?;
    let mut file = platform::open_no_follow(
        platform::private_options().write(true).create_new(true),
        &directory.join(ORIGINAL_BRANCH),
    )
    .map_err(|error| error.to_string())?;
    file.write_all(branch.as_bytes())
        .map_err(|error| error.to_string())
}

pub(super) struct Branch(String);
impl Branch {
    pub fn read(worktree: &Path, current: &str) -> Option<Self> {
        let directory = super::worktree_id(worktree).ok()?;
        let mut file = platform::open_no_follow(
            platform::private_options().read(true),
            &directory.join(ORIGINAL_BRANCH),
        )
        .ok()?;
        let mut original = String::new();
        file.read_to_string(&mut original).ok()?;
        if original != current {
            return None;
        }
        let reference = format!("refs/heads/{original}");
        let log = super::git(worktree, &["reflog", "show", "--format=%gs", &reference])?;
        let log = std::str::from_utf8(&log).ok()?;
        // A rename away and back is still a rename. If the creation record was expired or
        // removed, keep the branch rather than inferring that the missing history was safe.
        if log.lines().any(|line| line.starts_with("Branch: renamed "))
            || !log.lines().last()?.starts_with("branch: Created from ")
        {
            return None;
        }
        Some(Self(original))
    }

    pub fn remove_if_shared(self, common: &Path) {
        let reference = format!("refs/heads/{}", self.0);
        // If another direct ref contains the tip, it also reaches every ancestor of that
        // tip. Symbolic aliases of this branch would not keep its commits after deletion.
        let Some(refs) = git(
            common,
            &[
                "for-each-ref",
                &format!("--contains={reference}"),
                "--format=%(refname)%00%(symref)",
            ],
        ) else {
            return;
        };
        let Ok(refs) = std::str::from_utf8(&refs) else {
            return;
        };
        let Some((covering, _)) = refs
            .lines()
            .filter_map(|line| line.split_once('\0'))
            .find(|(name, symbolic)| *name != reference && symbolic.is_empty())
        else {
            return;
        };
        // -d (never -D) makes Git recheck merge safety and refuse a branch checked out by
        // another worktree. A command-local upstream lets tags and other refs, not just
        // primary HEAD, establish that the commits remain reachable. No config is saved.
        let _ = git(
            common,
            &[
                "-c",
                &format!("branch.{}.remote=.", self.0),
                "-c",
                &format!("branch.{}.merge={covering}", self.0),
                "branch",
                "--delete",
                "--",
                &self.0,
            ],
        );
    }
}

fn git(common: &Path, arguments: &[&str]) -> Option<Vec<u8>> {
    crate::process::tool_output(
        "git",
        [OsStr::new("--git-dir"), common.as_os_str()]
            .into_iter()
            .chain(arguments.iter().map(OsStr::new)),
    )
}
