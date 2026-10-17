//! Normalizing native Windows paths without changing which file they name.
use std::{
    ffi::{OsStr, OsString},
    io,
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf, Prefix},
};

/// The absolute path of an existing file with every link resolved, without the `\\?\`
/// prefix when the plain path names the same file. A logical Artifact path joined to a plain
/// path keeps `/` as a separator, and child programs accept it as a working directory.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let path = std::fs::canonicalize(path)?;
    Ok(plain(&path).unwrap_or(path))
}

/// `\\?\C:\a` as `C:\a` and `\\?\UNC\server\share\a` as `\\server\share\a`, when the Win32
/// layer would pass every component through unchanged and the result is below `MAX_PATH`.
fn plain(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return None;
    };
    let mut plain = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => OsString::from(format!("{}:", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => {
            let mut plain = OsString::from(r"\\");
            plain.push(server);
            plain.push(r"\");
            plain.push(share);
            plain
        }
        _ => return None,
    };
    if components.next() != Some(Component::RootDir) {
        return None;
    }
    plain.push(r"\");
    for (index, component) in components.enumerate() {
        let Component::Normal(name) = component else {
            return None;
        };
        if !plain_name(name) {
            return None;
        }
        if index > 0 {
            plain.push(r"\");
        }
        plain.push(name);
    }
    // Win32's legacy MAX_PATH includes the terminating NUL; longer paths must
    // retain their verbatim prefix rather than being rewritten to the plain form.
    const MAX_PLAIN_PATH_UNITS: usize = 260;
    (plain.encode_wide().count() < MAX_PLAIN_PATH_UNITS).then(|| PathBuf::from(plain))
}

/// A name the Win32 layer keeps as is: no trailing dot or space, no reserved character, and
/// no DOS device name such as `NUL` or `com1.txt`.
fn plain_name(name: &OsStr) -> bool {
    // Conservatively permit only short UTF-8 components when removing a
    // verbatim prefix; do not normalize names beyond this existing byte bound.
    const MAX_PLAIN_NAME_BYTES: usize = 255;
    let Some(name) = name.to_str() else {
        return false;
    };
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');
    let device = match stem.to_ascii_uppercase().as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        port => port
            .strip_prefix("COM")
            .or_else(|| port.strip_prefix("LPT"))
            .is_some_and(|number| {
                number.chars().count() == 1
                    && number.chars().all(|digit| "0123456789¹²³".contains(digit))
            }),
    };
    !name.is_empty()
        && name.len() <= MAX_PLAIN_NAME_BYTES
        && !name.ends_with(['.', ' '])
        && !name
            .chars()
            .any(|c| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        && !device
}
