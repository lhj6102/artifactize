use super::*;
use crate::test_os as os;
#[path = "../../../tests/support/tool_sessions.rs"]
mod harness;

mod fixture;
use fixture::OwnedFixture;

static CLOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn advance_lifecycle_clock() {
    crate::tools::sessions::executor()
        .unwrap()
        .spawn(async {
            tokio::time::pause();
            tokio::time::advance(std::time::Duration::from_secs(60)).await;
            tokio::time::resume();
        })
        .await
        .unwrap();
}

fn fixture(stop: &str) -> OwnedFixture {
    let mut fixture = Fixture::new("openai");
    let log = fixture.directory.path().join("log");
    fs::create_dir(&log).unwrap();
    let path = fixture.config.root.join("index.artf");
    let mut definition = crate::test_declaration::read(fs::read(&path).unwrap()).unwrap();
    let mut views = harness::views(&log, "start", stop, "hold");
    views["agent_tools"]["act"]["input_schema"] = json!({"type":"object"});
    definition["views"] = views;
    crate::test_declaration::write(&path, definition.to_string()).unwrap();
    fs::write(fixture.config.root.join("harness.py"), harness::script()).unwrap();
    fixture.config = read_workspace_config(&fixture.config.root).unwrap();
    OwnedFixture::new(fixture)
}

async fn owned_review(fixture: &OwnedFixture, cancellation: CancellationToken) -> Review {
    let sessions = fixture.start_sessions(cancellation.clone()).await.unwrap();
    let http = SequencedHttpClient::new(vec![
        openai_response(
            "exact-model",
            "completed",
            vec![call("c1", "act_a")],
            Value::Null,
        ),
        final_openai(),
    ]);
    let client = Client::Openai(
        Box::new(
            OpenAIConfig::new("fake-key")
                .connect(http)
                .responses("exact-model"),
        ),
        "fake-key".into(),
    );
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
    let _clock = CLOCK.lock().await;
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
    let _clock = CLOCK.lock().await;
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
    advance_lifecycle_clock().await;
    assert_eq!(future.await.result.unwrap_err().code, Code::Cancelled);
    harness::assert_cleaned(&fixture.state(), &log, 1);
}

#[tokio::test]
async fn an_unready_harness_prepares_without_a_fixed_bound_until_cancelled() {
    let _clock = CLOCK.lock().await;
    let mut fixture = fixture("stop");
    let log = fixture.directory.path().join("log");
    let path = fixture.config.root.join("index.artf");
    let mut declaration = crate::test_declaration::read(fs::read(&path).unwrap()).unwrap();
    declaration["views"]["agent_sessions"]["app"]["start"]["args"][1] = json!("unready");
    crate::test_declaration::write(&path, declaration.to_string()).unwrap();
    fixture.config = read_workspace_config(&fixture.config.root).unwrap();
    let state = fixture.state();
    let cancellation = CancellationToken::new();
    let mut start = fixture.start_sessions(cancellation.clone());
    tokio::select! {
        _ = reached(|| !harness::records(&log, "start").is_empty()) => {},
        _ = &mut start => panic!("unready session was admitted"),
    }
    // Well past the stop bound, a harness still preparing is neither admitted nor failed.
    advance_lifecycle_clock().await;
    tokio::select! {
        biased;
        _ = &mut start => panic!("an unready harness was given up on"),
        _ = tokio::task::yield_now() => {},
    }
    cancellation.cancel();
    assert!(start.await.err().unwrap().contains("start was cancelled"));
    harness::assert_cleaned(&state, &log, 1);
}

#[test]
fn panic_and_caller_runtime_shutdown_still_run_stop_and_cleanup_bound_calls() {
    let _clock = CLOCK.blocking_lock();
    for panic in [true, false] {
        let fixture = fixture("stop");
        let log = fixture.directory.path().join("log");
        let config = read_workspace_config(&fixture.config.root).unwrap();
        let starting = fixture.start_sessions(CancellationToken::new());
        let output = fixture.output.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let review = runtime.spawn(async move {
            let registry = Registry::new(&config, &config.evals[0].id).unwrap();
            let sessions = starting.await.unwrap();
            if panic {
                panic!("review panic after owning a session");
            }
            registry
                .with_sessions(&sessions)
                .call("act_a", json!({}), &output, CancellationToken::new())
                .await;
            sessions.close().await.unwrap();
        });
        if panic {
            assert!(runtime.block_on(review).unwrap_err().is_panic());
        } else {
            runtime.block_on(reached(|| !harness::records(&log, "call").is_empty()));
        }
        drop(runtime);
        os::wait_until(std::time::Duration::from_secs(15), || {
            !harness::records(&log, "stop").is_empty()
                && harness::directories(&fixture.state()).is_empty()
        });
        harness::assert_cleaned(&fixture.state(), &log, 1);
        for call in harness::records(&log, "call") {
            assert!(!os::running(call["pid"].as_u64().unwrap() as u32));
        }
    }
}

// This fixture uses Unix directory write permission; Windows directory attributes
// are not equivalent. Windows pending-delete sharing has its own CLI seam.
#[cfg(unix)]
#[tokio::test]
async fn directory_cleanup_failure_is_recovered_after_lease_release_even_with_a_live_owner() {
    let _clock = CLOCK.lock().await;
    let mut fixture = fixture("readonly-stop");
    let log = fixture.directory.path().join("log");
    let state = fixture.state();
    let sessions = fixture
        .start_sessions(CancellationToken::new())
        .await
        .unwrap();
    assert!(sessions.close().await.is_err());
    let start = harness::records(&log, "start").pop().unwrap();
    let directory = PathBuf::from(start["directory"].as_str().unwrap());
    assert!(directory.join("owner.json").is_file());
    os::wait_until(std::time::Duration::from_secs(10), || {
        !os::running(start["pid"].as_u64().unwrap() as u32)
            && !os::running(start["child"].as_u64().unwrap() as u32)
    });
    // The caller process remains alive, but the failed supervisor's lease is gone.
    os::restore_directory_writes(&directory);
    let path = fixture.config.root.join("index.artf");
    let mut declaration = crate::test_declaration::read(fs::read(&path).unwrap()).unwrap();
    declaration["views"]["agent_sessions"]["app"]["stop"]["args"][1] = json!("stop");
    crate::test_declaration::write(&path, declaration.to_string()).unwrap();
    fixture.config = read_workspace_config(&fixture.config.root).unwrap();
    let next = fixture
        .start_sessions(CancellationToken::new())
        .await
        .unwrap();
    assert!(!directory.exists());
    next.close().await.unwrap();
    harness::assert_cleaned(&state, &log, 2);
}

#[test]
fn fixture_unwind_joins_sessions_before_removing_their_inputs() {
    let _clock = CLOCK.blocking_lock();
    let mut observed = None;
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let fixture = fixture("hang-stop");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _sessions = runtime
            .block_on(fixture.start_sessions(CancellationToken::new()))
            .unwrap();
        let start = harness::records(&fixture.directory.path().join("log"), "start")
            .pop()
            .unwrap();
        observed = Some((fixture.directory.path().to_owned(), start));
        panic!("fixture assertion after native readiness");
    }));
    assert!(failed.is_err());
    let (directory, start) = observed.unwrap();
    assert!(
        !directory.exists(),
        "the fixture was released only after its session close joined"
    );
    assert!(!os::running(start["pid"].as_u64().unwrap() as u32));
    assert!(!os::running(start["child"].as_u64().unwrap() as u32));
}
