#[path = "support/os.rs"]
mod os;

use artifactize_tools::{Content, ToolResult, result};
use serde_json::json;

#[test]
fn saved_results_keep_the_exact_wire_bytes_and_inline_images() {
    // Images in saved results are already canonicalized, not file references.
    // Replay must not revalidate them or apply stdout's byte-size budgets.
    for saved in [
        r#"{"content":[{"type":"text","text":""}]}"#,
        r#"{"content":[{"type":"text","text":"hello"},{"type":"json","data":{"answer":42}}]}"#,
        r#"{"content":[{"type":"image","data":"saved-inline-bytes","mimeType":"image/png"}]}"#,
        r#"{"content":[{"type":"text","text":" failure\n"}],"isError":true}"#,
    ] {
        let result: ToolResult = serde_json::from_str(saved).unwrap();
        assert_eq!(serde_json::to_string(&result).unwrap(), saved);
    }
    let explicit_success: ToolResult = serde_json::from_value(json!({
        "content":[{"type":"text","text":"ok"}], "isError":false,
    }))
    .unwrap();
    assert!(!explicit_success.is_error());
    assert_eq!(
        serde_json::to_string(&explicit_success).unwrap(),
        r#"{"content":[{"type":"text","text":"ok"}]}"#,
    );
}

#[test]
fn contradictory_saved_results_are_rejected_at_deserialization() {
    for (content, diagnostic) in [
        (json!([]), "exactly one text block"),
        (json!([{"type":"json","data":42}]), "exactly one text block"),
        (
            json!([{"type":"image","data":"bytes","mimeType":"image/png"}]),
            "exactly one text block",
        ),
        (
            json!([{"type":"text","text":"a"},{"type":"text","text":"b"}]),
            "exactly one text block",
        ),
        (json!([{"type":"text","text":" \n\t"}]), "must not be blank"),
    ] {
        let error = serde_json::from_value::<ToolResult>(json!({"content":content,"isError":true}))
            .unwrap_err();
        assert!(error.to_string().contains(diagnostic), "{error}");
    }
    for content in [
        json!([]),
        json!(vec![json!({"type":"text","text":"x"}); 33]),
    ] {
        assert!(serde_json::from_value::<ToolResult>(json!({"content":content})).is_err());
    }
}

#[test]
fn constructors_guard_content_counts_and_replace_blank_diagnostics() {
    assert!(ToolResult::try_success(vec![]).is_err());
    assert!(ToolResult::try_success(vec![Content::Text { text: "x".into() }; 33]).is_err());
    let boundary = ToolResult::try_success(vec![Content::Text { text: "".into() }; 32]).unwrap();
    assert_eq!(boundary.content().len(), 32);
    assert!(!boundary.is_error());
    for message in ["", " \n\t"] {
        assert_eq!(
            serde_json::to_string(&ToolResult::error(message)).unwrap(),
            r#"{"content":[{"type":"text","text":"Tool failed."}],"isError":true}"#,
        );
    }
}

#[test]
fn command_output_and_saved_results_share_shape_validation() {
    let directory = os::tempdir();
    for wire in [
        json!({"content":[{"type":"text","text":"ok"}]}),
        json!({"content":[{"type":"text","text":"failure"}],"isError":true}),
        json!({"content":[],"isError":true}),
        json!({"content":[{"type":"json","data":42}],"isError":true}),
        json!({"content":[{"type":"text","text":" "}],"isError":true}),
    ] {
        let bytes = serde_json::to_vec(&wire).unwrap();
        let parsed = result::parse(&bytes, directory.path());
        let saved = serde_json::from_slice::<ToolResult>(&bytes);
        assert_eq!(parsed.is_ok(), saved.is_ok(), "{wire}");
        if let (Ok(parsed), Ok(saved)) = (parsed, saved) {
            assert_eq!(parsed, saved);
        }
    }
}
