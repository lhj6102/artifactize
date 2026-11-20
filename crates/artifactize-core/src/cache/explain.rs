//! Explain cache misses using the earlier execution's fingerprints and file manifest.

use super::{Key, Manifest};
use crate::{config::ArtifactName, store::Execution};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Keep change explanations readable in CLI output; the complete path list remains in Changes.
const MAX_SUMMARY_PATHS: usize = 10;

/// Why the current key differs from an earlier cached result for the same eval and Eval
/// definition.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub since_run_id: crate::types::RunId,
    /// The target's own files: `path` changed, `+path` added, `-path` removed; absent when
    /// not comparable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    /// Dependency Artifacts: `name` changed, `+name` added, `-name` removed; absent when none
    /// changed or not comparable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    pub summary: String,
}

/// Explain a changed key against an earlier cached execution of the same Eval definition:
/// which of the target's files changed, and which dependency Artifacts' fingerprints changed.
pub fn changes(
    previous: &Execution,
    target: &ArtifactName,
    current: &Key,
    manifest: Option<&Manifest>,
) -> Changes {
    let mut parts = Vec::new();
    let mut files = None;
    if previous.fingerprints.get(target) != current.fingerprints.get(target) {
        match previous.manifest.as_ref().zip(manifest) {
            Some((old, new)) if old.inputs == new.inputs => files = Some(Vec::new()),
            Some((old, new)) => {
                files = old
                    .files
                    .as_ref()
                    .zip(new.files.as_ref())
                    .map(|(old, new)| diff(old, new));
                match &files {
                    Some(files) if !files.is_empty() => {
                        let shown = files
                            .iter()
                            .take(MAX_SUMMARY_PATHS)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        parts.push(match files.len() {
                            0..=MAX_SUMMARY_PATHS => format!("changed: {shown}"),
                            total => {
                                format!("changed: {shown} and {} more", total - MAX_SUMMARY_PATHS)
                            }
                        });
                    }
                    _ => parts.push("inputs changed".to_owned()),
                }
            }
            None => parts.push("fingerprint changed".to_owned()),
        }
    }
    let dependencies = (!previous.fingerprints.is_empty())
        .then(|| {
            let others = |map: &BTreeMap<crate::types::ArtifactName, crate::types::Fingerprint>| {
                map.iter()
                    .filter(|(name, _)| *name != target)
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<BTreeMap<_, _>>()
            };
            diff(
                &others(&previous.fingerprints),
                &others(&current.fingerprints),
            )
        })
        .filter(|dependencies| !dependencies.is_empty());
    parts.extend(dependencies.iter().flatten().map(|dependency| {
        if let Some(id) = dependency.strip_prefix('+') {
            format!("dependency {id} added")
        } else if let Some(id) = dependency.strip_prefix('-') {
            format!("dependency {id} removed")
        } else {
            format!("dependency {dependency} changed")
        }
    }));
    if parts.is_empty() {
        parts.push("fingerprint changed".to_owned());
    }
    Changes {
        since_run_id: previous.provenance.run_id.clone(),
        files,
        dependencies,
        summary: parts.join("; "),
    }
}

fn diff<K: Ord + std::fmt::Display, T: PartialEq>(
    old: &BTreeMap<K, T>,
    new: &BTreeMap<K, T>,
) -> Vec<String> {
    old.keys()
        .chain(new.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|key| match (old.get(key), new.get(key)) {
            (Some(old), Some(new)) if old != new => Some(key.to_string()),
            (None, Some(_)) => Some(format!("+{key}")),
            (Some(_), None) => Some(format!("-{key}")),
            _ => None,
        })
        .collect()
}
