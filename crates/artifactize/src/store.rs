//! Versioned state, canonical objects, receipts, and storage maintenance.

use sha2::{Digest, Sha256};
use std::{
    env, fs, io,
    os::unix::fs::DirBuilderExt,
    path::{Component, Path, PathBuf},
};

mod receipts;
pub use receipts::{DATABASE, Receipts, Request, Run, RunView, read_run};

/// Default receipts are repository-bound; global services keep using state_home.
pub fn receipts_dir(repo: &Path, explicit: Option<&Path>) -> Result<PathBuf, String> {
    let path = match explicit {
        Some(path) => path.to_path_buf(),
        None => {
            let hash = format!("{:x}", Sha256::digest(repo.as_os_str().as_encoded_bytes()));
            state_home().map_err(|e| e.to_string())?.join(&hash[..24])
        }
    };
    canonical_target(&path).map_err(|e| e.to_string())
}

pub(crate) fn canonical_target(path: &Path) -> io::Result<PathBuf> {
    let mut resolved = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            _ => {
                resolved.push(component);
                match resolved.symlink_metadata() {
                    Ok(_) => resolved = resolved.canonicalize()?,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

pub(crate) fn outside_workspace(workspace: &Path, output: &Path) -> io::Result<()> {
    if output.starts_with(workspace) {
        return Err(io::Error::other(
            "State and output directories must be outside the reviewed repository.",
        ));
    }
    Ok(())
}

pub(crate) fn prepare_directory(path: &Path, workspace: &Path) -> io::Result<PathBuf> {
    let path = canonical_target(path)?;
    outside_workspace(workspace, &path)?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&path)?;
    let path = path.canonicalize()?;
    outside_workspace(workspace, &path)?;
    Ok(path)
}

/// SQLite `user_version` for artifactize state databases.
pub const STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
#[error("cannot resolve state home: set ARTIFACTIZE_STATE_HOME, XDG_STATE_HOME, or HOME")]
pub struct StateHomeError;

/// Resolve the global state home without creating directories; empty variables are ignored.
pub fn state_home() -> Result<PathBuf, StateHomeError> {
    resolve_state_home(
        env::var_os("ARTIFACTIZE_STATE_HOME").map(PathBuf::from),
        env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

fn resolve_state_home(
    artifactize: Option<PathBuf>,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, StateHomeError> {
    if let Some(path) = artifactize.filter(|path| !path.as_os_str().is_empty()) {
        return Ok(path);
    }
    if let Some(path) = xdg.filter(|path| !path.as_os_str().is_empty()) {
        return Ok(path.join("artifactize"));
    }
    home.filter(|path| !path.as_os_str().is_empty())
        .map(|path| path.join(".local/state/artifactize"))
        .ok_or(StateHomeError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_home_precedence() {
        let artifactize = PathBuf::from("/custom/state");
        let xdg = PathBuf::from("/xdg/state");
        let home = PathBuf::from("/home/reviewer");

        assert_eq!(
            resolve_state_home(
                Some(artifactize.clone()),
                Some(xdg.clone()),
                Some(home.clone())
            )
            .unwrap(),
            artifactize
        );
        assert_eq!(
            resolve_state_home(None, Some(xdg.clone()), Some(home.clone())).unwrap(),
            xdg.join("artifactize")
        );
        assert_eq!(
            resolve_state_home(None, None, Some(home.clone())).unwrap(),
            home.join(".local/state/artifactize")
        );
    }

    #[test]
    fn empty_state_home_variables_are_ignored() {
        let empty = Some(PathBuf::new());
        let home = PathBuf::from("/home/reviewer");

        assert_eq!(
            resolve_state_home(empty.clone(), empty.clone(), Some(home.clone())).unwrap(),
            home.join(".local/state/artifactize")
        );
        assert!(resolve_state_home(empty.clone(), empty.clone(), empty).is_err());
        assert!(resolve_state_home(None, None, None).is_err());
    }
}
