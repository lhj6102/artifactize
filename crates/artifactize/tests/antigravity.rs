//! Drives the public Chat library and CLI against an owned ACP v1 stdio agent.
#[path = "support/run.rs"]
mod owned_run;
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use artifactize_core::{
    acp::{AgentId, ConfigCategory, ConfigSelection, PermissionStatus, ToolStatus},
    auth::antigravity as account,
    changes::Subscription,
    chat::{Block, Chat, ChatTarget, SendError, TurnState, View},
    runs,
    types::SessionId,
};
use serde_json::{Value, json};
use support::os;
use tokio_util::sync::CancellationToken;

/// Fixture watchdog; ordering comes from the transcript and the scripted peer's barriers.
const BOUND: Duration = Duration::from_secs(30);
const PROBE: &str = "isolated_acp_probe";
const ROOT: &str = "ACP_FIXTURE";

#[test]
fn antigravity_cli_and_chat_at_the_acp_boundary() {
    let root = owned_run::Root::from(os::tempdir());
    let fixture = root.path();
    fs::create_dir(fixture.join("repo")).unwrap();
    root.track(&fixture.join("state"), &fixture.join("repo"));
    fs::write(fixture.join("agy.py"), include_str!("fixtures/agy.py")).unwrap();
    fs::write(fixture.join("mode"), "signed-out").unwrap();
    // Cargo's Rust compiler builds a native PATH entry on every OS; the ACP peer
    // behind it uses the same Python runtime as the portable project fixtures.
    os::compiled_program(
        fixture,
        "agy",
        &format!(
            r#"
fn main() -> std::process::ExitCode {{
    let script = std::path::PathBuf::from(std::env::var_os({ROOT:?}).unwrap()).join("agy.py");
    let status = std::process::Command::new({python:?}).args(["-I", "-X", "utf8"])
        .arg(script).args(std::env::args_os().skip(1)).status().unwrap();
    if status.success() {{ std::process::ExitCode::SUCCESS }} else {{ std::process::ExitCode::FAILURE }}
}}
"#,
            python = os::python_program()
        ),
    );
    let result = os::run_test(
        PROBE,
        &[(ROOT, fixture.into()), ("PATH", os::prepend_path(fixture))],
    );
    assert!(
        result.status.success(),
        "{}\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
        fs::read_to_string(fixture.join("fixture-error")).unwrap_or_default()
    );
}

#[tokio::test]
async fn isolated_acp_probe() {
    let Some(root) = os::environment_variable(ROOT).map(PathBuf::from) else {
        return;
    };
    let fixture = Fixture::new(root);
    fixture.accounts().await;
    fixture.chat().await;
    fixture.cli_chat_agent().await;
    artifactize_core::changes::drain().await;
}

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    state: PathBuf,
    cli: PathBuf,
}

impl Fixture {
    fn new(root: PathBuf) -> Self {
        let repo = root.join("repo");
        let state = root.join("state");
        Self {
            root,
            repo,
            state,
            cli: os::cli(),
        }
    }

