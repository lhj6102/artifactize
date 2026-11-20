//! Interactive OAuth, from the request state through the authorization-code exchange.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    io::{self, Write},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

/// A stalled browser must not hold up a completed callback while receiving its short reply.
pub(super) const CALLBACK_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// Release the callback listener from a client that stops sending its request headers.
pub(super) const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Give a person five minutes to finish browser or pasted-redirect sign-in,
/// without leaving an unattended callback listener alive indefinitely.
pub(super) const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

/// Choose 128 unpredictable bits for OAuth state/CSRF correlation, encoded as 32 hex
/// characters. This is this client's chosen size, not an OAuth-mandated exact length.
pub(super) const OAUTH_STATE_BYTES: usize = 16;

/// 32 random bytes give 256 bits and 43 unpadded base64url characters, within RFC 7636's
/// permitted 43–128-character code_verifier length; the S256 challenge hashes that text.
pub(super) const PKCE_VERIFIER_BYTES: usize = 32;

pub(super) const SCOPE: &str = "openid profile email offline_access";

pub(super) const CALLBACK_PATH: &str = "/auth/callback";

/// Match the localhost callback convention of the Codex public-client flow that Pi
/// also uses; listener and OAuth redirect must agree for browser/pasted-URL interoperability.
pub(super) const CALLBACK_PORT: u16 = 1455;

pub(super) fn random(bytes: usize) -> Result<Vec<u8>, String> {
    let mut value = vec![0; bytes];
    getrandom::fill(&mut value).map_err(|_| "Cannot obtain secure randomness.".to_owned())?;
    Ok(value)
}

/// One sign-in attempt: PKCE verifier, its S256 challenge and the `state` it expects.
pub(super) struct Pending {
    pub(super) verifier: String,
    pub(super) state: crate::types::OAuthState,
}

impl Pending {
    pub(super) fn new() -> Result<Self, String> {
        let state: String = random(OAUTH_STATE_BYTES)?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok(Self {
            verifier: URL_SAFE_NO_PAD.encode(random(PKCE_VERIFIER_BYTES)?),
            state: state.parse().expect("generated OAuth state is valid"),
        })
    }

