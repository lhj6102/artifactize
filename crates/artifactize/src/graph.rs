//! Dependency edges, strongly connected components, gates, and obligations.

use std::collections::{BTreeMap, BTreeSet};

use petgraph::{algo::kosaraju_scc, graph::DiGraph};
use thiserror::Error;

use crate::{
    config::{ArtifactName, Profile, RepoConfig},
    runtime::Verdict,
};

#[derive(Debug, Error)]
#[error("{0}")]
pub struct GraphError(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactNode<'a> {
    pub basis: bool,
    pub evals: Vec<&'a str>,
    pub component: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component<'a> {
    pub artifacts: Vec<&'a str>,
    /// Direct condensation dependencies, always earlier in the component list.
    pub dependencies: Vec<usize>,
    /// External Eval gates shared by every member; never includes SCC peers.
    pub gates: Vec<&'a str>,
}

#[derive(Debug)]
pub struct Graph<'a> {
    artifacts: BTreeMap<&'a str, ArtifactNode<'a>>,
    evals: BTreeMap<&'a str, &'a str>,
    components: Vec<Component<'a>>,
    derived: BTreeMap<&'a str, Vec<&'a str>>,
    derived_order: Vec<&'a str>,
}

impl<'a> Graph<'a> {
    /// Build from scope's resolved relations without reading files or running code.
    pub fn new(config: &'a RepoConfig) -> Result<Self, GraphError> {
        let mut graph = DiGraph::<&str, ()>::new();
        let nodes: BTreeMap<_, _> = config
            .artifacts
            .keys()
            .map(|id| (id.as_str(), graph.add_node(id.as_str())))
            .collect();
        let mut artifacts: BTreeMap<_, _> = config
            .artifacts
            .iter()
            .map(|(id, artifact)| {
                (
                    id.as_str(),
                    ArtifactNode {
                        basis: artifact.basis == Some(true),
                        evals: Vec::new(),
                        component: 0,
                    },
                )
            })
            .collect();
        let mut evals = BTreeMap::new();
        for eval in &config.evals {
            let artifact = artifacts
                .get_mut(eval.target.as_str())
                .ok_or_else(|| GraphError(format!("Unknown Eval target: {}", eval.target)))?;
            if artifact.basis {
                return Err(GraphError(format!(
                    "Basis Artifact {} cannot own Evals.",
                    eval.target
                )));
            }
            if evals
                .insert(eval.id.as_str(), eval.target.as_str())
                .is_some()
            {
                return Err(GraphError(format!("Duplicate Eval: {}", eval.id)));
            }
            artifact.evals.push(eval.id.as_str());
        }
        for artifact in artifacts.values_mut() {
            artifact.evals.sort_unstable();
        }
        let mut edges = BTreeSet::new();
        for relation in &config.relations {
            for id in [&relation.source, &relation.target] {
                if !nodes.contains_key(id.as_str()) {
                    return Err(GraphError(format!("Unknown relation Artifact: {id}")));
                }
            }
            edges.insert((relation.target.as_str(), relation.source.as_str()));
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
                artifacts.get_mut(id).unwrap().component = index;
            }
        }
        let mut dependencies = vec![BTreeSet::new(); components.len()];
        for (consumer, dependency) in edges {
            let consumer = artifacts[consumer].component;
            let dependency = artifacts[dependency].component;
            if consumer != dependency {
                debug_assert!(dependency < consumer);
                dependencies[consumer].insert(dependency);
            }
        }
        for (index, dependencies) in dependencies.into_iter().enumerate() {
            let mut gates: Vec<_> = dependencies
                .iter()
                .flat_map(|&dependency| &components[dependency].artifacts)
                .flat_map(|id| &artifacts[id].evals)
                .copied()
                .collect();
            gates.sort_unstable();
            components[index].dependencies = dependencies.into_iter().collect();
            components[index].gates = gates;
        }
        let derived: BTreeMap<_, _> = config
            .evals
            .iter()
            .filter(|eval| matches!(eval.declaration.profile, Profile::Dependency { .. }))
            .map(|eval| {
                (
                    eval.id.as_str(),
                    eval.deps
                        .iter()
                        .map(ArtifactName::as_str)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let mut waits = DiGraph::<&str, ()>::new();
        let waiting: BTreeMap<_, _> = derived.keys().map(|&id| (id, waits.add_node(id))).collect();
        for (&id, targets) in &derived {
            for target in targets {
                for dependency in &artifacts[target].evals {
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
                    cycles.join(", ")
                ))
            })?
            .into_iter()
            .rev()
            .map(|node| waits[node])
            .collect();
        Ok(Self {
            artifacts,
            evals,
            components,
            derived,
            derived_order,
        })
    }

    pub fn artifacts(&self) -> &BTreeMap<&'a str, ArtifactNode<'a>> {
        &self.artifacts
    }

    /// Components are dependency-first; members and gates are lexically ordered.
    pub fn components(&self) -> &[Component<'a>] {
        &self.components
    }

    /// Required material and verification, including roots and all cycle peers.
    pub fn dependency_closure(&self, roots: &[&str]) -> Result<Vec<&'a str>, GraphError> {
        let mut pending = Vec::new();
        for root in roots {
            let artifact = self
                .artifacts
                .get(root)
                .ok_or_else(|| GraphError(format!("Unknown Artifact: {root}")))?;
            pending.push(artifact.component);
        }
        let mut visited = BTreeSet::new();
        let mut required = BTreeSet::new();
        while let Some(index) = pending.pop() {
            if !visited.insert(index) {
                continue;
            }
            let component = &self.components[index];
            required.extend(component.artifacts.iter().copied());
            pending.extend(&component.dependencies);
        }
        Ok(required.into_iter().collect())
    }

