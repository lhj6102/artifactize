//! A branch's definitions as its `.artf` code declares them now, read-only: the Current draws
//! them and its Detail shows where each Artifact is bound and the code that binds it. The code
//! changes only by editing its files, in Chat or anywhere else.
use crate::{App, Failure, browse::BranchKey};
use artifactize_core::types::ArtifactName;
use serde::Serialize;
use specta::Type;

/// One Artifact as its code declares it now.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionNode {
    pub name: String,
    /// The file that binds it, relative to the project, with `/`, and the binding's line.
    pub definition: String,
    pub line: u32,
    /// The macro its binding calls, as `rust-crate!`; none for one written out.
    pub macro_name: Option<String>,
    /// Its `path` as written: a folder ends in `/`, a Live Artifact is an address.
    pub target: String,
    pub kind: DefinitionKind,
    pub target_missing: bool,
    /// Its parts, by name.
    pub parts: Vec<String>,
    /// The toolsets it composes, as written: `files & html`.
    pub tools: Option<String>,
}

/// What an Artifact's `path` names, as the library reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DefinitionKind {
    Folder,
    File,
    Aspect,
    Graph,
    /// An address outside the worktree: a Live Artifact.
    Live,
}

impl From<artifactize_tools::scope::ArtifactKind> for DefinitionKind {
    fn from(kind: artifactize_tools::scope::ArtifactKind) -> Self {
        use artifactize_tools::scope::ArtifactKind as K;
        match kind {
            K::Folder => Self::Folder,
            K::File => Self::File,
            K::Aspect => Self::Aspect,
            K::Graph => Self::Graph,
            K::Live => Self::Live,
        }
    }
}

/// A branch's definitions.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionsView {
    pub nodes: Vec<DefinitionNode>,
}

/// The code that binds one Artifact: as written, and, for a macro call, what it expands to.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionSource {
    /// Its file, relative to the project, with `/`, and the line the code starts at.
    pub file: String,
    pub line: u32,
    pub authored: String,
    /// The declaration a macro call expands to; none for an Artifact written out, whose code
    /// is what it expands to.
    pub expanded: Option<String>,
}

fn line(number: usize) -> u32 {
    u32::try_from(number).unwrap_or(u32::MAX)
}

/// The definitions of `branch`'s worktree as they are now.
#[tauri::command]
#[specta::specta]
pub async fn definitions(
    app: tauri::State<'_, App>,
    branch: BranchKey,
) -> Result<DefinitionsView, Failure> {
    let workspace = app.current_workspace(&branch).await?;
    crate::failure::joined(
        tokio::task::spawn_blocking(move || {
            let config = artifactize_core::config::read_workspace_config(&workspace)
                .map_err(|error| Failure::Read(error.to_string()))?;
            let nodes = config
                .artifacts
                .values()
                .map(|artifact| DefinitionNode {
                    name: artifact.name.to_string(),
                    definition: artifactize_tools::files::path_text(&artifact.definition),
                    line: line(artifact.binding.as_ref().map_or(1, |binding| binding.line)),
                    macro_name: artifact
                        .binding
                        .as_ref()
                        .and_then(|binding| binding.macro_name.clone()),
                    target: artifact.written_path(),
                    kind: artifact.kind().into(),
                    target_missing: artifact.target_error.is_some(),
                    parts: artifact.parts.iter().map(ToString::to_string).collect(),
                    tools: artifact.tools.clone(),
                })
                .collect();
            Ok(DefinitionsView { nodes })
        })
        .await,
    )?
}

/// The code that binds `artifact` in `branch`'s worktree now.
#[tauri::command]
#[specta::specta]
pub async fn definition_source(
    app: tauri::State<'_, App>,
    branch: BranchKey,
    artifact: String,
) -> Result<DefinitionSource, Failure> {
    let artifact: ArtifactName = artifact.parse().map_err(Failure::Invalid)?;
    let workspace = app.current_workspace(&branch).await?;
    crate::failure::joined(
        tokio::task::spawn_blocking(move || {
            let [expansion] = artifactize_core::config::expand::expand(&workspace, Some(&artifact))
                .map_err(|error| Failure::Read(error.to_string()))?
                .try_into()
                .map_err(|_| Failure::Read(format!("Unknown Artifact {artifact}.")))?;
            Ok(DefinitionSource {
                file: artifactize_tools::files::path_text(&expansion.file),
                line: line(expansion.line),
                expanded: expansion.macro_name.is_some().then_some(expansion.expanded),
                authored: expansion.authored,
            })
        })
        .await,
    )?
}
