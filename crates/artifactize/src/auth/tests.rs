use super::*;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener as StdListener,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn test_storage(root: &Path) -> Storage {
    use std::os::unix::fs::DirBuilderExt;
    let directory = root.join("auth");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    Storage { directory }
}

fn identity() -> Identity {
    Identity {
        issuer: oauth::ISSUER.into(),
        subject: "test-subject".into(),
        email: Some("test@example.invalid".into()),
    }
}

fn registered() -> Registration {
    Registration {
        ext_agent_host_id: "urn:uuid:8f7c2c32-4bdf-4eaa-854f-2817ce26540e".into(),
        client_id: Some("oaiapp_test".into()),
        identity: Some(identity()),
    }
}

fn claims() -> Value {
    json!({ "iss": oauth::ISSUER, "sub": "test-subject", "aud": "oaiapp_test", "exp": now().unwrap() + 3600, "iat": now().unwrap(), "nonce": "test-nonce", "email": "test@example.invalid" })
}

fn sign(claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key".into());
    // Disposable fixture key, never used outside offline tests.
    encode(
        &header,
        claims,
        &EncodingKey::from_rsa_der(include_bytes!("fixtures/signing-key.der")),
    )
    .unwrap()
}

fn keys() -> JwkSet {
    serde_json::from_str(include_str!("fixtures/jwks.json")).unwrap()
}

fn expired_credentials() -> Credentials {
    Credentials {
        access_token: "old-access-secret".into(),
        refresh_token: "old-refresh-secret".into(),
        id_token: sign(&claims()),
        token_type: "Bearer".into(),
        expires_at: now().unwrap() - 1,
        saved_at: now().unwrap() - 3601,
        client_id: "oaiapp_test".into(),
        ext_agent_host_id: registered().ext_agent_host_id,
        scopes: oauth::SCOPES
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
        identity: identity(),
    }
}

