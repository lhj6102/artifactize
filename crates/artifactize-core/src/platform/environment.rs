//! The process environment, read in one place: variable lookup, which ignores the case of
//! names on Windows, snapshots handed to children, and lookups in such snapshots. Every child
//! gets a snapshot, never this process's environment by inheritance.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
};

/// One variable of this process's environment; `None` when it is unset.
pub fn var(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// One variable as text; `None` when it is unset or not Unicode.
pub fn var_text(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// The whole environment of a child that runs as the person using artifactize: this
/// process's, without what an AppImage added to it ([`artifactize_tools::environment::user`]).
pub(crate) fn snapshot() -> BTreeMap<OsString, OsString> {
    artifactize_tools::environment::user()
}

/// The prefix of artifactize's own variables: its credentials, settings and the variables it
/// sets for one command.
const OWN_PREFIX: &str = "ARTIFACTIZE_";

/// The environment of a command an author declared: this process's whole environment, as in
/// the author's terminal, without artifactize's own variables. A caller adds the few that
/// the command is meant to see.
pub(crate) fn for_command() -> BTreeMap<OsString, OsString> {
    let mut environment = snapshot();
    environment.retain(|name, _| {
        !name
            .to_str()
            .is_some_and(|name| name.to_ascii_uppercase().starts_with(OWN_PREFIX))
    });
    environment
}

/// Set one variable of a built environment, replacing a variable of the same name as this
/// system compares names.
pub(crate) fn set(
    environment: &mut BTreeMap<OsString, OsString>,
    name: &str,
    value: impl Into<OsString>,
) {
    if super::ENV_NAMES_IGNORE_CASE {
        environment.retain(|key, _| {
            !key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case(name))
        });
    }
    environment.insert(name.into(), value.into());
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
