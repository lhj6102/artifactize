//! The process environment, read in one place: variable lookup, which ignores the case of
//! names on Windows, snapshots handed to children, and lookups in such snapshots.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
};

/// One variable of this process's environment; `None` when it is unset.
pub(crate) fn var(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// One variable as text; `None` when it is unset or not Unicode.
pub(crate) fn var_text(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// This process's whole environment, for a child that runs as the person using artifactize.
pub(crate) fn snapshot() -> BTreeMap<OsString, OsString> {
    std::env::vars_os().collect()
}

/// One variable of an environment built for a child, matching names as this system does:
/// exactly on Unix, without regard to case on Windows.
pub(crate) fn lookup<'a>(
    environment: &'a BTreeMap<OsString, OsString>,
    name: &str,
) -> Option<&'a OsStr> {
    if super::ENV_NAMES_IGNORE_CASE {
        environment.iter().find_map(|(key, value)| {
            key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case(name))
                .then_some(value.as_os_str())
        })
    } else {
        environment.get(OsStr::new(name)).map(OsString::as_os_str)
    }
}
