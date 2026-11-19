//! Reversible path text, distinct from the native spelling supplied to OS APIs.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use std::{
    ffi::OsString,
    io,
    path::{Path, PathBuf},
};

// Reserve the prefix on both hosts, including for a literal path that starts with it.
const ENCODED: &str = "native-path:/";
#[cfg(unix)]
const CODEC: &str = "unix/";
#[cfg(windows)]
const CODEC: &str = "windows/";
#[cfg(windows)]
const VERBATIM: &str = "//?/";

pub(super) fn native_path_text(path: &Path) -> String {
    if let Some(text) = readable(path).filter(|text| !text.starts_with(ENCODED)) {
        return text;
    }
    format!(
        "{ENCODED}{CODEC}{}",
        URL_SAFE_NO_PAD.encode(native_bytes(path))
    )
}

/// Read the platform path representation without losing native bytes or UTF-16 units.
pub fn path_from_text(text: &str) -> io::Result<PathBuf> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "Invalid native path text.");
    if let Some(encoded) = text.strip_prefix(ENCODED) {
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded.strip_prefix(CODEC).ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
        return from_native_bytes(bytes)
            .map(PathBuf::from)
            .ok_or_else(invalid);
    }
    // Shown path text uses '/'; Windows' own spelling uses '\', which verbatim paths need and
    // every other path accepts.
    #[cfg(windows)]
    return Ok(PathBuf::from(text.replace('/', "\\")));
    #[cfg(not(windows))]
    Ok(PathBuf::from(text))
}

#[cfg(unix)]
fn readable(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned)
}

#[cfg(windows)]
fn readable(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};

    let plain = super::windows::plain(path);
    let path = plain.as_deref().unwrap_or(path);
    let text = path.to_str()?;
    let verbatim = matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(..)));
    let display = text.replace('\\', "/");
    // A verbatim slash is literal; do not conflate it with an encoded backslash.
    (verbatim && !text.contains('/') || !verbatim && !display.starts_with(VERBATIM))
        .then_some(display)
}

#[cfg(unix)]
fn native_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(unix)]
fn from_native_bytes(bytes: Vec<u8>) -> Option<OsString> {
    use std::os::unix::ffi::OsStringExt;
    Some(OsString::from_vec(bytes))
}

#[cfg(windows)]
fn native_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(windows)]
fn from_native_bytes(bytes: Vec<u8>) -> Option<OsString> {
    use std::os::windows::ffi::OsStringExt;
    let (units, rest) = bytes.as_chunks();
    rest.is_empty().then(|| {
        OsString::from_wide(
            &units
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect::<Vec<_>>(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_codec_prefix_round_trips() {
        let path = Path::new("native-path:/plain");
        assert_eq!(path_from_text(&native_path_text(path)).unwrap(), path);
    }

    // Only Unix paths can contain native bytes that are not UTF-8.
    #[cfg(unix)]
    #[test]
    fn unrepresentable_unix_bytes_round_trip() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(OsString::from_vec(b"dir/non-utf8-\xff".to_vec()));
        assert_eq!(
            path_from_text(&native_path_text(&path))
                .unwrap()
                .as_os_str(),
            path.as_os_str()
        );
    }

    // Windows has UTF-16 paths, which can contain an unpaired surrogate.
    #[cfg(windows)]
    #[test]
    fn unpaired_windows_units_round_trip() {
        use std::os::windows::ffi::OsStringExt;
        let path = PathBuf::from(OsString::from_wide(&[u16::from(b'a'), 0xd800]));
        assert_eq!(
            path_from_text(&native_path_text(&path))
                .unwrap()
                .as_os_str(),
            path.as_os_str()
        );
    }

    // Only Windows has a verbatim namespace that preserves trailing dots.
    #[cfg(windows)]
    #[test]
    fn verbatim_spelling_reopens_the_exact_entry() {
        let fixture = crate::test_os::tempdir();
        let root = crate::test_os::trailing_dot_alias(fixture.path());
        let verbatim = root.join("literal.");
        let rendered = native_path_text(&verbatim);
        assert!(rendered.starts_with(VERBATIM));
        assert!(!rendered.contains('\\'));
        assert_eq!(
            std::fs::read(path_from_text(&rendered).unwrap()).unwrap(),
            b"verbatim"
        );
        assert_eq!(std::fs::read(root.join("literal")).unwrap(), b"ordinary");
    }
}