#[test]
fn pkce_matches_rfc7636_and_values_are_fresh() {
    assert_eq!(
        oauth::challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let first = oauth::random_value().unwrap();
    assert_eq!(first.len(), 43);
    assert_ne!(first, oauth::random_value().unwrap());
    assert!(
        first
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    let host = oauth::host_id().unwrap();
    assert_eq!(host.len(), 45);
    assert_eq!(&host[23..24], "4");
    assert!(matches!(&host[28..29], "8" | "9" | "a" | "b"));
}

#[tokio::test]
async fn authorize_registration_reauth_and_callback_checks() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut registration = registered();
    registration.client_id = None;
    registration.identity = None;
    let mut pending = PendingLogin::new(&listener, &registration).unwrap();
    let url = pending.authorize_url(
        &Url::parse("https://auth.openai.com/api/accounts/authorize").unwrap(),
        &registration,
    );
    let params: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(params["client_id"], "dynamic_agent_client");
    assert_eq!(params["agent_name_hint"], "artifactize");
    assert_eq!(params["resource"], RESOURCE);
    assert_eq!(params["scope"], oauth::SCOPES);
    assert_eq!(params["redirect_uri"], pending.redirect_uri);
    assert_eq!(
        params["code_challenge"],
        oauth::challenge(&pending.verifier)
    );
    assert_eq!(params["ext_agent_host_id"], registration.ext_agent_host_id);
    assert_ne!(pending.state, pending.nonce);
    let callback = format!(
        "/auth/callback?state={}&code=abc&client_id=oaiapp_new",
        pending.state
    );
    assert_eq!(
        pending.callback(&callback).unwrap(),
        ("abc".into(), "oaiapp_new".into())
    );
    assert!(
        pending
            .callback(&callback)
            .unwrap_err()
            .contains("already consumed")
    );

    for (query, expected) in [
        (
            "state=wrong&code=abc&client_id=oaiapp_new",
            "state mismatch",
        ),
        ("state={state}&error=access_denied", "declined"),
        ("state=wrong&error=access_denied", "state mismatch"),
        ("state={state}&code=abc", "issued client ID"),
        (
            "state={state}&code=abc&client_id=dynamic_agent_client",
            "issued client ID",
        ),
        (
            "state={state}&state={state}&code=abc&client_id=oaiapp_new",
            "Duplicate",
        ),
        (
            "state={state}&code=abc&code=def&client_id=oaiapp_new",
            "Duplicate",
        ),
    ] {
        let mut pending = PendingLogin::new(&listener, &registration).unwrap();
        let target = format!(
            "/auth/callback?{}",
            query.replace("{state}", &pending.state)
        );
        assert!(
            pending.callback(&target).unwrap_err().contains(expected),
            "{query}"
        );
    }
    let registration = registered();
    let mut pending = PendingLogin::new(&listener, &registration).unwrap();
    let url = pending.authorize_url(
        &Url::parse("https://auth.openai.com/api/accounts/authorize").unwrap(),
        &registration,
    );
    assert!(
        !url.query_pairs()
            .any(|(key, _)| key == "id_token_hint" || key == "agent_name_hint")
    );
    assert_eq!(
        url.query_pairs()
            .find(|(key, _)| key == "client_id")
            .unwrap()
            .1,
        "oaiapp_test"
    );
    let target = format!("/auth/callback?state={}&code=abc", pending.state);
    assert_eq!(pending.callback(&target).unwrap().1, "oaiapp_test");
    let mut pending = PendingLogin::new(&listener, &registration).unwrap();
    let target = format!(
        "/auth/callback?state={}&code=abc&client_id=oaiapp_other",
        pending.state
    );
    assert!(
        pending
            .callback(&target)
            .unwrap_err()
            .contains("client ID mismatch")
    );
}

#[tokio::test]
async fn loopback_is_one_shot_and_rejects_wrong_host_and_path() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut pending = PendingLogin::new(&listener, &registered()).unwrap();
    let target = format!("/auth/callback?state={}&code=abc", pending.state);
    let receiver = tokio::spawn(async move { pending.receive(listener).await });
    for (path, host, expected) in [
        ("/favicon.ico", address.to_string(), "404"),
        (target.as_str(), "attacker.invalid".into(), "404"),
        (target.as_str(), address.to_string(), "200"),
    ] {
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.contains(expected));
        assert!(!response.contains("abc"));
    }
    assert_eq!(
        receiver.await.unwrap().unwrap(),
        ("abc".into(), "oaiapp_test".into())
    );
    // Concurrent process tests can inherit the CLOEXEC listener between fork and exec.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if tokio::net::TcpStream::connect(address).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("one-shot listener closes after fork/exec children release it");
}

#[test]
fn id_token_signature_and_claims_are_validated() {
    let good = claims();
    let validated =
        oauth::validate_id_token(&sign(&good), &keys(), "oaiapp_test", Some("test-nonce")).unwrap();
    assert!(validated.matches(&identity()));
    for (field, value) in [
        ("aud", json!("oaiapp_wrong")),
        ("nonce", json!("wrong")),
        ("exp", json!(now().unwrap() - 1)),
        ("iss", json!("https://wrong.invalid")),
        ("iat", json!(now().unwrap() + 3600)),
        ("nbf", json!(now().unwrap() + 3600)),
        ("azp", json!("other")),
    ] {
        let mut changed = good.clone();
        changed[field] = value;
        assert!(
            oauth::validate_id_token(&sign(&changed), &keys(), "oaiapp_test", Some("test-nonce"))
                .is_err(),
            "{field}"
        );
    }
    let token = sign(&good);
    let mut parts: Vec<_> = token.split('.').map(str::to_owned).collect();
    use base64::Engine;
    parts[1] = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&json!({"sub":"attacker"})).unwrap());
    assert!(
        oauth::validate_id_token(&parts.join("."), &keys(), "oaiapp_test", Some("test-nonce"))
            .is_err()
    );
    for field in ["aud", "iss", "exp", "nonce", "sub", "iat"] {
        let mut changed = good.clone();
        changed.as_object_mut().unwrap().remove(field);
        assert!(
            oauth::validate_id_token(&sign(&changed), &keys(), "oaiapp_test", Some("test-nonce"))
                .is_err(),
            "{field}"
        );
    }
}

