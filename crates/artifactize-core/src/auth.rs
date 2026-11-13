//! Protected local credential storage, Codex sign-in, Claude Code's own sign-in (run, never
//! read), saved provider API keys and custom providers, and the remote review store's
//! configuration and token.

pub mod claude_agent;
pub mod codex;
pub mod providers;
pub mod remote;
mod storage;
#[cfg(test)]
mod tests;
