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
numbered!(
    /// A shape of the selected Run's graph, its Artifacts and edges whatever their lights,
    /// numbered by the backend as it reads one.
    ShapeKey
);

numbered!(
    /// A scan of a branch's Current, numbered by the backend as it starts one: a later scan
    /// has a higher number.
    ScanId
);

impl ScanId {
    /// The scan after the last one `counter` handed out.
    pub(crate) fn allocate(counter: &AtomicU32) -> Self {
        Self(allocate(counter) + 1)
    }
}

impl ShapeKey {
    /// Before the first shape.
    pub(crate) const NONE: Self = Self(0);

    /// The next shape after this one, which this one becomes.
    pub(crate) fn next(&mut self) -> Self {
        self.0 = self.0.checked_add(1).expect("graph shapes exhausted");
        *self
    }
}

impl Default for ShapeKey {
    fn default() -> Self {
        Self::NONE
    }
}

/// The library's Tool log run identity, serialized as decimal text without losing its type.
/// This is an outbound event key, never an IPC argument to parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(into = "String")]
pub struct ToolRunKey(artifactize_core::review::log::RunNo);

impl From<artifactize_core::review::log::RunNo> for ToolRunKey {
    fn from(run: artifactize_core::review::log::RunNo) -> Self {
        Self(run)
    }
}

impl From<ToolRunKey> for String {
    fn from(key: ToolRunKey) -> Self {
        key.0.get().to_string()
    }
}

impl Type for ToolRunKey {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        String::definition(types)
    }
}

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
                key.into_inner().to_string()
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(text: String) -> Result<Self, String> {
                text.parse().map(Self)
            }
        }

        impl $name {
            /// The library's identifier it names.
            pub fn inner(&self) -> &$inner {
                &self.0
            }

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
    /// A fingerprint kept validated until it crosses the IPC serialization boundary.
    FingerprintKey,
    artifactize_core::types::Fingerprint
);
key!(
    /// A saved session id in IPC, which change hints name.
    SessionKey,
    artifactize_core::types::SessionId
);

#[cfg(test)]
mod tests {
    use super::FingerprintKey;
    use serde_json::json;

    #[test]
    fn ipc_fingerprints_preserve_the_core_guard_and_string_wire_value() {
        for invalid in [String::new(), "with spaces".into(), "x".repeat(129)] {
            assert!(serde_json::from_value::<FingerprintKey>(json!(invalid)).is_err());
        }
        let key = serde_json::from_value::<FingerprintKey>(json!("graph:abcd")).unwrap();
        assert_eq!(serde_json::to_value(key).unwrap(), json!("graph:abcd"));
    }
}
