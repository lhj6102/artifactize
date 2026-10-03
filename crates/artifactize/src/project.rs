//! Project selection, verification orchestration, and status.

pub mod selection;

mod verify;
pub use verify::{VerifyOptions, verify};
