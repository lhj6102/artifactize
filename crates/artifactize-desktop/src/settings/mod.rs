//! Settings: the accounts, keys, stores and limits the CLI configures, through the same library
//! calls (`login`/`logout codex`, `models`, `remote login|logout|status|push`, `limits`).
//! Nothing here stores, validates or reaches a service on its own; secrets never come back to
//! the frontend.
mod codex;
mod keys;
mod limits;
mod remote;

pub use codex::*;
pub use keys::*;
pub use limits::*;
pub use remote::*;

/// The Codex sign-in in progress, if any.
#[derive(Default)]
pub struct Settings {
    sign_ins: codex::SignIns,
}
