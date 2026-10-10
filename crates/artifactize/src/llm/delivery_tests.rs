//! Offline staged SSE exercises delivered data before part/turn completion and keeps replay safe.
use super::*;
use crate::agent::session::{self, DeliveryKind, DeliveryState, Header, Kind, Recorder};
use rig_core::{completion::CompletionRequest, message::Message, providers::openai::OpenAIConfig};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
};

fn frame(mut value: serde_json::Value, sequence: usize) -> String {
    value["sequence_number"] = json!(sequence);
    format!(
        "event: {}\r\ndata: {value}\r\n\r\n",
        value["type"].as_str().unwrap()
    )
}
/// Bound fixture input so a malformed request cannot grow an unbounded test buffer.
const REQUEST_BYTES: usize = 1024 * 1024;

/// Read the complete fake request before answering; TCP may split both headers and body.
async fn read_request(socket: &mut tokio::net::TcpStream) {
    tokio::time::timeout(Duration::from_secs(5), read_request_bytes(socket))
        .await
        .expect("fixture request read timed out");
}
async fn read_request_bytes(socket: &mut tokio::net::TcpStream) {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 4096];
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0, "fixture request ended before its headers");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() <= REQUEST_BYTES, "fixture headers exceed bound");
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
    let length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
        .unwrap_or(0);
    let request_end = header_end
        .checked_add(length)
        .filter(|end| *end <= REQUEST_BYTES)
        .expect("fixture request length within bound");
    while bytes.len() < request_end {
        let mut chunk = [0; 4096];
        let count = socket.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0, "fixture request ended before its body");
        bytes.extend_from_slice(&chunk[..count]);
    }
}

