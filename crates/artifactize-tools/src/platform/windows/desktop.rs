//! Windows desktop handoff through ShellExecute.

use std::{ffi::OsStr, io, os::windows::ffi::OsStrExt, ptr};
use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

/// Hand the target to ShellExecute's default verb on a blocking thread.
pub(crate) async fn open_desktop(target: &OsStr) -> io::Result<()> {
    #[cfg(feature = "fixture-opener")]
    if let Some(program) = std::env::var_os("ARTIFACTIZE_E2E_BROWSER_PROGRAM") {
        let node = std::env::var_os("ARTIFACTIZE_E2E_BROWSER_NODE").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the fixture opener requires its declared executable",
            )
        })?;
        super::check_launch_program(std::path::Path::new(&node))?;
        let status = tokio::process::Command::new(node)
            .arg(program)
            .arg(target)
            .status()
            .await?;
        return if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("fixture opener exited {status}")))
        };
    }
    let target = target.to_owned();
    match tokio::task::spawn_blocking(move || shell_execute(&target)).await {
        Ok(result) => result,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => Err(io::Error::other(error)),
    }
}

/// ShellExecuteW returns an error code at or below 32; greater values mean a successful
/// desktop handoff, not a handle that this process owns.
const SHELL_EXECUTE_ERROR_MAX: isize = 32;

fn shell_execute(target: &OsStr) -> io::Result<()> {
    handoff(target, |target| {
        // SAFETY: handoff supplies a NUL-terminated target alive for this call.
        // No verb, parameters or directory: the file/URL is one literal target.
        unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                ptr::null(),
                target.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            ) as isize
        }
    })
}

/// The native handoff boundary can be observed without changing a user's default apps.
fn handoff(target: &OsStr, execute: impl FnOnce(&[u16]) -> isize) -> io::Result<()> {
    let mut target: Vec<_> = target.encode_wide().collect();
    if target.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Open target contains NUL.",
        ));
    }
    target.push(0);
    let result = execute(&target);
    // ShellExecute returns a value greater than 32 on success, not an owned handle.
    if result <= SHELL_EXECUTE_ERROR_MAX {
        return Err(io::Error::other(format!(
            "ShellExecute could not open the target (code {result})."
        )));
    }
    Ok(())
}

// Windows has a ShellExecute handoff, not a Unix opener subprocess. Observe its
// native argument seam without launching a browser or changing file associations.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_is_one_literal_argument() {
        for target in [
            "notes with spaces.md",
            "https://example.invalid/?a=1&b=two words",
            "한글 문서.md",
        ] {
            let mut calls = Vec::new();
            handoff(OsStr::new(target), |argument| {
                calls.push(argument.to_vec());
                33 // ShellExecute's smallest successful handoff value.
            })
            .unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].last(), Some(&0));
            assert_eq!(
                String::from_utf16(&calls[0][..calls[0].len() - 1]).unwrap(),
                target
            );
        }
    }

    #[test]
    fn invalid_target_never_reaches_the_desktop_and_native_errors_are_reported() {
        let invalid = handoff(OsStr::new("before\0after"), |_| {
            panic!("NUL reached ShellExecute")
        });
        assert_eq!(invalid.unwrap_err().kind(), io::ErrorKind::InvalidInput);
        assert!(
            handoff(OsStr::new("missing.md"), |_| 2)
                .unwrap_err()
                .to_string()
                .contains("code 2")
        );
    }
}
