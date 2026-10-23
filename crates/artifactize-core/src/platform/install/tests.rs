use super::*;
use std::fs;

const SITE: &str = "https://github.com/lhj6102/artifactize";

/// An executable at `relative` below `root`, created with its directories.
fn executable(root: &Path, relative: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "").unwrap();
    path
}

#[test]
fn install_scripts_are_known_by_their_receipt_and_an_unpacked_archive_is_not() {
    let root = crate::test_os::tempdir();
    let places = Places::default();
    let sh = executable(root.path(), "sh/bin/artifactize");
    fs::write(root.path().join("sh/bin").join(RECEIPT), "install.sh\n").unwrap();
    let ps1 = executable(root.path(), "ps1/artifactize.exe");
    fs::write(root.path().join("ps1").join(RECEIPT), "install.ps1\r\n").unwrap();
    let unpacked = executable(root.path(), "unpacked/artifactize");
    assert_eq!(classify(&sh, &places), Channel::InstallSh);
    assert_eq!(classify(&ps1, &places), Channel::InstallPs1);
    assert_eq!(classify(&unpacked, &places), Channel::Unknown);
}

#[test]
fn package_managers_are_known_by_their_directories() {
    let root = crate::test_os::tempdir();
    let places = Places {
        cargo_bin: Some(root.path().join("cargo/bin")),
        ..Places::default()
    };
    let cases = [
        (
            "Program Files/WindowsApps/lhj6102.Artifactize_0.15.1.0_x64__ayfzfzbsv48sg/artifactize.exe",
            Channel::Store,
        ),
        (
            "Microsoft/WinGet/Packages/lhj6102.Artifactize_Microsoft.Winget.Source_8wekyb3d8bbwe/artifactize-v0.15.1/artifactize.exe",
            Channel::Winget,
        ),
        (
            "homebrew/Cellar/artifactize/0.15.1/bin/artifactize",
            Channel::Homebrew,
        ),
        ("cargo/bin/artifactize", Channel::Cargo),
    ];
    for (relative, channel) in cases {
        assert_eq!(
            classify(&executable(root.path(), relative), &places),
            channel,
            "{relative}"
        );
    }
}

#[test]
fn the_mac_app_is_known_by_where_its_bundle_is_and_whether_a_cask_installed_it() {
    let root = crate::test_os::tempdir();
    let home = root.path().join("home");
    let caskroom = root.path().join("homebrew/Caskroom");
    let places = Places {
        home: Some(home.clone()),
        caskrooms: vec![caskroom.clone()],
        ..Places::default()
    };
    let script = executable(
        &home,
        "Applications/Artifactize.app/Contents/MacOS/artifactize",
    );
    let system = executable(
        root.path(),
        "Applications/Artifactize.app/Contents/MacOS/artifactize-desktop",
    );
    assert_eq!(classify(&script, &places), Channel::InstallSh);
    assert_eq!(classify(&system, &places), Channel::Dmg);
    fs::create_dir_all(caskroom.join("artifactize/0.15.1")).unwrap();
    assert_eq!(classify(&system, &places), Channel::HomebrewCask);
}

#[test]
fn linux_bundles_are_known_by_the_data_directory_the_appimage_and_dpkg() {
    let root = crate::test_os::tempdir();
    let data = root.path().join("share");
    let info = root.path().join("dpkg/info");
    let places = Places {
        data: Some(data.clone()),
        dpkg_info: Some(info.clone()),
        ..Places::default()
    };
    let unpacked = executable(&data, "artifactize/desktop/usr/bin/artifactize-desktop");
    let copied = executable(&data, "artifactize/bin/artifactize");
    let packaged = executable(root.path(), "usr/bin/artifactize");
    assert_eq!(classify(&unpacked, &places), Channel::InstallSh);
    assert_eq!(classify(&copied, &places), Channel::AppImage);
    assert_eq!(classify(&packaged, &places), Channel::Unknown);
    fs::create_dir_all(&info).unwrap();
    fs::write(
        info.join("artifactize.list"),
        format!("/.\n{}\n", path_text(&packaged)),
    )
    .unwrap();
    assert_eq!(classify(&packaged, &places), Channel::Deb);
    let mounted = Places {
        appimage: true,
        ..Places::default()
    };
    let mount = executable(root.path(), "mount/usr/bin/artifactize-desktop");
    assert_eq!(classify(&mount, &mounted), Channel::AppImage);
}

fn path_text(path: &Path) -> String {
    real(path).to_str().unwrap().to_owned()
}

#[test]
fn every_channel_but_the_store_says_how_to_update() {
    let latest = semver::Version::parse("0.16.0").unwrap();
    let command = |command: &str| {
        Some(Update::Command {
            command: command.into(),
        })
    };
    assert_eq!(update(Channel::Store, &latest, SITE), None);
    assert_eq!(
        update(Channel::InstallSh, &latest, SITE),
        command("curl -fsSL https://artifactize.dev/install.sh | sh")
    );
    assert_eq!(
        update(Channel::InstallPs1, &latest, SITE),
        command("irm https://artifactize.dev/install.ps1 | iex")
    );
    assert_eq!(
        update(Channel::Winget, &latest, SITE),
        command("winget upgrade lhj6102.Artifactize")
    );
    assert_eq!(
        update(Channel::HomebrewCask, &latest, SITE),
        command("brew upgrade --cask artifactize")
    );
    assert_eq!(
        update(Channel::Cargo, &latest, SITE),
        command("cargo install artifactize artifactize-tools --locked")
    );
    assert_eq!(
        update(Channel::Deb, &latest, SITE),
        Some(Update::Download {
            url: "https://github.com/lhj6102/artifactize/releases/latest/download/Artifactize-amd64.deb".into(),
            command: Some("sudo apt install ./Artifactize-amd64.deb".into()),
        })
    );
    assert_eq!(
        update(Channel::AppImage, &latest, SITE),
        Some(Update::Download {
            url: "https://github.com/lhj6102/artifactize/releases/download/v0.16.0/artifactize-desktop-v0.16.0-x86_64-unknown-linux-gnu.AppImage".into(),
            command: None,
        })
    );
    assert_eq!(
        update(Channel::Unknown, &latest, SITE),
        Some(Update::Page {
            url: "https://github.com/lhj6102/artifactize/releases/latest".into()
        })
    );
}
