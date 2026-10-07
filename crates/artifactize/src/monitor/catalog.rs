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
    pub running: u64,
}
impl Badge {
    fn add(&mut self, run: &CatalogRun) {
        self.waiting.extend(run.waiting.iter().cloned());
        self.red += run.red;
        self.running += u64::from(run.status == RunStatus::Running);
    }
    pub fn text(&self) -> String {
        format!(
            "{}{}{}",
            if self.waiting.is_empty() {
                String::new()
            } else {
                format!(" ?{}", self.waiting.len())
            },
            if self.red == 0 {
                String::new()
            } else {
                format!(" RED{}", self.red)
            },
            if self.running == 0 {
                String::new()
            } else {
                format!(" ◐{}", self.running)
            }
        )
    }
}
#[derive(Debug, Clone)]
pub struct CatalogRow {
    pub scope: Scope,
    pub label: String,
    pub badge: Badge,
    pub depth: usize,
}
#[derive(Default)]
pub struct Catalog {
    identities: BTreeMap<PathBuf, Identity>,
    trees: BTreeMap<PathBuf, Vec<Worktree>>,
    pub rows: Vec<CatalogRow>,
    paths: BTreeMap<Scope, BTreeSet<PathBuf>>,
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
        if let Some((scope, _)) = paths.iter().find(|(scope, workspaces)| matches!(scope, Scope::Worktree(_, tree) if tree == path || workspaces.contains(path))) {
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
                    .filter(|workspace| path.starts_with(workspace))
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
            let scope = Scope::Repository(repository.clone());
            rows.push(CatalogRow {
                scope: scope.clone(),
                label: format!("▾ {label}"),
                badge: badges.get(&scope).cloned().unwrap_or_default(),
                depth: 0,
            });
            rows.push(CatalogRow {
                scope: scope.clone(),
                label: "ALL".into(),
                badge: badges.get(&scope).cloned().unwrap_or_default(),
                depth: 1,
            });
            for (scope, _) in paths.iter().filter(
                |(scope, _)| matches!(scope, Scope::Worktree(repo, _) if repo == &repository),
            ) {
                let Scope::Worktree(_, tree) = scope else {
                    unreachable!("worktree filter")
                };
                let branch = branches.get(scope).and_then(Option::as_deref);
                let name = tree
                    .file_name()
                    .unwrap_or(tree.as_os_str())
                    .to_string_lossy();
                let label = branch.map_or_else(
                    || format!("{name} (detached/non-Git)"),
                    |branch| format!("{branch} · {name}"),
                );
                rows.push(CatalogRow {
                    scope: scope.clone(),
                    label,
                    badge: badges.get(scope).cloned().unwrap_or_default(),
                    depth: 1,
                });
            }
        }
        self.rows = rows;
        self.paths = paths;
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
