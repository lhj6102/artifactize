//! The shown snapshot as the frontend draws it: plain data, every word and glyph from the
//! library's model.
use super::{Opened, Shown};
use crate::status::{Mark, Tone, Weight};
use artifactize_core::{
    monitor::{
        self, Completion as Done, Kind, Node, NodeId,
        live::{self, Branch, Head, Place, Waiting},
    },
    types::RunStatus,
};
use artifactize_tools::files::{path_from_text, path_text};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use time::OffsetDateTime;

/// A path as the frontend holds it: text with `/` on every system and a reversible encoding
/// for names that are not Unicode, read back into the same native path where it enters.
#[derive(Debug, Clone, PartialEq, Eq, Type)]
#[specta(transparent)]
pub struct PathKey(#[specta(type = String)] PathBuf);

impl PathKey {
    pub fn new(path: &std::path::Path) -> Self {
        Self(path.to_owned())
    }
    pub fn to_path(&self) -> PathBuf {
        self.0.clone()
    }
}

impl Serialize for PathKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&path_text(&self.0))
    }
}

impl<'de> Deserialize<'de> for PathKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        path_from_text(&text)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// A branch by value, so a selection survives refreshes that reorder the pane. Paths are
/// round-trip; names for people come with the branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BranchKey {
    /// A Git repository's branch, by the repository's common directory.
    Branch {
        common: PathKey,
        /// Refused at the boundary unless a branch name.
        #[specta(type = String)]
        branch: live::BranchName,
    },
    /// A detached Git worktree, by its folder.
    Detached { common: PathKey, path: PathKey },
    /// A folder outside Git.
    Folder { path: PathKey },
}

impl BranchKey {
    pub fn new(place: &Place) -> Self {
        match place {
            Place::Git {
                common,
                head: Head::Branch(branch),
            } => Self::Branch {
                common: PathKey::new(common),
                branch: branch.clone(),
            },
            Place::Git {
                common,
                head: Head::Detached(path),
            } => Self::Detached {
                common: PathKey::new(common),
                path: PathKey::new(path),
            },
            Place::Folder(path) => Self::Folder {
                path: PathKey::new(path),
            },
        }
    }

    pub fn to_place(&self) -> Place {
        match self {
            Self::Branch { common, branch } => Place::Git {
                common: common.to_path(),
                head: Head::Branch(branch.clone()),
            },
            Self::Detached { common, path } => Place::Git {
                common: common.to_path(),
                head: Head::Detached(path.to_path()),
            },
            Self::Folder { path } => Place::Folder(path.to_path()),
        }
    }
}

/// One Run in a branch's timeline.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Chip {
    Run {
        id: crate::ids::RunKey,
        mark: Mark,
        /// Its status in words: `GREEN`, `RED`, `Running`, `Waiting for sign-off`.
        status: String,
        /// How long ago it started, `25m ago`; an open one's time so far, `2m 22s`.
        age: String,
        running: bool,
        /// What its requester said it is working on (`verify --comment`).
        comment: Option<String>,
    },
}

/// One live branch of the pane: `project / branch`, then its timeline, newest first.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BranchView {
    pub key: BranchKey,
    pub project: String,
    /// The branch, a detached worktree's folder, or empty for a folder outside Git.
    pub name: String,
    /// Where its Current is scanned and its tools run.
    pub workspace: PathKey,
    /// Its timeline: its newest Runs, newest first, led by the one in progress.
    pub chips: Vec<Chip>,
    /// The newest Run by creation time, independently of the timeline page.
    pub newest: Option<Chip>,
    /// Older Runs than the timeline holds exist: **Older runs** at its end shows them.
    pub older: bool,
    /// Starred: listed before the branches that are not.
    pub starred: bool,
    /// A saved timeline entry pinned by a reference, not a live checkout.
    pub historical: bool,
}

/// One Human review waiting for a person.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WaitingRow {
    pub id: crate::ids::RequestKey,
    pub eval: String,
    pub title: String,
    pub mark: Mark,
    pub project: String,
    pub branch: String,
    /// How long it waits.
    pub age: String,
    /// `-`, the claimant, or `name (you)`.
    pub claim: String,
    pub mine: bool,
}

