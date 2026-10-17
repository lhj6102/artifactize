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
