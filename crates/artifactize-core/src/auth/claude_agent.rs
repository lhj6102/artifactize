//! Claude Code's own sign-in, behind `login`, `logout` and Settings for the `claude-agent`
//! backend. artifactize only asks and launches: it runs the user's `claude auth status`,
//! `claude auth login` and `claude auth logout`, and never reads, copies or stores the login
//! Claude Code keeps.
//!
//! Without a terminal, `claude auth login` opens the browser itself, prints a URL to open
//! when the browser did not, and then waits until the browser returns to it or the code that
//! URL's page shows is written to its input.

use std::{io, process::ExitStatus, time::Duration};

use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::oneshot,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

use crate::{
    llm::claude_agent::{command, shown},
    platform::{self, environment},
    process,
};

/// `claude auth status` reads local files and answers at once; a stalled one is stopped well
/// before a person would give up on Settings.
const ASK_TIMEOUT: Duration = Duration::from_secs(25);
/// Give a person ten minutes to sign in, which on claude.ai may include a code sent by
/// email, without leaving Claude Code waiting indefinitely.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Keep this much of what Claude Code writes, for the error it fails with; it writes a few
/// lines.
const KEPT_BYTES: usize = 16 * 1024;

const MISSING: &str =
    "Claude Code (`claude`) is not on PATH; install it to use the claude-agent backend.";

/// Claude Code's sign-in, as Claude Code itself reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// `claude` is not on `PATH`.
    Missing,
    SignedOut,
    /// Signed in with `method` (for example `claude.ai`), on `plan` (for example `max`) when
    /// it names one.
    SignedIn {
        method: String,
        plan: Option<String>,
    },
    /// Claude Code could not tell, and why.
    Failed {
        error: String,
    },
}

/// The fields of `claude auth status --json` artifactize shows; the account's email and
/// organization are left unread.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reported {
    logged_in: bool,
    #[serde(default)]
    auth_method: Option<String>,
    #[serde(default)]
    subscription_type: Option<String>,
}

/// A run of `claude` with `args`, with the environment its reviews get.
fn claude(args: &[&str], timeout: Duration) -> process::Command {
    process::Command {
        program: command::PROGRAM.into(),
        args: args.iter().map(Into::into).collect(),
        cwd: platform::temporary_directory(),
        env: command::environment(environment::for_command()),
        timeout,
    }
}

/// Run `claude` with `args` to its end; `None` when it is not on `PATH`.
async fn ask(args: &[&str]) -> Result<Option<process::Output>, String> {
    match process::run_unregistered(
        claude(args, ASK_TIMEOUT),
        None,
        KEPT_BYTES,
        CancellationToken::new(),
    )
    .await
    {
        Ok(output) => Ok(Some(output)),
        Err(process::Error::Spawn(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(process::Error::Timeout) => Err(format!(
            "Claude Code did not answer within {} seconds.",
            ASK_TIMEOUT.as_secs()
        )),
        Err(error) => Err(format!("Cannot run Claude Code: {error}.")),
    }
}

/// Ask Claude Code whether it is signed in (`claude auth status --json`).
pub async fn status() -> Status {
    match ask(&["auth", "status", "--json"]).await {
        Ok(None) => Status::Missing,
        Ok(Some(output)) => read_status(&output.stdout, &output.stderr, &output.status),
        Err(error) => Status::Failed { error },
    }
}

/// What `claude auth status --json` printed. It prints its JSON also when signed out, then
/// with exit status 1.
fn read_status(stdout: &[u8], stderr: &[u8], ended: &ExitStatus) -> Status {
    match serde_json::from_slice::<Reported>(stdout) {
        Ok(reported) if reported.logged_in => Status::SignedIn {
            method: reported
                .auth_method
                .filter(|method| !method.is_empty())
                .unwrap_or_else(|| "unknown".into()),
            plan: reported.subscription_type.filter(|plan| !plan.is_empty()),
        },
        Ok(_) => Status::SignedOut,
        Err(_) => Status::Failed {
            error: failure("could not report its sign-in", stdout, stderr, ended),
        },
    }
}

/// Why Claude Code failed to `doing`: what it wrote on stderr, else on stdout, else how it
/// ended.
fn failure(doing: &str, stdout: &[u8], stderr: &[u8], ended: &ExitStatus) -> String {
    let said = [stderr, stdout]
        .into_iter()
        .map(|bytes| shown(&String::from_utf8_lossy(bytes)))
        .find(|text| !text.is_empty());
    match said {
        Some(said) => format!("Claude Code {doing}: {said}"),
        None => format!(
            "Claude Code {doing}; it ended with {}.",
            platform::exit_description(ended)
        ),
    }
}

/// Sign Claude Code out (`claude auth logout`).
pub async fn logout() -> Result<(), String> {
    let output = ask(&["auth", "logout"]).await?.ok_or(MISSING)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failure(
            "could not sign out",
            &output.stdout,
            &output.stderr,
            &output.status,
        ))
    }
}