/// The pane's search as typed, and why it lists nothing when it cannot be read.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueryView {
    pub text: String,
    pub error: Option<String>,
    /// The query the search starts with, which Esc puts back.
    pub start: String,
}

/// The completion of a search's last term.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    /// The search before the completed term; a candidate replaces the rest.
    pub keep: String,
    pub candidates: Vec<String>,
}

/// The Artifacts a row depends on, with whether each is complete (dim) or unmet (bright).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamMark {
    /// The Artifact row's tree id.
    pub node: crate::ids::NodeKey,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    Run,
    Artifact,
    Eval,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub text: String,
    pub tone: Option<Tone>,
}

/// One tree row with its children: the Run row, an Artifact or an eval.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    /// Stable tree id: `run:<id>`, `a:<artifact>` or `e:<eval>`.
    pub id: crate::ids::NodeKey,
    pub kind: NodeKind,
    pub glyph: String,
    pub tone: Tone,
    pub weight: Weight,
    pub name: String,
    /// Dim markers after the name: `↻ peer`, `[file]`.
    pub marks: String,
    pub text: Vec<Segment>,
    /// Elapsed time of an eval, `passed/total` of an Artifact.
    pub right: String,
    /// The derived state changed after the Run ended.
    pub changed: bool,
    /// An Artifact whose evals are all done.
    pub done: bool,
    /// An in-progress or failed eval.
    pub attention: bool,
    /// An ERROR, RED or waiting Human eval.
    pub urgent: bool,
    /// The Artifacts this row depends on, unmet first.
    pub upstream: Vec<UpstreamMark>,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub mark: Mark,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RunOrigin {
    Cli,
    Mcp,
    Chat {
        session_id: crate::ids::SessionKey,
        title: String,
    },
}

impl From<&artifactize_core::runs::Origin> for RunOrigin {
    fn from(origin: &artifactize_core::runs::Origin) -> Self {
        use artifactize_core::runs::Origin;
        match origin {
            Origin::Cli => Self::Cli,
            Origin::Mcp => Self::Mcp,
            Origin::Chat {
                session_id, title, ..
            } => Self::Chat {
                session_id: session_id.clone().into(),
                title: title.clone(),
            },
        }
    }
}

/// The shown Run: its headline, the verdict and what needs attention; its tally, time and
/// comment are its Detail's. Its rows come one at a time, as a Detail asks for them ([`row`]):
/// a Run can hold tens of thousands.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunPanel {
    pub id: crate::ids::RunKey,
    /// The Run's timeline branch, including a historical branch pinned by its reference.
    pub branch: Option<BranchKey>,
    /// The branch for Open in Chat, even when it is no longer checked out.
    pub chat_branch: BranchKey,
    pub origin: Option<RunOrigin>,
    /// The Run comment's first line. The full comment belongs in Detail.
    pub headline: Option<String>,
    /// The Run's own row, whose Detail its headline opens.
    pub node: crate::ids::NodeKey,
    pub mark: Mark,
    /// Each eval that is RED or ERROR and each sign-off that waits, by its status and ID, the
    /// failures first.
    pub needs: Vec<Attention>,
}

/// The Run the Main area shows.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShownRun {
    /// No Run is shown.
    None,
    /// The shown Run, read.
    Read(Box<RunPanel>),
    /// The shown Run no longer exists or cannot be read.
    Missing,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowseView {
    pub query: QueryView,
    /// The branches the search keeps, in its order.
    pub branches: Vec<BranchView>,
    /// The waiting Human reviews the search keeps, newest first.
    pub waiting: Vec<WaitingRow>,
    /// The branch of the folder the app was started for.
    pub launch: Option<BranchKey>,
    /// The Run shown in the Main area: none, read, or no longer there to read.
    pub run: ShownRun,
    /// What went wrong: a failed read, Run records that cannot be read.
    pub notice: Option<String>,
    /// False until the first snapshot was read.
    pub refreshed: bool,
    /// The state was replaced since the last view: Details and drafts no longer apply.
    pub replaced: bool,
    /// Grows with every change of the shown snapshot; a response with a lower revision is
    /// older than one already shown, and views derived from the snapshot (Detail) read again
    /// when it grows.
    pub revision: u32,
}

