use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    sync::Arc,
};

use rig_core::{http_client::StatusCode, test_utils::MockHttpResponse};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::llm::tests::Server;

/// An unsigned JWT with the ChatGPT account claim, as the token endpoint issues them.
pub(crate) fn jwt(account: &str, exp: u64) -> String {
    let encode = |value: Value| URL_SAFE_NO_PAD.encode(value.to_string());
    let mut claims = json!({"exp":exp});
    claims[CLAIM] = json!({"chatgpt_account_id":account});
    format!(
        "{}.{}.signature",
        encode(json!({"alg":"RS256","typ":"JWT"})),
        encode(claims)
    )
}

fn storage(root: &Path) -> Storage {
    let directory = root.join("auth");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    Storage { directory }
}

fn tokens(access: &str, refresh: &str) -> MockHttpResponse {
    MockHttpResponse::success(
        json!({"access_token":access,"refresh_token":refresh,"expires_in":3600,"id_token":"id"})
            .to_string(),
    )
}

fn stored(storage: &Storage, access: &str, expires_at: u64) {
    storage
        .save(
            CREDENTIALS,
            &Credentials {
                access_token: access.into(),
                refresh_token: "old-refresh".into(),
                account_id: "account-1".into(),
                expires_at,
                saved_at: 1,
            },
        )
        .unwrap();
}

/// The server's root, which the fake treats as `/v1`.
fn root(server: &Server) -> String {
    server.base.clone()
}

fn form(body: &Value) -> BTreeMap<String, String> {
    url::form_urlencoded::parse(body.as_str().unwrap().as_bytes())
        .into_owned()
        .collect()
}

#[test]
fn authorize_url_carries_pkce_and_the_codex_flow_parameters() {
    let pending = Pending::new().unwrap();
    assert_eq!(pending.verifier.len(), 43);
    assert_eq!(pending.state.len(), 32);
    assert!(pending.state.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(pending.state, Pending::new().unwrap().state);
    let url = pending
        .authorize_url("https://auth.openai.com", REDIRECT_URI)
        .unwrap();
    assert_eq!(url.path(), "/oauth/authorize");
    let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes()));
    let expected: BTreeMap<String, String> = [
        ("response_type", "code"),
        ("client_id", "app_EMoamEEZ73f0CkXaXp7hrann"),
        ("redirect_uri", "http://localhost:1455/auth/callback"),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("state", &pending.state),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", "artifactize"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    assert_eq!(query, expected);
}

#[test]
fn pasted_redirects_codes_and_states_are_checked() {
    let pending = Pending::new().unwrap();
    let state = pending.state.clone();
    for input in [
        format!("http://localhost:1455/auth/callback?code=abc&state={state}\n"),
        format!("abc#{state}"),
        format!("code=abc&state={state}"),
        "  abc  ".to_owned(),
    ] {
        assert_eq!(pending.pasted(&input).unwrap(), "abc", "{input}");
    }
    for input in [
        "http://localhost:1455/auth/callback?code=abc&state=other",
        "abc#other",
        "http://localhost:1455/auth/callback?state=x",
        "",
    ] {
        assert!(pending.pasted(input).is_err(), "{input}");
    }
}

async fn browser(port: u16, host: &str, target: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn login_exchanges_the_callback_code_and_saves_private_tokens() {
    let temp = tempfile::tempdir().unwrap();
    let storage = storage(temp.path());
    let access = jwt("account-1", now().unwrap() + 3600);
    let server = Server::new(vec![tokens(&access, "refresh-1")]).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let redirect = format!("http://localhost:{port}{CALLBACK_PATH}");
    let (send, receive) = oneshot::channel();
    let browse = async {
        let url: Url = receive.await.unwrap();
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        let host = format!("localhost:{port}");
        // Wrong state, host or path never end the sign-in.
        let wrong = browser(port, &host, "/auth/callback?code=x&state=wrong").await;
        assert!(wrong.starts_with("HTTP/1.1 400"), "{wrong}");
        let foreign = format!("/auth/callback?code=x&state={state}");
        assert!(
            browser(port, "evil.example", &foreign)
                .await
                .starts_with("HTTP/1.1 400")
        );
        assert!(
            browser(port, &host, "/other")
                .await
                .starts_with("HTTP/1.1 404")
        );
        let done = browser(
            port,
            &host,
            &format!("/auth/callback?code=the-code&state={state}"),
        )
        .await;
        assert!(done.starts_with("HTTP/1.1 200"), "{done}");
        url
    };
    let root = root(&server);
    let (result, url) = tokio::join!(
        sign_in(
            &storage,
            &root,
            Some(listener),
            &redirect,
            None,
            false,
            |url, callback| {
                assert!(callback);
                send.send(url.clone()).unwrap();
            }
        ),
        browse
    );
    result.unwrap();
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].line, "POST /v1/oauth/token HTTP/1.1");
    assert_eq!(
        requests[0].headers["content-type"],
        "application/x-www-form-urlencoded"
    );
    let verifier = form(&requests[0].body)["code_verifier"].clone();
    assert_eq!(
        url.query_pairs()
            .find(|(key, _)| key == "code_challenge")
            .unwrap()
            .1,
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
    );
    assert_eq!(
        form(&requests[0].body),
        BTreeMap::from([
            ("grant_type".into(), "authorization_code".into()),
            ("client_id".into(), CLIENT_ID.into()),
            ("code".into(), "the-code".into()),
            ("code_verifier".into(), verifier),
            ("redirect_uri".into(), redirect.clone()),
        ])
    );
    let path = storage.directory.join(CREDENTIALS);
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    let saved: Credentials = storage.read(CREDENTIALS).unwrap().unwrap();
    assert_eq!(saved.access_token, access);
    assert_eq!(saved.refresh_token, "refresh-1");
    assert_eq!(saved.account_id, "account-1");
    assert!(saved.expires_at >= saved.saved_at + 3600);
    // The ID token is not kept.
    assert!(!fs::read_to_string(path).unwrap().contains("\"id\""));
}

