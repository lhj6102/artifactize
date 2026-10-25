//! The live branches of every project with Runs on this machine, as the desktop's pane lists
//! them: each Git worktree that exists now with the branch it has checked out (a detached one by
//! its folder), and each folder outside Git that exists now and holds declarations. A branch's
//! Runs are its project's Runs on that branch, from whichever folder they ran in; who ran them
//! makes no branch of its own. The terminal monitor keeps its own catalog of scopes.
mod current;
mod loader;
mod query;
mod starred;
mod waiting;

#[cfg(test)]
mod tests;

pub use current::{Current, CurrentArtifact, CurrentEdge, CurrentEval, Plan, scan};
pub use loader::{Input, Loader, Opened, Snapshot};
pub use query::{DEFAULT, Query, Sort, complete, show};
pub use starred::{star, starred};
pub use waiting::{Waiting, waiting};

use crate::{
    repository::{self, Identity, Worktree},
    store::CatalogRun,
    types::{RunId, RunStatus, Timestamp},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// A branch's name as Git accepts one: not empty, and none of the characters or sequences
/// `git check-ref-format --branch` refuses. Made at the Git and serde boundaries, so no other
/// name becomes a branch's key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BranchName(String);

impl BranchName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BranchName {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        // Each `/`-separated part is a name of its own: not empty, not starting with `.`, not
        // ending with `.lock`.
        let refused = name.starts_with('-')
            || name.ends_with('.')
            || name == "@"
            || name.contains("..")
            || name.contains("@{")
            || name
                .split('/')
                .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
            || name.chars().any(|c| {
                c.is_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
            });
        if refused {
            Err(format!("not a branch name: {name:?}"))
        } else {
            Ok(Self(name))
        }
    }
}

impl From<BranchName> for String {
    fn from(name: BranchName) -> Self {
        name.0
    }
}

impl std::fmt::Display for BranchName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a Git worktree has its project checked out.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Head {
    Branch(BranchName),
    /// A detached worktree, known by its folder.
    Detached(#[serde(with = "crate::platform::path_serde")] PathBuf),
}

/// Where a branch lives: a Git repository's checked-out branch, by the repository's common
/// directory, or a folder outside Git.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Place {
    Git {
        #[serde(with = "crate::platform::path_serde")]
        common: PathBuf,
        head: Head,
    },
    Folder(#[serde(with = "crate::platform::path_serde")] PathBuf),
}

/// One Run of a branch as its chip shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub id: RunId,
    pub status: RunStatus,
    pub created_at: Timestamp,
    pub completed_at: Option<Timestamp>,
    /// What the requester said it is working on (`verify --comment`).
    pub comment: Option<crate::types::RunComment>,
}

/// A live branch and its Runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub place: Place,
    /// The project's name: its repository's or folder's.
    pub project: String,
    /// The branch, a detached worktree's folder name, or empty for a folder outside Git.
    pub name: String,
    /// Where its Current is scanned and its Artifacts' tools run: the worktree (or folder) as it
    /// is now, at the workspace its newest Run used (or the app was started for) within it.
    pub workspace: PathBuf,
    /// Its Runs, oldest first.
    pub runs: Vec<Chip>,
}

impl Branch {
    /// The Run in progress a branch's timeline starts with: the newest Run, while it runs.
    pub fn running(&self) -> Option<&Chip> {
        self.runs.last().filter(|chip| chip.status.is_open())
    }

    /// When its newest Run started.
    pub fn latest(&self) -> Option<Timestamp> {
        self.runs.last().map(|chip| chip.created_at)
    }
}

/// Which project and branch a saved Run ran on, by name, for every Run whether or not its
/// branch still lives: what a waiting review of it is listed with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Label {
    pub project: String,
    pub branch: String,
}

pub type Labels = BTreeMap<RunId, Label>;

/// Cached Git facts. Identities of old Run records without a saved identity are asked of Git
/// once; which worktrees exist is asked again on every [`Discovery::rescan`].
#[derive(Default)]
pub struct Discovery {
    identities: BTreeMap<PathBuf, Identity>,
    trees: BTreeMap<PathBuf, Vec<Worktree>>,
}

impl Discovery {
    /// Ask Git again which worktrees exist, on the next listing.
    pub fn rescan(&mut self) {
        self.trees.clear();
    }

    fn identify(&mut self, path: &Path) -> Identity {
        self.identities
            .entry(path.to_path_buf())
            .or_insert_with(|| repository::identify(path))
            .clone()
    }

    fn worktrees(&mut self, common: &Path) -> Vec<Worktree> {
        self.trees
            .entry(common.to_path_buf())
            .or_insert_with(|| {
                repository::worktrees(common)
                    .into_iter()
                    .map(|tree| Worktree {
                        // Saved identities are canonical; a detached head is matched by path.
                        path: crate::platform::canonicalize(&tree.path).unwrap_or(tree.path),
                        branch: tree.branch,
                    })
                    .collect()
            })
            .clone()
    }
}

/// Where `identity` (of the workspace `repo`) lives, and `repo` relative to its worktree.
/// The head of the worktree at `tree` with `branch` checked out, or detached without one. A
/// name Git reports that is no branch name is known by the worktree's folder instead.
fn head(branch: Option<String>, tree: &Path) -> Head {
    branch
        .and_then(|name| BranchName::try_from(name).ok())
        .map_or_else(|| Head::Detached(tree.to_path_buf()), Head::Branch)
}