fn node(node: &Node, kind: NodeKind, now: OffsetDateTime, upstream: Vec<UpstreamMark>) -> TreeNode {
    TreeNode {
        id: node.id.clone().into(),
        kind,
        glyph: node.glyph.into(),
        tone: node.tone.into(),
        weight: node.weight.into(),
        name: node.name.clone(),
        marks: node.marks.clone(),
        text: node
            .text
            .iter()
            .map(|segment| Segment {
                text: segment.text.clone(),
                tone: segment.tone.map(Into::into),
            })
            .collect(),
        right: node.right_at(now, false),
        changed: node.changed,
        done: node.done(),
        attention: node.attention(),
        urgent: node.urgent(),
        upstream,
        children: Vec::new(),
    }
}

fn marks(nodes: &[Node], path: &[NodeId]) -> Vec<UpstreamMark> {
    monitor::upstream(nodes, path)
        .into_iter()
        .map(|up| UpstreamMark {
            node: NodeId::artifact(up.artifact).into(),
            complete: up.completion == Done::Complete,
        })
        .collect()
}

/// The row `id` of the Run's tree: the Run's own row, or an Artifact with its eval rows.
pub(crate) fn row(
    run: &artifactize_core::store::RunView,
    nodes: &[Node],
    id: &NodeId,
    now: OffsetDateTime,
) -> Option<TreeNode> {
    let own = monitor::run_node(run);
    if &own.id == id {
        return Some(node(&own, NodeKind::Run, now, Vec::new()));
    }
    let artifact = nodes.iter().find(|artifact| &artifact.id == id)?;
    let path = [artifact.id.clone()];
    let mut row = node(artifact, NodeKind::Artifact, now, marks(nodes, &path));
    row.children = artifact
        .children
        .iter()
        .map(|eval| {
            let path = [artifact.id.clone(), eval.id.clone()];
            let kind = match eval.kind {
                Kind::Eval(_) => NodeKind::Eval,
                Kind::Artifact { .. } => NodeKind::Artifact,
            };
            node(eval, kind, now, marks(nodes, &path))
        })
        .collect();
    Some(row)
}

/// A Run's status as its chip names it.
fn status_word(status: RunStatus) -> String {
    match status {
        RunStatus::Running => "Running".into(),
        RunStatus::WaitingHuman => "Waiting for sign-off".into(),
        other => other.as_str().replace('_', " "),
    }
}

fn run_chip(chip: &live::Chip, now: OffsetDateTime) -> Chip {
    let running = chip.status.is_open();
    let age = monitor::duration(now - chip.created_at.time());
    Chip::Run {
        id: chip.id.clone().into(),
        mark: monitor::Status::from(chip.status).into(),
        status: status_word(chip.status),
        age: if running { age } else { format!("{age} ago") },
        running,
        comment: chip.comment.as_ref().map(ToString::to_string),
    }
}

fn branch(branch: &Branch, limit: usize, starred: bool, now: OffsetDateTime) -> BranchView {
    let older = branch.runs.len() > limit;
    let chips = branch
        .timeline()
        .take(limit)
        .map(|chip| run_chip(chip, now))
        .collect();
    BranchView {
        key: BranchKey::new(&branch.place),
        project: branch.project.clone(),
        name: branch.name.clone(),
        workspace: PathKey::new(&branch.workspace),
        chips,
        newest: branch.runs.last().map(|chip| run_chip(chip, now)),
        older,
        starred,
        historical: false,
    }
}

fn waiting(waiting: &Waiting, now: OffsetDateTime) -> WaitingRow {
    let request = &waiting.view.request;
    let entry = &waiting.entry;
    WaitingRow {
        id: request.id.clone().into(),
        eval: entry.eval.clone(),
        title: request.title.clone(),
        mark: monitor::Status::from(entry.status).into(),
        project: waiting.label.project.clone(),
        branch: waiting.label.branch.clone(),
        age: monitor::duration(now - request.created_at.time()),
        claim: match &entry.claim {
            artifactize_core::review::search::Claim::Unclaimed => "-".into(),
            artifactize_core::review::search::Claim::Mine(reviewer) => format!("{reviewer} (you)"),
            artifactize_core::review::search::Claim::Other(reviewer) => reviewer.clone(),
        },
        mine: matches!(
            entry.claim,
            artifactize_core::review::search::Claim::Mine(_)
        ),
    }
}

