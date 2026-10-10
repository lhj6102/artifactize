//! Eval view states: what one tree row shows, derived from the saved definitions and the
//! current request states with the readiness rules of [`crate::graph`].
//!
//! Dependencies target Artifacts. An ordinary eval waits for every Artifact of its own
//! component's direct dependency components (its gates); a dependency eval waits for its
//! `depends_on` Artifacts, which may be cycle peers. Cycle peers never wait for each other
//! otherwise.
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
};

use super::Saved;
use crate::{
    config::ProfileKind,
    graph::EvalStatus,
    store::RequestView,
    types::{RequestStatus, RunStatus},
};

/// Where a done eval's GREEN came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Executed,
    Reused,
    Derived,
    /// A saved result the Run used for an eval it has no request for.
    Saved,
}

/// Why a queued eval has not started although nothing upstream holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Queue {
    /// Waiting for one of the Run's `--jobs`.
    Jobs,
    /// Waiting for a backend slot (limits.json); holds the backend name.
    Slot(crate::config::Backend),
    /// Waiting for the running execution of the same reuse key.
    Joined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activity {
    Running,
    /// The claimant, when someone has claimed it.
    Human(Option<crate::types::ReviewerId>),
    Queued(Queue),
}

/// The Artifacts an eval waits for, most actionable first, and one level of root cause when
/// every one of them is itself waiting: `(via, root)` reads "via waits for root".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waits {
    pub x: Vec<crate::types::ArtifactName>,
    pub root: Option<(crate::types::ArtifactName, crate::types::ArtifactName)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRun {
    /// A finished Run ended while the eval still waited for these Artifacts.
    Dependency(Waits),
    Budget,
    Stale,
    Unreviewed,
    /// The eval has no request in this Run.
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalView {
    Done(Source),
    /// `verdict` is RED; otherwise an operational ERROR.
    Failed {
        verdict: bool,
    },
    InProgress(Activity),
    /// Only while the Run is running.
    WaitingOn(Waits),
    /// Upstream failed with a RED verdict or is itself blocked.
    BlockedBy(Vec<crate::types::ArtifactName>),
    NotRun(NotRun),
}

impl EvalView {
    /// Roll-up rank, most urgent first: failed, in progress, blocked, waiting, not run, done.
    pub fn rank(&self) -> u8 {
        match self {
            Self::Failed { .. } => 0,
            Self::InProgress(_) => 1,
            Self::BlockedBy(_) => 2,
            Self::WaitingOn(_) => 3,
            Self::NotRun(_) => 4,
            Self::Done(_) => 5,
        }
    }
}

/// The most urgent kind of activity in an in-progress Artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Busy {
    Running,
    Human,
    Queued,
}

/// An Artifact's evaluation as a whole, as shown where another eval waits for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// Every eval is done; also basis Artifacts and Artifacts without evals.
    Complete,
    InProgress(Busy),
    /// Some eval waits, is blocked or did not run.
    Waiting {
        blocked: bool,
    },
    /// `verdict` when an eval is RED, otherwise only operational ERRORs.
    Failed {
        verdict: bool,
    },
}

impl Completion {
    /// Order of X: failed, in progress, waiting.
    fn rank(self) -> u8 {
        match self {
            Self::Failed { .. } => 0,
            Self::InProgress(_) => 1,
            Self::Waiting { .. } => 2,
            Self::Complete => 3,
        }
    }
}

/// One Artifact an eval depends on, with its current completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub artifact: crate::types::ArtifactName,
    pub completion: Completion,
}

/// Per-Run index from saved definitions: display order, evals per Artifact and the
/// Artifacts each eval depends on. Built in one pass over the definitions and requests.
pub(super) struct Index<'a> {
    pub order: Vec<&'a str>,
    position: HashMap<&'a str, usize>,
    requests: HashMap<&'a str, &'a RequestView>,
    pub evals: BTreeMap<&'a str, Vec<&'a str>>,
    /// Eval → direct dependency Artifacts, in display order.
    pub upstream: BTreeMap<&'a str, Vec<&'a str>>,
    pub dependency: BTreeSet<&'a str>,
    /// Artifact → its cycle peers (same component), by name.
    pub peers: BTreeMap<&'a str, Vec<&'a str>>,
    pub cyclic: BTreeSet<&'a str>,
}

type Component = crate::store::definitions::Component;