#[tokio::test]
async fn storage_is_atomic_private_and_preserves_registration() {
    let temp = tempfile::tempdir().unwrap();
    let storage = test_storage(temp.path());
    let _lock = storage.lock().await.unwrap();
    let first = registration(&storage).unwrap();
    assert_eq!(
        first.ext_agent_host_id,
        registration(&storage).unwrap().ext_agent_host_id
    );
    storage.save(CREDENTIALS, &expired_credentials()).unwrap();
    let path = storage.directory.join(CREDENTIALS);
    let old = fs::File::open(&path).unwrap();
    let old_inode = old.metadata().unwrap().ino();
    let mut updated = expired_credentials();
    updated.access_token = "new-access".into();
    storage.save(CREDENTIALS, &updated).unwrap();
    assert_ne!(old_inode, path.metadata().unwrap().ino());
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(
        storage.directory.metadata().unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        storage
            .read::<Credentials>(CREDENTIALS)
            .unwrap()
            .unwrap()
            .access_token,
        "new-access"
    );
    assert_eq!(
        serde_json::from_reader::<_, Credentials>(old)
            .unwrap()
            .access_token,
        "old-access-secret"
    );
    assert_eq!(fs::read_dir(&storage.directory).unwrap().count(), 3);
    storage.remove_credentials().unwrap();
    assert!(storage.read::<Credentials>(CREDENTIALS).unwrap().is_none());
    assert_eq!(
        first.ext_agent_host_id,
        registration(&storage).unwrap().ext_agent_host_id
    );
    let target = temp.path().join("public.json");
    fs::write(&target, "{}").unwrap();
    symlink(&target, &path).unwrap();
    assert!(storage.read::<Credentials>(CREDENTIALS).is_err());
}

#[test]
fn credential_storage_rejects_repositories_and_symlink_escapes() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join(".git"), "worktree marker").unwrap();
    assert!(Storage::new(Some(&repo.join("state")), None).is_err());
    let alias = temp.path().join("alias");
    symlink(&repo, &alias).unwrap();
    assert!(Storage::new(Some(&alias.join("state")), None).is_err());
    fs::remove_file(repo.join(".git")).unwrap();
    assert!(Storage::new(Some(&repo.join("state")), Some(&repo)).is_err());
    assert!(!repo.join("state").exists());
}

