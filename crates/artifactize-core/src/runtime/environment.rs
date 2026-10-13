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
    for name in ["output", "tmp"] {
        platform::create_private_dir(&root.join(name))?;
    }
    // The command runs in the author's own environment, as in their terminal. Only what is
    // artifactize's own differs: its variables stay out, and the command gets its per-run
    // output and temporary directories.
    let mut environment = platform::environment::for_command();
    platform::environment::set(&mut environment, "ARTIFACTIZE_WORKSPACE_DIR", &workspace);
    for (key, name) in [
        ("ARTIFACTIZE_OUTPUT_DIR", "output"),
        ("ARTIFACTIZE_TMP_DIR", "tmp"),
    ]
    .into_iter()
    .chain(platform::TEMP_VARIABLES.iter().map(|key| (*key, "tmp")))
    {
        platform::environment::set(&mut environment, key, root.join(name));
    }
    Ok((workspace, directory.keep(), environment))
}

fn outside_workspace(workspace: &Path, output: &Path) -> Result<(), Error> {
    crate::workspace::outside_workspace(workspace, output).map_err(|_| Error::OutputInsideWorkspace)
}
