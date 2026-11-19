//! Bounded image reads at the scoped filesystem edge.

use std::{fs::File, io::Read, path::Path};

use super::IMAGE_LIMIT;
use crate::scope;

/// Read one extra byte to detect a file growing past the limit after the metadata check.
const GROWTH_PROBE_BYTES: usize = 1;

pub(crate) fn read_output(root: &Path, path: &Path) -> Result<Vec<u8>, String> {
    let invalid = "Image path must be a path below the tool output directory.";
    // One native path, absolute below the output directory or relative to it, parsed once
    // into its validated names by the scoped opener.
    let relative = if crate::files::is_absolute(path) {
        crate::files::relative_below(root, path).ok_or(invalid)?
    } else {
        path
    };
    let file = scope::open_native_scoped(root, relative)
        .map_err(|e| format!("Image {}: {e}", crate::files::path_text(path)))?;
    read(file)
}

pub(crate) fn read(file: File) -> Result<Vec<u8>, String> {
    let metadata = file.metadata().map_err(|_| "Cannot inspect image file.")?;
    if !metadata.is_file() {
        return Err("Viewing an image requires a regular file.".into());
    }
    if metadata.len() == 0 || metadata.len() > IMAGE_LIMIT as u64 {
        return Err("Image exceeds the 4 MiB limit or contains no data.".into());
    }
    let mut bytes = Vec::new();
    file.take((IMAGE_LIMIT + GROWTH_PROBE_BYTES) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read image file.")?;
    Ok(bytes)
}