#[tokio::test]
async fn public_summary_and_text_arrive_before_end_and_private_reasoning_never_enters_delivery() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (release_tx, release_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let early = [
            json!({
                "type":"response.output_item.added",
                "output_index":0,
                "item":{"type":"reasoning","id":"rs_public","summary":[]},
            }),
            json!({
                "type":"response.reasoning_summary_text.delta",
                "item_id":"rs_public",
                "output_index":0,
                "summary_index":0,
                "delta":"Public first ",
            }),
            json!({
                "type":"response.reasoning_text.delta",
                "item_id":"rs_public",
                "output_index":0,
                "delta":"PRIVATE_RAW_REASONING",
            }),
            json!({
                "type":"response.reasoning_summary_text.delta",
                "item_id":"rs_public",
                "output_index":0,
                "summary_index":0,
                "delta":"public second",
            }),
            json!({
                "type":"response.output_item.added",
                "output_index":1,
                "item":{
                    "type":"message",
                    "id":"msg_text",
                    "role":"assistant",
                    "status":"in_progress",
                    "content":[],
                },
            }),
            json!({
                "type":"response.output_text.delta",
                "item_id":"msg_text",
                "output_index":1,
                "content_index":0,
                "delta":"Visible first ",
            }),
            json!({
                "type":"response.output_text.delta",
                "item_id":"msg_text",
                "output_index":1,
                "content_index":0,
                "delta":"visible second",
            }),
        ];
        for (index, event) in early.into_iter().enumerate() {
            let bytes = frame(event, index).into_bytes();
            let split = bytes.len() / 2;
            socket.write_all(&bytes[..split]).await.unwrap();
            socket.write_all(&bytes[split..]).await.unwrap();
        }
        release_rx.await.unwrap();
        let response = json!({
            "id":"resp_fixture",
            "object":"response",
            "created_at":1,
            "model":"fixture-model",
            "status":"completed",
            "output":[
                {
                    "type":"reasoning",
                    "id":"rs_public",
                    "summary":[{"type":"summary_text","text":"Public first public second"}],
                    "encrypted_content":"PRIVATE_ENCRYPTED_PAYLOAD",
                },
                {
                    "type":"message",
                    "id":"msg_text",
                    "role":"assistant",
                    "status":"completed",
                    "content":[
                        {
                            "type":"output_text",
                            "text":"Visible first visible second",
                            "annotations":[],
                        },
                    ],
                },
            ],
            "usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},
        });
        socket
            .write_all(
                frame(json!({"type":"response.completed","response":response}), 7).as_bytes(),
            )
            .await
            .unwrap();
    });
    let client = Client::Openai(Box::new(
        OpenAIConfig::new("fixture-key")
            .with_base_url(format!("http://{address}/v1"))
            .connect(delivery::Tap::new(http_client().unwrap()))
            .responses("fixture-model"),
    ));
    let root = crate::test_os::tempdir();
    let mut recorder = Recorder::test(root.path(), "stream-fixture");
    recorder.start(Header::default());
    let cancellation = CancellationToken::new();
    let mut attempts = Vec::new();
    let mut release = Some(release_tx);
    let mut early_observed = false;
    let mut received = Vec::new();
    let request = CompletionRequest::new(Message::user("fixture"));
    let response = client
        .turn_observed(
            &request,
            Turn {
                number: 1,
                deadline: Instant::now() + Duration::from_secs(5),
                cancellation: &cancellation,
            },
            &mut attempts,
            &mut |event| {
                received.push(event.clone());
                recorder.event(Kind::Delivery(event));
                let summary = received
                    .iter()
                    .filter(|event| event.kind == DeliveryKind::Summary)
                    .map(|event| event.text.as_str())
                    .collect::<String>();
                let text = received
                    .iter()
                    .filter(|event| event.kind == DeliveryKind::Text)
                    .map(|event| event.text.as_str())
                    .collect::<String>();
                if summary.contains("Public first public second")
                    && text.contains("Visible first visible second")
                    && release.is_some()
                {
                    let saved = session::Conversation::load(
                        &session::path(root.path(), "stream-fixture").unwrap(),
                    )
                    .unwrap()
                    .unwrap();
                    assert!(
                        saved
                            .events
                            .iter()
                            .any(|event| matches!(event.kind, Kind::Delivery(_)))
                    );
                    assert!(saved.history().unwrap().is_empty());
                    early_observed = true;
                    release.take().unwrap().send(()).unwrap();
                }
            },
        )
        .await
        .unwrap();
    assert!(early_observed);
    assert_eq!(
        received[0].kind,
        DeliveryKind::Summary,
        "public summary delivered before later text"
    );
    assert_eq!(attempts.len(), 1);
    assert_eq!(
        response
            .choice
            .iter()
            .filter(|part| matches!(part, rig_core::message::AssistantContent::Text(_)))
            .count(),
        1
    );
    assert!(
        received
            .iter()
            .all(|event| !event.text.contains("PRIVATE_RAW")
                && !event.text.contains("PRIVATE_ENCRYPTED"))
    );
    let mut transcript = session::transcript::Transcript::default();
    let mut blocks = std::collections::BTreeMap::new();
    for delivery in received {
        for patch in transcript.apply(&session::Event {
            at: None,
            send: None,
            kind: Kind::Delivery(delivery),
        }) {
            blocks.insert(patch.id, patch.text);
        }
    }
    for patch in transcript.apply(&session::Event {
        at: None,
        send: None,
        kind: Kind::Message(session::MessageEvent {
            turn: 1,
            message: Message::Assistant {
                id: response.message_id,
                content: response.choice,
            },
            repair: false,
            is_error: vec![],
            question: None,
        }),
    }) {
        blocks.insert(patch.id, patch.text);
    }
    let shown = blocks.values().cloned().collect::<String>();
    assert_eq!(shown.matches("Visible first visible second").count(), 1);
    assert_eq!(shown.matches("Public first public second").count(), 1);
    server.await.unwrap();
}

#[tokio::test]
async fn cancelled_text_keeps_partial_and_does_not_retry() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop_tx, stop_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        socket
            .write_all(
                frame(
                    json!({
                        "type":"response.output_text.delta",
                        "item_id":"message",
                        "output_index":0,
                        "content_index":0,
                        "delta":"partial visible",
                    }),
                    0,
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let _ = stop_rx.await;
    });
    let client = Client::Openai(Box::new(
        OpenAIConfig::new("fixture")
            .with_base_url(format!("http://{address}/v1"))
            .connect(delivery::Tap::new(http_client().unwrap()))
            .responses("fixture-model"),
    ));
    let cancel = CancellationToken::new();
    let mut attempts = Vec::new();
    let mut deliveries = Vec::new();
    let error = client
        .turn_observed(
            &CompletionRequest::new(Message::user("fixture")),
            Turn {
                number: 1,
                deadline: Instant::now() + Duration::from_secs(5),
                cancellation: &cancel,
            },
            &mut attempts,
            &mut |event| {
                if event.text.contains("partial visible") {
                    cancel.cancel();
                }
                deliveries.push(event);
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, Code::Cancelled);
    assert_eq!(attempts.len(), 1);
    assert!(
        deliveries
            .iter()
            .any(|event| event.state == DeliveryState::Interrupted)
    );
    let _ = stop_tx.send(());
    server.await.unwrap();
}
