//! Eval view states: what one tree row shows, derived from the saved definitions and the
//! current request states with the readiness rules of [`crate::graph`].
//!
//! Dependencies target Artifacts. An ordinary eval waits for every Artifact of its own
//! component's direct dependency components (its gates); a dependency eval waits for its
//! `depends_on` Artifacts, which may be cycle peers. Cycle peers never wait for each other
//! otherwise.
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::Saved;
use crate::{
    config::ProfileKind,
    store::RequestView,
    types::{RequestStatus, RunStatus},
};

/// Where a done eval's GREEN came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Executed,
    Reused,
    Derived,
}

/// Why a queued eval has not started although nothing upstream holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Queue {
    /// Waiting for one of the Run's `--jobs`.
    Jobs,
    /// Waiting for a backend slot (limits.json); holds the backend name.
    Slot(String),
    /// Waiting for the running execution of the same reuse key.
    Joined,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activity {
    Running,
    /// The claimant, when someone has claimed it.
    Human(Option<String>),
    Queued(Queue),
}

/// The Artifacts an eval waits for, most actionable first, and one level of root cause when
/// every one of them is itself waiting: `(via, root)` reads "via waits for root".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waits {
    pub x: Vec<String>,
    pub root: Option<(String, String)>,
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
    BlockedBy(Vec<String>),
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
    pub artifact: String,
    pub completion: Completion,
}

/// Per-Run index from saved definitions: display order, evals per Artifact and the
/// Artifacts each eval depends on.
pub(super) struct Index<'a> {
    pub order: Vec<&'a str>,
    pub evals: BTreeMap<&'a str, Vec<&'a str>>,
    /// Eval → direct dependency Artifacts, in display order.
    pub upstream: BTreeMap<&'a str, Vec<&'a str>>,
    pub dependency: BTreeSet<&'a str>,
    /// Artifact → its cycle peers (same component), by name.
    pub peers: BTreeMap<&'a str, Vec<&'a str>>,
    pub cyclic: BTreeSet<&'a str>,
}

