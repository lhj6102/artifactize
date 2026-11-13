//! Settings: the accounts, keys, stores and limits the CLI configures, through the same library
//! calls (`login`/`logout`, `provider`, `models`, `remote login|logout|status|push`, `limits`).
//! Nothing here stores, validates or reaches a service on its own; secrets never come back to
//! the frontend.
mod claude_agent;
mod codex;
mod limits;
mod providers;
mod remote;
mod sign_in;

pub use claude_agent::*;
pub use codex::*;
pub use limits::*;
pub use providers::*;
pub use remote::*;

/// The Codex and Claude Code sign-ins in progress, if any.
#[derive(Default)]
pub struct Settings {
    codex_sign_ins: sign_in::SignIns<codex::Codex>,
    claude_sign_ins: sign_in::SignIns<claude_agent::ClaudeCode>,
}

impl Settings {
    /// Stop the sign-ins; the returned future ends once they have ended.
    pub(crate) fn close(&self) -> impl std::future::Future<Output = ()> + use<> {
        let (codex, claude) = (self.codex_sign_ins.close(), self.claude_sign_ins.close());
        async move {
            codex.await;
            claude.await;
        }
    }
}
