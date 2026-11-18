//! Shared safe file access: pinned no-follow opens and directory listings.

use std::{
    ffi::OsStr,
    fs::File,
    io,
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{
    EntryName, canonicalize, case_sensitive, entry_kind, exact_name, is_link_refusal,
    open_directory, open_entry, open_no_follow, open_nonblocking, read_dir, verified_path,
};
#[cfg(windows)]
pub use windows::{
    EntryName, canonicalize, case_sensitive, entry_kind, exact_name, is_link_refusal,
    open_directory, open_entry, open_no_follow, open_nonblocking, read_dir, verified_path,
};

/// Native lexical equality: Windows accepts either separator; Unix does not.
pub(crate) fn paths_equal(first: &Path, second: &Path) -> bool {
    first == second
}

/// Whether a native path is absolute on this system.
pub fn is_absolute(path: &std::path::Path) -> bool {
    path.is_absolute()
}

/// The logical form of `path` below `root`: an absolute native path is taken relative to
/// `root` (`None` outside it), a relative one is already logical.
pub fn logical_below(root: &std::path::Path, path: &str) -> Option<String> {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        logical_from_native(path.strip_prefix(root).ok()?)
    } else {
        path.to_str().map(str::to_owned)
    }
}

pub(crate) fn parent(path: &Path) -> Option<&Path> {
    path.parent()
}

pub(crate) fn file_name(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()
}

/// The filesystem root of an absolute path (`/`, or a Windows volume or share) and the names
/// below it, in order. `None` when a component is `.` or `..`.
pub fn split_root(path: &std::path::Path) -> Option<(&std::path::Path, Vec<&std::ffi::OsStr>)> {
    use std::path::Component;
    let root = path.ancestors().last()?;
    path.components()
        .filter_map(|component| match component {
            Component::Prefix(_) | Component::RootDir => None,
            Component::Normal(name) => Some(Some(name)),
            Component::CurDir | Component::ParentDir => Some(None),
        })
        .collect::<Option<Vec<_>>>()
        .map(|names| (root, names))
}

/// The target a link names, read without following it: a symlink's text, or on Windows a
/// junction's or symbolic link's substitute name.
pub fn link_target(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    std::fs::read_link(path)
}

/// Whether a name can reach an entry spelled differently, as case-insensitive Windows and
/// macOS volumes allow; path-returning interfaces then also check the exact spelling.
pub const ALIASED_NAMES: bool = cfg!(any(windows, target_os = "macos"));

/// The type of a directory entry, read without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    /// FIFOs, sockets, devices, and on Windows reparse points that are not links.
    Other,
}

/// Read the process's current directory and resolve its native filesystem aliases.
pub fn current_directory() -> std::io::Result<std::path::PathBuf> {
    canonicalize(&std::env::current_dir()?)
}

/// Text shown for a native path or a logical external target, never an I/O conversion.
pub trait PathText {
    fn path_text(&self) -> String;
}
impl<P: AsRef<std::path::Path> + ?Sized> PathText for P {
    fn path_text(&self) -> String {
        native_path_text(self.as_ref())
    }
}
/// Render a path for people and JSON clients; logical external targets stay opaque.
pub fn path_text<P: PathText + ?Sized>(path: &P) -> String {
    path.path_text()
}

fn native_path_text(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let text = text.replace('\\', "/");
        // Canonical Windows paths carry verbatim prefixes that are not useful in a
        // breadcrumb. Only disk/UNC prefixes have equivalent ordinary path spellings.
        const VERBATIM: &str = "//?/";
        const VERBATIM_UNC: &str = "//?/UNC/";
        match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(_) => text[VERBATIM.len()..].to_owned(),
                Prefix::VerbatimUNC(_, _) => format!("//{}", &text[VERBATIM_UNC.len()..]),
                _ => text,
            },
            _ => text,
        }
    }
    #[cfg(unix)]
    {
        text.into_owned()
    }
}

/// Convert a native relative path into logical components; roots, traversal and non-UTF-8
/// names are refused rather than guessing at the host's separator or prefix rules.
pub fn logical_from_native(path: &std::path::Path) -> Option<String> {
    relative_names(path)
        .ok()?
        .into_iter()
        .map(logical_entry_name)
        .collect::<Option<Vec<_>>>()
        .map(|names| names.join("/"))
}

/// The native name can be represented as one logical component, with no reinterpretation.
pub(crate) fn logical_entry_name(name: &OsStr) -> Option<&str> {
    let text = name.to_str()?;
    (!matches!(text, "" | "." | "..")
        && !text
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'/' | b'\\' | b':')))
    .then_some(text)
}

/// Validate native relative paths before traversing them. In particular, Windows prefixes
/// and root-only paths can replace a join's base without being absolute.
pub(crate) fn relative_names(path: &Path) -> io::Result<Vec<&OsStr>> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Artifact path must be relative to its declared root and use logical names.",
        )
    };
    let text = native_path_text(path);
    if text.split('/').any(|part| matches!(part, "." | "..")) {
        return Err(invalid());
    }
    path.components()
        .map(|component| match component {
            Component::Normal(name) if logical_entry_name(name).is_some() => Ok(name),
            _ => Err(invalid()),
        })
        .collect()
}

