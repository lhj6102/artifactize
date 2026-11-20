//! How this copy was installed, read from where its executable is: each channel puts it in a
//! place of its own. The install scripts, whose directory may be shared with an unpacked
//! archive, leave a receipt beside the commands they install.

use std::path::{Path, PathBuf};

use crate::release::{Channel, Update};

/// The receipt `install.sh` or `install.ps1` writes beside the commands it installs; it holds
/// the script's name.
const RECEIPT: &str = ".artifactize-receipt";
/// The macOS app's bundle name, as the disk image, the cask and `install.sh` install it.
const BUNDLE: &str = "Artifactize.app";
/// The dpkg file lists that name the `.deb` package's files: Tauri names the package after
/// the app's product name.
const DPKG_LISTS: [&str; 2] = ["artifactize.list", "artifactize:amd64.list"];

/// Where the channels put artifactize on this system. A place a system does not have is
/// `None` or empty.
#[derive(Debug, Default)]
pub(crate) struct Places {
    /// The person's home directory, whose `Applications` holds `install.sh`'s macOS app.
    pub home: Option<PathBuf>,
    /// Cargo's `bin` directory.
    pub cargo_bin: Option<PathBuf>,
    /// The XDG data directory, where `install.sh` unpacks the AppImage and the AppImage
    /// copies its commands.
    pub data: Option<PathBuf>,
    /// This process runs from an AppImage.
    pub appimage: bool,
    /// Homebrew's cask directories.
    pub caskrooms: Vec<PathBuf>,
    /// dpkg's directory of package file lists.
    pub dpkg_info: Option<PathBuf>,
}