struct MockServer {
    endpoint: Url,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl MockServer {
    fn new(status: u16, response: Value) -> Self {
        Self::expect_form(
            status,
            response,
            BTreeMap::from([
                ("grant_type".into(), "refresh_token".into()),
                ("client_id".into(), "oaiapp_test".into()),
                ("refresh_token".into(), "old-refresh-secret".into()),
                ("resource".into(), RESOURCE.into()),
            ]),
        )
    }

    fn expect_form(status: u16, response: Value, expected: BTreeMap<String, String>) -> Self {
        let listener = StdListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint =
            Url::parse(&format!("http://{}/token", listener.local_addr().unwrap())).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let count = requests.clone();
        let done = stop.clone();
        let thread = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut data = Vec::new();
                let mut byte = [0];
                while !data.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    data.push(byte[0]);
                }
                let headers = String::from_utf8(data).unwrap();
                assert!(headers.starts_with("POST /token HTTP/1.1"));
                assert!(
                    headers
                        .to_ascii_lowercase()
                        .contains("content-type: application/x-www-form-urlencoded")
                );
                let length: usize = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                    .unwrap()
                    .1
                    .trim()
                    .parse()
                    .unwrap();
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                let url = Url::parse(&format!(
                    "http://local/?{}",
                    String::from_utf8(body).unwrap()
                ))
                .unwrap();
                let form: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
                assert_eq!(form, expected);
                count.fetch_add(1, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(75));
                let body = response.to_string();
                write!(stream, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn discovery(&self) -> Discovery {
        Discovery {
            issuer: oauth::ISSUER.into(),
            authorization_endpoint: self.endpoint.clone(),
            token_endpoint: self.endpoint.clone(),
            jwks_uri: self.endpoint.clone(),
            revocation_endpoint: None,
        }
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn seed(storage: &Storage) {
    storage.save(REGISTRATION, &registered()).unwrap();
    storage.save(CREDENTIALS, &expired_credentials()).unwrap();
}

fn refreshed_response() -> Value {
    json!({ "access_token":"new-access", "refresh_token":"new-refresh", "token_type":"Bearer", "expires_in":3600 })
}

#[tokio::test]
async fn code_exchange_sends_issued_client_verifier_and_identical_redirect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let pending = PendingLogin::new(&listener, &registered()).unwrap();
    let code = "code+with&reserved=characters";
    let server = MockServer::expect_form(
        200,
        refreshed_response(),
        BTreeMap::from([
            ("grant_type".into(), "authorization_code".into()),
            ("client_id".into(), "oaiapp_test".into()),
            ("code".into(), code.into()),
            ("code_verifier".into(), pending.verifier.clone()),
            ("redirect_uri".into(), pending.redirect_uri.clone()),
            ("resource".into(), RESOURCE.into()),
        ]),
    );
    let tokens = exchange_code(
        &oauth::client().unwrap(),
        &server.discovery(),
        &pending,
        code,
        "oaiapp_test",
    )
    .await
    .unwrap();
    assert_eq!(tokens.access_token, "new-access");
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_refreshes_make_exactly_one_request_and_reuse_rotated_tokens() {
    let temp = tempfile::tempdir().unwrap();
    let storage = test_storage(temp.path());
    seed(&storage);
    let server = MockServer::new(200, refreshed_response());
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let directory = storage.directory.clone();
            let discovery = server.discovery();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async {
                        refresh(
                            &Storage { directory },
                            &oauth::client().unwrap(),
                            &discovery,
                        )
                        .await
                        .unwrap()
                    })
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap(), "new-access");
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    let saved = load_credentials(&storage).unwrap();
    assert_eq!(saved.refresh_token, "new-refresh");
    assert_eq!(saved.scopes, expired_credentials().scopes);
    assert!(saved.expires_at > now().unwrap() + 3500);
}

#[tokio::test]
async fn terminal_refresh_errors_clear_tokens_but_transient_errors_keep_them() {
    let temp = tempfile::tempdir().unwrap();
    let storage = test_storage(temp.path());
    for (status, code, cleared) in [
        (503, "unavailable", false),
        (400, "invalid_grant", true),
        (400, "refresh_token_reused", true),
    ] {
        seed(&storage);
        let server = MockServer::new(
            status,
            json!({"error": code, "error_description":"old-refresh-secret"}),
        );
        let error = refresh(&storage, &oauth::client().unwrap(), &server.discovery())
            .await
            .unwrap_err();
        assert!(!error.contains("old-refresh-secret"));
        assert_eq!(
            storage.read::<Credentials>(CREDENTIALS).unwrap().is_none(),
            cleared
        );
        if cleared {
            assert!(error.contains("artifactize login chatgpt"));
        }
        assert_eq!(
            registration(&storage).unwrap().client_id.as_deref(),
            Some("oaiapp_test")
        );
    }
}

#[test]
fn credentials_require_plan_scope_and_keep_new_refresh_response_together() {
    let tokens = |scope: &str| TokenResponse {
        access_token: "access".into(),
        refresh_token: "refresh".into(),
        id_token: Some("validated-id".into()),
        token_type: "Bearer".into(),
        expires_in: 3600,
        scope: Some(scope.into()),
    };
    assert!(
        credentials(
            tokens("openid profile"),
            &registered(),
            identity(),
            now().unwrap(),
            None
        )
        .is_err()
    );
    let saved = credentials(
        tokens(oauth::SCOPES),
        &registered(),
        identity(),
        now().unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(saved.client_id, "oaiapp_test");
    assert_eq!(saved.refresh_token, "refresh");
    assert_eq!(saved.expires_at - saved.saved_at, 3600);
}

#[test]
fn cli_accepts_only_chatgpt_auth() {
    use clap::Parser;
    for command in ["login", "logout"] {
        assert!(crate::cli::Cli::try_parse_from(["artifactize", command, "chatgpt"]).is_ok());
        assert!(crate::cli::Cli::try_parse_from(["artifactize", command, "other"]).is_err());
    }
}