pub fn scoped_relative(path: &Path) -> io::Result<String> {
    Ok(relative_names(path)?
        .into_iter()
        .map(|name| name.to_str().expect("validated logical name"))
        .collect::<Vec<_>>()
        .join("/"))
}

/// Decode a persisted project-relative target as slash syntax, not host path syntax.
pub(crate) fn parse_target_path(text: &str) -> io::Result<PathBuf> {
    // Empty and dot both denote the project root in saved targets.
    if matches!(text, "" | ".") {
        return Ok(PathBuf::new());
    }
    text.split('/')
        .map(|name| {
            logical_entry_name(OsStr::new(name)).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid slash path for an Artifact target.",
                )
            })
        })
        .collect()
}

/// Determine aliasing by looking up the reserved spelling in the same pinned directory.
/// This uses that directory's filesystem comparison (including Unicode and short names),
/// rather than a guessed case policy or a string fold shared by unlike filesystems.
pub(crate) fn same_entry_name(parent: &File, actual: &OsStr, reserved: &OsStr) -> io::Result<bool> {
    if actual == reserved {
        return Ok(true);
    }
    let Some(name) = EntryName::new(reserved) else {
        return Ok(false);
    };
    match open_entry(parent, &name) {
        // Two exact entries can be distinct directory names for the same inode.
        Ok(alias) => {
            Ok(!exact_name(parent, &alias, reserved)? && exact_name(parent, &alias, actual)?)
        }
        Err(error)
            if is_link_refusal(&error)
                || matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::PermissionDenied
                        | io::ErrorKind::InvalidInput
                ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

/// Keep declaration inputs bounded at the existing 1024 UTF-16-unit wire limit.
const MAX_DECLARATION_PATH_UNITS: usize = 1024;

/// The declaration-path grammar is portable even on a host that permits these names.
pub fn declaration_path(value: &str) -> Result<(), String> {
    let drive_rooted = value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && value.as_bytes().get(1) == Some(&b':')
        && value.as_bytes().get(2) == Some(&b'/');
    if value.encode_utf16().count() > MAX_DECLARATION_PATH_UNITS
        || drive_rooted
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("Execution input must be a safe project-relative path.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::path_text;

    // reason: only Unix allows '\' in file names.
    #[cfg(unix)]
    #[test]
    fn literal_backslash_in_a_file_name_is_not_a_separator() {
        let root = crate::test_os::tempdir();
        let path = crate::test_os::file_with_backslash(root.path());
        assert_eq!(
            path_text(std::path::Path::new(path.file_name().unwrap())),
            r"input\name"
        );
    }

    // Windows supports prefixes and roots that are not absolute in Rust's Path grammar.
    #[cfg(windows)]
    #[test]
    fn pinned_owners_refuse_drive_relative_and_rooted_paths() {
        use crate::scope::{Artifact, open_input};
        use std::{collections::BTreeMap, path::PathBuf};
        let root = crate::test_os::tempdir();
        for path in [r"C:owner", r"\owner"] {
            let owner = Artifact {
                path: PathBuf::from(path).into(),
                name: "owner".into(),
                nested: BTreeMap::new(),
                covering_part: None,
            };
            assert!(
                open_input(root.path(), &owner, "")
                    .unwrap_err()
                    .to_string()
                    .contains("relative to its declared root")
            );
        }
    }

    // Only the Windows verbatim namespace preserves trailing dots in a file name.
    #[cfg(windows)]
    #[test]
    fn a_pin_with_a_win32_alias_is_not_returned_as_an_ordinary_path() {
        use crate::scope::{open_scoped, scoped_path};
        let root = crate::test_os::tempdir();
        let root = crate::test_os::trailing_dot_alias(root.path());
        assert!(open_scoped(&root, "literal.").is_ok());
        assert!(
            scoped_path(&root, std::path::Path::new("literal."))
                .unwrap_err()
                .to_string()
                .contains("ordinary Windows path")
        );
    }

    // reason: Windows native paths use backslash separators.
    #[cfg(windows)]
    #[test]
    fn native_separators_are_rendered_as_slashes() {
        assert_eq!(
            path_text(std::path::Path::new(r"C:\input\name")),
            "C:/input/name"
        );
    }

    // reason: Windows canonical paths can carry verbatim disk and UNC prefixes.
    #[cfg(windows)]
    #[test]
    fn verbatim_paths_render_without_the_native_prefix() {
        assert_eq!(
            path_text(std::path::Path::new(r"\\?\C:\input\name")),
            "C:/input/name"
        );
        assert_eq!(
            path_text(std::path::Path::new(r"\\?\UNC\server\share\input")),
            "//server/share/input"
        );
    }
}
