//! Where the desktop app is installed, read from file metadata only: nothing is started.
//! Every install channel leaves `artifactize-desktop` beside the commands it bundles (the
//! deb, the Windows zip and MSIX, the Mac app the cask links), on `PATH` (install.sh's
//! launcher, winget's alias), or at one of this system's app locations.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

/// The desktop app's program name, without this system's executable suffix.
const PROGRAM: &str = "artifactize-desktop";

/// The file, below the data home's `artifactize` folder, that names the AppImage last started.
#[cfg(target_os = "linux")]
const APPIMAGE_RECORD: &str = "appimage-path";

/// The Microsoft Store package's family name; its execution-alias folder exists while it is
/// installed.
#[cfg(windows)]
const STORE_PACKAGE_FAMILY: &str = "lhj6102.Artifactize_ayfzfzbsv48sg";

/// The installed desktop app this command finds, if any.
pub(crate) fn installed() -> Option<PathBuf> {
    // A Homebrew or AppImage link names the bundle only once resolved.
    let executable = std::env::current_exe()
        .ok()
        .map(|executable| std::fs::canonicalize(&executable).unwrap_or(executable));
    let cwd = std::env::current_dir().ok();
    let home = super::home_directory();
    find(
        executable.as_deref(),
        cwd.as_deref(),
        super::environment::var,
        locations(super::environment::var, home.as_deref()),
    )
}

fn find(
    executable: Option<&Path>,
    cwd: Option<&Path>,
    variable: impl Fn(&str) -> Option<OsString>,
    locations: Vec<PathBuf>,
) -> Option<PathBuf> {
    executable
        .and_then(Path::parent)
        .map(|bin| bin.join(format!("{PROGRAM}{}", std::env::consts::EXE_SUFFIX)))
        .filter(|beside| beside.is_file())
        .or_else(|| {
            cwd.and_then(|cwd| {
                super::program::resolve_with(OsStr::new(PROGRAM), cwd, variable).ok()
            })
        })
        .or_else(|| locations.into_iter().find(|location| location.exists()))
}

/// Where this system's installers put the app when it is not beside the command or on `PATH`.
#[cfg(target_os = "macos")]
fn locations(_variable: impl Fn(&str) -> Option<OsString>, home: Option<&Path>) -> Vec<PathBuf> {
    // The disk image and the cask install into /Applications, install.sh into ~/Applications.
    let bundle = Path::new("Artifactize.app/Contents/MacOS").join(PROGRAM);
    std::iter::once(PathBuf::from("/Applications"))
        .chain(home.map(|home| home.join("Applications")))
        .map(|applications| applications.join(&bundle))
        .collect()
}

#[cfg(target_os = "linux")]
fn locations(variable: impl Fn(&str) -> Option<OsString>, home: Option<&Path>) -> Vec<PathBuf> {
    let Some(artifactize) = home.map(|home| data_home(variable, home).join("artifactize")) else {
        return Vec::new();
    };
    // install.sh unpacks the AppImage here; an AppImage started directly records its path.
    let mut locations = vec![artifactize.join("desktop/AppRun")];
    locations.extend(
        std::fs::read(artifactize.join(APPIMAGE_RECORD))
            .ok()
            .filter(|path| !path.is_empty())
            .map(|path| super::path_from_bytes(&path)),
    );
    locations
}