fn members(component: &Component) -> Vec<&str> {
    component
        .artifacts
        .value()
        .into_iter()
        .flatten()
        .map(crate::types::ArtifactName::as_str)
        .collect()
}

impl<'a> Index<'a> {
    pub fn new(saved: &Saved<'a>) -> Self {
        let order = saved.artifact_ids();
        let position: HashMap<_, _> = order
            .iter()
            .enumerate()
            .map(|(index, id)| (*id, index))
            .collect();
        let mut requests = HashMap::new();
        for view in saved.requests {
            requests
                .entry(view.request.eval_id.as_str())
                .or_insert(view);
        }
        let definitions: HashMap<_, _> = saved
            .eval_definitions()
            .iter()
            .map(|eval| (eval.id.as_str(), eval))
            .collect();
        // Saved Eval definitions targeting each Artifact, then requests without one.
        let mut targets: HashMap<&str, Vec<&str>> = HashMap::new();
        for eval in saved.eval_definitions() {
            targets
                .entry(eval.target.as_str())
                .or_default()
                .push(eval.id.as_str());
        }
        for view in saved.requests {
            let request = &view.request;
            if !definitions.contains_key(request.eval_id.as_str()) {
                let evals = targets.entry(request.target.as_str()).or_default();
                if !evals.contains(&request.eval_id.as_str()) {
                    evals.push(request.eval_id.as_str());
                }
            }
        }
        let mut dependency = BTreeSet::new();
        let mut evals = BTreeMap::new();
        for &artifact in &order {
            let mut ids: Vec<(bool, &str)> = targets
                .remove(artifact)
                .unwrap_or_default()
                .into_iter()
                .map(|eval| {
                    let kind = definitions
                        .get(eval)
                        .and_then(|eval| eval.declaration.value())
                        .and_then(|declaration| declaration.profile.value())
                        .map(|profile| profile.known.kind())
                        .or_else(|| requests.get(eval).map(|view| view.request.profile.kind()));
                    (kind == Some(ProfileKind::Dependency), eval)
                })
                .collect();
            // Ordinary evals by id, then dependency evals.
            ids.sort();
            dependency.extend(
                ids.iter()
                    .filter(|(derived, _)| *derived)
                    .map(|(_, id)| *id),
            );
            evals.insert(artifact, ids.into_iter().map(|(_, id)| id).collect());
        }
        let components = saved.components();
        let by_id: HashMap<usize, &Component> = components
            .iter()
            .enumerate()
            .map(|(index, component)| (*component.id.value().unwrap_or(&index), component))
            .collect();
        let mut component_of: HashMap<&str, &Component> = HashMap::new();
        let mut peers = BTreeMap::new();
        let mut cyclic = BTreeSet::new();
        for component in components {
            let mut names = members(component);
            names.sort_unstable();
            for &artifact in &names {
                component_of.entry(artifact).or_insert(component);
                peers.insert(
                    artifact,
                    names.iter().copied().filter(|id| *id != artifact).collect(),
                );
                if component.cyclic.value() == Some(&true) {
                    cyclic.insert(artifact);
                }
            }
        }
        let mut upstream = BTreeMap::new();
        for (&artifact, ids) in &evals {
            let component = component_of.get(artifact).copied();
            let own = component.map(members).unwrap_or_else(|| vec![artifact]);
            let gates: Option<Vec<&str>> = component.map(|component| {
                match component.dependencies.value() {
                    Some(dependencies) => dependencies
                        .iter()
                        .filter_map(|id| by_id.get(id))
                        .flat_map(|component| members(component))
                        .collect(),
                    // Saved before components named their dependencies: follow relations.
                    None => saved
                        .definitions()
                        .into_iter()
                        .flat_map(|graph| graph.relations())
                        .filter(|relation| {
                            own.contains(&relation.target.as_str())
                                && !own.contains(&relation.source.as_str())
                        })
                        .flat_map(|relation| {
                            component_of
                                .get(relation.source.as_str())
                                .map(|component| members(component))
                                .unwrap_or_else(|| vec![relation.source.as_str()])
                        })
                        .collect(),
                }
            });
            for &eval in ids {
                let request = requests.get(eval).map(|view| &view.request);
                let deps: Vec<&str> = if dependency.contains(eval) {
                    let definition = definitions
                        .get(eval)
                        .and_then(|definition| definition.deps.value());
                    match (definition, request) {
                        (Some(deps), _) => deps
                            .iter()
                            .map(crate::types::ArtifactName::as_str)
                            .collect(),
                        (None, Some(request)) => request
                            .deps
                            .iter()
                            .map(crate::types::ArtifactName::as_str)
                            .collect(),
                        (None, None) => Vec::new(),
                    }
                } else {
                    match (&gates, request) {
                        (Some(gates), _) => gates.clone(),
                        // Runs saved without definitions: the request's referenced Artifacts.
                        (None, Some(request)) => request
                            .deps
                            .iter()
                            .map(crate::types::ArtifactName::as_str)
                            .collect(),
                        (None, None) => Vec::new(),
                    }
                };
                let mut deps: Vec<&str> = deps
                    .into_iter()
                    .filter(|id| *id != artifact)
                    .filter(|id| dependency.contains(eval) || !own.contains(id))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                deps.sort_by_key(|id| position.get(id).copied().unwrap_or(usize::MAX));
                upstream.insert(eval, deps);
            }
        }
        Self {
            order,
            position,
            requests,
            evals,
            upstream,
            dependency,
            peers,
            cyclic,
        }
    }
}

