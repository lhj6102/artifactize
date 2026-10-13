//! The frozen old envelope and HTTP checks define status/message precedence independently.
use super::*;

#[test]
fn typed_keys_keep_previous_http_status_messages_for_all_shapes_and_validation_orders() {
    let hash = "a".repeat(64);
    let many = vec!["malformed"; MAX_LOOKUP_KEYS + 1];
    let shapes = [
        json!({"keys":[hash.clone(),hash.clone()]}),
        json!({"keys":many}),
        json!({"keys":["bad",{}]}),
        json!({"keys":[null,"bad",{}],"extra":true}),
        json!({"keys":["bad"]}),
        json!({"keys":["A".repeat(64)]}),
        json!({"keys":["g".repeat(64)]}),
        json!({"keys":["a".repeat(63)]}),
        json!({"keys":["a".repeat(65)]}),
        json!({"keys":[null]}),
        json!({"keys":[1]}),
        json!({"keys":[false]}),
        json!({"keys":[[hash.clone()]]}),
        json!({"keys":null}),
        json!({}),
        json!([["bad"]]),
        json!([[{}]]),
        json!([[hash.clone()], []]),
        json!({"keys":[{}],"extra":1}),
        json!({"keys":[hash.clone()],"extra":1}),
    ];
    let mut fixtures = shapes
        .into_iter()
        .map(|shape| shape.to_string())
        .collect::<Vec<_>>();
    fixtures.extend([
        format!(r#"{{"keys":[{{}}],"keys":["{hash}"]}}"#),
        format!(r#"{{"keys":["{hash}"],"keys":[{{}}]}}"#),
        r#"{"keys":[{}],"keys":[null]}"#.into(),
        r#"{"keys":null,"keys":["bad"]}"#.into(),
        r#"{"keys":["bad"],"keys":null}"#.into(),
        r#"{"keys":[{}],"extra":1e400}"#.into(),
        "null".into(),
        "17".into(),
        "true".into(),
        "\"string\"".into(),
        r#"{"keys":["bad",]}"#.into(),
    ]);
    let mut over_count_legacy = vec![json!("bad"); MAX_LOOKUP_KEYS + 1];
    over_count_legacy.push(json!({}));
    fixtures.push(json!({"keys":over_count_legacy}).to_string());
    let mut over_count_invalid = vec![json!("bad"); MAX_LOOKUP_KEYS + 1];
    over_count_invalid.push(Value::Null);
    fixtures.push(json!({"keys":over_count_invalid}).to_string());
    for body in fixtures {
        let expected = super::lookup_tests::previous_lookup(body.as_bytes());
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
fn valid_duplicate_keys_and_max_count_remain_typed_without_deduplication() {
    let hash = "a".repeat(64);
    let keys = vec![hash.clone(); MAX_LOOKUP_KEYS];
    let lookup = parse_lookup(json!({"keys":keys}).to_string().as_bytes())
        .ok()
        .unwrap();
    assert_eq!(lookup.keys.len(), MAX_LOOKUP_KEYS);
    assert!(lookup.keys.iter().all(|key| key.as_str() == hash));
}
