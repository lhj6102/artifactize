//! A branch's definitions as the workbench edits them: one file per Artifact in
//! `.artifactize/`, read and written only through the library's [`edit`] operations. The
//! frontend holds what it was shown and names it with every edit, so an edit made on text that
//! changed on disk meanwhile (an agent in a terminal) is refused rather than overwriting it.
//! Places and comments are sections of the same files; nothing here keeps a file of its own.
use crate::{App, Failure, browse::BranchKey};
use artifactize_core::{config::edit, types::ArtifactName};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

/// A place on a graph, `[x, y]`.
pub type Place = [f64; 2];

/// One Artifact's definition as its file says now.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionNode {
    pub name: String,
    /// Its file, relative to the project, with `/`.
    pub definition: String,
    /// Its PATH as written: a folder ends in `/`, a Live Artifact is an address.
    pub target: String,
    pub kind: DefinitionKind,
    pub target_missing: bool,
    /// Its parts, by name; none while the project does not read.
    pub parts: Vec<String>,
    /// Its own place at the top level, when the top level is placed by hand.
    pub at: Option<Place>,
    /// Its parts' places inside its box, by part, when the box is placed by hand.
    pub places: BTreeMap<String, Place>,
    pub comment: Option<String>,
    /// Why its place or its comment cannot be read; drawing ignores them.
    pub style_error: Option<String>,
    pub comment_error: Option<String>,
}

/// What an Artifact's PATH names, as the library reads it.
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

/// A branch's definitions, and the exact text of every definition file, which the next edit
/// names as what it was based on.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionsView {
    pub nodes: Vec<DefinitionNode>,
    /// Each definition file's text, by its path relative to the project, with `/`.
    pub files: BTreeMap<String, String>,
}

/// An edit of a branch's definitions, the library's [`edit::Operation`] by name.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditOperation {
    Create {
        name: String,
        path: String,
    },
    SetPath {
        name: String,
        path: String,
    },
    AddPart {
        whole: String,
        part: String,
        at: Option<Place>,
    },
    RemovePart {
        whole: String,
        part: String,
    },
    MovePart {
        part: String,
        from: Option<String>,
        to: Option<String>,
        at: Option<Place>,
    },
    SaveSource {
        name: String,
        source: String,
    },
    Rename {
        old: String,
        new: String,
    },
    Delete {
        name: String,
    },
    SavePlaces {
        places: BTreeMap<String, Place>,
    },
    ResetPlaces {
        whole: Option<String>,
    },
    SetComment {
        name: String,
        text: String,
    },
    MoveComment {
        from: String,
        to: String,
    },
}

/// Why an edit wrote nothing.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditRefusal {
    /// The result would not be a valid project.
    Invalid {
        file: String,
        message: String,
        line: Option<u32>,
        column: Option<u32>,
    },
    /// A file the edit writes changed on disk since it was read.
    Changed { file: String },
    /// The edit is not allowed: a cycle, a name still in use, a comment already there.
    NotAllowed { reason: String },
    /// Reading or writing failed.
    Io { message: String },
}

/// What an edit did: written, or refused with why. Either way the caller reads the
/// definitions again before the next edit.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditOutcome {
    Written,
    Refused { refusal: EditRefusal },
}

fn name(text: &str) -> Result<ArtifactName, Failure> {
    text.parse().map_err(Failure::Invalid)
}

fn optional(text: Option<String>) -> Result<Option<ArtifactName>, Failure> {
    text.as_deref().map(name).transpose()
}

impl TryFrom<EditOperation> for edit::Operation {
    type Error = Failure;
    fn try_from(operation: EditOperation) -> Result<Self, Failure> {
        use EditOperation as E;
        Ok(match operation {
            E::Create { name: new, path } => Self::Create {
                name: name(&new)?,
                path: path.parse().map_err(Failure::Invalid)?,
            },
            E::SetPath { name: of, path } => Self::SetPath {
                name: name(&of)?,
                path: path.parse().map_err(Failure::Invalid)?,
            },
            E::AddPart { whole, part, at } => Self::AddPart {
                whole: name(&whole)?,
                part: name(&part)?,
                at,
            },
            E::RemovePart { whole, part } => Self::RemovePart {
                whole: name(&whole)?,
                part: name(&part)?,
            },
            E::MovePart { part, from, to, at } => Self::MovePart {
                part: name(&part)?,
                from: optional(from)?,
                to: optional(to)?,
                at,
            },
            E::SaveSource { name: of, source } => Self::SaveSource {
                name: name(&of)?,
                source,
            },
            E::Rename { old, new } => Self::Rename {
                old: name(&old)?,
                new: name(&new)?,
            },
            E::Delete { name: of } => Self::Delete { name: name(&of)? },
            E::SavePlaces { places } => Self::SavePlaces {
                places: places
                    .into_iter()
                    .map(|(key, at)| {
                        Ok((
                            key.parse::<edit::PlacementKey>()
                                .map_err(Failure::Invalid)?,
                            at,
                        ))
                    })
                    .collect::<Result<_, Failure>>()?,
            },
            E::ResetPlaces { whole } => Self::ResetPlaces {
                whole: optional(whole)?,
            },
            E::SetComment { name: of, text } => Self::SetComment {
                name: name(&of)?,
                text,
            },
            E::MoveComment { from, to } => Self::MoveComment {
                from: name(&from)?,
                to: name(&to)?,
            },
        })
    }
}

