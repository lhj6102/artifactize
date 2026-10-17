//! The Detail of a tree row: the monitor's sections in their order, and saved runtime logs.
//! Human requests open the shared Human Detail ([`crate::human`]); Agent requests read their
//! session ([`crate::transcript`]).
use crate::ids::{NodeKey, RunKey};
use crate::{App, Failure, status::Mark};
use artifactize_core::types::RunId;
use artifactize_core::{
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
}

impl From<ProfileKind> for Profile {
    fn from(kind: ProfileKind) -> Self {
        match kind {
            ProfileKind::Runtime => Self::Runtime,
            ProfileKind::Agent => Self::Agent,
            ProfileKind::Human => Self::Human,
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

/// One entry of an eval's result, the verdict left out: a text, or a list of findings.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResultEntry {
    pub key: String,
    pub text: Option<String>,
    pub items: Vec<ResultItem>,
}

/// A finding: its rule and place as the head (`item · where`), then its other fields.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResultItem {
    pub head: Option<String>,
    pub fields: Vec<Field>,
}

/// An eval at a glance, from the library's [`monitor::Card`].
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    pub instruction: String,
    /// `None` until the eval has a result.
    pub result: Option<Vec<ResultEntry>>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub reused: bool,
    pub took: Option<String>,
    pub tokens: Option<u32>,
    pub model: Option<String>,
}

fn text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn result_item(value: &serde_json::Value) -> ResultItem {
    let Some(object) = value.as_object() else {
        return ResultItem {
            head: Some(text(value)),
            fields: Vec::new(),
        };
    };
    let head: Vec<_> = ["item", "where"]
        .into_iter()
        .filter_map(|key| object.get(key).map(text))
        .collect();
    ResultItem {
        head: (!head.is_empty()).then(|| head.join(" · ")),
        fields: object
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "item" | "where"))
            .map(|(key, value)| Field {
                key: key.clone(),
                value: text(value),
            })
            .collect(),
    }
}

impl From<monitor::Card> for CardView {
    fn from(card: monitor::Card) -> Self {
        Self {
            instruction: card.instruction,
            result: card.result.map(|entries| {
                entries
                    .iter()
                    .map(|(key, value)| match value {
                        serde_json::Value::Array(items) => ResultEntry {
                            key: key.clone(),
                            text: None,
                            items: items.iter().map(result_item).collect(),
                        },
                        other => ResultEntry {
                            key: key.clone(),
                            text: Some(text(other)),
                            items: Vec::new(),
                        },
                    })
                    .collect()
            }),
            exit_code: card.exit_code,
            error: card.error,
            reused: card.reused,
            took: card.took,
            tokens: card
                .tokens
                .map(|tokens| u32::try_from(tokens).unwrap_or(u32::MAX)),
            model: card.model,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetailView {
    pub run: RunKey,
    pub node: NodeKey,
    pub title: String,
    /// One-line outcome: state, elapsed time, profile and dependencies.
    pub summary: String,
    /// The eval's request, when it has one in this Run.
    pub request: Option<crate::ids::RequestKey>,
    pub profile: Option<Profile>,
    pub mark: Option<Mark>,
    /// Outcome → Waits for → What → Provenance → Technical; empty sections are left out.
    pub sections: Vec<SectionView>,
    /// Saved runtime stdout/stderr, or why there are none.
    pub evidence: Option<Evidence>,
    /// An eval's status line, instruction and result; `None` for an Artifact or the Run.
    pub card: Option<CardView>,
    /// A running request: its Detail follows live changes.
    pub running: bool,
}

/// The Detail of `node` in the selected Run `run`, from the shown snapshot.
/// The Detail of `node` in Run `run` with its `requests`: the monitor's model, laid out.
pub fn view(
    state: &std::path::Path,
    run: &artifactize_core::store::RunView,
    requests: &[artifactize_core::store::RequestView],
    id: &NodeId,
    now: OffsetDateTime,
) -> DetailView {
    let target = Target::from(id);
    let detail = monitor::detail(run, requests, &target, now);
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
        .filter(|view| view.request.profile.kind() == ProfileKind::Runtime)
        .map(|view| monitor::evidence(state, view))
        .filter(|evidence| !evidence.is_empty())
        .map(|evidence| Evidence {
            title: evidence.title,
            text: evidence.text,
        });
    DetailView {
        run: run.run.id.clone().into(),
        node: id.clone().into(),
        title: detail.title.clone(),
        summary: detail.summary.clone(),
        request: request.map(|view| view.request.id.clone().into()),
        profile: request.map(|view| view.request.profile.kind().into()),
        mark: request.map(|view| monitor::Status::from(view.request.status()).into()),
        running: request.is_some_and(|view| {
            view.request.status() == artifactize_core::types::RequestStatus::Running
        }),
        sections,
        evidence,
        card: request.map(|view| monitor::card(view, now).into()),
    }
}

/// The Detail of `node` in the selected Run `run`, from the shown snapshot.
#[tauri::command]
#[specta::specta]
pub fn detail(
    app: tauri::State<'_, App>,
    run: RunKey,
    node: NodeKey,
) -> Result<DetailView, Failure> {
    let id: NodeId = node.into_inner();
    let run: RunId = run.into_inner();
    app.browse.with_run(&run, |run, requests| {
        view(
            &app.launch.state,
            run,
            requests,
            &id,
            OffsetDateTime::now_utc(),
        )
    })
}
