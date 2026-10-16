//! Settings at the command boundary, against the CLI tests' loopback fake for the Codex
//! sign-in, the model listings and a remote review store: API-key presence (never values),
//! `login`/`logout codex` with a pasted redirect and a cancel, `models`, and `remote
//! login|status|logout`. A child probe owns its environment, separate from the shared fixture
//! lifecycle tests running in this binary.
use artifactize_desktop::{Launch, Mode};
use serde_json::{Value, json};
use tauri::{
    WebviewWindow,
    ipc::{CallbackFn, InvokeBody},
    test::{INVOKE_KEY, MockRuntime, mock_builder, mock_context, noop_assets},
    webview::InvokeRequest,
};

/// The CLI tests' fake provider and operating-system helpers.
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

use support::{FakeProvider, Reply, codex, openai};

const OPENAI_KEY: &str = "sk-desktop-settings-secret";
const REMOTE_TOKEN: &str = "azt_desktopsettingstoken0123456789";
/// Answered with a client error that echoes it back.
const ECHO_TOKEN: &str = "azt_echoedbythestore0123456789abcd";

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
            ("GET", "/v1/whoami") if bearer(ECHO_TOKEN) => Reply::Json(
                400,
                json!({"error": format!(
                    "Bad request; Authorization: {}",
                    request.headers.get("authorization").cloned().unwrap_or_default()
                )}),
            ),
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
    let mut command = support::os::test_command(
        "settings_command_probe",
        &[
            ("OPENAI_API_KEY", OPENAI_KEY.into()),
            ("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base().into()),
            ("ARTIFACTIZE_CODEX_BASE_URL", provider.codex_base().into()),
            ("ARTIFACTIZE_CODEX_AUTH_URL", provider.url.clone().into()),
        ],
    );
    command.arg("--ignored").env_remove("ANTHROPIC_API_KEY");
    for variable in [
        "ARTIFACTIZE_CODEX_AUTH_FILE",
        "ARTIFACTIZE_REMOTE",
        "ARTIFACTIZE_REMOTE_TOKEN",
        "ARTIFACTIZE_REMOTE_SHARE",
    ] {
        command.env_remove(variable);
    }
    let child = support::os::start(&mut command);
    let output = support::os::finish(child, "settings command probe");
    assert!(
        output.status.success(),
        "settings probe failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The joined child can exchange only the two codes explicitly pasted while active;
    // late pastes after cancellation never reach this provider.
    let requests = provider.requests();
    let exchanges: Vec<_> = requests
        .iter()
        .filter(|request| request.path == "/oauth/token")
        .map(|request| request.body.as_str().unwrap())
        .collect();
    assert_eq!(exchanges.len(), 2, "{exchanges:?}");
    assert!(exchanges[0].contains("code=fake-code"), "{exchanges:?}");
    assert!(exchanges[1].contains("code=newer-code"), "{exchanges:?}");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.path == "/oauth/revoke")
            .count(),
        2
    );
}

