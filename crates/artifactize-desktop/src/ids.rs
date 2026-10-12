//! Numbers the frontend and the backend agree on to name a running job, a sign-in or an open
//! transcript. Each kind is its own type, so one cannot be passed for another.
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::{AtomicU32, Ordering};

macro_rules! numbered {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
        #[serde(transparent)]
        pub struct $name(u32);

    };
}

numbered!(
    /// A cancellable job of one Human action, numbered by the frontend.
    JobId
);
numbered!(
    /// A Codex sign-in in progress.
    SignInId
);
numbered!(
    /// An open transcript reader.
    TranscriptHandle
);

/// The next number of `counter`, which hands out each one once.
fn allocate(counter: &AtomicU32) -> u32 {
    counter.fetch_add(1, Ordering::Relaxed)
}

impl SignInId {
    pub(crate) fn allocate(counter: &AtomicU32) -> Self {
        Self(allocate(counter))
    }
}

impl TranscriptHandle {
    pub(crate) fn allocate(counter: &AtomicU32) -> Self {
        Self(allocate(counter))
    }

    /// The number itself, for the library, which numbers readers by `u64`.
    pub(crate) fn number(self) -> u64 {
        u64::from(self.0)
    }
}
