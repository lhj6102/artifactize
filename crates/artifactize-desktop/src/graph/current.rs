//! A branch's Current: its worktree as it is now, scanned by the library's
//! [`live::scan`] as `artifactize status` does. A scan runs on its own: each step it publishes
//! is kept for the branch's graph and Details and announced to the window, and a request
//! while the branch's scan still runs shows that scan instead of starting over. A refresh
//! asked meanwhile is for files as they are now, so a new scan follows once that one ends.
use super::{GraphEdge, GraphNode, Shape, Source, node_id};
use crate::{
    App, Failure,
    browse::{BranchKey, NodeKind, Segment, TreeNode},
    detail::{
        CardView, DetailTarget, DetailView, Field, PlanLine, Profile, SectionName, SectionView,
    },
    status::{Tone, Weight},
};
use artifactize_core::monitor::{
    NodeId,
    live::{self, Current, CurrentArtifact, CurrentEval, Place},
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tauri_specta::Event;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

/// A branch's newest scan step, its number among this app's steps, and the shape of its graph.
#[derive(Clone)]
pub struct Shown {
    pub current: Arc<Current>,
    pub revision: crate::ids::ScanId,
    /// Stays while the scans find the same Artifacts and edges, whatever their plans.
    pub shape: crate::ids::ShapeKey,
}

/// How far a scan is: started, published at least once, or ended (`Err` holds why it failed).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stage {
    Started,
    Published,
    Ended(Result<(), String>),
}

/// A scan running for a branch: its number, how to stop it, how far it is, and whether a
/// refresh asked while it ran wants a new scan after it.
struct Running {
    scan: crate::ids::ScanId,
    token: CancellationToken,
    stage: watch::Receiver<Stage>,
    again: bool,
}

/// A scan to run: its number, how to stop it, and where it tells how far it is.
struct Fresh {
    scan: crate::ids::ScanId,
    token: CancellationToken,
    stage: watch::Sender<Stage>,
}

/// What a scan request does: show the branch's scan still running, or run a new one.
enum Request {
    Join(watch::Receiver<Stage>),
    Run(Fresh),
}

/// The shown Current of `branch` changed: a scan published a step, or ended.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct CurrentChanged {
    pub branch: BranchKey,
}

/// The last scan step of each branch, the scans running, and why the last ones failed.
pub struct Currents {
    shown: Mutex<BTreeMap<Place, Shown>>,
    running: Mutex<BTreeMap<Place, Running>>,
    failed: Mutex<BTreeMap<Place, String>>,
    /// Numbers scans and their steps alike: a later one has a higher number.
    scans: std::sync::atomic::AtomicU32,
    /// The last graph shape handed out.
    shapes: Mutex<crate::ids::ShapeKey>,
    changed: mpsc::UnboundedSender<BranchKey>,
    /// The receiving end of `changed`, until the app starts forwarding it.
    changes: Mutex<Option<mpsc::UnboundedReceiver<BranchKey>>>,
}

impl Default for Currents {
    fn default() -> Self {
        let (changed, changes) = mpsc::unbounded_channel();
        Self {
            shown: Mutex::default(),
            running: Mutex::default(),
            failed: Mutex::default(),
            scans: Default::default(),
            shapes: Mutex::default(),
            changed,
            changes: Mutex::new(Some(changes)),
        }
    }
}

impl Currents {
    pub fn shown(&self, place: &Place) -> Result<Shown, Failure> {
        self.shown
            .lock()
            .expect("currents lock")
            .get(place)
            .cloned()
            .ok_or_else(|| {
                Failure::Missing("This Current is not scanned yet; choose it again.".into())
            })
    }

    /// The changed branches, for the one forwarder of this app.
    pub(crate) fn take_changes(&self) -> Option<mpsc::UnboundedReceiver<BranchKey>> {
        self.changes.lock().expect("changes lock").take()
    }

    /// Show the scan of `place` still running, or start one. Starting over would throw away
    /// every fingerprint the running scan has computed, and a scan reads the review store
    /// afresh once its fingerprints are in, so the one running also sees a Run that ended
    /// meanwhile. A `refresh` is for the files as they are now, which the running scan may
    /// have read before they changed: a new scan follows it.
    fn request(&self, place: &Place, refresh: bool) -> Request {
        let mut running = self.running.lock().expect("scans lock");
        // A scan whose task is gone without ending (its sender dropped) is not waited for.
        if let Some(scan) = running.get_mut(place)
            && scan.stage.has_changed().is_ok()
        {
            scan.again |= refresh;
            return Request::Join(scan.stage.clone());
        }
        Request::Run(self.start(&mut running, place))
    }

