//! Antigravity's native Google sign-in, built on top of the unified ACP layer.
//! URLs and a single pasted code are relayed to the official CLI; its credentials and vendor
//! files never pass through Artifactize.

use std::{ffi::OsString, path::PathBuf, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::oneshot,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::{
    acp::{
        AgentId, Problem,
        catalog::{
            ANTIGRAVITY_MINIMUM, AgentAvailability, AgentSpec, PROBE_TIMEOUT, bump_auth_generation,
            invalidate_probe_cache, preset_by_id, probe_spec,
        },
        wire::{AUTH_LINK_MARKER, Connection},
    },
    agent::error::Failure,
    llm::models::ListedModel,
    platform, process,
};

/// A person may need to confirm the account in a browser before pasting the code.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Enough room for a Google authorization URL or a full callback URL, not unbounded output.
const LOGIN_LINE_BYTES: u64 = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Missing,
    TooOld { error: String },
    SignedOut,
    SignedIn { account: Option<String> },
    Quota { error: String },
    RateLimited { error: String },
    Failed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub path: Option<PathBuf>,
    pub version: Option<semver::Version>,
    pub required_version: semver::Version,
    pub state: State,
}

fn antigravity_spec() -> AgentSpec {
    AgentSpec::from_preset(preset_by_id("antigravity").expect("antigravity preset exists"))
}

fn antigravity_id() -> AgentId {
    "antigravity".parse().expect("antigravity id is valid")
}

/// Probe Antigravity in an empty private tempdir without authenticating or sending a prompt.
pub async fn status() -> Status {
    let spec = antigravity_spec();
    match probe_spec(&spec, true).await {
        AgentAvailability::Ready {
            executable,
            version,
            ..
        } => Status {
            path: Some(executable),
            version: version.and_then(|v| semver::Version::parse(&v).ok()),
            required_version: ANTIGRAVITY_MINIMUM,
            state: State::SignedIn { account: None },
        },
        AgentAvailability::Missing { .. } => Status {
            path: None,
            version: None,
            required_version: ANTIGRAVITY_MINIMUM,
            state: State::Missing,
        },
        AgentAvailability::TooOld {
            executable,
            version,
            reason,
            ..
        } => Status {
            path: Some(executable),
            version: semver::Version::parse(&version).ok(),
            required_version: ANTIGRAVITY_MINIMUM,
            state: State::TooOld { error: reason },
        },
        AgentAvailability::NotSetUp {
            executable,
            version,
            ..
        } => Status {
            path: executable,
            version: version.and_then(|v| semver::Version::parse(&v).ok()),
            required_version: ANTIGRAVITY_MINIMUM,
            state: State::SignedOut,
        },
        AgentAvailability::Failed {
            executable,
            version,
            error,
        } => {
            let state = match Problem::classify_text("Antigravity", "", &error) {
                Some(Problem::Quota { .. }) => State::Quota { error },
                Some(Problem::RateLimit { .. }) => State::RateLimited { error },
                Some(Problem::SignedOut { .. }) => State::SignedOut,
                _ => State::Failed { error },
            };
            Status {
                path: executable,
                version: version.and_then(|v| semver::Version::parse(&v).ok()),
                required_version: ANTIGRAVITY_MINIMUM,
                state,
            }
        }
    }
}

pub async fn models() -> Result<Vec<ListedModel>, String> {
    let spec = antigravity_spec();
    match probe_spec(&spec, true).await {
        AgentAvailability::Ready { models, .. } => Ok(models),
        AgentAvailability::Missing { reason } => Err(reason),
        AgentAvailability::NotSetUp { reason, .. } => Err(format!(
            "Antigravity needs your Google sign-in. {reason} Run `artifactize login antigravity` or use Settings → Chat Agents."
        )),
        AgentAvailability::TooOld { reason, .. } => Err(reason),
        AgentAvailability::Failed { error, .. } => Err(error),
    }
}

