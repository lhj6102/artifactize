//! The shown snapshot as the frontend draws it: plain data, every word and glyph from the
//! monitor's model.
use super::Shown;
use crate::status::{Count, Mark, Tone, Weight};
use artifactize::monitor::{self, Completion, Kind, Node, NodeId, Repository, Scope};
use artifactize_tools::files::path_text;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use time::OffsetDateTime;

/// A repository as the catalog groups Runs: Git clones by their common directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RepositoryKey {
    /// The common Git directory, as a native path.
    Git { path: String },
    /// A non-Git workspace.
    Workspace { path: String },
}

/// A scope by value, so a selection survives refreshes that reorder the catalog. Paths are
/// native and round-trip; labels for people come with the rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScopeKey {
    All,
    Repository {
        repository: RepositoryKey,
    },
    Worktree {
        repository: RepositoryKey,
        path: String,
    },
}

fn native(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

impl RepositoryKey {
    fn new(repository: &Repository) -> Self {
        match repository {
            Repository::Git(path) => Self::Git { path: native(path) },
            Repository::Workspace(path) => Self::Workspace { path: native(path) },
        }
    }
    pub fn to_repository(&self) -> Repository {
        match self {
            Self::Git { path } => Repository::Git(PathBuf::from(path)),
            Self::Workspace { path } => Repository::Workspace(PathBuf::from(path)),
        }
    }
}

impl ScopeKey {
    fn new(scope: &Scope) -> Self {
        match scope {
            Scope::All => Self::All,
            Scope::Repository(repository) => Self::Repository {
                repository: RepositoryKey::new(repository),
            },
            Scope::Worktree(repository, path) => Self::Worktree {
                repository: RepositoryKey::new(repository),
                path: native(path),
            },
        }
    }
    pub fn to_scope(&self) -> Scope {
        match self {
            Self::All => Scope::All,
            Self::Repository { repository } => Scope::Repository(repository.to_repository()),
            Self::Worktree { repository, path } => {
                Scope::Worktree(repository.to_repository(), PathBuf::from(path))
            }
        }
    }
}

/// One catalog row: ALL, a repository, one of its worktrees, or its fold of worktrees
/// without Runs.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScopeRow {
    pub scope: ScopeKey,
    pub label: String,
    pub depth: u32,
    pub badge: Vec<Count>,
    /// Set on the fold row; toggling it shows or hides those worktrees.
    pub fold: Option<RepositoryKey>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunItem {
    pub id: String,
    pub repository: String,
    pub mark: Mark,
    /// Request glyph counts, most urgent first: `✗1 ✓2`.
    pub counts: String,
    pub age: String,
    /// Wall time of a finished Run.
    pub took: String,
    /// Records of the Run that could not be read; an unreadable Run cannot open.
    pub unreadable: Vec<String>,
    pub readable: bool,
}

/// The Artifacts a row depends on, with whether each is complete (dim) or unmet (bright).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamMark {
    /// The Artifact row's tree id.
    pub node: String,
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
    pub id: String,
    pub kind: NodeKind,
    pub glyph: String,
    pub tone: Tone,
    pub weight: Weight,
    pub name: String,
    /// Dim markers after the name: `↻ peer`, `[file]`, `[dep]`.
    pub marks: String,
    pub text: Vec<Segment>,
    /// Elapsed time of an eval, `passed/total` of an Artifact.
    pub right: String,
    /// The derived state changed after the Run ended.
    pub changed: bool,
    /// An Artifact whose evals are all done: folded unless the user unfolded it.
    pub done: bool,
    /// An in-progress or failed eval: where a new Run's cursor starts.
    pub attention: bool,
    /// An ERROR, RED or waiting Human eval: what "next attention" visits.
    pub urgent: bool,
    /// The Artifacts this row depends on, unmet first; `b` cycles through them.
    pub upstream: Vec<UpstreamMark>,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Attention {
    pub mark: Mark,
    pub text: String,
}

/// The selected Run: its headline, what needs attention, and the tree under the Run row.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunPanel {
    pub id: String,
    pub mark: Mark,
    /// Validation at Run end, counts, time and tokens.
    pub headline: String,
    /// Counts alone, for a narrow pane.
    pub counts: String,
    pub attention: Vec<Attention>,
    /// Attention lines beyond the shown ones.
    pub more: u32,
    pub tree: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowseView {
    pub scope: ScopeKey,
    /// The breadcrumb's name for the scope.
    pub scope_label: String,
    pub scopes: Vec<ScopeRow>,
    /// The catalog's attention counters for the header: `!N ✗N ◐N ?N`.
    pub badge: Vec<Count>,
    pub runs: Vec<RunItem>,
    pub selected_run: Option<String>,
    /// More Runs on an older page, or on a newer one.
    pub older: bool,
    pub newer: bool,
    pub run: Option<RunPanel>,
    pub error: Option<String>,
    /// False until the first snapshot was read.
    pub refreshed: bool,
    /// The state was replaced since the last view: Details and drafts no longer apply.
    pub replaced: bool,
    /// Grows with every change of the shown snapshot; a response with a lower revision is
    /// older than one already shown, and views derived from the snapshot (Detail) read again
    /// when it grows.
    pub revision: u32,
}

fn badge(row: &monitor::CatalogRow) -> Vec<Count> {
    row.badge
        .parts()
        .into_iter()
        .map(|(status, count)| Count::new(status, count))
        .collect()
}

fn node(node: &Node, kind: NodeKind, now: OffsetDateTime, upstream: Vec<UpstreamMark>) -> TreeNode {
    TreeNode {
        id: node.id.to_string(),
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
            node: NodeId::artifact(up.artifact).to_string(),
            complete: up.completion == Completion::Complete,
        })
        .collect()
}

