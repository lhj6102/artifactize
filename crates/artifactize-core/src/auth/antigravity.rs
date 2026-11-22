//! Antigravity's native Google sign-in. URLs and a single pasted code are relayed to the
//! official CLI; its credentials and vendor files never pass through Artifactize.
use std::time::Duration;

use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::oneshot,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::{
    agent::error::Failure,
    llm::{
        antigravity::{self, Acp, PROBE_TIMEOUT, Problem},
        models::ListedModel,
    },
    platform, process,
};

/// A person may need to confirm the account in a browser before pasting the code.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Enough room for a Google authorization URL or a full callback URL, not unbounded output.
const LOGIN_LINE_BYTES: u64 = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Missing,
    TooOld { version: String, error: String },
    SignedOut,
    SignedIn,
    Quota { error: String },
    RateLimited { error: String },
    Failed { error: String },
}

impl From<Problem> for Status {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::Missing => Self::Missing,
            Problem::SignedOut => Self::SignedOut,
            Problem::TooOld { ref version } => Self::TooOld {
                version: version.to_string(),
                error: Failure::from(problem).message,
            },
            Problem::Quota => Self::Quota {
                error: Failure::from(problem).message,
            },
            Problem::RateLimit => Self::RateLimited {
                error: Failure::from(problem).message,
            },
            Problem::Failed(error) => Self::Failed { error },
        }
    }
}

/// The check never authenticates or sends a prompt. `session/new` distinguishes initialized
/// but signed-out agents from ones that can start a session.
pub async fn status() -> Status {
    match probe(Acp::inspect).await {
        Ok(_) => Status::SignedIn,
        Err(problem) => problem.into(),
    }
}

pub async fn models() -> Result<Vec<ListedModel>, String> {
    probe(Acp::inspect).await.map_err(message)
}

pub async fn logout() -> Result<(), String> {
    probe(Acp::logout).await.map_err(message)
}

async fn probe<T>(
    ask: impl for<'a> AsyncFnOnce(&'a mut Acp) -> Result<T, Problem>,
) -> Result<T, Problem> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let cancel = CancellationToken::new();
    let mut acp = Acp::start(deadline, &cancel).await?;
    let result = tokio::time::timeout_at(deadline, ask(&mut acp))
        .await
        .unwrap_or_else(|_| {
            Err(Problem::Failed(
                "Antigravity did not answer the ACP probe in time.".into(),
            ))
        });
    let diagnostic = acp.stop().await;
    result.map_err(|problem| match problem {
        Problem::Failed(_) => diagnostic.unwrap_or(problem),
        _ => problem,
    })
}

fn message(problem: Problem) -> String {
    Failure::from(problem).message
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
    let directory = antigravity::workspace().map_err(message)?;
    antigravity::check_version(directory.path(), deadline, &CancellationToken::new())
        .await
        .map_err(message)?;
    let command = antigravity::command(&["acp", "--login"], directory.path(), deadline);
    let mut cli = process::spawn_handoff(&command)
        .await
        .map_err(antigravity::start_problem)
        .map_err(message)?;
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
        .map_err(|_| "Cannot release the finished Antigravity sign-in.".to_owned())
}

async fn finish_login(
    cli: &mut platform::Child,
    mut output: impl AsyncRead + Unpin,
    mut errors: impl AsyncRead + Unpin,
) -> Result<(), &'static str> {
    // Drain both pipes while waiting; never include sign-in output or the code in a diagnostic.
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
