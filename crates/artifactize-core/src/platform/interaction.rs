//! Portable boundaries for path identity and interactive editing.
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

pub(crate) fn selected_editor() -> String {
    ["VISUAL", "EDITOR"]
        .into_iter()
        .find_map(|name| {
            super::environment::var_text(name).filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| super::DEFAULT_EDITOR.into())
}

/// An editor draft is owner-only even on platforms where tempfile's default ACL inherits.
pub(crate) fn private_temp_file(prefix: &str) -> io::Result<PrivateTempFile> {
    // Build the protected ACL/mode at creation, not after creating a public draft.
    let root = super::canonicalize(&std::env::temp_dir())?;
    let directory = super::private_tempdir_in(prefix, &root)?;
    let file = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(".json")
        .tempfile_in(directory.path())?;
    super::restrict_file(file.as_file())?;
    Ok(PrivateTempFile {
        file,
        _directory: directory,
    })
}

/// An unnamed owner-only scratch file in the system temporary directory, removed when
/// closed.
pub(crate) fn private_anonymous_file() -> io::Result<std::fs::File> {
    let file = tempfile::tempfile()?;
    super::restrict_file(&file)?;
    Ok(file)
}

/// Do not expose signal-specific process statuses outside the platform boundary.
pub(crate) async fn run_editor(command: &str, file: &Path) -> io::Result<()> {
    let status = super::editor(command, file).status().await?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "The editor exited unsuccessfully ({status}); nothing was submitted."
        )))
    }
}

/// Keep the protected parent alive until its editor draft has been removed.
pub(crate) struct PrivateTempFile {
    file: tempfile::NamedTempFile,
    _directory: super::PrivateTempDir,
}
impl PrivateTempFile {
    pub fn path(&self) -> &Path {
        self.file.path()
    }
}
