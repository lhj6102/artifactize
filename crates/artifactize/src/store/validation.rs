//! Validate state-appropriate data before writes, without migrating or rejecting old reads.

use super::ExecutionResult;
use super::{Execution, Request, Run};
use crate::types::{ExecutionStatus, RequestStatus, RunStatus};

fn result(
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
impl Run {
    pub(crate) fn validate(&self) -> Result<(), String> {
        match self.status {
            RunStatus::Running if self.completed_at.is_none() && self.error.is_none() => Ok(()),
            RunStatus::Running => {
                Err("A running Run cannot be completed or carry an error.".into())
            }
            RunStatus::Green | RunStatus::Red
                if self.completed_at.is_some() && self.error.is_none() =>
            {
                Ok(())
            }
            RunStatus::Green | RunStatus::Red => Err(
                "A completed verdict Run needs a completion time and cannot carry an error.".into(),
            ),
            RunStatus::Error if self.completed_at.is_some() => Ok(()),
            RunStatus::Incomplete if self.completed_at.is_some() => Ok(()),
            _ => Err("A finished Run needs its completion time.".into()),
        }
    }
}
impl Request {
    pub(crate) fn validate(&self) -> Result<(), String> {
        result(
            self.status,
            self.result.as_ref(),
            self.error.as_deref(),
            self.error_code.as_deref(),
        )?;
        if matches!(
            self.status,
            RequestStatus::Green | RequestStatus::Red | RequestStatus::Error
        ) && self.completed_at.is_none()
        {
            return Err("A finished request needs its completion time.".into());
        }
        if matches!(
            self.status,
            RequestStatus::Running | RequestStatus::WaitingHuman
        ) && self.completed_at.is_some()
        {
            return Err("An active request cannot have a completion time.".into());
        }
        Ok(())
    }
}
impl Execution {
    pub(crate) fn validate(&self) -> Result<(), String> {
        result(
            self.status.into(),
            self.result.as_ref(),
            self.error.as_deref(),
            self.error_code.as_deref(),
        )?;
        match self.status {
            ExecutionStatus::Running | ExecutionStatus::WaitingHuman
                if self.completed_at.is_none() =>
            {
                Ok(())
            }
            ExecutionStatus::Green | ExecutionStatus::Red | ExecutionStatus::Error
                if self.completed_at.is_some() =>
            {
                Ok(())
            }
            _ => Err("Execution state and completion time disagree.".into()),
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