pub async fn logout() -> Result<(), String> {
    let spec = antigravity_spec();
    let probe_dir =
        platform::private_tempdir_in("artifactize-acp-", &platform::temporary_directory())
            .map_err(|error| format!("Cannot create Antigravity logout directory: {error}."))?;
    let resolved = crate::acp::trust::resolve_executable(&spec.program, None, false)?;
    let mut args: Vec<OsString> = spec.args.iter().map(OsString::from).collect();
    args.push("--sandbox".into());
    args.push("--include-default-customizations=false".into());
    let command = process::Command {
        program: resolved.into_os_string(),
        args,
        cwd: probe_dir.path().to_path_buf(),
        env: spec.build_env(false),
        timeout: PROBE_TIMEOUT,
    };
    let cancel = CancellationToken::new();
    let mut child = process::spawn_supervised(&command, &cancel)
        .await
        .map_err(|error| format!("Cannot start Antigravity: {error}."))?;
    let stdout = child.stdout().take().expect("stdout piped");
    let stdin = child.stdin().take().expect("stdin piped");
    let mut stderr = child.stderr().take().expect("stderr piped");
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = tokio::io::AsyncReadExt::read_to_end(&mut stderr, &mut buf).await;
        buf
    });
    let mut conn = Connection::new(
        &spec.name,
        &spec.install_hint,
        stdout,
        stdin,
        false,
        spec.known_secrets(),
    );
    let result = tokio::time::timeout(PROBE_TIMEOUT, conn.logout())
        .await
        .unwrap_or_else(|_| {
            Err(Problem::Failed(
                "Antigravity did not answer ACP logout in time.".into(),
            ))
        });
    drop(conn);
    let _ = process::terminate(&mut child).await;
    let _ = child.cleanup().await;
    let stderr_bytes = crate::task::joined(stderr_task.await).unwrap_or_default();
    let stderr_text = platform::output_text(&stderr_bytes);

    bump_auth_generation();
    invalidate_probe_cache(&antigravity_id());

    match result {
        Ok(()) => Ok(()),
        Err(problem) => {
            let final_problem = if stderr_text.contains(AUTH_LINK_MARKER) {
                Problem::SignedOut {
                    agent: spec.name,
                    hint: spec.install_hint,
                }
            } else {
                problem
            };
            Err(Failure::from(final_problem).message)
        }
    }
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub url: url::Url,
}

