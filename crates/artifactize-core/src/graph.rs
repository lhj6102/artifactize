//! Dependency edges, strongly connected components, gates, and obligations.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};

use petgraph::{algo::kosaraju_scc, graph::DiGraph};
use thiserror::Error;

use crate::{
    config::{ArtifactName, EvalId, Profile, RepoConfig},
    runtime::Verdict,
};

#[derive(Debug, Error)]
#[error("{0}")]
pub struct GraphError(pub String);

/// A strongly-connected component's position in `Graph::components`, guarding it from
/// arbitrary `usize` arithmetic: dependency-first, so a lower id is never a later
/// component's dependency.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct ComponentId(usize);

impl ComponentId {
    /// Live graph and legacy saved components derive their identity from a list position.
    /// Saved explicit IDs deserialize as the same unsigned integer, never arithmetic indices.
    pub(crate) fn from_position(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

impl std::fmt::Display for ComponentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactNode<'a> {
    pub basis: bool,
    pub evals: Vec<&'a EvalId>,
    pub component: ComponentId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component<'a> {
    pub artifacts: Vec<&'a ArtifactName>,
    /// Direct condensation dependencies, always earlier in the component list.
    pub dependencies: Vec<ComponentId>,
    /// External Eval gates shared by every member; never includes SCC peers.
    pub gates: Vec<&'a EvalId>,
}

#[derive(Debug)]
pub struct Graph<'a> {
    artifacts: BTreeMap<ArtifactName, ArtifactNode<'a>>,
    evals: BTreeMap<EvalId, &'a ArtifactName>,
    components: Vec<Component<'a>>,
    derived: BTreeMap<EvalId, Vec<&'a ArtifactName>>,
    derived_order: Vec<&'a EvalId>,
}

