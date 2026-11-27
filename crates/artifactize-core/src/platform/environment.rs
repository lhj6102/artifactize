//! The process environment, read in one place: variable lookup, which ignores the case of
//! names on Windows, snapshots handed to children, and lookups in such snapshots. Every child
//! gets a snapshot, never this process's environment by inheritance.

use std::{
    borrow::Cow,
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

/// Use the same environment for program lookup and launch. When a caller supplies aliases,
/// the last entry in map order wins, as when passing the ordered map to `Command::envs`.
pub(crate) fn for_child(
    environment: &BTreeMap<OsString, OsString>,
) -> Cow<'_, BTreeMap<OsString, OsString>> {
    if !super::ENV_NAMES_IGNORE_CASE {
        return Cow::Borrowed(environment);
    }
    let mut normalized = BTreeMap::new();
    for (name, value) in environment {
        insert(&mut normalized, name, value.clone());
    }
    Cow::Owned(normalized)
}

/// Set one variable of a built environment, replacing a variable of the same name as this
/// system compares names.
pub(crate) fn set(
    environment: &mut BTreeMap<OsString, OsString>,
    name: &str,
    value: impl Into<OsString>,
) {
    insert(environment, OsStr::new(name), value.into());
}

fn insert(environment: &mut BTreeMap<OsString, OsString>, name: &OsStr, value: OsString) {
    if super::ENV_NAMES_IGNORE_CASE {
        environment.retain(|key, _| !same_name(key, name));
    }
    environment.insert(name.into(), value);
}

/// Remove one variable of a built environment, matching its name as this system compares
/// names.
pub(crate) fn remove(environment: &mut BTreeMap<OsString, OsString>, name: &str) {
    if super::ENV_NAMES_IGNORE_CASE {
        environment.retain(|key, _| !same_name(key, OsStr::new(name)));
    } else {
        environment.remove(OsStr::new(name));
    }
}

/// One variable of an environment built for a child, matching names as this system does:
/// exactly on Unix, without regard to case on Windows.
pub(crate) fn lookup<'a>(
    environment: &'a BTreeMap<OsString, OsString>,
    name: &str,
) -> Option<&'a OsStr> {
    if super::ENV_NAMES_IGNORE_CASE {
        environment
            .iter()
            .find_map(|(key, value)| same_name(key, OsStr::new(name)).then_some(value.as_os_str()))
    } else {
        environment.get(OsStr::new(name)).map(OsString::as_os_str)
    }
}

#[cfg(unix)]
fn same_name(left: &OsStr, right: &OsStr) -> bool {
    left == right
}

#[cfg(windows)]
fn same_name(left: &OsStr, right: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    let left: Vec<_> = left.encode_wide().collect();
    let right: Vec<_> = right.encode_wide().collect();
    let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };
    // SAFETY: both UTF-16 buffers remain live for their explicit lengths; no pointer is retained.
    unsafe {
        CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) == CSTR_EQUAL
    }
}

/// Have a child allocate its ordinary temporary files within its owned run directory.
pub(crate) fn set_temporary_directory(
    environment: &mut BTreeMap<OsString, OsString>,
    dir: &std::path::Path,
) {
    for key in super::TEMP_VARIABLES {
        set(environment, key, dir);
    }
}

/// Remove empty and relative segments (such as `.` or `./bin`) from `PATH` before spawning a
/// child with `cwd = worktree`, so sub-scripts (`#!/usr/bin/env node`) cannot be hijacked by
/// repository files. With no absolute entry left, `PATH` is removed: an empty `PATH` is itself
/// one empty entry, which Unix searches as the current directory.
pub(crate) fn strip_relative_path_entries(environment: &mut BTreeMap<OsString, OsString>) {
    let Some(current) = lookup(environment, "PATH").map(ToOwned::to_owned) else {
        return;
    };
    let absolute: Vec<_> = std::env::split_paths(&current)
        .filter(|entry| !entry.as_os_str().is_empty() && entry.is_absolute())
        .collect();
    match std::env::join_paths(&absolute) {
        Ok(joined) if !joined.is_empty() => set(environment, "PATH", joined),
        _ => remove(environment, "PATH"),
    }
}

#[cfg(test)]
mod path_hygiene_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn strip_relative_path_entries_keeps_only_absolute_entries() {
        let root = crate::test_os::tempdir();
        let abs_a = root.path().join("bin_a");
        let abs_b = root.path().join("bin_b");
        let raw = std::env::join_paths([
            PathBuf::from("."),
            PathBuf::from(""),
            abs_a.clone(),
            PathBuf::from("relative/bin"),
            abs_b.clone(),
        ])
        .unwrap();
        let mut env = BTreeMap::new();
        set(&mut env, "PATH", raw);
        strip_relative_path_entries(&mut env);
        let cleaned: Vec<_> = std::env::split_paths(lookup(&env, "PATH").unwrap()).collect();
        assert_eq!(cleaned, vec![abs_a, abs_b]);
        // Only relative entries: no PATH at all, never an empty one that searches the cwd.
        let mut env = BTreeMap::new();
        set(
            &mut env,
            "PATH",
            std::env::join_paths([PathBuf::from("."), PathBuf::from("bin")]).unwrap(),
        );
        strip_relative_path_entries(&mut env);
        assert_eq!(lookup(&env, "PATH"), None);
    }
}