pub(super) fn build(shown: &Shown, now: OffsetDateTime) -> BrowseView {
    let historical = (shown.reference && shown.opened_branch.is_none())
        .then(|| {
            shown
                .opened
                .loaded()
                .map(|selected| Branch::historical(&selected.run.run))
        })
        .flatten();
    let timeline_branch = shown
        .opened_branch
        .as_ref()
        .or_else(|| historical.as_ref().map(|branch| &branch.place));
    let run = shown.opened.loaded().map(|selected| {
        let (run, requests) = (&selected.run, &selected.requests);
        let progress = monitor::progress(run, requests, now);
        RunPanel {
            id: run.run.id.clone().into(),
            branch: timeline_branch.map(BranchKey::new),
            chat_branch: shown.opened_branch.as_ref().map_or_else(
                || BranchKey::new(&Place::recorded_run(&run.run)),
                BranchKey::new,
            ),
            origin: run.run.origin.as_ref().map(Into::into),
            headline: run
                .run
                .comment
                .as_ref()
                .and_then(|comment| comment.as_str().lines().next())
                .map(str::to_owned),
            node: NodeId::run(&run.run.id).into(),
            mark: progress.status.into(),
            needs: progress
                .needs
                .iter()
                .map(|(status, eval)| Attention {
                    mark: (*status).into(),
                    text: eval.clone(),
                })
                .collect(),
        }
    });
    let none = std::collections::BTreeSet::new();
    let starred = shown.starred.as_ref().unwrap_or(&none);
    let mut branches = shown.query.parsed.as_ref().map_or_else(
        |_| Vec::new(),
        |query| live::show(query, &shown.branches, &[], starred).0,
    );
    // A direct Run reference is visible without changing the saved Verify search.
    if shown.reference
        && let Some(place) = &shown.opened_branch
        && !branches.iter().any(|branch| &branch.place == place)
        && let Some(linked) = shown.branches.iter().find(|branch| &branch.place == place)
    {
        branches.insert(0, linked);
    }
    if let Some(branch) = &historical {
        branches.insert(0, branch);
    }
    let branches = branches
        .into_iter()
        .map(|shown_branch| {
            let limit = shown.chip_limit(shown_branch);
            let mut row = branch(
                shown_branch,
                limit,
                starred.contains(&shown_branch.place),
                now,
            );
            row.historical = historical
                .as_ref()
                .is_some_and(|archived| archived.place == shown_branch.place);
            row
        })
        .collect();
    let waiting_rows = shown.waiting.iter().map(|row| waiting(row, now)).collect();
    let notice: Vec<String> = shown
        .error
        .iter()
        .cloned()
        .chain(shown.unreadable.iter().map(ToString::to_string))
        .chain(
            shown
                .opened
                .loaded()
                .into_iter()
                .flat_map(|selected| &selected.run.unreadable)
                .filter(|record| !shown.unreadable.contains(record))
                .map(ToString::to_string),
        )
        .chain(
            shown
                .unstarred
                .iter()
                .map(|error| format!("Starred branches: {error}")),
        )
        .collect();
    BrowseView {
        query: QueryView {
            text: shown.query.text.clone(),
            error: shown.query.parsed.as_ref().err().cloned(),
            start: live::DEFAULT.to_owned(),
        },
        branches,
        waiting: waiting_rows,
        launch: shown.launch.as_ref().map(BranchKey::new),
        run: match (run, &shown.opened) {
            (Some(panel), _) => ShownRun::Read(Box::new(panel)),
            (None, Opened::Missing(_)) => ShownRun::Missing,
            (None, _) => ShownRun::None,
        },
        notice: (!notice.is_empty()).then(|| notice.join("; ")),
        refreshed: shown.refreshed,
        replaced: shown.replaced,
        revision: shown.revision,
    }
}

#[cfg(test)]
mod tests;