    fn cli(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.cli);
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .arg("--json")
            .args(args);
        command
    }

    fn mode(&self, mode: &str) {
        fs::write(self.root.join("mode"), mode).unwrap();
        // Force fresh probe when fixture mode changes.
        artifactize_core::acp::catalog::bump_auth_generation();
    }

    async fn accounts(&self) {
        let result = os::run(
            self.cli(&["models", "antigravity"])
                .env("PATH", os::isolated_path(&self.repo)),
        );
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stdout).contains("not on PATH"));
        let listing = || {
            artifactize_core::llm::models::availability("antigravity".parse().unwrap(), None, None)
        };
        use artifactize_core::llm::models::Availability;
        self.mode("old");
        let old = account::status().await;
        assert!(old.path.as_ref().is_some_and(|path| path.is_file()));
        assert_eq!(old.required_version.to_string(), "1.5.1");
        assert_eq!(old.version.unwrap().to_string(), "1.5.0");
        assert!(matches!(old.state, account::State::TooOld { error } if error.contains("1.5.1")));
        assert!(
            matches!(listing().await, Availability::CliTooOld { version, required_version, .. }
            if version.to_string() == "1.5.0" && required_version.to_string() == "1.5.1")
        );
        self.mode("signed-out");
        assert_eq!(account::status().await.state, account::State::SignedOut);
        assert!(matches!(listing().await, Availability::NotSetUp { .. }));
        self.mode("auth-link");
        assert_eq!(account::status().await.state, account::State::SignedOut);
        self.mode("quota");
        assert!(matches!(
            account::status().await.state,
            account::State::Quota { .. }
        ));
        self.mode("rate");
        assert!(matches!(
            account::status().await.state,
            account::State::RateLimited { .. }
        ));
        self.mode("failed");
        assert!(matches!(
            account::status().await.state,
            account::State::Failed { .. }
        ));
        self.mode("signed-out");

        let mut login = self
            .cli(&["login", "antigravity"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(login.stdin.take().unwrap(), "fixture-code").unwrap();
        let output = os::finish(login, "the Antigravity login relay exits");
        assert!(output.status.success(), "{output:?}");
        let said = String::from_utf8(output.stderr).unwrap();
        assert!(said.contains("https://accounts.google.com/o/oauth2/auth?"));
        assert!(!said.contains("example.invalid") && !said.contains("fixture-code"));
        let signed_in = account::status().await;
        assert_eq!(signed_in.state, account::State::SignedIn { account: None });
        assert!(signed_in.path.is_some());
        assert!(signed_in.version.unwrap() >= signed_in.required_version);

        let models = os::run(&mut self.cli(&["models", "antigravity"]));
        assert!(models.status.success(), "{models:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&models.stdout).unwrap(),
            json!({
                "backend":"antigravity", "models":[
                    {"slug":"first-model", "display_name":"First", "efforts":[], "default_effort":null},
                    {"slug":"chosen-model", "display_name":"Chosen", "efforts":["balanced","deep-thinking"], "default_effort":"balanced"}
                ]
            })
        );
        self.mode("no-logout");
        assert!(account::logout().await.unwrap_err().contains("`/logout`"));
        self.mode("signed-in");
        let ended = os::run(&mut self.cli(&["logout", "antigravity"]));
        assert!(ended.status.success(), "{ended:?}");
        assert_eq!(account::status().await.state, account::State::SignedOut);
        self.mode("signed-in");
    }

    async fn chat(&self) {
        fs::write(self.repo.join("notes.txt"), "fixture notes").unwrap();
        let declaration = self.repo.join("index.artf");
        let declare = |profile| {
            support::declaration::write(
                &self.repo,
                declaration.clone(),
                json!({
                    "name":"app", "fingerprint":false, "type":"files", "evals":[{
                        "id":"check", "title":"Check", "profile":profile,
                        "payload":{"instruction":"Check the fixture."}
                    }]
                })
                .to_string(),
            )
            .unwrap()
        };
        // Antigravity is statically rejected for evals (`C-EVAL-ARTIFACTIZE-DRIVEN`).
        declare(json!({"kind":"agent", "backend":"antigravity", "model":"chosen-model"}));
        let refused = os::run(&mut self.cli(&["config", "check"]));
        assert!(!refused.status.success());
        let said = String::from_utf8(refused.stdout).unwrap();
        assert!(
            said.contains("Antigravity") && said.contains("Chat"),
            "{said}"
        );
        declare(json!({"kind":"runtime", "command":os::true_program(), "args":[]}));

        // `antigravity` is no longer a valid eval `Provider`, so `--alias antigravity` is refused.
        let alias =
            os::run(&mut self.cli(&["provider", "add", "agy-alias", "--alias", "antigravity"]));
        assert!(!alias.status.success());

        let chat = Chat::new(self.state.clone(), self.cli.clone());
        let agent: AgentId = "antigravity".parse().unwrap();
        let target = ChatTarget::Acp {
            agent: agent.clone(),
            config: vec![
                ConfigSelection {
                    id: "account-model".into(),
                    value: "chosen-model".into(),
                    category: Some(ConfigCategory::Model),
                },
                ConfigSelection {
                    id: "think".into(),
                    value: "deep-thinking".into(),
                    category: Some(ConfigCategory::ThoughtLevel),
                },
            ],
        };
        let session = chat.create(&self.repo, Some(target.clone())).await.unwrap();

        // Workspace Trust gate: untrusted send MUST return TrustRequired with zero JSONL written!
        let err = chat
            .send(&session.id, "first message".into(), None)
            .await
            .unwrap_err();
        let SendError::TrustRequired(requirement) = err else {
            panic!("an untrusted send asks for trust: {err:?}");
        };
        assert_eq!(requirement.agent_id, agent);
        assert!(
            chat.open(&session.id).await.unwrap().turns.is_empty(),
            "untrusted send must not record Kind::Send in JSONL"
        );

        // Grant explicit Workspace Trust.
        let trust_record = chat
            .trust_workspace(
                &self.repo,
                &agent,
                &requirement.scope,
                &requirement.fingerprint,
            )
            .await
            .unwrap();
        assert_eq!(trust_record.agent_id, agent);

        owned_run::subscribed(&self.state, async |changes| {
            chat.send(&session.id, "first message".into(), None).await.unwrap();

            // Wait for interactive permission request from ACP peer.
            let waiting = observe(&chat, &session.id, changes, |view| {
                view.waiting_permission
                    && matches!(view.turns[0].state, TurnState::WaitingPermission)
            }).await;
            let perm_id = waiting.turns[0]
                .blocks
                .iter()
                .find_map(|block| match block {
                    Block::Permission {
                        permission_id,
                        status: PermissionStatus::Pending,
                        ..
                    } => Some(permission_id.clone()),
                    _ => None,
                })
                .expect("pending ACP permission block");
            assert!(
                chat.respond_permission(&session.id, &perm_id, "permit")
                    .await
                    .unwrap()
            );

            // Wait until the peer emits rich blocks, calls verify_start, and waits for Stop.
            let streaming = observe(&chat, &session.id, changes, |view| {
                view.running
                    && view.turns[0].blocks.iter().any(|block| {
                        matches!(block, Block::Text { text } if text == "Waiting for Stop.")
                    })
            })
            .await;
            let turn = &streaming.turns[0];
            assert!(turn.blocks.iter().any(|block| matches!(block, Block::Thought { text } if text.contains("Inspecting workspace"))));
            assert!(turn.blocks.iter().any(|block| matches!(block, Block::Plan { steps } if steps.len() == 2)));
            assert!(turn.blocks.iter().any(|block| matches!(
                block,
                Block::Tool {
                    title: Some(title),
                    status: ToolStatus::Completed,
                    locations,
                    diff: Some(_),
                    ..
                } if title == "Read notes.txt" && !locations.is_empty()
            )));
            let texts = turn
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(texts, ["Reading notes.", "Waiting for Stop."]);
            let run = turn
                .blocks
                .iter()
                .find_map(|block| match block {
                    Block::Run { card } => Some(card.id.clone()),
                    _ => None,
                })
                .expect("Verify Run reference");
            let pids = peer_pids(&self.root);
            assert!(pids.iter().all(|&pid| os::running(pid)));
            assert!(chat.stop(&session.id).await.unwrap());
            let stopped = observe(&chat, &session.id, changes, |view| !view.running).await;
            assert!(matches!(stopped.turns[0].state, TurnState::Stopped));
            assert_eq!(
                fs::read_to_string(self.root.join("cancelled")).unwrap(),
                "fixture-session"
            );
            assert!(
                pids.iter().all(|&pid| !os::running(pid)),
                "ACP peer and MCP shim must exit"
            );
            chat.close().await;

            // Admitted Verify Run settles independently to GREEN with Origin::Chat including send=1.
            let context = runs::Context::new(&self.repo, Some(&self.state), &self.cli).unwrap();
            let settled =
                runs::wait(&context, &run, Some(os::patience(BOUND)), CancellationToken::new())
                    .await
                    .unwrap();
            assert!(matches!(settled.state, runs::State::Green), "{settled:?}");
            let receipt = artifactize_core::store::read_run(&self.state, run.as_str())
                .await
                .unwrap();
            assert_eq!(
                receipt.run.origin,
                Some(runs::Origin::Chat {
                    session_id: session.id.clone(),
                    title: "first message".into(),
                    send: Some(1),
                })
            );

            // Turn 2 after restart: opens session/new + Handoff, completes and parks warm child.
            let restarted = Chat::new(self.state.clone(), self.cli.clone());
            let restored = restarted.open(&session.id).await.unwrap();
            assert_eq!(restored.session.target.as_ref(), Some(&target));
            assert!(
                restored.turns[0]
                    .blocks
                    .iter()
                    .any(|block| matches!(block, Block::Run { card } if card.id == run))
            );
            restarted
                .send(&session.id, "continue after restart".into(), None)
                .await
                .unwrap();
            let resumed = observe(&restarted, &session.id, changes, |view| {
                !view.running && view.turns.len() == 2
            })
            .await;
            assert!(
                matches!(resumed.turns[1].state, TurnState::Completed),
                "{:?}",
                resumed.turns[1]
            );
            assert!(resumed.turns[1].blocks.iter().any(|block| matches!(block, Block::Text { text } if text == "Ready again.")));

            // Turn 3 on the same Chat instance: reuses warm child via Path A Same-Child `session/load`
            // and suppresses replayed LoadPhase noise!
            restarted
                .send(&session.id, "warm follow-up".into(), None)
                .await
                .unwrap();
            let warm = observe(&restarted, &session.id, changes, |view| {
                !view.running && view.turns.len() == 3
            })
            .await;
            assert!(matches!(warm.turns[2].state, TurnState::Completed));
            assert_eq!(
                fs::read_to_string(self.root.join("warm-pid")).unwrap(),
                fs::read_to_string(self.root.join("warm-followup-pid")).unwrap(),
                "consecutive completed ACP turns should reuse the same warm child process"
            );
            assert_eq!(
                fs::read_to_string(self.root.join("loaded")).unwrap(),
                "fixture-session"
            );
            let turn3_text: String = warm.turns[2]
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(turn3_text, "Warm pool reused.");
            assert!(
                !turn3_text.contains("REPLAYED_NOISE"),
                "LoadPhase replay must be suppressed"
            );
            restarted.close().await;
        })
        .await;
    }

    async fn cli_chat_agent(&self) {
        let list = os::run(&mut self.cli(&["chat-agent", "list"]));
        assert!(list.status.success(), "{list:?}");
        let entries: Value = serde_json::from_slice(&list.stdout).unwrap();
        assert!(
            entries
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == "antigravity")
        );

        let added = os::run(&mut self.cli(&[
            "chat-agent",
            "add",
            "local-agy",
            "--name",
            "Local AGY",
            "--command",
            "agy",
            "--arg",
            "acp",
        ]));
        assert!(added.status.success(), "{added:?}");

        let probe = os::run(&mut self.cli(&["chat-agent", "probe", "local-agy"]));
        assert!(probe.status.success(), "{probe:?}");
        let probed: Value = serde_json::from_slice(&probe.stdout).unwrap();
        assert_eq!(probed["kind"], "ready");

        let trust = os::run(&mut self.cli(&["chat-agent", "trust", "list"]));
        assert!(trust.status.success(), "{trust:?}");

        let removed = os::run(&mut self.cli(&["chat-agent", "remove", "local-agy"]));
        assert!(removed.status.success(), "{removed:?}");
    }
}

fn peer_pids(root: &Path) -> Vec<u32> {
    let pids = fs::read_to_string(root.join("pids")).unwrap();
    pids.split_whitespace()
        .map(|pid| pid.parse().unwrap())
        .collect()
}

async fn observe(
    chat: &Chat,
    id: &SessionId,
    changes: &mut Subscription,
    ready: impl Fn(&View) -> bool,
) -> View {
    let observation = tokio::time::timeout(os::patience(BOUND), async {
        loop {
            let view = chat.open(id).await.unwrap();
            if ready(&view) {
                return view;
            }
            if view
                .turns
                .last()
                .is_some_and(|turn| matches!(turn.state, TurnState::Failed { .. }))
            {
                panic!("unexpected Chat failure: {:?}", view.turns);
            }
            changes.next_reconciled().await;
        }
    })
    .await;
    if let Ok(view) = observation {
        return view;
    }
    let last = chat.open(id).await.unwrap();
    chat.close().await;
    panic!(
        "expected transcript state not observed: running={}, turns={:?}",
        last.running, last.turns
    );
}
