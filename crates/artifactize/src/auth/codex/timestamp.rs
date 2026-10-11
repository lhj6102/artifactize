//! OAuth timestamps have integer-second precision and the full saved u64 domain.
//! Represent the instant as elapsed time since the Unix epoch, rather than narrowing
//! older credential files to a calendar library's supported year range.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(Duration);

impl Timestamp {
    pub fn now() -> Result<Self, String> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| Self::from_seconds(elapsed.as_secs()))
            .map_err(|_| "The system clock is before the Unix epoch.".into())
    }

    pub fn from_seconds(seconds: u64) -> Self {
        Self(Duration::from_secs(seconds))
    }

    pub fn seconds(self) -> u64 {
        self.0.as_secs()
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        self.0
            .checked_add(duration)
            .map(|elapsed| Self::from_seconds(elapsed.as_secs()))
    }

    pub fn saturating_add(self, duration: Duration) -> Self {
        Self::from_seconds(self.0.saturating_add(duration).as_secs())
    }

    /// How long after `earlier` this is; zero when it is not later.
    pub fn saturating_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        seconds::serialize(self, serializer)
    }
}
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        seconds::deserialize(deserializer)
    }
}

pub(super) mod seconds {
    use super::*;
    pub fn serialize<S: Serializer>(time: &Timestamp, serializer: S) -> Result<S::Ok, S::Error> {
        time.seconds().serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Timestamp, D::Error> {
        u64::deserialize(deserializer).map(Timestamp::from_seconds)
    }
}

pub(super) mod optional_duration {
    use super::*;
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Duration>, D::Error> {
        Option::<u64>::deserialize(deserializer).map(|value| value.map(Duration::from_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_seconds_keep_the_entire_saved_domain() {
        for seconds in [0, 1, 10_000, u64::MAX] {
            let wire = seconds.to_string();
            let time: Timestamp = serde_json::from_str(&wire).unwrap();
            assert_eq!(serde_json::to_string(&time).unwrap(), wire);
        }
        for wire in ["-1", "1.5", "\"100\"", "null"] {
            assert!(serde_json::from_str::<Timestamp>(wire).is_err());
        }
    }

    #[test]
    fn timestamp_margins_preserve_checked_and_saturating_overflow() {
        let time = Timestamp::from_seconds(10_000);
        assert_eq!(
            time.checked_add(Duration::from_secs(3600))
                .unwrap()
                .seconds(),
            13_600
        );
        let last = Timestamp::from_seconds(u64::MAX);
        assert!(last.checked_add(Duration::from_secs(1)).is_none());
        assert_eq!(last.saturating_add(Duration::from_secs(300)), last);
    }
}
