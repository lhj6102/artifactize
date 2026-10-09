use super::*;

#[test]
fn public_summary_allowlist_survives_every_chunk_split_utf8_crlf_and_multiline_sse() {
    let frame = "\u{feff}event: response.reasoning_summary_text.delta\r\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\r\ndata: \"item_id\":\"rs\",\"output_index\":0,\"summary_index\":1,\"delta\":\"공개 요약\"}\r\n\r\n";
    for split in 0..=frame.len() {
        let sink = Sink::default();
        let mut observer = Observer::default();
        observer.push(&frame.as_bytes()[..split], &sink);
        observer.push(&frame.as_bytes()[split..], &sink);
        let events = sink.take(2, 3);
        assert_eq!(events.len(), 1, "split {split}");
        assert_eq!(events[0].text, "공개 요약");
        assert!(events[0].block.ends_with("-1"));
    }
    let sink = Sink::default();
    let mut observer = Observer::default();
    observer.push(
        b"data: {\"type\":\"response.reasoning_text.delta\",\"delta\":\"private\"}\n\ndata: {\"type\":\"unknown\",\"encrypted_content\":\"private\"}\n\n",
        &sink,
    );
    assert!(sink.take(1, 1).is_empty());
}

#[tokio::test]
async fn task_local_sinks_are_isolated_and_overflow_only_degrades_observer() {
    let first = Sink::default();
    let second = Sink::default();
    tokio::join!(
        first.scope(async {
            ACTIVE.with(|sink| sink.push(DeliveryKind::Text, "one".into(), "first"));
            tokio::task::yield_now().await;
        }),
        second.scope(async {
            ACTIVE.with(|sink| sink.push(DeliveryKind::Text, "two".into(), "second"));
            tokio::task::yield_now().await;
        })
    );
    assert_eq!(first.take(1, 1)[0].text, "first");
    assert_eq!(second.take(1, 1)[0].text, "second");
    let sink = Sink::default();
    let mut observer = Observer::default();
    observer.push(&vec![b'x'; FRAME_BYTES + FRAME_SLICE], &sink);
    observer.push(
        b"\n\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"later\",\"summary_index\":0,\"delta\":\"recovered\"}\n\n",
        &sink,
    );
    assert_eq!(sink.take(1, 1)[0].text, "recovered");
}
