//! Portable read-only tools, scoped paths, and pinned no-follow file access.

pub mod builtin;
pub mod files;
pub mod image;
pub mod opener;
pub mod program;
pub mod result;
pub mod schema;
pub mod scope;

pub use result::{Content, ToolResult};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    Read,
    List,
    Glob,
    Grep,
    ViewImage,
}

/// JSON clients represent numbers as IEEE-754 doubles; larger integer counters
/// and offsets cannot round-trip exactly through them.
pub const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;
