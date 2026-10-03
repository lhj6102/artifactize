//! Dependency edges, strongly connected components, gates, and obligations.

use std::collections::{BTreeMap, BTreeSet};

use petgraph::{algo::kosaraju_scc, graph::DiGraph};
use thiserror::Error;

use crate::{config::RepoConfig, runtime::Verdict};

#[derive(Debug, Error)]
#[error("{0}")]
pub struct GraphError(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactNode<'a> {
    pub basis: bool,
    pub critics: Vec<&'a str>,
    pub component: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component<'a> {
    pub artifacts: Vec<&'a str>,
    /// Direct condensation dependencies, always earlier in the component list.
    pub dependencies: Vec<usize>,
    /// External Critic gates shared by every member; never includes SCC peers.
    pub gates: Vec<&'a str>,
}

#[derive(Debug)]
pub struct Graph<'a> {
    artifacts: BTreeMap<&'a str, ArtifactNode<'a>>,
    critics: BTreeMap<&'a str, &'a str>,
    components: Vec<Component<'a>>,
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
                        critics: Vec::new(),
                        component: 0,
                    },
                )
            })
            .collect();
        let mut critics = BTreeMap::new();
        for critic in &config.critics {
            let artifact = artifacts
                .get_mut(critic.target.as_str())
                .ok_or_else(|| GraphError(format!("Unknown Critic target: {}", critic.target)))?;
            if artifact.basis {
                return Err(GraphError(format!(
                    "Basis Artifact {} cannot own Critics.",
                    critic.target
                )));
            }
            if critics
                .insert(critic.id.as_str(), critic.target.as_str())
                .is_some()
            {
                return Err(GraphError(format!("Duplicate Critic: {}", critic.id)));
            }
            artifact.critics.push(critic.id.as_str());
        }
        for artifact in artifacts.values_mut() {
            artifact.critics.sort_unstable();
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
                .flat_map(|id| &artifacts[id].critics)
                .copied()
                .collect();
            gates.sort_unstable();
            components[index].dependencies = dependencies.into_iter().collect();
            components[index].gates = gates;
        }
        Ok(Self {
            artifacts,
            critics,
            components,
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
        let mut critics: BTreeMap<&str, CriticEvaluation<'_>> = BTreeMap::new();
        for component in &self.components {
            let unmet_gates: Vec<_> = component
                .gates
                .iter()
                .copied()
                .filter(|id| critics[id].status != CriticStatus::Green)
                .collect();
            let readiness = if unmet_gates.iter().any(|id| {
                matches!(
                    critics[id].status,
                    CriticStatus::Red | CriticStatus::Blocked
                )
            }) {
                Readiness::Blocked
            } else if unmet_gates.is_empty() {
                Readiness::Ready
            } else {
                Readiness::Wait
            };
            for id in component
                .artifacts
                .iter()
                .flat_map(|id| &self.artifacts[id].critics)
            {
                let evidence = evidence.get(*id).copied();
                let status = match readiness {
                    Readiness::Blocked => CriticStatus::Blocked,
                    Readiness::Wait => CriticStatus::Wait,
                    Readiness::Ready => match evidence {
                        Some(Evidence::Current(Verdict::Green)) => CriticStatus::Green,
                        Some(Evidence::Current(Verdict::Red)) => CriticStatus::Red,
                        Some(Evidence::OperationalError) => CriticStatus::Error,
                        Some(Evidence::Stale) => CriticStatus::Stale,
                        None => CriticStatus::Unreviewed,
                    },
                };
                critics.insert(
                    *id,
                    CriticEvaluation {
                        readiness,
                        status,
                        evidence,
                        unmet_gates: unmet_gates.clone(),
                    },
                );
            }
        }
        let own_satisfied: BTreeMap<_, _> = self
            .artifacts
            .iter()
            .map(|(&id, artifact)| {
                let satisfied = artifact.basis
                    || (!artifact.critics.is_empty()
                        && artifact
                            .critics
                            .iter()
                            .all(|id| critics[id].status == CriticStatus::Green));
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
                        (CriticStatus::Error, ArtifactStatus::Error),
                        (CriticStatus::Red, ArtifactStatus::Red),
                        (CriticStatus::Blocked, ArtifactStatus::Blocked),
                        (CriticStatus::Wait, ArtifactStatus::Wait),
                        (CriticStatus::Stale, ArtifactStatus::Stale),
                        (CriticStatus::Unreviewed, ArtifactStatus::Unreviewed),
                    ]
                    .into_iter()
                    .find_map(|(critic_status, artifact_status)| {
                        artifact
                            .critics
                            .iter()
                            .any(|id| critics[id].status == critic_status)
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
                            .critics
                            .iter()
                            .filter(|id| critics[*id].status == CriticStatus::Green)
                            .count(),
                        total: artifact.critics.len(),
                    },
                )
            })
            .collect();
        let obligations: Vec<_> = own_satisfied
            .into_iter()
            .filter_map(|(id, satisfied)| (!satisfied).then_some(id))
            .collect();
        let status = if critics.values().any(|c| c.status == CriticStatus::Error) {
            FinalStatus::Error
        } else if critics.values().any(|c| c.status == CriticStatus::Red) {
            FinalStatus::Red
        } else if obligations.is_empty() {
            FinalStatus::Green
        } else {
            FinalStatus::Incomplete
        };
        Evaluation {
            critics,
            artifacts,
            obligations,
            status,
        }
    }

    pub fn critic_target(&self, id: &str) -> Option<&'a str> {
        self.critics.get(id).copied()
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
pub enum CriticStatus {
    Green,
    Red,
    Error,
    Stale,
    Unreviewed,
    Wait,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticEvaluation<'a> {
    /// Gate readiness only; READY never requests re-execution of existing evidence.
    pub readiness: Readiness,
    pub status: CriticStatus,
    /// Retained for audit even when gates mask its effective status.
    pub evidence: Option<Evidence>,
    pub unmet_gates: Vec<&'a str>,
}

impl CriticEvaluation<'_> {
    /// Operational errors require an explicit retry, not automatic redispatch.
    pub fn can_execute(&self) -> bool {
        self.readiness == Readiness::Ready && matches!(self.evidence, None | Some(Evidence::Stale))
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
    /// Includes all required dependencies and SCC peers, even without Critics.
    pub satisfied: bool,
    pub passed: usize,
    pub total: usize,
}

/// Aggregate validation status, never a fabricated Critic verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalStatus {
    Green,
    Red,
    Error,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evaluation<'a> {
    pub critics: BTreeMap<&'a str, CriticEvaluation<'a>>,
    pub artifacts: BTreeMap<&'a str, ArtifactEvaluation>,
    /// Artifacts whose own obligations are unmet, not execution gates.
    pub obligations: Vec<&'a str>,
    pub status: FinalStatus,
}

#[cfg(test)]
mod tests;
