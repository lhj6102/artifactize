//! A point in time as artifactize records it: UTC, written as RFC 3339 with nine fractional
//! digits, so that saved timestamps also sort as text.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, str::FromStr, time::Duration};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

/// RFC 3339 uses a four-digit year. Keep UTC years within that width so normalized
/// timestamps retain the fixed-width representation required by SQLite text ordering.
const MAX_SORTABLE_YEAR: i32 = 9999;

/// A UTC time within years 0 to 9999, the range whose text form sorts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(OffsetDateTime);

impl Timestamp {
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    /// Any time, in UTC; `None` outside the sortable years.
    pub fn new(time: OffsetDateTime) -> Option<Self> {
        // An otherwise valid offset can normalize beyond time's representable UTC range.
        time.checked_to_offset(UtcOffset::UTC)
            .filter(|time| (0..=MAX_SORTABLE_YEAR).contains(&time.year()))
            .map(Self)
    }

    pub fn time(self) -> OffsetDateTime {
        self.0
    }

    /// How long after `earlier` this is; zero when it is not later.
    pub fn since(self, earlier: Self) -> Duration {
        (self.0 - earlier.0).try_into().unwrap_or_default()
    }
}

impl FromStr for Timestamp {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        OffsetDateTime::parse(value, &Rfc3339)
            .ok()
            .and_then(Self::new)
            .ok_or_else(|| format!("Invalid RFC 3339 timestamp: {value:?}."))
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let time = self.0;
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
            time.year(),
            u8::from(time.month()),
            time.day(),
            time.hour(),
            time.minute(),
            time.second(),
            time.nanosecond()
        )
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl ToSql for Timestamp {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.to_string()))
    }
}

impl FromSql for Timestamp {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        value
            .as_str()?
            .parse()
            .map_err(|error: String| FromSqlError::Other(error.into()))
    }
}
