//! Project selection, verification orchestration, and status.

pub mod selection;

mod status;
pub use status::{ArtifactState, Counts, EvalState, StatusView, status};

mod verify;
pub use verify::{VerifyOptions, verify};