    pub(super) fn challenge(&self) -> String {
        URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()))
    }

    pub(super) fn authorize_url(&self, root: &Url, redirect_uri: &str) -> Result<Url, String> {
        let mut url = endpoint(root, "authorize")?;
        url.query_pairs_mut().extend_pairs([
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", redirect_uri),
            ("scope", SCOPE),
            ("code_challenge", &self.challenge()),
            ("code_challenge_method", "S256"),
            ("state", &self.state),
            ("id_token_add_organizations", "true"),
            ("codex_cli_simplified_flow", "true"),
            ("originator", ORIGINATOR),
        ]);
        Ok(url)
    }

    /// The code from a pasted redirect URL, `code#state`, a query string or a bare code,
    /// as Pi accepts them. A pasted state must match.
    pub(super) fn pasted(&self, input: &str) -> Result<String, String> {
        let input = input.trim();
        let (code, state) = if let Ok(url) = Url::parse(input) {
            let pairs: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
            (pairs.get("code").cloned(), pairs.get("state").cloned())
        } else if let Some((code, state)) = input.split_once('#') {
            (Some(code.to_owned()), Some(state.to_owned()))
        } else if input.contains("code=") {
            let pairs: BTreeMap<_, _> = url::form_urlencoded::parse(input.as_bytes())
                .into_owned()
                .collect();
            (pairs.get("code").cloned(), pairs.get("state").cloned())
        } else {
            (Some(input.to_owned()), None)
        };
        if state.is_some_and(|state| state != self.state) {
            return Err("Codex sign-in state mismatch.".into());
        }
        code.filter(|code| !code.is_empty())
            .ok_or_else(|| "The pasted input has no authorization code.".into())
    }

    /// Answer one callback request. `Some` ends the sign-in; `None` keeps waiting.
    pub(super) fn callback(
        &self,
        target: &str,
    ) -> (&'static str, &'static str, Option<Result<String, String>>) {
        let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
            return ("400 Bad Request", "Invalid callback.", None);
        };
        if url.path() != CALLBACK_PATH {
            return ("404 Not Found", "Not found.", None);
        }
        let mut parameters = BTreeMap::new();
        for (key, value) in url.query_pairs().into_owned() {
            if parameters.insert(key, value).is_some() {
                return ("400 Bad Request", "Duplicate callback parameter.", None);
            }
        }
        if parameters.get("state").map(String::as_str) != Some(self.state.as_str()) {
            return ("400 Bad Request", "State mismatch.", None);
        }
        if let Some(error) = parameters.get("error") {
            let error: String = error
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .take(MAX_OAUTH_ERROR_CHARS)
                .collect();
            return (
                "400 Bad Request",
                "Codex sign-in was not completed. Return to artifactize.",
                Some(Err(format!("Codex sign-in failed ({error})."))),
            );
        }
        match parameters.get("code").filter(|code| !code.is_empty()) {
            Some(code) => (
                "200 OK",
                "Signed in to Codex. Return to artifactize; you can close this page.",
                Some(Ok(code.clone())),
            ),
            None => ("400 Bad Request", "Missing authorization code.", None),
        }
    }

    /// Serve the loopback callback until one completes the sign-in. Requests must be
    /// `GET` with a loopback `Host` matching the reserved port.
    pub(super) async fn serve(&self, listener: TcpListener) -> Result<String, String> {
        // Allow browser callback headers and the authorization URL while bounding
        // memory consumed by an untrusted loopback client before HTTP validation.
        const MAX_CALLBACK_HEADER_BYTES: usize = 16 * 1024;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let hosts = [
            format!("localhost:{port}"),
            format!("127.0.0.1:{port}"),
            format!("[::1]:{port}"),
        ];
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let head = tokio::time::timeout(CALLBACK_READ_TIMEOUT, async {
                let mut head = Vec::new();
                let mut byte = [0];
                while !head.ends_with(b"\r\n\r\n") && head.len() < MAX_CALLBACK_HEADER_BYTES {
                    if stream.read(&mut byte).await? == 0 {
                        break;
                    }
                    head.push(byte[0]);
                }
                Ok::<_, std::io::Error>(head)
            })
            .await;
            let Ok(Ok(head)) = head else { continue };
            let head = String::from_utf8_lossy(&head);
            let line: Vec<_> = head
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let host: Vec<_> = head
                .lines()
                .skip(1)
                .filter_map(|line| line.split_once(':'))
                .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
                .map(|(_, value)| value.trim())
                .collect();
            let valid = head.ends_with("\r\n\r\n")
                && line.len() == 3
                && line[0] == "GET"
                && matches!(line[2], "HTTP/1.0" | "HTTP/1.1")
                && host.len() == 1
                && hosts.iter().any(|expected| expected == host[0])
                && line[1].starts_with('/')
                && !line[1].starts_with("//");
            let (status, body, result) = if valid {
                self.callback(line[1])
            } else {
                ("400 Bad Request", "Invalid callback.", None)
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = tokio::time::timeout(
                CALLBACK_WRITE_TIMEOUT,
                stream.write_all(response.as_bytes()),
            )
            .await;
            if let Some(result) = result {
                return result;
            }
        }
    }

    /// The authorization code from the callback or a pasted redirect, whichever is first.
    pub(super) async fn wait(
        &self,
        listener: Option<Callback>,
        pasted: Option<oneshot::Receiver<String>>,
    ) -> Result<String, String> {
        let callback = async {
            match listener {
                Some(listener) => listener.wait(self).await,
                None => std::future::pending().await,
            }
        };
        let pasted = async {
            match pasted {
                Some(receiver) => match receiver.await {
                    Ok(input) => self.pasted(&input),
                    Err(_) => std::future::pending().await,
                },
                None => std::future::pending().await,
            }
        };
        tokio::time::timeout(LOGIN_TIMEOUT, async {
            tokio::select! {
                result = callback => result,
                result = pasted => result,
            }
        })
        .await
        .map_err(|_| "Codex sign-in timed out; run `artifactize login codex` again.".to_owned())?
    }
}

/// What a sign-in asks the person to do: open `url`, and whether the browser can return to
/// this machine's callback (when the port is taken, only a pasted redirect URL finishes it).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub url: String,
    pub callback: bool,
}