/// What a sign-in asks the person to do: open `url` when the browser Claude Code opened did
/// not, and paste the code its page then shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub url: String,
}

/// The code Claude Code's sign-in page shows, as pasted: one line, written to Claude Code's
/// input as it is.
fn code(pasted: &str) -> Result<String, String> {
    let code = pasted.trim();
    if code.is_empty() || code.chars().any(char::is_control) {
        Err("Paste the code Claude Code's sign-in page shows, on one line.".into())
    } else {
        Ok(code.to_owned())
    }
}

/// The first `http(s)://` URL in `line`.
fn url_in(line: &str) -> Option<&str> {
    let start = line.find("https://").or_else(|| line.find("http://"))?;
    line[start..].split_whitespace().next()
}

/// Read `reader` to its end, keeping its last [`KEPT_BYTES`].
async fn tail(reader: Option<impl AsyncRead + Unpin>) -> Vec<u8> {
    let Some(mut reader) = reader else {
        return Vec::new();
    };
    let mut kept = Vec::new();
    let mut buffer = [0; 4096];
    while let Ok(count) = reader.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        kept.extend_from_slice(&buffer[..count]);
        if kept.len() > KEPT_BYTES {
            kept.drain(..kept.len() - KEPT_BYTES);
        }
    }
    kept
}

/// The sign-in behind `login claude-agent` for any interface: run `claude auth login`, give
/// `show` the URL it prints once it has opened the browser, and write a code arriving on
/// `pasted` to its input. It ends when Claude Code exits, signed in or with its error, or
/// after ten minutes; dropping the future stops Claude Code and
/// whatever it started, which Claude Code ending on its own leaves running.
pub async fn login_with(
    pasted: Option<oneshot::Receiver<String>>,
    show: impl FnOnce(&Prompt),
) -> Result<(), String> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut child = match process::spawn_handoff(&claude(&["auth", "login"], LOGIN_TIMEOUT)).await {
        Ok(child) => child,
        Err(process::Error::Spawn(error)) if error.kind() == io::ErrorKind::NotFound => {
            return Err(MISSING.into());
        }
        Err(error) => return Err(format!("Cannot start Claude Code: {error}.")),
    };
    let input = child.stdin().take();
    let stdout = child.stdout().take().expect("stdout is piped");
    let stderr = child.stderr().take();
    let signing_in = async {
        let mut lines = BufReader::new(stdout).lines();
        let mut said = Vec::new();
        let mut show = Some(show);
        // The URL comes first; what follows is kept for the error Claude Code may end with.
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = url_in(&line)
                && let Some(show) = show.take()
            {
                show(&Prompt {
                    url: url.to_owned(),
                });
                continue;
            }
            said.extend_from_slice(line.as_bytes());
            said.push(b'\n');
            if said.len() > KEPT_BYTES {
                said.drain(..said.len() - KEPT_BYTES);
            }
        }
        said
    };
    let paste = async {
        // Held open until a code arrives: without one, only the browser finishes the sign-in.
        let mut input = input;
        if let Some(pasted) = pasted
            && let Ok(pasted) = pasted.await
        {
            let code = code(&pasted)?;
            if let Some(mut input) = input.take() {
                // One code per sign-in: Claude Code reads it, then its input closes. A failed
                // write means Claude Code already exited, which ends the sign-in on its own.
                let _ = input.write_all(format!("{code}\n").as_bytes()).await;
            }
        }
        std::future::pending::<Result<(), String>>().await
    };
    let (said, errors, exited) = tokio::select! {
        biased;
        ended = async { tokio::join!(signing_in, tail(stderr), child.wait_leader()) } => ended,
        Err(error) = paste => return Err(error),
        () = tokio::time::sleep_until(deadline) => {
            return Err("Claude Code sign-in timed out; run `artifactize login claude-agent` again.".into());
        }
    };
    let exited = exited.map_err(|error| format!("Cannot wait for Claude Code: {error}."))?;
    // Claude Code ended on its own: what it started, such as the browser, goes on.
    child
        .release()
        .map_err(|error| format!("Cannot let go of Claude Code's sign-in: {error}."))?;
    if exited.success() {
        Ok(())
    } else {
        Err(failure("could not sign in", &said, &errors, &exited))
    }
}

