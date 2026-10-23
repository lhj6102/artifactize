//! Settings: the accounts, keys and stores the CLI configures, through the same library calls
//! (`login`/`logout`, `provider`, `models`, `remote login|logout|status|push`). Nothing here stores,
//! validates or reaches a service on its own; secrets never come back to the frontend.
mod codex;
mod providers;
mod remote;

pub use codex::*;
pub use providers::*;
pub use remote::*;

/// The Codex sign-in in progress, if any.
#[derive(Default)]
pub struct Settings {
    sign_ins: codex::SignIns,
}