/// Sign in interactively and save artifactize's own tokens. The sign-in URL goes to
/// stderr, also with --json.
pub async fn login(state: Option<&Path>, repo: Option<&Path>) -> Result<(), String> {
    // Refuse before reading the terminal: where the tokens go and the sign-in root.
    Storage::new(state, repo, Tokens::Codex)?;
    auth_root()?;
    let terminal = crate::platform::stdin_is_terminal();
    let pasted = if terminal {
        let (send, receive) = oneshot::channel();
        // Detached: a sign-in finished in the browser leaves this read unanswered.
        std::thread::Builder::new()
            .name("codex-login-input".into())
            .spawn(move || {
                for line in std::io::stdin().lines() {
                    match line {
                        Ok(line) if line.trim().is_empty() => continue,
                        Ok(line) => {
                            let _ = send.send(line);
                        }
                        Err(_) => {}
                    }
                    break;
                }
            })
            .map_err(|error| format!("Cannot start Codex sign-in input: {error}"))?;
        Some(receive)
    } else {
        None
    };
    login_with(state, repo, pasted, true, |prompt| {
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "Open this URL to sign in with Codex:\n{}", prompt.url);
        if !prompt.callback {
            let _ = writeln!(
                err,
                "Port {CALLBACK_PORT} is in use (another sign-in, such as the Codex CLI or app, may hold it), so the browser cannot return here."
            );
        }
        if terminal {
            let _ = writeln!(
                err,
                "When the browser cannot return here, paste the full URL it was sent to (starting http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}?code=) and press Enter. Find it in the browser's address bar after the failed load, or in DevTools → Network if the page keeps loading."
            );
        }
    })
    .await
}

/// The sign-in behind `login codex` for any interface: `show` receives the prompt once the
/// callback listener is up, `pasted` may deliver the redirect URL the browser was sent to,
/// and `browser` hands the URL to the desktop's opener. It ends when the tokens are saved, the
/// sign-in fails, or after five minutes; dropping the future stops it and frees the port.
pub async fn login_with(
    state: Option<&Path>,
    repo: Option<&Path>,
    pasted: Option<oneshot::Receiver<String>>,
    browser: bool,
    show: impl FnOnce(&Prompt),
) -> Result<(), String> {
    // Check where the tokens will go before anyone signs in.
    let storage = Storage::new(state, repo, Tokens::Codex)?;
    let root = auth_root()?;
    let listener = Callback::bind(CALLBACK_PORT)
        .map_err(|error| format!("Cannot bind Codex sign-in callback: {error}"))?;
    let redirect_uri = format!("http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}");
    sign_in(
        &storage,
        &root,
        listener,
        &redirect_uri,
        pasted,
        browser,
        |url, callback| {
            show(&Prompt {
                url: url.to_string(),
                callback,
            })
        },
    )
    .await
}

pub(super) async fn sign_in(
    storage: &Storage,
    root: &Url,
    listener: Option<Callback>,
    redirect_uri: &str,
    pasted: Option<oneshot::Receiver<String>>,
    browser: bool,
    show: impl FnOnce(&Url, bool),
) -> Result<(), String> {
    let open = browser.then_some(crate::platform::program::open_desktop);
    sign_in_opening(storage, root, listener, redirect_uri, pasted, open, show).await
}

/// [`sign_in`] that hands the sign-in URL to `open`, when given, as the browser's opener.
pub(super) async fn sign_in_opening(
    storage: &Storage,
    root: &Url,
    listener: Option<Callback>,
    redirect_uri: &str,
    pasted: Option<oneshot::Receiver<String>>,
    open: Option<impl AsyncFnOnce(&OsStr) -> io::Result<()>>,
    show: impl FnOnce(&Url, bool),
) -> Result<(), String> {
    if listener.is_none() && pasted.is_none() {
        return Err(format!(
            "Port {CALLBACK_PORT} is in use and there is no terminal to paste the redirect URL into; finish the other sign-in and try again."
        ));
    }
    let pending = Pending::new()?;
    let url = pending.authorize_url(root, redirect_uri)?;
    show(&url, listener.is_some());
    let opened = async {
        if let Some(open) = open {
            open_browser(&url, open).await;
        }
    };
    // The callback answers while the opener hands the URL off: the browser may come back
    // before the opener returns, as when the opener waits for what it opened.
    let (code, ()) = tokio::join!(pending.wait(listener, pasted), opened);
    let code = code?;
    let tokens = token_request(
        root,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", &code),
            ("code_verifier", &pending.verifier),
            ("redirect_uri", redirect_uri),
        ],
        None,
    )
    .await
    .map_err(|error| error.message)?;
    let credentials = credentials(tokens, now()?)?;
    let _lock = storage.lock(LOCK).await?;
    storage.save(CREDENTIALS, &credentials)
}

pub(super) async fn open_browser(url: &Url, open: impl AsyncFnOnce(&OsStr) -> io::Result<()>) {
    // The printed authorization URL remains available if desktop handoff fails.
    let _ = open(OsStr::new(url.as_str())).await;
}