    /// Note a new scan of `place` as running.
    fn start(&self, running: &mut BTreeMap<Place, Running>, place: &Place) -> Fresh {
        let scan = crate::ids::ScanId::allocate(&self.scans);
        let token = CancellationToken::new();
        let (stage, receiver) = watch::channel(Stage::Started);
        running.insert(
            place.clone(),
            Running {
                scan,
                token: token.clone(),
                stage: receiver,
                again: false,
            },
        );
        self.failed.lock().expect("failures lock").remove(place);
        Fresh { scan, token, stage }
    }

    /// Keep a step of scan `scan` of `place`, unless the state was replaced since.
    fn publish(&self, place: &Place, scan: crate::ids::ScanId, current: Current) -> bool {
        let running = self.running.lock().expect("scans lock");
        if running
            .get(place)
            .is_none_or(|running| running.scan != scan)
        {
            return false;
        }
        let mut shown = self.shown.lock().expect("currents lock");
        let shape = match shown.get(place) {
            Some(before) if same_shape(&before.current, &current) => before.shape,
            _ => self.shapes.lock().expect("shapes lock").next(),
        };
        shown.insert(
            place.clone(),
            Shown {
                current: Arc::new(current),
                revision: crate::ids::ScanId::allocate(&self.scans),
                shape,
            },
        );
        true
    }

    /// Scan `scan` of `place` ended, failing with `failure`, and the scan a refresh asked for
    /// meanwhile, if any, starts; `None` for a scan of a replaced state, of which nothing is
    /// kept.
    fn end(
        &self,
        place: &Place,
        scan: crate::ids::ScanId,
        failure: Option<String>,
    ) -> Option<Option<Fresh>> {
        let mut running = self.running.lock().expect("scans lock");
        let again = match running.get(place) {
            Some(running) if running.scan == scan => running.again,
            _ => return None,
        };
        running.remove(place);
        if again {
            return Some(Some(self.start(&mut running, place)));
        }
        if let Some(failure) = failure {
            self.failed
                .lock()
                .expect("failures lock")
                .insert(place.clone(), failure);
        }
        Some(None)
    }

    /// Tell the window that `branch`'s Current changed.
    fn announce(&self, branch: &BranchKey) {
        // Nobody forwards without a window; the next read shows the change anyway.
        let _ = self.changed.send(branch.clone());
    }

    /// Cancel every running scan at once; the returned future ends when each has stopped,
    /// that is when its task let go of its stage.
    pub(crate) fn stop(&self) -> impl std::future::Future<Output = ()> + use<> {
        let stopping: Vec<_> = std::mem::take(&mut *self.running.lock().expect("scans lock"))
            .into_values()
            .map(|running| {
                running.token.cancel();
                running.stage
            })
            .collect();
        async move {
            for mut stage in stopping {
                while stage.changed().await.is_ok() {}
            }
        }
    }

    /// Forget every scan of a replaced state and cancel those running.
    pub fn reset(&self) {
        drop(self.stop());
        self.shown.lock().expect("currents lock").clear();
        self.failed.lock().expect("failures lock").clear();
    }
}

/// Forward the changed Currents to every window until the app exits, each branch once per
/// batch.
pub fn forward<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    mut changes: mpsc::UnboundedReceiver<BranchKey>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(first) = changes.recv().await {
            let mut batch = vec![first];
            while let Ok(branch) = changes.try_recv() {
                if !batch.contains(&branch) {
                    batch.push(branch);
                }
            }
            for branch in batch {
                // The app is closing; nobody listens any more.
                if (CurrentChanged { branch }).emit(&app).is_err() {
                    return;
                }
            }
        }
    });
}

/// A branch's Current as the Main area shows it; its structure text is read on its own.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CurrentView {
    pub branch: BranchKey,
    /// When this step was scanned, at milliseconds since the Unix epoch.
    pub scanned_at: f64,
    pub fingerprint: crate::ids::FingerprintKey,
    /// Grows with each step: the graph and an open Detail read again when it does.
    pub revision: crate::ids::ScanId,
    /// The branch's scan is still running.
    pub scanning: bool,
    /// How many Artifacts are still being checked, of how many.
    pub checking: u32,
    pub artifacts: u32,
    /// Why the branch's last scan failed; what it published before stays shown.
    pub failure: Option<String>,
}

fn segment(text: String, tone: Tone) -> Vec<Segment> {
    vec![Segment {
        text,
        tone: Some(tone),
    }]
}

