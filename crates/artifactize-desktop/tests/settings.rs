//! Settings at the command boundary, against the CLI tests' loopback fake for the Codex
//! sign-in, the model listings and a remote review store: API-key presence (never values),
//! `login`/`logout codex` with a pasted redirect and a cancel, `models`, and `remote
//! login|status|logout`. One test, so this process's environment is its own.
use artifactize_desktop::{Launch, Mode};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{
    WebviewWindow,
    ipc::{CallbackFn, InvokeBody},
    test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets},
    webview::InvokeRequest,
};

/// The CLI tests' fake provider and operating-system helpers.
#[path = "../../artifactize/tests/support/mod.rs"]
mod support;

use support::{FakeProvider, Reply, codex, openai};

const OPENAI_KEY: &str = "sk-desktop-settings-secret";
const REMOTE_TOKEN: &str = "azt_desktopsettingstoken0123456789";
/// Answered with a client error that echoes it back.
const ECHO_TOKEN: &str = "azt_echoedbythestore0123456789abcd";
/// Every whoami echoes its Authorization header in a client error.
static ECHO: AtomicBool = AtomicBool::new(false);

fn invoke(window: &WebviewWindow<MockRuntime>, command: &str, args: Value) -> Result<Value, Value> {
    tauri::test::get_ipc_response(
        window,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
    )
    .map(|body| body.deserialize::<Value>().unwrap())
}

/// The `state=` of a Codex authorize URL.
fn oauth_state(url: &str) -> String {
    url.split(['?', '&'])
        .find_map(|part| part.strip_prefix("state="))
        .unwrap()
        .to_owned()
}

