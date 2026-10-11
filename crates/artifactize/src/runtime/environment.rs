use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

use super::Error;
use crate::{platform, workspace::canonical_target};

pub(super) fn prepare(
    workspace: &Path,
    run_dir: &Path,
) -> Result<(PathBuf, PathBuf, BTreeMap<OsString, OsString>), Error> {
    let workspace = platform::canonicalize(workspace)?;
    if !workspace.is_dir() {
        return Err(io::Error::other("runtime workspace must be a directory").into());
    }
    let output_root = canonical_target(run_dir)?;
    outside_workspace(&workspace, &output_root)?;
    platform::create_private_dir_all(&output_root)?;
    let output_root = platform::canonicalize(&output_root)?;
    outside_workspace(&workspace, &output_root)?;

    let directory = platform::private_tempdir_in("runtime-", &output_root)?;
    let root = directory.path();
    for name in ["output", "tmp", "home", "cache"] {
        platform::create_private_dir(&root.join(name))?;
    }
    let mut environment = BTreeMap::from([
        (
            "PATH".into(),
            platform::environment::var("PATH").unwrap_or_default(),
        ),
        (
            "LANG".into(),
            platform::environment::var("LANG").unwrap_or_else(|| "en_US.UTF-8".into()),
        ),
        ("ARTIFACTIZE_WORKSPACE_DIR".into(), workspace.clone().into()),
    ]);
    // The child's home, cache and temporary directories are private ones, under every
    // variable the platform's programs read them from.
    let private = [
        ("ARTIFACTIZE_OUTPUT_DIR", "output"),
        ("ARTIFACTIZE_TMP_DIR", "tmp"),
    ]
    .into_iter()
    .chain(platform::TEMP_VARIABLES.iter().map(|key| (*key, "tmp")))
    .chain(platform::HOME_VARIABLES.iter().map(|key| (*key, "home")))
    .chain(platform::CACHE_VARIABLES.iter().map(|key| (*key, "cache")));
    for (key, name) in private {
        environment.insert(key.into(), root.join(name).into());
    }
    // Some systems' programs cannot start without a few system variables.
    for key in platform::SYSTEM_VARIABLES {
        if let Some(value) = platform::environment::var(key) {
            environment.insert(key.into(), value);
        }
    }
    Ok((workspace, directory.keep(), environment))
}

fn outside_workspace(workspace: &Path, output: &Path) -> Result<(), Error> {
    crate::workspace::outside_workspace(workspace, output).map_err(|_| Error::OutputInsideWorkspace)
}
