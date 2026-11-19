//! Decode native command output before interpreting terminal controls.

use std::borrow::Cow;

/// A program's captured output as text: UTF-8 or Windows UTF-16, its CRLF line endings as `\n`.
pub fn output(bytes: &[u8]) -> Cow<'_, str> {
    let text = decode(bytes);
    if text.contains("\r\n") {
        Cow::Owned(text.replace("\r\n", "\n"))
    } else {
        text
    }
}

/// The UTF-8 byte-order mark some Windows programs write before their output; it is no text.
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

fn decode(bytes: &[u8]) -> Cow<'_, str> {
    #[cfg(windows)]
    if let Some(text) = utf16_output(bytes) {
        return Cow::Owned(text);
    }
    String::from_utf8_lossy(bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes))
}

/// BOM-less UTF-16LE is recognized only when more than this share of its code units are ASCII
/// characters with a zero high byte: cmd's Unicode mode writes mostly ASCII text that way, and
/// a lower share would mistake ordinary bytes for UTF-16.
#[cfg(windows)]
const UTF16_ASCII_MAJORITY_DIVISOR: usize = 2;

#[cfg(windows)]
fn utf16_output(bytes: &[u8]) -> Option<String> {
    let (bytes, little_endian) = if let Some(bytes) = bytes.strip_prefix(b"\xff\xfe") {
        (bytes, true)
    } else if let Some(bytes) = bytes.strip_prefix(b"\xfe\xff") {
        (bytes, false)
    } else {
        // cmd's Unicode mode can emit BOM-less UTF-16LE. Recognize the
        // alternating NULs in its ASCII text without guessing on arbitrary bytes.
        let (units, _) = bytes.as_chunks::<2>();
        let ascii = units
            .iter()
            .filter(|[low, high]| *low != 0 && low.is_ascii() && *high == 0)
            .count();
        if ascii <= units.len() / UTF16_ASCII_MAJORITY_DIVISOR || units.contains(&[0, 0]) {
            return None;
        }
        (bytes, true)
    };
    let (units, rest) = bytes.as_chunks::<2>();
    let byte_order = if little_endian {
        u16::from_le_bytes
    } else {
        u16::from_be_bytes
    };
    Some(
        char::decode_utf16(units.iter().copied().map(byte_order))
            .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
            .chain((!rest.is_empty()).then_some(char::REPLACEMENT_CHARACTER))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_output_normalizes_crlf() {
        assert_eq!(output(b"first\r\nsecond\n"), "first\nsecond\n");
    }

    // Only Windows command output uses the alternate UTF-16 encodings.
    #[cfg(windows)]
    #[test]
    fn native_utf16_text_keeps_unicode_and_line_boundaries() {
        let utf16le = "help: 한글\r\n".encode_utf16().flat_map(u16::to_le_bytes);
        let bom: Vec<_> = [0xff, 0xfe].into_iter().chain(utf16le.clone()).collect();
        assert_eq!(output(&bom), "help: 한글\n");
        assert_eq!(output(&utf16le.collect::<Vec<_>>()), "help: 한글\n");
    }
}
