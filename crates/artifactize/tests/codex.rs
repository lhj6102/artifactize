//! The `codex` backend end to end against a fake Codex endpoint and sign-in server.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use support::{FakeProvider, Reply, Request, codex, openai};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

struct Project {
    root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Project {
    /// One `spec` Artifact with a built-in `read` tool, a content fingerprint (so its
    /// review has a reuse key) and a `codex` Agent eval.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("spec")).unwrap();
        fs::write(repo.join("spec/spec.md"), "R1: the spec covers R1.\n").unwrap();
        let profile = json!({"kind":"agent","backend":"codex","model":"gpt-6-luna","reasoning":"max","maxToolCalls":2});
        fs::write(
            repo.join("spec/artifactize.json"),
            json!({"name":"spec","views":{"agentTools":{"read":{"builtin":"read"}}},
                "fingerprint":{},
                "evals":[{"id":"review","title":"Review","profile":profile,
                    "payload":{"instruction":"Review {spec}."}}]})
            .to_string(),
        )
        .unwrap();
        Self {
            state: root.path().join("state"),
            repo,
            root,
        }
    }

    fn profile(&self, profile: Value) {
        let path = self.repo.join("spec/artifactize.json");
        let mut declaration: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        declaration["evals"][0]["profile"] = profile;
        fs::write(path, declaration.to_string()).unwrap();
    }

    fn credentials(&self) -> PathBuf {
        self.state.join("auth/codex.json")
    }

    /// artifactize's own sign-in, as `login codex` saves it.
    fn sign_in(&self, access: &str, expires_at: u64) {
        let auth = self.state.join("auth");
        support::os::create_private_dir_all(&auth);
        let credentials = json!({"access_token":access,"refresh_token":"stored-refresh",
            "account_id":"account-1","expires_at":expires_at,"saved_at":1});
        let _ = fs::remove_file(self.credentials());
        support::os::write_private_file(&self.credentials(), credentials.to_string());
    }

    /// A Codex CLI auth file outside the state directory.
    fn auth_file(&self, access: &str) -> PathBuf {
        let path = self.root.path().join("codex-auth.json");
        let auth = json!({"OPENAI_API_KEY":null,"tokens":{"id_token":"id","access_token":access,
            "refresh_token":"file-refresh","account_id":"file-account"},"last_refresh":"2026-10-01T00:00:00Z"});
        fs::write(&path, auth.to_string()).unwrap();
        path
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .arg("--json")
            .env("ARTIFACTIZE_REMOTE", "off")
            .env_remove("ARTIFACTIZE_CODEX_AUTH_FILE")
            .env_remove("ARTIFACTIZE_CODEX_BASE_URL")
            .env_remove("ARTIFACTIZE_CODEX_AUTH_URL");
        command
    }

    fn against(&self, provider: &FakeProvider, args: &[&str]) -> Command {
        let mut command = self.command(args);
        command
            .env("ARTIFACTIZE_CODEX_BASE_URL", provider.codex_base())
            .env("ARTIFACTIZE_CODEX_AUTH_URL", &provider.url);
        command
    }
}

