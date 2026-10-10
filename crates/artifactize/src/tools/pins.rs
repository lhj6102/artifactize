//! SHA-256 pins of the files Agent tools execute (`executionPaths`), recorded in the
//! provenance of every Agent result so that results show which binary produced them.

use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

use sha2::{Digest, Sha256};

use super::Registry;
use crate::{
    config::{AgentTool, RepoConfig},
    platform, scope,
};

/// Bound traversal and retained digest metadata for a tool's executable directory pin.
const MAX_ENTRIES: usize = 10_000;
/// Bound total pinning I/O independently of entry count, as for content fingerprints.
const MAX_BYTES: u64 = 1024 * 1024 * 1024;

/// Pins by registered tool name, then by declared workspace-relative path.
pub type Pins = BTreeMap<
    crate::config::ToolName,
    BTreeMap<crate::config::LogicalPath, crate::types::Sha256Digest>,
>;

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
            pins.entry(name)
                .or_default()
                .insert(path, digest.parse().expect("SHA-256 pin digest"));
        }
        Ok(pins)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn pin(path: &Path) -> Result<String, String> {
    let kind = platform::path_kind(path).map_err(|e| e.to_string())?;
    let mut bytes = 0;
    if kind == platform::FileKind::File {
        let file = platform::open_regular(path).map_err(|e| e.to_string())?;
        return Ok(hex(&hash_file(file, &mut bytes)?));
    }
    if kind != platform::FileKind::Directory {
        return Err("execution paths must be regular files or directories.".into());
    }
    let directory = platform::open_directory(path).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    walk(&directory, path, "", &mut entries, &mut bytes)?;
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

/// Collect `(logical path, kind, digest)` for each file (`f`) and link (`l`) below the
/// pinned `directory`, which `path` names; entries are opened relative to their parent
/// without following links.
fn walk(
    directory: &File,
    path: &Path,
    relative: &str,
    entries: &mut Vec<(String, u8, [u8; 32])>,
    bytes: &mut u64,
) -> Result<(), String> {
    for entry in platform::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let text = name.to_str().ok_or("execution paths must be UTF-8.")?;
        let label = if relative.is_empty() {
            text.to_owned()
        } else {
            format!("{relative}/{text}")
        };
        if entries.len() >= MAX_ENTRIES {
            return Err(format!("more than {MAX_ENTRIES} entries to pin."));
        }
        let open = || {
            let entry_name = platform::EntryName::new(&name).ok_or("invalid entry name.")?;
            platform::open_entry(directory, &entry_name).map_err(|e| e.to_string())
        };
        match entry.file_type().map_err(|e| e.to_string())? {
            platform::FileKind::Directory => {
                walk(&open()?, &path.join(&name), &label, entries, bytes)?;
            }
            platform::FileKind::File => {
                entries.push((label, b'f', hash_file(open()?, bytes)?));
            }
            platform::FileKind::Symlink => {
                let target = platform::link_target(&path.join(&name)).map_err(|e| e.to_string())?;
                target.to_str().ok_or("execution paths must be UTF-8.")?;
                let target = platform::path_text(&target);
                entries.push((label, b'l', Sha256::digest(target.as_bytes()).into()));
            }
            platform::FileKind::Other => {
                return Err(format!(
                    "{label} is not a regular file, directory or symlink."
                ));
            }
        }
    }
    Ok(())
}

fn hash_file(mut file: File, bytes: &mut u64) -> Result<[u8; 32], String> {
    let mut digest = Sha256::new();
    let mut buffer = vec![0; crate::cache::HASH_BUFFER_BYTES];
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
