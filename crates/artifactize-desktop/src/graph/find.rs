//! The graph's find field: the Artifacts and evals of a Run or a Current whose names contain
//! a text, in the graph's order, each with the node and tab its Detail opens on.
use super::{Source, node_id};
use crate::{App, Failure, ids::NodeKey, status::Tone};
use artifactize_core::monitor::{Kind, Node, NodeId};
use serde::Serialize;
use specta::Type;

/// The most found names listed; the rest are only counted.
pub const LISTED: usize = 50;

/// An Artifact or an eval whose name contains the text.
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

/// The Artifacts and evals of `artifacts`, each Artifact with its evals, whose names contain
/// `text`, ignoring case. An empty text finds nothing.
pub(crate) fn find_in<'a, E>(
    artifacts: impl IntoIterator<Item = (Named<'a>, E)>,
    text: &str,
) -> Finding
where
    E: IntoIterator<Item = Named<'a>>,
{
    let wanted = text.trim().to_lowercase();
    let mut found = Vec::new();
    let mut more = 0u32;
    if wanted.is_empty() {
        return Finding { found, more };
    }
    let mut take = |item: Found| {
        if found.len() < LISTED {
            found.push(item);
        } else {
            more = more.saturating_add(1);
        }
    };
    let matches = |name: &str| name.to_lowercase().contains(&wanted);
    for (artifact, evals) in artifacts {
        let node: NodeKey = artifact.id.into();
        if matches(artifact.name) {
            take(Found {
                node: node.clone(),
                tab: None,
                artifact: artifact.name.to_owned(),
                eval: None,
                glyph: artifact.glyph.to_owned(),
                tone: artifact.tone,
            });
        }
        for eval in evals.into_iter().filter(|eval| matches(eval.name)) {
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

/// The Artifacts and evals of `source` whose names contain `text`: the shown Run's, or the
/// branch's Current as its last scan found it.
#[tauri::command]
#[specta::specta]
pub async fn find(
    app: tauri::State<'_, App>,
    source: Source,
    text: String,
) -> Result<Finding, Failure> {
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
                &text,
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
                &text,
            ))
        }
    }
}

#[cfg(test)]
mod tests;
