//! Minimal behavior checks, not visual snapshots or spacing/color contracts.
use super::{
    Event, Kind, MessageEvent,
    transcript::{BlockId, Transcript},
};
use rig_core::message::{
    AssistantContent, Message, ToolCall, ToolFunction, ToolName, ToolResultContent,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn event(message: Message, failed: Vec<bool>) -> Event {
    Event {
        at: None,
        send: None,
        kind: Kind::Message(MessageEvent {
            turn: 1,
            message,
            repair: false,
            is_error: failed,
            question: None,
        }),
    }
}
fn call(id: &str, path: &str) -> ToolCall {
    ToolCall::from_wire(
        id,
        ToolFunction::new(
            ToolName::new("read_cli").unwrap(),
            json!({"path":path,"private":"ARGUMENT_SECRET"}),
        ),
    )
}
fn apply(transcript: &mut Transcript, blocks: &mut BTreeMap<BlockId, String>, event: &Event) {
    for patch in transcript.apply(event) {
        blocks.insert(patch.id, patch.text);
    }
}
fn shown(blocks: &BTreeMap<BlockId, String>) -> String {
    blocks.values().cloned().collect()
}

#[test]
fn prose_real_newlines_markdown_and_verdict_are_readable_without_envelope_or_media() {
    let mut transcript = Transcript::default();
    let mut blocks = BTreeMap::new();
    let message:Message=serde_json::from_value(json!({"role":"user","content":[{"type":"text","text":"first\nsecond\n```rust\nlet x = 1;\n```"},{"type":"image","data":{"type":"base64","value":"OPAQUE_MEDIA_PAYLOAD"},"media_type":"png","detail":null}]})).unwrap();
    apply(&mut transcript, &mut blocks, &event(message, vec![]));
    apply(
        &mut transcript,
        &mut blocks,
        &Event {
            at: None,
            send: None,
            kind: Kind::End(super::End {
                result: Some(
                    json!({"verdict":"GREEN","summary":"# Complete\nWorks","custom":{"approved":true}}),
                ),
                ..super::End::default()
            }),
        },
    );
    let text = shown(&blocks);
    assert!(text.contains("first\nsecond"));
    assert!(text.contains("let x = 1;"));
    assert!(text.contains("Verdict: GREEN"));
    assert!(text.contains("approved: true"));
    for hidden in [
        "OPAQUE_MEDIA_PAYLOAD",
        "\"role\"",
        "\"content\"",
        "Turn 1",
        "\\nsecond",
    ] {
        assert!(!text.contains(hidden), "{hidden}: {text}");
    }
}

#[test]
fn final_json_is_kept_until_matching_end_then_owner_failure_fields_remain_readable() {
    let result = json!({"verdict":"RED","mismatches":[{"path":"src/a.rs","reason":"wrong behavior"}],"custom":{"expected":"value"}});
    let mut transcript = Transcript::default();
    let mut blocks = BTreeMap::new();
    apply(
        &mut transcript,
        &mut blocks,
        &event(Message::assistant(result.to_string()), vec![]),
    );
    assert!(
        shown(&blocks).contains("\"verdict\""),
        "without authoritative End the text is untouched"
    );
    apply(
        &mut transcript,
        &mut blocks,
        &Event {
            at: None,
            send: None,
            kind: Kind::End(super::End {
                result: Some(result),
                ..super::End::default()
            }),
        },
    );
    let text = shown(&blocks);
    assert!(
        text.contains("Verdict: RED")
            && text.contains("wrong behavior")
            && text.contains("expected: value")
    );
    assert!(!text.contains("\"verdict\""));
    let mut transcript = Transcript::default();
    let mut blocks = BTreeMap::new();
    apply(
        &mut transcript,
        &mut blocks,
        &event(
            Message::assistant("Example:\n```json\n{\"verdict\":\"RED\"}\n```"),
            vec![],
        ),
    );
    apply(
        &mut transcript,
        &mut blocks,
        &Event {
            at: None,
            send: None,
            kind: Kind::End(super::End {
                result: Some(json!({"verdict":"GREEN"})),
                ..super::End::default()
            }),
        },
    );
    assert!(shown(&blocks).contains("Example:") && shown(&blocks).contains("\"verdict\":\"RED\""));
}

#[test]
fn recorded_tool_ids_link_results_adjacent_activity_groups_and_prose_splits_them() {
    let mut transcript = Transcript::default();
    let mut blocks = BTreeMap::new();
    let first = call("first", "src/a.rs");
    let second = call("second", "src/b.rs");
    apply(
        &mut transcript,
        &mut blocks,
        &event(
            Message::Assistant {
                id: None,
                content: vec![
                    AssistantContent::ToolCall(first.clone()),
                    AssistantContent::ToolCall(second.clone()),
                ],
            },
            vec![],
        ),
    );
    let group = *blocks.keys().next().unwrap();
    apply(
        &mut transcript,
        &mut blocks,
        &event(
            Message::tool_results(vec![
                second.result(vec![ToolResultContent::text("SUCCESS_BLOB".repeat(10000))]),
                first.result(vec![ToolResultContent::text("permission denied")]),
            ]),
            vec![false, true],
        ),
    );
    let text = shown(&blocks);
    assert!(!text.contains("SUCCESS_BLOB") && !text.contains("ARGUMENT_SECRET"));
    assert!(text.contains("permission denied") && text.contains("src/a.rs"));
    for patch in transcript.expansion(&BTreeSet::from([group])) {
        blocks.insert(patch.id, patch.text);
    }
    assert!(shown(&blocks).contains("src/b.rs") && shown(&blocks).contains("done"));
    apply(
        &mut transcript,
        &mut blocks,
        &event(Message::assistant("Checking the next file."), vec![]),
    );
    apply(
        &mut transcript,
        &mut blocks,
        &event(
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(call("third", "src/c.rs"))],
            },
            vec![],
        ),
    );
    assert_eq!(
        blocks.keys().filter(|id| transcript.is_group(**id)).count(),
        2
    );
    assert!(
        blocks[&group].contains("src/b.rs"),
        "expanded group stayed expanded"
    );
}

#[test]
fn expanded_group_survives_late_result_and_orphan_status_is_honest() {
    let mut transcript = Transcript::default();
    let mut blocks = BTreeMap::new();
    let first = call("first", "src/visible.rs");
    apply(
        &mut transcript,
        &mut blocks,
        &event(
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::ToolCall(first.clone())],
            },
            vec![],
        ),
    );
    let id = *blocks.keys().next().unwrap();
    for patch in transcript.expansion(&BTreeSet::from([id])) {
        blocks.insert(patch.id, patch.text);
    }
    apply(
        &mut transcript,
        &mut blocks,
        &event(
            Message::tool_results(vec![
                first.result(vec![ToolResultContent::text("hidden success")]),
            ]),
            vec![false],
        ),
    );
    assert!(blocks[&id].contains("src/visible.rs") && blocks[&id].contains("done"));
    let orphan = call("unknown", "not guessed");
    apply(
        &mut transcript,
        &mut blocks,
        &event(
            Message::tool_results(vec![orphan.result(vec![ToolResultContent::text(
                "error-looking success must not imply failure",
            )])]),
            vec![],
        ),
    );
    assert!(
        blocks[&id].contains("unmatched recorded result")
            && blocks[&id].contains("status not recorded")
    );
    assert!(!blocks[&id].contains("error-looking success"));
}
