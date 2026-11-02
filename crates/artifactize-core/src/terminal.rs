//! One independent PTY host per Git worktree. Closing an attachment never owns the host.
mod client;
mod host;
mod metadata;
mod screen;
mod shutdown;
pub(crate) use shutdown::stop_for_removal;
#[cfg(test)]
mod tests;
mod wire;

use crate::types::SessionId;
pub use client::{Attachment, attach};
pub use host::serve;
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
    #[serde(serialize_with = "crate::platform::serialize_program")]
    #[cfg_attr(feature = "desktop-types", specta(type = String))]
    pub program: String,
    pub args: Vec<String>,
}

/// A known session and a latest-session choice are distinct states, never a nullable ID
/// combined with a flag that can disagree with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ResumeSession {
    Exact {
        #[cfg_attr(feature = "desktop-types", specta(type = String))]
        id: SessionId,
    },
    Latest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "desktop-types", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RestoreOffer {
    Shell {
        #[serde(with = "crate::platform::path_serde::option")]
        #[cfg_attr(feature = "desktop-types", specta(type = Option<String>))]
        cwd: Option<PathBuf>,
    },
    Command {
        #[serde(with = "crate::platform::path_serde")]
        #[cfg_attr(feature = "desktop-types", specta(type = String))]
        cwd: PathBuf,
        command: TerminalCommand,
    },
    Harness {
        #[serde(with = "crate::platform::path_serde")]
        #[cfg_attr(feature = "desktop-types", specta(type = String))]
        cwd: PathBuf,
        command: TerminalCommand,
        harness: Harness,
        session: ResumeSession,
        // The exact argv both the client displays and the confirmed resume executes.
        resume: TerminalCommand,
    },
}

impl RestoreOffer {
    pub(crate) fn confirmed(
        &self,
        choice: RestoreChoice,
    ) -> Result<(PathBuf, TerminalCommand), String> {
        match (self, choice) {
            (
                Self::Command { cwd, command } | Self::Harness { cwd, command, .. },
                RestoreChoice::Rerun,
            ) => Ok((cwd.clone(), command.clone())),
            (Self::Harness { cwd, resume, .. }, RestoreChoice::Resume) => {
                Ok((cwd.clone(), resume.clone()))
            }
            _ => Err("That restore command is unknown".into()),
        }
    }
}

impl Harness {
    pub(crate) fn resume(&self, session: &ResumeSession) -> TerminalCommand {
        match self {
            Self::ClaudeCode => TerminalCommand {
                program: "claude".into(),
                args: match session {
                    ResumeSession::Exact { id } => vec!["--resume".into(), id.to_string()],
                    ResumeSession::Latest => vec!["--continue".into()],
                },
            },
            Self::Codex => TerminalCommand {
                program: "codex".into(),
                args: vec![
                    "resume".into(),
                    match session {
                        ResumeSession::Exact { id } => id.to_string(),
                        ResumeSession::Latest => "--last".into(),
                    },
                ],
            },
        }
    }
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

pub(crate) fn dimensions(cols: u16, rows: u16) -> std::io::Result<crate::platform::terminal::Size> {
    if cols == 0 || rows == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Terminal dimensions must be nonzero",
        ));
    }
    Ok(crate::platform::terminal::Size {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })
}