#[test]
fn settings_use_the_clis_sign_ins_keys_listings_and_remote_store() {
    let provider = FakeProvider::start(|request| {
        let bearer = |token: &str| {
            request
                .headers
                .get("authorization")
                .is_some_and(|value| value == &format!("Bearer {token}"))
        };
        match (request.method.as_str(), request.path.as_str()) {
            ("POST", "/oauth/token") => {
                codex::tokens(&codex::jwt("account-1", codex::VALID_EXPIRY), "refresh-1")
            }
            ("POST", "/oauth/revoke") => Reply::Json(200, json!({})),
            // A store or proxy that echoes the Authorization header in a client error.
            ("GET", "/v1/whoami") if ECHO.load(Ordering::SeqCst) || bearer(ECHO_TOKEN) => {
                Reply::Json(
                    400,
                    json!({"error": format!(
                        "Bad request; Authorization: {}",
                        request.headers.get("authorization").cloned().unwrap_or_default()
                    )}),
                )
            }
            ("GET", "/v1/whoami") if bearer(REMOTE_TOKEN) => Reply::Json(
                200,
                json!({"principal":"alice-laptop","scopes":["read","publish"]}),
            ),
            ("GET", "/v1/whoami") => Reply::Json(401, json!({"error":"Invalid or revoked token."})),
            ("GET", "/v1/models") => openai::models(&["gpt-fake"]),
            ("GET", path) if path.starts_with("/backend-api/codex/models") => Reply::Json(
                200,
                json!({"models":[{"slug":"gpt-6-luna","display_name":"GPT-6 Luna","visibility":"list"}]}),
            ),
            _ => Reply::Json(404, json!({"error":"not found"})),
        }
    });
    // SAFETY: the only test in this binary, before the app starts any thread that reads them.
    unsafe {
        std::env::set_var("OPENAI_API_KEY", OPENAI_KEY);
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::set_var("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base());
        std::env::set_var("ARTIFACTIZE_CODEX_BASE_URL", provider.codex_base());
        std::env::set_var("ARTIFACTIZE_CODEX_AUTH_URL", &provider.url);
        for variable in [
            "ARTIFACTIZE_CODEX_AUTH_FILE",
            "ARTIFACTIZE_REMOTE",
            "ARTIFACTIZE_REMOTE_TOKEN",
            "ARTIFACTIZE_REMOTE_SHARE",
        ] {
            std::env::remove_var(variable);
        }
    }
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let launch = Launch {
        mode: Mode::Monitor,
        state: root.path().join("state"),
        repo: Some(repo),
        reviewer: "alice".parse().unwrap(),
        request: None,
        search: artifactize::review::search::DEFAULT.into(),
    };
    let app = artifactize_desktop::app(mock_builder(), launch)
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let call = |command: &str, args: Value| invoke(&window, command, args);
    let secret_free = |value: &Value| {
        let text = value.to_string();
        for secret in [OPENAI_KEY, REMOTE_TOKEN, ECHO_TOKEN] {
            // Not even a prefix of it.
            assert!(!text.contains(&secret[..12]), "{text}");
        }
    };

    // API keys: whether each is in the environment, never its value.
    let keys = call("settings_keys", json!({})).unwrap();
    secret_free(&keys);
    assert_eq!(
        keys,
        json!([
            {"provider": "openai", "variable": "OPENAI_API_KEY", "present": true},
            {"provider": "anthropic", "variable": "ANTHROPIC_API_KEY", "present": false},
        ])
    );

    // Models, with the CLI's error for a missing key.
    let models = call("models_list", json!({"provider": "openai"})).unwrap();
    assert_eq!(
        models,
        json!([{"slug": "gpt-fake", "displayName": "gpt-fake"}])
    );
    let missing = call("models_list", json!({"provider": "anthropic"})).unwrap_err();
    assert_eq!(
        missing,
        json!({"kind": "account", "message": "ANTHROPIC_API_KEY is required to list models."})
    );
    let signed_out = call("models_list", json!({"provider": "codex"})).unwrap_err();
    assert!(
        signed_out["message"]
            .as_str()
            .unwrap()
            .contains("artifactize login codex"),
        "{signed_out}"
    );

    // Codex, in the frontend's order: start, then wait, and while the wait is pending a paste
    // of the redirect URL (as the CLI accepts it on stdin) or a cancel must still reach the
    // sign-in.
    assert_eq!(
        call("codex_status", json!({})).unwrap(),
        json!({"state": "absent"})
    );
    let exchanges = || {
        provider
            .requests()
            .iter()
            .filter(|request| request.path == "/oauth/token")
            .count()
    };
    let prompt = call("codex_login_start", json!({"browser": false})).unwrap();
    let url = prompt["url"].as_str().unwrap().to_owned();
    assert!(
        url.starts_with(&format!("{}/oauth/authorize?", provider.url)),
        "{url}"
    );
    let redirect = format!(
        "http://localhost:1455/auth/callback?code=fake-code&state={}",
        oauth_state(&url)
    );
    let status = std::thread::scope(|scope| {
        let waiting = scope.spawn(|| call("codex_login_wait", json!({})));
        std::thread::sleep(std::time::Duration::from_millis(200));
        call("codex_login_paste", json!({"redirect": redirect})).unwrap();
        waiting.join().unwrap()
    })
    .unwrap();
    assert_eq!(status["state"], "stored");
    assert_eq!(status["expired"], json!(false));
    secret_free(&status);
    assert!(!status.to_string().contains("refresh-1"));
    let exchange = provider
        .requests()
        .into_iter()
        .find(|request| request.path == "/oauth/token")
        .unwrap();
    assert!(exchange.body.as_str().unwrap().contains("code=fake-code"));
    let models = call("models_list", json!({"provider": "codex"})).unwrap();
    assert_eq!(
        models,
        json!([{"slug": "gpt-6-luna", "displayName": "GPT-6 Luna"}])
    );
    // A finished sign-in takes no more pastes.
    let late = call("codex_login_paste", json!({"redirect": redirect}));
    assert_eq!(late.unwrap_err()["kind"], "missing");

    // Signing in again while signed in is the CLI's re-login; cancelling it keeps the old one.
    let cancel_while_waiting = |then: &dyn Fn(&str)| {
        let prompt = call("codex_login_start", json!({"browser": false})).unwrap();
        let url = prompt["url"].as_str().unwrap().to_owned();
        let cancelled = std::thread::scope(|scope| {
            let waiting = scope.spawn(|| call("codex_login_wait", json!({})));
            std::thread::sleep(std::time::Duration::from_millis(200));
            call("codex_login_cancel", json!({})).unwrap();
            waiting.join().unwrap()
        });
        assert_eq!(
            cancelled.unwrap_err(),
            json!({"kind": "account", "message": "Codex sign-in was cancelled."})
        );
        then(&url);
    };
    cancel_while_waiting(&|_| {});
    assert_eq!(call("codex_status", json!({})).unwrap()["state"], "stored");

    // Sign out: revoked at the fake, removed locally.
    let out = call("codex_logout", json!({})).unwrap();
    assert_eq!(out, json!({"revoked": true, "status": {"state": "absent"}}));
    assert!(
        provider
            .requests()
            .iter()
            .any(|request| request.path == "/oauth/revoke")
    );

    // A cancelled sign-in stores nothing, even when the browser returns afterwards: the
    // callback finds no listener and a paste no sign-in.
    let before = exchanges();
    cancel_while_waiting(&|url| {
        let callback = format!("/auth/callback?code=late-code&state={}", oauth_state(url));
        if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", 1455)) {
            use std::io::Write;
            let _ = write!(
                stream,
                "GET {callback} HTTP/1.1\r\nHost: localhost:1455\r\nConnection: close\r\n\r\n"
            );
        }
        let late = call(
            "codex_login_paste",
            json!({"redirect": format!("http://localhost:1455{callback}")}),
        );
        assert_eq!(late.unwrap_err()["kind"], "missing");
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(
        exchanges(),
        before,
        "no code was exchanged after the cancel"
    );
    assert_eq!(
        call("codex_status", json!({})).unwrap(),
        json!({"state": "absent"})
    );

    // Overlapping starts: a newer start stops the older sign-in, whose waiter then finishes
    // (cancelled) without clearing the newer one, which a paste still completes.
    let older = std::thread::scope(|scope| {
        call("codex_login_start", json!({"browser": false})).unwrap();
        let waiting = scope.spawn(|| call("codex_login_wait", json!({})));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let newer = call("codex_login_start", json!({"browser": false})).unwrap();
        let ended = waiting.join().unwrap();
        (newer, ended)
    });
    let (newer, ended) = older;
    assert_eq!(
        ended.unwrap_err(),
        json!({"kind": "account", "message": "Codex sign-in was cancelled."})
    );
    let redirect = format!(
        "http://localhost:1455/auth/callback?code=newer-code&state={}",
        oauth_state(newer["url"].as_str().unwrap())
    );
    let status = std::thread::scope(|scope| {
        let waiting = scope.spawn(|| call("codex_login_wait", json!({})));
        std::thread::sleep(std::time::Duration::from_millis(200));
        call("codex_login_paste", json!({"redirect": redirect})).unwrap();
        waiting.join().unwrap()
    })
    .unwrap();
    assert_eq!(status["state"], "stored");
    assert!(provider.requests().iter().any(|request| {
        request
            .body
            .as_str()
            .is_some_and(|body| body.contains("code=newer-code"))
    }));
    call("codex_logout", json!({})).unwrap();

    // Remote: not configured, a refused token saves nothing, then login, status, logout.
    let none = call("remote_status", json!({})).unwrap();
    assert_eq!(none["configured"], json!(false));
    let store = format!("{}/", provider.url);
    let refused = call(
        "remote_login",
        json!({"url": store, "share": "summary", "token": "azt_wrongtoken0123456789abcdef"}),
    )
    .unwrap_err();
    assert_eq!(refused["kind"], "account");
    assert_eq!(
        call("remote_status", json!({})).unwrap()["configured"],
        json!(false)
    );
    // Signing in with a token the store echoes back: refused, and never shown.
    let echoed = call(
        "remote_login",
        json!({"url": store, "share": "summary", "token": ECHO_TOKEN}),
    )
    .unwrap_err();
    secret_free(&echoed);
    assert_eq!(
        echoed["message"],
        "The remote store answered HTTP 400: [withheld: it contained part of the credential sent]"
    );
    let invalid = call(
        "remote_login",
        json!({"url": "http://reviews.example/", "share": "summary", "token": REMOTE_TOKEN}),
    )
    .unwrap_err();
    assert_eq!(invalid["kind"], "invalid");
    let status = call(
        "remote_login",
        json!({"url": store, "share": "full", "token": format!(" {REMOTE_TOKEN}\n")}),
    )
    .unwrap();
    secret_free(&status);
    assert_eq!(
        status,
        json!({
            "configured": true,
            "url": store,
            "share": "full",
            "tokenSource": "file",
            "reachable": true,
            "principal": "alice-laptop",
            "scopes": ["read", "publish"],
            "error": null,
        })
    );
    assert_eq!(call("remote_status", json!({})).unwrap(), status);
    // A store that echoes the saved token in a client error: status and push say so, without
    // the token.
    ECHO.store(true, Ordering::SeqCst);
    let echoed = call("remote_status", json!({})).unwrap();
    secret_free(&echoed);
    assert_eq!(echoed["reachable"], json!(true));
    assert_eq!(echoed["principal"], Value::Null);
    assert_eq!(
        echoed["error"],
        "The remote store answered HTTP 400: [withheld: it contained part of the credential sent]"
    );
    let failed = call("remote_push", json!({"dryRun": true})).unwrap_err();
    secret_free(&failed);
    assert!(
        failed["message"].as_str().unwrap().contains("[withheld"),
        "{failed}"
    );
    ECHO.store(false, Ordering::SeqCst);
    let pushed = call("remote_push", json!({"dryRun": true})).unwrap();
    assert_eq!(pushed["pushed"], json!(0));
    let out = call("remote_logout", json!({})).unwrap();
    assert_eq!(out["configured"], json!(false));
    std::mem::forget(app);
}
