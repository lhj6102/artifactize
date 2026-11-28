//! Resolved Artifacts, their Evals, and the workspace model shared by callers.

use super::{
    ArtifactKind, ArtifactName, ChildPrefix, ConfigError, EvalDeclaration, EvalId, Fingerprint,
    LogicalPath, Profile, ProfileName, Site, Target, Views,
    site::{declaration_of, folder_of},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Where an Artifact is bound: the line of its `artf` statement and the macro it calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub line: usize,
    #[serde(rename = "macro", skip_serializing_if = "Option::is_none")]
    pub macro_name: Option<String>,
}

/// Nested folder paths with their Artifacts as tool-side IDs. Discovery validated both, so a
/// failure here is a bug.
fn tool_nested(
    nested: &BTreeMap<ChildPrefix, ArtifactName>,
) -> BTreeMap<artifactize_tools::scope::ChildPrefix, artifactize_tools::scope::ArtifactId> {
    nested
        .iter()
        .map(|(path, name)| {
            (
                artifactize_tools::scope::ChildPrefix::new(path.as_ref())
                    .expect("validated child prefix"),
                crate::scope::tool_id(name),
            )
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub definition: PathBuf,
    /// Exact PATH from a definition node; legacy migration inputs lack this field.
    pub(crate) authored_path: Option<String>,
    /// Current target availability is not part of the saved declaration or its identity.
    pub target_error: Option<String>,
    pub path: Target,
    /// Folder Artifacts declared in subfolders, by their path here. Their files are theirs,
    /// not this Artifact's; discovery also records them as parts.
    pub nested: BTreeMap<ChildPrefix, ArtifactName>,
    /// A folder part at the same target or an ancestor, owning every file here.
    pub covering_part: Option<ArtifactName>,
    /// What it is made of: the Artifacts its folder holds and those its `parts` names.
    pub parts: BTreeSet<ArtifactName>,
    pub name: ArtifactName,
    pub tags: Vec<String>,
    pub views: Views,
    /// The tool expression as written (`files & html`) and the toolsets it composes.
    pub tools: Option<String>,
    pub toolsets: Vec<String>,
    /// Absent only for Artifacts read from saved Runs.
    pub binding: Option<Binding>,
    pub basis: Option<bool>,
    pub fingerprint: Option<Fingerprint>,
}

impl serde::Serialize for Artifact {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("definition", &crate::platform::path_text(&self.definition))?;
        map.serialize_entry("path", &self.path)?;
        map.serialize_entry("kind", &self.kind())?;
        map.serialize_entry("nested", &self.nested)?;
        if let Some(part) = &self.covering_part {
            map.serialize_entry("coveringPart", part)?;
        }
        if !self.select().is_empty() {
            map.serialize_entry("select", self.select())?;
        }
        if !self.parts.is_empty() {
            map.serialize_entry("parts", &self.parts)?;
        }
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("tags", &self.tags)?;
        map.serialize_entry("views", &self.views)?;
        if let Some(tools) = &self.tools {
            map.serialize_entry("tools", tools)?;
            map.serialize_entry("toolsets", &self.toolsets)?;
        }
        if let Some(binding) = &self.binding {
            map.serialize_entry("binding", binding)?;
        }
        map.serialize_entry("basis", &self.basis)?;
        map.serialize_entry("fingerprint", &self.fingerprint)?;
        map.end()
    }
}

impl Artifact {
    pub fn kind(&self) -> ArtifactKind {
        self.path.kind()
    }
    /// Its `path` as written: a folder ends in `/`, a Live Artifact is an address.
    pub fn written_path(&self) -> String {
        self.authored_path
            .clone()
            .unwrap_or_else(|| self.path.to_string())
    }
    pub fn select(&self) -> &[LogicalPath] {
        self.path.select()
    }
    pub(crate) fn tool_scope(&self) -> artifactize_tools::scope::Artifact {
        artifactize_tools::scope::Artifact {
            path: self.path.clone(),
            name: self.name.to_string(),
            nested: tool_nested(&self.nested),
            covering_part: self.covering_part.as_ref().map(crate::scope::tool_id),
        }
    }

    /// Runtime commands and relative declarations use the containing folder.
    pub fn folder(&self) -> &Path {
        folder_of(self.kind(), &self.path)
    }

    /// What `{name}` stands for: the folder or file, or an aspect's folder, since an
    /// aspect's path is only its declaration's label.
    pub fn referenced_path(&self) -> Option<&Path> {
        if self.kind() == ArtifactKind::Aspect {
            Some(self.folder())
        } else {
            self.path.local()
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        (self.kind() == ArtifactKind::File)
            .then(|| {
                self.path
                    .local()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
            })
            .flatten()
    }

    pub fn declaration_path(&self) -> PathBuf {
        if self.definition.as_os_str().is_empty() {
            declaration_of(self.kind(), &self.path)
        } else {
            self.definition.clone()
        }
    }

    /// Where it is declared.
    pub fn site(&self) -> Site {
        Site {
            path: self.path.clone(),
            definition: self.declaration_path(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Eval {
    /// Workspace-qualified id; declaration.id remains the owner's local id.
    pub id: EvalId,
    pub target: ArtifactName,
    pub references: BTreeMap<crate::config::ReferenceName, ArtifactName>,
    pub deps: Vec<ArtifactName>,
    pub declaration: EvalDeclaration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRequirement {
    Basis,
    Unreviewed,
    Evals,
    /// No evals of its own: its parts' verdicts are its verdict.
    Parts,
}

#[derive(Debug)]
pub struct RepoConfig {
    pub root: PathBuf,
    pub artifacts: BTreeMap<ArtifactName, Artifact>,
    pub evals: Vec<Eval>,
    pub relations: Vec<crate::scope::Relation>,
    /// The project's profiles, each in the branch the environment selects.
    pub profiles: BTreeMap<ProfileName, Profile>,
    /// The code the declarations were read from; absent for configurations saved with Runs.
    pub(crate) program: Option<Arc<super::lang::Program>>,
    /// The plugins whose toolsets it may compose, with their viewers.
    pub(crate) plugins: super::plugins::Catalog,
}

impl RepoConfig {
    /// An error at the declaration of `artifact` that `keys` names (`["evals", id,
    /// "instruction"]`, `["parts"]`, …), with the call site when a macro made it.
    pub(crate) fn error_at(
        &self,
        artifact: &ArtifactName,
        keys: &[&str],
        message: impl Into<String>,
    ) -> ConfigError {
        let message = message.into();
        let Some(program) = &self.program else {
            let definition = self
                .artifacts
                .get(artifact)
                .map(Artifact::declaration_path)
                .unwrap_or_default();
            return ConfigError::new(self.root.join(definition), message);
        };
        let lowered = program
            .artifacts
            .iter()
            .find(|lowered| lowered.name == artifact.as_str());
        let Some(lowered) = lowered else {
            return ConfigError::new(&self.root, message);
        };
        super::discovery::located(
            &self.root,
            program,
            lowered,
            super::lang::span_of(lowered, keys),
            message,
        )
    }

    /// Every Artifact `name` is made of, directly or through its parts, each once.
    pub fn descendants<'a>(&'a self, name: &'a ArtifactName) -> Vec<&'a ArtifactName> {
        let mut seen = BTreeSet::new();
        let mut pending = vec![name];
        while let Some(next) = pending.pop() {
            for part in self.artifacts.get(next).into_iter().flat_map(|a| &a.parts) {
                if seen.insert(part) {
                    pending.push(part);
                }
            }
        }
        seen.into_iter().collect()
    }

    /// Local obligations only; a basis does not satisfy its dependency scope.
    pub fn review_requirement(&self, artifact_id: &str) -> Option<ReviewRequirement> {
        let artifact = self.artifacts.get(artifact_id)?;
        Some(if artifact.basis == Some(true) {
            ReviewRequirement::Basis
        } else if self.evals.iter().any(|eval| eval.target == artifact_id) {
            ReviewRequirement::Evals
        } else if !artifact.parts.is_empty() {
            ReviewRequirement::Parts
        } else {
            ReviewRequirement::Unreviewed
        })
    }
}
