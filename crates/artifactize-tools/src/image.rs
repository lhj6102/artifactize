use std::{fs::File, io::Read, path::Path};

use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::STANDARD},
};

use crate::scope;

use super::Content;

/// Bound decoded image memory and base64 provider payloads while allowing review screenshots.
pub const IMAGE_LIMIT: usize = 4 * 1024 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub fn from_base64(data: &str, mime_type: &str) -> Result<Content, String> {
    if data.len() > IMAGE_LIMIT.div_ceil(3) * 4 {
        return Err("Image exceeds the 4 MiB limit.".into());
    }
    let decoder = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    let bytes = decoder
        .decode(data)
        .map_err(|_| "Image requires valid base64 data.")?;
    normalize(&bytes, Some(mime_type))
}

pub fn from_output(root: &Path, path: &str, mime_type: &str) -> Result<Content, String> {
    let path = Path::new(path);
    let relative = if path.is_absolute() {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "Image is outside the tool output directory.")?;
        scope::logical_from_native(relative)
            .ok_or("Image path must be a UTF-8 path below the tool output directory.")?
    } else {
        path.to_str().ok_or("Image path must be UTF-8.")?.to_owned()
    };
    let file = scope::open_scoped(root, &relative).map_err(|e| format!("Image {relative}: {e}"))?;
    normalize(&read(file)?, Some(mime_type))
}

pub fn read(file: File) -> Result<Vec<u8>, String> {
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

pub fn normalize(bytes: &[u8], declared: Option<&str>) -> Result<Content, String> {
    if bytes.is_empty() || bytes.len() > IMAGE_LIMIT {
        return Err("Image exceeds the 4 MiB limit or contains no data.".into());
    }
    let mime_type = mime_type(bytes)
        .ok_or("Image must be PNG, JPEG or WebP; GIF, BMP and animated PNG are not supported.")?;
    if declared.is_some_and(|declared| declared != mime_type) {
        return Err("Image bytes do not match declared MIME type.".into());
    }
    Ok(Content::Image {
        data: STANDARD.encode(bytes),
        mime_type: mime_type.into(),
    })
}

fn mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\xff\xd8\xff") && bytes.get(3) != Some(&0xf7) {
        Some("image/jpeg")
    } else if bytes.starts_with(PNG_SIGNATURE) {
        if bytes.get(8..16) != Some(b"\0\0\0\rIHDR") {
            return None;
        }
        // Scan chunk boundaries, not image payloads, for animation control.
        let mut offset = PNG_SIGNATURE.len();
        while offset < bytes.len() {
            let header = bytes.get(offset..offset + 8)?;
            let length = u32::from_be_bytes(header[..4].try_into().ok()?) as usize;
            let end = offset.checked_add(12)?.checked_add(length)?;
            if end > bytes.len() || &header[4..] == b"acTL" {
                return None;
            }
            if &header[4..] == b"IEND" {
                return Some("image/png");
            }
            offset = end;
        }
        None
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}
