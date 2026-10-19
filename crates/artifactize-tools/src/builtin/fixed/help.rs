//! Bounded help-process execution through the declared launcher.
use super::TEXT_BYTES;
use crate::{
    launch::{Launch, LaunchError, Launcher},
    program,
};
use std::{
    ffi::{OsStr, OsString},
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
/// Help is documentation, not a long-running owner command.
const HELP_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn help(
    args: &[String],
    cwd: &Path,
    launcher: &dyn Launcher,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    let program = program::resolve(OsStr::new(&args[0]), cwd).map_err(|error| error.to_string())?;
    let launch = Launch {
        program,
        args: args[1..]
            .iter()
            .map(OsString::from)
            .chain([OsString::from("--help")])
            .collect(),
        cwd: cwd.to_owned(),
        timeout: HELP_TIMEOUT,
        output_limit: TEXT_BYTES,
    };
    let finished = launcher
        .run(launch, cancellation)
        .await
        .map_err(|error| match error {
            LaunchError::TimedOut => "Help timed out.".to_owned(),
            error => error.to_string(),
        })?;
    if !crate::platform::succeeded(&finished.status) {
        return Err(format!(
            "Help exited unsuccessfully ({}).\n{}",
            crate::platform::exit_description(&finished.status),
            String::from_utf8_lossy(&finished.stderr)
        ));
    }
    let mut bytes = finished.stdout;
    bytes.extend(finished.stderr);
    if finished.truncated || bytes.len() > TEXT_BYTES {
        return Err("Help output exceeds 64 KiB.".into());
    }
    Ok(String::from_utf8(crate::result::clean_output(&bytes)).expect("clean output is UTF-8"))
}
