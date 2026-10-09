//! Tree folding, where the cursor starts, and the blocker jumps (`b` / Backspace).
use std::{
    collections::{BTreeSet, HashSet},
    sync::Arc,
};

use time::OffsetDateTime;

use super::{
    Monitor,
    model::{self, Node, Upstream},
};
use crate::types::RunId;

/// Tree state that outlives a refresh but not a change of Run.
#[derive(Debug, Default)]
pub(super) struct Folds {
    /// Artifacts the user folded or unfolded; automatic folding leaves them alone.
    touched: BTreeSet<String>,
    blocker: Option<Blocker>,
    /// The open Run's tree, built once per state change and shared by every frame.
    cache: Option<(RunId, Arc<Vec<Node>>)>,
}

/// A `b` jump: the row it started from and the Artifact row it reached.
#[derive(Debug)]
struct Blocker {
    origin: Vec<String>,
    at: Vec<String>,
}

/// The Artifacts a row depends on: an eval's own, or the union over an Artifact's evals.
pub(super) fn upstream(nodes: &[Node], path: &[String]) -> Vec<Upstream> {
    let Some(id) = path.last() else {
        return Vec::new();
    };
    let Some(artifact) = nodes.iter().find(|node| Some(&node.id) == path.first()) else {
        return Vec::new();
    };
    if artifact.id == *id {
        let mut merged: Vec<Upstream> = Vec::new();
        for child in &artifact.children {
            for up in &child.upstream {
                if !merged.iter().any(|known| known.artifact == up.artifact) {
                    merged.push(up.clone());
                }
            }
        }
        // Unmet first, keeping each eval's X order.
        merged.sort_by_key(|up| up.completion == model::Completion::Complete);
        return merged;
    }
    artifact
        .children
        .iter()
        .find(|child| child.id == *id)
        .map_or_else(Vec::new, |child| child.upstream.clone())
}

impl Monitor {
    /// The open Run's tree. `sync_tree` rebuilds it when the Run's state changes; frames,
    /// keys and hints reuse it, and redraw elapsed times from each row's clock.
    pub(super) fn nodes(&mut self) -> Arc<Vec<Node>> {
        let Some((run, requests)) = &self.run else {
            self.folds.cache = None;
            return Arc::default();
        };
        if let Some((id, nodes)) = &self.folds.cache
            && *id == run.run.id
        {
            return nodes.clone();
        }
        let nodes = Arc::new(model::tree(run, requests, OffsetDateTime::now_utc()));
        self.folds.cache = Some((run.run.id.clone(), nodes.clone()));
        nodes
    }

    /// Fold Artifacts whose evals are all done and unfold the rest, except the ones the user
    /// toggled. A new Run starts on the first in-progress or failed eval, else the first row.
    pub(super) fn sync_tree(&mut self, first: bool) {
        if first {
            self.folds = Folds::default();
        }
        self.folds.cache = None;
        let nodes = self.nodes();
        for node in nodes.iter().filter(|node| !node.children.is_empty()) {
            if self.folds.touched.contains(&node.id) {
                continue;
            }
            let path = vec![node.id.clone()];
            if !node.done() {
                self.tree.open(path);
            } else if self.tree.close(&path) && self.tree.selected().len() > 1 {
                // Never leave the cursor on a row that just folded away.
                if self.tree.selected().first() == Some(&node.id) {
                    self.tree.select(path);
                }
            }
        }
        if first {
            let start = nodes
                .iter()
                .find_map(|artifact| {
                    let eval = artifact.children.iter().find(|eval| eval.attention())?;
                    Some(vec![artifact.id.clone(), eval.id.clone()])
                })
                .or_else(|| nodes.first().map(|node| vec![node.id.clone()]));
            if let Some(path) = start {
                self.tree.select(path);
            }
        }
    }

    /// Remember Artifacts whose fold state the user changed since `before`.
    pub(super) fn touched(&mut self, before: &HashSet<Vec<String>>) {
        let after = self.tree.opened();
        for path in before.symmetric_difference(after) {
            if let [id] = path.as_slice() {
                self.folds.touched.insert(id.clone());
            }
        }
    }

    /// The row whose upstream is highlighted: a `b` cycle's origin while the cursor stays
    /// where the last jump left it, otherwise the selected row.
    fn origin(&self) -> Vec<String> {
        match &self.folds.blocker {
            Some(blocker) if blocker.at == self.tree.selected() => blocker.origin.clone(),
            _ => self.tree.selected().to_vec(),
        }
    }

    /// The Artifacts to mark with `↑`.
    pub(super) fn highlighted(&self, nodes: &[Node]) -> Vec<Upstream> {
        upstream(nodes, &self.origin())
    }

    /// `b`: move to the next Artifact the origin row waits for, unmet ones first.
    pub(super) fn jump_blocker(&mut self) {
        let nodes = self.nodes();
        let origin = self.origin();
        let targets = upstream(&nodes, &origin);
        if targets.is_empty() {
            return;
        }
        let next = match &self.folds.blocker {
            Some(blocker) if blocker.at == self.tree.selected() => targets
                .iter()
                .position(|up| Some(&format!("a:{}", up.artifact)) == blocker.at.first())
                .map_or(0, |index| (index + 1) % targets.len()),
            _ => 0,
        };
        let at = vec![format!("a:{}", targets[next].artifact)];
        self.tree.select(at.clone());
        self.folds.blocker = Some(Blocker { origin, at });
    }

    /// Backspace: return to the row the `b` jumps started from. If that eval's Artifact
    /// folded meanwhile (say, its evals all finished), unfold it so the cursor stays visible;
    /// the user's choice then holds against automatic folding.
    pub(super) fn jump_back(&mut self) {
        let Some(blocker) = self.folds.blocker.take() else {
            return;
        };
        if blocker.at != self.tree.selected() {
            return;
        }
        if let [artifact, _, ..] = blocker.origin.as_slice() {
            let artifact = artifact.clone();
            if self.tree.open(vec![artifact.clone()]) {
                self.folds.touched.insert(artifact);
            }
        }
        self.tree.select(blocker.origin);
    }

    /// The key hint for `b`, shown only when the selected row depends on an Artifact.
    pub(super) fn blocker_hint(&mut self) -> &'static str {
        if self.focus != super::Pane::Artifacts {
            return "";
        }
        let nodes = self.nodes();
        if !self.highlighted(&nodes).is_empty() {
            " · b/Backspace blocker"
        } else {
            ""
        }
    }
}
