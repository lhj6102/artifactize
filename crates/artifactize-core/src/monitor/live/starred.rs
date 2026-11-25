//! What a person starred, kept in the state directory beside the remote review store's
//! settings, so every monitor of this machine's user shows them the same way: the branches,
//! listed before the others whatever the search's order, and the Artifacts of each project,
//! its favorites on every one of its branches.

use super::{Place, Project};
use crate::types::ArtifactName;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::BTreeSet, io::Write, path::Path};

const BRANCHES: &str = "starred-branches.json";
const ARTIFACTS: &str = "starred-artifacts.json";

/// The starred branches; none before the first star.
pub fn starred(state: Option<&Path>) -> Result<BTreeSet<Place>, String> {
    read(state, BRANCHES)
}

/// Star `place`, or take its star away, and return the starred branches as kept.
pub fn star(state: Option<&Path>, place: &Place, on: bool) -> Result<BTreeSet<Place>, String> {
    let dir = crate::store::state_dir(state)?;
    let _lock = lock(&dir, BRANCHES)?;
    let mut kept: BTreeSet<Place> = read_file(&dir, BRANCHES)?;
    if on {
        kept.insert(place.clone());
    } else {
        kept.remove(place);
    }
    write(&dir, BRANCHES, &kept)?;
    Ok(kept)
}

/// A project's starred Artifacts, as kept.
#[derive(Debug, Serialize, Deserialize)]
struct Starred {
    project: Project,
    artifacts: BTreeSet<ArtifactName>,
}

/// The starred Artifacts of `project`; none before its first star.
pub fn starred_artifacts(
    state: Option<&Path>,
    project: &Project,
) -> Result<BTreeSet<ArtifactName>, String> {
    let kept: Vec<Starred> = read(state, ARTIFACTS)?;
    Ok(kept
        .into_iter()
        .find(|starred| &starred.project == project)
        .map(|starred| starred.artifacts)
        .unwrap_or_default())
}

/// Star the Artifact `name` of `project`, or take its star away, and return the project's
/// starred Artifacts as kept. A project left with none is no longer kept.
pub fn star_artifact(
    state: Option<&Path>,
    project: &Project,
    name: &ArtifactName,
    on: bool,
) -> Result<BTreeSet<ArtifactName>, String> {
    let dir = crate::store::state_dir(state)?;
    let _lock = lock(&dir, ARTIFACTS)?;
    let mut kept: Vec<Starred> = read_file(&dir, ARTIFACTS)?;
    let at = match kept.iter().position(|starred| &starred.project == project) {
        Some(at) => at,
        None => {
            kept.push(Starred {
                project: project.clone(),
                artifacts: BTreeSet::new(),
            });
            kept.len() - 1
        }
    };
    if on {
        kept[at].artifacts.insert(name.clone());
    } else {
        kept[at].artifacts.remove(name);
    }
    let artifacts = kept[at].artifacts.clone();
    kept.retain(|starred| !starred.artifacts.is_empty());
    write(&dir, ARTIFACTS, &kept)?;
    Ok(artifacts)
}

/// The stars kept in `file`; none before the first.
fn read<T: DeserializeOwned + Default>(state: Option<&Path>, file: &str) -> Result<T, String> {
    read_file(&crate::store::state_dir(state)?, file)
}

fn read_file<T: DeserializeOwned + Default>(dir: &Path, file: &str) -> Result<T, String> {
    use std::io::Read;
    let path = dir.join(file);
    let mut handle = match crate::platform::open_regular(&path) {
        Ok(handle) => handle,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(format!("{}: {e}", crate::platform::path_text(&path))),
    };
    let mut bytes = Vec::new();
    handle
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", crate::platform::path_text(&path)))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("{}: {e}", crate::platform::path_text(&path)))
}

/// Lock a stable sidecar instead of the file being atomically replaced.
fn lock(dir: &Path, file: &str) -> Result<std::fs::File, String> {
    crate::platform::paths::lock_file(&dir.join(format!("{file}.lock")))
        .map_err(|error| format!("Cannot lock {file}: {error}"))
}

/// Replace `file` with `kept` at once: a reader sees the stars before or after, never a part.
fn write(dir: &Path, file: &str, kept: &impl Serialize) -> Result<(), String> {
    let mut temporary = tempfile::NamedTempFile::new_in(dir).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut temporary, kept).map_err(|e| e.to_string())?;
    temporary.flush().map_err(|e| e.to_string())?;
    crate::platform::paths::replace_open_file(temporary, &dir.join(file))
        .map_err(|error| error.to_string())?;
    Ok(())
}
