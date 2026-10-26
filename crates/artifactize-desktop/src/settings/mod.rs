//! Settings: the accounts, keys, stores and limits the CLI configures, through the same library
//! calls (`login`/`logout`, `provider`, `models`, `remote login|logout|status|push`, `limits`).
//! Nothing here stores, validates or reaches a service on its own; secrets never come back to
//! the frontend.
mod codex;
mod limits;
mod providers;
mod remote;

pub use codex::*;
pub use limits::*;
pub use providers::*;
pub use remote::*;

/// The Codex sign-in in progress, if any.
#[derive(Default)]
pub struct Settings {
    sign_ins: codex::SignIns,
}

impl Settings {
    /// Stop the Codex sign-ins; the returned future ends once they have ended.
    pub(crate) fn close(&self) -> impl std::future::Future<Output = ()> + use<> {
        self.sign_ins.close()
    }
}
