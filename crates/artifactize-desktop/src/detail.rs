//! The Detail of a tree row: the monitor's sections in their order, and saved runtime logs.
//! Human requests open the shared Human Detail ([`crate::human`]); Agent requests read their
//! session ([`crate::transcript`]).
use crate::{App, Failure, status::Mark};
use artifactize::{
    config::ProfileKind,
    monitor::{self, NodeId, Section, Target},
};
use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Profile {
    Runtime,
    Agent,
    Human,
    Dependency,
}

impl From<ProfileKind> for Profile {
    fn from(kind: ProfileKind) -> Self {
        match kind {
            ProfileKind::Runtime => Self::Runtime,
            ProfileKind::Agent => Self::Agent,
            ProfileKind::Human => Self::Human,
            ProfileKind::Dependency => Self::Dependency,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SectionName {
    Outcome,
    Waits,
    What,
    Provenance,
    Technical,
}

impl From<Section> for SectionName {
    fn from(section: Section) -> Self {
        match section {
            Section::Outcome => Self::Outcome,
            Section::Waits => Self::Waits,
            Section::What => Self::What,
            Section::Provenance => Self::Provenance,
            Section::Technical => Self::Technical,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub key: String,
    pub value: String,
}

/// One `Waits for` line: an upstream Artifact, or (`pending`) one of its unfinished evals.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WaitLine {
    pub text: String,
    pub pending: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SectionView {
    pub name: SectionName,
    pub title: String,
    pub fields: Vec<Field>,
    /// The `Waits for` section's lines.
    pub waits: Vec<WaitLine>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub title: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetailView {
    pub run: String,
    pub node: String,
    pub title: String,
    /// One-line outcome: state, elapsed time, profile and dependencies.
    pub summary: String,
    /// The eval's request, when it has one in this Run.
    pub request: Option<String>,
    pub profile: Option<Profile>,
    pub mark: Option<Mark>,
    /// Outcome → Waits for → What → Provenance → Technical; empty sections are left out.
    pub sections: Vec<SectionView>,
    /// Saved runtime stdout/stderr, or why there are none.
    pub evidence: Option<Evidence>,
    /// A running request: its Detail follows live changes.
    pub running: bool,
}

/// The Detail of `node` in the selected Run `run`, from the shown snapshot.
#[tauri::command]
#[specta::specta]
pub fn detail(
    app: tauri::State<'_, App>,
    run: String,
    node: String,
) -> Result<DetailView, Failure> {
    let id: NodeId = node.parse().map_err(Failure::Missing)?;
    let target = Target::from(&id);
    app.browse.with_run(&run, |view, requests| {
        let detail = monitor::detail(view, requests, &target, OffsetDateTime::now_utc());
        let request = match &target {
            Target::Eval(eval) => requests.iter().find(|view| &view.request.eval_id == eval),
            _ => None,
        };
        let sections = Section::ALL
            .into_iter()
            .filter_map(|section| {
                let fields: Vec<_> = detail
                    .fields
                    .iter()
                    .filter(|(key, _)| Section::of(key) == section)
                    .collect();
                if fields.is_empty() {
                    return None;
                }
                let waits = if section == Section::Waits {
                    fields
                        .iter()
                        .flat_map(|(_, value)| value.lines())
                        .map(|line| WaitLine {
                            pending: line.starts_with(' '),
                            text: line.trim_start().to_owned(),
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                Some(SectionView {
                    name: section.into(),
                    title: section.name().into(),
                    fields: fields
                        .into_iter()
                        .filter(|_| section != Section::Waits)
                        .map(|(key, value)| Field {
                            key: (*key).into(),
                            value: value.clone(),
                        })
                        .collect(),
                    waits,
                })
            })
            .collect();
        let evidence = request
            .map(|view| monitor::evidence(&app.launch.state, view))
            .filter(|evidence| !evidence.is_empty())
            .filter(|_| {
                request.is_some_and(|view| view.request.profile.kind() == ProfileKind::Runtime)
            })
            .map(|evidence| Evidence {
                title: evidence.title,
                text: evidence.text,
            });
        DetailView {
            run: view.run.id.to_string(),
            node: id.to_string(),
            title: detail.title.clone(),
            summary: detail.summary.clone(),
            request: request.map(|view| view.request.id.to_string()),
            profile: request.map(|view| view.request.profile.kind().into()),
            mark: request.map(|view| monitor::Status::from(view.request.status()).into()),
            running: request.is_some_and(|view| {
                view.request.status() == artifactize::types::RequestStatus::Running
            }),
            sections,
            evidence,
        }
    })
}
