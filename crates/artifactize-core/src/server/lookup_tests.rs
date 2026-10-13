use super::*;

/// Frozen pre-refactor parser used only as an acceptance/error oracle for fake wire fixtures.
pub(super) fn previous_lookup(body: &[u8]) -> Result<Vec<String>, (StatusCode, String)> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct PreviousLookup {
        keys: Vec<String>,
    }
    let expected = || {
        (
            StatusCode::BAD_REQUEST,
            "Expected {\"keys\":[KEY,...]}.".to_owned(),
        )
    };
    let lookup: Value = serde_json::from_slice(body).map_err(|_| expected())?;
    if lookup["keys"]
        .as_array()
        .is_some_and(|keys| keys.iter().any(Value::is_object))
    {
        return Err((StatusCode::GONE, UPGRADE.to_owned()));
    }
    let lookup: PreviousLookup = serde_json::from_value(lookup).map_err(|_| expected())?;
    // Preserve the pre-typed HTTP handler's validation order after envelope parsing.
    if lookup.keys.len() > MAX_LOOKUP_KEYS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("At most {MAX_LOOKUP_KEYS} keys per lookup."),
        ));
    }
    for key in &lookup.keys {
        if key.len() != 64
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "Keys are lowercase SHA-256 reuse keys.".into(),
            ));
        }
    }
    Ok(lookup.keys)
}

#[test]
fn typed_lookup_matches_previous_parser_for_positional_and_numeric_edges() {
    let hash = "a".repeat(crate::types::SHA256_HEX_BYTES);
    let fixtures = [
        "[[]]".to_owned(),
        format!("[[\"{hash}\"]]"),
        "[[{}]]".to_owned(),
        "[]".to_owned(),
        "[[],[]]".to_owned(),
        "[null]".to_owned(),
        r#"{"keys":[{}],"extra":1e400}"#.to_owned(),
        r#"{"keys":[{}],"extra":{"nested":[1e400]}}"#.to_owned(),
        r#"{"keys":[],"extra":18446744073709551616}"#.to_owned(),
        r#"{"keys":[{}],"extra":1e400,"extra":null}"#.to_owned(),
    ];
    for body in fixtures {
        let expected = previous_lookup(body.as_bytes());
        let actual = parse_lookup(body.as_bytes())
            .map(|lookup| {
                lookup
                    .keys
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .map_err(|error| (error.0, error.1));
        assert_eq!(actual, expected, "{body}");
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
        assert_eq!(error.1, UPGRADE);
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
    ] {
        let error = parse_lookup(body.as_bytes()).err().unwrap();
        assert_eq!(error.0, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(error.1, "Expected {\"keys\":[KEY,...]}.");
    }
}