    /// Evaluate the whole graph. Evidence freshness is supplied by the caller;
    /// absent entries are unreviewed, and unrelated entries are ignored.
    pub fn evaluate(&self, evidence: &BTreeMap<String, Evidence>) -> Evaluation<'a> {
        self.evaluate_with_policy(evidence, false)
    }

    /// Gate bypass changes readiness, never the required final obligations.
    pub fn evaluate_with_policy(
        &self,
        evidence: &BTreeMap<String, Evidence>,
        ignore_gates: bool,
    ) -> Evaluation<'a> {
        let mut evals: BTreeMap<&str, EvalEvaluation<'_>> = BTreeMap::new();
        for component in &self.components {
            let unmet_gates: Vec<_> = component
                .gates
                .iter()
                .copied()
                .filter(|id| !ignore_gates && evals[id].status != EvalStatus::Green)
                .collect();
            let readiness = if unmet_gates
                .iter()
                .any(|id| matches!(evals[id].status, EvalStatus::Red | EvalStatus::Blocked))
            {
                Readiness::Blocked
            } else if unmet_gates.is_empty() {
                Readiness::Ready
            } else {
                Readiness::Wait
            };
            let ordinary = component
                .artifacts
                .iter()
                .flat_map(|id| &self.artifacts[id].evals)
                .copied()
                .filter(|id| !self.derived.contains_key(id));
            let derived = self
                .derived_order
                .iter()
                .copied()
                .filter(|id| component.artifacts.contains(&self.evals[id]));
            for id in ordinary.chain(derived) {
                if let Some(targets) = self.derived.get(id) {
                    if evidence.get(id) == Some(&Evidence::OperationalError) {
                        evals.insert(
                            id,
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
                        .flat_map(|target| &self.artifacts[target].evals)
                        .copied()
                        .filter(|id| evals[id].status != EvalStatus::Green)
                        .collect();
                    let mut blocked_by = Vec::new();
                    for target in targets {
                        let pending: Vec<_> = self.artifacts[target]
                            .evals
                            .iter()
                            .copied()
                            .filter(|id| evals[id].status != EvalStatus::Green)
                            .collect();
                        if !pending.is_empty() {
                            blocked_by.push(*target);
                            blocked_by.extend(pending);
                        }
                    }
                    let readiness = if unmet
                        .iter()
                        .any(|id| matches!(evals[id].status, EvalStatus::Red | EvalStatus::Blocked))
                    {
                        Readiness::Blocked
                    } else if unmet.is_empty() {
                        Readiness::Ready
                    } else {
                        Readiness::Wait
                    };
                    evals.insert(
                        id,
                        EvalEvaluation {
                            status: match readiness {
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
                let evidence = evidence.get(id).copied();
                let status = match readiness {
                    Readiness::Blocked => EvalStatus::Blocked,
                    Readiness::Wait => EvalStatus::Wait,
                    Readiness::Ready => match evidence {
                        Some(Evidence::Current(Verdict::Green)) => EvalStatus::Green,
                        Some(Evidence::Current(Verdict::Red)) => EvalStatus::Red,
                        Some(Evidence::OperationalError) => EvalStatus::Error,
                        Some(Evidence::Stale) => EvalStatus::Stale,
                        None => EvalStatus::Unreviewed,
                    },
                };
                evals.insert(
                    id,
                    EvalEvaluation {
                        readiness,
                        status,
                        evidence,
                        unmet_gates: unmet_gates.clone(),
                        blocked_by: unmet_gates.clone(),
                        derived: false,
                    },
                );
            }
        }
        let own_satisfied: BTreeMap<_, _> = self
            .artifacts
            .iter()
            .map(|(&id, artifact)| {
                let satisfied = artifact.basis
                    || (!artifact.evals.is_empty()
                        && artifact
                            .evals
                            .iter()
                            .all(|id| evals[id].status == EvalStatus::Green));
                (id, satisfied)
            })
            .collect();
        let mut satisfied = vec![false; self.components.len()];
        for (index, component) in self.components.iter().enumerate() {
            satisfied[index] = component.artifacts.iter().all(|id| own_satisfied[id])
                && component.dependencies.iter().all(|&index| satisfied[index]);
        }
        let artifacts = self
            .artifacts
            .iter()
            .map(|(&id, artifact)| {
                let satisfied = satisfied[artifact.component];
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
                        (EvalStatus::Stale, ArtifactStatus::Stale),
                        (EvalStatus::Unreviewed, ArtifactStatus::Unreviewed),
                    ]
                    .into_iter()
                    .find_map(|(eval_status, artifact_status)| {
                        artifact
                            .evals
                            .iter()
                            .any(|id| evals[id].status == eval_status)
                            .then_some(artifact_status)
                    })
                    .unwrap_or(if own_satisfied[id] {
                        ArtifactStatus::Incomplete
                    } else {
                        ArtifactStatus::Unreviewed
                    })
                };
                (
                    id,
                    ArtifactEvaluation {
                        status,
                        own_satisfied: own_satisfied[id],
                        satisfied,
                        passed: artifact
                            .evals
                            .iter()
                            .filter(|id| evals[*id].status == EvalStatus::Green)
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

    pub fn eval_target(&self, id: &str) -> Option<&'a str> {
        self.evals.get(id).copied()
    }
}

/// Current means usable for this input/run, not merely the latest stored result.
/// Stale evidence carries no current verdict; operational failure is not RED.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    Current(Verdict),
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
    pub unmet_gates: Vec<&'a str>,
    pub blocked_by: Vec<&'a str>,
    pub derived: bool,
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
    pub evals: BTreeMap<&'a str, EvalEvaluation<'a>>,
    pub artifacts: BTreeMap<&'a str, ArtifactEvaluation>,
    /// Artifacts whose own obligations are unmet, not execution gates.
    pub obligations: Vec<&'a str>,
    pub status: FinalStatus,
}

#[cfg(test)]
mod tests;
