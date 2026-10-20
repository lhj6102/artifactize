//! Desktop-only command registration. OS paths, login shells and elevation stay here.

use std::{io, path::Path};

#[cfg(any(target_os = "macos", test))]
mod macos;
// AppImage registration uses Unix symlinks; its boundary tests also run on macOS.
#[cfg(all(unix, any(target_os = "linux", test)))]
mod appimage;

/// Make the bundled commands available to new terminals, without replacing an existing
/// installation. The desktop calls this on a blocking worker; failures are non-fatal.
pub fn setup(state: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let executable = std::env::current_exe()?;
        let Some(bin) = executable.parent().filter(|bin| {
            bin.file_name().is_some_and(|name| name == "MacOS")
                && bin.parent().is_some_and(|contents| {
                    contents.file_name().is_some_and(|name| name == "Contents")
                })
        }) else {
            return Ok(()); // A source build, not an installed bundle.
        };
        macos::register(
            bin,
            Path::new("/etc/paths.d/artifactize"),
            state,
            terminal_has_command(bin)?,
            macos::write_privileged,
        )
    }
    #[cfg(target_os = "linux")]
    {
        let _ = state;
        if super::environment::var("APPIMAGE").is_none_or(|path| path.is_empty()) {
            return Ok(()); // Includes .deb and install.sh's extracted AppImage.
        }
        let home = super::home_directory().ok_or_else(|| io::Error::other("no home directory"))?;
        let data = super::environment::var("XDG_DATA_HOME")
            .filter(|path| !path.is_empty())
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        let executable = std::env::current_exe()?;
        let bin = executable
            .parent()
            .ok_or_else(|| io::Error::other("no executable directory"))?;
        appimage::register(
            bin,
            &data.join("artifactize/bin"),
            &home.join(".local/bin"),
            env!("CARGO_PKG_VERSION"),
            terminal_has_command(bin)?,
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = state;
        Ok(()) // Windows installers and Store execution aliases own PATH.
    }
}

// Finder's PATH can omit the terminal's commands; AppRun's can include the mounted
// sidecars. Combine the inherited PATH with our channels' terminal locations, and
// on macOS the path_helper inputs, without executing shell startup files.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn terminal_has_command(bundled: &Path) -> io::Result<bool> {
    let home = super::home_directory().ok_or_else(|| io::Error::other("no home directory"))?;
    let mut directories: Vec<_> = super::environment::var("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .collect();
    directories.extend([home.join(".local/bin"), home.join(".cargo/bin")]);
    directories.extend(["/usr/local/bin", "/usr/bin"].map(std::path::PathBuf::from));
    #[cfg(target_os = "macos")]
    {
        directories.push("/opt/homebrew/bin".into());
        directories.extend(macos::terminal_paths(Path::new("/etc"))?);
    }
    let mount = super::environment::var("APPDIR").map(std::path::PathBuf::from);
    let excluded = std::iter::once(bundled)
        .chain(mount.as_deref())
        .collect::<Vec<_>>();
    let path = std::env::join_paths(directories.into_iter().filter(|directory| {
        let canonical = std::fs::canonicalize(directory).unwrap_or_else(|_| directory.clone());
        !excluded.iter().any(|root| {
            canonical
                .starts_with(std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()))
        })
    }))
    .map_err(io::Error::other)?;
    match super::program::resolve_with(
        std::ffi::OsStr::new("artifactize"),
        &std::env::current_dir()?,
        |name| {
            if name == "PATH" {
                Some(path.clone())
            } else {
                super::environment::var(name)
            }
        },
    ) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
fn bounded_status(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
) -> io::Result<std::process::ExitStatus> {
    use std::{
        os::unix::process::CommandExt,
        time::{Duration, Instant},
    };
    // A short poll bounds shell/elevation waits without occupying the UI runtime.
    const POLL: Duration = Duration::from_millis(25);
    let mut child = command.process_group(0).spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            // SAFETY: process_group(0) made this unreaped child's nonzero PID its
            // group ID. Until wait reaps it, that ID cannot name an unrelated group.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "command registration timed out",
            ));
        }
        std::thread::sleep(POLL);
    }
}
