//! Live targets through process argv, reuse, observed snapshots and saved public records.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[path = "support/run.rs"]
mod owned_run;
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;
use support::runs::RecordOutput;

struct Project {
    root: owned_run::Root,
    repo: PathBuf,
    state: PathBuf,
    provider: Option<String>,
}
impl Project {
    fn new() -> Self {
        let root = owned_run::Root::from(support::os::tempdir());
        let repo = root.path().join("repo");
        let state = root.path().join("state");
        fs::create_dir_all(repo.join(".artifactize")).unwrap();
        root.track(&state, &repo);
        Self {
            root,
            repo,
            state,
            provider: None,
        }
    }
    fn node(&self, name: &str, declaration: Value) {
        fs::write(
            self.repo.join(format!(".artifactize/{name}.artf")),
            support::declaration::to_toml(declaration).unwrap(),
        )
        .unwrap();
    }
    fn json(&self, args: &[&str], code: i32) -> Value {
        let mut command = Command::new(support::os::cli());
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(&self.state)
            .args(args)
            .arg("--json")
            .env("ARTIFACTIZE_REMOTE", "off")
            .env("LIVE_AUTHOR_MARKER", "from-the-terminal");
        if let Some(endpoint) = &self.provider {
            command
                .env("OPENAI_API_KEY", "fake-key")
                .env("ARTIFACTIZE_OPENAI_BASE_URL", endpoint);
        }
        let output = command.record_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn snapshot(&self) -> Value {
        fs::write(self.repo.join("snapshot.txt"), "before").unwrap();
        json!({"script":{"command":support::os::cat_program(),"args":["snapshot.txt"]}})
    }
}
fn runtime(command: String, args: Value, instruction: &str) -> Value {
    json!({"title":"Observe","profile":{"kind":"runtime","command":command,"args":args},"payload":{"instruction":instruction}})
}
fn request<'a>(run: &'a Value, id: &str) -> &'a Value {
    run["requests"]
        .as_array()
        .unwrap()
        .iter()
        .find(|request| request["evalId"] == id)
        .unwrap()
}
fn passing(request: &support::Request) -> support::Reply {
    support::openai::completed(
        request,
        vec![support::openai::message("{\"findings\":[]}")],
        support::openai::usage(10, 2),
    )
}

#[test]
fn live_paths_are_opaque_and_configuration_rejects_file_observation_forms() {
    let project = Project::new();
    project.node(
        "service",
        json!({"path":"https://does-not-exist.invalid/opaque","basis":true}),
    );
    let check = project.json(&["config", "check"], 0);
    assert!(
        check
            .get("notes")
            .is_none_or(|notes| notes.as_array().is_some_and(Vec::is_empty))
    );
    let graph = project.json(&["config", "graph"], 0);
    assert_eq!(graph["artifacts"]["service"]["kind"], "live");
    assert_eq!(
        graph["artifacts"]["service"]["path"],
        "https://does-not-exist.invalid/opaque"
    );
    for extra in [
        json!({"select":["file"]}),
        json!({"fingerprint":{}}),
        json!({"views":{"agent_tools":{"read":{"builtin":"read"}}}}),
    ] {
        let mut declaration = json!({"path":"service:opaque","basis":true});
        declaration
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        project.node("service", declaration);
        let error = project.json(&["config", "check"], 2);
        assert!(error["error"].as_str().unwrap().contains("line"));
    }
}