/// Sign Claude Code in from a terminal: the URL goes to stderr, also with --json, and a code
/// pasted into this process's input goes to Claude Code.
pub async fn login() -> Result<(), String> {
    let (send, receive) = oneshot::channel();
    // Detached: a sign-in finished in the browser leaves this read unanswered.
    std::thread::Builder::new()
        .name("claude-agent-login-input".into())
        .spawn(move || {
            let line = std::io::stdin()
                .lines()
                .map_while(Result::ok)
                .find(|line| !line.trim().is_empty());
            if let Some(line) = line {
                let _ = send.send(line);
            }
        })
        .map_err(|error| format!("Cannot start Claude Code sign-in input: {error}"))?;
    login_with(Some(receive), |prompt| {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr().lock(),
            "Claude Code opened the browser to sign in. If it did not open, open this URL, then paste the code its page shows and press Enter:\n{}",
            prompt.url
        );
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_in_status_names_the_method_and_plan_but_not_the_account() {
        let stdout = br#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","email":"a@example.com","orgName":"A","subscriptionType":"max"}"#;
        assert_eq!(
            read_status(stdout, b"", &ExitStatus::default()),
            Status::SignedIn {
                method: "claude.ai".into(),
                plan: Some("max".into())
            }
        );
    }

    #[test]
    fn a_signed_out_status_is_read_from_its_json() {
        let stdout = br#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#;
        assert_eq!(
            read_status(stdout, b"", &ExitStatus::default()),
            Status::SignedOut
        );
    }

    #[test]
    fn an_unreadable_status_is_claude_codes_own_error() {
        assert_eq!(
            read_status(
                b"",
                b"error: unknown command 'auth'\n",
                &ExitStatus::default()
            ),
            Status::Failed {
                error: "Claude Code could not report its sign-in: error: unknown command 'auth'"
                    .into()
            }
        );
    }

    #[test]
    fn the_printed_url_is_found_in_its_line() {
        assert_eq!(
            url_in(
                "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&state=s"
            ),
            Some("https://claude.com/cai/oauth/authorize?code=true&state=s")
        );
        assert_eq!(url_in("Opening browser to sign in…"), None);
    }

    #[test]
    fn a_pasted_code_is_one_line() {
        assert_eq!(code("  abc#def\n").unwrap(), "abc#def");
        assert!(code("abc\ndef").is_err());
        assert!(code("  ").is_err());
    }
}
