use super::*;

#[test]
fn exact_object_and_declared_owner_fields_only() {
    let schema = VerdictSchema::new(None, None).unwrap();
    for invalid in [
        "",
        " ",
        "null",
        "[]",
        "{}",
        r#"{"verdict":"green"}"#,
        r#"{"verdict":"GREEN"} {}"#,
        "```json\n{\"verdict\":\"GREEN\"}\n```",
        r#"Verdict: {"verdict":"GREEN"}"#,
        r#"{"verdict":"RED","reason":"undeclared"}"#,
        r#"{"verdict":"GREEN","model":"forged"}"#,
    ] {
        assert!(schema.parse(invalid).is_err(), "{invalid}");
    }
    assert_eq!(
        schema.parse(" \n{\"verdict\":\"RED\"}\t").unwrap(),
        json!({"verdict":"RED"})
    );
}

#[test]
fn schema_uses_jsonschema_without_rewriting_owner_values() {
    let owner = json!({
        "type":"object",
        "$defs":{"nonempty":{"type":"string","minLength":1}},
        "properties":{"evidence":{"type":"array","items":{"anyOf":[{"$ref":"#/$defs/nonempty"},{"type":"integer","minimum":0}]},"minItems":1}},
        "required":["evidence"]
    });
    let schema = VerdictSchema::new(owner.as_object(), None).unwrap();
    let result = json!({"verdict":"GREEN","evidence":["  keep spacing  ",3]});
    assert_eq!(schema.parse(&result.to_string()).unwrap(), result);
    assert!(
        schema
            .parse(r#"{"verdict":"GREEN","evidence":[-1]}"#)
            .is_err()
    );
    assert!(
        schema
            .parse(r#"{"verdict":"RED","evidence":["x"]}"#)
            .is_err()
    );
    let pattern = json!({"type":"object","patternProperties":{".*":{} }});
    let schema = VerdictSchema::new(pattern.as_object(), None).unwrap();
    assert!(
        schema
            .parse(r#"{"verdict":"GREEN","provider":"forged"}"#)
            .is_err()
    );
}

#[test]
fn raw_and_normalized_size_limits_are_explicit_not_truncation() {
    let owner = json!({"properties":{"reason":{"type":"string"}}});
    let schema = VerdictSchema::new(owner.as_object(), None).unwrap();
    let prefix = r#"{"verdict":"GREEN","reason":""#;
    let text = format!(
        "{prefix}{}\"}}",
        "x".repeat(MAX_RESULT_CHARS - prefix.len() - 2)
    );
    assert!(schema.parse(&text).is_ok());
    let too_big = text.replacen('x', "xx", 1);
    assert!(schema.parse(&too_big).unwrap_err().contains("normalized"));
    assert!(
        schema
            .parse(&"x".repeat(MAX_RESPONSE_BYTES + 1))
            .unwrap_err()
            .contains("1 MiB")
    );
    let compact = r#"{"verdict":"GREEN"}"#;
    assert!(
        schema
            .parse(&format!(
                "{compact}{}",
                " ".repeat(MAX_RESPONSE_BYTES - compact.len())
            ))
            .is_ok()
    );
}

#[test]
fn diagnostics_never_echo_invalid_values_or_instance_names() {
    let schema = VerdictSchema::new(None, None).unwrap();
    for text in [
        "PRIVATE_SECRET",
        r#"{"verdict":"PRIVATE_SECRET"}"#,
        r#"{"verdict":"GREEN","PRIVATE_SECRET":"PRIVATE_SECRET"}"#,
    ] {
        let error = schema.parse(text).unwrap_err();
        assert!(error.len() < 160);
        assert!(!error.contains("PRIVATE_SECRET"));
    }
}

#[test]
fn parsed_human_submission_uses_same_validation_without_repair() {
    let eval: EvalDeclaration = serde_json::from_value(json!({
        "id":"human", "title":"Review", "profile":{"kind":"human"},
        "payload":{"instruction":"Review."},
        "passSchema":{"properties":{"reason":{"type":"string"}},"required":["reason"]}
    }))
    .unwrap();
    let valid = json!({"verdict":"GREEN","reason":"  preserved  "});
    assert_eq!(validate_result(&eval, &valid).unwrap(), valid);
    assert!(validate_result(&eval, &json!({"verdict":"RED"})).is_ok());
    for invalid in [
        json!({"verdict":"GREEN"}),
        json!({"verdict":"RED","reason":"undeclared"}),
        json!({"verdict":"GREEN","reason":"x".repeat(MAX_RESULT_CHARS)}),
        json!([{"verdict":"GREEN"}]),
    ] {
        assert!(validate_result(&eval, &invalid).is_err());
    }
}
