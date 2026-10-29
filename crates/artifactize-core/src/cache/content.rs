//! Built-in content fingerprint: the hash of an Artifact's own input files.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::File,
    io::Read,
    path::PathBuf,
};

use ignore::{
    Match,
    gitignore::{Gitignore, GitignoreBuilder},
};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::{
    config::RepoConfig,
    platform::{self, FileKind},
    process, scope,
    scope::LogicalTarget,
};

/// Bound input traversal and retained per-file digest metadata for one fingerprint.
const MAX_ENTRIES: usize = 10_000;
/// Bound total hashing I/O so one undeclared large input cannot monopolize preparation.
const MAX_BYTES: u64 = 1024 * 1024 * 1024;
/// Bound inherited ignore-file parsing while supporting substantial repository rules.
const GITIGNORE_BYTES: u64 = 1024 * 1024;
/// Bound user-declared ignore fan-out and compiled matcher work during fingerprinting.
const MAX_IGNORE_PATTERNS: usize = 64;
/// Stream hashes in bounded heap memory while amortizing filesystem read calls.
/// Content fingerprints and executable pins use the same I/O chunk, not a file-size cap.
pub(crate) const HASH_BUFFER_BYTES: usize = 64 * 1024;
const BUILTIN_IGNORES: [&str; 5] = [".git", "__pycache__/", "*.pyc", "target/", "node_modules/"];

/// Sorted owner-relative file paths with their SHA-256, and one digest over all of them.
pub(super) struct Files {
    pub digest: crate::types::Sha256Digest,
    pub files: BTreeMap<LogicalTarget, crate::types::Sha256Bytes>,
}

/// Declared `fingerprint.ignore` globs, parsed once from the declaration together with their
/// matcher, which always excludes the built-ins too.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct IgnorePatterns {
    patterns: Vec<String>,
    matcher: Gitignore,
}

impl IgnorePatterns {
    /// The globs as declared.
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }
}

impl Default for IgnorePatterns {
    /// No declared globs: only the built-ins.
    fn default() -> Self {
        Self::try_from(Vec::new()).expect("built-in ignore patterns")
    }
}

impl TryFrom<Vec<String>> for IgnorePatterns {
    type Error = String;

    fn try_from(patterns: Vec<String>) -> Result<Self, String> {
        let matcher = matcher(&patterns)?;
        Ok(Self { patterns, matcher })
    }
}

impl From<IgnorePatterns> for Vec<String> {
    fn from(ignore: IgnorePatterns) -> Self {
        ignore.patterns
    }
}

fn matcher(patterns: &[String]) -> Result<Gitignore, String> {
    if patterns.len() > MAX_IGNORE_PATTERNS
        || patterns.iter().collect::<BTreeSet<_>>().len() != patterns.len()
    {
        return Err("fingerprint.ignore must contain at most 64 unique patterns.".into());
    }
    let mut builder = GitignoreBuilder::new(".");
    // Match actual entry spellings on every volume: a differently cased rule must never
    // widen the ignored set and silently hide a content change.
    builder
        .case_insensitive(false)
        .expect("case-sensitive ignore matching");
    for pattern in BUILTIN_IGNORES {
        builder.add_line(None, pattern).expect("built-in pattern");
    }
    for pattern in patterns {
        if pattern.trim().is_empty()
            || pattern.starts_with(['!', '#'])
            || pattern.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(
                "fingerprint.ignore patterns must be nonblank .gitignore globs without negation or comments."
                    .into(),
            );
        }
        builder
            .add_line(None, pattern)
            .map_err(|error| format!("fingerprint.ignore: {error}"))?;
    }
    builder.build().map_err(|error| error.to_string())
}

/// Hash an Artifact's own inputs off the async runtime.
pub(super) async fn files(
    config: &RepoConfig,
    id: &crate::types::ArtifactName,
    inputs: &[crate::config::LogicalPath],
    ignore: &IgnorePatterns,
    cancellation: &CancellationToken,
) -> Result<Files, String> {
    if cancellation.is_cancelled() {
        return Err(process::Error::Cancelled.to_string());
    }
    let walk = Walk::new(config, id, inputs, ignore)?;
    let token = cancellation.clone();
    crate::task::joined(tokio::task::spawn_blocking(move || walk.digest(&token)).await)
        .map_err(|error| error.to_string())?
}

struct Walk {
    root: PathBuf,
    owner: PathBuf,
    /// The owner folder relative to the repository root, where `.gitignore` bases start.
    prefix: LogicalTarget,
    inputs: Vec<LogicalTarget>,
    file: bool,
    /// Child Artifact folders and declaration files, skipped while walking.
    excluded: BTreeSet<crate::config::ChildPrefix>,
    ignore: Gitignore,
}

