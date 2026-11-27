//! Recent pushes to the remote review store, retained for two days.
use crate::types::Timestamp;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path, time::Duration};

const FILE: &str = "remote-pushed.json";
/// A separate lock inode stays in place across atomic history updates.
const LOCK: &str = "remote-pushed.lock";
/// Retain every push that could still belong to the reader's local day.
const KEPT: Duration = Duration::from_secs(48 * 60 * 60);
/// The existing JSON wire uses epoch milliseconds; Timestamp keeps nanoseconds internally.
const NANOS_PER_MILLISECOND: i128 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pushed {
    #[serde(with = "milliseconds")]
    pub at: Timestamp,
    pub count: usize,
}
mod milliseconds {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub(super) fn serialize<S: Serializer>(
        at: &Timestamp,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let value = u64::try_from(at.time().unix_timestamp_nanos() / NANOS_PER_MILLISECOND)
            .map_err(serde::ser::Error::custom)?;
        serializer.serialize_u64(value)
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Timestamp, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(millis) * NANOS_PER_MILLISECOND)
            .ok()
            .and_then(Timestamp::new)
            .ok_or_else(|| serde::de::Error::custom("Push timestamp is out of range."))
    }
}
/// One ledger read supplies the recent records and the local day's total.
pub struct PushHistory {
    pub recent: Vec<Pushed>,
    pub pushed_today: usize,
}

pub fn history_at(state: Option<&Path>, now: Timestamp) -> PushHistory {
    history_in(state, now, crate::platform::local_day)
}

fn history_in(
    state: Option<&Path>,
    now: Timestamp,
    local_day: impl Fn(time::OffsetDateTime) -> time::Date,
) -> PushHistory {
    let recent = recent_at(state, now);
    let day = local_day(now.time());
    let pushed_today = recent
        .iter()
        .filter(|push| push.at <= now && local_day(push.at.time()) == day)
        .fold(0usize, |total, push| total.saturating_add(push.count));
    PushHistory {
        recent,
        pushed_today,
    }
}

/// Recent pushes, oldest first; absent or unreadable state has no history to show.
pub fn recent(state: Option<&Path>) -> Vec<Pushed> {
    recent_at(state, Timestamp::now())
}

/// Get the recent pushes relative to the supplied time.
pub fn recent_at(state: Option<&Path>, now: Timestamp) -> Vec<Pushed> {
    crate::store::state_dir(state).map_or_else(|_| Vec::new(), |dir| kept_at(&dir, now))
}
fn kept_at(state: &Path, now: Timestamp) -> Vec<Pushed> {
    let Ok(text) = fs::read_to_string(state.join(FILE)) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<Pushed>>(&text)
        .unwrap_or_default()
        .into_iter()
        .filter(|pushed| now.since(pushed.at) <= KEPT)
        .collect()
}
pub(super) fn record_at(state: &Path, count: usize, at: Timestamp) -> Result<(), String> {
    let lock = crate::platform::open_no_follow(
        crate::platform::private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false),
        &state.join(LOCK),
    )
    .map_err(|error| error.to_string())?;
    crate::platform::paths::lock(&lock).map_err(|error| error.to_string())?;
    let mut pushes = kept_at(state, at);
    pushes.push(Pushed { at, count });
    let mut file = tempfile::NamedTempFile::new_in(state).map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, &pushes).map_err(|error| error.to_string())?;
    file.flush().map_err(|error| error.to_string())?;
    crate::platform::paths::replace_open_file(file, &state.join(FILE))
        .map_err(|error| error.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ledger_today_uses_civil_days_at_each_push_not_the_current_offset() {
        let state = crate::test_os::tempdir();
        let stamp = |text: &str| text.parse::<Timestamp>().unwrap();
        let before = stamp("2026-03-08T04:59:59Z");
        let midnight = stamp("2026-03-08T05:00:00Z");
        let daylight = stamp("2026-03-08T07:00:00Z");
        let now = stamp("2026-03-08T16:00:00Z");
        let future = stamp("2026-03-08T16:01:00Z");
        for (at, count) in [(before, 9), (midnight, 3), (daylight, 2), (future, 11)] {
            record_at(state.path(), count, at).unwrap();
        }
        let history = history_in(Some(state.path()), now, |instant| {
            let hours = if instant < daylight.time() { -5 } else { -4 };
            instant
                .to_offset(time::UtcOffset::from_hms(hours, 0, 0).unwrap())
                .date()
        });
        assert_eq!(history.recent.len(), 4);
        assert_eq!(history.pushed_today, 5);
    }

    #[test]
    fn concurrent_pushes_keep_all_counts() {
        let state = crate::test_os::tempdir();
        let at = Timestamp::new(time::OffsetDateTime::UNIX_EPOCH).unwrap();
        let counts = 1..=8;
        let ready = std::sync::Barrier::new(counts.clone().count());
        std::thread::scope(|scope| {
            for count in counts.clone() {
                let path = state.path();
                let ready = &ready;
                scope.spawn(move || {
                    ready.wait();
                    record_at(path, count, at).unwrap();
                });
            }
        });
        let mut saved: Vec<_> = kept_at(state.path(), at)
            .into_iter()
            .map(|entry| entry.count)
            .collect();
        saved.sort_unstable();
        assert_eq!(saved, counts.collect::<Vec<_>>());
    }

    #[test]
    fn keeps_each_push_with_its_count_and_drops_old_ones() {
        let state = crate::test_os::tempdir();
        let at =
            Timestamp::new(time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(10)).unwrap();
        let after = |duration| Timestamp::new(at.time() + duration).unwrap();
        assert!(kept_at(state.path(), at).is_empty());
        record_at(state.path(), 3, at).unwrap();
        record_at(state.path(), 2, after(time::Duration::seconds(1))).unwrap();
        let wire: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(state.path().join(FILE)).unwrap()).unwrap();
        assert_eq!(
            wire,
            serde_json::json!([{"at":10000,"count":3},{"at":11000,"count":2}])
        );
        let counts = |now| {
            kept_at(state.path(), now)
                .into_iter()
                .map(|push| push.count)
                .collect::<Vec<_>>()
        };
        assert_eq!(counts(after(time::Duration::hours(48))), [3, 2]);
        assert_eq!(
            counts(after(
                time::Duration::hours(48) + time::Duration::milliseconds(1)
            )),
            [2]
        );
        assert!(counts(after(time::Duration::hours(49))).is_empty());
    }
}
