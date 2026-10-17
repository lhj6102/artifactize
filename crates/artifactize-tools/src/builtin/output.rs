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
    #[serde(flatten)]
    pub rows: ReadRows,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReadRows {
    pub lines: Vec<Line>,
    pub start_line: usize,
    pub end_line: Option<usize>,
    pub line_count: usize,
    #[serde(flatten)]
    pub page: ReadPage,
}

/// EOF carries the known total; a bounded page carries only its continuation.
#[derive(Debug)]
pub(super) enum ReadPage {
    Complete { total_lines: usize },
    Bounded { next_offset: usize },
}
impl Serialize for ReadPage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut output = serializer.serialize_struct(
            "ReadPage",
            match self {
                Self::Complete { .. } => 3,
                Self::Bounded { .. } => 2,
            },
        )?;
        match self {
            Self::Complete { total_lines } => {
                output.serialize_field("truncated", &false)?;
                output.serialize_field("nextOffset", &Option::<usize>::None)?;
                output.serialize_field("totalLines", total_lines)?;
            }
            Self::Bounded { next_offset } => {
                output.serialize_field("truncated", &true)?;
                output.serialize_field("nextOffset", next_offset)?;
            }
        }
        output.end()
    }
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
    #[serde(flatten)]
    pub page: Page,
}

#[derive(Debug)]
pub(super) enum Page {
    Complete,
    Bounded { next_offset: usize },
}
impl Serialize for Page {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let next = match self {
            Self::Complete => None,
            Self::Bounded { next_offset } => Some(next_offset),
        };
        let mut output = serializer.serialize_struct("Page", 2)?;
        output.serialize_field("truncated", &next.is_some())?;
        output.serialize_field("nextOffset", &next)?;
        output.end()
    }
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
