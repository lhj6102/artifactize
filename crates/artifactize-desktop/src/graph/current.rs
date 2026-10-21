//! A branch's Current: its worktree as it is now, scanned by the library's
//! [`live::scan`] as `artifactize status` does. The last scan of each branch is kept for its
//! graph and Details; scanning a branch again cancels its scan still running.
use super::{Graph, GraphEdge, GraphNode, Source, node_id};
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
use serde::Serialize;
use specta::Type;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

/// A branch's last scan and its number among this app's scans.
#[derive(Clone)]
pub struct Shown {
    pub current: Arc<Current>,
    pub revision: u32,
}

/// The last scan of each branch, and the scans running.
#[derive(Default)]
pub struct Currents {
    shown: Mutex<BTreeMap<Place, Shown>>,
    running: Mutex<BTreeMap<Place, (u32, CancellationToken)>>,
    scans: std::sync::atomic::AtomicU32,
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

    /// Start scan number `scan` of `place`, cancelling the one still running.
    fn start(&self, place: &Place) -> (u32, CancellationToken) {
        let scan = self
            .scans
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let token = CancellationToken::new();
        if let Some((_, old)) = self
            .running
            .lock()
            .expect("scans lock")
            .insert(place.clone(), (scan, token.clone()))
        {
            old.cancel();
        }
        (scan, token)
    }

    /// Scan `scan` of `place` ended: keep what it found unless a newer scan started.
    fn end(&self, place: &Place, scan: u32, found: Option<Current>) -> bool {
        let mut running = self.running.lock().expect("scans lock");
        if running.get(place).is_none_or(|(at, _)| *at != scan) {
            return false;
        }
        running.remove(place);
        if let Some(current) = found {
            self.shown.lock().expect("currents lock").insert(
                place.clone(),
                Shown {
                    current: Arc::new(current),
                    revision: scan,
                },
            );
        }
        true
    }

    /// Forget every scan of a replaced state and cancel those running.
    pub fn reset(&self) {
        for (_, token) in self.running.lock().expect("scans lock").values() {
            token.cancel();
        }
        self.shown.lock().expect("currents lock").clear();
    }
}

/// A branch's Current as the Main area shows it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CurrentView {
    pub branch: BranchKey,
    /// When it was scanned, at milliseconds since the Unix epoch.
    pub scanned_at: f64,
    pub fingerprint: crate::ids::FingerprintKey,
    /// Every Artifact, eval criterion and labeled relation as text.
    pub structure: String,
    /// Each Artifact with its evals, as the Detail's chips and lists show them.
    pub tree: Vec<TreeNode>,
    /// Grows with each scan: the graph and an open Detail read again when it does.
    pub revision: u32,
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

fn view(branch: BranchKey, shown: &Shown) -> CurrentView {
    let current = &shown.current;
    CurrentView {
        branch,
        scanned_at: (current.scanned_at.unix_timestamp_nanos() / 1_000_000) as f64,
        fingerprint: current.fingerprint.clone().into(),
        structure: current.structure.clone(),
        tree: current.artifacts.iter().map(artifact_row).collect(),
        revision: shown.revision,
    }
}

pub(crate) fn graph(current: &Current, source: Source) -> Graph {
    Graph {
        nodes: current
            .artifacts
            .iter()
            .map(|artifact| GraphNode {
                id: node_id(&artifact.name),
                name: artifact.name.to_string(),
                light: artifact.plan.into(),
                note: Some(artifact.text()),
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
        source,
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

/// Scan `branch`'s worktree as it is now. Scanning it again cancels this scan, which then
/// answers with the newer one's view once that one ends.
#[tauri::command]
#[specta::specta]
pub async fn current_scan(
    app: tauri::State<'_, App>,
    branch: BranchKey,
) -> Result<CurrentView, Failure> {
    let place = branch.to_place();
    let listed = app.browse.branch(&place)?;
    let (scan, token) = app.currents.start(&place);
    let found = live::scan(&listed.workspace, &app.launch.state, token).await;
    match found {
        Ok(current) => {
            app.currents.end(&place, scan, Some(current));
        }
        Err(error) => {
            if app.currents.end(&place, scan, None) {
                return Err(Failure::Read(error));
            }
        }
    }
    Ok(view(branch, &app.currents.shown(&place)?))
}
