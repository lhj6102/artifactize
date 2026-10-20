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
            terminal_has_command()?,
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
        appimage::register(
            executable
                .parent()
                .ok_or_else(|| io::Error::other("no executable directory"))?,
            &data.join("artifactize/bin"),
            &home.join(".local/bin"),
            env!("CARGO_PKG_VERSION"),
            terminal_has_command()?,
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = state;
        Ok(()) // Windows installers and Store execution aliases own PATH.
    }
}

// Interactive login startup files reconstruct a terminal's PATH (including .zshrc
// and .bashrc), unlike Finder's or AppRun's environment. Start from the system path,
// not the GUI's inherited PATH, which can include the temporary AppImage mount.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn terminal_has_command() -> io::Result<bool> {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    let shell = super::environment::var("SHELL")
        .filter(|shell| !shell.is_empty())
        .ok_or_else(|| io::Error::other("no login shell in SHELL"))?;
    let mut command = Command::new(shell);
    command
        .args(["-l", "-i", "-c", "command -v artifactize >/dev/null 2>&1"])
        .env("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Shell startup may run arbitrary user programs. Never let it hold up registration;
    // an unknown result is an error, not permission to overwrite an existing install.
    bounded_status(&mut command, Duration::from_secs(5)).map(|status| status.success())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
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
