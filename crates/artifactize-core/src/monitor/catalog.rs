//! Repository/worktree selection and cached Git discovery; no artifact config is read here.
#[cfg(test)]
mod tests;
use super::model::Status;
use crate::{
    repository::{self, Identity, Worktree},
    store::{CatalogRun, Signoff},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Repository {
    Git(PathBuf),
    Workspace(PathBuf),
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    All,
    Repository(Repository),
    Worktree(Repository, PathBuf),
}
#[derive(Debug, Clone, Default)]
pub struct Badge {
    pub waiting: BTreeSet<Signoff>,
    pub red: u64,
    pub error: u64,
    pub running: u64,
}
impl Badge {
    fn add(&mut self, run: &CatalogRun) {
        self.waiting.extend(run.waiting.iter().cloned());
        self.red += run.red;
        self.error += run.error;
        self.running += u64::from(run.status.is_open());
    }
    /// Status glyphs and counts in the urgency order of the header, Runs and the tree:
    /// `!1 ✗2 ◐1 ?1`.
    pub fn parts(&self) -> Vec<(Status, u64)> {
        let mut parts: Vec<_> = [
            (Status::WaitingHuman, self.waiting.len() as u64),
            (Status::Red, self.red),
            (Status::Error, self.error),
            (Status::Running, self.running),
        ]
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect();
        parts.sort_by_key(|(status, _)| *status);
        parts
    }
}
#[derive(Debug, Clone)]
pub struct CatalogRow {
    pub scope: Scope,
    pub label: String,
    pub badge: Badge,
    pub depth: usize,
    /// A row standing for this repository's worktrees without Runs; Space shows or hides them.
    pub fold: Option<Repository>,
}
/// The representative paths for physical directories in this catalog snapshot.
#[derive(Default)]
struct Directories {
    spellings: BTreeMap<PathBuf, PathBuf>,
    identities: BTreeMap<crate::platform::FileIdentity, PathBuf>,
}

impl Directories {
    fn resolve(&mut self, path: PathBuf) -> PathBuf {
        self.spellings
            .entry(path.clone())
            .or_insert_with(|| {
                crate::platform::paths::directory_identity(&path).map_or_else(
                    |_| path,
                    |(id, canonical)| self.identities.entry(id).or_insert(canonical).clone(),
                )
            })
            .clone()
    }
}

#[derive(Clone, Default)]
pub struct Catalog {
    facts: std::sync::Arc<Vec<CatalogRun>>,
    initial_path: Option<PathBuf>,
    identities: BTreeMap<PathBuf, Identity>,
    trees: BTreeMap<PathBuf, Vec<Worktree>>,
    pub rows: Vec<CatalogRow>,
    paths: BTreeMap<Scope, BTreeSet<PathBuf>>,
    /// Repositories whose worktrees without Runs are shown.
    expanded: BTreeSet<Repository>,
    /// The selected scope stays listed even without Runs.
    pub selected: Option<Scope>,
}
impl Catalog {
    /// Rediscover Git metadata without losing the selected scope or worktree folds.
    pub(super) fn reset_git(&mut self) {
        self.identities.clear();
        self.trees.clear();
    }
    fn identity(&mut self, path: &Path) -> Identity {
        self.identities
            .entry(path.to_path_buf())
            .or_insert_with(|| repository::identify(path))
            .clone()
    }
    fn trees(&mut self, common_dir: &Path) -> Vec<Worktree> {
        self.trees
            .entry(common_dir.to_path_buf())
            .or_insert_with(|| repository::worktrees(common_dir))
            .clone()
    }
    pub fn initial(&mut self, path: Option<&Path>) -> Scope {
        path.map_or(Scope::All, |path| {
            let paths = self.paths.clone();
            self.initial_scope(path, &paths)
        })
    }
    fn initial_scope(&mut self, path: &Path, paths: &BTreeMap<Scope, BTreeSet<PathBuf>>) -> Scope {
        // Take the filesystem and Git observations here; the choice below takes only facts.
        let exact = paths.iter().find(|(scope, workspaces)| {
            matches!(scope, Scope::Worktree(_, tree) if crate::platform::paths_equal(tree, path) || workspaces.iter().any(|workspace| crate::platform::paths_equal(workspace, path)))
        }).map(|(scope, _)| scope.clone());
        if let Some(scope) = exact {
            return scope;
        }
        let identity = self.identity(path);
        let ancestors = if identity.common_dir.is_some() && identity.worktree_path.is_some() {
            Vec::new()
        } else {
            paths
                .iter()
                .filter(|(scope, _)| matches!(scope, Scope::Worktree(Repository::Workspace(_), _)))
                .flat_map(|(scope, workspaces)| {
                    workspaces
                        .iter()
                        .filter(move |workspace| crate::platform::is_within(path, workspace))
                        .map(move |workspace| (workspace.components().count(), scope))
                })
                .collect()
        };
        Self::initial_path(path, identity, ancestors.into_iter())
    }

    fn initial_path<'a>(
        path: &Path,
        identity: Identity,
        ancestors: impl Iterator<Item = (usize, &'a Scope)>,
    ) -> Scope {
        if let (Some(common), Some(tree)) = (identity.common_dir, identity.worktree_path) {
            return Scope::Worktree(Repository::Git(common), tree);
        }
        // Initial selection only: a known non-Git workspace can contain cwd. Choose the
        // closest ancestor by components, never a string prefix or inferred Git grouping.
        ancestors
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, scope)| scope.clone())
            .unwrap_or_else(|| {
                Scope::Worktree(
                    Repository::Workspace(path.to_path_buf()),
                    path.to_path_buf(),
                )
            })
    }
    /// Old-path discovery stays cached between resets, including database-only refreshes.
    pub fn update(&mut self, runs: &[CatalogRun], initial: Option<&Path>) {
        self.facts = std::sync::Arc::new(runs.to_vec());
        self.initial_path = initial.map(Path::to_path_buf);
        let mut badges: BTreeMap<Scope, Badge> = BTreeMap::new();
        let mut paths: BTreeMap<Scope, BTreeSet<PathBuf>> = BTreeMap::new();
        let mut branches: BTreeMap<Scope, Option<String>> = BTreeMap::new();
        let mut directories = Directories::default();
        for run in runs {
            let identity =
                if run.repository.common_dir.is_some() && run.repository.worktree_path.is_some() {
                    run.repository.clone()
                } else {
                    self.identity(&run.repo_path)
                };
            let (repository, tree) = match (identity.common_dir, identity.worktree_path) {
                (Some(common), Some(tree)) => (
                    Repository::Git(directories.resolve(common)),
                    directories.resolve(tree),
                ),
                _ => {
                    let path = directories.resolve(run.repo_path.clone());
                    (Repository::Workspace(path.clone()), path)
                }
            };
            let scope = Scope::Worktree(repository.clone(), tree);
            branches.insert(scope.clone(), identity.branch);
            for scope in [Scope::All, Scope::Repository(repository), scope] {
                paths
                    .entry(scope.clone())
                    .or_default()
                    .insert(run.repo_path.clone());
                badges.entry(scope).or_default().add(run);
            }
        }
        if let Some(path) = initial
            && let Scope::Worktree(repository, tree) = self.initial_scope(path, &paths)
        {
            let (repository, tree) = match repository {
                Repository::Git(common) => (
                    Repository::Git(directories.resolve(common)),
                    directories.resolve(tree),
                ),
                workspace => (workspace, tree),
            };
            paths
                .entry(Scope::Repository(repository.clone()))
                .or_default();
            paths.entry(Scope::Worktree(repository, tree)).or_default();
        }
        let repositories: Vec<_> = paths
            .keys()
            .filter_map(|scope| match scope {
                Scope::Repository(repo) => Some(repo.clone()),
                _ => None,
            })
            .collect();
        for repository in &repositories {
            if let Repository::Git(common) = repository {
                for tree in self.trees(common) {
                    let scope = Scope::Worktree(repository.clone(), directories.resolve(tree.path));
                    paths.entry(scope.clone()).or_default();
                    branches.entry(scope).or_insert(tree.branch);
                }
            }
        }
        let mut rows = vec![CatalogRow {
            scope: Scope::All,
            label: "ALL".into(),
            badge: badges.get(&Scope::All).cloned().unwrap_or_default(),
            depth: 0,
            fold: None,
        }];
        for repository in repositories {
            let root = match &repository {
                Repository::Git(path) => path.parent().unwrap_or(path),
                Repository::Workspace(path) => path.as_path(),
            };
            let label = root.file_name().map_or_else(
                || crate::platform::path_text(root),
                |name| name.to_string_lossy().into_owned(),
            );
            // The repository row is the whole repository; there is no separate `ALL` row.
            let scope = Scope::Repository(repository.clone());
            rows.push(CatalogRow {
                scope: scope.clone(),
                label,
                badge: badges.get(&scope).cloned().unwrap_or_default(),
                depth: 0,
                fold: None,
            });
            let mut folded = 0;
            let expanded = self.expanded.contains(&repository);
            for (scope, workspaces) in paths.iter().filter(
                |(scope, _)| matches!(scope, Scope::Worktree(repo, _) if repo == &repository),
            ) {
                let Scope::Worktree(_, tree) = scope else {
                    unreachable!("worktree filter")
                };
                if workspaces.is_empty() && !expanded && self.selected.as_ref() != Some(scope) {
                    folded += 1;
                    continue;
                }
                let branch = branches.get(scope).and_then(Option::as_deref);
                let name = tree.file_name().map_or_else(
                    || crate::platform::path_text(tree),
                    |name| name.to_string_lossy().into_owned(),
                );
                let label = match (branch, &repository) {
                    (Some(branch), _) if branch == name => branch.to_owned(),
                    (Some(branch), _) => format!("{branch} · {name}"),
                    (None, Repository::Git(_)) => format!("{name} (detached)"),
                    (None, Repository::Workspace(_)) => format!("{name} (non-Git)"),
                };
                rows.push(CatalogRow {
                    scope: scope.clone(),
                    label,
                    badge: badges.get(scope).cloned().unwrap_or_default(),
                    depth: 1,
                    fold: None,
                });
            }
            let empty = paths
                .iter()
                .filter(|(scope, workspaces)| {
                    matches!(scope, Scope::Worktree(repo, _) if repo == &repository)
                        && workspaces.is_empty()
                })
                .count();
            if folded > 0 || expanded && empty > 0 {
                rows.push(CatalogRow {
                    scope: scope.clone(),
                    label: if expanded {
                        format!("− hide {empty} without Runs (Space)")
                    } else {
                        format!("+{folded} without Runs (Space)")
                    },
                    badge: Badge::default(),
                    depth: 1,
                    fold: Some(repository.clone()),
                });
            }
        }
        self.rows = rows;
        self.paths = paths;
    }
    /// Navigation/fold changes reuse the last accepted database facts, not a catalog query.
    pub fn rebuild(&mut self) {
        let facts = self.facts.clone();
        let initial = self.initial_path.clone();
        self.update(&facts, initial.as_deref());
    }
    /// Show or hide a repository's worktrees without Runs; applied on the next update.
    pub fn toggle(&mut self, repository: &Repository) {
        if !self.expanded.remove(repository) {
            self.expanded.insert(repository.clone());
        }
    }
    /// The name the breadcrumb shows for `scope`: its row's label, else its folder name.
    pub fn label(&self, scope: &Scope) -> String {
        if *scope == Scope::All {
            return "all repositories".into();
        }
        self.rows
            .iter()
            .find(|row| row.fold.is_none() && row.scope == *scope)
            .map_or_else(
                || match scope {
                    Scope::Worktree(_, path) => path.file_name().map_or_else(
                        || crate::platform::path_text(path),
                        |name| name.to_string_lossy().into(),
                    ),
                    _ => "repository".into(),
                },
                |row| row.label.clone(),
            )
    }
    pub fn paths(&self, scope: &Scope) -> Option<Vec<PathBuf>> {
        match scope {
            Scope::All => None,
            _ => Some(
                self.paths
                    .get(scope)
                    .map_or_else(Vec::new, |paths| paths.iter().cloned().collect()),
            ),
        }
    }
}
