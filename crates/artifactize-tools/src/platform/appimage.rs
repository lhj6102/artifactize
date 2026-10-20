//! What an AppImage adds to the environment of the app it starts, and that environment
//! without it.
//!
//! The AppImage runtime exports `APPDIR`, `APPIMAGE`, `ARGV0` and `OWD`. The linuxdeploy GTK
//! hook exports `APPDIR` too, points GTK, GDK, GIO and GObject variables at the bundle,
//! prepends the bundle's and the system's data directories to `XDG_DATA_DIRS` and sets
//! `GTK_THEME`. AppRun (AppImageKit's AppRun.c) then prepends the bundle's directories to
//! `PATH`, `LD_LIBRARY_PATH`, `PYTHONPATH`, `XDG_DATA_DIRS`, `PERLLIB`,
//! `GSETTINGS_SCHEMA_DIR`, `QT_PLUGIN_PATH` and the GStreamer plugin paths, and sets
//! `PYTHONHOME` and `PYTHONDONTWRITEBYTECODE`. None of them records the value it replaced, so
//! the person's environment is what is left without the bundle's entries and variables.

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// The bundle's directory, exported by the AppImage runtime and the GTK hook.
const BUNDLE_DIRECTORY: &str = "APPDIR";

/// Variables the AppImage sets to values that name nothing inside the bundle: the runtime's
/// image path, `argv[0]` and working directory, AppRun's Python setting and the GTK hook's
/// theme. Whatever the person had in them was replaced, not kept.
const SET_BY_BUNDLE: [&str; 5] = [
    "APPIMAGE",
    "ARGV0",
    "OWD",
    "PYTHONDONTWRITEBYTECODE",
    "GTK_THEME",
];

/// The GTK hook adds the system's data directory right after the bundle's own.
const DATA_DIRECTORIES: &str = "XDG_DATA_DIRS";
const HOOK_DATA_DIRECTORY: &str = "/usr/share";

/// The AppImage bundle this process runs from.
pub(super) struct Bundle {
    /// `APPDIR` as exported and as resolved: entries were built from the exported text.
    roots: Vec<PathBuf>,
}

impl Bundle {
    /// The bundle when this executable is inside `APPDIR`. An `APPDIR` inherited from
    /// another AppImage, such as a terminal started from one, names a bundle this program
    /// is not part of, and changes nothing.
    pub(super) fn current() -> Option<Self> {
        let named = PathBuf::from(std::env::var_os(BUNDLE_DIRECTORY).filter(|v| !v.is_empty())?);
        let resolved = std::fs::canonicalize(&named).ok()?;
        let executable = std::env::current_exe().ok()?;
        executable
            .starts_with(&resolved)
            .then(|| Self::new(named, resolved))
    }

    fn new(named: PathBuf, resolved: PathBuf) -> Self {
        let roots = if named == resolved {
            vec![named]
        } else {
            vec![named, resolved]
        };
        Self { roots }
    }

    /// `environment` without what the bundle added: its own variables, every variable
    /// left without an entry of the person's, and the bundle's entries in the rest.
    pub(super) fn without(
        &self,
        environment: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> BTreeMap<OsString, OsString> {
        environment
            .into_iter()
            .filter(|(name, _)| !SET_BY_BUNDLE.iter().any(|set| name == set))
            .filter_map(|(name, value)| {
                let value = self.person_entries(&name, &value)?;
                Some((name, value))
            })
            .collect()
    }

    fn contains(&self, entry: &Path) -> bool {
        self.roots.iter().any(|root| entry.starts_with(root))
    }

    /// `value`, read as a path list, without its entries inside the bundle and, in
    /// `XDG_DATA_DIRS`, the hook's system entry after them. `None` when nothing of the
    /// person's is left; a value naming nothing inside the bundle is returned as it is.
    fn person_entries(&self, name: &OsStr, value: &OsStr) -> Option<OsString> {
        let entries: Vec<PathBuf> = std::env::split_paths(value).collect();
        if !entries.iter().any(|entry| self.contains(entry)) {
            return Some(value.to_owned());
        }
        let mut hook_entry_pending = name == DATA_DIRECTORIES;
        let mut after_bundle = false;
        let mut kept = Vec::with_capacity(entries.len());
        for entry in entries {
            let inside = self.contains(&entry);
            let hook_entry = hook_entry_pending
                && after_bundle
                && !inside
                && entry == Path::new(HOOK_DATA_DIRECTORY);
            hook_entry_pending &= !hook_entry;
            after_bundle = inside;
            if !inside && !hook_entry {
                kept.push(entry);
            }
        }
        kept.iter()
            .any(|entry| !entry.as_os_str().is_empty())
            .then(|| std::env::join_paths(kept).expect("entries split from one list join again"))
    }
}