#[tokio::test]
async fn login_accepts_a_pasted_redirect_and_rejects_unusable_token_responses() {
    let temp = tempfile::tempdir().unwrap();
    let storage = storage(temp.path());
    let without_account = MockHttpResponse::success(
        json!({"access_token":"not-a-jwt","refresh_token":"r","expires_in":3600}).to_string(),
    );
    let incomplete =
        MockHttpResponse::success(json!({"access_token":"a","expires_in":3600}).to_string());
    let rejected = MockHttpResponse::error(
        StatusCode::BAD_REQUEST,
        json!({"error":"invalid_grant","error_description":"secret detail"}).to_string(),
    );
    let server = Server::new(vec![without_account, incomplete, rejected]).await;
    for expected in [
        "names no ChatGPT account",
        "Invalid Codex token response",
        "invalid_grant",
    ] {
        let (send, receive) = oneshot::channel();
        let error = sign_in(
            &storage,
            &root(&server),
            None,
            REDIRECT_URI,
            Some(receive),
            false,
            |url, callback| {
                assert!(!callback);
                let state = url.query_pairs().find(|(key, _)| key == "state").unwrap().1;
                send.send(format!("{REDIRECT_URI}?code=pasted&state={state}"))
                    .unwrap();
            },
        )
        .await
        .err()
        .unwrap();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("secret detail"), "{error}");
    }
    assert!(storage.read::<Credentials>(CREDENTIALS).unwrap().is_none());
    assert_eq!(form(&server.requests()[0].body)["code"], "pasted");
    let none = sign_in(
        &storage,
        &root(&server),
        None,
        REDIRECT_URI,
        None,
        false,
        |_, _| panic!("nothing to show without a way back"),
    )
    .await;
    assert!(none.unwrap_err().contains("Port 1455"));
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn expiring_tokens_refresh_once_under_the_lock_and_rotate() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Arc::new(storage(temp.path()));
    stored(&storage, "old-access", now().unwrap() + 60);
    let fresh = jwt("account-1", now().unwrap() + 3600);
    let server = Arc::new(Server::new(vec![tokens(&fresh, "new-refresh")]).await);
    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let storage = storage.clone();
            let server = server.clone();
            tokio::spawn(async move { stored_token(&storage, &root(&server)).await.unwrap() })
        })
        .collect();
    for task in tasks {
        let token = task.await.unwrap();
        assert_eq!(token.access_token, fresh);
        assert_eq!(token.account_id, "account-1");
    }
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        form(&requests[0].body),
        BTreeMap::from([
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), "old-refresh".into()),
            ("client_id".into(), CLIENT_ID.into()),
        ])
    );
    let saved: Credentials = storage.read(CREDENTIALS).unwrap().unwrap();
    assert_eq!(saved.refresh_token, "new-refresh");
    assert!(storage.directory.join("codex.lock").exists());
}