impl<'a> Graph<'a> {
    /// Build from scope's resolved relations without reading files or running code.
    pub fn new(config: &'a RepoConfig) -> Result<Self, GraphError> {
        let mut graph = DiGraph::<&ArtifactName, ()>::new();
        let nodes: BTreeMap<_, _> = config
            .artifacts
            .keys()
            .map(|id| (id, graph.add_node(id)))
            .collect();
        let mut artifacts: BTreeMap<_, _> = config
            .artifacts
            .iter()
            .map(|(id, artifact)| {
                (
                    id.clone(),
                    ArtifactNode {
                        basis: artifact.basis == Some(true),
                        evals: Vec::new(),
                        component: ComponentId(0),
                    },
                )
            })
            .collect();
        let mut evals = BTreeMap::new();
        for eval in &config.evals {
            let artifact = artifacts
                .get_mut(&eval.target)
                .ok_or_else(|| GraphError(format!("Unknown Eval target: {}", eval.target)))?;
            if artifact.basis {
                return Err(GraphError(format!(
                    "Basis Artifact {} cannot own Evals.",
                    eval.target
                )));
            }
            if evals.insert(eval.id.clone(), &eval.target).is_some() {
                return Err(GraphError(format!("Duplicate Eval: {}", eval.id)));
            }
            artifact.evals.push(&eval.id);
        }
        for artifact in artifacts.values_mut() {
            artifact.evals.sort_unstable();
        }
        let mut edges = BTreeSet::new();
        for relation in &config.relations {
            for id in [&relation.source, &relation.target] {
                if !nodes.contains_key(id) {
                    return Err(GraphError(format!("Unknown relation Artifact: {id}")));
                }
            }
            edges.insert((&relation.target, &relation.source));
        }
        // Consumer -> input makes iterative SCC traversal dependency-first.
        for &(consumer, dependency) in &edges {
            graph.add_edge(nodes[consumer], nodes[dependency], ());
        }
        let mut components: Vec<_> = kosaraju_scc(&graph)
            .into_iter()
            .map(|members| {
                let mut artifacts: Vec<_> = members.into_iter().map(|node| graph[node]).collect();
                artifacts.sort_unstable();
                Component {
                    artifacts,
                    dependencies: Vec::new(),
                    gates: Vec::new(),
                }
            })
            .collect();
        for (index, component) in components.iter().enumerate() {
            for id in &component.artifacts {
                artifacts.get_mut(id.as_str()).unwrap().component = ComponentId(index);
            }
        }
        let mut dependencies = vec![BTreeSet::new(); components.len()];
        for (consumer, dependency) in edges {
            let consumer = artifacts[consumer.as_str()].component;
            let dependency = artifacts[dependency.as_str()].component;
            if consumer != dependency {
                debug_assert!(dependency < consumer);
                dependencies[consumer.index()].insert(dependency);
            }
        }
        for (index, dependencies) in dependencies.into_iter().enumerate() {
            let mut gates: Vec<_> = dependencies
                .iter()
                .flat_map(|&dependency| &components[dependency.index()].artifacts)
                .flat_map(|id| &artifacts[id.as_str()].evals)
                .copied()
                .collect();
            gates.sort_unstable();
            components[index].dependencies = dependencies.into_iter().collect();
            components[index].gates = gates;
        }
        let derived: BTreeMap<_, _> = config
            .evals
            .iter()
            .filter(|eval| matches!(eval.declaration.profile(), Profile::Dependency { .. }))
            .map(|eval| (eval.id.clone(), eval.deps.iter().collect::<Vec<_>>()))
            .collect();
        let mut waits = DiGraph::<&EvalId, ()>::new();
        let waiting: BTreeMap<_, _> = derived.keys().map(|id| (id, waits.add_node(id))).collect();
        for (id, targets) in &derived {
            for target in targets {
                for dependency in &artifacts[target.as_str()].evals {
                    if let Some(&node) = waiting.get(dependency) {
                        waits.add_edge(waiting[id], node, ());
                    }
                }
            }
        }
        let derived_order = petgraph::algo::toposort(&waits, None)
            .map_err(|_| {
                let mut cycles: Vec<_> = kosaraju_scc(&waits)
                    .into_iter()
                    .filter(|members| members.len() > 1)
                    .flatten()
                    .map(|node| waits[node])
                    .collect();
                cycles.sort_unstable();
                GraphError(format!(
                    "Dependency Eval cycle: {}. Dependency Evals cannot wait on each other.",
                    cycles
                        .iter()
                        .map(|id| id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?
            .into_iter()
            .rev()
            .map(|node| {
                config
                    .evals
                    .iter()
                    .find(|eval| eval.id == *waits[node])
                    .expect("derived Eval exists")
                    .id
                    .borrow()
            })
            .collect();
        Ok(Self {
            artifacts,
            evals,
            components,
            derived,
            derived_order,
        })
    }

    pub fn artifacts(&self) -> &BTreeMap<ArtifactName, ArtifactNode<'a>> {
        &self.artifacts
    }

    /// Components are dependency-first; members and gates are lexically ordered.
    pub fn components(&self) -> &[Component<'a>] {
        &self.components
    }

    /// Required material and verification, including roots and all cycle peers.
    pub fn dependency_closure(
        &self,
        roots: &[&ArtifactName],
    ) -> Result<Vec<&'a ArtifactName>, GraphError> {
        let mut pending = Vec::new();
        for root in roots {
            let artifact = self
                .artifacts
                .get(*root)
                .ok_or_else(|| GraphError(format!("Unknown Artifact: {root}")))?;
            pending.push(artifact.component);
        }
        let mut visited = BTreeSet::new();
        let mut required = BTreeSet::new();
        while let Some(index) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            let component = &self.components[index.index()];
            required.extend(component.artifacts.iter().copied());
            pending.extend(&component.dependencies);
        }
        Ok(required.into_iter().collect())
    }

    /// Evaluate the whole graph. Evidence freshness is supplied by the caller;
    /// absent entries are unreviewed, and unrelated entries are ignored.
    pub fn evaluate(&self, evidence: &BTreeMap<EvalId, Evidence>) -> Evaluation<'_> {
        self.evaluate_for_run(evidence, false)
    }

    /// Speculation turns operational failures into immediate blockers, without changing
    /// the default read/verification policy of waiting for replacement GREEN evidence.
    pub(crate) fn evaluate_for_run(
        &self,
        evidence: &BTreeMap<EvalId, Evidence>,
        errors_block: bool,
    ) -> Evaluation<'_> {
        let mut evals: BTreeMap<EvalId, EvalEvaluation<'_>> = BTreeMap::new();
        for component in &self.components {
            let unmet_gates: Vec<_> = component
                .gates
                .iter()
                .copied()
                .filter(|id| !evals[id.as_str()].status.passes_gate())
                .collect();
            let readiness = if unmet_gates.iter().any(|id| {
                matches!(
                    evals[id.as_str()].status,
                    EvalStatus::Red | EvalStatus::Blocked
                ) || (errors_block && evals[id.as_str()].status == EvalStatus::Error)
            }) {
                Readiness::Blocked
            } else if unmet_gates.is_empty() {
                Readiness::Ready
            } else {
                Readiness::Wait
            };
            let ordinary = component
                .artifacts
                .iter()
                .flat_map(|id| &self.artifacts[id.as_str()].evals)
                .copied()
                .filter(|id| !self.derived.contains_key(id.as_str()));
            let derived = self
                .derived_order
                .iter()
                .copied()
                .filter(|id| component.artifacts.contains(&self.evals[id.as_str()]));
            for id in ordinary.chain(derived) {
                if let Some(targets) = self.derived.get(id.as_str()) {
                    if evidence.get(id.as_str()) == Some(&Evidence::OperationalError) {
                        evals.insert(
                            id.clone(),
                            EvalEvaluation {
                                status: EvalStatus::Error,
                                readiness: Readiness::Ready,
                                evidence: Some(Evidence::OperationalError),
                                unmet_gates: Vec::new(),
                                blocked_by: Vec::new(),
                                derived: true,
                            },
                        );
                        continue;
                    }
                    let unmet: Vec<_> = targets
                        .iter()
                        .flat_map(|target| &self.artifacts[target.as_str()].evals)
                        .copied()
                        .filter(|id| !evals[id.as_str()].status.passes_gate())
                        .collect();
                    let mut blocked_by = Vec::new();
                    for target in targets {
                        let pending: Vec<_> = self.artifacts[target.as_str()]
                            .evals
                            .iter()
                            .copied()
                            .filter(|id| !evals[id.as_str()].status.passes_gate())
                            .collect();
                        if !pending.is_empty() {
                            blocked_by.push(BlockedReference::Artifact(target));
                            blocked_by.extend(pending.into_iter().map(BlockedReference::Eval));
                        }
                    }
                    let readiness = if unmet.iter().any(|id| {
                        matches!(
                            evals[id.as_str()].status,
                            EvalStatus::Red | EvalStatus::Blocked
                        ) || (errors_block && evals[id.as_str()].status == EvalStatus::Error)
                    }) {
                        Readiness::Blocked
                    } else if unmet.is_empty() {
                        Readiness::Ready
                    } else {
                        Readiness::Wait
                    };
                    evals.insert(
                        id.clone(),
                        EvalEvaluation {
                            status: match readiness {
                                Readiness::Ready
                                    if unmet.is_empty()
                                        && targets
                                            .iter()
                                            .flat_map(|target| {
                                                &self.artifacts[target.as_str()].evals
                                            })
                                            .any(|id| {
                                                matches!(
                                                    evals[id.as_str()].status,
                                                    EvalStatus::Deferred | EvalStatus::Provisional
                                                )
                                            }) =>
                                {
                                    EvalStatus::Provisional
                                }
                                Readiness::Ready => EvalStatus::Green,
                                Readiness::Blocked => EvalStatus::Blocked,
                                Readiness::Wait => EvalStatus::Wait,
                            },
                            readiness,
                            evidence: None,
                            unmet_gates: unmet,
                            blocked_by,
                            derived: true,
                        },
                    );
                    continue;
                }
                let provisional = component.gates.iter().any(|id| {
                    matches!(
                        evals[id.as_str()].status,
                        EvalStatus::Deferred | EvalStatus::Provisional
                    )
                });
                let evidence = evidence.get(id.as_str()).copied();
                let status = match readiness {
                    Readiness::Blocked => EvalStatus::Blocked,
                    Readiness::Wait => match evidence {
                        Some(Evidence::Current(Verdict::Red)) if errors_block => EvalStatus::Red,
                        Some(Evidence::OperationalError) if errors_block => EvalStatus::Error,
                        _ => EvalStatus::Wait,
                    },
                    Readiness::Ready => match evidence {
                        Some(Evidence::Deferred) => EvalStatus::Deferred,
                        Some(Evidence::Current(Verdict::Green)) if provisional => {
                            EvalStatus::Provisional
                        }
                        Some(Evidence::Current(Verdict::Green)) => EvalStatus::Green,
                        Some(Evidence::Current(Verdict::Red)) => EvalStatus::Red,
                        Some(Evidence::OperationalError) => EvalStatus::Error,
                        Some(Evidence::Stale) => EvalStatus::Stale,
                        None => EvalStatus::Unreviewed,
                    },
                };
                evals.insert(
                    id.clone(),
                    EvalEvaluation {
                        readiness,
                        status,
                        evidence,
                        unmet_gates: unmet_gates.clone(),
                        blocked_by: unmet_gates
                            .iter()
                            .copied()
                            .map(BlockedReference::Eval)
                            .collect(),
                        derived: false,
                    },
                );
            }
        }
        let own_satisfied: BTreeMap<_, _> = self
            .artifacts
            .iter()
            .map(|(id, artifact)| {
                let satisfied = artifact.basis
                    || (!artifact.evals.is_empty()
                        && artifact
                            .evals
                            .iter()
                            .all(|id| evals[id.as_str()].status == EvalStatus::Green));
                (id, satisfied)
            })
            .collect();
        let mut satisfied = vec![false; self.components.len()];
        for (index, component) in self.components.iter().enumerate() {
            satisfied[index] = component.artifacts.iter().all(|id| own_satisfied[id])
                && component
                    .dependencies
                    .iter()
                    .all(|&dependency| satisfied[dependency.index()]);
        }
        let artifacts = self
            .artifacts
            .iter()
            .map(|(id, artifact)| {
                let satisfied = satisfied[artifact.component.index()];
                let status = if satisfied {
                    if artifact.basis {
                        ArtifactStatus::Basis
                    } else {
                        ArtifactStatus::Green
                    }
                } else {
                    [
                        (EvalStatus::Error, ArtifactStatus::Error),
                        (EvalStatus::Red, ArtifactStatus::Red),
                        (EvalStatus::Blocked, ArtifactStatus::Blocked),
                        (EvalStatus::Wait, ArtifactStatus::Wait),
                        (EvalStatus::Deferred, ArtifactStatus::Deferred),
                        (EvalStatus::Provisional, ArtifactStatus::Provisional),
                        (EvalStatus::Stale, ArtifactStatus::Stale),
                        (EvalStatus::Unreviewed, ArtifactStatus::Unreviewed),
                    ]
                    .into_iter()
                    .find_map(|(eval_status, artifact_status)| {
                        artifact
                            .evals
                            .iter()
                            .any(|id| evals[id.as_str()].status == eval_status)
                            .then_some(artifact_status)
                    })
                    .unwrap_or(if own_satisfied[id] {
                        ArtifactStatus::Incomplete
                    } else {
                        ArtifactStatus::Unreviewed
                    })
                };
                (
                    id.clone(),
                    ArtifactEvaluation {
                        status,
                        own_satisfied: own_satisfied[id],
                        satisfied,
                        passed: artifact
                            .evals
                            .iter()
                            .filter(|id| evals[id.as_str()].status == EvalStatus::Green)
                            .count(),
                        total: artifact.evals.len(),
                    },
                )
            })
            .collect();
        let obligations: Vec<_> = own_satisfied
            .into_iter()
            .filter_map(|(id, satisfied)| (!satisfied).then_some(id))
            .collect();
        let status = if evals.values().any(|c| c.status == EvalStatus::Error) {
            FinalStatus::Error
        } else if evals.values().any(|c| c.status == EvalStatus::Red) {
            FinalStatus::Red
        } else if obligations.is_empty() {
            FinalStatus::Green
        } else {
            FinalStatus::Incomplete
        };
        Evaluation {
            evals,
            artifacts,
            obligations,
            status,
        }
    }

