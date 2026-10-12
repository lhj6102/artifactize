//! Repository/worktree -> Run -> artifacts/evals -> Detail: the model every interface shows.
pub mod catalog;
pub mod evidence;
pub mod fold;
pub mod loader;
pub mod model;
pub mod session;

pub use catalog::{Badge, Catalog, CatalogRow, Repository, Scope};
pub use evidence::{Evidence, EvidenceStamp, evidence, original};
pub use fold::upstream;
pub use loader::{Input, Loader, Origin, Page, Snapshot, newer_offset, older_offset};
pub use model::{
    Activity, Busy, Clock, Completion, Detail, EvalView, Graph, GraphEdge, GraphNode, Kind, Light,
    Node, NodeId, NotRun, Progress, Queue, RunRow, Section, Segment, Source, Status, Strip, Target,
    Tone, Upstream, Waits, Weight, counts, detail, duration, glyph, graph, progress, run_node,
    run_rows, strip, tree, upstream_index,
};

use crate::workspace::canonical_target;
use std::{borrow::Cow, path::PathBuf};

/// Bound retained Run pages while allowing the selected scope to page older records.
pub const PAGE: u32 = 100;

/// The path that picks the first scope from cwd or `--repo`, canonical like the catalog's
/// paths; `None` (`--all`) starts on every repository.
pub fn initial(repo: Option<PathBuf>) -> Option<PathBuf> {
    repo.map(|path| canonical_target(&path).unwrap_or(path))
}

/// Text for one row: line breaks and tabs become spaces and other control characters go, so
/// every interface measures and shows what a terminal would.
pub fn plain(text: &str) -> Cow<'_, str> {
    if !text.contains(char::is_control) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .filter_map(|character| match character {
                '\n' | '\t' => Some(' '),
                character if character.is_control() => None,
                character => Some(character),
            })
            .collect(),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use crate::test_request::request;
}
