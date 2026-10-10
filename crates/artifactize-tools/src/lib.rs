//! Portable read-only tools, scoped paths, and pinned no-follow file access.

pub mod builtin;
pub mod image;
pub mod launch;
pub mod opener;
mod platform;
pub mod result;
pub mod schema;
pub mod scope;

pub use platform::{files, program};

/// Temporary roots, links and executable files, shared with the integration tests.
#[cfg(test)]
#[path = "../tests/support/os.rs"]
mod test_os;

pub use result::{Content, ToolResult};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    Read,
    List,
    Glob,
    Grep,
    ViewImage,
    Section,
    Help,
    Open,
}

/// JSON clients represent numbers as IEEE-754 doubles; larger integer counters
/// and offsets cannot round-trip exactly through them.
pub const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;
