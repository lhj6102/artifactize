//! Keep transport failure causes distinct until the MCP response is rendered.
use super::page::CursorError;
use crate::config::ConfigError;
use std::fmt;

#[derive(Debug)]
pub enum ArgumentsError {
    ProjectRelative,
    ProjectType,
    UnknownTool(String),
    Decode(serde_json::Error),
    CursorType,
    Cursor(CursorError),
    ToolOptions(String),
    LogOptions(String),
}

impl fmt::Display for ArgumentsError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProjectRelative => out.write_str("project must be an absolute folder."),
            Self::ProjectType => out.write_str("Invalid arguments: project must be a string."),
            Self::UnknownTool(name) => write!(out, "Unknown tool: {name}"),
            Self::Decode(error) => write!(out, "Invalid arguments: {error}"),
            Self::CursorType => out.write_str("cursor must be a string"),
            Self::Cursor(error) => error.fmt(out),
            Self::ToolOptions(message) | Self::LogOptions(message) => out.write_str(message),
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Arguments(ArgumentsError),
    Config(ConfigError),
    Operation(String),
    RequestOutsideProject,
    Timeout,
    Encode(serde_json::Error),
    Cursor(CursorError),
}

impl fmt::Display for Error {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arguments(error) => error.fmt(out),
            Self::Config(error) => error.fmt(out),
            Self::Operation(message) => out.write_str(message),
            Self::RequestOutsideProject => out.write_str("Request not found in this repository."),
            Self::Timeout => {
                out.write_str("State unavailable within the MCP observation deadline.")
            }
            Self::Encode(error) => error.fmt(out),
            Self::Cursor(error) => error.fmt(out),
        }
    }
}

// The shared operations expose their own user-facing diagnostic when they fail.
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Operation(message)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Encode(error)
    }
}
