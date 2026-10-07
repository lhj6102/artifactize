//! Opt-in fixture driven by the scratch PTY harness, never a model or default state.
use super::*;
use crate::agent::session::{self as saved, Answer, Header, Kind, Recorder, Saving};
use serde_json::json;
use std::fs;

#[tokio::test]
#[ignore = "requires the external fixture PTY harness"]
async fn live_session_pty_fixture() {
    let Some(path) = std::env::var_os("ARTIFACTIZE_LIVE_PTY_FIXTURE") else {
        return;
    };
    let root = std::path::PathBuf::from(path);
    let repo = root.join("repo");
    let state = root.join("state");
    fs::create_dir_all(&repo).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    let run: store::Run = serde_json::from_value(json!({"id":"fixture-run","repoPath":repo,"stateDir":state,"status":"RUNNING","createdAt":"2026-01-01T00:00:00Z","selection":{"kind":"all"},"validation":null,"definitions":{"artifacts":{"app":{"path":""}},"evals":[{"id":"app/check","target":"app","deps":[],"declaration":{"title":"Fixture Agent","profile":{"kind":"agent","backend":"openai","model":"fixture"},"payload":{"instruction":"fixture"}}},{"id":"app/runtime","target":"app","deps":[],"declaration":{"title":"Fixture Runtime","profile":{"kind":"runtime","command":"true","args":[]},"payload":{"instruction":"fixture"}}}]}})).unwrap();
    let mut view = tests::request(
        "app/check",
        "RUNNING",
        json!({"profile":{"kind":"agent","backend":"openai","model":"fixture"},"sessionId":"fixture-session"}),
    );
    view.request.run_id = run.id.clone();
    let mut runtime = tests::request(
        "app/runtime",
        "GREEN",
        json!({"completedAt":"2026-01-01T00:01:00Z","result":{"verdict":"GREEN","exitCode":0,"stdout":"other modal unaffected","stderr":""}}),
    );
    runtime.request.run_id = run.id.clone();
    receipts
        .create_run(&run, &[view.request.clone(), runtime.request.clone()])
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
        recorder.event(Kind::Answer(Answer {
            text: Some(format!("initial row {n} 한글 e\u{301}")),
            ..Answer::default()
        }));
    }
    view.request.session = recorder.reference().cloned();
    receipts.save_request(&view.request).await.unwrap();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let control = root.clone();
    let token = cancellation.clone();
    let writer = tokio::spawn(async move {
        fs::write(control.join("ready"), "ready").unwrap();
        loop {
            if control.join("quit").exists() {
                token.cancel();
                break;
            }
            if let Ok(text) = fs::read_to_string(control.join("append")) {
                let _ = fs::remove_file(control.join("append"));
                recorder.event(Kind::Answer(Answer {
                    text: Some(text),
                    ..Answer::default()
                }));
                crate::changes::drain().await;
                fs::write(control.join("appended"), "flushed and notified").unwrap();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    });
    terminal::run(state, None, cancellation).await.unwrap();
    fs::write(root.join("quit"), "quit").unwrap();
    writer.await.unwrap();
    assert!(
        saved::path(&saving.state, "fixture-session")
            .unwrap()
            .exists()
    );
}
