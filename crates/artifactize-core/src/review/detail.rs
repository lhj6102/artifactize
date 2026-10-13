//! The facts of one Human review that every interface shows alike: where the request stands
//! for a reviewer, its one-line metadata, the Technical section and its outcome.
use crate::{
    monitor::{duration, plain},
    store::{Request, RequestView},
    types::RequestStatus,
};
use serde_json::Value;
use time::OffsetDateTime;

/// Where a Human request stands for one reviewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Claim,
    Review,
    Completed,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Claim => "CLAIM",
            Stage::Review => "REVIEW (yours)",
            Stage::Completed => "completed",
        }
    }
}

/// Status, claim and waiting time in one row; errors keep their first line.
pub fn meta(view: &RequestView, reviewer: &str, now: OffsetDateTime) -> String {
    let request = &view.request;
    let waiting = request.status() == RequestStatus::WaitingHuman;
    let mut parts = vec![request.status().to_string()];
    if let Some(error) = &request.error() {
        let first = error.lines().next().unwrap_or_default();
        parts.push(match &request.error_code() {
            Some(code) => format!("[{code}] {first}"),
            None => first.to_owned(),
        });
    }
    match &view.claim {
        None if waiting => parts.push("unclaimed".into()),
        None => {}
        Some(claim) if claim.reviewer == reviewer => {
            parts.push(format!("claimed by you ({reviewer})"))
        }
        Some(claim) => parts.push(format!("claimed by {} · read-only", claim.reviewer)),
    }
    if waiting {
        parts.push(format!(
            "waiting {}",
            duration((now - request.created_at.time()).whole_seconds())
        ));
    }
    plain(&parts.join(" · ")).into_owned()
}

/// A follower's actions go to the request that owns the shared execution.
pub fn shared(view: &RequestView) -> Option<String> {
    let source = view
        .execution
        .as_ref()
        .map(|execution| &execution.provenance)?;
    (source.request_id != view.request.id).then(|| {
        let text = format!(
            "actions go to {} in {}",
            source.request_id,
            crate::platform::path_text(&source.repo_path)
        );
        plain(&text).into_owned()
    })
}

fn schema(schema: Option<&Value>) -> String {
    schema.map_or("none (no owner fields)".into(), |schema| {
        serde_json::to_string_pretty(schema).unwrap_or_default()
    })
}

/// The folded Technical section: identifiers, the repository and the raw owner schemas.
pub fn technical(view: &RequestView, reviewer: &str) -> Vec<(&'static str, String)> {
    let request = &view.request;
    let definition = request.human_definition.as_ref();
    let declaration = definition
        .and_then(|definition| definition.eval.declaration.value())
        .map(|declaration| serde_json::to_value(declaration).expect("saved declaration is JSON"));
    let status = match (&request.error(), &request.error_code()) {
        (Some(error), Some(code)) => format!("{} [{code}] {error}", request.status()),
        (Some(error), None) => format!("{} {error}", request.status()),
        _ => request.status().to_string(),
    };
    let claim = match &view.claim {
        None if request.status() == RequestStatus::WaitingHuman => {
            format!("unclaimed; c claims it for {reviewer}")
        }
        None => "none".into(),
        Some(claim) if claim.reviewer == reviewer => {
            format!("claimed by you ({reviewer}) at {}", claim.claimed_at)
        }
        Some(claim) => format!(
            "claimed by {} at {}; read-only",
            claim.reviewer, claim.claimed_at
        ),
    };
    let mut fields = vec![
        ("Request", request.id.to_string()),
        ("Run", request.run_id.to_string()),
        (
            "Repository",
            definition
                .and_then(|definition| definition.repo.value())
                .map(|repo| crate::platform::path_text(repo))
                .unwrap_or_else(|| "-".into()),
        ),
        ("Status", status),
        ("Claim", claim),
    ];
    if let Some(shared) = shared(view) {
        fields.push(("Shared", shared));
    }
    fields.push(("Created", request.created_at.to_string()));
    let owner = |key| {
        schema(
            declaration
                .as_ref()
                .and_then(|declaration| declaration.get(key)),
        )
    };
    fields.push(("GREEN fields", owner("passSchema")));
    fields.push(("RED fields", owner("failSchema")));
    fields
}

/// The stage of a request that is `settled`, or open and claimed by this reviewer (`mine`).
pub fn stage(settled: bool, mine: bool) -> Stage {
    if settled {
        Stage::Completed
    } else if mine {
        Stage::Review
    } else {
        Stage::Claim
    }
}

/// A settled request's outcome as the completed Detail shows it: its result as pretty JSON, or
/// its error.
pub fn outcome_text(request: &Request) -> String {
    request.result().map_or_else(
        || request.error().map(str::to_owned).unwrap_or_default(),
        |result| serde_json::to_string_pretty(result).unwrap_or_default(),
    )
}