#[tokio::test]
async fn refresh_failures_keep_or_drop_the_sign_in() {
    let temp = tempfile::tempdir().unwrap();
    let storage = storage(temp.path());
    let other = jwt("account-2", now().unwrap() + 3600);
    let server = Server::new(vec![
        MockHttpResponse::error(StatusCode::INTERNAL_SERVER_ERROR, "{}"),
        tokens(&other, "other-refresh"),
        MockHttpResponse::error(
            StatusCode::BAD_REQUEST,
            json!({"error":{"code":"refresh_token_reused"}}).to_string(),
        ),
    ])
    .await;
    stored(&storage, "old-access", 1);
    // A failing token endpoint is transient, so a review may retry it.
    let transient = stored_token(&storage, &root(&server)).await.err().unwrap();
    assert!(transient.transient);
    let transient = transient.message;
    assert!(
        transient.contains("HTTP 500") && transient.contains("kept"),
        "{transient}"
    );
    let mismatch = stored_token(&storage, &root(&server)).await.err().unwrap();
    assert!(!mismatch.transient);
    let mismatch = mismatch.message;
    assert!(mismatch.contains("another ChatGPT account"), "{mismatch}");
    assert!(storage.read::<Credentials>(CREDENTIALS).unwrap().is_some());
    let terminal = stored_token(&storage, &root(&server)).await.err().unwrap();
    assert!(!terminal.transient);
    let terminal = terminal.message;
    assert!(terminal.contains("refresh_token_reused"), "{terminal}");
    assert!(terminal.contains("artifactize login codex"), "{terminal}");
    assert!(storage.read::<Credentials>(CREDENTIALS).unwrap().is_none());
    let missing = stored_token(&storage, &root(&server)).await.err().unwrap();
    assert_eq!(missing.message, LOGIN_REQUIRED);
}

#[test]
fn auth_files_are_read_only_and_expired_tokens_are_never_refreshed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("auth.json");
    let access = jwt("claim-account", now().unwrap() + 3600);
    let write = |value: Value| fs::write(&path, value.to_string()).unwrap();

    write(
        json!({"OPENAI_API_KEY":null,"tokens":{"id_token":"id","access_token":access,"refresh_token":"r","account_id":"file-account"},"last_refresh":"2026-10-01T00:00:00Z"}),
    );
    let before = (fs::read(&path).unwrap(), path.metadata().unwrap().mtime());
    let token = read_auth_file(&path).unwrap();
    assert_eq!(token.access_token, access);
    assert_eq!(token.account_id, "file-account");
    assert_eq!(
        (fs::read(&path).unwrap(), path.metadata().unwrap().mtime()),
        before
    );

    write(json!({"tokens":{"access_token":access}}));
    assert_eq!(read_auth_file(&path).unwrap().account_id, "claim-account");

    // The fake-provider docs' dummy file: an opaque token has no expiry to check.
    write(json!({"tokens":{"access_token":"dummy","account_id":"test"}}));
    assert_eq!(read_auth_file(&path).unwrap().account_id, "test");

    write(json!({"tokens":{"access_token":jwt("a", now().unwrap() + 30),"refresh_token":"r"}}));
    let expired = read_auth_file(&path).err().unwrap();
    assert!(expired.contains("has expired"), "{expired}");
    assert!(expired.contains("sign in with Codex again"), "{expired}");
    assert!(expired.contains("never refreshes"), "{expired}");

    for (contents, expected) in [
        (
            json!({"OPENAI_API_KEY":"sk-secret"}).to_string(),
            "no ChatGPT sign-in tokens",
        ),
        ("not json".to_owned(), "not a Codex auth file"),
        (
            json!({"tokens":{"access_token":"opaque"}}).to_string(),
            "names no ChatGPT account",
        ),
    ] {
        fs::write(&path, contents).unwrap();
        let error = read_auth_file(&path).err().unwrap();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("sk-secret"), "{error}");
    }
    let error = read_auth_file(&temp.path().join("missing.json"))
        .err()
        .unwrap();
    assert!(
        error.starts_with("ARTIFACTIZE_CODEX_AUTH_FILE: cannot read"),
        "{error}"
    );
}

#[tokio::test]
async fn logout_revokes_the_refresh_token_and_removes_only_own_tokens() {
    let temp = tempfile::tempdir().unwrap();
    let storage = storage(temp.path());
    let server = Server::new(vec![
        MockHttpResponse::success("{}".to_owned()),
        MockHttpResponse::error(StatusCode::BAD_REQUEST, "{}"),
    ])
    .await;
    stored(&storage, "access", now().unwrap() + 3600);
    assert!(sign_out(&storage, &root(&server)).await.unwrap());
    assert!(storage.read::<Credentials>(CREDENTIALS).unwrap().is_none());
    let request = &server.requests()[0];
    assert_eq!(request.line, "POST /v1/oauth/revoke HTTP/1.1");
    assert_eq!(
        request.body,
        json!({"token":"old-refresh","token_type_hint":"refresh_token","client_id":CLIENT_ID})
    );
    // Nothing stored: nothing to revoke.
    assert!(sign_out(&storage, &root(&server)).await.unwrap());
    assert_eq!(server.requests().len(), 1);
    stored(&storage, "access", now().unwrap() + 3600);
    assert!(!sign_out(&storage, &root(&server)).await.unwrap());
    assert!(storage.read::<Credentials>(CREDENTIALS).unwrap().is_none());
}