#[test]
#[ignore = "launched with its declared environment by the settings command test"]
fn settings_command_probe() {
    let provider_url = support::os::environment_variable("ARTIFACTIZE_CODEX_AUTH_URL")
        .unwrap()
        .into_string()
        .unwrap();
    let root = support::os::tempdir();
    let repo = root.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let state = root.path().join("state");
    let launch = Launch {
        mode: Mode::Monitor,
        state: state.clone(),
        repo: Some(repo),
        reviewer: "alice".parse().unwrap(),
        request: None,
        search: artifactize_core::review::search::DEFAULT.into(),
    };
    let app = artifactize_desktop::app(mock_builder(), launch)
        // Settings commands do not observe live hints. Keep this fixture free of an
        // app-lifetime subscription on Tauri's process-global test runtime.
        .setup(|_| Ok(()))
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
    let prompt = call("codex_login_start", json!({"browser": false})).unwrap();
    let url = prompt["url"].as_str().unwrap().to_owned();
    assert!(
        url.starts_with(&format!("{provider_url}/oauth/authorize?")),
        "{url}"
    );
    let redirect = format!(
        "http://localhost:1455/auth/callback?code=fake-code&state={}",
        oauth_state(&url)
    );
    // The paste reaches the sign-in whether or not its waiter has taken it yet.
    let wait = |prompt: &Value| json!({"signIn": prompt["signIn"]});
    let status = std::thread::scope(|scope| {
        let waiting = scope.spawn(|| call("codex_login_wait", wait(&prompt)));
        call("codex_login_paste", json!({"redirect": redirect})).unwrap();
        waiting.join().unwrap()
    })
    .unwrap();
    assert_eq!(status["state"], "stored");
    assert_eq!(status["expiry"]["state"], json!("usable"));
    secret_free(&status);
    assert!(!status.to_string().contains("refresh-1"));
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
        // The cancel reaches the sign-in whether or not its waiter has taken it yet.
        let cancelled = std::thread::scope(|scope| {
            let waiting = scope.spawn(|| call("codex_login_wait", wait(&prompt)));
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
    // The parent observes revocations after this probe exits.

    // A cancelled sign-in stores nothing and a late redirect paste finds no sign-in.
    // Do not probe the released callback port: another fixture or app could own it now.
    cancel_while_waiting(&|url| {
        let callback = format!("/auth/callback?code=late-code&state={}", oauth_state(url));
        let late = call(
            "codex_login_paste",
            json!({"redirect": format!("http://localhost:1455{callback}")}),
        );
        assert_eq!(late.unwrap_err()["kind"], "missing");
    });
    // The waiter returned once the sign-in's task was gone. The parent checks that no
    // cancelled code reached its provider after joining this entire probe.
    assert_eq!(
        call("codex_status", json!({})).unwrap(),
        json!({"state": "absent"})
    );

    // Overlapping starts, in order and without waiting on time: B's start stops A and
    // registers B; only then does A's waiter run, so A ends after B is registered. A reads as
    // cancelled, B is untouched, and nothing of A is exchanged or saved.
    let older = call("codex_login_start", json!({"browser": false})).unwrap();
    let newer = call("codex_login_start", json!({"browser": false})).unwrap();
    assert_ne!(older["signIn"], newer["signIn"]);
    assert_eq!(
        call("codex_login_wait", wait(&older)).unwrap_err(),
        json!({"kind": "account", "message": "Codex sign-in was cancelled."})
    );
    assert_eq!(
        call("codex_status", json!({})).unwrap(),
        json!({"state": "absent"})
    );
    let redirect = format!(
        "http://localhost:1455/auth/callback?code=newer-code&state={}",
        oauth_state(newer["url"].as_str().unwrap())
    );
    call("codex_login_paste", json!({"redirect": redirect})).unwrap();
    let status = call("codex_login_wait", wait(&newer)).unwrap();
    assert_eq!(status["state"], "stored");
    call("codex_logout", json!({})).unwrap();

    // Remote: not configured, a refused token saves nothing, then login, status, logout.
    let none = call("remote_status", json!({})).unwrap();
    assert_eq!(none["state"], json!("unconfigured"));
    let store = format!("{provider_url}/");
    let refused = call(
        "remote_login",
        json!({"url": store, "share": "summary", "token": "azt_wrongtoken0123456789abcdef"}),
    )
    .unwrap_err();
    assert_eq!(refused["kind"], "account");
    assert_eq!(
        call("remote_status", json!({})).unwrap()["state"],
        json!("unconfigured")
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
            "state": "configured",
            "url": store,
            "share": "full",
            "tokenSource": "file",
            "connection": {
                "state": "signedIn",
                "principal": "alice-laptop",
                "scopes": ["read", "publish"],
            },
        })
    );
    assert_eq!(call("remote_status", json!({})).unwrap(), status);
    // A store that echoes the saved token in a client error: status and push say so, without
    // the token.
    let credential = state.join("auth/remote-token.json");
    std::fs::remove_file(&credential).unwrap();
    support::os::write_private_file(
        &credential,
        json!({"url": store, "token": ECHO_TOKEN}).to_string(),
    );
    let echoed = call("remote_status", json!({})).unwrap();
    secret_free(&echoed);
    assert_eq!(echoed["connection"]["state"], json!("refused"));
    assert_eq!(
        echoed["connection"]["error"],
        "The remote store answered HTTP 400: [withheld: it contained part of the credential sent]"
    );
    let failed = call("remote_push", json!({"dryRun": true})).unwrap_err();
    secret_free(&failed);
    assert!(
        failed["message"].as_str().unwrap().contains("[withheld"),
        "{failed}"
    );
    std::fs::remove_file(&credential).unwrap();
    support::os::write_private_file(
        &credential,
        json!({"url": store, "token": REMOTE_TOKEN}).to_string(),
    );
    let pushed = call("remote_push", json!({"dryRun": true})).unwrap();
    assert_eq!(pushed["pushed"], json!(0));
    let out = call("remote_logout", json!({})).unwrap();
    assert_eq!(out["state"], json!("unconfigured"));
    window.destroy().unwrap();
    // The owned window and app drop before the fixture root and fake provider, also on panic.
}
