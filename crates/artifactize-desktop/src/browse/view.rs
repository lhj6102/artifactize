//! The shown snapshot as the frontend draws it: plain data, every word and glyph from the
//! library's model.
use super::{CHIPS, Opened, Shown};
use crate::status::{Mark, Tone, Weight};
use artifactize_core::{
    monitor::{
        self, Completion as Done, Kind, Node, NodeId,
        live::{self, Branch, Head, Place, Waiting},
    },
    types::RunStatus,
};
use artifactize_tools::files::path_text;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use time::OffsetDateTime;

/// A path as the frontend holds it: text with `/` on every system, which `PathBuf` reads back
/// on all of them. Only this type makes or reads one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct PathKey(String);

impl PathKey {
    pub fn new(path: &std::path::Path) -> Self {
        Self(path_text(path))
    }
    pub fn to_path(&self) -> PathBuf {
        PathBuf::from(&self.0)
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

/// One chip of a branch's row: a Run, or the branch's Current.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Chip {
    Run {
        id: crate::ids::RunKey,
        mark: Mark,
        /// Its status and age, a running one's its time so far: `RED 25m`, `Running 2m 22s`.
        text: String,
        running: bool,
    },
    /// The worktree as it is now.
    Current,
}

/// One live branch of the pane: `project  branch`, then its row of chips, newest first.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BranchView {
    pub key: BranchKey,
    pub project: String,
    /// The branch, a detached worktree's folder, or empty for a folder outside Git.
    pub name: String,
    /// Where its Current is scanned and its tools run.
    pub workspace: PathKey,
    /// Its newest Runs, newest first, led by the one in progress or else the Current chip.
    pub chips: Vec<Chip>,
    /// Older Runs than the row holds exist: reaching its oldest end, on the right, shows them.
    pub older: bool,
    /// Starred: listed before the branches that are not.
    pub starred: bool,
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

/// The shown Run: its headline and what needs attention. Its rows come one at a time, as a
/// Detail asks for them ([`row`]): a Run can hold tens of thousands.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunPanel {
    pub id: crate::ids::RunKey,
    /// The Run's own row, whose Detail its headline opens.
    pub node: crate::ids::NodeKey,
    pub mark: Mark,
    /// Validation at Run end, counts, time and tokens.
    pub headline: String,
    /// Counts alone, for a narrow pane.
    pub counts: String,
    pub attention: Vec<Attention>,
    /// Attention lines beyond the shown ones.
    pub more: u32,
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
    /// The shown Run, once read.
    pub run: Option<RunPanel>,
    /// The shown Run no longer exists or cannot be read.
    pub missing: bool,
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
        other => other.as_str().replace('_', " "),
    }
}

fn branch(branch: &Branch, limit: usize, starred: bool, now: OffsetDateTime) -> BranchView {
    let skipped = branch.runs.len().saturating_sub(limit);
    let mut chips: Vec<Chip> = branch.runs[skipped..]
        .iter()
        .map(|chip| Chip::Run {
            id: chip.id.clone().into(),
            mark: monitor::Status::from(chip.status).into(),
            text: format!(
                "{} {}",
                status_word(chip.status),
                monitor::duration(now - chip.created_at.time())
            ),
            running: false,
        })
        .collect();
    match (branch.running(), chips.last_mut()) {
        (Some(_), Some(Chip::Run { running, .. })) => *running = true,
        _ => chips.push(Chip::Current),
    }
    // What is happening now comes first, right under the branch's name.
    chips.reverse();
    BranchView {
        key: BranchKey::new(&branch.place),
        project: branch.project.clone(),
        name: branch.name.clone(),
        workspace: PathKey::new(&branch.workspace),
        chips,
        older: skipped > 0,
        starred,
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
        claim: match (&entry.reviewer, entry.mine) {
            (None, _) => "-".into(),
            (Some(reviewer), true) => format!("{reviewer} (you)"),
            (Some(reviewer), false) => reviewer.clone(),
        },
        mine: entry.mine,
    }
}

pub(super) fn build(shown: &Shown, now: OffsetDateTime) -> BrowseView {
    let run = shown.opened.loaded().map(|selected| {
        let (run, requests) = (&selected.run, &selected.requests);
        let progress = monitor::progress(run, requests, now);
        let strip = monitor::strip(&progress);
        RunPanel {
            id: run.run.id.clone().into(),
            node: NodeId::run(&run.run.id).into(),
            mark: strip.status.into(),
            headline: strip.headline.clone(),
            counts: monitor::counts(progress.counts.iter().copied()),
            attention: strip
                .attention
                .iter()
                .map(|(status, text)| Attention {
                    mark: (*status).into(),
                    text: text.clone(),
                })
                .collect(),
            more: u32::try_from(strip.more).unwrap_or(u32::MAX),
        }
    });
    let (branches, waiting_rows) = match &shown.query.parsed {
        Ok(query) => {
            let none = std::collections::BTreeSet::new();
            let starred = shown.starred.as_ref().unwrap_or(&none);
            let (branches, waiting_rows) =
                live::show(query, &shown.branches, &shown.waiting, starred);
            (
                branches
                    .into_iter()
                    .map(|shown_branch| {
                        let limit = shown
                            .chips
                            .get(&shown_branch.place)
                            .copied()
                            .unwrap_or(CHIPS);
                        branch(
                            shown_branch,
                            limit,
                            starred.contains(&shown_branch.place),
                            now,
                        )
                    })
                    .collect(),
                waiting_rows
                    .into_iter()
                    .map(|row| waiting(row, now))
                    .collect(),
            )
        }
        Err(_) => (Vec::new(), Vec::new()),
    };
    let notice: Vec<String> = shown
        .error
        .iter()
        .cloned()
        .chain(shown.unreadable.iter().map(ToString::to_string))
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
        },
        branches,
        waiting: waiting_rows,
        launch: shown.launch.as_ref().map(BranchKey::new),
        run,
        missing: matches!(shown.opened, Opened::Missing(_)),
        notice: (!notice.is_empty()).then(|| notice.join("; ")),
        refreshed: shown.refreshed,
        replaced: shown.replaced,
        revision: shown.revision,
    }
}