/// Lazily derived views over one Run; dependency evals may read peers in any order.
pub(super) struct States<'a> {
    run: &'a crate::store::Run,
    #[cfg(test)]
    pub derivations: std::cell::Cell<usize>,
    pub index: Index<'a>,
    pub running: bool,
    ignore_gates: bool,
    views: RefCell<HashMap<&'a str, Option<EvalView>>>,
    effective: RefCell<HashMap<&'a str, Option<EvalStatus>>>,
    completions: RefCell<HashMap<&'a str, Completion>>,
    waits: RefCell<HashMap<&'a str, Vec<crate::types::ArtifactName>>>,
}

impl<'a> States<'a> {
    /// Derive every Artifact once, upstream before downstream, so each lookup afterwards
    /// is memoized and no derivation recurses along a long dependency chain.
    pub fn new(saved: &Saved<'a>) -> Self {
        let run = &saved.run.run;
        let states = Self {
            run,
            #[cfg(test)]
            derivations: std::cell::Cell::new(0),
            index: Index::new(saved),
            running: run.status == RunStatus::Running,
            ignore_gates: run.ignore_gates,
            views: RefCell::new(HashMap::new()),
            effective: RefCell::new(HashMap::new()),
            completions: RefCell::new(HashMap::new()),
            waits: RefCell::new(HashMap::new()),
        };
        for artifact in states.upstream_first() {
            for &eval in states.evals(artifact) {
                states.effective(eval);
            }
            states.completion(artifact);
            states.waits_of(artifact);
        }
        states
    }

