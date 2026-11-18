//! Semantic, minimal session presentation. Stored events remain lossless; this view keeps only
//! prose and bounded activity headers, never tool output, arguments, reasoning or media payloads.

mod candidate;
mod fold;
mod render;

pub use fold::Transcript;
pub use render::{Activity, Group, State, markdown};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlockId(pub usize);

/// Disk-document edits use stable slots. A group update replaces only that group's text;
/// prose strings leave this layer immediately and are retained by the existing private pages.
pub struct Patch {
    pub id: BlockId,
    pub text: String,
}
