//! A Run as a graph of Artifacts: one node per Artifact of the tree, one edge per Artifact an
//! Artifact's evals depend on, each with one of six lights taken from the tree's own states.
use std::collections::{BTreeMap, BTreeSet};

use super::{
    NodeId, Target,
    states::{Activity, Busy, Completion, EvalView, NotRun},
    tree::{Kind, Node},
};
use crate::types::ArtifactName;

/// How a graph node or edge is lit. Declared in precedence order: an Artifact takes the first
/// light any of its evals has (RED before ERROR before in progress before blocked before
/// waiting before GREEN).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Light {
    /// An eval's verdict is RED.
    Red,
    /// An eval failed operationally: ERROR, not a verdict.
    Error,
    /// Running, or waiting for its Human.
    Active,
    /// Cannot proceed: an upstream Artifact is RED, ERROR or itself blocked.
    Blocked,
    /// Runnable but still waiting: queued, for upstream Artifacts that are not done yet, or
    /// not run in a finished Run.
    Waiting,
    /// Every eval is GREEN (an Artifact without evals is complete too).
    Green,
}

impl From<Completion> for Light {
    /// An upstream Artifact's light where the graph has not lit it (a cycle peer lit later).
    fn from(completion: Completion) -> Self {
        match completion {
            Completion::Complete => Self::Green,
            Completion::Failed { verdict: true } => Self::Red,
            Completion::Failed { verdict: false } => Self::Error,
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
            EvalView::Failed { verdict: true } => Self::Red,
            EvalView::Failed { verdict: false } => Self::Error,
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

/// Gate failures come from the Eval-level schedule, never an Artifact-wide inference.
fn eval_light(eval: &Node, view: &EvalView) -> Light {
    if eval.held
        && matches!(
            view,
            EvalView::WaitingOn(_) | EvalView::NotRun(NotRun::Dependency(_))
        )
    {
        Light::Blocked
    } else {
        Light::from(view)
    }
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
    /// The input Artifact's current light.
    pub light: Light,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    /// In the tree's order: upstream first where the graph allows.
    pub nodes: Vec<GraphNode>,
    /// By `to` in node order, then `from` in node order.
    pub edges: Vec<GraphEdge>,
}

impl Graph {
    /// Whether `other` has the same Artifacts and edges in the same order, whatever their
    /// lights: a view can keep where it placed them and only relight them.
    pub fn same_shape(&self, other: &Self) -> bool {
        self.nodes.len() == other.nodes.len()
            && self.edges.len() == other.edges.len()
            && self
                .nodes
                .iter()
                .zip(&other.nodes)
                .all(|(one, other)| one.id == other.id)
            && self
                .edges
                .iter()
                .zip(&other.edges)
                .all(|(one, other)| one.from == other.from && one.to == other.to)
    }
}

/// The graph of the Artifact rows of a Run's [`tree`](super::tree), lit upstream first in the
/// tree's order.
pub fn graph(tree: &[Node]) -> Graph {
    let artifacts: Vec<(ArtifactName, &Node, Completion)> = tree
        .iter()
        .filter_map(|node| match node.kind {
            Kind::Artifact { completion, .. } => match Target::from(&node.id) {
                Target::Artifact(name) => Some((name, node, completion)),
                _ => None,
            },
            Kind::Eval(_) => None,
        })
        .collect();
    let mut lights: BTreeMap<ArtifactName, Light> = BTreeMap::new();
    for (name, node, completion) in &artifacts {
        // The first light of its evals in precedence order.
        let evals = node
            .children
            .iter()
            .filter_map(|eval| match &eval.kind {
                Kind::Eval(view) => Some(eval_light(eval, view)),
                Kind::Artifact { .. } => None,
            })
            .min()
            .unwrap_or(Light::Green);
        // Every eval done, yet graph.rs still holds them on their own gates, or a constituent
        // is not done: not complete.
        let light = match evals {
            Light::Green => Light::from(*completion),
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
        let upstream: BTreeSet<_> = node
            .children
            .iter()
            .flat_map(|eval| eval.upstream.iter().map(|dependency| &dependency.artifact))
            .collect();
        for from in upstream {
            let Some(&own) = lights.get(from).filter(|_| from != name) else {
                continue;
            };
            edges.push(GraphEdge {
                from: from.clone(),
                to: name.clone(),
                light: own,
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
