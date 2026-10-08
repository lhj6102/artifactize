//! Saved Run waitTimeoutMs retains schema-5 Option<u32> acceptance, unlike strict config.
#[cfg(test)]
mod tests;
use serde::{Deserialize, Deserializer, Serializer};
use std::time::Duration;

pub(super) fn serialize<S: Serializer>(
    value: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => {
            let milliseconds = u32::try_from(value.as_millis()).map_err(|_| {
                serde::ser::Error::custom("Saved waitTimeoutMs exceeds u32 milliseconds.")
            })?;
            if Duration::from_millis(u64::from(milliseconds)) != *value {
                return Err(serde::ser::Error::custom(
                    "Saved waitTimeoutMs must be whole milliseconds.",
                ));
            }
            serializer.serialize_u32(milliseconds)
        }
        None => serializer.serialize_none(),
    }
}
pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Duration>, D::Error> {
    // Historic schema 5 accepted 0..=u32::MAX integer tokens, but not floats (even integral).
    Option::<u32>::deserialize(deserializer)
        .map(|value| value.map(|ms| Duration::from_millis(u64::from(ms))))
}
