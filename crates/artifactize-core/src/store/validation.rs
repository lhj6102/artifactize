//! Parse Run, request and execution lifecycle data at the saved-record edge.
//! Earlier versions validated every write; contradictory rows indicate corruption. They
//! fail individually as unreadable records, never as reusable evidence or a whole listing.

use super::ExecutionResult;
use crate::types::RequestStatus;

pub(super) fn result(
    status: RequestStatus,
    result: Option<&ExecutionResult>,
    error: Option<&str>,
    error_code: Option<&str>,
) -> Result<(), String> {
    match status {
        RequestStatus::Provisional
            if result.is_some_and(|result| result.verdict() == crate::runtime::Verdict::Green)
                && error.is_none()
                && error_code.is_none() =>
        {
            Ok(())
        }
        RequestStatus::Provisional => Err("A provisional result needs GREEN evidence.".into()),
        RequestStatus::Green | RequestStatus::Red
            if result.is_some_and(|result| result.verdict().as_str() == status.as_str())
                && error.is_none()
                && error_code.is_none() =>
        {
            Ok(())
        }
        RequestStatus::Green | RequestStatus::Red => {
            Err("A verdict needs its matching result and cannot carry an operational error.".into())
        }
        RequestStatus::Error if result.is_none() && error.is_some() => Ok(()),
        RequestStatus::Error => {
            Err("An operational error needs an error and cannot carry a verdict result.".into())
        }
        _ if result.is_none() && error.is_none() && error_code.is_none() => Ok(()),
        _ => {
            Err("A pending request or execution cannot carry a result or operational error.".into())
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::store::Request;
    use serde_json::{Value, json};

    fn wire(status: &str) -> Value {
        let request = crate::monitor::tests::request("app/check", status, json!({})).request;
        serde_json::to_value(request).unwrap()
    }

    #[test]
    fn saved_request_verdict_and_error_data_cannot_contradict_the_state() {
        for (status, opposite) in [("GREEN", "RED"), ("RED", "GREEN")] {
            let valid = wire(status);
            assert!(serde_json::from_value::<Request>(valid.clone()).is_ok());
            for (field, value) in [
                ("result", Value::Null),
                ("result", json!({"verdict":opposite})),
                ("result", json!({"verdict":"ERROR"})),
                ("error", json!("failed")),
                ("errorCode", json!("SPAWN_FAILED")),
            ] {
                let mut invalid = valid.clone();
                invalid[field] = value;
                assert!(
                    serde_json::from_value::<Request>(invalid).is_err(),
                    "{status}: {field}"
                );
            }
        }
        let valid = wire("ERROR");
        let mut coded = valid.clone();
        coded["errorCode"] = json!("SPAWN_FAILED");
        assert!(serde_json::from_value::<Request>(coded).is_ok());
        for (field, value) in [
            ("error", Value::Null),
            ("result", json!({"verdict":"GREEN"})),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(
                serde_json::from_value::<Request>(invalid).is_err(),
                "ERROR: {field}"
            );
        }
        for status in ["QUEUED", "RUNNING", "WAITING_HUMAN", "BLOCKED"] {
            let valid = wire(status);
            assert!(serde_json::from_value::<Request>(valid.clone()).is_ok());
            for (field, value) in [
                ("result", json!({"verdict":"GREEN"})),
                ("error", json!("failed")),
                ("errorCode", json!("SPAWN_FAILED")),
            ] {
                let mut invalid = valid.clone();
                invalid[field] = value;
                assert!(
                    serde_json::from_value::<Request>(invalid).is_err(),
                    "{status}: {field}"
                );
            }
        }
    }
}
