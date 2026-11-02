//! One independent PTY host per Git worktree. Closing an attachment never owns the host.
mod client;
mod host;
mod metadata;
mod restore;
mod screen;
#[cfg(test)]
mod tests;
mod wire;

pub use client::{Attachment, attach};
pub use host::serve;
pub use restore::run_confirmed;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Harness {
    ClaudeCode,
    Codex,
}

/// An argv, not shell source. Confirmed restores execute this directly in `cwd`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TerminalCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RestoreOffer {
    #[serde(with = "crate::platform::path_serde::option")]
    #[cfg_attr(feature = "desktop-types", specta(type = Option<String>))]
    pub cwd: Option<PathBuf>,
    pub command: Option<TerminalCommand>,
    pub harness: Option<Harness>,
    pub session_id: Option<String>,
    pub resume: Option<TerminalCommand>,
    pub latest_session: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum RestoreChoice {
    FreshShell,
    Rerun,
    Resume,
}

/// Bytes are kept intact across UTF-8 and escape-sequence boundaries. Feed `data` to the
/// terminal as Uint8Array. A snapshot resets the emulator, including both screen buffers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TerminalFrame {
    Snapshot {
        data: Vec<u8>,
        cols: u16,
        rows: u16,
    },
    Output {
        data: Vec<u8>,
    },
    Ended {
        exit_code: Option<u32>,
    },
    RestoreOffer {
        offer: RestoreOffer,
    },
    NotPersistent,
    AgentMoved {
        #[serde(with = "crate::platform::path_serde")]
        #[cfg_attr(feature = "desktop-types", specta(type = String))]
        worktree: PathBuf,
    },
}

pub(crate) fn dimensions(cols: u16, rows: u16) -> std::io::Result<portable_pty::PtySize> {
    if cols == 0 || rows == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Terminal dimensions must be nonzero",
        ));
    }
    Ok(portable_pty::PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}
