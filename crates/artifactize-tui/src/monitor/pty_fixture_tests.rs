//! Exercise the real Monitor loop and live transcript on the shared Unix/Windows PTY.
use super::*;
use crate::{
    agent::session::{
        Answer, Delivery, DeliveryKind, DeliveryState, Header, Kind, Recorder, Saving,
    },
    test_os::{self, PseudoTerminal},
    types::DeliveryBlock,
};
use serde_json::json;
use std::{
    fs,
    sync::mpsc::Receiver,
    time::{Duration, Instant},
};

const TEST: &str = "monitor::pty_fixture_tests::live_session_pty_fixture";
// A second exact libtest filter carries the state path; it matches no test of its own.
const STATE_ARGUMENT: &str = "fixture-state=";
/// Bound failures, not the time given to redraw: every key waits for the previous frame.
const FRAME_DEADLINE: Duration = Duration::from_secs(10);
/// A complete terminal sequence for Left, which backs out of Detail.
const LEFT: &[u8] = b"\x1b[D";
/// Uppercase, non-overlapping markers replace the lowercase rows and loading text
/// without Ratatui eliding unchanged letters from the terminal's patch stream.
const SEED: &str = "QUACK-BUZZ";
const DELIVERY: &str = "LIVE_PROVIDER_FRAGMENT";
const RUNTIME: &str = "RUNTIME_OUTPUT_VISIBLE";
const APPEND: &str = "NEW_ANSWER_AFTER_SWITCH";

#[tokio::test]
async fn live_session_pty_fixture() {
    if let Some(state) =
        std::env::args().find_map(|arg| arg.strip_prefix(STATE_ARGUMENT).map(PathBuf::from))
    {
        terminal::run(state, None, tokio_util::sync::CancellationToken::new())
            .await
            .unwrap();
        return;
    }

    let root = test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir_all(&repo).unwrap();
    let receipts = store::OwnedReceipts::open(&state, &repo).await.unwrap();
    let run: store::Run = serde_json::from_value(json!({
        "id":"fixture-run",
        "repoPath":repo,
        "stateDir":state,
        "status":"RUNNING",
        "createdAt":"2026-01-01T00:00:00Z",
        "selection":{"kind":"all"},
        "validation":null,
        "definitions":{
            "artifacts":{"app":{"path":""}},
            "evals":[
                {
                    "id":"app/check",
                    "target":"app",
                    "deps":[],
                    "declaration":{
                        "title":"Fixture Agent",
                        "profile":{"kind":"agent","backend":"openai","model":"fixture"},
                        "payload":{"instruction":"fixture"},
                    },
                },
                {
                    "id":"app/runtime",
                    "target":"app",
                    "deps":[],
                    "declaration":{
                        "title":"Fixture Runtime",
                        "profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]},
                        "payload":{"instruction":"fixture"},
                    },
                },
            ],
        },
    }))
    .unwrap();
    let mut view = tests::request(
        "app/check",
        "RUNNING",
        json!({
            "profile":{"kind":"agent","backend":"openai","model":"fixture"},
            "sessionId":"fixture-session",
        }),
    );
    std::sync::Arc::make_mut(&mut view.request).run_id = run.id.clone();
    let mut runtime = tests::request(
        "app/runtime",
        "GREEN",
        json!({
            "completedAt":"2026-01-01T00:01:00Z",
            "result":{"verdict":"GREEN","exitCode":0,"stdout":RUNTIME,"stderr":""},
        }),
    );
    std::sync::Arc::make_mut(&mut runtime.request).run_id = run.id.clone();
    receipts
        .create_run(
            &run,
            &[
                view.request.as_ref().clone(),
                runtime.request.as_ref().clone(),
            ],
        )
        .await
        .unwrap();
    let saving = Saving {
        state: state.clone(),
        state_id: receipts.state_id().await.unwrap(),
        producer: store::Producer::current().name,
    };
    let mut recorder = Recorder::new(
        Some(&saving),
        &view.request,
        &"fixture-session".parse().unwrap(),
    );
    recorder.start(Header::default());
    for n in 0..80 {
        recorder.event(Kind::Answer(Answer::Completed(format!(
            "initial row {n} 한글 e\u{301}"
        ))));
    }
    recorder.event(Kind::Answer(Answer::Completed(SEED.into())));
    std::sync::Arc::make_mut(&mut view.request).session = recorder.reference().cloned();
    receipts.save_request(&view.request).await.unwrap();
    drop(receipts);

    let state_argument = format!("{STATE_ARGUMENT}{}", state.to_str().unwrap());
    // The peek fits one short transcript line; the opened pane has room for evidence below it.
    let (mut terminal, output) = PseudoTerminal::start(
        &crate::platform::current_executable().unwrap(),
        &["--exact", TEST, "--nocapture", &state_argument],
        &[],
        180,
        40,
    );
    wait_for_frame(&output, "fixture-run");
    terminal.type_text(b"\r");
    wait_for_frame(&output, SEED);
    terminal.type_text(b"\r");
    wait_for_frame(&output, "[Bottom]");

    recorder.event(Kind::Delivery(Delivery {
        turn: 1,
        attempt: 1,
        block: DeliveryBlock::text(0),
        kind: DeliveryKind::Text,
        text: DELIVERY.into(),
        state: DeliveryState::Delta,
    }));
    crate::changes::drain().await;
    wait_for_frame(&output, DELIVERY);

    // Leave the Agent modal, select the next eval, and wait for the separate Runtime evidence.
    terminal.type_text(LEFT);
    wait_for_frame(&output, "Enter: open");
    terminal.type_text(b"j\r");
    wait_for_frame(&output, RUNTIME);
    recorder.event(Kind::Answer(Answer::Completed(APPEND.into())));
    crate::changes::drain().await;
    terminal.type_text(LEFT);
    wait_for_frame(&output, "Enter: open");
    terminal.type_text(b"k\r");
    wait_for_frame(&output, APPEND);
    terminal.type_text(b"q");
    let mut rest = Vec::new();
    assert!(
        terminal.finish_and_drain(output, &mut rest),
        "{}",
        String::from_utf8_lossy(&rest)
    );
    assert!(
        crate::agent::session::path(&saving.state, &"fixture-session".parse().unwrap()).exists()
    );
}

fn wait_for_frame(output: &Receiver<Vec<u8>>, expected: &str) {
    let deadline = Instant::now() + test_os::patience(FRAME_DEADLINE);
    let mut frame = Vec::new();
    while !frame
        .windows(expected.len())
        .any(|bytes| bytes == expected.as_bytes())
    {
        let chunk = output
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|error| {
                panic!(
                    "waiting for {expected:?}: {error}; terminal output: {:?}",
                    String::from_utf8_lossy(&frame)
                )
            });
        frame.extend(chunk);
    }
}
