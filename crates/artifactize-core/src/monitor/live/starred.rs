//! The branches a person starred: listed before the others, whatever the search's order. They
//! are kept in the state directory, beside the remote review store's settings, so every monitor
//! of this machine's user lists them the same way.

use super::Place;
use std::{collections::BTreeSet, io::Write, path::Path};

const FILE: &str = "starred-branches.json";

/// The starred branches; none before the first star.
pub fn starred(state: Option<&Path>) -> Result<BTreeSet<Place>, String> {
    let path = crate::store::state_dir(state)?.join(FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("{}: {e}", crate::platform::path_text(&path))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(format!("{}: {e}", crate::platform::path_text(&path))),
    }
}

/// Star `place`, or take its star away, and return the starred branches as kept.
pub fn star(state: Option<&Path>, place: &Place, on: bool) -> Result<BTreeSet<Place>, String> {
    let mut kept = starred(state)?;
    if on {
        kept.insert(place.clone());
    } else {
        kept.remove(place);
    }
    let dir = crate::store::state_dir(state)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, &kept).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.persist(dir.join(FILE))
        .map_err(|e| e.error.to_string())?;
    Ok(kept)
}
