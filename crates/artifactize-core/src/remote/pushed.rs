//! What this machine pushed to the remote review store lately: each push that sent records,
//! when and how many, kept two days so a reader can say what was pushed on its own local day.

use std::{
    fs,
    io::Write,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// The file in the state directory that keeps the recent pushes.
const FILE: &str = "remote-pushed.json";
/// How long a push is kept: any local day still holds its own pushes.
const KEPT: Duration = Duration::from_secs(48 * 60 * 60);

/// One push that sent records: when, in milliseconds since the Unix epoch, and how many.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pushed {
    pub at: u64,
    pub count: usize,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The pushes this machine made in the last two days, oldest first, from the state directory
/// `state` names (the shared one by default); none when nothing was kept or it cannot be read.
pub fn recent(state: Option<&Path>) -> Vec<Pushed> {
    crate::store::state_dir(state).map_or_else(|_| Vec::new(), |dir| kept(&dir))
}

/// The pushes kept in the state directory `state` from the last two days, oldest first.
fn kept(state: &Path) -> Vec<Pushed> {
    let Ok(text) = fs::read_to_string(state.join(FILE)) else {
        return Vec::new();
    };
    let since = now().saturating_sub(u64::try_from(KEPT.as_millis()).unwrap_or(u64::MAX));
    serde_json::from_str::<Vec<Pushed>>(&text)
        .unwrap_or_default()
        .into_iter()
        .filter(|pushed| pushed.at >= since)
        .collect()
}

/// Keep a push of `count` records made now, with the others of the last two days.
pub(crate) fn record(state: &Path, count: usize) -> Result<(), String> {
    let mut pushes = kept(state);
    pushes.push(Pushed { at: now(), count });
    let mut file = tempfile::NamedTempFile::new_in(state).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, &pushes).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.persist(state.join(FILE))
        .map_err(|e| e.error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_each_push_with_its_count_and_drops_old_ones() {
        let state = tempfile::tempdir().expect("state");
        assert!(kept(state.path()).is_empty());
        record(state.path(), 3).expect("first push");
        record(state.path(), 2).expect("second push");
        assert_eq!(
            kept(state.path())
                .iter()
                .map(|pushed| pushed.count)
                .collect::<Vec<_>>(),
            [3, 2]
        );
        // A push older than two days is no longer kept.
        let old = [Pushed { at: 1, count: 9 }];
        fs::write(
            state.path().join(FILE),
            serde_json::to_string(&old).unwrap(),
        )
        .unwrap();
        assert!(kept(state.path()).is_empty());
    }
}
