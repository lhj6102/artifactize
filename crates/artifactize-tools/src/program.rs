//! Literal program lookup, shared by preflight and process launches.

use std::{
    env,
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

/// Resolve against the inherited PATH and, on Windows, PATHEXT. A bare name never
/// implicitly searches the working directory; relative PATH entries are based on `cwd`.
pub fn resolve(program: &OsStr, cwd: &Path) -> io::Result<PathBuf> {
    resolve_with(
        program,
        cwd,
        env::var_os("PATH").as_deref(),
        env::var_os("PATHEXT").as_deref(),
    )
}

/// Resolve using the environment that the child will receive, not the caller's PATH.
pub fn resolve_with(
    program: &OsStr,
    cwd: &Path,
    path: Option<&OsStr>,
    pathext: Option<&OsStr>,
) -> io::Result<PathBuf> {
    let program = Path::new(program);
    let candidates = candidates(program, pathext);
    let explicit = program.is_absolute() || program.components().count() > 1;
    let found = if explicit {
        candidates
            .into_iter()
            .map(|candidate| cwd.join(candidate))
            .find(|candidate| is_executable(candidate))
    } else {
        env::split_paths(path.unwrap_or_default()).find_map(|directory| {
            candidates
                .iter()
                .map(|candidate| cwd.join(&directory).join(candidate))
                .find(|candidate| is_executable(candidate))
        })
    };
    found.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("Program is unavailable: {}", program.display()),
        )
    })
}

/// Candidate paths in lookup order. Scoped callers validate each candidate inside
/// their own access boundary before allowing it to reach a process launcher.
pub fn candidates(program: &Path, pathext: Option<&OsStr>) -> Vec<PathBuf> {
    #[cfg(windows)]
    if program.extension().is_none() {
        // The Windows default when the variable is absent, not when explicitly empty.
        const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";
        return pathext
            .unwrap_or_else(|| OsStr::new(DEFAULT_PATHEXT))
            .to_string_lossy()
            .split(';')
            .filter(|extension| extension.starts_with('.') && !extension.contains(['/', '\\']))
            .map(|extension| {
                let mut candidate = program.as_os_str().to_owned();
                candidate.push(extension);
                PathBuf::from(candidate)
            })
            .collect();
    }
    #[cfg(not(windows))]
    let _ = pathext;
    vec![program.to_owned()]
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Any execute bit is the existing declared-command preflight rule.
        const EXECUTE_BITS: u32 = 0o111;
        path.metadata().is_ok_and(|metadata| {
            metadata.is_file() && metadata.permissions().mode() & EXECUTE_BITS != 0
        })
    }
    #[cfg(windows)]
    {
        path.metadata().is_ok_and(|metadata| metadata.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_uses_path_order_relative_to_child_cwd_and_never_implicit_local_search() {
        let root = tempfile::tempdir().unwrap();
        for directory in ["first", "second"] {
            std::fs::create_dir(root.path().join(directory)).unwrap();
            let file = root.path().join(directory).join("shim.cmd");
            std::fs::write(&file, "fixture").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        let path = env::join_paths(["second", "first"]).unwrap();
        assert_eq!(
            resolve_with(OsStr::new("shim.cmd"), root.path(), Some(&path), None).unwrap(),
            root.path().join("second/shim.cmd")
        );
        assert!(resolve_with(OsStr::new("shim.cmd"), root.path(), None, None).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn extensionless_names_follow_pathext_order_not_exe_preference() {
        let root = tempfile::tempdir().unwrap();
        for extension in ["cmd", "exe", "bat"] {
            std::fs::write(root.path().join(format!("shim.{extension}")), "fixture").unwrap();
        }
        let path = env::join_paths([root.path()]).unwrap();
        assert_eq!(
            resolve_with(
                OsStr::new("shim"),
                root.path(),
                Some(&path),
                Some(OsStr::new(".CMD;.EXE"))
            )
            .unwrap(),
            root.path().join("shim.CMD")
        );
        assert_eq!(
            resolve_with(
                OsStr::new("shim"),
                root.path(),
                Some(&path),
                Some(OsStr::new(".BAT;.CMD"))
            )
            .unwrap(),
            root.path().join("shim.BAT")
        );
    }
}
