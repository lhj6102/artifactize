//! Program lookup metadata policies shared by scoped execution and preflight.

use std::path::{Path, PathBuf};

pub(crate) fn reject_link_ancestors(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        if super::path_kind(ancestor).is_ok_and(|kind| kind == super::FileKind::Symlink) {
            return Err("Artifact symlinks are not supported.".into());
        }
    }
    Ok(())
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

/// A short, synchronous host-tool probe; no shell is added.
pub(crate) fn tool_output<'a>(
    program: &str,
    args: impl IntoIterator<Item = &'a std::ffi::OsStr>,
) -> Option<Vec<u8>> {
    let cwd = std::env::current_dir().ok()?;
    let program = artifactize_tools::program::resolve(std::ffi::OsStr::new(program), &cwd).ok()?;
    let output = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}
