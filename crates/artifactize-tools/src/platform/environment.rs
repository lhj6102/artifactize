//! The environment of the programs artifactize starts: the person's own, as their terminal
//! or desktop session gave it to artifactize.
//!
//! Only an app started from an AppImage runs with more. The bundle's AppRun and its hooks
//! put the bundle's directories first in path lists such as `PATH` and `LD_LIBRARY_PATH` and
//! set variables of their own; the app needs them to load its bundled libraries, while a
//! system program it starts would load those libraries instead of its own and fail.
//! Everywhere else this is the process environment unchanged.

use std::{collections::BTreeMap, ffi::OsString};

/// This process's environment as the person using artifactize has it, for a program it
/// starts.
pub fn user() -> BTreeMap<OsString, OsString> {
    #[cfg(target_os = "linux")]
    if let Some(bundle) = super::appimage::Bundle::current() {
        return bundle.without(std::env::vars_os());
    }
    std::env::vars_os().collect()
}

/// One variable of [`user`]; `None` when it is unset there. Names compare as this system
/// compares them.
pub fn var(name: &str) -> Option<OsString> {
    #[cfg(target_os = "linux")]
    if let Some(bundle) = super::appimage::Bundle::current() {
        return bundle
            .without(std::env::vars_os())
            .remove(std::ffi::OsStr::new(name));
    }
    std::env::var_os(name)
}
