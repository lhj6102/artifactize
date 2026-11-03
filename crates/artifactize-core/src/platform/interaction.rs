//! Portable boundaries for path identity and owner-only scratch files.
use std::{io, path::Path, sync::OnceLock};

/// Compare opened identities, not spelling: case aliases and system directory aliases
/// on case-insensitive volumes must name the same repository.
pub(crate) fn paths_equal(left: &Path, right: &Path) -> bool {
    let identity = |path: &Path| {
        super::open_directory(path)
            .or_else(|_| super::open_regular(path))
            .and_then(|file| super::file_identity(&file))
    };
    match (identity(left), identity(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// How a system spells one path in different ways, for paths compared as text: a record
/// that names a directory may no longer be openable, and another system's paths never are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathRules {
    /// Paths are the same only byte for byte.
    Unix,
    /// Either separator, any letter case and the verbatim `\\?\` prefix name the same path.
    Windows,
}
impl PathRules {
    pub(crate) const NATIVE: Self = if cfg!(windows) {
        Self::Windows
    } else {
        Self::Unix
    };
    pub(crate) fn same(self, left: &Path, right: &Path) -> bool {
        match self {
            Self::Unix => left == right,
            Self::Windows => windows_spelling(left) == windows_spelling(right),
        }
    }
}

fn windows_spelling(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = match text.strip_prefix(r"\\?\UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned(),
    };
    // A trailing separator names the same folder, except the one that makes a drive's root.
    let trimmed = text.trim_end_matches('\\');
    let text = if trimmed.ends_with(':') {
        &text
    } else {
        trimmed
    };
    text.to_lowercase()
}

/// Include the root itself; compare ancestor identities to honor volume semantics.
pub(crate) fn is_within(path: &Path, root: &Path) -> bool {
    path.ancestors().any(|ancestor| paths_equal(ancestor, root))
}

/// The host label used in provider-facing diagnostics and user agents.
pub(crate) fn label() -> &'static str {
    static LABEL: OnceLock<String> = OnceLock::new();
    LABEL.get_or_init(|| format!("{} {}", std::env::consts::OS, std::env::consts::ARCH))
}

/// An unnamed owner-only scratch file in the system temporary directory, removed when
/// closed.
pub(crate) fn private_anonymous_file() -> io::Result<std::fs::File> {
    let file = tempfile::tempfile()?;
    super::restrict_file(&file)?;
    Ok(file)
}