#[derive(Default)]
struct State {
    files: BTreeMap<LogicalTarget, crate::types::Sha256Bytes>,
    entries: usize,
    bytes: u64,
}

impl Walk {
    fn new(
        config: &RepoConfig,
        id: &crate::types::ArtifactName,
        inputs: &[crate::config::LogicalPath],
        ignore: &IgnorePatterns,
    ) -> Result<Self, String> {
        let artifact = &config.artifacts[id];
        let owner =
            scope::scoped_path(&config.root, artifact.folder()).map_err(|e| e.to_string())?;
        // Preserve the UTF-8 input contract before rendering portable separators.
        artifact
            .folder()
            .to_str()
            .ok_or("Artifact paths must be UTF-8.")?;
        let prefix = platform::path_text(artifact.folder())
            .parse::<LogicalTarget>()
            .map_err(|error| error.to_string())?;
        let excluded = artifact.nested.keys().cloned().collect();
        let inputs = inputs
            .iter()
            .map(|input| {
                if input.as_str() == "." {
                    Ok(LogicalTarget::Root)
                } else {
                    input
                        .as_str()
                        .parse::<LogicalTarget>()
                        .map_err(|error| error.to_string())
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            root: config.root.clone(),
            owner,
            prefix,
            inputs,
            file: artifact.file_name().is_some(),
            excluded,
            ignore: ignore.matcher.clone(),
        })
    }

    fn digest(&self, cancellation: &CancellationToken) -> Result<Files, String> {
        let mut state = State::default();
        for input in &self.inputs {
            let label = if input.is_empty() {
                "."
            } else {
                input.as_str()
            };
            let file =
                scope::open_scoped(&self.owner, input).map_err(|e| format!("{label}: {e}"))?;
            if self.file && !file.metadata().map_err(|e| e.to_string())?.is_file() {
                return Err(format!(
                    "{label}: File Artifact target must remain a regular file."
                ));
            }
            if file.metadata().map_err(|e| e.to_string())?.is_dir() {
                let mut gitignores = self.ancestors(input)?;
                self.directory(&file, input, &mut gitignores, &mut state, cancellation)?;
            } else if !input.ends_with(".artf") {
                hash_file(file, input, &mut state)?;
            }
        }
        let mut digest = Sha256::new();
        for (path, file) in &state.files {
            digest.update(path.as_bytes());
            digest.update([0]);
            digest.update(file.as_bytes());
        }
        Ok(Files {
            digest: hex(&digest.finalize())
                .parse()
                .expect("SHA-256 input digest"),
            files: state.files,
        })
    }

    fn directory(
        &self,
        directory: &File,
        path: &LogicalTarget,
        gitignores: &mut Vec<(LogicalTarget, Gitignore)>,
        state: &mut State,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        let label = if path.is_empty() { "." } else { path.as_str() };
        // Enumerate the pinned directory, not a path that could have been replaced by a link.
        let mut entries = Vec::new();
        for entry in platform::read_dir(directory).map_err(|e| format!("{label}: {e}"))? {
            let entry = entry.map_err(|e| format!("{label}: {e}"))?;
            let kind = entry.file_type().map_err(|e| format!("{label}: {e}"))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Artifact paths must be UTF-8.")?;
            entries.push((name, kind == FileKind::Directory));
        }
        entries.sort();
        let gitignore =
            read_gitignore(directory).map_err(|e| format!("{}: {e}", gitignore_name(path)))?;
        let pushed = gitignore.is_some();
        if let Some(gitignore) = gitignore {
            gitignores.push((join(&self.prefix, path)?, gitignore));
        }
        for (name, is_dir) in entries {
            if cancellation.is_cancelled() {
                return Err(process::Error::Cancelled.to_string());
            }
            let child = join(path, &name)?;
            if (!is_dir && name.ends_with(".artf"))
                || self.excluded.contains(child.as_str())
                || self.ignored(&child, is_dir, gitignores)?
            {
                continue;
            }
            state.entries += 1;
            if state.entries > MAX_ENTRIES {
                return Err(format!(
                    "Content inputs exceed {MAX_ENTRIES} entries; narrow fingerprint.files or add fingerprint.ignore."
                ));
            }
            let entry = scope::open_child(directory, OsStr::new(&name))
                .map_err(|e| format!("{child}: {e} Add it to fingerprint.ignore to skip it."))?;
            if entry.metadata().map_err(|e| e.to_string())?.is_dir() {
                self.directory(&entry, &child, gitignores, state, cancellation)?;
            } else {
                hash_file(entry, &child, state)?;
            }
        }
        if pushed {
            gitignores.pop();
        }
        Ok(())
    }

    /// `.gitignore` files from the repository root down to a walked input's parent folder.
    fn ancestors(&self, input: &LogicalTarget) -> Result<Vec<(LogicalTarget, Gitignore)>, String> {
        let full = join(&self.prefix, input)?;
        let parts: Vec<_> = full.split('/').filter(|part| !part.is_empty()).collect();
        let mut gitignores = Vec::new();
        for depth in 0..parts.len() {
            let base: LogicalTarget = parts[..depth]
                .join("/")
                .parse()
                .expect("ancestors of a validated logical path");
            let label = |e: String| format!("{}: {e}", gitignore_name(&base));
            let directory =
                scope::open_scoped(&self.root, &base).map_err(|e| label(e.to_string()))?;
            if let Some(gitignore) = read_gitignore(&directory).map_err(label)? {
                gitignores.push((base, gitignore));
            }
        }
        Ok(gitignores)
    }

    /// Built-ins and declared globs always exclude; `.gitignore` files decide the rest,
    /// each relative to its own folder and the deepest match winning, as in git.
    fn ignored(
        &self,
        path: &LogicalTarget,
        is_dir: bool,
        gitignores: &[(LogicalTarget, Gitignore)],
    ) -> Result<bool, String> {
        if self.ignore.matched(path, is_dir).is_ignore() {
            return Ok(true);
        }
        let full = join(&self.prefix, path)?;
        for (base, gitignore) in gitignores.iter().rev() {
            let relative = if base.is_empty() {
                full.as_str()
            } else {
                &full[base.len() + 1..]
            };
            match gitignore.matched(relative, is_dir) {
                Match::Ignore(_) => return Ok(true),
                Match::Whitelist(_) => return Ok(false),
                Match::None => {}
            }
        }
        Ok(false)
    }
}

/// A pinned folder's `.gitignore`, if any. Lines git would reject are skipped, as git skips them.
fn read_gitignore(directory: &File) -> Result<Option<Gitignore>, String> {
    match platform::entry_kind(directory, OsStr::new(".gitignore")) {
        Ok(FileKind::Directory) => return Ok(None),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let file = scope::open_child(directory, OsStr::new(".gitignore")).map_err(|e| e.to_string())?;
    let mut text = Vec::new();
    file.take(GITIGNORE_BYTES + 1)
        .read_to_end(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 > GITIGNORE_BYTES {
        return Err("A .gitignore file exceeds 1 MiB.".into());
    }
    let mut builder = GitignoreBuilder::new(".");
    // Match actual entry spellings on every volume: a differently cased rule must never
    // widen the ignored set and silently hide a content change.
    builder
        .case_insensitive(false)
        .expect("case-sensitive ignore matching");
    for line in String::from_utf8_lossy(&text).lines() {
        let _ = builder.add_line(None, line);
    }
    builder.build().map(Some).map_err(|e| e.to_string())
}

fn hash_file(mut file: File, path: &LogicalTarget, state: &mut State) -> Result<(), String> {
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(format!("{path}: Content input must be a regular file."));
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0; HASH_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(|e| format!("{path}: {e}"))?;
        if read == 0 {
            break;
        }
        state.bytes += read as u64;
        if state.bytes > MAX_BYTES {
            return Err(
                "Content inputs exceed 1 GiB; narrow fingerprint.files or add fingerprint.ignore."
                    .into(),
            );
        }
        digest.update(&buffer[..read]);
    }
    state.files.insert(
        path.clone(),
        crate::types::Sha256Bytes::from_digest(digest.finalize().into()),
    );
    Ok(())
}

/// The `.gitignore` of a folder, for messages: its logical path, or when that would be too
/// long to be one, the folder and the name as written.
fn gitignore_name(folder: &LogicalTarget) -> String {
    join(folder, ".gitignore")
        .map_or_else(|_| format!("{folder}/.gitignore"), |path| path.to_string())
}

fn join(base: &LogicalTarget, name: &str) -> Result<LogicalTarget, String> {
    if name.is_empty() {
        Ok(base.clone())
    } else {
        let path = if base.is_empty() {
            name.to_owned()
        } else {
            format!("{base}/{name}")
        };
        path.parse::<LogicalTarget>()
            .map_err(|error| error.to_string())
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
