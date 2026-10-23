//! Eval view states: what one tree row shows, derived from the saved definitions and the
//! current request states with the readiness rules of [`crate::graph`].
//!
//! Each Eval has its own references and gates, and its target's children count as references;
//! its own Artifact and that Artifact's cycle peers never gate it.
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
};

use super::Saved;
use crate::{
    graph::EvalStatus,
    store::RequestView,
    types::{ArtifactName, EvalId, RequestStatus},
};

/// Where a done eval's GREEN came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Executed,
    Provisional,
    Reused,
    /// A saved result the Run used for an eval it has no request for.
    Saved,
}

/// Why a queued eval has not started although nothing upstream holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Queue {
    /// Ready, waiting for scheduler admission without a Run cap.
    Ready,
    /// Waiting for the Run's explicitly chosen `--jobs` cap.
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
    pub artifacts: Vec<crate::types::ArtifactName>,
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

/// Roll-up urgency, ordered from failed to done for eval rows and Artifact completions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Failed,
    InProgress,
    Blocked,
    Waiting,
    NotRun,
    Done,
}

impl EvalView {
    pub fn rank(&self) -> Rank {
        match self {
            Self::Failed { .. } => Rank::Failed,
            Self::InProgress(_) => Rank::InProgress,
            Self::BlockedBy(_) => Rank::Blocked,
            Self::WaitingOn(_) => Rank::Waiting,
            Self::NotRun(_) => Rank::NotRun,
            Self::Done(_) => Rank::Done,
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
    /// Order of X: failed, in progress, waiting, complete; blocked completions still wait.
    fn rank(self) -> Rank {
        match self {
            Self::Failed { .. } => Rank::Failed,
            Self::InProgress(_) => Rank::InProgress,
            Self::Waiting { .. } => Rank::Waiting,
            Self::Complete => Rank::Done,
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
    pub order: Vec<&'a ArtifactName>,
    position: HashMap<&'a ArtifactName, usize>,
    requests: HashMap<&'a EvalId, &'a RequestView>,
    pub evals: BTreeMap<&'a ArtifactName, Vec<&'a EvalId>>,
    /// Eval → direct dependency Artifacts, in display order.
    pub upstream: BTreeMap<&'a EvalId, Vec<&'a ArtifactName>>,
    gates: BTreeMap<&'a EvalId, Vec<&'a EvalId>>,
    execution_order: Vec<&'a EvalId>,
    targets: BTreeMap<&'a EvalId, &'a ArtifactName>,
    /// Artifact → its cycle peers (same component), by name.
    pub peers: BTreeMap<&'a ArtifactName, Vec<&'a ArtifactName>>,
    pub cyclic: BTreeSet<&'a ArtifactName>,
}

type Component = crate::store::definitions::Component;

fn members(component: &Component) -> Vec<&ArtifactName> {
    component.artifacts.value().into_iter().flatten().collect()
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
            requests.entry(&view.request.eval_id).or_insert(view);
        }
        let definitions: HashMap<_, _> = saved
            .eval_definitions()
            .iter()
            .map(|eval| (&eval.id, eval))
            .collect();
        // Saved Eval definitions targeting each Artifact, then requests without one.
        let mut targets: HashMap<&ArtifactName, Vec<&EvalId>> = HashMap::new();
        for eval in saved.eval_definitions() {
            targets.entry(&eval.target).or_default().push(&eval.id);
        }
        for view in saved.requests {
            let request = &view.request;
            if !definitions.contains_key(&request.eval_id) {
                let evals = targets.entry(&request.target).or_default();
                if !evals.contains(&&request.eval_id) {
                    evals.push(&request.eval_id);
                }
            }
        }
        let evals: BTreeMap<_, _> = order
            .iter()
            .map(|&artifact| {
                let mut ids = targets.remove(artifact).unwrap_or_default();
                ids.sort_unstable();
                (artifact, ids)
            })
            .collect();
        let components = saved.components();
        let mut peers = BTreeMap::new();
        let mut cyclic = BTreeSet::new();
        for component in components {
            let mut names = members(component);
            names.sort_unstable();
            for &artifact in &names {
                peers.insert(
                    artifact,
                    names.iter().copied().filter(|id| *id != artifact).collect(),
                );
                if component.cyclic.value() == Some(&true) {
                    cyclic.insert(artifact);
                }
            }
        }
        let targets: BTreeMap<_, _> = evals
            .iter()
            .flat_map(|(&target, ids)| ids.iter().map(move |&id| (id, target)))
            .collect();
        let children: BTreeMap<_, Vec<_>> = saved
            .artifacts()
            .map(|(parent, artifact)| {
                let children = artifact.children.value().into_iter().flatten();
                (parent, children.map(|(_, child)| child).collect())
            })
            .collect();
        // An eval depends on what it references and, like every eval of its target, on the
        // target's children.
        let inputs: BTreeMap<_, Vec<_>> = targets
            .iter()
            .map(|(&eval, &target)| {
                let mut deps: Vec<_> = definitions
                    .get(eval)
                    .and_then(|definition| definition.deps.value())
                    .map(|deps| deps.iter().collect())
                    .unwrap_or_else(|| {
                        requests
                            .get(eval)
                            .map_or(Vec::new(), |view| view.request.deps.iter().collect())
                    });
                deps.extend(children.get(target).into_iter().flatten().copied());
                (eval, deps)
            })
            .collect();
        let human = |eval: &EvalId| match requests.get(eval) {
            Some(view) => view.request.profile.kind() == crate::config::ProfileKind::Human,
            None => definitions
                .get(eval)
                .and_then(|definition| definition.declaration.value())
                .and_then(|declaration| declaration.profile.value())
                .is_some_and(|profile| {
                    matches!(profile.known, crate::config::StoredProfile::Human { .. })
                }),
        };
        let schedule = crate::graph::schedule::Schedule::new(
            targets
                .iter()
                .map(|(&eval, &target)| crate::graph::schedule::Input {
                    id: eval,
                    target,
                    deps: inputs[eval].clone(),
                    human: human(eval),
                }),
            children
                .iter()
                .flat_map(|(&parent, children)| children.iter().map(move |&child| (parent, child))),
        );
        let upstream = inputs
            .into_iter()
            .map(|(eval, mut deps)| {
                deps.extend(schedule.gates[eval].iter().map(|gate| targets[gate]));
                let mut deps: Vec<_> = deps
                    .into_iter()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                deps.sort_by_key(|id| position.get(id).copied().unwrap_or(usize::MAX));
                (eval, deps)
            })
            .collect();
        let execution_order = schedule.order;
        Self {
            order,
            position,
            requests,
            evals,
            upstream,
            gates: schedule.gates,
            execution_order,
            targets,
            peers,
            cyclic,
        }
    }
}

/// Lazily derived views over one Run, using the same Eval gates as live scheduling.
pub(super) struct States<'a> {
    run: &'a crate::store::Run,
    #[cfg(any(test, feature = "test-support"))]
    pub derivations: std::cell::Cell<usize>,
    pub index: Index<'a>,
    pub running: bool,
    views: RefCell<HashMap<&'a EvalId, Option<EvalView>>>,
    effective: RefCell<HashMap<&'a EvalId, Option<EvalStatus>>>,
    completions: RefCell<HashMap<&'a ArtifactName, Completion>>,
    waits: RefCell<HashMap<&'a ArtifactName, Vec<ArtifactName>>>,
    held: HashMap<&'a EvalId, bool>,
}

impl<'a> States<'a> {
    /// Derive every Artifact once, upstream before downstream, so each lookup afterwards
    /// is memoized and no derivation recurses along a long dependency chain.
    pub fn new(saved: &Saved<'a>) -> Self {
        let run = &saved.run.run;
        let mut states = Self {
            run,
            #[cfg(any(test, feature = "test-support"))]
            derivations: std::cell::Cell::new(0),
            index: Index::new(saved),
            running: run.status().is_open(),
            views: RefCell::new(HashMap::new()),
            effective: RefCell::new(HashMap::new()),
            completions: RefCell::new(HashMap::new()),
            waits: RefCell::new(HashMap::new()),
            held: HashMap::new(),
        };
        for &eval in &states.index.execution_order {
            states.effective(eval);
            let held = states.index.gates[eval].iter().any(|&gate| {
                matches!(
                    states.effective(gate),
                    EvalStatus::Red | EvalStatus::Blocked | EvalStatus::Error
                ) || states.held[gate]
            });
            states.held.insert(eval, held);
        }
        for artifact in states.upstream_first() {
            states.completion(artifact);
            states.waits_of(artifact);
        }
        states
    }

