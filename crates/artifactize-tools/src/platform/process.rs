//! The standalone command's process tree: start a program with an explicit environment,
//! capture its output, and kill it with its children when it ends, times out or is
//! cancelled.

use std::{collections::BTreeMap, ffi::OsString, process::Stdio};

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

use crate::launch::{Finished, Launch, LaunchError, ProcessOutcome};

/// Read one extra byte to detect output beyond the requested limit.
const OUTPUT_PROBE_BYTES: usize = 1;

/// Run one program to completion in its own process group or Job Object.
pub(crate) async fn run(
    launch: Launch,
    cancellation: &CancellationToken,
) -> Result<Finished, LaunchError> {
    let limit = launch.output_limit;
    let capture_limit = limit.checked_add(OUTPUT_PROBE_BYTES).ok_or_else(|| {
        LaunchError::Failed("Output limit leaves no room for the truncation probe.".into())
    })? as u64;
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
    let execution = async {
        let (stdout, stderr, status) = tokio::try_join!(
            capture(stdout, capture_limit),
            capture(stderr, capture_limit),
            tree.child.wait()
        )
        .map_err(|error| LaunchError::Failed(error.to_string()))?;
        Ok(Finished {
            status: ProcessOutcome::from_native(status),
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
    // Expiry/cancellation must reap the direct child before returning, not leave it
    // to Tokio's best-effort orphan queue when the caller shuts down its runtime.
    let _ = tree.child.wait().await;
    result
}

async fn capture(reader: impl AsyncRead + Unpin, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes).await?;
    Ok(bytes)
}

/// The variables a standalone program receives: the person's whole environment, as in the
/// author's terminal, without artifactize's own `ARTIFACTIZE_*` variables (its credentials and
/// settings).
pub(crate) fn passed_environment() -> BTreeMap<OsString, OsString> {
    super::environment::user()
        .into_iter()
        .filter(|(name, _)| {
            !name
                .to_str()
                .is_some_and(|name| name.to_ascii_uppercase().starts_with("ARTIFACTIZE_"))
        })
        .collect()
}
