//! Literal HTTP outcomes pin lookup shape and validation precedence without a second parser.
use super::*;

/// The HTTP contract accepts 1000 keys, pinned independently of the parser limit.
const HTTP_KEY_LIMIT: usize = 1000;

#[test]
fn typed_keys_keep_http_status_messages_for_all_shapes_and_validation_orders() {
    let hash = "a".repeat(64);
    let many = vec!["malformed"; HTTP_KEY_LIMIT + 1];
    let shape_error = "Expected {\"keys\":[KEY,...]}.";
    let key_error = "Keys are lowercase SHA-256 reuse keys.";
    let upgrade = "This review store holds artifactize 0.5 reuse keys (store schema 3); upgrade this client to artifactize 0.5 or later.";
    let shapes = [
        (
            json!({"keys":many}),
            StatusCode::BAD_REQUEST,
            "At most 1000 keys per lookup.",
        ),
        (json!({"keys":["bad",{}]}), StatusCode::GONE, upgrade),
        (
            json!({"keys":[null,"bad",{}],"extra":true}),
            StatusCode::GONE,
            upgrade,
        ),
        (json!({"keys":["bad"]}), StatusCode::BAD_REQUEST, key_error),
        (
            json!({"keys":["A".repeat(64)]}),
            StatusCode::BAD_REQUEST,
            key_error,
        ),
        (
            json!({"keys":["g".repeat(64)]}),
            StatusCode::BAD_REQUEST,
            key_error,
        ),
        (
            json!({"keys":["a".repeat(63)]}),
            StatusCode::BAD_REQUEST,
            key_error,
        ),
        (
            json!({"keys":["a".repeat(65)]}),
            StatusCode::BAD_REQUEST,
            key_error,
        ),
        (json!({"keys":[null]}), StatusCode::BAD_REQUEST, shape_error),
        (json!({"keys":[1]}), StatusCode::BAD_REQUEST, shape_error),
        (
            json!({"keys":[false]}),
            StatusCode::BAD_REQUEST,
            shape_error,
        ),
        (
            json!({"keys":[[hash.clone()]]}),
            StatusCode::BAD_REQUEST,
            shape_error,
        ),
        (json!({"keys":null}), StatusCode::BAD_REQUEST, shape_error),
        (json!({}), StatusCode::BAD_REQUEST, shape_error),
        (json!([["bad"]]), StatusCode::BAD_REQUEST, key_error),
        (json!([[{}]]), StatusCode::BAD_REQUEST, shape_error),
        (
            json!([[hash.clone()], []]),
            StatusCode::BAD_REQUEST,
            shape_error,
        ),
        (json!({"keys":[{}],"extra":1}), StatusCode::GONE, upgrade),
        (
            json!({"keys":[hash.clone()],"extra":1}),
            StatusCode::BAD_REQUEST,
            shape_error,
        ),
    ];
    let mut fixtures = shapes
        .into_iter()
        .map(|(shape, status, message)| (shape.to_string(), status, message))
        .collect::<Vec<_>>();
    fixtures.extend([
        (
            format!(r#"{{"keys":["{hash}"],"keys":[{{}}]}}"#),
            StatusCode::GONE,
            upgrade,
        ),
        (
            r#"{"keys":[{}],"keys":[null]}"#.into(),
            StatusCode::BAD_REQUEST,
            shape_error,
        ),
        (
            r#"{"keys":null,"keys":["bad"]}"#.into(),
            StatusCode::BAD_REQUEST,
            key_error,
        ),
        (
            r#"{"keys":["bad"],"keys":null}"#.into(),
            StatusCode::BAD_REQUEST,
            shape_error,
        ),
        (
            r#"{"keys":[{}],"extra":1e400}"#.into(),
            StatusCode::BAD_REQUEST,
            shape_error,
        ),
        ("null".into(), StatusCode::BAD_REQUEST, shape_error),
        ("17".into(), StatusCode::BAD_REQUEST, shape_error),
        ("true".into(), StatusCode::BAD_REQUEST, shape_error),
        ("\"string\"".into(), StatusCode::BAD_REQUEST, shape_error),
        (
            r#"{"keys":["bad",]}"#.into(),
            StatusCode::BAD_REQUEST,
            shape_error,
        ),
    ]);
    let mut over_count_legacy = vec![json!("bad"); HTTP_KEY_LIMIT + 1];
    over_count_legacy.push(json!({}));
    fixtures.push((
        json!({"keys":over_count_legacy}).to_string(),
        StatusCode::GONE,
        upgrade,
    ));
    let mut over_count_invalid = vec![json!("bad"); HTTP_KEY_LIMIT + 1];
    over_count_invalid.push(Value::Null);
    fixtures.push((
        json!({"keys":over_count_invalid}).to_string(),
        StatusCode::BAD_REQUEST,
        shape_error,
    ));
    for (body, status, message) in fixtures {
        let error = parse_lookup(body.as_bytes()).err().unwrap();
        assert_eq!(error.0, status, "{body}");
        assert_eq!(error.1, message, "{body}");
    }
    // Duplicate map fields keep the final field, including an earlier legacy object array.
    let body = format!(r#"{{"keys":[{{}}],"keys":["{hash}"]}}"#);
    let lookup = parse_lookup(body.as_bytes()).ok().unwrap();
    assert_eq!(
        lookup
            .keys
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
        [hash]
    );
}

#[test]
fn valid_duplicate_keys_and_max_count_remain_typed_without_deduplication() {
    let hash = "a".repeat(64);
    let keys = vec![hash.clone(); HTTP_KEY_LIMIT];
    let lookup = parse_lookup(json!({"keys":keys}).to_string().as_bytes())
        .ok()
        .unwrap();
    assert_eq!(lookup.keys.len(), HTTP_KEY_LIMIT);
    assert!(lookup.keys.iter().all(|key| key.as_str() == hash));
}
