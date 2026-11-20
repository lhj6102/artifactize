//! OS-specific pieces of shared integration and unit-test fixtures.

use std::{path::Path, process::Command};

#[cfg(unix)]
pub fn make_executable(path: &Path) {
    use std::{fs, os::unix::fs::PermissionsExt};
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(windows)]
pub fn make_executable(_path: &Path) {}

#[cfg(unix)]
pub fn symlink_file(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    std::os::unix::fs::symlink(target, link).expect("test runner can create symbolic links");
}

#[cfg(windows)]
pub fn symlink_file(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    std::os::windows::fs::symlink_file(target, link).expect(
        "Windows test runner must enable Developer Mode or run with symbolic-link privileges",
    );
}

#[cfg(target_os = "macos")]
pub const OPENER: Option<&str> = Some("open");
#[cfg(all(unix, not(target_os = "macos")))]
pub const OPENER: Option<&str> = Some("xdg-open");
#[cfg(windows)]
pub const OPENER: Option<&str> = None;

#[cfg(unix)]
pub fn set_program_extensions(_command: &mut Command) {}

#[cfg(windows)]
pub fn set_program_extensions(command: &mut Command) {
    command.env("PATHEXT", ".CMD;.BAT;.EXE");
}
