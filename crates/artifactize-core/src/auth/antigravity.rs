//! Antigravity's native Google sign-in, built on top of the unified ACP layer.
//! URLs and a single pasted code are relayed to the official CLI; its credentials and vendor
//! files never pass through Artifactize.

use std::{path::PathBuf, time::Duration};

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
            check_probe_dir_isolated, invalidate_probe_cache, logout_spec, preset_by_id,
            probe_spec,
        },
    },
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
    match probe_spec(&spec, None, true).await {
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

/// The models Antigravity offers, probed as for `repo`: an executable inside it is never run.
pub async fn models(repo: Option<&std::path::Path>) -> Result<Vec<ListedModel>, String> {
    let spec = antigravity_spec();
    match probe_spec(&spec, repo, true).await {
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
    logout_spec(&antigravity_spec()).await
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
    let executable = crate::acp::trust::resolve_executable(&spec.program, None, false)?;
    // Verify minimum CLI version in its own isolated empty private tempdir before interactive login.
    let version_dir =
        platform::private_tempdir_in("artifactize-acp-version-", &platform::temporary_directory())
            .map_err(|error| {
                format!("Cannot create Antigravity version check directory: {error}.")
            })?;
    check_probe_dir_isolated(version_dir.path(), None).map_err(|error| {
        format!("Refusing to run Antigravity version check inside a workspace: {error}")
    })?;
    let version_cmd = process::Command {
        program: executable.as_os_str().to_owned(),
        args: vec!["--version".into()],
        cwd: version_dir.path().to_path_buf(),
        env: spec.build_env(true),
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
    drop(version_dir);

    let login_dir =
        platform::private_tempdir_in("artifactize-acp-login-", &platform::temporary_directory())
            .map_err(|error| format!("Cannot create Antigravity sign-in directory: {error}."))?;
    check_probe_dir_isolated(login_dir.path(), None).map_err(|error| {
        format!("Refusing to run Antigravity sign-in inside a workspace: {error}")
    })?;
    let command = process::Command {
        program: executable.into_os_string(),
        args: vec!["acp".into(), "--login".into()],
        cwd: login_dir.path().to_path_buf(),
        env: spec.build_env(true),
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