    pub(crate) fn gates(&self, id: &EvalId) -> Vec<&'a EvalId> {
        if let Some(targets) = self.derived.get(id) {
            targets
                .iter()
                .flat_map(|target| self.artifacts[target.as_str()].evals.iter().copied())
                .collect()
        } else {
            self.components[self.artifacts[self.evals[id].as_str()].component.index()]
                .gates
                .clone()
        }
    }

    pub fn eval_target(&self, id: &str) -> Option<&'a ArtifactName> {
        self.evals.get(id).copied()
    }
}

/// Current means usable for this input/run, not merely the latest stored result.
/// Stale evidence carries no current verdict; operational failure is not RED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    Current(Verdict),
    Deferred,
    OperationalError,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Wait,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalStatus {
    Deferred,
    Provisional,
    Green,
    Red,
    Error,
    Stale,
    Unreviewed,
    Wait,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalEvaluation<'a> {
    /// Gate readiness only; READY never requests re-execution of existing evidence.
    pub readiness: Readiness,
    pub status: EvalStatus,
    /// Retained for audit even when gates mask its effective status.
    pub evidence: Option<Evidence>,
    pub unmet_gates: Vec<&'a EvalId>,
    pub blocked_by: Vec<BlockedReference<'a>>,
    pub derived: bool,
}