fn eval_row(eval: &CurrentEval) -> TreeNode {
    let tone: Tone = eval.plan.tone().into();
    TreeNode {
        id: NodeId::eval(eval.id.clone()).into(),
        kind: NodeKind::Eval,
        glyph: eval.plan.glyph().into(),
        tone,
        weight: Weight::Normal,
        name: eval.name.clone(),
        marks: String::new(),
        text: segment(eval.plan.word().into(), tone),
        right: eval.title.clone(),
        changed: false,
        done: false,
        attention: false,
        urgent: false,
        upstream: Vec::new(),
        children: Vec::new(),
    }
}

fn artifact_row(artifact: &CurrentArtifact) -> TreeNode {
    let tone: Tone = artifact.plan.tone().into();
    TreeNode {
        id: node_id(&artifact.name),
        kind: NodeKind::Artifact,
        glyph: artifact.plan.glyph().into(),
        tone,
        weight: Weight::Normal,
        name: artifact.name.to_string(),
        marks: String::new(),
        text: segment(artifact.text(), tone),
        right: String::new(),
        changed: false,
        done: false,
        attention: false,
        urgent: false,
        upstream: Vec::new(),
        children: artifact.evals.iter().map(eval_row).collect(),
    }
}

/// The row of the Artifact `node` of `current`, with its evals.
pub(crate) fn row(current: &Current, node: &NodeId) -> Option<TreeNode> {
    current
        .artifacts
        .iter()
        .find(|artifact| node_id(&artifact.name).inner() == node)
        .map(artifact_row)
}

/// The newest step of `branch`'s Current, and how its scan is doing.
fn view(currents: &Currents, branch: BranchKey) -> Result<CurrentView, Failure> {
    let place = branch.to_place();
    let shown = currents.shown(&place)?;
    let current = &shown.current;
    let count = |artifacts: usize| u32::try_from(artifacts).unwrap_or(u32::MAX);
    Ok(CurrentView {
        branch,
        scanned_at: (current.scanned_at.unix_timestamp_nanos() / 1_000_000) as f64,
        fingerprint: current.fingerprint.clone().into(),
        revision: shown.revision,
        scanning: currents
            .running
            .lock()
            .expect("scans lock")
            .contains_key(&place),
        checking: count(
            current
                .artifacts
                .iter()
                .filter(|artifact| artifact.plan == live::Plan::Check)
                .count(),
        ),
        artifacts: count(current.artifacts.len()),
        failure: currents
            .failed
            .lock()
            .expect("failures lock")
            .get(&place)
            .cloned(),
    })
}

/// The same Artifacts, parts and edges in the same order, whatever they would do.
fn same_shape(before: &Current, after: &Current) -> bool {
    before
        .artifacts
        .iter()
        .map(|artifact| (&artifact.name, &artifact.parts))
        .eq(after
            .artifacts
            .iter()
            .map(|artifact| (&artifact.name, &artifact.parts)))
        && before
            .edges
            .iter()
            .map(|edge| (&edge.from, &edge.to))
            .eq(after.edges.iter().map(|edge| (&edge.from, &edge.to)))
}

/// The graph of `current`, every Artifact lit and noted by what verify would do.
pub(crate) fn shape(current: &Current) -> Shape {
    let names: std::collections::BTreeSet<_> = current
        .artifacts
        .iter()
        .map(|artifact| &artifact.name)
        .collect();
    Shape {
        nodes: current
            .artifacts
            .iter()
            .map(|artifact| GraphNode {
                id: node_id(&artifact.name),
                name: artifact.name.to_string(),
                light: artifact.plan.into(),
                note: Some(artifact.text()),
                parts: artifact
                    .parts
                    .iter()
                    .filter(|part| names.contains(part))
                    .map(node_id)
                    .collect(),
            })
            .collect(),
        edges: current
            .edges
            .iter()
            .map(|edge| GraphEdge {
                source: node_id(&edge.from),
                target: node_id(&edge.to),
                light: edge.plan.into(),
            })
            .collect(),
    }
}

fn plan_line(plan: live::Plan, text: String, reason: &str) -> PlanLine {
    PlanLine {
        glyph: plan.glyph().into(),
        text,
        tone: plan.tone().into(),
        reason: reason.to_owned(),
    }
}

fn section(name: SectionName, title: &str, fields: Vec<Field>) -> Option<SectionView> {
    (!fields.is_empty()).then(|| SectionView {
        name,
        title: title.into(),
        fields,
        waits: Vec::new(),
    })
}

fn field(key: &str, value: impl Into<String>) -> Field {
    Field {
        key: key.into(),
        value: value.into(),
    }
}

