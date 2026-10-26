//! Providers at the CLI seam: a built-in provider's saved key, Gemini, and custom providers
//! (an alias and an OpenAI-compatible endpoint) that an Agent profile names in `backend`,
//! each reviewing against a loopback fake. Keys go in on stdin and never come back out.

#[path = "support/run.rs"]
mod owned_run;
use owned_run::Watcher;

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};
use support::{FakeProvider, Reply, chat, gemini, openai};

const GEMINI_KEY: &str = "gm-saved-gemini-key-0123";
const ENDPOINT_KEY: &str = "sk-saved-endpoint-key-0123";
const VERDICT: &str = r#"{"verdict":"GREEN","covered":["R1"]}"#;

struct Project {
    _root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
}

impl Project {
    /// One `spec` Artifact with one Agent eval reviewed with `profile`.
    fn new(profile: Value) -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("spec")).unwrap();
        fs::write(repo.join("spec/spec.md"), "R1: the spec covers R1.\n").unwrap();
        let state = root.path().join("state");
        root.track(&state, &repo);
        let project = Self {
            state,
            repo,
            _root: root,
        };
        project.declare(profile);
        project
    }

    fn declare(&self, profile: Value) {
        support::declaration::write(
            self.repo.join("spec/index.artf"),
            json!({
                "name":"spec",
                "evals":[{
                    "id":"review",
                    "title":"Review",
                    "profile":profile,
                    "payload":{"instruction":"Review {spec}."},
                    "pass_schema":{
                        "type":"object",
                        "properties":{"covered":{"type":"array","items":{"type":"string"}}},
                        "required":["covered"],
                    },
                }],
            })
            .to_string(),
        )
        .unwrap();
    }

    /// The CLI with no provider keys or test endpoints of its own environment.
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(support::os::cli());
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .arg("--json")
            .env("ARTIFACTIZE_REMOTE", "off");
        for variable in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "GEMINI_API_KEY",
            "ARTIFACTIZE_OPENAI_BASE_URL",
            "ARTIFACTIZE_ANTHROPIC_BASE_URL",
            "ARTIFACTIZE_GEMINI_BASE_URL",
        ] {
            command.env_remove(variable);
        }
        command
    }

    /// `args` with `secret` on stdin; neither output holds the secret.
    fn with_stdin(&self, args: &[&str], secret: &str) -> Value {
        let mut command = self.command(args);
        command.stdin(Stdio::piped());
        let mut child = Watcher::new(support::os::start(&mut command));
        child
            .stdin
            .take()
            .unwrap()
            .write_all(format!("{secret}\n").as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.code(), Some(0), "{text}");
        assert!(!text.contains(secret), "{text}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn verify(&self, command: &mut Command, code: i32) -> Value {
        parsed(command.record_output().unwrap(), code)
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

#[test]
fn a_custom_endpoint_reviews_by_name_with_its_saved_key() {
    let project = Project::new(json!({
        "kind":"agent", "backend":"local", "model":"local-model", "reasoning":"high",
    }));
    let provider =
        FakeProvider::start(
            |request| match (request.method.as_str(), request.path.as_str()) {
                ("POST", "/v1/chat/completions") => chat::text(request, VERDICT, 20, 4),
                ("GET", "/v1/models") => openai::models(&["local-model"]),
                _ => Reply::Json(404, json!({"error":{"message":"not found"}})),
            },
        );
    let base = provider.openai_base();
    let saved = project.with_stdin(
        &[
            "provider",
            "add",
            "local",
            "--api",
            "openai-chat",
            "--url",
            &base,
            "--key",
        ],
        ENDPOINT_KEY,
    );
    assert_eq!(saved, json!({"provider":"local","saved":true}));
    let list = parsed(
        support::os::run(&mut project.command(&["provider", "list"])),
        0,
    );
    assert_eq!(
        list["custom"],
        json!({"local":{"kind":"endpoint","api":"openai-chat","url":base,"key":true}})
    );
    assert!(!list.to_string().contains(ENDPOINT_KEY));

    let run = project.verify(&mut project.command(&["verify", "--wait", "--all"]), 0);
    assert_eq!(run["requests"][0]["status"], "GREEN", "{run}");
    let review = &provider.requests()[0];
    assert_eq!(review.path, "/v1/chat/completions");
    assert_eq!(
        review.headers["authorization"],
        format!("Bearer {ENDPOINT_KEY}")
    );
    assert_eq!(review.body["model"], "local-model");
    assert_eq!(review.body["reasoning_effort"], "high");
    assert_eq!(review.body["response_format"]["type"], "json_schema");

    let listing = parsed(
        support::os::run(&mut project.command(&["models", "local"])),
        0,
    );
    assert_eq!(
        listing,
        json!({"backend":"local","models":[{"slug":"local-model","display_name":"local-model"}]})
    );

    // Saved again without --key, the endpoint has none.
    parsed(
        support::os::run(&mut project.command(&[
            "provider",
            "add",
            "local",
            "--api",
            "openai-chat",
            "--url",
            &base,
        ])),
        0,
    );
    let list = parsed(
        support::os::run(&mut project.command(&["provider", "list"])),
        0,
    );
    assert_eq!(list["custom"]["local"]["key"], false);
}

#[test]
fn gemini_reviews_with_a_saved_key_and_its_thinking_level() {
    let project = Project::new(json!({
        "kind":"agent", "backend":"gemini", "model":"fake-gemini", "reasoning":"high",
    }));
    let provider = FakeProvider::start(|request| {
        if request
            .path
            .starts_with("/v1beta/models/fake-gemini:streamGenerateContent")
        {
            gemini::text("fake-gemini", VERDICT, 30, 6)
        } else if request.path.starts_with("/v1beta/models") {
            gemini::models(&["fake-gemini"])
        } else {
            Reply::Json(404, json!({"error":{"message":"not found"}}))
        }
    });
    let gemini_base = |command: &mut Command| {
        command.env("ARTIFACTIZE_GEMINI_BASE_URL", &provider.url);
    };

    // No key yet: the review is an authentication ERROR that names both ways to give one.
    let mut command = project.command(&["verify", "--wait", "--all"]);
    gemini_base(&mut command);
    let missing = project.verify(&mut command, 2);
    let error = missing["requests"][0]["error"].to_string();
    assert!(
        error.contains("GEMINI_API_KEY or a saved gemini key is required"),
        "{missing}"
    );

    assert_eq!(
        project.with_stdin(&["login", "gemini"], GEMINI_KEY),
        json!({"provider":"gemini","saved":true})
    );
    let mut command = project.command(&["verify", "--wait", "--all", "--force"]);
    gemini_base(&mut command);
    let run = project.verify(&mut command, 0);
    assert_eq!(run["requests"][0]["status"], "GREEN", "{run}");
    let review = provider
        .requests()
        .into_iter()
        .find(|request| request.method == "POST")
        .unwrap();
    assert!(
        review.path.contains(&format!("key={GEMINI_KEY}")),
        "{}",
        review.path
    );
    assert_eq!(
        review.body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "high"
    );
    assert_eq!(
        review.body["generationConfig"]["responseMimeType"],
        "application/json"
    );

    // A set variable is the key in use; an invalid one never falls back to the saved key.
    let mut command = project.command(&["models", "gemini"]);
    gemini_base(&mut command);
    command.env("GEMINI_API_KEY", "not a key");
    let invalid = parsed(support::os::run(&mut command), 2);
    assert!(
        invalid["error"]
            .to_string()
            .contains("GEMINI_API_KEY is set but invalid"),
        "{invalid}"
    );

    let mut command = project.command(&["models", "gemini"]);
    gemini_base(&mut command);
    let listing = parsed(support::os::run(&mut command), 0);
    assert_eq!(listing["models"][0]["slug"], "fake-gemini", "{listing}");

    // An effort Gemini does not take is refused when the declaration is read.
    project.declare(json!({
        "kind":"agent", "backend":"gemini", "model":"fake-gemini", "reasoning":"xhigh",
    }));
    let check = support::os::run(&mut project.command(&["config", "check"]));
    assert_ne!(check.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&check.stdout).contains("Unsupported reasoning"));

    assert_eq!(
        parsed(
            support::os::run(&mut project.command(&["logout", "gemini"])),
            0
        ),
        json!({"provider":"gemini","saved":false})
    );
    let list = parsed(
        support::os::run(&mut project.command(&["provider", "list"])),
        0,
    );
    let gemini = list["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|key| key["provider"] == "gemini")
        .unwrap();
    assert_eq!(gemini["source"], "absent");
}

#[test]
fn an_alias_reaches_its_provider_and_an_unknown_name_is_an_authentication_error() {
    let project = Project::new(json!({"kind":"agent", "backend":"work", "model":"fake-model"}));
    let provider = FakeProvider::start(|request| {
        openai::completed(
            request,
            vec![openai::message(VERDICT)],
            openai::usage(10, 2),
        )
    });
    let with_openai = |command: &mut Command| {
        command
            .env("OPENAI_API_KEY", "fake-openai-key")
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base());
    };

    // A project may name a provider this machine lacks; it is checked where it runs.
    parsed(
        support::os::run(&mut project.command(&["config", "check"])),
        0,
    );
    let mut command = project.command(&["verify", "--wait", "--all"]);
    with_openai(&mut command);
    let unknown = project.verify(&mut command, 2);
    assert_eq!(
        unknown["requests"][0]["errorCode"], "AUTHENTICATION",
        "{unknown}"
    );
    assert!(
        unknown["requests"][0]["error"]
            .to_string()
            .contains("No provider named work is set up on this machine"),
        "{unknown}"
    );

    parsed(
        support::os::run(&mut project.command(&["provider", "add", "work", "--alias", "openai"])),
        0,
    );
    let mut command = project.command(&["verify", "--wait", "--all", "--force"]);
    with_openai(&mut command);
    let run = project.verify(&mut command, 0);
    assert_eq!(run["requests"][0]["status"], "GREEN", "{run}");
    let review = &provider.requests()[0];
    assert_eq!(review.path, "/v1/responses");
    assert_eq!(review.headers["authorization"], "Bearer fake-openai-key");

    // A built-in name is never a custom provider's.
    let builtin =
        support::os::run(&mut project.command(&["provider", "add", "openai", "--alias", "codex"]));
    assert_ne!(builtin.status.code(), Some(0));
    parsed(
        support::os::run(&mut project.command(&["provider", "remove", "work"])),
        0,
    );
    let list = parsed(
        support::os::run(&mut project.command(&["provider", "list"])),
        0,
    );
    assert_eq!(list["custom"], json!({}));
}