fn parsed(output: Output, code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn replayed(request: &Request) -> bool {
    request.body["input"].as_array().is_some_and(|input| {
        input
            .iter()
            .any(|item| item["type"] == "function_call_output")
    })
}

/// Read `spec.md` once, then pass.
fn reviewer(request: &Request) -> Reply {
    match request.path.as_str() {
        "/backend-api/codex/responses" if replayed(request) => codex::completed(
            request,
            vec![openai::message(r#"{"verdict":"GREEN"}"#)],
            openai::usage(40, 8),
        ),
        "/backend-api/codex/responses" => codex::completed(
            request,
            vec![openai::function_call(
                "call_1",
                "read_spec",
                &json!({"path":"spec.md"}),
            )],
            openai::usage(30, 5),
        ),
        "/oauth/token" => codex::tokens(&codex::jwt("account-1", now() + 3600), "rotated-refresh"),
        path => Reply::Json(
            404,
            json!({"error":{"message":format!("unexpected {path}")}}),
        ),
    }
}

fn form(request: &Request) -> Vec<(String, String)> {
    url::form_urlencoded::parse(request.body.as_str().unwrap().as_bytes())
        .into_owned()
        .collect()
}

#[test]
fn a_review_calls_tools_with_codex_headers_and_exact_max_reasoning() {
    let project = Project::new();
    let access = codex::jwt("account-1", now() + 3600);
    project.sign_in(&access, now() + 3600);
    let provider = FakeProvider::start(reviewer);
    let run = parsed(
        project
            .against(&provider, &["verify", "--all"])
            .output()
            .unwrap(),
        0,
    );
    let review = &run["requests"][0];
    assert_eq!(review["status"], "GREEN", "{run}");
    assert_eq!(review["toolCalls"][0]["name"], "read_spec");
    assert_eq!(review["toolCalls"][0]["isError"], false);
    let attempts = review["usage"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[1]["usage"]["inputTokens"], 40);

    let calls = provider.requests();
    assert_eq!(calls.len(), 2, "no refresh while the token is fresh");
    for call in &calls {
        assert_eq!(
            (call.method.as_str(), call.path.as_str()),
            ("POST", "/backend-api/codex/responses")
        );
        let headers = &call.headers;
        assert_eq!(headers["authorization"], format!("Bearer {access}"));
        assert_eq!(headers["chatgpt-account-id"], "account-1");
        assert_eq!(headers["originator"], "artifactize");
        assert_eq!(headers["openai-beta"], "responses=experimental");
        assert_eq!(headers["accept"], "text/event-stream");
        assert!(headers["user-agent"].starts_with("artifactize/"));
        assert!(headers.contains_key("session_id"));
        let body = &call.body;
        assert_eq!(body["model"], "gpt-6-luna");
        assert_eq!(body["reasoning"], json!({"effort":"max","summary":"auto"}));
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["name"], "read_spec");
        let instructions = body["instructions"].as_str().unwrap();
        assert!(instructions.starts_with("Follow the artifactize review instructions."));
        assert!(!instructions.contains("You are ChatGPT"));
        for absent in ["temperature", "max_output_tokens", "text", "metadata"] {
            assert!(body.get(absent).is_none(), "{absent}: {body}");
        }
    }
    let output = calls[1].body["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "function_call_output")
        .unwrap();
    assert_eq!(output["call_id"], "call_1");
    assert!(output.to_string().contains("the spec covers R1"));
}

#[test]
fn an_expiring_sign_in_is_refreshed_and_rotated_before_the_review() {
    let project = Project::new();
    project.sign_in(&codex::jwt("account-1", now() + 60), now() + 60);
    let provider = FakeProvider::start(reviewer);
    let run = parsed(
        project
            .against(&provider, &["verify", "--all"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(run["requests"][0]["status"], "GREEN", "{run}");
    let calls = provider.requests();
    let paths: Vec<_> = calls.iter().map(|call| call.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/oauth/token",
            "/backend-api/codex/responses",
            "/backend-api/codex/responses"
        ]
    );
    assert_eq!(
        form(&calls[0]),
        [
            ("grant_type".to_owned(), "refresh_token".to_owned()),
            ("refresh_token".to_owned(), "stored-refresh".to_owned()),
            (
                "client_id".to_owned(),
                "app_EMoamEEZ73f0CkXaXp7hrann".to_owned()
            ),
        ]
    );
    let saved: Value = serde_json::from_slice(&fs::read(project.credentials()).unwrap()).unwrap();
    assert_eq!(saved["refresh_token"], "rotated-refresh");
    assert_eq!(
        calls[1].headers["authorization"],
        format!("Bearer {}", saved["access_token"].as_str().unwrap())
    );
    assert!(support::os::private_file(&project.credentials()));
}

fn failed(project: &Project, provider: &FakeProvider) -> Value {
    let run = parsed(
        project
            .against(provider, &["verify", "--all"])
            .output()
            .unwrap(),
        2,
    );
    let request = run["requests"][0].clone();
    assert_eq!(request["status"], "ERROR", "{run}");
    request
}

#[test]
fn provider_errors_explain_themselves_and_permanent_ones_are_not_retried() {
    let project = Project::new();
    project.sign_in(&codex::jwt("account-1", now() + 3600), now() + 3600);

    let expired = FakeProvider::start(|_| {
        Reply::Json(
            401,
            json!({"error":{"message":"Your authentication token has expired.","code":"token_expired"}}),
        )
    });
    let request = failed(&project, &expired);
    let error = request["error"].as_str().unwrap();
    assert!(
        error.starts_with("token_expired: Your authentication token has expired. (HTTP 401)"),
        "{error}"
    );
    assert!(error.contains("artifactize login codex"), "{error}");
    assert_eq!(request["usage"].as_array().unwrap().len(), 1);
    assert_eq!(expired.requests().len(), 1);

    let resets = now() + 90 * 60;
    let limited = FakeProvider::start(move |_| {
        Reply::Json(
            429,
            json!({"error":{"type":"usage_limit_reached","message":"The usage limit has been reached","plan_type":"plus","resets_at":resets}}),
        )
    });
    let request = failed(&project, &limited);
    let error = request["error"].as_str().unwrap();
    assert!(
        error.contains("You have hit your ChatGPT usage limit (plus plan). Try again in ~90 min."),
        "{error}"
    );
    assert_eq!(limited.requests().len(), 1);

    let broken = FakeProvider::start(|_| {
        codex::stream(vec![
            json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_1","object":"response","created_at":1,"model":"gpt-6-luna","status":"in_progress","output":[]}}),
            json!({"type":"response.failed","sequence_number":1,"response":{"id":"resp_1","object":"response","created_at":1,"model":"gpt-6-luna","status":"failed","output":[],"error":{"code":"invalid_prompt","message":"The prompt was rejected."}}}),
        ])
    });
    let error = failed(&project, &broken)["error"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(error.contains("The prompt was rejected."), "{error}");

    fs::remove_file(project.credentials()).unwrap();
    let unused = FakeProvider::script(Vec::new());
    let error = failed(&project, &unused)["error"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        error.starts_with("Codex is not signed in; run `artifactize login codex`"),
        "{error}"
    );
    assert!(unused.requests().is_empty());
}

#[test]
fn a_codex_auth_file_is_used_read_only_and_never_refreshed() {
    let project = Project::new();
    // artifactize's own sign-in is ignored while a file is set.
    project.sign_in(&codex::jwt("account-1", now() + 3600), now() + 3600);
    let access = codex::jwt("claim-account", now() + 3600);
    let file = project.auth_file(&access);
    let before = (
        fs::read(&file).unwrap(),
        file.metadata().unwrap().modified().unwrap(),
    );
    let own = fs::read(project.credentials()).unwrap();
    let provider = FakeProvider::start(reviewer);
    let run = parsed(
        project
            .against(&provider, &["verify", "--all"])
            .env("ARTIFACTIZE_CODEX_AUTH_FILE", &file)
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(run["requests"][0]["status"], "GREEN", "{run}");
    for call in provider.requests() {
        assert_eq!(call.headers["authorization"], format!("Bearer {access}"));
        assert_eq!(call.headers["chatgpt-account-id"], "file-account");
    }
    assert_eq!(
        (
            fs::read(&file).unwrap(),
            file.metadata().unwrap().modified().unwrap()
        ),
        before
    );
    assert_eq!(fs::read(project.credentials()).unwrap(), own);

    let expired = project.auth_file(&codex::jwt("claim-account", now() - 10));
    let provider = FakeProvider::start(reviewer);
    let run = parsed(
        project
            .against(&provider, &["verify", "--all", "--force"])
            .env("ARTIFACTIZE_CODEX_AUTH_FILE", &expired)
            .output()
            .unwrap(),
        2,
    );
    let error = run["requests"][0]["error"].as_str().unwrap();
    assert!(
        error.contains("has expired; sign in with Codex again"),
        "{error}"
    );
    assert!(error.contains("never refreshes that file"), "{error}");
    assert!(provider.requests().is_empty(), "no refresh and no review");
}

#[test]
fn models_lists_the_accounts_picker_models_in_server_order() {
    let project = Project::new();
    project.sign_in(&codex::jwt("account-1", now() + 3600), now() + 3600);
    let provider = FakeProvider::start(|_| {
        Reply::Json(
            200,
            json!({"models":[
                {"slug":"gpt-6-luna","display_name":"GPT-6 Luna","visibility":"list","supported_in_api":true},
                {"slug":"internal","display_name":"Internal","visibility":"hide"},
                {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol","visibility":"list"},
            ]}),
        )
    });
    let listing = parsed(
        project
            .against(&provider, &["models", "codex"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(
        listing,
        json!({"backend":"codex","models":[
            {"slug":"gpt-6-luna","display_name":"GPT-6 Luna"},
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol"},
        ]})
    );
    let call = &provider.requests()[0];
    assert_eq!(
        (call.method.as_str(), call.path.as_str()),
        ("GET", "/backend-api/codex/models?client_version=0.160.0")
    );
    assert_eq!(call.headers["chatgpt-account-id"], "account-1");
    assert_eq!(call.headers["originator"], "artifactize");
    assert!(call.headers["authorization"].starts_with("Bearer "));

    fs::remove_file(project.credentials()).unwrap();
    let failure = parsed(
        project
            .against(&provider, &["models", "codex"])
            .output()
            .unwrap(),
        2,
    );
    assert!(
        failure["error"]
            .as_str()
            .unwrap()
            .contains("artifactize login codex")
    );
}

#[test]
fn logout_revokes_and_removes_only_artifactizes_tokens() {
    let project = Project::new();
    project.sign_in(&codex::jwt("account-1", now() + 3600), now() + 3600);
    let file = project.auth_file(&codex::jwt("claim-account", now() + 3600));
    let before = fs::read(&file).unwrap();
    let provider = FakeProvider::start(|request| match request.path.as_str() {
        "/oauth/revoke" => Reply::Json(200, json!({})),
        _ => Reply::Json(404, json!({})),
    });
    let result = parsed(
        project
            .against(&provider, &["logout", "codex"])
            .env("ARTIFACTIZE_CODEX_AUTH_FILE", &file)
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(
        result,
        json!({"provider":"codex","signed_in":false,"revoked":true})
    );
    assert!(!project.credentials().exists());
    assert_eq!(fs::read(&file).unwrap(), before);
    let calls = provider.requests();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].body,
        json!({"token":"stored-refresh","token_type_hint":"refresh_token","client_id":"app_EMoamEEZ73f0CkXaXp7hrann"})
    );
}

fn check(report: &Value) -> &Value {
    let checks = report["checks"].as_array().unwrap();
    checks
        .iter()
        .find(|check| check["name"] == "codex")
        .unwrap()
}

#[test]
fn doctor_reports_the_sign_in_offline() {
    let project = Project::new();
    let doctor = |command: &mut Command, code| parsed(command.output().unwrap(), code);
    let report = doctor(&mut project.command(&["doctor"]), 0);
    assert_eq!(check(&report)["status"], "WARN");
    assert_eq!(check(&report)["details"]["source"], "none");

    project.sign_in(&codex::jwt("account-1", now() - 10), now() - 10);
    let report = doctor(&mut project.command(&["doctor"]), 0);
    assert_eq!(check(&report)["status"], "PASS");
    assert_eq!(check(&report)["details"]["source"], "stored");
    assert_eq!(check(&report)["details"]["expired"], true);
    assert!(!report.to_string().contains("stored-refresh"));

    let file = project.auth_file(&codex::jwt("claim-account", now() - 10));
    let report = doctor(
        project
            .command(&["doctor"])
            .env("ARTIFACTIZE_CODEX_AUTH_FILE", &file),
        0,
    );
    assert_eq!(check(&report)["status"], "WARN");
    assert_eq!(check(&report)["details"]["source"], "file");
    assert!(
        check(&report)["message"]
            .as_str()
            .unwrap()
            .contains("has expired")
    );

    let report = doctor(
        project
            .command(&["doctor"])
            .env(
                "ARTIFACTIZE_CODEX_BASE_URL",
                "http://127.0.0.1:9/backend-api/codex/",
            )
            .env("ARTIFACTIZE_CODEX_AUTH_URL", "http://localhost:9"),
        0,
    );
    let entry = check(&report);
    assert_eq!(entry["status"], "WARN");
    assert_eq!(
        entry["details"]["testEndpoint"],
        "http://127.0.0.1:9/backend-api/codex"
    );
    assert_eq!(entry["details"]["testAuthEndpoint"], "http://localhost:9");

    let report = doctor(
        project
            .command(&["doctor"])
            .env("ARTIFACTIZE_CODEX_AUTH_URL", "https://auth.openai.com"),
        1,
    );
    assert_eq!(check(&report)["status"], "FAIL");
}

#[test]
fn sign_in_endpoints_are_loopback_only_and_guard_the_review_store() {
    let project = Project::new();
    let failure = parsed(
        project
            .command(&["login", "codex"])
            .env("ARTIFACTIZE_CODEX_AUTH_URL", "https://auth.example")
            .output()
            .unwrap(),
        2,
    );
    let error = failure["error"].as_str().unwrap();
    assert!(
        error.starts_with("ARTIFACTIZE_CODEX_AUTH_URL: a test endpoint"),
        "{error}"
    );
    assert!(!project.credentials().exists());

    let failure = parsed(
        project
            .command(&["verify", "--all"])
            .env("ARTIFACTIZE_CODEX_AUTH_URL", "http://127.0.0.1:9")
            .env("ARTIFACTIZE_REMOTE", "http://127.0.0.1:9/")
            .env("ARTIFACTIZE_REMOTE_TOKEN", "team-token")
            .output()
            .unwrap(),
        2,
    );
    assert!(
        failure["error"]
            .as_str()
            .unwrap()
            .contains("ARTIFACTIZE_CODEX_AUTH_URL")
    );
}

#[test]
fn removed_backend_messages_name_codex_without_a_version() {
    let project = Project::new();
    let path = project.repo.join("spec/artifactize.json");
    let declaration = fs::read_to_string(&path)
        .unwrap()
        .replace("\"codex\"", "\"chatgpt\"");
    fs::write(&path, declaration).unwrap();
    let failure = parsed(project.command(&["config", "check"]).output().unwrap(), 2);
    let error = failure["error"].as_str().unwrap();
    assert!(
        error.ends_with(r#"backend "chatgpt" was removed in 0.5.0; use "openai" or "anthropic" with an API key, or "codex" with a ChatGPT/Codex sign-in"#),
        "{error}"
    );
    assert!(!Path::new(&project.state).join("auth").exists());
}

#[test]
fn codex_and_openai_results_for_the_same_key_reuse_each_other() {
    let project = Project::new();
    project.sign_in(&codex::jwt("account-1", now() + 3600), now() + 3600);
    let provider = FakeProvider::start(|request| {
        let pass = vec![openai::message(r#"{"verdict":"GREEN"}"#)];
        match request.path.as_str() {
            "/backend-api/codex/responses" => codex::completed(request, pass, openai::usage(30, 5)),
            "/v1/responses" => openai::completed(request, pass, openai::usage(20, 4)),
            path => Reply::Json(
                404,
                json!({"error":{"message":format!("unexpected {path}")}}),
            ),
        }
    });
    let verify = |args: &[&str]| {
        let mut command = project.against(&provider, args);
        command
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
            .env("OPENAI_API_KEY", "fake-openai-key");
        parsed(command.output().unwrap(), 0)
    };
    let codex_options =
        json!({"backend":"codex","model":"gpt-6-luna","reasoning":"max","maxToolCalls":2});
    let openai_profile =
        json!({"kind":"agent","backend":"openai","model":"fake-openai-model","reasoning":"high"});

    // The codex review records its execution options beside the result.
    let first = verify(&["verify", "--all"]);
    let produced = &first["requests"][0];
    assert_eq!(produced["status"], "GREEN", "{first}");
    assert_eq!(produced["options"], codex_options);
    let key = produced["key"].as_str().unwrap().to_owned();
    let record = verify(&["cache", "show", &key]);
    assert_eq!(record["options"], codex_options);

    // An openai profile has the same key and reuses the codex result.
    project.profile(openai_profile.clone());
    let second = verify(&["verify", "--all"]);
    let reused = &second["requests"][0];
    assert_eq!(second["executionsStarted"], 0);
    assert_eq!(reused["key"], key);
    assert_eq!(reused["executionId"], produced["executionId"]);
    assert_eq!(reused["options"], codex_options);
    assert_eq!(reused["requestedProfile"]["backend"], "openai");
    assert_eq!(provider.requests().len(), 1);

    // A forced openai review adds a newer record, which codex then reuses.
    let forced = verify(&["verify", "--all", "--force"]);
    let newer = &forced["requests"][0];
    assert_eq!(forced["executionsStarted"], 1);
    assert_eq!(
        newer["options"],
        json!({"backend":"openai","model":"fake-openai-model","reasoning":"high"})
    );
    assert_eq!(provider.requests()[1].path, "/v1/responses");
    project.profile(json!({"kind":"agent","backend":"codex","model":"gpt-6-luna","reasoning":"max","maxToolCalls":2}));
    let back = verify(&["verify", "--all"]);
    assert_eq!(back["executionsStarted"], 0);
    assert_eq!(back["requests"][0]["executionId"], newer["executionId"]);
    assert_eq!(back["requests"][0]["options"]["backend"], "openai");
    assert_eq!(provider.requests().len(), 2);
}