#[test]
fn runtime_observations_receive_uris_run_from_the_project_and_never_reuse_even_with_a_snapshot() {
    let project = Project::new();
    let program = support::os::steps_program(project.root.path());
    project.node("service", json!({
        "path":"service:opaque-instance","fingerprint":project.snapshot(),
        "evals":{"observe":runtime(program, json!(["append","received.txt","{service}","append","received.txt","--target={service}/health","env","environment.txt","LIVE_AUTHOR_MARKER","cwd"]), "Observe {service}.")}
    }));
    let first = project.json(&["verify", "--all", "--wait"], 0);
    let second = project.json(&["verify", "--all", "--wait"], 0);
    assert_eq!(first["executionsStarted"], 1);
    assert_eq!(second["executionsStarted"], 1);
    assert_ne!(
        request(&first, "service/observe")["executionId"],
        request(&second, "service/observe")["executionId"]
    );
    assert!(request(&second, "service/observe")["key"].is_null());
    assert!(request(&second, "service/observe")["fingerprints"]["service"].is_string());
    assert_eq!(
        fs::read_to_string(project.repo.join("received.txt")).unwrap(),
        "service:opaque-instance\n--target=service:opaque-instance/health\nservice:opaque-instance\n--target=service:opaque-instance/health\n"
    );
    assert_eq!(
        fs::read_to_string(project.repo.join("environment.txt")).unwrap(),
        "from-the-terminal\nfrom-the-terminal\n"
    );
    assert_eq!(
        request(&second, "service/observe")["result"]["stdout"]
            .as_str()
            .unwrap()
            .trim(),
        support::os::path_text(&support::os::canonical(&project.repo))
    );
    let saved = project.json(&["run", "show", first["id"].as_str().unwrap()], 0);
    assert!(saved.to_string().contains("service:opaque-instance"));
}