    /// Every Artifact after the Artifacts its evals depend on (an iterative depth-first
    /// post-order; projected Artifact cycles may come either way).
    fn upstream_first(&self) -> Vec<&'a ArtifactName> {
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

    fn dependencies(&self, artifact: &ArtifactName) -> Vec<&'a ArtifactName> {
        let mut dependencies: Vec<&'a ArtifactName> = Vec::new();
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
    fn key(&self, artifact: &ArtifactName) -> Option<&'a ArtifactName> {
        self.index
            .position
            .get_key_value(artifact)
            .map(|(&id, _)| id)
    }

    pub fn request(&self, eval: &EvalId) -> Option<&'a RequestView> {
        self.index.requests.get(eval).copied()
    }

    /// Evidence the graph used for an eval without a request in this Run: the Run's
    /// record of saved results, or for older Runs the validation saved at their end.
    fn outside(&self, eval: &EvalId) -> Option<RequestStatus> {
        if let Some(status) = self.run.evidence.get(eval) {
            return Some(*status);
        }
        let saved = self.run.validation.snapshot()?.evals.value()?;
        let status = *saved
            .iter()
            .find(|saved| &saved.id == eval)?
            .status
            .value()?;
        matches!(
            status,
            RequestStatus::Green | RequestStatus::Red | RequestStatus::Error | RequestStatus::Stale
        )
        .then_some(status)
    }

    pub fn evals(&self, artifact: &ArtifactName) -> &[&'a EvalId] {
        self.index.evals.get(artifact).map_or(&[], Vec::as_slice)
    }

    pub fn upstream(&self, eval: &EvalId) -> &[&'a ArtifactName] {
        self.index.upstream.get(eval).map_or(&[], Vec::as_slice)
    }

    fn position(&self, artifact: &ArtifactName) -> usize {
        self.index
            .position
            .get(artifact)
            .copied()
            .unwrap_or(usize::MAX)
    }

    pub fn view(&self, eval: &'a EvalId) -> EvalView {
        if let Some(view) = self.views.borrow().get(eval) {
            // Artifact-level presentation can revisit an Eval through a projected cycle.
            return view.clone().unwrap_or(EvalView::NotRun(NotRun::Unreviewed));
        }
        self.views.borrow_mut().insert(eval, None);
        #[cfg(any(test, feature = "test-support"))]
        self.derivations.set(self.derivations.get() + 1);
        let view = self.derive(eval);
        self.views.borrow_mut().insert(eval, Some(view.clone()));
        view
    }

    /// The eval's status as `graph.rs` evaluates it for gates: a GREEN result whose own
    /// gates are unmet still waits or is blocked, and never fulfils a downstream gate.
    pub fn effective(&self, eval: &'a EvalId) -> EvalStatus {
        if let Some(status) = self.effective.borrow().get(eval) {
            // Eval gates are acyclic; a revisit cannot supply a verdict.
            return status.unwrap_or(EvalStatus::Wait);
        }
        self.effective.borrow_mut().insert(eval, None);
        #[cfg(any(test, feature = "test-support"))]
        self.derivations.set(self.derivations.get() + 1);
        let status = self.evaluate(eval);
        self.effective.borrow_mut().insert(eval, Some(status));
        status
    }

    /// `Graph::evaluate` over the saved snapshot: gate readiness first, then
    /// the eval's own evidence.
    fn evaluate(&self, eval: &'a EvalId) -> EvalStatus {
        let request = self.request(eval).map(|view| &view.request);
        let status = request
            .map(|request| request.status())
            .or_else(|| self.outside(eval));
        let unmet: Vec<_> = self
            .index
            .gates
            .get(eval)
            .into_iter()
            .flatten()
            .map(|&gate| (gate, self.effective(gate)))
            .filter(|(_, status)| !status.passes_gate())
            .collect();
        if unmet
            .iter()
            .any(|&(gate, status)| self.status_blocks(eval, gate, status))
        {
            return EvalStatus::Blocked;
        }
        if !unmet.is_empty() {
            return EvalStatus::Wait;
        }
        match status {
            Some(RequestStatus::Deferred) => EvalStatus::Deferred,
            // A provisional result stands once nothing it relies on is pending any more.
            Some(RequestStatus::Provisional) if self.pending(eval).is_empty() => EvalStatus::Green,
            Some(RequestStatus::Provisional) => EvalStatus::Provisional,
            Some(RequestStatus::Green) => EvalStatus::Green,
            Some(RequestStatus::Red) => EvalStatus::Red,
            Some(RequestStatus::Error) => EvalStatus::Error,
            Some(RequestStatus::Stale) => EvalStatus::Stale,
            _ => EvalStatus::Unreviewed,
        }
    }

    fn met(&self, artifact: &ArtifactName) -> bool {
        self.evals(artifact)
            .iter()
            .all(|eval| self.effective(eval) == EvalStatus::Green)
    }

    fn derive(&self, eval: &'a EvalId) -> EvalView {
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
        match request.status() {
            RequestStatus::Provisional if self.effective(eval) == EvalStatus::Provisional => {
                EvalView::Done(Source::Provisional)
            }
            RequestStatus::Provisional | RequestStatus::Green => {
                EvalView::Done(if crate::query::reused(request) {
                    Source::Reused
                } else {
                    Source::Executed
                })
            }
            RequestStatus::Red => EvalView::Failed { verdict: true },
            RequestStatus::Error => EvalView::Failed { verdict: false },
            RequestStatus::Running => EvalView::InProgress(Activity::Running),
            RequestStatus::WaitingHuman | RequestStatus::Deferred => EvalView::InProgress(
                Activity::Human(view.claim.as_ref().map(|claim| claim.reviewer.clone())),
            ),
            RequestStatus::BudgetExhausted => EvalView::NotRun(NotRun::Budget),
            RequestStatus::Stale => EvalView::NotRun(NotRun::Stale),
            RequestStatus::Queued
            | RequestStatus::WaitDependency
            | RequestStatus::Unreviewed
            | RequestStatus::Blocked => {
                let unmet = self.unmet(eval);
                let blocking: Vec<_> = unmet
                    .iter()
                    .filter(|artifact| self.gate_blocks(eval, artifact))
                    .cloned()
                    .collect();
                if !blocking.is_empty() {
                    return EvalView::BlockedBy(blocking);
                }
                if self.run.modes.speculate {
                    return self.ready(view);
                }
                if !unmet.is_empty() {
                    let waits = Waits {
                        root: self.root(&unmet),
                        artifacts: unmet,
                    };
                    return if self.running {
                        EvalView::WaitingOn(waits)
                    } else {
                        EvalView::NotRun(NotRun::Dependency(waits))
                    };
                }
                self.ready(view)
            }
        }
    }

    /// Nothing upstream holds the eval.
    fn ready(&self, view: &RequestView) -> EvalView {
        let request = &view.request;
        if !self.running || request.status() != RequestStatus::Queued {
            return EvalView::NotRun(NotRun::Unreviewed);
        }
        EvalView::InProgress(Activity::Queued(
            if let Some(crate::store::QueueReason::Slot { backend }) = request.queue {
                Queue::Slot(backend)
            } else if request.execution_id.is_some() {
                Queue::Joined
            } else if self.run.jobs.is_some() {
                Queue::Jobs
            } else {
                Queue::Ready
            },
        ))
    }

    /// The Artifacts of this Eval's gates that pass only provisionally: a sign-off not yet
    /// submitted, or a result that relies on one.
    fn pending(&self, eval: &EvalId) -> Vec<ArtifactName> {
        let mut artifacts: Vec<_> = self
            .index
            .gates
            .get(eval)
            .into_iter()
            .flatten()
            .filter(|&&gate| {
                matches!(
                    self.effective(gate),
                    EvalStatus::Deferred | EvalStatus::Provisional
                )
            })
            .map(|gate| self.index.targets[gate])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        artifacts
            .sort_by_key(|artifact| (self.completion(artifact).rank(), self.position(artifact)));
        artifacts.into_iter().cloned().collect()
    }

    /// Only this Eval's gates contribute waits; its own Artifact and cycle peers never do.
    fn unmet(&self, eval: &EvalId) -> Vec<ArtifactName> {
        let mut artifacts: Vec<_> = self
            .index
            .gates
            .get(eval)
            .into_iter()
            .flatten()
            .filter(|&&gate| !self.effective(gate).passes_gate())
            .map(|gate| self.index.targets[gate])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        artifacts
            .sort_by_key(|artifact| (self.completion(artifact).rank(), self.position(artifact)));
        artifacts.into_iter().cloned().collect()
    }

    fn views_of(&self, artifact: &ArtifactName) -> Vec<EvalView> {
        self.evals(artifact)
            .iter()
            .map(|eval| self.view(eval))
            .collect()
    }

    /// The Artifact's completion from its eval rows; done rows whose own gates are unmet
    /// do not complete it.
    pub fn completion(&self, artifact: &ArtifactName) -> Completion {
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

    /// Group only this Eval's actual gates, never unrelated Evals on the same material.
    fn gate_blocks(&self, eval: &EvalId, artifact: &ArtifactName) -> bool {
        self.index
            .gates
            .get(eval)
            .into_iter()
            .flatten()
            .filter(|gate| self.index.targets[*gate] == artifact)
            .any(|&gate| self.status_blocks(eval, gate, self.effective(gate)))
    }

    /// A RED or blocked gate blocks; an ERROR blocks under `--speculate`, and always when the
    /// gate is a machine eval of a Human eval's own Artifact.
    fn status_blocks(&self, eval: &EvalId, gate: &EvalId, status: EvalStatus) -> bool {
        match status {
            EvalStatus::Red | EvalStatus::Blocked => true,
            EvalStatus::Error => {
                self.run.modes.speculate
                    || self.index.targets.get(gate) == self.index.targets.get(eval)
            }
            _ => false,
        }
    }

    /// Graph lights treat an external ERROR as held too, without treating cycle peers
    /// or unrelated sibling Evals as gates.
    pub fn held_for_run(&self, eval: &EvalId) -> bool {
        self.held.get(eval).copied().unwrap_or(false)
    }

    /// The graph's Blocked readiness downstream: an effective RED or blocked eval.
    fn blocks(&self, artifact: &ArtifactName) -> bool {
        self.evals(artifact).iter().any(|eval| {
            matches!(self.effective(eval), EvalStatus::Red | EvalStatus::Blocked)
                || (self.run.modes.speculate && self.effective(eval) == EvalStatus::Error)
        })
    }

    /// Done evals whose own gates are unmet: the Artifacts holding them back.
    fn held(&self, artifact: &ArtifactName, blocked: bool) -> Vec<crate::types::ArtifactName> {
        let mut x: Vec<crate::types::ArtifactName> = Vec::new();
        for &eval in self.evals(artifact) {
            let wanted = if blocked {
                EvalStatus::Blocked
            } else {
                EvalStatus::Wait
            };
            if matches!(self.view(eval), EvalView::Done(_)) && self.effective(eval) == wanted {
                for id in self.unmet(eval) {
                    if (!blocked || self.gate_blocks(eval, &id)) && !x.contains(&id) {
                        x.push(id);
                    }
                }
            }
            // A provisional result waits for the sign-offs it relies on.
            if !blocked && self.effective(eval) == EvalStatus::Provisional {
                for id in self.pending(eval) {
                    if !x.contains(&id) {
                        x.push(id);
                    }
                }
            }
        }
        x
    }

    /// The Artifacts this Artifact's evals wait for, as one X list.
    pub fn waits_of(&self, artifact: &ArtifactName) -> Vec<crate::types::ArtifactName> {
        if let Some(x) = self.waits.borrow().get(artifact) {
            return x.clone();
        }
        let mut x = self.held(artifact, false);
        for view in self.views_of(artifact) {
            if let EvalView::WaitingOn(waits) | EvalView::NotRun(NotRun::Dependency(waits)) = view {
                for id in waits.artifacts {
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
    pub fn blockers_of(&self, artifact: &ArtifactName) -> Vec<crate::types::ArtifactName> {
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
    pub fn upstream_states(&self, eval: &EvalId) -> Vec<Upstream> {
        let mut upstream: Vec<_> = self
            .upstream(eval)
            .iter()
            .map(|artifact| Upstream {
                artifact: (*artifact).clone(),
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
