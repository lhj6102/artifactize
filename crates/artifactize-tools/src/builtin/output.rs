//! Typed builtin results. Only the content boundary converts them to dynamic JSON.
use serde::Serialize;

use crate::scope::ArtifactId;

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(super) enum Output {
    Read(Read),
    List(List),
    Glob(Glob),
    Grep(Grep),
}

/// A listed entry always has a kind; only a mount carries its target Artifact.
#[derive(Debug, Serialize)]
pub(super) struct Entry {
    #[serde(flatten)]
    pub kind: EntryKind,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(super) enum EntryKind {
    File,
    Directory,
    Symlink,
    Other,
    Mount {
        #[serde(rename = "artifactId")]
        artifact_id: ArtifactId,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Read {
    pub artifact_id: ArtifactId,
    pub resolved_artifact_id: ArtifactId,
    pub path: String,
    pub lines: Vec<Line>,
    pub start_line: usize,
    pub end_line: Option<usize>,
    pub line_count: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_lines: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(super) struct Line {
    pub number: usize,
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct List {
    pub artifact_id: ArtifactId,
    pub path: String,
    pub entries: Vec<Entry>,
    pub total_entries: usize,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(super) struct Glob {
    pub path: String,
    pub files: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct Grep {
    pub path: String,
    pub matches: Vec<GrepMatch>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct GrepMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entry_json_keeps_mount_identity_only_on_mounts() {
        let entry = |kind| Entry {
            kind,
            name: "notes".into(),
            path: "docs/notes".into(),
        };
        assert_eq!(
            serde_json::to_value(entry(EntryKind::File)).unwrap(),
            json!({"kind":"file","name":"notes","path":"docs/notes"})
        );
        assert_eq!(
            serde_json::to_value(entry(EntryKind::Mount {
                artifact_id: ArtifactId::new("target").unwrap(),
            }))
            .unwrap(),
            json!({"artifactId":"target","kind":"mount","name":"notes","path":"docs/notes"})
        );
    }
}
