//! Strip terminal escape sequences and control bytes from captured tool output.

/// ECMA-48 control sequence bytes: after `ESC [`, parameter bytes, then intermediate bytes,
/// then one final byte end the sequence.
const CSI_PARAMETER: std::ops::RangeInclusive<u8> = 0x30..=0x3f;
const CSI_INTERMEDIATE: std::ops::RangeInclusive<u8> = 0x20..=0x2f;
const CSI_FINAL: std::ops::RangeInclusive<u8> = 0x40..=0x7e;

/// The two-byte prefix introducing an ECMA-48 control sequence.
const CSI_PREFIX: &[u8] = b"\x1b[";
/// C0 controls except tab, line feed and carriage return are not printable tool output.
const C0_CONTROLS: &[std::ops::RangeInclusive<u8>] = &[0..=8, 11..=12, 14..=31];

pub fn clean_output(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let mut bytes = text.as_bytes();
    let mut clean = Vec::with_capacity(bytes.len());
    while let Some((&byte, tail)) = bytes.split_first() {
        if bytes.starts_with(CSI_PREFIX) {
            let parameters = bytes[CSI_PREFIX.len()..]
                .iter()
                .take_while(|byte| CSI_PARAMETER.contains(byte))
                .count();
            let intermediate_start = CSI_PREFIX.len() + parameters;
            let intermediates = bytes[intermediate_start..]
                .iter()
                .take_while(|byte| CSI_INTERMEDIATE.contains(byte))
                .count();
            let end = intermediate_start + intermediates;
            if bytes.get(end).is_some_and(|byte| CSI_FINAL.contains(byte)) {
                bytes = &bytes[end + 1..];
                continue;
            }
        }
        if !C0_CONTROLS.iter().any(|range| range.contains(&byte)) {
            clean.push(byte);
        }
        bytes = tail;
    }
    clean
}
