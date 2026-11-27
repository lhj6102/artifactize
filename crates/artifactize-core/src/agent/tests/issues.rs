//! The issues result crosses the HTTP boundary: every backend is sent the one result schema,
//! and artifactize derives the verdict from the answer's priorities and the eval's `red_at`.
use super::*;

fn response(backend: &str, text: &str) -> MockHttpResponse {
    match backend {
        "openai" => openai_response("exact-model", "completed", vec![message(text)], Value::Null),
        "anthropic" => anthropic_text(text),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn every_backend_is_sent_the_issues_schema_and_the_verdict_follows_red_at() {
    for backend in ["openai", "anthropic"] {
        // The fixture's eval declares red_at = "P1".
        for (answer, verdict) in [
            (json!({"issues":[]}), "GREEN"),
            (
                json!({"issues":[{"priority":"P2","reason":"a minor point"}]}),
                "GREEN",
            ),
            (
                json!({"issues":[{"priority":"P3","reason":"a nit"},{"priority":"P1","reason":"breaks it"}]}),
                "RED",
            ),
        ] {
            let fixture = Fixture::new(backend);
            let (review, http) = fixture
                .run(vec![response(backend, &answer.to_string())])
                .await;
            let mut saved = answer.clone();
            saved["verdict"] = json!(verdict);
            assert_eq!(
                review.result.unwrap().into_json(),
                saved,
                "{backend} {answer}"
            );
            let body: Value = serde_json::from_slice(&http.requests()[0].body).unwrap();
            let format = if backend == "openai" {
                &body["text"]["format"]
            } else {
                &body["output_config"]["format"]
            };
            assert_eq!(format["type"], "json_schema");
            let schema = &format["schema"];
            assert_eq!(schema["required"], json!(["issues"]));
            assert_eq!(schema["additionalProperties"], false);
            let finding = &schema["properties"]["issues"]["items"];
            assert_eq!(finding["required"], json!(["priority", "reason"]));
            assert_eq!(
                finding["properties"]["priority"]["enum"],
                json!(["P0", "P1", "P2", "P3"])
            );
            // The evaluator never chooses the verdict, nor learns the bar.
            assert!(!schema.to_string().contains("verdict"));
            assert!(!body.to_string().contains("red_at"));
            assert!(jsonschema::validator_for(schema).unwrap().is_valid(&answer));
            assert!(
                body["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty())
            );
        }
    }
}

#[tokio::test]
async fn a_repair_keeps_the_same_format() {
    for backend in ["openai", "anthropic"] {
        let good = json!({"issues":[{"priority":"P0","reason":"ok"}]});
        let (review, http) = Fixture::new(backend)
            .run(vec![
                response(backend, r#"{"issues":[{"priority":"P0","reason":" "}]}"#),
                response(backend, &good.to_string()),
            ])
            .await;
        assert_eq!(review.result.unwrap().verdict.as_str(), "RED");
        let calls = http.requests();
        assert_eq!(calls.len(), 2);
        let first: Value = serde_json::from_slice(&calls[0].body).unwrap();
        let repair: Value = serde_json::from_slice(&calls[1].body).unwrap();
        let field = if backend == "openai" {
            "text"
        } else {
            "output_config"
        };
        assert_eq!(repair[field], first[field]);
        assert!(repair["tools"].as_array().is_none_or(Vec::is_empty));
        assert!(repair.to_string().contains("every issue needs a reason"));
    }
}
