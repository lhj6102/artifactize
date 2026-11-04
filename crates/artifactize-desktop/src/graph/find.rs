//! The graph's find field: the Artifacts and evals of a Run or a Current whose names contain
//! a text, or with nothing typed its starred Artifacts, in the graph's order, each with the node
//! and tab its Detail opens on. An Artifact is starred for every branch of its project.
use super::{Source, node_id};
use crate::{App, Failure, browse::BranchKey, ids::NodeKey, status::Tone};
use artifactize_core::{
    monitor::{Kind, Node, NodeId, live},
    types::ArtifactName,
};
use serde::Serialize;
use specta::Type;
use std::collections::BTreeSet;

/// The most found names listed; the rest are only counted.
pub const LISTED: usize = 50;

/// An Artifact or an eval whose name contains the text, or a starred Artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// The Artifact's node, whose Detail opens.
    pub node: NodeKey,
    /// The eval's tab the Detail opens on; none for the Artifact's own.
    pub tab: Option<NodeKey>,
    /// The Artifact's whole name.
    pub artifact: String,
    /// The eval's name within its Artifact.
    pub eval: Option<String>,
    /// Its verdict's glyph and tone; on a Current, what verify would do.
    pub glyph: String,
    pub tone: Tone,
}

/// What a text found: the first [`LISTED`] (50) in the graph's order, and how many more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub found: Vec<Found>,
    pub more: u32,
}

/// An Artifact or an eval as the find field reads it.
pub(crate) struct Named<'a> {
    pub id: NodeId,
    pub name: &'a str,
    pub glyph: &'a str,
    pub tone: Tone,
}

/// What the find field lists: the Artifacts and evals whose names contain a text, ignoring
/// case, or the starred Artifacts.
pub(crate) enum Wanted<'w> {
    Text(&'w str),
    Starred(&'w BTreeSet<ArtifactName>),
}

/// The Artifacts and evals of `artifacts`, each Artifact with its evals, that `wanted` asks
/// for. An empty text finds nothing.
pub(crate) fn find_in<'a, E>(
    artifacts: impl IntoIterator<Item = (Named<'a>, E)>,
    wanted: &Wanted<'_>,
) -> Finding
where
    E: IntoIterator<Item = Named<'a>>,
{
    let mut found = Vec::new();
    let mut more = 0u32;
    let text = match wanted {
        Wanted::Text(text) => Some(text.trim().to_lowercase()),
        Wanted::Starred(_) => None,
    };
    if text.as_deref() == Some("") {
        return Finding { found, more };
    }
    let artifact_wanted = |name: &str| match (wanted, &text) {
        (Wanted::Starred(starred), _) => starred.contains(name),
        (_, Some(text)) => name.to_lowercase().contains(text),
        (_, None) => false,
    };
    let eval_wanted = |name: &str| {
        text.as_ref()
            .is_some_and(|text| name.to_lowercase().contains(text))
    };
    let mut take = |item: Found| {
        if found.len() < LISTED {
            found.push(item);
        } else {
            more = more.saturating_add(1);
        }
    };
    for (artifact, evals) in artifacts {
        let node: NodeKey = artifact.id.into();
        if artifact_wanted(artifact.name) {
            take(Found {
                node: node.clone(),
                tab: None,
                artifact: artifact.name.to_owned(),
                eval: None,
                glyph: artifact.glyph.to_owned(),
                tone: artifact.tone,
            });
        }
        for eval in evals.into_iter().filter(|eval| eval_wanted(eval.name)) {
            take(Found {
                node: node.clone(),
                tab: Some(eval.id.into()),
                artifact: artifact.name.to_owned(),
                eval: Some(eval.name.to_owned()),
                glyph: eval.glyph.to_owned(),
                tone: eval.tone,
            });
        }
    }
    Finding { found, more }
}

/// What `wanted` asks for in `source`: the shown Run's graph, or the branch's Current as its
/// last scan found it.
fn search(app: &App, source: Source, wanted: &Wanted<'_>) -> Result<Finding, Failure> {
    match source {
        Source::Run { run } => {
            let selected = app.browse.selected(&run.into_inner())?;
            fn named(node: &Node) -> Named<'_> {
                Named {
                    id: node.id.clone(),
                    name: &node.name,
                    glyph: node.glyph,
                    tone: node.tone.into(),
                }
            }
            Ok(find_in(
                selected.tree.iter().map(|artifact| {
                    let evals = artifact
                        .children
                        .iter()
                        .filter(|child| matches!(child.kind, Kind::Eval(_)))
                        .map(named);
                    (named(artifact), evals)
                }),
                wanted,
            ))
        }
        Source::Current { branch } => {
            let shown = app.currents.shown(&branch.to_place())?;
            Ok(find_in(
                shown.current.artifacts.iter().map(|artifact| {
                    let evals = artifact.evals.iter().map(|eval| Named {
                        id: NodeId::eval(eval.id.clone()),
                        name: &eval.name,
                        glyph: eval.plan.glyph(),
                        tone: eval.plan.tone().into(),
                    });
                    let own = Named {
                        id: node_id(&artifact.name).into_inner(),
                        name: artifact.name.as_str(),
                        glyph: artifact.plan.glyph(),
                        tone: artifact.plan.tone().into(),
                    };
                    (own, evals)
                }),
                wanted,
            ))
        }
    }
}

/// The Artifacts and evals of `source` whose names contain `text`: the shown Run's, or the
/// branch's Current as its last scan found it.
#[tauri::command]
#[specta::specta]
pub async fn find(
    app: tauri::State<'_, App>,
    source: Source,
    text: String,
) -> Result<Finding, Failure> {
    search(&app, source, &Wanted::Text(&text))
}

/// The starred Artifacts of `branch`'s project that `source` has, in the graph's order.
#[tauri::command]
#[specta::specta]
pub async fn favorites(
    app: tauri::State<'_, App>,
    source: Source,
    branch: BranchKey,
) -> Result<Finding, Failure> {
    let starred = starred_of(&app, &branch)?;
    search(&app, source, &Wanted::Starred(&starred))
}

/// The starred Artifacts of `branch`'s project, by name.
#[tauri::command]
#[specta::specta]
pub async fn starred_artifacts(
    app: tauri::State<'_, App>,
    branch: BranchKey,
) -> Result<Vec<String>, Failure> {
    Ok(starred_of(&app, &branch)?
        .into_iter()
        .map(String::from)
        .collect())
}

/// Star the Artifact `artifact` for every branch of `branch`'s project, or take its star away;
/// answers the project's starred Artifacts as kept.
#[tauri::command]
#[specta::specta]
pub async fn star_artifact(
    app: tauri::State<'_, App>,
    branch: BranchKey,
    artifact: String,
    on: bool,
) -> Result<Vec<String>, Failure> {
    let name: ArtifactName = artifact.parse().map_err(Failure::Setting)?;
    let kept = live::star_artifact(
        Some(&app.launch.state),
        &branch.to_place().project(),
        &name,
        on,
    )
    .map_err(Failure::Setting)?;
    Ok(kept.into_iter().map(String::from).collect())
}

fn starred_of(app: &App, branch: &BranchKey) -> Result<BTreeSet<ArtifactName>, Failure> {
    live::starred_artifacts(Some(&app.launch.state), &branch.to_place().project())
        .map_err(Failure::Read)
}

#[cfg(test)]
mod tests;