fn place_of(identity: &Identity, repo: &Path) -> (Place, PathBuf) {
    match (&identity.common_dir, &identity.worktree_path) {
        (Some(common), Some(tree)) => {
            let head = head(identity.branch.clone(), tree);
            let relative = repo
                .strip_prefix(tree)
                .map(Path::to_path_buf)
                .unwrap_or_default();
            (
                Place::Git {
                    common: common.clone(),
                    head,
                },
                relative,
            )
        }
        _ => (Place::Folder(repo.to_path_buf()), PathBuf::new()),
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || crate::platform::path_text(path),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The project's name: a repository's main folder (the parent of its `.git`), a bare
/// repository's own folder without `.git`, or the folder outside Git.
fn project_name(place: &Place) -> String {
    match place {
        Place::Git { common, .. } if common.file_name().is_some_and(|name| name == ".git") => {
            folder_name(common.parent().unwrap_or(common))
        }
        Place::Git { common, .. } => {
            let name = folder_name(common);
            name.strip_suffix(".git")
                .map_or(name.clone(), str::to_owned)
        }
        Place::Folder(path) => folder_name(path),
    }
}

fn branch_name(place: &Place) -> String {
    match place {
        Place::Git {
            head: Head::Branch(branch),
            ..
        } => branch.to_string(),
        Place::Git {
            head: Head::Detached(tree),
            ..
        } => folder_name(tree),
        Place::Folder(_) => String::new(),
    }
}

fn label(place: &Place) -> Label {
    Label {
        project: project_name(place),
        branch: branch_name(place),
    }
}

/// What the listing of the branches found.
pub struct Listing {
    /// The live branches, the most recent Run first.
    pub branches: Vec<Branch>,
    pub labels: Labels,
    /// The branch of the folder the app was started for, when it lives.
    pub launch: Option<Place>,
}

/// A branch's Runs so far, and when its newest started with its workspace in the worktree.
type Grouped = (Vec<Chip>, Option<(Timestamp, PathBuf)>);

/// The live branches of the projects of `runs`, with their Runs, and the branch of `launch`
/// (the folder the app was started for) even before its project has a Run.
pub fn branches(runs: &[CatalogRun], launch: Option<&Path>, discovery: &mut Discovery) -> Listing {
    let mut grouped: BTreeMap<Place, Grouped> = BTreeMap::new();
    let mut labels = Labels::new();
    for run in runs {
        let identity =
            if run.repository.common_dir.is_some() && run.repository.worktree_path.is_some() {
                run.repository.clone()
            } else {
                discovery.identify(&run.repo_path)
            };
        let (place, relative) = place_of(&identity, &run.repo_path);
        labels.insert(run.id.clone(), label(&place));
        let (chips, newest) = grouped.entry(place).or_default();
        chips.push(Chip {
            id: run.id.clone(),
            status: run.status,
            created_at: run.created_at,
            completed_at: run.completed_at,
            comment: run.comment.clone(),
        });
        if newest.as_ref().is_none_or(|(at, _)| run.created_at >= *at) {
            *newest = Some((run.created_at, relative));
        }
    }
    let launched = launch.map(|path| {
        let (place, _) = place_of(&discovery.identify(path), path);
        (place, path.to_path_buf())
    });
    let commons: BTreeSet<PathBuf> = grouped
        .keys()
        .chain(launched.iter().map(|(place, _)| place))
        .filter_map(|place| match place {
            Place::Git { common, .. } => Some(common.clone()),
            Place::Folder(_) => None,
        })
        .collect();
    let mut live: Vec<(Place, PathBuf)> = commons
        .into_iter()
        .flat_map(|common| {
            discovery.worktrees(&common).into_iter().map(move |tree| {
                let head = head(tree.branch, &tree.path);
                (
                    Place::Git {
                        common: common.clone(),
                        head,
                    },
                    tree.path,
                )
            })
        })
        .collect();
    let folders: BTreeSet<&PathBuf> = grouped
        .keys()
        .chain(launched.iter().map(|(place, _)| place))
        .filter_map(|place| match place {
            Place::Folder(path) => Some(path),
            Place::Git { .. } => None,
        })
        .collect();
    // A Run's folder is live while it still holds declarations; the folder the app was started
    // for is live while it exists, as the CLI takes any folder as `--repo`, its Artifacts
    // perhaps only in subfolders.
    let started = |path: &PathBuf| {
        launched
            .as_ref()
            .is_some_and(|(place, _)| matches!(place, Place::Folder(folder) if folder == path))
    };
    live.extend(
        folders
            .into_iter()
            .filter(|path| {
                path.is_dir()
                    && (started(path)
                        || crate::workspace::has_artifact_marker(path).unwrap_or(false))
            })
            .map(|path| (Place::Folder(path.clone()), path.clone())),
    );
    let mut branches: Vec<Branch> = live
        .into_iter()
        .map(|(place, tree)| {
            let (mut runs, newest) = grouped.remove(&place).unwrap_or_default();
            runs.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
            let workspace = match &launched {
                Some((launch, path)) if *launch == place => path.clone(),
                _ => newest
                    .map(|(_, relative)| tree.join(relative))
                    .filter(|path| path.is_dir())
                    .unwrap_or(tree),
            };
            let Label { project, branch } = label(&place);
            Branch {
                place,
                project,
                name: branch,
                workspace,
                runs,
            }
        })
        .collect();
    query::order(&mut branches, Sort::Recent);
    let launch = launched
        .map(|(place, _)| place)
        .filter(|place| branches.iter().any(|branch| &branch.place == place));
    Listing {
        branches,
        labels,
        launch,
    }
}
