//! Read-only state queries and result projections.

use serde_json::{Value, json};

use crate::store::RunView;

/// Compact requester output never copies process stdout/stderr or owner payloads.
pub fn requester_run(view: &RunView) -> Value {
    let reference = |request_id: Option<&str>| {
        json!({
            "runId":view.run.id,"requestId":request_id,"stateDir":view.run.state_dir,
        })
    };
    json!({
        "id":view.run.id,
        "status":view.run.status,
        "reference":reference(None),
        "error":view.run.error,
        "validation":view.run.validation,
        "requests":view.requests.iter().map(|request| json!({
            "id":request.id,"runId":request.run_id,"criticId":request.critic_id,
            "target":request.target,"status":request.status,"profile":request.profile,
            "reference":reference(Some(&request.id)),"error":request.error,
            "errorCode":request.error_code,"blockedReason":request.blocked_reason,
            "result":request.result.as_ref().map(|_| json!({"requestId":request.id})),
        })).collect::<Vec<_>>(),
        "results":view.requests.iter().filter_map(|request| request.result.as_ref().map(|result| json!({
            "verdict":result["verdict"],"reference":reference(Some(&request.id)),"profile":request.profile,
        }))).collect::<Vec<_>>(),
    })
}
