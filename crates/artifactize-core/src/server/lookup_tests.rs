use super::*;

#[test]
fn lookup_positional_and_numeric_edges_keep_literal_http_results() {
    let hash = "a".repeat(64);
    for (body, keys) in [
        ("[[]]".to_owned(), Vec::<String>::new()),
        (format!("[[\"{hash}\"]]"), vec![hash]),
    ] {
        assert_eq!(
            parse_lookup(body.as_bytes())
                .ok()
                .unwrap()
                .keys
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>(),
            keys,
            "{body}"
        );
    }
    for body in [
        "[[{}]]",
        "[]",
        "[[],[]]",
        "[null]",
        r#"{"keys":[{}],"extra":1e400}"#,
        r#"{"keys":[{}],"extra":{"nested":[1e400]}}"#,
        r#"{"keys":[{}],"extra":1e400,"extra":null}"#,
    ] {
        let error = parse_lookup(body.as_bytes()).err().unwrap();
        assert_eq!(error.0, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(error.1, "Expected {\"keys\":[KEY,...]}.", "{body}");
    }
}

#[test]
fn lookup_wire_shapes_keep_legacy_priority_and_current_unknown_field_policy() {
    let hash = "a".repeat(64);
    for body in [
        r#"{"keys":[]}"#.to_owned(),
        format!(r#"{{"keys":["{hash}"]}}"#),
        format!(r#"{{"keys":17,"keys":["{hash}"]}}"#),
        r#"[[]]"#.to_owned(),
        format!(r#"[["{hash}"]]"#),
    ] {
        assert!(parse_lookup(body.as_bytes()).is_ok(), "{body}");
    }
    for body in [
        r#"{"keys":[{}]}"#,
        r#"{"keys":[null,{},"a"],"extra":true}"#,
        r#"{"keys":[{"unexpected":17}]}"#,
    ] {
        let error = parse_lookup(body.as_bytes()).err().unwrap();
        assert_eq!(error.0, StatusCode::GONE, "{body}");
        assert_eq!(
            error.1,
            "This review store holds artifactize 0.5 reuse keys (store schema 3); upgrade this client to artifactize 0.5 or later."
        );
    }
    for body in [
        r#"{"keys":["a"],"extra":true}"#,
        r#"{"keys":[1]}"#,
        r#"{"keys":null}"#,
        r#"{"keys":[["a"]]}"#,
        r#"[[{}]]"#,
        r#"[]"#,
        r#"[[],[]]"#,
        r#"{"keys":[{}],"keys":[1]}"#,
        r#"{"keys":[{}],"extra":1e400}"#,
        r#"{"keys":[{}],"extra":{"nested":[1e400]}}"#,
        "not json",
        r#"{"keys":[{},]}"#,
        r#"{"keys":[],"extra":18446744073709551616}"#,
    ] {
        let error = parse_lookup(body.as_bytes()).err().unwrap();
        assert_eq!(error.0, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(error.1, "Expected {\"keys\":[KEY,...]}.");
    }
}
