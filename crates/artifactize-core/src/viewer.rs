//! The desktop hosts a viewer; this library decides what its bridge can read.

use std::{
    fs::File,
    path::{Path, PathBuf},
};

use artifactize_tools::scope::{self, InputPath};
use serde::Serialize;

use crate::{
    config::{
        self, Artifact, ArtifactKind, ArtifactType, RepoConfig,
        plugins::{BuiltinViewer, ViewerEntry},
    },
    graph_structure, platform, repository,
    types::{ArtifactName, RequestId, ReviewerId},
};

/// Relative file reads and directory listings use the selected Artifact's own scope, not the
/// workspace or the rest of the review's scope. A viewer cannot choose its own scope.
#[derive(Debug, Clone)]
pub struct Session {
    root: PathBuf,
    checkout: repository::Identity,
    artifact: Artifact,
    entry: ViewerEntry,
}

/// The ownership boundaries that can change while the Artifact's target remains the same.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    nested: std::collections::BTreeMap<config::ChildPrefix, ArtifactName>,
    covering_part: Option<ArtifactName>,
}

#[derive(Debug, Clone)]
pub struct Review {
    pub id: RequestId,
    pub reviewer: ReviewerId,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context<'a> {
    pub artifact: &'a ArtifactName,
    #[serde(rename = "type")]
    pub type_name: &'a ArtifactType,
    pub title: &'a str,
    pub target: &'a config::Target,
    pub target_kind: ArtifactKind,
    pub review: Option<&'a RequestId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: InputPath,
    pub kind: EntryKind,
}

impl Session {
    pub fn open(repo: &Path, artifact: &ArtifactName) -> Result<Self, String> {
        let config = config::read_workspace_config(repo).map_err(|error| error.to_string())?;
        Self::from_config(&config, artifact)
    }

    pub(crate) fn from_config(
        config: &RepoConfig,
        artifact: &ArtifactName,
    ) -> Result<Self, String> {
        let artifact = config
            .artifacts
            .get(artifact)
            .ok_or_else(|| format!("Unknown Artifact {artifact}."))?;
        let entry = artifact
            .plugin
            .as_ref()
            .and_then(|plugin| plugin.viewer.clone())
            .ok_or_else(|| format!("Artifact {} does not declare a viewer.", artifact.name))?;
        if !artifact
            .views
            .human_tools
            .values()
            .any(|tool| tool.kind() == config::HumanToolKind::Viewer)
        {
            return Err(format!(
                "Artifact {} has no viewer Human tool.",
                artifact.name
            ));
        }
        crate::scope::validate_file_target(&config.root, artifact)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            root: config.root.clone(),
            checkout: repository::identify(&config.root),
            artifact: artifact.clone(),
            entry,
        })
    }

    pub fn scope(&self) -> Scope {
        Scope {
            nested: self.artifact.nested.clone(),
            covering_part: self.artifact.covering_part.clone(),
        }
    }

    pub fn workspace(&self) -> &Path {
        &self.root
    }

    pub fn checkout(&self) -> &repository::Identity {
        &self.checkout
    }

    /// The same path may later hold different branch contents and must not be reused.
    pub fn check_checkout(&self) -> Result<(), String> {
        check_checkout(&self.root, &self.checkout)
    }

    /// An already-open viewer's scope cannot authorize access after a declaration changes.
    pub fn check_scope(&self) -> Result<(), String> {
        self.check_checkout()?;
        let config =
            config::read_workspace_config(&self.root).map_err(|error| error.to_string())?;
        if config
            .artifacts
            .get(&self.artifact.name)
            .is_some_and(|current| {
                current.path == self.artifact.path
                    && current.type_name == self.artifact.type_name
                    && current.nested == self.artifact.nested
                    && current.covering_part == self.artifact.covering_part
            })
        {
            Ok(())
        } else {
            Err("This Artifact's viewer scope has changed. Reopen the viewer.".into())
        }
    }

    pub fn entry(&self) -> &ViewerEntry {
        &self.entry
    }

    pub fn context<'a>(&'a self, review: Option<&'a Review>) -> Context<'a> {
        let plugin = self.artifact.plugin.as_ref().expect("viewer has a plugin");
        Context {
            artifact: &self.artifact.name,
            type_name: &plugin.info.name,
            title: &plugin.info.title,
            target: &self.artifact.path,
            target_kind: self.artifact.kind(),
            review: review.map(|review| &review.id),
        }
    }

    /// Append a Point to the host-selected review; the store rechecks its claim atomically.
    pub async fn pointing(
        &self,
        state: &Path,
        review: &Review,
        text: &str,
    ) -> Result<crate::review::Draft, String> {
        let session = self.clone();
        crate::task::blocking(move || session.check_scope()).await??;
        let context = self.context(None);
        let source = format!("{} · {}", context.artifact, context.title);
        crate::review::append_point(state, &review.id, &review.reviewer, text, &source).await
    }

    pub fn read_text(&self, path: &InputPath) -> Result<String, String> {
        self.check_scope()?;
        let path = self
            .artifact
            .file_name()
            .filter(|_| path.as_str().is_empty())
            .unwrap_or(path.as_str());
        config::plugins::read_document(self.open_file(path)?)
    }

    /// Read one directory at a time. Never list non-owned siblings of a file Artifact or
    /// directories outside an aspect's selection.
    pub fn files(&self, path: &InputPath) -> Result<Vec<FileEntry>, String> {
        self.check_scope()?;
        let path = path.as_str();
        if path.is_empty()
            && let Some(name) = self.artifact.file_name()
        {
            self.open_file(name)?;
            return Ok(vec![FileEntry {
                name: name.into(),
                path: name
                    .parse()
                    .map_err(|error: scope::ScopeError| error.to_string())?,
                kind: EntryKind::File,
            }]);
        }
        let directory = self.open_file(path)?;
        let mut entries = Vec::new();
        for entry in platform::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            let Some(name) = platform::os_text(&name) else {
                continue;
            };
            let child = if path.is_empty() {
                name.to_owned()
            } else {
                format!("{path}/{name}")
            };
            let Ok(child_path) = child.parse::<InputPath>() else {
                continue;
            };
            let kind = match entry.file_type().map_err(|error| error.to_string())? {
                platform::FileKind::File => EntryKind::File,
                platform::FileKind::Directory => EntryKind::Directory,
                _ => continue,
            };
            if self.open_file(child_path.as_str()).is_ok() {
                entries.push(FileEntry {
                    name: name.into(),
                    path: child_path,
                    kind,
                });
            }
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(entries)
    }

    pub fn declarations(&self) -> Result<graph_structure::Structure, String> {
        self.check_scope()?;
        if !matches!(
            self.entry,
            ViewerEntry::Builtin {
                viewer: BuiltinViewer::Artifactize
            }
        ) {
            return Err("Only an Artifactize Viewer can read the declared graph.".into());
        }
        graph_structure::read_scoped(&self.root, &self.artifact)
    }

    fn open_file(&self, path: &str) -> Result<File, String> {
        if self.artifact.covering_part.is_some() {
            return Err("This Artifact's files belong to a folder part.".into());
        }
        scope::open_input(&self.root, &self.artifact.tool_scope(), path)
            .map_err(|error| error.to_string())
    }
}

fn check_checkout(worktree: &Path, checkout: &repository::Identity) -> Result<(), String> {
    if worktree.is_dir() && repository::identify(worktree) == *checkout {
        Ok(())
    } else {
        Err("This Artifact Viewer's checkout changed. Reopen the viewer.".into())
    }
}
