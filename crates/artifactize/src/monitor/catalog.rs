//! Repository/worktree selection and cached Git discovery; no artifact config is read here.
#[cfg(test)]
mod tests;
use crate::{
    repository::{self, Identity, Worktree},
    store::{CatalogRun, Signoff},
    types::RunStatus,
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
        self.running += u64::from(run.status == RunStatus::Running);
    }
    /// Status glyphs and counts in the urgency order of the header, Runs and the tree:
    /// `!1 ✗2 ◐1 ?1`.
    pub fn parts(&self) -> Vec<(&'static str, u64)> {
        let mut parts: Vec<_> = [
            ("WAITING_HUMAN", self.waiting.len() as u64),
            ("RED", self.red),
            ("ERROR", self.error),
            ("RUNNING", self.running),
        ]
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect();
        parts.sort_by_key(|(status, _)| super::model::urgency(status));
        parts
    }
    pub fn text(&self) -> String {
        self.parts()
            .into_iter()
            .map(|(status, count)| format!("{}{count}", super::model::glyph(Some(status))))
            .collect::<Vec<_>>()
            .join(" ")
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
#[derive(Default)]
pub struct Catalog {
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
            self.initial_path(path, &self.paths.clone())
        })
    }
    fn initial_path(&mut self, path: &Path, paths: &BTreeMap<Scope, BTreeSet<PathBuf>>) -> Scope {
        if let Some((scope, _)) = paths.iter().find(|(scope, workspaces)| {
            matches!(scope, Scope::Worktree(_, tree) if crate::platform::paths_equal(tree, path) || workspaces.iter().any(|workspace| crate::platform::paths_equal(workspace, path)))
        }) {
            return scope.clone();
        }
        let identity = self.identity(path);
        if let (Some(common), Some(tree)) = (identity.common_dir, identity.worktree_path) {
            return Scope::Worktree(Repository::Git(common), tree);
        }
        // Initial selection only: a known non-Git workspace can contain cwd. Choose the
        // closest ancestor by components, never a string prefix or inferred Git grouping.
        paths
            .iter()
            .filter_map(|(scope, workspaces)| match scope {
                Scope::Worktree(Repository::Workspace(_), _) => workspaces
                    .iter()
                    .filter(|workspace| crate::platform::is_within(path, workspace))
                    .map(|workspace| (workspace.components().count(), scope))
                    .max_by_key(|(depth, _)| *depth),
                _ => None,
            })
            .max_by_key(|(depth, _)| *depth)
            .map(|(_, scope)| scope.clone())
            .unwrap_or_else(|| {
                Scope::Worktree(
                    Repository::Workspace(path.to_path_buf()),
                    path.to_path_buf(),
                )
            })
    }
    /// Cached old-path discovery runs once per workspace, not once per refresh.
    pub fn update(&mut self, runs: &[CatalogRun], initial: Option<&Path>) {
        let mut badges: BTreeMap<Scope, Badge> = BTreeMap::new();
        let mut paths: BTreeMap<Scope, BTreeSet<PathBuf>> = BTreeMap::new();
        let mut branches: BTreeMap<Scope, Option<String>> = BTreeMap::new();
        for run in runs {
            let identity =
                if run.repository.common_dir.is_some() && run.repository.worktree_path.is_some() {
                    run.repository.clone()
                } else {
                    self.identity(&run.repo_path)
                };
            let repository = identity.common_dir.clone().map_or_else(
                || Repository::Workspace(run.repo_path.clone()),
                Repository::Git,
            );
            let tree = identity
                .worktree_path
                .unwrap_or_else(|| run.repo_path.clone());
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
        if let Some(path) = initial {
            let scope = self.initial_path(path, &paths);
            if let Scope::Worktree(repository, _) = &scope {
                paths
                    .entry(Scope::Repository(repository.clone()))
                    .or_default();
                paths.entry(scope).or_default();
            }
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
                    let scope = Scope::Worktree(repository.clone(), tree.path);
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
            let label = root
                .file_name()
                .unwrap_or(root.as_os_str())
                .to_string_lossy()
                .into_owned();
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
                let name = tree
                    .file_name()
                    .unwrap_or(tree.as_os_str())
                    .to_string_lossy();
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
    /// Show or hide a repository's worktrees without Runs; applied on the next update.
    pub fn toggle(&mut self, repository: &Repository) {
        if !self.expanded.remove(repository) {
            self.expanded.insert(repository.clone());
        }
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
