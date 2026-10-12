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

macro_rules! key {
    ($(#[$meta:meta])* $name:ident, $inner:ty) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name($inner);

        impl From<$inner> for $name {
            fn from(id: $inner) -> Self {
                Self(id)
            }
        }

        impl From<$name> for String {
            fn from(key: $name) -> Self {
                key.0.to_string()
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                text.parse().map(Self)
            }
        }

        impl $name {
            pub fn into_inner(self) -> $inner {
                self.0
            }
        }

        /// Text on the wire, as the library's own identifier is.
        impl Type for $name {
            fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
                String::definition(types)
            }
        }
    };
}

key!(
    /// A request id as an IPC argument, checked where it arrives.
    RequestKey,
    artifactize_core::types::RequestId
);
key!(
    /// A Run id as an IPC argument, checked where it arrives.
    RunKey,
    artifactize_core::types::RunId
);
key!(
    /// A tree node id as an IPC argument, checked where it arrives.
    NodeKey,
    artifactize_core::monitor::NodeId
);
key!(
    /// A saved session id in IPC, which change hints name.
    SessionKey,
    artifactize_core::types::SessionId
);
