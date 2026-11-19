//! Bounded help-process execution through the declared launcher.
use super::TEXT_BYTES;
use crate::{
    launch::{Finished, Launch, LaunchError, Launcher},
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
    program: &OsStr,
    mut args: Vec<OsString>,
    cwd: &Path,
    launcher: &dyn Launcher,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    let program = program::resolve(program, cwd).map_err(|error| error.to_string())?;
    args.push(OsString::from("--help"));
    let launch = Launch {
        program,
        args,
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
    help_output(finished)
}

fn help_output(finished: Finished) -> Result<String, String> {
    if !finished.status.success() {
        return Err(format!(
            "Help exited unsuccessfully ({}).\n{}",
            finished.status,
            crate::platform::text::output(&finished.stderr)
        ));
    }
    if finished.truncated || finished.stdout.len() + finished.stderr.len() > TEXT_BYTES {
        return Err("Help output exceeds 64 KiB.".into());
    }
    let mut bytes = crate::terminal_text::clean_output(&finished.stdout);
    bytes.extend(crate::terminal_text::clean_output(&finished.stderr));
    if bytes.len() > TEXT_BYTES {
        return Err("Help output exceeds 64 KiB.".into());
    }
    Ok(String::from_utf8(bytes).expect("clean output is UTF-8"))
}