#[test]
fn an_agent_reuses_a_live_snapshot_until_it_changes_while_a_runtime_reference_always_observes() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let provider = support::FakeProvider::start(move |request| {
        counted.fetch_add(1, Ordering::SeqCst);
        passing(request)
    });
    let mut project = Project::new();
    project.provider = Some(provider.openai_base());
    project.node("service", json!({"path":"service:opaque","fingerprint":project.snapshot(),"evals":{"review":{"title":"Review the snapshot","profile":{"kind":"agent","backend":"openai","model":"fixture-model"},"payload":{"instruction":"Review {service}."}}}}));
    fs::create_dir(project.repo.join("consumer")).unwrap();
    fs::write(project.repo.join("consumer/input.txt"), "consumer").unwrap();
    project.node("consumer", json!({"path":"consumer/","evals":{"observe":runtime(support::os::echo_program(),json!(["{service}/health"]),"Observe {service}.")}}));
    let first = project.json(&["verify", "--all", "--wait"], 0);
    let second = project.json(&["verify", "--all", "--wait"], 0);
    assert_eq!(first["executionsStarted"], 2);
    assert_eq!(second["executionsStarted"], 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        request(&first, "service/review")["executionId"],
        request(&second, "service/review")["executionId"]
    );
    assert!(request(&second, "consumer/observe")["key"].is_null());
    fs::write(project.repo.join("snapshot.txt"), "after").unwrap();
    let third = project.json(&["verify", "--all", "--wait"], 0);
    assert_eq!(third["executionsStarted"], 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn an_unkeyed_runtime_snapshot_changing_during_observation_is_input_changed() {
    let project = Project::new();
    project.node("service", json!({"path":"service:opaque","fingerprint":project.snapshot(),"evals":{"observe":runtime(support::os::steps_program(project.root.path()),json!(["write","snapshot.txt","after"]),"Observe {service}.")}}));
    let run = project.json(&["verify", "--all", "--wait"], 2);
    assert_eq!(
        request(&run, "service/observe")["errorCode"],
        "INPUT_CHANGED"
    );
    assert!(request(&run, "service/observe")["key"].is_null());
}

#[test]
fn an_agent_snapshot_changing_before_its_verdict_is_input_changed() {
    let mut project = Project::new();
    let snapshot = project.snapshot();
    let path = project.repo.join("snapshot.txt");
    let provider = support::FakeProvider::start(move |request| {
        fs::write(&path, "after").unwrap();
        passing(request)
    });
    project.provider = Some(provider.openai_base());
    project.node("service", json!({"path":"service:opaque","fingerprint":snapshot,"evals":{"review":{"title":"Review","profile":{"kind":"agent","backend":"openai","model":"fixture-model"},"payload":{"instruction":"Review {service}."}}}}));
    let run = project.json(&["verify", "--all", "--wait"], 2);
    assert_eq!(
        request(&run, "service/review")["errorCode"],
        "INPUT_CHANGED"
    );
}

#[tokio::test]
async fn human_live_snapshots_reuse_and_recheck_through_saved_requests() {
    use artifactize_core::{
        human,
        project::{VerifyOptions, selection::Selection},
        store::OwnedReceipts,
    };
    use tokio_util::sync::CancellationToken;
    let project = Project::new();
    project.node("service", json!({"path":"service:opaque","fingerprint":project.snapshot(),"evals":{"review":{"title":"Fixture snapshot sign-off","profile":{"kind":"human"},"payload":{"instruction":"Review {service}."}}}}));
    let verify = async || {
        support::runs::record(
            &project.repo,
            Some(&project.state),
            &Selection::All,
            &VerifyOptions::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap()
    };
    let first = verify().await;
    let reviewer = "fixture-reviewer".parse().unwrap();
    let receipts = OwnedReceipts::open(&project.state, &project.repo)
        .await
        .unwrap();
    let id = &first.requests[0].id;
    human::claim(&receipts, id, &reviewer).await.unwrap();
    let submitted = human::submit(
        &receipts,
        id,
        &reviewer,
        &human::Submission::try_from(json!({"verdict":"GREEN"})).unwrap(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(submitted.status().as_str(), "GREEN");
    drop(receipts);
    let second = verify().await;
    assert_eq!(second.run.executions_started, 0);
    assert_eq!(second.requests[0].status().as_str(), "GREEN");
    fs::write(project.repo.join("snapshot.txt"), "second").unwrap();
    let third = verify().await;
    assert_eq!(third.requests[0].status().as_str(), "DEFERRED");
    let receipts = OwnedReceipts::open(&project.state, &project.repo)
        .await
        .unwrap();
    let id = &third.requests[0].id;
    human::claim(&receipts, id, &reviewer).await.unwrap();
    fs::write(
        project.repo.join("snapshot.txt"),
        "changed-during-fixture-review",
    )
    .unwrap();
    let error = human::submit(
        &receipts,
        id,
        &reviewer,
        &human::Submission::try_from(json!({"verdict":"GREEN"})).unwrap(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("Fingerprint changed"), "{error}");
    drop(receipts);
}

#[tokio::test]
async fn command_tools_and_fingerprint_scripts_receive_the_same_opaque_live_address() {
    use artifactize_core::{
        config,
        tools::{Content, Registry},
    };
    use tokio_util::sync::CancellationToken;
    let project = Project::new();
    project.node("service", json!({
        "path":"service:opaque?argument=value",
        "fingerprint":{"script":{"command":support::os::python_program(),"args":["-I", "-c", "import pathlib,sys; pathlib.Path('fingerprint-argv.txt').write_text(sys.argv[1]); print('stable')", "{service}/snapshot"]}},
        "views":{"agent_tools":{"observe":{"description":"Observe a URI","protocol":"json","command":support::os::python_program(),"args":["-I", "-c", "import json,sys; print(json.dumps({'content':[{'type':'text','text':sys.argv[1]}]}))", "--target={service}/health"]}}},
        "evals":{"observe":runtime(support::os::true_program(),json!([]),"Observe {service}.")}
    }));
    let config = config::read_workspace_config(&project.repo).unwrap();
    let registry = Registry::for_artifact(&config, &"service".parse().unwrap()).unwrap();
    registry.preflight("observe_service").unwrap();
    let result = registry
        .call(
            "observe_service",
            json!({}),
            &project.root.path().join("tool-output"),
            CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error(), "{result:?}");
    let content = result.into_content();
    assert!(content.iter().any(|part| matches!(part, Content::Text { text } if text.contains("--target=service:opaque?argument=value/health"))), "{content:?}");
    let run = project.json(&["verify", "--all", "--wait"], 0);
    assert!(request(&run, "service/observe")["fingerprints"]["service"].is_string());
    assert_eq!(
        fs::read_to_string(project.repo.join("fingerprint-argv.txt")).unwrap(),
        "service:opaque?argument=value/snapshot"
    );
}
