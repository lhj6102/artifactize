use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs::{self, DirBuilder, Permissions},
    io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};

use super::Error;

pub(super) fn prepare(
    workspace: &Path,
    run_dir: &Path,
) -> Result<(PathBuf, PathBuf, BTreeMap<OsString, OsString>), Error> {
    let workspace = workspace.canonicalize()?;
    if !workspace.is_dir() {
        return Err(io::Error::other("runtime workspace must be a directory").into());
    }
    let output_root = canonical_target(run_dir)?;
    outside_workspace(&workspace, &output_root)?;
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&output_root)?;
    let output_root = output_root.canonicalize()?;
    outside_workspace(&workspace, &output_root)?;

    let directory = tempfile::Builder::new()
        .prefix("runtime-")
        .permissions(Permissions::from_mode(0o700))
        .tempdir_in(output_root)?;
    let root = directory.path();
    fs::set_permissions(root, Permissions::from_mode(0o700))?;
    for name in ["output", "tmp", "home", "cache"] {
        let path = root.join(name);
        DirBuilder::new().mode(0o700).create(&path)?;
        fs::set_permissions(path, Permissions::from_mode(0o700))?;
    }
    let mut environment = BTreeMap::from([
        ("PATH".into(), env::var_os("PATH").unwrap_or_default()),
        (
            "LANG".into(),
            env::var_os("LANG").unwrap_or_else(|| "en_US.UTF-8".into()),
        ),
        ("ARTIFACTIZE_WORKSPACE_DIR".into(), workspace.clone().into()),
    ]);
    for (key, name) in [
        ("ARTIFACTIZE_OUTPUT_DIR", "output"),
        ("ARTIFACTIZE_TMP_DIR", "tmp"),
        ("TMPDIR", "tmp"),
        ("TMP", "tmp"),
        ("TEMP", "tmp"),
        ("HOME", "home"),
        ("XDG_CACHE_HOME", "cache"),
    ] {
        environment.insert(key.into(), root.join(name).into());
    }
    Ok((workspace, directory.keep(), environment))
}

fn outside_workspace(workspace: &Path, output: &Path) -> Result<(), Error> {
    if output.starts_with(workspace) {
        return Err(Error::OutputInsideWorkspace);
    }
    Ok(())
}

fn canonical_target(path: &Path) -> io::Result<PathBuf> {
    let mut ancestor = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::ParentDir => {
                ancestor.pop();
            }
            _ => ancestor.push(component),
        }
    }
    let mut missing = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(mut path) => {
                for name in missing.into_iter().rev() {
                    path.push(name);
                }
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(ancestor.file_name().ok_or(error)?.to_os_string());
                ancestor.pop();
            }
            Err(error) => return Err(error),
        }
    }
}