/// The Run row, then each Artifact with its eval rows.
fn tree(run: &artifactize::store::RunView, nodes: &[Node], now: OffsetDateTime) -> Vec<TreeNode> {
    let run_row = node(&monitor::run_node(run), NodeKind::Run, now, Vec::new());
    std::iter::once(run_row)
        .chain(nodes.iter().map(|artifact| {
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
            row
        }))
        .collect()
}

pub(super) fn build(shown: &Shown, now: OffsetDateTime) -> BrowseView {
    let scope = &shown.page.scope;
    let run = shown.run.as_ref().map(|(run, requests, nodes)| {
        let progress = monitor::progress(run, requests, now);
        let strip = monitor::strip(&progress);
        RunPanel {
            id: run.run.id.to_string(),
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
            tree: tree(run, nodes, now),
        }
    });
    let runs = monitor::run_rows(&shown.runs, now)
        .into_iter()
        .zip(&shown.runs)
        .map(|(row, summary)| RunItem {
            id: row.id.to_string(),
            repository: path_text(&row.repo),
            mark: monitor::Status::from(row.status).into(),
            counts: row.counts,
            age: row.age,
            took: row.took,
            readable: !summary.unreadable.iter().any(|record| record.kind == "run"),
            unreadable: summary.unreadable.iter().map(ToString::to_string).collect(),
        })
        .collect();
    BrowseView {
        scope: ScopeKey::new(scope),
        scope_label: shown.catalog.label(scope),
        scopes: shown
            .catalog
            .rows
            .iter()
            .map(|row| ScopeRow {
                scope: ScopeKey::new(&row.scope),
                label: row.label.clone(),
                depth: u32::try_from(row.depth).unwrap_or(u32::MAX),
                badge: badge(row),
                fold: row.fold.as_ref().map(RepositoryKey::new),
                selected: row.fold.is_none() && row.scope == *scope,
            })
            .collect(),
        badge: shown
            .catalog
            .rows
            .first()
            .filter(|row| row.scope == Scope::All)
            .map(badge)
            .unwrap_or_default(),
        runs,
        selected_run: shown.page.open.as_ref().map(ToString::to_string),
        older: shown.runs.len() as u32 >= shown.page.limit,
        newer: shown.page.offset > 0,
        run,
        error: shown.error.clone(),
        refreshed: shown.refreshed,
        replaced: shown.replaced,
        revision: shown.revision,
    }
}
