//! Fingerprint acquisition, reuse keys and cache history.

mod acquire;
mod content;
mod explain;
mod key;
mod scratch;

pub use crate::store::history::{Entry, list, remove, show};
use crate::store::{Execution, Request};
pub use acquire::{Manifest, Parallelism, PreparedFingerprint, prepare, recheck};
pub(crate) use acquire::{prepare_each, recheck_observations, validate_file_inputs};
pub(crate) use content::HASH_BUFFER_BYTES;
pub use content::IgnorePatterns;
pub use explain::{Changes, changes};
pub use key::{
    Key, Unkeyed, dependencies, eval_definition_hash, eval_key, eval_keys, key, key_with_kinds,
};
pub(crate) use key::{fingerprint_targets, live_input, parts_key};

/// Give a request another execution's result. A request that waited for this very
/// execution (a live owner it joined, or the Human wait it followed) joined it; any other
/// found a completed record.
pub fn reuse(request: &mut Request, execution: &Execution, completed_at: crate::types::Timestamp) {
    request.joined = request.execution_id.as_ref() == Some(&execution.id);
    request.state = match execution.state.clone() {
        crate::store::ExecutionState::Completed { result, .. } => {
            crate::store::RequestState::completed(result, completed_at)
        }
        crate::store::ExecutionState::Failed { error, code, .. } => {
            crate::store::RequestState::failed(error, code, completed_at)
        }
        state => state.into(),
    };
    request.execution_id = Some(execution.id.clone());
    request.profile = execution.profile.clone();
    request.options = execution.options.clone();
    request.provenance = Some(execution.provenance.clone());
    request.usage = None;
    request.reused_usage = execution.usage.clone();
    request.producer = execution.producer.clone();
    request.session = execution
        .producer
        .as_ref()
        .and_then(|producer| producer.session.clone());
    request.reviewer = execution.reviewer.clone();
    request.origin = execution.origin.clone();
    request.blocked_reason = None;
}

#[cfg(test)]
mod tests;
