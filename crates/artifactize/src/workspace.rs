//! Workspace paths and external state/output safety checks.

use std::{
    io,
    path::{Path, PathBuf},
};

/// Legacy markers remain safety boundaries even though discovery refuses their format.
pub(crate) const ARTIFACT_MARKERS: [&str; 3] = [
    crate::config::CONFIG_FILE,
    "artifactize.json",
    ".artifactizeignore",
];

fn has_marker(path: &Path) -> io::Result<bool> {
    match crate::platform::marker_metadata(path) {
        Ok(_) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn has_artifact_marker(path: &Path) -> io::Result<bool> {
    for marker in ARTIFACT_MARKERS {
        if has_marker(&path.join(marker))? {
            return Ok(true);
        }
    }
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(false);
        }
        // Search-only ancestors can be traversed but not enumerated. Retain fixed-marker
        // stat probes, including .git, without rejecting an otherwise usable state path.
        // Unknown sidecars cannot be detected here, but discovery also cannot enumerate
        // this directory, so artifactize cannot run it as a discovered workspace.
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return has_marker(&path.join(".git"));
        }
        Err(error) => return Err(error),
    };
    for entry in entries {
        if entry?.file_name().as_encoded_bytes().ends_with(b".artf") {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn canonical_target(path: &Path) -> io::Result<PathBuf> {
    crate::platform::paths::canonical_target(path)
}

pub(crate) fn outside_workspace(workspace: &Path, output: &Path) -> io::Result<()> {
    if crate::platform::is_within(output, workspace) {
        return Err(io::Error::other(
            "State and output directories must be outside the reviewed repository.",
        ));
    }
    outside_artifact_workspaces(output)
}

pub(crate) fn outside_artifact_workspaces(output: &Path) -> io::Result<()> {
    for ancestor in output.ancestors() {
        if has_artifact_marker(ancestor)? {
            return Err(io::Error::other(
                "State and output directories must be outside artifactize workspaces.",
            ));
        }
    }
    Ok(())
}

pub(crate) fn prepare_directory(path: &Path, workspace: &Path) -> io::Result<PathBuf> {
    let path = canonical_target(path)?;
    outside_workspace(workspace, &path)?;
    crate::platform::create_private_dir_all(&path)?;
    let path = crate::platform::canonicalize(&path)?;
    outside_workspace(workspace, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_and_output_creation_refuse_other_current_or_legacy_workspaces() {
        for marker in [
            "index.artf",
            "artifactize.json",
            ".artifactizeignore",
            "file.txt.artf",
        ] {
            let root = crate::test_os::tempdir();
            let reviewed = root.path().join("reviewed");
            let other = root.path().join("other");
            std::fs::create_dir(&reviewed).unwrap();
            std::fs::create_dir(&other).unwrap();
            std::fs::write(other.join(marker), "marker").unwrap();
            let state = other.join("state");
            let error = prepare_directory(&state, &reviewed).unwrap_err();
            assert!(
                error.to_string().contains("outside artifactize workspaces"),
                "{marker}: {error}"
            );
            assert!(!state.exists());
        }
    }
}
