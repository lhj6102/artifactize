use super::*;
use crate::test_os as os;
#[path = "../../../tests/support/tool_sessions.rs"]
mod harness;

fn fixture(stop: &str) -> Fixture {
    let mut fixture = Fixture::new("openai");
    let log = fixture.directory.path().join("log");
    fs::create_dir(&log).unwrap();
    let path = fixture.config.root.join("index.artf");
    let mut definition = crate::test_declaration::read(fs::read(&path).unwrap()).unwrap();
    let mut views = harness::views(&log, "start", stop, "hold");
    views["agent_tools"]["act"]["input_schema"] = json!({"type":"object"});
    definition["views"] = views;
    crate::test_declaration::write(&path, definition.to_string()).unwrap();
    fs::write(fixture.config.root.join("harness.py"), harness::HARNESS).unwrap();
    fixture.config = read_workspace_config(&fixture.config.root).unwrap();
    fixture
}

async fn owned_review(fixture: &Fixture, cancellation: CancellationToken) -> Review {
    let registry = Registry::new(&fixture.config, &fixture.config.evals[0].id).unwrap();
    let sessions = registry
        .start_sessions(&fixture.state(), None, cancellation.clone())
        .await
        .unwrap();
    let http = SequencedHttpClient::new(vec![
        openai_response(
            "exact-model",
            "completed",
            vec![call("c1", "act_a")],
            Value::Null,
        ),
        final_openai(),
    ]);
    let client = Client::Openai(Box::new(
        OpenAIConfig::new("fake-key")
            .connect(http)
            .responses("exact-model"),
    ));
    let mut recorder = session::Recorder::off(&SESSION.parse().unwrap());
    let result = review(
        &client,
        &fixture.config,
        &fixture.config.evals[0],
        &fixture.output,
        &mut recorder,
        cancellation,
        &sessions,
    )
    .await;
    sessions.close().await.unwrap();
    result
}

async fn reached(condition: impl Fn() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("harness handshake");
}

#[tokio::test]
async fn cancellation_and_dropped_review_run_stop_with_an_independent_token() {
    for cancel in [true, false] {
        let fixture = fixture("stop");
        let log = fixture.directory.path().join("log");
        let cancellation = CancellationToken::new();
        let mut future = Box::pin(owned_review(&fixture, cancellation.clone()));
        tokio::select! {
            _ = reached(|| !harness::records(&log, "call").is_empty()) => {},
            _ = &mut future => panic!("review completed before tool handshake"),
        }
        if cancel {
            cancellation.cancel();
            assert_eq!(future.await.result.unwrap_err().code, Code::Cancelled);
        } else {
            drop(future);
        }
        reached(|| {
            !harness::records(&log, "stop").is_empty()
                && harness::directories(&fixture.state()).is_empty()
        })
        .await;
        harness::assert_cleaned(&fixture.state(), &log, 1);
    }
}

#[tokio::test]
async fn hanging_stop_cannot_retain_the_group_or_session_directory() {
    let fixture = fixture("hang-stop");
    let log = fixture.directory.path().join("log");
    let cancellation = CancellationToken::new();
    let mut future = Box::pin(owned_review(&fixture, cancellation.clone()));
    tokio::select! {
        _ = reached(|| !harness::records(&log, "call").is_empty()) => {},
        _ = &mut future => panic!("review completed before tool handshake"),
    }
    cancellation.cancel();
    tokio::select! {
        _ = reached(|| !harness::records(&log, "stop").is_empty()) => {},
        _ = &mut future => panic!("stop did not hang"),
    }
    // Advance only after stop has actually entered. This checks bounded cleanup
    // without assuming how fast a process starts on any operating system.
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    tokio::time::resume();
    assert_eq!(future.await.result.unwrap_err().code, Code::Cancelled);
    harness::assert_cleaned(&fixture.state(), &log, 1);
}

#[tokio::test]
async fn readiness_timeout_cleans_a_started_but_unready_harness() {
    let mut fixture = fixture("stop");
    let log = fixture.directory.path().join("log");
    let path = fixture.config.root.join("index.artf");
    let mut declaration = crate::test_declaration::read(fs::read(&path).unwrap()).unwrap();
    declaration["views"]["agent_sessions"]["app"]["start"]["args"][1] = json!("unready");
    crate::test_declaration::write(&path, declaration.to_string()).unwrap();
    fixture.config = read_workspace_config(&fixture.config.root).unwrap();
    let registry = Registry::new(&fixture.config, &fixture.config.evals[0].id).unwrap();
    let state = fixture.state();
    let mut start = Box::pin(registry.start_sessions(&state, None, CancellationToken::new()));
    tokio::select! {
        _ = reached(|| !harness::records(&log, "start").is_empty()) => {},
        _ = &mut start => panic!("unready session was admitted"),
    }
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    tokio::time::resume();
    assert!(start.await.err().unwrap().contains("start timed out"));
    harness::assert_cleaned(&state, &log, 1);
}