/// A blocked dependency names either material or an Eval, never an unclassified string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedReference<'a> {
    Artifact(&'a ArtifactName),
    Eval(&'a EvalId),
}
impl BlockedReference<'_> {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Artifact(id) => id.as_str(),
            Self::Eval(id) => id.as_str(),
        }
    }
}
impl std::fmt::Display for BlockedReference<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl EvalStatus {
    pub fn passes_gate(self) -> bool {
        matches!(self, Self::Green | Self::Deferred | Self::Provisional)
    }
}

impl EvalEvaluation<'_> {
    /// Operational errors require an explicit retry, not automatic redispatch.
    pub fn can_execute(&self) -> bool {
        !self.derived
            && self.readiness == Readiness::Ready
            && matches!(self.evidence, None | Some(Evidence::Stale))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactStatus {
    Deferred,
    Provisional,
    Basis,
    Green,
    Incomplete,
    Error,
    Red,
    Blocked,
    Wait,
    Stale,
    Unreviewed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactEvaluation {
    pub status: ArtifactStatus,
    pub own_satisfied: bool,
    /// Includes all required dependencies and SCC peers, even without Evals.
    pub satisfied: bool,
    pub passed: usize,
    pub total: usize,
}

/// Aggregate validation status, never a fabricated Eval verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalStatus {
    Green,
    Red,
    Error,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation<'a> {
    pub evals: BTreeMap<EvalId, EvalEvaluation<'a>>,
    pub artifacts: BTreeMap<ArtifactName, ArtifactEvaluation>,
    /// Artifacts whose own obligations are unmet, not execution gates.
    pub obligations: Vec<&'a ArtifactName>,
    pub status: FinalStatus,
}

#[cfg(test)]
mod tests;

impl serde::Serialize for BlockedReference<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl From<BlockedReference<'_>> for crate::store::Blocker {
    fn from(reference: BlockedReference<'_>) -> Self {
        match reference {
            BlockedReference::Artifact(id) => Self::Artifact(id.clone()),
            BlockedReference::Eval(id) => Self::Eval(id.clone()),
        }
    }
}