impl<'a> Index<'a> {
    pub fn new(saved: &Saved<'a>) -> Self {
        let order = saved.artifact_ids();
        let position = |id: &str| order.iter().position(|artifact| *artifact == id);
        let mut dependency = BTreeSet::new();
        let mut evals = BTreeMap::new();
        for &artifact in &order {
            let mut ids: Vec<(bool, &str)> = saved
                .evals(artifact)
                .into_iter()
                .map(|(eval, definition)| {
                    let kind = definition
                        .and_then(|eval| eval.declaration.value())
                        .and_then(|declaration| declaration.profile.value())
                        .map(|profile| profile.known.kind())
                        .or_else(|| saved.request(eval).map(|view| view.request.profile.kind()));
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
        let members = |component: &'a crate::store::definitions::Component| -> Vec<&'a str> {
            component
                .artifacts
                .value()
                .into_iter()
                .flatten()
                .map(String::as_str)
                .collect()
        };
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
        let mut upstream = BTreeMap::new();
        for (&artifact, ids) in &evals {
            let component = saved.component(artifact);
            let own = component.map(members).unwrap_or_else(|| vec![artifact]);
            let gates: Option<Vec<&str>> = component.map(|component| {
                match component.dependencies.value() {
                    Some(dependencies) => dependencies
                        .iter()
                        .filter_map(|id| {
                            components.iter().enumerate().find(|(index, candidate)| {
                                candidate.id.value().unwrap_or(index) == id
                            })
                        })
                        .flat_map(|(_, component)| members(component))
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
                            saved
                                .component(&relation.source)
                                .map(members)
                                .unwrap_or_else(|| vec![relation.source.as_str()])
                        })
                        .collect(),
                }
            });
            for &eval in ids {
                let request = saved.request(eval).map(|view| &view.request);
                let deps: Vec<&str> = if dependency.contains(eval) {
                    let definition = saved
                        .eval_definitions()
                        .iter()
                        .find(|definition| definition.id == eval)
                        .and_then(|definition| definition.deps.value());
                    match (definition, request) {
                        (Some(deps), _) => deps.iter().map(String::as_str).collect(),
                        (None, Some(request)) => request.deps.iter().map(String::as_str).collect(),
                        (None, None) => Vec::new(),
                    }
                } else {
                    match (&gates, request) {
                        (Some(gates), _) => gates.clone(),
                        // Runs saved without definitions: the request's referenced Artifacts.
                        (None, Some(request)) => request.deps.iter().map(String::as_str).collect(),
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
                deps.sort_by_key(|id| position(id).unwrap_or(usize::MAX));
                upstream.insert(eval, deps);
            }
        }
        Self {
            order,
            evals,
            upstream,
            dependency,
            peers,
            cyclic,
        }
    }
}

/// Lazily derived views over one Run; dependency evals may read peers in any order.
pub(super) struct States<'s, 'a> {
    saved: &'s Saved<'a>,
    pub index: Index<'a>,
    pub running: bool,
    ignore_gates: bool,
    views: RefCell<BTreeMap<&'a str, Option<EvalView>>>,
}

impl<'s, 'a> States<'s, 'a> {
    pub fn new(saved: &'s Saved<'a>) -> Self {
        let run = &saved.run.run;
        Self {
            saved,
            index: Index::new(saved),
            running: run.status == RunStatus::Running,
            ignore_gates: run.ignore_gates,
            views: RefCell::new(BTreeMap::new()),
        }
    }

    pub fn evals(&self, artifact: &str) -> &[&'a str] {
        self.index.evals.get(artifact).map_or(&[], Vec::as_slice)
    }

    pub fn upstream(&self, eval: &str) -> &[&'a str] {
        self.index.upstream.get(eval).map_or(&[], Vec::as_slice)
    }

    fn position(&self, artifact: &str) -> usize {
        let mut order = self.index.order.iter();
        order.position(|id| *id == artifact).unwrap_or(usize::MAX)
    }

    pub fn view(&self, eval: &'a str) -> EvalView {
        if let Some(view) = self.views.borrow().get(eval) {
            // A wait cycle among dependency evals (rejected by graph.rs) reads as not run.
            return view.clone().unwrap_or(EvalView::NotRun(NotRun::Unreviewed));
        }
        self.views.borrow_mut().insert(eval, None);
        let view = self.derive(eval);
        self.views.borrow_mut().insert(eval, Some(view.clone()));
        view
    }

    fn derive(&self, eval: &'a str) -> EvalView {
        let Some(view) = self.saved.request(eval) else {
            return EvalView::NotRun(NotRun::Absent);
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
                if request.status == RequestStatus::Blocked && !unmet.is_empty() {
                    return EvalView::BlockedBy(unmet);
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
            .map(|(backend, _)| backend.to_owned());
        EvalView::InProgress(Activity::Queued(if let Some(backend) = slot {
            Queue::Slot(backend)
        } else if request.execution_id.is_some() {
            Queue::Joined
        } else {
            Queue::Jobs
        }))
    }

    /// Dependency Artifacts that are not complete, most actionable first.
    fn unmet(&self, artifacts: &[&'a str]) -> Vec<String> {
        let mut unmet: Vec<_> = artifacts
            .iter()
            .map(|artifact| (*artifact, self.completion(artifact)))
            .filter(|(_, completion)| *completion != Completion::Complete)
            .collect();
        unmet.sort_by_key(|(artifact, completion)| (completion.rank(), self.position(artifact)));
        unmet
            .into_iter()
            .map(|(artifact, _)| artifact.to_owned())
            .collect()
    }

    fn views_of(&self, artifact: &str) -> Vec<EvalView> {
        self.evals(artifact)
            .iter()
            .map(|eval| self.view(eval))
            .collect()
    }

    pub fn completion(&self, artifact: &str) -> Completion {
        let views = self.views_of(artifact);
        if views
            .iter()
            .any(|view| matches!(view, EvalView::Failed { .. }))
        {
            return Completion::Failed {
                verdict: views.contains(&EvalView::Failed { verdict: true }),
            };
        }
        let busy = views
            .iter()
            .filter_map(|view| match view {
                EvalView::InProgress(Activity::Running) => Some(Busy::Running),
                EvalView::InProgress(Activity::Human(_)) => Some(Busy::Human),
                EvalView::InProgress(Activity::Queued(_)) => Some(Busy::Queued),
                _ => None,
            })
            .min();
        if let Some(busy) = busy {
            return Completion::InProgress(busy);
        }
        if views.iter().any(|view| !matches!(view, EvalView::Done(_))) {
            return Completion::Waiting {
                blocked: views
                    .iter()
                    .any(|view| matches!(view, EvalView::BlockedBy(_))),
            };
        }
        Completion::Complete
    }

    /// The graph's Blocked readiness: a RED verdict or a blocked eval among the gates.
    fn blocks(&self, artifact: &str) -> bool {
        self.views_of(artifact).iter().any(|view| {
            matches!(
                view,
                EvalView::Failed { verdict: true } | EvalView::BlockedBy(_)
            )
        })
    }

    /// The Artifacts this Artifact's evals wait for, as one X list.
    pub fn waits_of(&self, artifact: &str) -> Vec<String> {
        let mut x: Vec<String> = Vec::new();
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
        x
    }

    /// The Artifacts this Artifact's blocked evals are blocked by.
    pub fn blockers_of(&self, artifact: &str) -> Vec<String> {
        let mut x: Vec<String> = Vec::new();
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

    /// When every X is itself waiting, follow the first X to the first Artifact that is not.
    fn root(&self, x: &[String]) -> Option<(String, String)> {
        if x.iter()
            .any(|id| !matches!(self.completion(id), Completion::Waiting { blocked: false }))
        {
            return None;
        }
        let mut via = x.first()?.clone();
        let mut visited = BTreeSet::from([via.clone()]);
        loop {
            let next = self.waits_of(&via).into_iter().next()?;
            match self.completion(&next) {
                Completion::Waiting { .. } if visited.insert(next.clone()) => via = next,
                Completion::Waiting { .. } => return None,
                _ => return Some((via, next)),
            }
        }
    }

    /// Every dependency Artifact of the eval, unmet ones first in X order.
    pub fn upstream_states(&self, eval: &str) -> Vec<Upstream> {
        let mut upstream: Vec<_> = self
            .upstream(eval)
            .iter()
            .map(|artifact| Upstream {
                artifact: (*artifact).to_owned(),
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

    /// The row's state differs from what the Run saw when it ended.
    pub fn changed(&self, eval: &'a str) -> bool {
        let Some(end) = self.saved.run.run.completed_at.as_deref() else {
            return false;
        };
        let Some(view) = self.saved.request(eval) else {
            return false;
        };
        let request = &view.request;
        let parse = |time: &str| OffsetDateTime::parse(time, &Rfc3339).ok();
        if let (Some(end), Some(completed)) =
            (parse(end), request.completed_at.as_deref().and_then(parse))
            && completed > end
        {
            return true;
        }
        match (request.status, self.view(eval)) {
            (RequestStatus::WaitDependency, EvalView::NotRun(NotRun::Dependency(_)))
            | (RequestStatus::Blocked, EvalView::BlockedBy(_)) => false,
            (RequestStatus::WaitDependency | RequestStatus::Blocked, _) => true,
            _ => false,
        }
    }
}
