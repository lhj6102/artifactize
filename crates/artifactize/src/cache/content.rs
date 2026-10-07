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
    config::{CONFIG_FILE, RepoConfig},
    platform::{self, FileKind},
    process, scope,
};

/// The built-in Agent tools' traversal bound, and the cache's retained-byte cap.
const MAX_ENTRIES: usize = 10_000;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const GITIGNORE_BYTES: u64 = 1024 * 1024;
/// Bound user-declared ignore fan-out and compiled matcher work during fingerprinting.
const MAX_IGNORE_PATTERNS: usize = 64;
/// Stream hashes in bounded heap memory while amortizing filesystem read calls.
/// Content fingerprints and executable pins use the same I/O chunk, not a file-size cap.
pub(crate) const HASH_BUFFER_BYTES: usize = 64 * 1024;
const BUILTIN_IGNORES: [&str; 5] = [".git", "__pycache__/", "*.pyc", "target/", "node_modules/"];

/// Sorted owner-relative file paths with their SHA-256, and one digest over all of them.
pub(super) struct Files {
    pub digest: String,
    pub files: BTreeMap<String, [u8; 32]>,
}

/// Validate declared `fingerprint.ignore` globs, which always exclude like the built-ins.
pub(crate) fn ignore_patterns(patterns: &[String]) -> Result<(), String> {
    matcher(patterns).map(drop)
}

fn matcher(patterns: &[String]) -> Result<Gitignore, String> {
    if patterns.len() > MAX_IGNORE_PATTERNS
        || patterns.iter().collect::<BTreeSet<_>>().len() != patterns.len()
    {
        return Err("fingerprint.ignore must contain at most 64 unique patterns.".into());
    }
    let mut builder = GitignoreBuilder::new(".");
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
    id: &str,
    inputs: &[String],
    ignore: &[String],
    cancellation: &CancellationToken,
) -> Result<Files, String> {
    if cancellation.is_cancelled() {
        return Err(process::Error::Cancelled.to_string());
    }
    let walk = Walk::new(config, id, inputs, ignore)?;
    let token = cancellation.clone();
    tokio::task::spawn_blocking(move || walk.digest(&token))
        .await
        .map_err(|error| error.to_string())?
}

struct Walk {
    root: PathBuf,
    owner: PathBuf,
    /// The owner folder relative to the repository root, where `.gitignore` bases start.
    prefix: String,
    inputs: Vec<String>,
    /// Child Artifact folders, declaration files and family material, skipped while walking.
    excluded: BTreeSet<String>,
    ignore: Gitignore,
}

#[derive(Default)]
struct State {
    files: BTreeMap<String, [u8; 32]>,
    entries: usize,
    bytes: u64,
}

impl Walk {
    fn new(
        config: &RepoConfig,
        id: &str,
        inputs: &[String],
        ignore: &[String],
    ) -> Result<Self, String> {
        let artifact = &config.artifacts[id];
        let owner = scope::scoped_path(&config.root, &artifact.path).map_err(|e| e.to_string())?;
        let prefix = artifact
            .path
            .to_str()
            .ok_or("Artifact paths must be UTF-8.")?
            .to_owned();
        let mut excluded = BTreeSet::from([CONFIG_FILE.to_owned()]);
        for (path, child) in &artifact.children {
            let folder = if config.artifacts[child].family.is_some() {
                path.strip_suffix(&format!("/{child}")).unwrap_or(path)
            } else {
                path
            };
            excluded.insert(folder.to_owned());
        }
        let mut inputs: Vec<_> = inputs
            .iter()
            .map(|input| {
                if input == "." {
                    String::new()
                } else {
                    input.clone()
                }
            })
            .collect();
        if let Some(family) = &artifact.family {
            excluded.extend(family.instances.iter().cloned());
            for sibling in config.artifacts.values() {
                if let Some(membership) = &sibling.family
                    && membership.name == family.name
                {
                    excluded.extend(membership.material.iter().cloned());
                }
            }
            inputs.extend(family.material.iter().cloned());
        }
        Ok(Self {
            root: config.root.clone(),
            owner,
            prefix,
            inputs,
            excluded,
            ignore: matcher(ignore)?,
        })
    }

    fn digest(&self, cancellation: &CancellationToken) -> Result<Files, String> {
        let mut state = State::default();
        for input in &self.inputs {
            let label = if input.is_empty() { "." } else { input };
            let file =
                scope::open_scoped(&self.owner, input).map_err(|e| format!("{label}: {e}"))?;
            if file.metadata().map_err(|e| e.to_string())?.is_dir() {
                let mut gitignores = self.ancestors(input)?;
                self.directory(&file, input, &mut gitignores, &mut state, cancellation)?;
            } else {
                hash_file(file, input, &mut state)?;
            }
        }
        let mut digest = Sha256::new();
        for (path, file) in &state.files {
            digest.update(path.as_bytes());
            digest.update([0]);
            digest.update(file);
        }
        Ok(Files {
            digest: hex(&digest.finalize()),
            files: state.files,
        })
    }

    fn directory(
        &self,
        directory: &File,
        path: &str,
        gitignores: &mut Vec<(String, Gitignore)>,
        state: &mut State,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        let label = if path.is_empty() { "." } else { path };
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
            read_gitignore(directory).map_err(|e| format!("{}: {e}", join(path, ".gitignore")))?;
        let pushed = gitignore.is_some();
        if let Some(gitignore) = gitignore {
            gitignores.push((join(&self.prefix, path), gitignore));
        }
        for (name, is_dir) in entries {
            if cancellation.is_cancelled() {
                return Err(process::Error::Cancelled.to_string());
            }
            let child = join(path, &name);
            if self.excluded.contains(&child) || self.ignored(&child, is_dir, gitignores) {
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
    fn ancestors(&self, input: &str) -> Result<Vec<(String, Gitignore)>, String> {
        let full = join(&self.prefix, input);
        let parts: Vec<_> = full.split('/').filter(|part| !part.is_empty()).collect();
        let mut gitignores = Vec::new();
        for depth in 0..parts.len() {
            let base = parts[..depth].join("/");
            let label = |e: String| format!("{}: {e}", join(&base, ".gitignore"));
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
    fn ignored(&self, path: &str, is_dir: bool, gitignores: &[(String, Gitignore)]) -> bool {
        if self.ignore.matched(path, is_dir).is_ignore() {
            return true;
        }
        let full = join(&self.prefix, path);
        for (base, gitignore) in gitignores.iter().rev() {
            let relative = if base.is_empty() {
                &full
            } else {
                &full[base.len() + 1..]
            };
            match gitignore.matched(relative, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
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
    for line in String::from_utf8_lossy(&text).lines() {
        let _ = builder.add_line(None, line);
    }
    builder.build().map(Some).map_err(|e| e.to_string())
}

fn hash_file(mut file: File, path: &str, state: &mut State) -> Result<(), String> {
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
    state
        .files
        .insert(path.to_owned(), digest.finalize().into());
    Ok(())
}

fn join(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.into()
    } else {
        format!("{base}/{name}")
    }
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
