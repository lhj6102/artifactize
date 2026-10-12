//! Which Artifacts a tree row waits on.
use super::model::{self, Node, Upstream};

/// The Artifacts a row depends on: an eval's own, or the union over an Artifact's evals.
pub fn upstream(nodes: &[Node], path: &[model::NodeId]) -> Vec<Upstream> {
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
