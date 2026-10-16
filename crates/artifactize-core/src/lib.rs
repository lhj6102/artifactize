//! Pull validation and explicit review execution.

pub mod agent;
pub mod auth;
pub mod broker;
pub mod cache;
pub mod changes;
pub mod config;
pub mod diagnostics;
pub mod graph;
pub mod graph_structure;
pub mod human;
pub mod inspect;
mod json;
pub mod limits;
pub mod llm;
pub mod monitor;
pub mod platform;
pub mod process;
pub mod project;
pub mod query;
pub mod remote;
pub mod repository;
pub mod review;
pub mod runs;
pub mod runtime;
pub mod scope;
mod secret;
pub mod server;
pub mod store;
pub mod task;
pub mod tools;
pub mod types;
pub mod workspace;

/// Links, permissions, processes and the Windows stand-ins for Unix utilities, shared with
/// the integration tests.
#[cfg(test)]
#[path = "../tests/support/os.rs"]
mod test_os;

#[cfg(test)]
mod test_declaration;

/// A saved request, shared with the unit tests of `artifactize-tui`.
#[cfg(test)]
#[path = "../tests/support/request.rs"]
mod test_request;

/// Saved Agent conversation files, shared with the unit tests of `artifactize-tui`.
#[cfg(test)]
#[path = "../tests/support/session_files.rs"]
mod test_session_files;