/// Run the native manual sign-in with a URL relay and a one-time stdin paste. Dropping the
/// future stops the owned CLI; finishing successfully leaves its native login in place.
pub async fn login_with(
    pasted: oneshot::Receiver<String>,
    show: impl FnOnce(&Prompt),
) -> Result<(), String> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let spec = antigravity_spec();
    let directory =
        platform::private_tempdir_in("artifactize-acp-", &platform::temporary_directory())
            .map_err(|error| format!("Cannot create Antigravity sign-in directory: {error}."))?;
    let executable = crate::acp::trust::resolve_executable(&spec.program, None, false)?;
    // Verify minimum CLI version before launching interactive login.
    let version_cmd = process::Command {
        program: executable.as_os_str().to_owned(),
        args: vec!["--version".into()],
        cwd: directory.path().to_path_buf(),
        env: spec.build_env(false),
        timeout: PROBE_TIMEOUT,
    };
    if let Ok(output) =
        process::run(version_cmd, CancellationToken::new(), |_| async { Ok(()) }).await
        && output.status.success()
        && let Some(ver) = platform::output_text(&output.stdout)
            .split_whitespace()
            .find_map(|word| semver::Version::parse(word.trim_start_matches('v')).ok())
        && ver < ANTIGRAVITY_MINIMUM
    {
        return Err(format!(
            "Antigravity CLI {ver} is too old; update `agy` to {ANTIGRAVITY_MINIMUM} or newer."
        ));
    }

    let command = process::Command {
        program: executable.into_os_string(),
        args: vec!["acp".into(), "--login".into()],
        cwd: directory.path().to_path_buf(),
        env: spec.build_env(false),
        timeout: deadline.saturating_duration_since(Instant::now()),
    };
    let mut cli = process::spawn_handoff(&command)
        .await
        .map_err(|error| format!("Cannot start Antigravity: {error}."))?;
    let mut input = cli.stdin().take().expect("sign-in stdin is piped");
    let mut output = BufReader::new(cli.stdout().take().expect("sign-in stdout is piped"));
    let mut errors = BufReader::new(cli.stderr().take().expect("sign-in stderr is piped"));
    let mut read_output = true;
    let mut read_errors = true;
    let mut output_line = Vec::new();
    let mut error_line = Vec::new();
    let sign_in = async {
        let url = loop {
            let line = tokio::select! {
                result = url_line(&mut output, &mut output_line), if read_output => {
                    let (open, url) = result?;
                    read_output = open;
                    url
                }
                result = url_line(&mut errors, &mut error_line), if read_errors => {
                    let (open, url) = result?;
                    read_errors = open;
                    url
                }
                else => break None,
            };
            if line.is_some() {
                break line;
            }
        };
        if let Some(url) = url {
            show(&Prompt { url });
            let paste = async {
                let pasted = pasted
                    .await
                    .map_err(|_| "Antigravity sign-in was cancelled.")?;
                let code = pasted.trim();
                if code.is_empty()
                    || code.len() as u64 > LOGIN_LINE_BYTES
                    || code.chars().any(char::is_control)
                {
                    return Err("Paste the Antigravity sign-in code on one line.");
                }
                let _ = input.write_all(format!("{code}\n").as_bytes()).await;
                drop(input);
                std::future::pending::<Result<(), &str>>().await
            };
            let exited = finish_login(&mut cli, output, errors);
            tokio::select! {
                result = exited => result,
                result = paste => result,
            }
        } else {
            finish_login(&mut cli, output, errors).await
        }
    };
    let result = tokio::time::timeout_at(deadline, sign_in)
        .await
        .map_err(|_| "Antigravity sign-in timed out. Start sign-in again.")?;
    result?;
    cli.release()
        .map_err(|_| "Cannot release the finished Antigravity sign-in.".to_owned())?;
    bump_auth_generation();
    invalidate_probe_cache(&antigravity_id());
    Ok(())
}

async fn finish_login(
    cli: &mut platform::Child,
    mut output: impl AsyncRead + Unpin,
    mut errors: impl AsyncRead + Unpin,
) -> Result<(), &'static str> {
    let (mut discard_output, mut discard_errors) = (tokio::io::sink(), tokio::io::sink());
    let (ended, _, _) = tokio::join!(
        cli.wait_leader(),
        tokio::io::copy(&mut output, &mut discard_output),
        tokio::io::copy(&mut errors, &mut discard_errors)
    );
    if ended.is_ok_and(|ended| ended.success()) {
        Ok(())
    } else {
        Err(
            "Antigravity could not sign in. Start again, or sign in directly in the Antigravity CLI.",
        )
    }
}

async fn url_line(
    reader: &mut (impl AsyncBufReadExt + Unpin),
    line: &mut Vec<u8>,
) -> Result<(bool, Option<url::Url>), &'static str> {
    let remaining = LOGIN_LINE_BYTES.saturating_sub(line.len() as u64);
    let read = reader
        .take(remaining + 1)
        .read_until(b'\n', line)
        .await
        .map_err(|_| "Cannot read the Antigravity sign-in prompt.")?;
    if line.len() as u64 > LOGIN_LINE_BYTES {
        return Err("Antigravity wrote an oversized sign-in prompt.");
    }
    let text = platform::output_text(line);
    let url = text
        .split_whitespace()
        .filter_map(|word| url::Url::parse(word).ok())
        .find(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("accounts.google.com")
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
                && matches!(url.path(), "/o/oauth2/auth" | "/o/oauth2/v2/auth")
                && url.query_pairs().any(|(key, value)| {
                    key == "redirect_uri" && value == "https://antigravity.google/oauth-callback"
                })
        });
    line.clear();
    Ok((read != 0, url))
}
