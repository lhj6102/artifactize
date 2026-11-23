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
pub use loader::{Input, Loader, OpenRun, Opened, Snapshot};
pub use query::{DEFAULT, Query, Sort, complete, show};
pub use starred::{star, star_artifact, starred, starred_artifacts};
pub use waiting::{Waiting, waiting};

use crate::{
    platform::{FileIdentity, paths},
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

impl Place {
    /// A saved or live checkout's branch. Both the workspace and Git identity are canonical.
    pub(crate) fn for_checkout(workspace: &Path, identity: &Identity) -> Self {
        match (&identity.common_dir, &identity.worktree_path) {
            (Some(common), Some(tree)) => Self::Git {
                common: common.clone(),
                head: checkout_head(identity.branch.clone(), || tree.clone()),
            },
            _ => Self::Folder(workspace.to_owned()),
        }
    }

    /// The Run's original branch, even when no worktree currently has it checked out.
    pub fn recorded_run(run: &crate::store::Run) -> Self {
        Self::for_checkout(&run.repo_path, &run.repository)
    }

    /// Refuse a stale path rather than scanning or editing a different branch.
    pub async fn checked_workspace(&self, workspace: PathBuf) -> Result<PathBuf, String> {
        let place = self.clone();
        crate::task::blocking(move || {
            if !workspace.is_dir() {
                return Err("This worktree no longer exists.".into());
            }
            let path =
                crate::platform::canonicalize(&workspace).map_err(|error| error.to_string())?;
            if Self::for_checkout(&path, &repository::identify(&path)) != place {
                return Err("This branch is no longer checked out in the worktree.".into());
            }
            Ok(path)
        })
        .await?
    }

    /// The project the branch belongs to: every branch of a Git repository shares one.
    pub fn project(&self) -> Project {
        match self {
            Self::Git { common, .. } => Project::Git(common.clone()),
            Self::Folder(path) => Project::Folder(path.clone()),
        }
    }
}

/// A project: a Git repository, by its common directory, or a folder outside Git.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Project {
    Git(#[serde(with = "crate::platform::path_serde")] PathBuf),
    Folder(#[serde(with = "crate::platform::path_serde")] PathBuf),
}

impl Project {
    pub fn path(&self) -> &Path {
        match self {
            Self::Git(path) | Self::Folder(path) => path,
        }
    }
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
    /// A saved Run on an absent branch, shown only while following its exact reference.
    pub fn historical(run: &crate::store::Run) -> Self {
        let place = Place::recorded_run(run);
        let Label {
            project,
            branch: name,
            ..
        } = label(&place);
        Self {
            place,
            project,
            name,
            workspace: run.repo_path.clone(),
            runs: vec![Chip {
                id: run.id.clone(),
                status: run.status(),
                created_at: run.created_at,
                completed_at: run.completed_at(),
                comment: run.comment.clone(),
            }],
        }
    }

    /// In-progress Runs first, then ended Runs, newest first within each group.
    pub fn timeline(&self) -> impl Iterator<Item = &Chip> {
        let newest = self.runs.iter().rev();
        newest
            .clone()
            .filter(|chip| chip.status.is_open())
            .chain(newest.filter(|chip| !chip.status.is_open()))
    }

    /// The newest Run still in progress, even if a later one already ended.
    pub fn running(&self) -> Option<&Chip> {
        self.runs.iter().rev().find(|chip| chip.status.is_open())
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
    pub project_key: Option<Project>,
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

    /// Whether `path` is a folder now.
    fn is_folder(&self, path: &Path) -> bool {
        crate::platform::is_directory(path)
    }

    /// Whether the folder `path` holds declarations.
    fn declares(&self, path: &Path) -> bool {
        crate::workspace::has_artifact_marker(path).unwrap_or(false)
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

/// Pick one usable spelling per physical directory for this listing. Read each distinct
/// saved spelling only once, even if many Runs were saved from it.
#[derive(Default)]
struct DirectoryKeys {
    by_path: BTreeMap<PathBuf, PathBuf>,
    by_identity: BTreeMap<FileIdentity, PathBuf>,
}

impl DirectoryKeys {
    fn resolve(&mut self, path: &Path) -> PathBuf {
        if let Some(key) = self.by_path.get(path) {
            return key.clone();
        }
        let key = paths::directory_identity(path).map_or_else(
            |_| path.to_owned(),
            |(id, canonical)| self.by_identity.entry(id).or_insert(canonical).clone(),
        );
        self.by_path.insert(path.to_owned(), key.clone());
        key
    }

    fn head(&mut self, branch: Option<String>, tree: &Path) -> Head {
        checkout_head(branch, || self.resolve(tree))
    }
}

fn checkout_head(branch: Option<String>, detached: impl FnOnce() -> PathBuf) -> Head {
    branch
        .and_then(|name| BranchName::try_from(name).ok())
        .map_or_else(|| Head::Detached(detached()), Head::Branch)
}

/// Where `identity` (of the workspace `repo`) lives, and `repo` relative to its worktree.
/// The head of the worktree at `tree` with `branch` checked out, or detached without one. A
/// name Git reports that is no branch name is known by the worktree's folder instead.
fn place_of(identity: &Identity, repo: &Path, keys: &mut DirectoryKeys) -> (Place, PathBuf) {
    match (&identity.common_dir, &identity.worktree_path) {
        (Some(common), Some(tree)) => {
            let head = keys.head(identity.branch.clone(), tree);
            let relative = paths::relative_beneath(repo, tree).unwrap_or_default();
            (
                Place::Git {
                    common: keys.resolve(common),
                    head,
                },
                relative,
            )
        }
        _ => (Place::Folder(keys.resolve(repo)), PathBuf::new()),
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || crate::platform::path_text(path),
        crate::platform::display_name,
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
        project_key: Some(place.project()),
        branch: branch_name(place),
    }
}

impl Label {
    /// Name a worktree whose Run is not in the current monitor snapshot.
    pub fn at(repo: &Path) -> Self {
        Self::for_identity(repo, &crate::repository::identify(repo))
    }

    /// Name the saved Git head of a worktree, not the branch checked out there today.
    pub fn for_identity(repo: &Path, identity: &crate::repository::Identity) -> Self {
        let (place, _) = place_of(identity, repo, &mut DirectoryKeys::default());
        label(&place)
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

/// Resolve only one Run's live branch using the same identities as the timeline listing.
fn branch_for(run: &crate::store::Run, discovery: &mut Discovery) -> Option<Place> {
    let row = CatalogRun {
        id: run.id.clone(),
        created_at: run.created_at,
        completed_at: run.completed_at(),
        repo_path: run.repo_path.clone(),
        repository: run.repository.clone(),
        status: run.status(),
        comment: run.comment.clone(),
        red: 0,
        error: 0,
        waiting: Vec::new(),
    };
    branches(&[row], None, discovery)
        .branches
        .into_iter()
        .find(|branch| branch.runs.iter().any(|chip| chip.id == run.id))
        .map(|branch| branch.place)
}

/// The live branches of the projects of `runs`, with their Runs, and the branch of `launch`
/// (the folder the app was started for) even before its project has a Run.
pub fn branches(runs: &[CatalogRun], launch: Option<&Path>, discovery: &mut Discovery) -> Listing {
    let mut grouped: BTreeMap<Place, Grouped> = BTreeMap::new();
    let mut labels = Labels::new();
    let mut keys = DirectoryKeys::default();
    let launched = launch.map(|path| {
        let (place, _) = place_of(&discovery.identify(path), path, &mut keys);
        (place, path.to_path_buf())
    });
    for run in runs {
        let identity =
            if run.repository.common_dir.is_some() && run.repository.worktree_path.is_some() {
                run.repository.clone()
            } else {
                discovery.identify(&run.repo_path)
            };
        let (place, relative) = place_of(&identity, &run.repo_path, &mut keys);
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
    let commons: BTreeSet<PathBuf> = grouped
        .keys()
        .chain(launched.iter().map(|(place, _)| place))
        .filter_map(|place| match place {
            Place::Git { common, .. } => Some(common.clone()),
            Place::Folder(_) => None,
        })
        .collect();
    let mut live = Vec::new();
    for common in commons {
        live.extend(discovery.worktrees(&common).into_iter().map(|tree| {
            let head = keys.head(tree.branch, &tree.path);
            (
                Place::Git {
                    common: common.clone(),
                    head,
                },
                tree.path,
            )
        }));
    }
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
            .filter(|path| discovery.is_folder(path) && (started(path) || discovery.declares(path)))
            .map(|path| (Place::Folder(path.clone()), path.clone())),
    );
    // Which newest-Run folders still exist, as the filesystem boundary reports.
    let present: BTreeSet<PathBuf> = live
        .iter()
        .filter_map(|(place, tree)| {
            let (_, newest) = grouped.get(place)?;
            let folder = tree.join(&newest.as_ref()?.1);
            discovery.is_folder(&folder).then_some(folder)
        })
        .collect();
    let mut branches: Vec<Branch> = live
        .into_iter()
        .map(|(place, tree)| {
            let (mut runs, newest) = grouped.remove(&place).unwrap_or_default();
            runs.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
            let workspace = match &launched {
                Some((launch, path)) if *launch == place => path.clone(),
                _ => newest
                    .map(|(_, relative)| tree.join(relative))
                    .filter(|path| present.contains(path))
                    .unwrap_or(tree),
            };
            let Label {
                project, branch, ..
            } = label(&place);
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