fn text(path: &std::path::Path) -> String {
    artifactize_tools::files::path_text(path)
}

fn count(number: Option<usize>) -> Option<u32> {
    number.and_then(|number| u32::try_from(number).ok())
}

impl From<edit::Refusal> for EditRefusal {
    fn from(refusal: edit::Refusal) -> Self {
        match refusal {
            edit::Refusal::Invalid {
                file,
                message,
                line,
                column,
            } => Self::Invalid {
                file: text(&file),
                message,
                line: count(line),
                column: count(column),
            },
            edit::Refusal::Changed { file, .. } => Self::Changed { file: text(&file) },
            edit::Refusal::NotAllowed { reason } => Self::NotAllowed { reason },
            edit::Refusal::Io { message } => Self::Io { message },
        }
    }
}

/// The project as the library reads it for verify, for what it says of parts.
type Config = Option<artifactize_core::config::RepoConfig>;

fn view(project: edit::Project, config: &Config) -> DefinitionsView {
    let files = project
        .files
        .iter()
        .filter_map(|(file, source)| Some((text(file), source.clone()?)))
        .collect();
    // Its parts as the library reads the project, by name.
    let parts = |name: &ArtifactName| -> Vec<String> {
        config
            .as_ref()
            .and_then(|config| config.artifacts.get(name))
            .map(|artifact| artifact.parts.iter().map(ToString::to_string).collect())
            .unwrap_or_default()
    };
    let nodes = project
        .nodes
        .iter()
        .map(|(name, node)| DefinitionNode {
            name: name.to_string(),
            definition: text(&node.definition),
            target: node.target.clone(),
            kind: node.kind.into(),
            target_missing: node.target_missing,
            parts: parts(name),
            at: node.annotations.at,
            places: node
                .annotations
                .parts
                .iter()
                .map(|(part, at)| (part.to_string(), *at))
                .collect(),
            comment: node.annotations.comment.clone(),
            style_error: node.annotations.style_error.clone(),
            comment_error: node.annotations.comment_error.clone(),
        })
        .collect();
    DefinitionsView { nodes, files }
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
            let project = edit::read_project(&workspace)?;
            let config = artifactize_core::config::read_workspace_config(&workspace).ok();
            Ok(view(project, &config))
        })
        .await,
    )?
    .map_err(|refusal: edit::Refusal| Failure::Read(refusal.to_string()))
}

/// What the library says of an edit before it is made: the definition files it would write, by
/// their paths relative to the project, or why it would refuse it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EditCheck {
    Checked { files: Vec<String> },
    Refused { refusal: EditRefusal },
}

/// Check `operation` on `branch`'s definitions as they are now, writing nothing.
#[tauri::command]
#[specta::specta]
pub async fn definition_check(
    app: tauri::State<'_, App>,
    branch: BranchKey,
    operation: EditOperation,
) -> Result<EditCheck, Failure> {
    let workspace = app.current_workspace(&branch).await?;
    let operation = edit::Operation::try_from(operation)?;
    crate::failure::joined(
        tokio::task::spawn_blocking(move || {
            match edit::read_project(&workspace)
                .and_then(|project| edit::footprint(&project, &operation))
            {
                Ok(footprint) => EditCheck::Checked {
                    files: footprint.files.keys().map(|file| text(file)).collect(),
                },
                Err(refusal) => EditCheck::Refused {
                    refusal: refusal.into(),
                },
            }
        })
        .await,
    )
}

/// Apply `operation` to `branch`'s definitions, `shown` holding the text of each file as it was
/// shown, by its path relative to the project. The library says which files the edit writes;
/// each is based on the text shown, a new one on nothing, so a file that changed on disk since,
/// or that was never shown, refuses the edit. Nothing is written when it is refused.
#[tauri::command]
#[specta::specta]
pub async fn definition_edit(
    app: tauri::State<'_, App>,
    branch: BranchKey,
    shown: BTreeMap<String, String>,
    operation: EditOperation,
) -> Result<EditOutcome, Failure> {
    let workspace = app.current_workspace(&branch).await?;
    let operation = edit::Operation::try_from(operation)?;
    let applied = crate::failure::joined(
        tokio::task::spawn_blocking(move || {
            let based_on = shown
                .into_iter()
                .map(|(file, source)| {
                    artifactize_tools::files::path_from_text(&file)
                        .map(|file| (file, Some(source)))
                        .map_err(|error| edit::Refusal::NotAllowed {
                            reason: error.to_string(),
                        })
                })
                .collect::<Result<edit::BasedOn, _>>()?;
            edit::apply_shown(&workspace, &based_on, operation)
        })
        .await,
    )?;
    Ok(match applied {
        Ok(_) => EditOutcome::Written,
        Err(refusal) => EditOutcome::Refused {
            refusal: refusal.into(),
        },
    })
}
