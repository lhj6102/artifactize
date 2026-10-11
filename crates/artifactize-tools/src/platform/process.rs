//! The standalone command's process tree: start a program with an explicit environment,
//! capture its output, and kill it with its children when it ends, times out or is
//! cancelled.

use std::{collections::BTreeMap, ffi::OsString, process::Stdio};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

use crate::launch::{Finished, Launch, LaunchError};

/// Run one program to completion in its own process group or Job Object.
pub(crate) async fn run(
    launch: Launch,
    cancellation: &CancellationToken,
) -> Result<Finished, LaunchError> {
    let mut command = tokio::process::Command::new(&launch.program);
    command
        .args(&launch.args)
        .current_dir(&launch.cwd)
        .env_clear()
        .envs(passed_environment())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut tree =
        super::spawn_tree(&mut command).map_err(|error| LaunchError::Failed(error.to_string()))?;
    let stdout = tree.child.stdout.take().expect("piped stdout");
    let stderr = tree.child.stderr.take().expect("piped stderr");
    let limit = launch.output_limit;
    let execution = async {
        let (stdout, stderr, status) = tokio::try_join!(
            capture(stdout, limit),
            capture(stderr, limit),
            tree.child.wait()
        )
        .map_err(|error| LaunchError::Failed(error.to_string()))?;
        Ok(Finished {
            status,
            truncated: stdout.len() > limit || stderr.len() > limit,
            stdout,
            stderr,
        })
    };
    let result = tokio::select! {
        _ = cancellation.cancelled() => Err(LaunchError::Cancelled),
        result = tokio::time::timeout(launch.timeout, execution) => {
            result.unwrap_or(Err(LaunchError::TimedOut))
        }
    };
    tree.kill();
    result
}

/// Read at most one byte past `limit`, enough to tell that a stream was too long.
async fn capture(reader: impl AsyncRead + Unpin, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    Ok(bytes)
}

/// The variables a standalone program receives: `PATH`, the locale, and the home, temporary
/// and system variables the platform's programs need to start; nothing else is inherited.
pub(crate) fn passed_environment() -> BTreeMap<OsString, OsString> {
    ["PATH", "LANG", "LC_ALL"]
        .into_iter()
        .chain(super::SYSTEM_VARIABLES.iter().copied())
        .filter_map(|name| std::env::var_os(name).map(|value| (name.into(), value)))
        .collect()
}
