use std::{fs::File, io::Read, path::Path};

use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::STANDARD},
};

use crate::scope;

use super::Content;

/// Bound decoded image memory and base64 provider payloads while allowing review screenshots.
pub const IMAGE_LIMIT: usize = 4 * 1024 * 1024;
/// The eight bytes every PNG file starts with.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// The first chunk header of a PNG: IHDR with its fixed 13-byte length.
const PNG_IHDR_HEADER: &[u8] = b"\0\0\0\rIHDR";
/// A PNG chunk header: a 4-byte big-endian data length and a 4-byte chunk type.
const PNG_CHUNK_HEADER_BYTES: usize = 8;
/// PNG chunk data lengths are encoded as one big-endian u32.
const PNG_LENGTH_BYTES: usize = std::mem::size_of::<u32>();
/// Base64 encodes each group of three bytes as four ASCII characters.
const BASE64_INPUT_BYTES: usize = 3;
const BASE64_OUTPUT_BYTES: usize = 4;
/// A PNG chunk's bytes besides its data: the header and a 4-byte CRC.
const PNG_CHUNK_OVERHEAD_BYTES: usize = 12;
/// The chunk type that marks an animated PNG, which reviews do not accept.
const PNG_ANIMATION_CONTROL: &[u8] = b"acTL";
/// The chunk type that ends a PNG.
const PNG_END: &[u8] = b"IEND";
/// A JPEG start-of-image marker followed by the first byte of the next marker.
const JPEG_START: &[u8] = b"\xff\xd8\xff";
/// The marker code after `JPEG_START` that means JPEG-LS, which is not a baseline JPEG.
const JPEG_LS_MARKER: u8 = 0xf7;
/// A WebP file is a RIFF container whose form type, at bytes 8 to 12, is `WEBP`.
const RIFF_SIGNATURE: &[u8] = b"RIFF";
const WEBP_FORM: &[u8] = b"WEBP";
const RIFF_FORM_TYPE: std::ops::Range<usize> = 8..12;

pub fn from_base64(data: &str, mime_type: &str) -> Result<Content, String> {
    if data.len() > IMAGE_LIMIT.div_ceil(BASE64_INPUT_BYTES) * BASE64_OUTPUT_BYTES {
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
    let relative = crate::files::logical_below(root, path)
        .ok_or("Image path must be a UTF-8 path below the tool output directory.")?;
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
    if bytes.starts_with(JPEG_START) && bytes.get(JPEG_START.len()) != Some(&JPEG_LS_MARKER) {
        Some("image/jpeg")
    } else if bytes.starts_with(PNG_SIGNATURE) {
        let ihdr = PNG_SIGNATURE.len()..PNG_SIGNATURE.len() + PNG_IHDR_HEADER.len();
        if bytes.get(ihdr) != Some(PNG_IHDR_HEADER) {
            return None;
        }
        // Scan chunk boundaries, not image payloads, for animation control.
        let mut offset = PNG_SIGNATURE.len();
        while offset < bytes.len() {
            let header = bytes.get(offset..offset + PNG_CHUNK_HEADER_BYTES)?;
            let (length, kind) = header.split_at(PNG_LENGTH_BYTES);
            let length = u32::from_be_bytes(length.try_into().ok()?) as usize;
            let end = offset
                .checked_add(PNG_CHUNK_OVERHEAD_BYTES)?
                .checked_add(length)?;
            if end > bytes.len() || kind == PNG_ANIMATION_CONTROL {
                return None;
            }
            if kind == PNG_END {
                return Some("image/png");
            }
            offset = end;
        }
        None
    } else if bytes.starts_with(RIFF_SIGNATURE) && bytes.get(RIFF_FORM_TYPE) == Some(WEBP_FORM) {
        Some("image/webp")
    } else {
        None
    }
}