    /// Every Artifact after the Artifacts its evals depend on (an iterative depth-first
    /// post-order; a dependency eval naming a cycle peer may come either way).
    fn upstream_first(&self) -> Vec<&'a str> {
        let mut done = HashSet::new();
        let mut seen = HashSet::new();
        let mut ordered = Vec::with_capacity(self.index.order.len());
        for &start in &self.index.order {
            if !seen.insert(start) {
                continue;
            }
            let mut stack = vec![(start, self.dependencies(start), 0)];
            while let Some((artifact, dependencies, next)) = stack.last_mut() {
                if let Some(&dependency) = dependencies.get(*next) {
                    *next += 1;
                    if seen.insert(dependency) {
                        let dependencies = self.dependencies(dependency);
                        stack.push((dependency, dependencies, 0));
                    }
                } else {
                    if done.insert(*artifact) {
                        ordered.push(*artifact);
                    }
                    stack.pop();
                }
            }
        }
        ordered
    }

    fn dependencies(&self, artifact: &str) -> Vec<&'a str> {
        let mut dependencies: Vec<&'a str> = Vec::new();
        for eval in self.evals(artifact) {
            for &upstream in self.upstream(eval) {
                if !dependencies.contains(&upstream) {
                    dependencies.push(upstream);
                }
            }
        }
        dependencies
    }

    /// The Artifact id as borrowed from the index, for memo keys.
    fn key(&self, artifact: &str) -> Option<&'a str> {
        self.index
            .position
            .get_key_value(artifact)
            .map(|(&id, _)| id)
    }

    pub fn request(&self, eval: &str) -> Option<&'a RequestView> {
        self.index.requests.get(eval).copied()
    }

    /// Evidence the graph used for an eval without a request in this Run: the Run's
    /// record of saved results, or for older Runs the validation saved at their end.
    fn outside(&self, eval: &str) -> Option<RequestStatus> {
        if let Some(status) = self.run.evidence.get(eval) {
            return Some(*status);
        }
        let saved = self.run.validation.snapshot()?.evals.value()?;
        let status = *saved
            .iter()
            .find(|saved| saved.id == eval)?
            .status
            .value()?;
        matches!(
            status,
            RequestStatus::Green | RequestStatus::Red | RequestStatus::Error | RequestStatus::Stale
        )
        .then_some(status)
    }

    pub fn evals(&self, artifact: &str) -> &[&'a str] {
        self.index.evals.get(artifact).map_or(&[], Vec::as_slice)
    }

    pub fn upstream(&self, eval: &str) -> &[&'a str] {
        self.index.upstream.get(eval).map_or(&[], Vec::as_slice)
    }

    fn position(&self, artifact: &str) -> usize {
        self.index
            .position
            .get(artifact)
            .copied()
            .unwrap_or(usize::MAX)
    }

    pub fn view(&self, eval: &'a str) -> EvalView {
        if let Some(view) = self.views.borrow().get(eval) {
            // A wait cycle among dependency evals (rejected by graph.rs) reads as not run.
            return view.clone().unwrap_or(EvalView::NotRun(NotRun::Unreviewed));
        }
        self.views.borrow_mut().insert(eval, None);
        #[cfg(test)]
        self.derivations.set(self.derivations.get() + 1);
        let view = self.derive(eval);
        self.views.borrow_mut().insert(eval, Some(view.clone()));
        view
    }

    /// The eval's status as `graph.rs` evaluates it for gates: a GREEN result whose own
    /// gates are unmet still waits or is blocked, and never fulfils a downstream gate.
    pub fn effective(&self, eval: &'a str) -> EvalStatus {
        if let Some(status) = self.effective.borrow().get(eval) {
            // Dependency evals cannot wait on each other (graph.rs rejects it).
            return status.unwrap_or(EvalStatus::Wait);
        }
        self.effective.borrow_mut().insert(eval, None);
        #[cfg(test)]
        self.derivations.set(self.derivations.get() + 1);
        let status = self.evaluate(eval);
        self.effective.borrow_mut().insert(eval, Some(status));
        status
    }

    /// `Graph::evaluate_with_policy` over the saved snapshot: gate readiness first, then
    /// the eval's own evidence.
    fn evaluate(&self, eval: &'a str) -> EvalStatus {
        let request = self.request(eval).map(|view| &view.request);
        let status = request
            .map(|request| request.status)
            .or_else(|| self.outside(eval));
        let derived = self.index.dependency.contains(eval);
        if derived && status == Some(RequestStatus::Error) {
            return EvalStatus::Error;
        }
        let mut unmet = Vec::new();
        if derived || !self.ignore_gates {
            for artifact in self.upstream(eval) {
                for &gate in self.evals(artifact) {
                    let status = self.effective(gate);
                    if status != EvalStatus::Green {
                        unmet.push(status);
                    }
                }
            }
        }
        if unmet
            .iter()
            .any(|status| matches!(status, EvalStatus::Red | EvalStatus::Blocked))
        {
            return EvalStatus::Blocked;
        }
        if !unmet.is_empty() {
            return EvalStatus::Wait;
        }
        if derived {
            return EvalStatus::Green;
        }
        match status {
            Some(RequestStatus::Green) => EvalStatus::Green,
            Some(RequestStatus::Red) => EvalStatus::Red,
            Some(RequestStatus::Error) => EvalStatus::Error,
            Some(RequestStatus::Stale) => EvalStatus::Stale,
            _ => EvalStatus::Unreviewed,
        }
    }

    /// Every eval of the Artifact is effectively GREEN: it fulfils downstream gates.
    fn met(&self, artifact: &str) -> bool {
        self.evals(artifact)
            .iter()
            .all(|eval| self.effective(eval) == EvalStatus::Green)
    }

    fn derive(&self, eval: &'a str) -> EvalView {
        let Some(view) = self.request(eval) else {
            // A saved result the Run used without a request of its own.
            return match self.outside(eval) {
                Some(RequestStatus::Green) => EvalView::Done(Source::Saved),
                Some(RequestStatus::Red) => EvalView::Failed { verdict: true },
                Some(RequestStatus::Error) => EvalView::Failed { verdict: false },
                Some(RequestStatus::Stale) => EvalView::NotRun(NotRun::Stale),
                _ => EvalView::NotRun(NotRun::Absent),
            };
        };
        let request = &view.request;
        let dependency = request.profile.kind() == ProfileKind::Dependency;
        match request.status {
            RequestStatus::Green => EvalView::Done(if dependency {
                Source::Derived
            } else if crate::query::reused(request) {
                Source::Reused
            } else {
                Source::Executed
            }),
            RequestStatus::Red => EvalView::Failed { verdict: true },
            RequestStatus::Error => EvalView::Failed { verdict: false },
            RequestStatus::Running => EvalView::InProgress(Activity::Running),
            RequestStatus::WaitingHuman => EvalView::InProgress(Activity::Human(
                view.claim.as_ref().map(|claim| claim.reviewer.clone()),
            )),
            RequestStatus::BudgetExhausted => EvalView::NotRun(NotRun::Budget),
            RequestStatus::Stale => EvalView::NotRun(NotRun::Stale),
            RequestStatus::Queued
            | RequestStatus::WaitDependency
            | RequestStatus::Unreviewed
            | RequestStatus::Blocked => {
                // Gate bypass changes ordinary readiness; derived verdicts still follow targets.
                if self.ignore_gates && !dependency {
                    return self.ready(view);
                }
                let unmet = self.unmet(self.upstream(eval));
                let blocking: Vec<_> = unmet
                    .iter()
                    .filter(|artifact| self.blocks(artifact))
                    .cloned()
                    .collect();
                if !blocking.is_empty() {
                    return EvalView::BlockedBy(blocking);
                }
                if !unmet.is_empty() {
                    let waits = Waits {
                        root: self.root(&unmet),
                        x: unmet,
                    };
                    return if self.running {
                        EvalView::WaitingOn(waits)
                    } else {
                        EvalView::NotRun(NotRun::Dependency(waits))
                    };
                }
                if dependency {
                    return EvalView::Done(Source::Derived);
                }
                self.ready(view)
            }
        }
    }

    /// Nothing upstream holds the eval.
    fn ready(&self, view: &RequestView) -> EvalView {
        let request = &view.request;
        if !self.running || request.status != RequestStatus::Queued {
            return EvalView::NotRun(NotRun::Unreviewed);
        }
        let slot = request
            .blocked_reason
            .as_deref()
            .and_then(|reason| reason.strip_prefix("Waiting for a free "))
            .and_then(|rest| rest.split_once(" slot"))
            .and_then(|(backend, _)| {
                serde_json::from_value(serde_json::Value::String(backend.into())).ok()
            });
        EvalView::InProgress(Activity::Queued(if let Some(backend) = slot {
            Queue::Slot(backend)
        } else if request.execution_id.is_some() {
            Queue::Joined
        } else {
            Queue::Jobs
        }))
    }

    /// Dependency Artifacts whose gates are not met, most actionable first.
    pub fn unmet(&self, artifacts: &[&'a str]) -> Vec<crate::types::ArtifactName> {
        let mut unmet: Vec<_> = artifacts
            .iter()
            .filter(|artifact| !self.met(artifact))
            .map(|artifact| (*artifact, self.completion(artifact)))
            .collect();
        unmet.sort_by_key(|(artifact, completion)| (completion.rank(), self.position(artifact)));
        unmet
            .into_iter()
            .map(|(artifact, _)| artifact.parse().expect("saved Artifact ID"))
            .collect()
    }

    fn views_of(&self, artifact: &str) -> Vec<EvalView> {
        self.evals(artifact)
            .iter()
            .map(|eval| self.view(eval))
            .collect()
    }

    /// The Artifact's completion from its eval rows; done rows whose own gates are unmet
    /// do not complete it.
    pub fn completion(&self, artifact: &str) -> Completion {
        if let Some(completion) = self.completions.borrow().get(artifact) {
            return *completion;
        }
        let views = self.views_of(artifact);
        let busy = views
            .iter()
            .filter_map(|view| match view {
                EvalView::InProgress(Activity::Running) => Some(Busy::Running),
                EvalView::InProgress(Activity::Human(_)) => Some(Busy::Human),
                EvalView::InProgress(Activity::Queued(_)) => Some(Busy::Queued),
                _ => None,
            })
            .min();
        let completion = if views
            .iter()
            .any(|view| matches!(view, EvalView::Failed { .. }))
        {
            // ERROR before RED, like the Artifact's own row; blocking follows `blocks`.
            Completion::Failed {
                verdict: !views.contains(&EvalView::Failed { verdict: false }),
            }
        } else if let Some(busy) = busy {
            Completion::InProgress(busy)
        } else if views.iter().any(|view| !matches!(view, EvalView::Done(_))) || !self.met(artifact)
        {
            Completion::Waiting {
                blocked: self.blocks(artifact)
                    || views
                        .iter()
                        .any(|view| matches!(view, EvalView::BlockedBy(_))),
            }
        } else {
            Completion::Complete
        };
        if let Some(id) = self.key(artifact) {
            self.completions.borrow_mut().insert(id, completion);
        }
        completion
    }

    /// The graph's Blocked readiness downstream: an effective RED or blocked eval.
    fn blocks(&self, artifact: &str) -> bool {
        self.evals(artifact)
            .iter()
            .any(|eval| matches!(self.effective(eval), EvalStatus::Red | EvalStatus::Blocked))
    }

    /// Done evals whose own gates are unmet: the Artifacts holding them back.
    fn held(&self, artifact: &str, blocked: bool) -> Vec<crate::types::ArtifactName> {
        let mut x: Vec<crate::types::ArtifactName> = Vec::new();
        for &eval in self.evals(artifact) {
            let wanted = if blocked {
                EvalStatus::Blocked
            } else {
                EvalStatus::Wait
            };
            if matches!(self.view(eval), EvalView::Done(_)) && self.effective(eval) == wanted {
                for id in self.unmet(self.upstream(eval)) {
                    if (!blocked || self.blocks(&id)) && !x.contains(&id) {
                        x.push(id);
                    }
                }
            }
        }
        x
    }

    /// The Artifacts this Artifact's evals wait for, as one X list.
    pub fn waits_of(&self, artifact: &str) -> Vec<crate::types::ArtifactName> {
        if let Some(x) = self.waits.borrow().get(artifact) {
            return x.clone();
        }
        let mut x = self.held(artifact, false);
        for view in self.views_of(artifact) {
            if let EvalView::WaitingOn(waits) | EvalView::NotRun(NotRun::Dependency(waits)) = view {
                for id in waits.x {
                    if !x.contains(&id) {
                        x.push(id);
                    }
                }
            }
        }
        x.sort_by_key(|id| (self.completion(id).rank(), self.position(id)));
        if let Some(id) = self.key(artifact) {
            self.waits.borrow_mut().insert(id, x.clone());
        }
        x
    }

    /// The Artifacts this Artifact's blocked evals are blocked by.
    pub fn blockers_of(&self, artifact: &str) -> Vec<crate::types::ArtifactName> {
        let mut x = self.held(artifact, true);
        for view in self.views_of(artifact) {
            if let EvalView::BlockedBy(blockers) = view {
                for id in blockers {
                    if !x.contains(&id) {
                        x.push(id);
                    }
                }
            }
        }
        x
    }

    /// When every X is itself waiting, exactly one level of cause: what the first X waits for.
    fn root(
        &self,
        x: &[crate::types::ArtifactName],
    ) -> Option<(crate::types::ArtifactName, crate::types::ArtifactName)> {
        if x.iter()
            .any(|id| !matches!(self.completion(id), Completion::Waiting { blocked: false }))
        {
            return None;
        }
        let via = x.first()?.clone();
        let cause = self.waits_of(&via).into_iter().next()?;
        Some((via, cause))
    }

    /// Every dependency Artifact of the eval, unmet ones first in X order.
    pub fn upstream_states(&self, eval: &str) -> Vec<Upstream> {
        let mut upstream: Vec<_> = self
            .upstream(eval)
            .iter()
            .map(|artifact| Upstream {
                artifact: (*artifact).parse().expect("saved Artifact ID"),
                completion: self.completion(artifact),
            })
            .collect();
        upstream.sort_by_key(|upstream| {
            (
                upstream.completion.rank(),
                self.position(&upstream.artifact),
            )
        });
        upstream
    }
}
