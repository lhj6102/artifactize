//! Shared safe file access: pinned no-follow opens and directory listings.

#[cfg(any(windows, test))]
mod directory_scan;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{
    EntryName, canonicalize, case_sensitive, entry_kind, exact_name, is_link_refusal,
    open_directory, open_entry, open_no_follow, open_nonblocking, read_dir,
};
#[cfg(windows)]
pub use windows::{
    EntryName, canonicalize, case_sensitive, entry_kind, exact_name, is_link_refusal,
    open_directory, open_entry, open_no_follow, open_nonblocking, read_dir,
};

/// The filesystem root of an absolute path (`/`, or a Windows volume or share) and the names
/// below it, in order. `None` when a component is `.` or `..`.
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

/// Render a native path for people and JSON clients with portable `/` separators.
pub fn path_text(path: &std::path::Path) -> String {
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
    use std::path::Component;
    path.components()
        .map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|names| names.join("/"))
}

/// Parse the physical relative input form used by declared Artifact roots. Preserve its
/// existing rejection of backslashes and traversal while keeping native root checks here.
pub fn scoped_relative(path: &std::path::Path) -> std::io::Result<String> {
    let path = path.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Artifact paths must be UTF-8.",
        )
    })?;
    if path.contains(['\0', '\\'])
        || std::path::Path::new(path).is_absolute()
        || path.split('/').any(|part| part == "." || part == "..")
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Artifact path must be relative to its declared root.",
        ));
    }
    Ok(path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/"))
}

#[cfg(test)]
mod tests {
    use super::path_text;

    // reason: only Unix allows '\' in file names.
    #[cfg(unix)]
    #[test]
    fn literal_backslash_in_a_file_name_is_not_a_separator() {
        let root = crate::test_os::tempdir();
        let path = root.path().join(r"input\name");
        std::fs::write(&path, "data").unwrap();
        assert_eq!(
            path_text(std::path::Path::new(path.file_name().unwrap())),
            r"input\name"
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