impl Places {
    /// This system's places.
    fn here() -> Self {
        let var = |name| super::environment::var(name).filter(|value| !value.is_empty());
        let home = super::home_directory();
        let cargo_bin = var("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".cargo")))
            .map(|cargo| cargo.join("bin"));
        #[cfg(target_os = "linux")]
        let places = Self {
            data: var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| home.as_ref().map(|home| home.join(".local/share"))),
            appimage: var("APPIMAGE").is_some(),
            dpkg_info: Some("/var/lib/dpkg/info".into()),
            ..Self::default()
        };
        #[cfg(target_os = "macos")]
        let places = Self {
            // Apple silicon and Intel prefixes, and a custom one.
            caskrooms: ["/opt/homebrew", "/usr/local"]
                .into_iter()
                .map(PathBuf::from)
                .chain(var("HOMEBREW_PREFIX").map(PathBuf::from))
                .map(|prefix| prefix.join("Caskroom"))
                .collect(),
            ..Self::default()
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let places = Self::default();
        Self {
            home,
            cargo_bin,
            ..places
        }
    }
}

/// How the running copy was installed.
pub(crate) fn channel() -> Channel {
    match std::env::current_exe() {
        Ok(executable) => installed_channel(&executable, &Places::here()),
        Err(_) => Channel::Unknown,
    }
}

/// The channel that puts an executable at `executable`, given this system's `places`.
fn installed_channel(executable: &Path, places: &Places) -> Channel {
    Evidence::read(executable, places)
        .as_ref()
        .map_or(Channel::Unknown, classify)
}

struct Evidence {
    directory: PathBuf,
    receipt: Option<String>,
    applications: Option<PathBuf>,
    cask_installed: bool,
    desktop: Option<PathBuf>,
    appimage_bin: Option<PathBuf>,
    appimage: bool,
    cargo_bin: Option<PathBuf>,
    listed_by_dpkg: bool,
}

impl Evidence {
    fn read(executable: &Path, places: &Places) -> Option<Self> {
        let executable = real(executable);
        let directory = executable.parent()?;
        Some(Self {
            directory: directory.to_owned(),
            receipt: std::fs::read_to_string(directory.join(RECEIPT)).ok(),
            applications: places
                .home
                .as_ref()
                .map(|home| real(&home.join("Applications"))),
            cask_installed: bundle(directory).is_some()
                && places
                    .caskrooms
                    .iter()
                    .any(|caskroom| caskroom.join("artifactize").is_dir()),
            desktop: places
                .data
                .as_ref()
                .map(|data| real(&data.join("artifactize/desktop"))),
            appimage_bin: places
                .data
                .as_ref()
                .map(|data| real(&data.join("artifactize/bin"))),
            appimage: places.appimage,
            cargo_bin: places.cargo_bin.as_deref().map(real),
            listed_by_dpkg: places
                .dpkg_info
                .as_ref()
                .is_some_and(|info| lists_file(info, &executable)),
        })
    }
}

fn classify(evidence: &Evidence) -> Channel {
    let directory = &evidence.directory;
    let names: Vec<_> = directory
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    let named = |wanted: &str| names.iter().any(|name| name.eq_ignore_ascii_case(wanted));
    match evidence.receipt.as_deref().map(str::trim) {
        Some("install.sh") => return Channel::InstallSh,
        Some("install.ps1") => return Channel::InstallPs1,
        _ => {}
    }
    if named("WindowsApps") {
        return Channel::Store;
    }
    if names
        .windows(2)
        .any(|pair| pair == ["Cellar", "artifactize"])
    {
        return Channel::Homebrew;
    }
    if let Some(bundle) = bundle(directory) {
        return if evidence
            .applications
            .as_deref()
            .is_some_and(|path| bundle.parent() == Some(path))
        {
            Channel::InstallSh
        } else if evidence.cask_installed {
            Channel::HomebrewCask
        } else {
            Channel::Dmg
        };
    }
    let within = |place: Option<&Path>| place.is_some_and(|path| directory.starts_with(path));
    if within(evidence.desktop.as_deref()) {
        return Channel::InstallSh;
    }
    if evidence.appimage || within(evidence.appimage_bin.as_deref()) {
        return Channel::AppImage;
    }
    if evidence.cargo_bin.as_deref() == Some(directory.as_path()) {
        return Channel::Cargo;
    }
    if evidence.listed_by_dpkg {
        return Channel::Deb;
    }
    Channel::Unknown
}

/// How to update an install of `channel` to `latest`, whose pages and files are below the
/// repository's `site`; `None` when the Microsoft Store updates it. Each channel's commands and
/// file names are its operating system's.
pub(crate) fn update(channel: Channel, latest: &semver::Version, site: &str) -> Option<Update> {
    let command = |command: &str| {
        Some(Update::Command {
            command: command.into(),
        })
    };
    match channel {
        Channel::Store => None,
        Channel::InstallSh => command("curl -fsSL https://artifactize.dev/install.sh | sh"),
        Channel::InstallPs1 => command("irm https://artifactize.dev/install.ps1 | iex"),
        Channel::Homebrew => command("brew upgrade artifactize"),
        Channel::HomebrewCask => command("brew upgrade --cask artifactize"),
        Channel::Cargo => command("cargo install artifactize artifactize-tools --locked"),
        Channel::Dmg => Some(Update::Download {
            url: format!("{site}/releases/latest/download/Artifactize.dmg"),
            command: None,
        }),
        Channel::Deb => Some(Update::Download {
            url: format!("{site}/releases/latest/download/Artifactize-amd64.deb"),
            command: Some("sudo apt install ./Artifactize-amd64.deb".into()),
        }),
        Channel::AppImage => Some(Update::Download {
            url: format!(
                "{site}/releases/download/v{latest}/artifactize-desktop-v{latest}-x86_64-unknown-linux-gnu.AppImage"
            ),
            command: None,
        }),
        Channel::Unknown => Some(Update::Page {
            url: format!("{site}/releases/latest"),
        }),
    }
}

/// The app bundle whose `Contents/MacOS` is `directory`.
fn bundle(directory: &Path) -> Option<&Path> {
    let contents = directory
        .parent()
        .filter(|_| directory.file_name().is_some_and(|name| name == "MacOS"))?;
    contents
        .parent()
        .filter(|_| contents.file_name().is_some_and(|name| name == "Contents"))
        .filter(|bundle| bundle.file_name().is_some_and(|name| name == BUNDLE))
}

/// Whether the `.deb` package's file list in `info` names `file`.
fn lists_file(info: &Path, file: &Path) -> bool {
    DPKG_LISTS.iter().any(|list| {
        std::fs::read_to_string(info.join(list))
            .is_ok_and(|listed| listed.lines().any(|line| real(Path::new(line)) == file))
    })
}

/// `path` with its links resolved, so a command linked onto `PATH` is found where its channel
/// put it; as given when it does not exist.
fn real(path: &Path) -> PathBuf {
    super::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests;