#[cfg(windows)]
fn locations(variable: impl Fn(&str) -> Option<OsString>, _home: Option<&Path>) -> Vec<PathBuf> {
    let Some(local) = variable("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    // install.ps1's default folder, and the alias folder the Store package owns.
    vec![
        local
            .join("Programs/artifactize")
            .join(format!("{PROGRAM}.exe")),
        local
            .join("Microsoft/WindowsApps")
            .join(STORE_PACKAGE_FAMILY),
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn locations(_variable: impl Fn(&str) -> Option<OsString>, _home: Option<&Path>) -> Vec<PathBuf> {
    Vec::new()
}

/// `$XDG_DATA_HOME` when it is an absolute path, else `~/.local/share`.
#[cfg(target_os = "linux")]
pub(super) fn data_home(variable: impl Fn(&str) -> Option<OsString>, home: &Path) -> PathBuf {
    variable("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"))
}

/// Name `appimage` as the installed app below `artifactize`, the data home's folder. The
/// record is replaced by rename and only when it changes.
#[cfg(target_os = "linux")]
pub(super) fn record_appimage(artifactize: &Path, appimage: &Path) -> std::io::Result<()> {
    use std::{io::Write, os::unix::ffi::OsStrExt};
    let record = artifactize.join(APPIMAGE_RECORD);
    let bytes = appimage.as_os_str().as_bytes();
    if std::fs::read(&record).is_ok_and(|recorded| recorded == bytes) {
        return Ok(());
    }
    std::fs::create_dir_all(artifactize)?;
    let mut temporary = tempfile::NamedTempFile::new_in(artifactize)?;
    temporary.write_all(bytes)?;
    temporary.persist(record).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variables(pairs: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, OsString)> = pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.as_os_str().to_owned()))
            .chain([("PATHEXT".to_owned(), OsString::from(".EXE"))])
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        }
    }

    #[test]
    fn the_app_beside_the_running_command_is_found_first() {
        let root = crate::test_os::tempdir();
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let app = bin.join(format!("{PROGRAM}{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&app, "").unwrap();
        let elsewhere = root.path().join("elsewhere");
        std::fs::write(&elsewhere, "").unwrap();
        assert_eq!(
            find(
                Some(&bin.join("artifactize")),
                Some(root.path()),
                variables(&[]),
                vec![elsewhere],
            ),
            Some(app)
        );
    }

    #[test]
    fn the_app_on_path_is_found_when_none_is_beside_the_command() {
        let root = crate::test_os::tempdir();
        let path = root.path().join("path");
        std::fs::create_dir(&path).unwrap();
        let app = path.join(PROGRAM);
        std::fs::write(&app, "").unwrap();
        crate::test_os::make_executable(&app);
        let found = find(
            Some(&root.path().join("artifactize")),
            Some(root.path()),
            variables(&[("PATH", &path)]),
            Vec::new(),
        )
        .expect("the app on PATH");
        assert_eq!(found.parent(), Some(path.as_path()));
    }

    #[test]
    fn an_existing_install_location_is_found_and_absent_ones_are_not() {
        let root = crate::test_os::tempdir();
        let missing = root.path().join("missing");
        let installed = root.path().join("Artifactize.app");
        assert_eq!(
            find(None, None, variables(&[]), vec![missing.clone()]),
            None
        );
        std::fs::create_dir(&installed).unwrap();
        assert_eq!(
            find(None, None, variables(&[]), vec![missing, installed.clone()]),
            Some(installed)
        );
    }

    // The Applications folders exist only on macOS.
    #[cfg(target_os = "macos")]
    #[test]
    fn mac_locations_are_the_system_and_home_applications_folders() {
        let root = crate::test_os::tempdir();
        let home = root.path();
        assert_eq!(
            locations(variables(&[]), Some(home)),
            [
                PathBuf::from("/Applications/Artifactize.app/Contents/MacOS/artifactize-desktop"),
                home.join("Applications/Artifactize.app/Contents/MacOS/artifactize-desktop"),
            ]
        );
    }

    // install.sh's unpacked AppImage and the AppImage record exist only on Linux.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_locations_include_the_unpacked_app_and_the_recorded_appimage() {
        let root = crate::test_os::tempdir();
        let data = root.path().join("data");
        let artifactize = data.join("artifactize");
        let read = || locations(variables(&[("XDG_DATA_HOME", &data)]), Some(root.path()));
        assert_eq!(read(), [artifactize.join("desktop/AppRun")]);
        let appimage = root.path().join("Downloads/Artifactize.AppImage");
        record_appimage(&artifactize, &appimage).unwrap();
        record_appimage(&artifactize, &appimage).unwrap();
        assert_eq!(read(), [artifactize.join("desktop/AppRun"), appimage]);
        // A relative XDG_DATA_HOME is ignored, as the XDG specification requires.
        assert_eq!(
            locations(
                variables(&[("XDG_DATA_HOME", Path::new("relative"))]),
                Some(root.path())
            ),
            [root.path().join(".local/share/artifactize/desktop/AppRun")]
        );
    }

    // install.ps1's folder and the Store package exist only on Windows.
    #[cfg(windows)]
    #[test]
    fn windows_locations_are_the_install_folder_and_the_store_package() {
        let root = crate::test_os::tempdir();
        let local = root.path();
        assert_eq!(
            locations(variables(&[("LOCALAPPDATA", local)]), None),
            [
                local.join("Programs/artifactize/artifactize-desktop.exe"),
                local
                    .join("Microsoft/WindowsApps")
                    .join(STORE_PACKAGE_FAMILY),
            ]
        );
    }
}
