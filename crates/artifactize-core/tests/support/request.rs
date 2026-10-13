//! A saved Human request as the screens and stores read it, for the unit tests of both crates.
use crate::store::RequestView;
use serde_json::{Value, json};

pub(crate) fn request(eval: &str, status: &str, extra: Value) -> RequestView {
    let mut value = json!({
        "id":format!("run-1-{}", eval.replace('/', "-")),
        "runId":"run-1",
        "evalId":eval,
        "target":eval.split('/').next(),
        "title":"Title",
        "profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]},
        "requestedProfile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]},
        "evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        "payload":{"instruction":"Check."},
        "references":{},
        "deps":[],
        "status":status,
        "result": if matches!(status, "GREEN" | "RED") { json!({"verdict":status}) } else { Value::Null },
        "error": if status == "ERROR" { json!("fixture error") } else { Value::Null },
        "completedAt": if matches!(status, "GREEN" | "RED" | "ERROR") { json!("2026-01-01T00:00:01Z") } else { Value::Null },
        "createdAt":"2026-01-01T00:00:00Z",
        "cwd":"/repo",
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    RequestView {
        request: serde_json::from_value(value).unwrap(),
        claim: None,
        execution: None,
        definition: None,
    }
}
