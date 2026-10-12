//! Protected local credential storage, Codex sign-in, and the remote review store's
//! configuration and token.

pub mod codex;
pub mod remote;
mod storage;
#[cfg(test)]
mod tests;
