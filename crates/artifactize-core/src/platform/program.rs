//! Program lookup metadata policies shared by scoped execution and preflight.

use std::{
    ffi::OsStr,
    io,
    path::{Component, Path, PathBuf},
};

pub(crate) fn reject_link_ancestors(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        if super::path_kind(ancestor).is_ok_and(|kind| kind == super::FileKind::Symlink) {
            return Err("Artifact symlinks are not supported.".into());
        }
    }
    Ok(())
}

/// The folder-relative path a declared command names, with native separators converted
/// for scope validation. A bare PATH name or an absolute command stays literal (`None`).
pub(crate) fn folder_relative(command: &str) -> Result<Option<String>, String> {
    let path = Path::new(command);
    if path.is_absolute() {
        return Ok(None);
    }
    if path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_))) {
        return Err("A program path must be absolute or relative to its Artifact.".into());
    }
    let command = super::program_text(command);
    if !command.contains('/') {
        return Ok(None);
    }
    let relative = command.strip_prefix("./").unwrap_or(&command);
    artifactize_tools::scope::logical_path(relative).map_err(|error| error.to_string())?;
    Ok(Some(relative.to_owned()))
}

pub(crate) fn candidates(path: &Path) -> Vec<PathBuf> {
    artifactize_tools::program::candidates(path, super::environment::var)
}

/// Lookup-added suffixes use actual entry spelling; caller-authored parents remain exact.
pub(crate) fn suffix_spelling(directory: &Path, candidate: &Path) -> Option<PathBuf> {
    let requested = candidate.file_name()?.to_str()?;
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            let name = entry.file_name();
            name.to_str()?
                .eq_ignore_ascii_case(requested)
                .then(|| candidate.with_file_name(name))
        })
}

/// Find `program` as a child whose environment `variable` reads would: by `PATH` (and on
/// Windows `PATHEXT`) for a bare name, below `cwd` for a path.
pub(crate) fn resolve_with(
    program: &std::ffi::OsStr,
    cwd: &Path,
    variable: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> std::io::Result<PathBuf> {
    artifactize_tools::program::resolve_with(program, cwd, variable)
}

/// Find `program` as a child with the person's environment ([`super::environment::snapshot`])
/// would.
pub(crate) fn resolve(program: &std::ffi::OsStr, cwd: &Path) -> std::io::Result<PathBuf> {
    artifactize_tools::program::resolve(program, cwd)
}

/// Find `program` beside this executable or one directory up (Cargo test binaries are
/// under `deps`), then on PATH. All candidates use the ordinary executable lookup rules.
pub(crate) fn resolve_near_current(program: &OsStr) -> io::Result<PathBuf> {
    let current = std::env::current_exe()?;
    if let Some(path) = resolve_sibling(program, &current) {
        return Ok(path);
    }
    resolve(program, &std::env::current_dir()?)
}

fn resolve_sibling(program: &OsStr, current: &Path) -> Option<PathBuf> {
    current
        .ancestors()
        .skip(1)
        .take(2)
        .find_map(|directory| resolve(directory.join(program).as_os_str(), directory).ok())
}

/// Hand `target`, a file or URL, to the desktop's opener.
pub(crate) async fn open_desktop(target: &std::ffi::OsStr) -> std::io::Result<()> {
    artifactize_tools::opener::open(target).await
}

/// A short, synchronous host-tool probe; no shell is added and no console window opens.
pub(crate) fn tool_output<'a>(
    program: &str,
    args: impl IntoIterator<Item = &'a std::ffi::OsStr>,
) -> Option<Vec<u8>> {
    let cwd = std::env::current_dir().ok()?;
    let program = resolve(std::ffi::OsStr::new(program), &cwd).ok()?;
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .env_clear()
        .envs(super::environment::snapshot())
        .stdin(std::process::Stdio::null());
    super::hide_console_window(&mut command);
    let output = command.output().ok()?;
    output.status.success().then_some(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_os;

    #[test]
    fn relative_program_paths_are_kept_inside_the_scope() {
        assert_eq!(folder_relative("tool").unwrap(), None);
        assert_eq!(
            folder_relative("./bin/tool").unwrap().as_deref(),
            Some("bin/tool")
        );
        assert!(folder_relative("../bin/tool").is_err());
    }

    // Windows alone recognizes native backslash separators and drive-relative paths.
    #[cfg(windows)]
    #[test]
    fn windows_relative_program_paths_cannot_bypass_scope_validation() {
        assert_eq!(
            folder_relative(r".\bin\tool").unwrap().as_deref(),
            Some("bin/tool")
        );
        assert!(folder_relative(r"..\bin\tool").is_err());
        assert!(folder_relative(r"C:bin\tool").is_err());
        assert!(folder_relative(r"\bin\tool").is_err());
    }

    #[test]
    fn a_directory_does_not_hide_a_program_one_directory_up() {
        sibling_fallback(|_, immediate| std::fs::create_dir(immediate).unwrap());
    }

    // Only Unix gives regular files execute permission bits.
    #[cfg(unix)]
    #[test]
    fn a_non_executable_file_does_not_hide_a_program_one_directory_up() {
        sibling_fallback(|candidate, immediate| {
            std::fs::copy(candidate, immediate).unwrap();
            test_os::deny_execution(immediate);
        });
    }

    fn sibling_fallback(place_unusable_sibling: impl FnOnce(&Path, &Path)) {
        let root = test_os::tempdir();
        let deps = root.path().join("deps");
        std::fs::create_dir(&deps).unwrap();
        let candidate = test_os::working_directory_program(root.path(), "worker");
        let name = candidate.file_name().unwrap();
        place_unusable_sibling(&candidate, &deps.join(name));
        assert_eq!(
            resolve_sibling(name, &deps.join("current")),
            Some(candidate)
        );
    }
}
