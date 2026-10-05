//! SHA-256 pins of the files Agent tools execute (`executionPaths`), recorded in the
//! provenance of every Agent result so that results show which binary produced them.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::Registry;
use crate::{
    config::{AgentTool, RepoConfig},
    scope,
};

/// The traversal bounds of one pinned directory, as for content fingerprints.
const MAX_ENTRIES: usize = 10_000;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// Pins by registered tool name, then by declared workspace-relative path.
pub type Pins = BTreeMap<String, BTreeMap<String, String>>;

/// Hash every `executionPaths` entry of the Agent eval's command tools, off the async runtime.
/// A file pins to the SHA-256 of its bytes; a directory to the SHA-256 over each entry's
/// relative path and digest, in path order, where a symlink inside it contributes its target
/// without being followed. A missing or unreadable path, a special file or a directory over
/// 10,000 entries or 1 GiB is an error.
pub async fn execution_paths(config: &RepoConfig, eval_id: &str) -> Result<Pins, String> {
    let registry = Registry::new(config, eval_id)?;
    let mut resolved = Vec::new();
    for (name, tool) in &registry.tools {
        let AgentTool::Command(command) = tool.declaration else {
            continue;
        };
        for path in &command.execution_paths {
            let absolute = scope::scoped_path(&config.root, Path::new(path))
                .map_err(|e| format!("Tool {name} executionPaths {path}: {e}"))?;
            resolved.push((name.clone(), path.clone(), absolute));
        }
    }
    if resolved.is_empty() {
        return Ok(Pins::new());
    }
    tokio::task::spawn_blocking(move || {
        let mut pins = Pins::new();
        for (name, path, absolute) in resolved {
            let digest =
                pin(&absolute).map_err(|e| format!("Tool {name} executionPaths {path}: {e}"))?;
            pins.entry(name).or_default().insert(path, digest);
        }
        Ok(pins)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn pin(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    let mut bytes = 0;
    if metadata.is_file() {
        return Ok(hex(&hash_file(path, &mut bytes)?));
    }
    if !metadata.is_dir() {
        return Err("execution paths must be regular files or directories.".into());
    }
    let mut entries = Vec::new();
    walk(path, PathBuf::new(), &mut entries, &mut bytes)?;
    entries.sort();
    let mut digest = Sha256::new();
    for (relative, kind, entry) in entries {
        digest.update([kind]);
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(entry);
    }
    Ok(hex(&digest.finalize()))
}

/// Collect `(relative path, kind, digest)` for each file (`f`) and symlink (`l`) below `root`.
fn walk(
    root: &Path,
    relative: PathBuf,
    entries: &mut Vec<(String, u8, [u8; 32])>,
    bytes: &mut u64,
) -> Result<(), String> {
    let directory = root.join(&relative);
    for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let child = relative.join(entry.file_name());
        let label = child
            .to_str()
            .ok_or("execution paths must be UTF-8.")?
            .to_owned();
        if entries.len() >= MAX_ENTRIES {
            return Err(format!("more than {MAX_ENTRIES} entries to pin."));
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            walk(root, child, entries, bytes)?;
        } else if kind.is_file() {
            entries.push((label, b'f', hash_file(&root.join(&child), bytes)?));
        } else if kind.is_symlink() {
            let target = fs::read_link(root.join(&child)).map_err(|e| e.to_string())?;
            let target = target.to_str().ok_or("execution paths must be UTF-8.")?;
            entries.push((label, b'l', Sha256::digest(target.as_bytes()).into()));
        } else {
            return Err(format!(
                "{label} is not a regular file, directory or symlink."
            ));
        }
    }
    Ok(())
}

fn hash_file(path: &Path, bytes: &mut u64) -> Result<[u8; 32], String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            return Ok(digest.finalize().into());
        }
        *bytes += read as u64;
        if *bytes > MAX_BYTES {
            return Err("more than 1 GiB to pin.".into());
        }
        digest.update(&buffer[..read]);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
