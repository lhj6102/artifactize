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
    use super::*;
    use serde_json::json;
    #[test]
    fn verdict_and_error_data_cannot_contradict_the_state() {
        for status in ["GREEN", "RED"] {
            assert!(result(status.parse().unwrap(), None, None, None).is_err());
            assert!(
                result(
                    status.parse().unwrap(),
                    Some(&json!({"verdict":status}).try_into().unwrap()),
                    None,
                    None
                )
                .is_ok()
            );
            assert!(ExecutionResult::try_from(json!({"verdict":"ERROR"})).is_err());
            let opposite = if status == "GREEN" { "RED" } else { "GREEN" };
            assert!(
                result(
                    status.parse().unwrap(),
                    Some(&json!({"verdict":opposite}).try_into().unwrap()),
                    None,
                    None
                )
                .is_err()
            );
            assert!(
                result(
                    status.parse().unwrap(),
                    Some(&json!({"verdict":status}).try_into().unwrap()),
                    Some("failed"),
                    None
                )
                .is_err()
            );
        }
        assert!(
            result(
                RequestStatus::Error,
                None,
                Some("failed"),
                Some("SPAWN_FAILED")
            )
            .is_ok()
        );
        assert!(
            result(
                RequestStatus::Error,
                Some(&json!({"verdict":"GREEN"}).try_into().unwrap()),
                Some("failed"),
                None
            )
            .is_err()
        );
        assert!(result(RequestStatus::Error, None, None, None).is_err());
        for status in ["QUEUED", "RUNNING", "WAITING_HUMAN", "BLOCKED"] {
            assert!(result(status.parse().unwrap(), None, None, None).is_ok());
            assert!(
                result(
                    status.parse().unwrap(),
                    Some(&json!({"verdict":"GREEN"}).try_into().unwrap()),
                    None,
                    None
                )
                .is_err()
            );
        }
    }
}
