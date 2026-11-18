//! Bounded image reads at the scoped filesystem edge.

use std::{fs::File, io::Read, path::Path};

use super::IMAGE_LIMIT;
use crate::scope;

pub(crate) fn read_output(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let relative = crate::files::logical_below(root, path)
        .ok_or("Image path must be a UTF-8 path below the tool output directory.")?;
    let file = scope::open_scoped(root, &relative)
        .map_err(|e| format!("Image {}: {e}", crate::files::path_text(&relative)))?;
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
    file.take((IMAGE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read image file.")?;
    Ok(bytes)
}
