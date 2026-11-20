use super::*;

fn answer(findings: &[(&str, &str)]) -> String {
    json!({"findings": findings
        .iter()
        .map(|(priority, reason)| json!({"priority": priority, "reason": reason}))
        .collect::<Vec<_>>()})
    .to_string()
}

#[test]
fn a_finding_at_or_above_red_at_makes_the_eval_red_and_one_below_it_does_not() {
    let p1 = Findings::new(Some(Priority::P1));
    for (findings, expected) in [
        (vec![], Verdict::Green),
        (vec![("P3", "a typo")], Verdict::Green),
        (vec![("P2", "minor"), ("P3", "a typo")], Verdict::Green),
        (vec![("P1", "breaks the rule")], Verdict::Red),
        (vec![("P3", "a typo"), ("P0", "data loss")], Verdict::Red),
    ] {
        let result = p1.parse_model(&answer(&findings)).unwrap();
        assert_eq!(result.verdict, expected, "{findings:?}");
        // Findings below the bar are recorded with their priorities, in the evaluator's order.
        assert_eq!(
            result.into_json(),
            json!({
                "verdict": expected,
                "findings": findings
                    .iter()
                    .map(|(priority, reason)| json!({"priority": priority, "reason": reason}))
                    .collect::<Vec<_>>(),
            })
        );
    }
}

#[test]
fn without_red_at_any_finding_makes_the_eval_red() {
    let any = Findings::new(None);
    assert_eq!(
        any.parse_model(&answer(&[])).unwrap().verdict,
        Verdict::Green
    );
    for priority in ["P0", "P1", "P2", "P3"] {
        assert_eq!(
            any.parse_model(&answer(&[(priority, "found")]))
                .unwrap()
                .verdict,
            Verdict::Red,
            "{priority}"
        );
    }
}

#[test]
fn the_evaluator_reports_findings_only_never_a_verdict() {
    let findings = Findings::new(None);
    for invalid in [
        "",
        " ",
        "[]",
        "{}",
        r#"{"verdict":"GREEN"}"#,
        r#"{"verdict":"GREEN","findings":[]}"#,
        r#"{"findings":[{"priority":"P4","reason":"x"}]}"#,
        r#"{"findings":[{"priority":"p1","reason":"x"}]}"#,
        r#"{"findings":[{"priority":"P1"}]}"#,
        r#"{"findings":[{"priority":"P1","reason":"  "}]}"#,
        r#"{"findings":[{"priority":"P1","reason":"x","where":"a.rs:1"}]}"#,
        "```json\n{\"findings\":[]}\n```",
    ] {
        assert!(findings.parse_model(invalid).is_err(), "{invalid}");
    }
    let error = findings
        .parse_model(r#"{"findings":[{"priority":"P4","reason":"x"}]}"#)
        .unwrap_err();
    assert!(error.to_string().starts_with("schema_mismatch"));
    assert!(Findings::repair_detail(error).contains("P4"));
}

#[test]
fn the_schema_sent_to_the_backend_is_the_one_closed_findings_list() {
    assert_eq!(
        Findings::new(Some(Priority::P2)).schema,
        json!({
            "type":"object",
            "properties":{"findings":{
                "type":"array",
                "description":"Every finding of the review; empty when there is none.",
                "items":{
                    "type":"object",
                    "properties":{
                        "priority":{
                            "type":"string",
                            "enum":["P0","P1","P2","P3"],
                            "description":"P0 is the most important, P3 the least.",
                        },
                        "reason":{"type":"string"},
                    },
                    "required":["priority","reason"],
                    "additionalProperties":false,
                },
            }},
            "required":["findings"],
            "additionalProperties":false,
        }),
        "the bar stays with artifactize; the schema is the same for every eval"
    );
    assert_eq!(
        Findings::new(None).schema,
        Findings::new(Some(Priority::P0)).schema
    );
}

#[test]
fn raw_and_normalized_size_limits_are_explicit_not_truncation() {
    let findings = Findings::new(None);
    let prefix = r#"{"findings":[{"priority":"P2","reason":""#;
    let suffix = r#""}]}"#;
    let fits = format!(
        "{prefix}{}{suffix}",
        "x".repeat(256_000 - prefix.len() - suffix.len())
    );
    assert!(findings.parse_model(&fits).is_ok());
    let too_big = fits.replacen('x', "xx", 1);
    assert!(
        findings
            .parse_model(&too_big)
            .unwrap_err()
            .to_string()
            .contains("normalized")
    );
    assert!(
        findings
            .parse_model(&"x".repeat(1_048_577))
            .unwrap_err()
            .to_string()
            .contains("1 MiB")
    );
    let compact = r#"{"findings":[]}"#;
    let padded = format!("{compact}{}", " ".repeat(1_048_576 - compact.len()));
    assert!(findings.parse_model(&padded).is_ok());
}
