//! A Run as a graph of Artifacts: one node per Artifact of the tree, one edge per Artifact an
//! Artifact's evals depend on, each with one of five lights taken from the tree's own states.
use std::collections::BTreeMap;

use super::{
    NodeId,
    states::{Activity, Busy, Completion, EvalView, NotRun},
    tree::{Kind, Node},
};
use crate::types::ArtifactName;

/// How a graph node or edge is lit, most urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Light {
    /// An eval is RED or ERROR.
    Failed,
    /// Cannot proceed: an upstream Artifact failed or is itself blocked.
    Blocked,
    /// Running, or waiting for its Human.
    Active,
    /// Runnable but still waiting: queued, for upstream Artifacts that are not done yet, or
    /// not run in a finished Run.
    Waiting,
    /// Every eval is GREEN (an Artifact without evals is complete too).
    Green,
}

impl From<Completion> for Light {
    fn from(completion: Completion) -> Self {
        match completion {
            Completion::Complete => Self::Green,
            Completion::Failed { .. } => Self::Failed,
            Completion::InProgress(Busy::Running | Busy::Human) => Self::Active,
            Completion::InProgress(Busy::Queued) => Self::Waiting,
            Completion::Waiting { blocked: true } => Self::Blocked,
            Completion::Waiting { blocked: false } => Self::Waiting,
        }
    }
}

impl From<&EvalView> for Light {
    fn from(view: &EvalView) -> Self {
        match view {
            EvalView::Done(_) => Self::Green,
            EvalView::Failed { .. } => Self::Failed,
            EvalView::InProgress(Activity::Running | Activity::Human(_)) => Self::Active,
            EvalView::BlockedBy(_) => Self::Blocked,
            EvalView::InProgress(Activity::Queued(_))
            | EvalView::WaitingOn(_)
            | EvalView::NotRun(
                NotRun::Dependency(_)
                | NotRun::Budget
                | NotRun::Stale
                | NotRun::Unreviewed
                | NotRun::Absent,
            ) => Self::Waiting,
        }
    }
}

impl Light {
    /// A failed or blocked Artifact holds everything downstream.
    fn holds(self) -> bool {
        matches!(self, Self::Failed | Self::Blocked)
    }
}

/// An eval row's light. An eval waiting for an upstream Artifact that failed (an ERROR too,
/// which the tree shows as "waits for … (retry … first)") or is itself blocked cannot proceed
/// in this Run: blocked. `lights` holds the Artifacts already lit, upstream first.
fn eval_light(eval: &Node, view: &EvalView, lights: &BTreeMap<ArtifactName, Light>) -> Light {
    let light = Light::from(view);
    let waits = matches!(
        view,
        EvalView::WaitingOn(_) | EvalView::NotRun(NotRun::Dependency(_))
    );
    let held = eval.upstream.iter().any(|upstream| {
        lights
            .get(&upstream.artifact)
            .copied()
            .unwrap_or(upstream.completion.into())
            .holds()
    });
    if waits && held { Light::Blocked } else { light }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    /// The Artifact row's tree id, `a:<artifact>`.
    pub id: NodeId,
    pub name: ArtifactName,
    pub light: Light,
}

/// `to`'s evals depend on `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: ArtifactName,
    pub to: ArtifactName,
    /// The most urgent light of `to`'s dependency evals naming `from`, or without one
    /// `from`'s own light.
    pub light: Light,
    /// A dependency eval of `to` names `from`.
    pub dependency: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    /// In the tree's order: upstream first where the graph allows.
    pub nodes: Vec<GraphNode>,
    /// By `to` in node order, then `from` in node order.
    pub edges: Vec<GraphEdge>,
}

/// The graph of the Artifact rows of a Run's [`tree`](super::tree), lit upstream first in the
/// tree's order.
pub fn graph(tree: &[Node]) -> Graph {
    let artifacts: Vec<(ArtifactName, &Node, Completion)> = tree
        .iter()
        .filter_map(|node| match node.kind {
            Kind::Artifact { completion, .. } => {
                node.name.parse().ok().map(|name| (name, node, completion))
            }
            Kind::Eval(_) => None,
        })
        .collect();
    let mut lights: BTreeMap<ArtifactName, Light> = BTreeMap::new();
    for (name, node, completion) in &artifacts {
        let blocked = node.children.iter().any(|eval| match &eval.kind {
            Kind::Eval(view) => eval_light(eval, view, &lights) == Light::Blocked,
            Kind::Artifact { .. } => false,
        });
        let light = match Light::from(*completion) {
            Light::Waiting if blocked => Light::Blocked,
            light => light,
        };
        lights.insert(name.clone(), light);
    }
    let position: BTreeMap<&ArtifactName, usize> = artifacts
        .iter()
        .enumerate()
        .map(|(index, (name, _, _))| (name, index))
        .collect();
    let mut edges = Vec::new();
    for (name, node, _) in &artifacts {
        // Each upstream Artifact, with the most urgent light of the dependency evals naming it.
        let mut upstream: BTreeMap<&ArtifactName, Option<Light>> = BTreeMap::new();
        for eval in &node.children {
            let Kind::Eval(view) = &eval.kind else {
                continue;
            };
            for dependency in &eval.upstream {
                let lit = upstream.entry(&dependency.artifact).or_default();
                if eval.dependency {
                    let light = eval_light(eval, view, &lights);
                    *lit = Some(lit.map_or(light, |lit| lit.min(light)));
                }
            }
        }
        for (from, lit) in upstream {
            let Some(&own) = lights.get(from).filter(|_| from != name) else {
                continue;
            };
            edges.push(GraphEdge {
                from: from.clone(),
                to: name.clone(),
                light: lit.unwrap_or(own),
                dependency: lit.is_some(),
            });
        }
    }
    edges.sort_by_key(|edge| (position[&edge.to], position[&edge.from]));
    Graph {
        nodes: artifacts
            .into_iter()
            .map(|(name, node, _)| GraphNode {
                id: node.id.clone(),
                light: lights[&name],
                name,
            })
            .collect(),
        edges,
    }
}
