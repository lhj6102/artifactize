//! Pull validation and explicit review execution.

pub mod agent;
pub mod auth;
pub mod broker;
pub mod cache;
pub mod changes;
pub mod cli;
pub mod config;
pub mod diagnostics;
pub mod graph;
pub mod human;
pub mod limits;
pub mod llm;
pub mod monitor;
mod platform;
pub mod process;
pub mod project;
pub mod query;
pub mod remote;
pub mod repository;
pub mod review;
pub mod runtime;
pub mod scope;
pub mod server;
pub mod store;
pub mod tools;
pub mod types;
pub mod workspace;

/// Links, permissions, processes and the Windows stand-ins for Unix utilities, shared with
/// the integration tests.
#[cfg(test)]
#[path = "../tests/support/os.rs"]
mod test_os;