/// The Detail of `node` of `current`: an Artifact's or an eval's.
pub(crate) fn detail(
    current: &Current,
    source: Source,
    node: &NodeId,
) -> Result<DetailView, Failure> {
    let missing = || Failure::Missing(format!("{node} is not in this Current."));
    let target = artifactize_core::monitor::Target::from(node);
    let (target, title, plan, sections, card, profile) = match &target {
        artifactize_core::monitor::Target::Artifact(name) => {
            let artifact = current
                .artifacts
                .iter()
                .find(|artifact| &artifact.name == name)
                .ok_or_else(missing)?;
            let fields = vec![
                field("Path", artifactize_tools::files::path_text(&artifact.path)),
                field("Kind", format!("{:?}", artifact.kind).to_lowercase()),
                field("State", artifact.state.to_string()),
                field("Reason", artifact.reason.clone()),
            ];
            (
                DetailTarget::Artifact,
                format!("Artifact {name}"),
                plan_line(artifact.plan, artifact.text(), &artifact.reason),
                section(SectionName::What, "What", fields),
                None,
                None,
            )
        }
        artifactize_core::monitor::Target::Eval(id) => {
            let eval = current
                .artifacts
                .iter()
                .flat_map(|artifact| &artifact.evals)
                .find(|eval| &eval.id == id)
                .ok_or_else(missing)?;
            let fields = eval
                .facts
                .iter()
                .map(|(key, value)| field(key, value.clone()))
                .collect();
            (
                DetailTarget::Eval,
                format!("{} · {}", eval.id, eval.title),
                plan_line(eval.plan, eval.plan.word().into(), &eval.reason),
                section(SectionName::Technical, "Technical", fields),
                Some(CardView::from(eval.card.clone())),
                Some(Profile::from(eval.kind)),
            )
        }
        artifactize_core::monitor::Target::Run => return Err(missing()),
    };
    Ok(DetailView {
        source,
        node: node.clone().into(),
        target,
        summary: format!("{} · {}", plan.text, plan.reason),
        title,
        request: None,
        profile,
        mark: None,
        plan: Some(plan),
        sections: sections.into_iter().collect(),
        evidence: None,
        card,
        running: false,
    })
}

/// Scan `branch`'s worktree as it is now, answering with the scan's first step: the graph,
/// every Artifact still being checked. Each later step is announced by [`CurrentChanged`].
/// While the branch's scan still runs, this answers with that scan instead; when the person
/// asked to `refresh`, a new scan follows it.
#[tauri::command]
#[specta::specta]
pub async fn current_scan(
    app: tauri::State<'_, App>,
    branch: BranchKey,
    refresh: bool,
) -> Result<CurrentView, Failure> {
    let place = branch.to_place();
    let listed = app.browse.branch(&place)?;
    let mut stage = match app.currents.request(&place, refresh) {
        Request::Join(stage) => stage,
        Request::Run(fresh) => {
            let receiver = fresh.stage.subscribe();
            let currents = app.currents.clone();
            let state = app.launch.state.clone();
            let key = branch.clone();
            tauri::async_runtime::spawn(async move {
                let mut next = Some(fresh);
                while let Some(Fresh { scan, token, stage }) = next.take() {
                    let found = live::scan(&listed.workspace, &state, token, |current| {
                        if currents.publish(&place, scan, current) {
                            stage.send_replace(Stage::Published);
                            currents.announce(&key);
                        }
                    })
                    .await;
                    let outcome = found.map(|current| {
                        currents.publish(&place, scan, current);
                    });
                    if let Some(again) = currents.end(&place, scan, outcome.clone().err()) {
                        currents.announce(&key);
                        next = again;
                    }
                    stage.send_replace(Stage::Ended(outcome));
                }
            });
            receiver
        }
    };
    let stage = stage
        .wait_for(|stage| *stage != Stage::Started)
        .await
        .map(|stage| stage.clone())
        .map_err(|_| {
            Failure::Read("The scan stopped before it began; choose Current again.".into())
        })?;
    if let Stage::Ended(Err(error)) = stage {
        return Err(Failure::Read(error));
    }
    view(&app.currents, branch)
}

/// The newest step of `branch`'s Current, without scanning.
#[tauri::command]
#[specta::specta]
pub async fn current_view(
    app: tauri::State<'_, App>,
    branch: BranchKey,
) -> Result<CurrentView, Failure> {
    view(&app.currents, branch)
}

/// Every Artifact, eval criterion and labeled relation of `branch`'s Current as text.
#[tauri::command]
#[specta::specta]
pub async fn current_structure(
    app: tauri::State<'_, App>,
    branch: BranchKey,
) -> Result<String, Failure> {
    Ok(app
        .currents
        .shown(&branch.to_place())?
        .current
        .structure
        .to_string())
}

#[cfg(test)]
mod tests;
