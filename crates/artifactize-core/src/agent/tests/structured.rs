//! Structured final answers cross the HTTP boundary without changing tools or saved owner JSON.
use super::*;

fn anthropic_text(text: &str) -> MockHttpResponse {
    sse(vec![
        json!({"type":"message_start","message":{
            "id":"msg_1","type":"message","role":"assistant","model":"exact-model",
            "content":[],"usage":{"input_tokens":1,"output_tokens":0}
        }}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
        json!({"type":"message_stop"}),
    ])
}

/// The discriminator sorts before its carrier regardless of owner field names.
fn native(mut result: Value) -> Value {
    let verdict = if let Some(encoded) = result.as_str() {
        serde_json::from_str::<Value>(encoded).unwrap()["verdict"].clone()
    } else {
        result.as_object_mut().unwrap().remove("verdict").unwrap()
    };
    json!({"result":{"verdict":verdict,"verdictFields":result}})
}

fn response(backend: &str, value: &Value) -> MockHttpResponse {
    let text = value.to_string();
    match backend {
        "openai" => openai_response(
            "exact-model",
            "completed",
            vec![message(&text)],
            Value::Null,
        ),
        "anthropic" => anthropic_text(&text),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn native_red_arrays_and_backend_constraints_reach_the_wire() {
    for backend in ["openai", "anthropic"] {
        let mut fixture = Fixture::new(backend);
        *fixture.config.evals[0].declaration.fail_schema_mut() = json!({
            "type":"object", "additionalProperties":false,
            "properties":{"violations":{
                "type":"array", "minItems":1, "maxItems":20,
                "items":{"type":"object", "additionalProperties":false,
                    "properties":{"where":{"type":"string","minLength":1}}, "required":["where"]}
            }}, "required":["violations"]
        })
        .as_object()
        .cloned();
        let result = json!({"verdict":"RED","violations":[{"where":"src/lib.rs:1"}]});
        let (review, http) = fixture
            .run(vec![response(backend, &native(result.clone()))])
            .await;
        assert_eq!(review.result.unwrap().into_json(), result);
        let body: Value = serde_json::from_slice(&http.requests()[0].body).unwrap();
        let format = if backend == "openai" {
            &body["text"]["format"]
        } else {
            &body["output_config"]["format"]
        };
        let schema = &format["schema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"], json!(["result"]));
        assert!(schema.get("anyOf").is_none());
        for branch in schema["properties"]["result"]["anyOf"].as_array().unwrap() {
            assert_eq!(
                branch["properties"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                ["verdict", "verdictFields"]
            );
        }
        assert!(
            jsonschema::validator_for(schema)
                .unwrap()
                .is_valid(&native(result.clone()))
        );
        let violations = &schema["properties"]["result"]["anyOf"][1]["properties"]["verdictFields"]
            ["properties"]["violations"];
        assert_eq!(violations["type"], "array");
        assert_eq!(violations["minItems"], 1);
        if backend == "openai" {
            assert_eq!(violations["maxItems"], 20);
        } else {
            assert!(violations.get("maxItems").is_none());
            assert!(
                violations["description"]
                    .as_str()
                    .unwrap()
                    .contains("\"maxItems\":20")
            );
        }
        let item = &violations["items"];
        assert_eq!(item["required"], json!(["where"]));
        assert_eq!(item["additionalProperties"], false);
        assert!(item["properties"]["where"].get("minLength").is_none());
        assert!(
            body["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty())
        );
    }
}

#[tokio::test]
async fn nullable_placeholders_and_json_encoded_shapes_preserve_owner_results() {
    for backend in ["openai", "anthropic"] {
        let mut fixture = Fixture::new(backend);
        *fixture.config.evals[0].declaration.pass_schema_mut() = json!({
            "type":"object", "additionalProperties":false,
            "properties":{
                "optional":{"type":"string"},
                "nested":{"type":"object","additionalProperties":false,"properties":{"note":{"type":"string"}}},
                "nullable":{"type":["string","null"]},
                "choice":{"anyOf":[{"type":"string"},{"type":"integer"}]},
                "constant":{"type":"object","additionalProperties":false,"properties":{"tag":{"type":"string"}},"required":["tag"],"const":{"tag":"p"}},
                "open":{"type":"object","additionalProperties":{"type":"integer"}},
                "label":{"$ref":"#/$defs/label"}
            },
            "$defs":{"label":{"type":"string","minLength":2}},
            "required":["nested","open","label","choice","constant"]
        }).as_object().cloned();
        let wire = native(json!({
            "verdict":"GREEN", "optional":null, "nested":{"note":null},
            "nullable":null, "choice":3, "constant":{"tag":"p"}, "open":"{\"dynamic\":3}", "label":"\"ok\""
        }));
        let (review, http) = fixture.run(vec![response(backend, &wire)]).await;
        assert_eq!(
            review.result.unwrap().into_json(),
            json!({
                "verdict":"GREEN", "nested":{}, "nullable":null, "choice":3, "constant":{"tag":"p"},
                "open":{"dynamic":3}, "label":"ok"
            })
        );
        let body: Value = serde_json::from_slice(&http.requests()[0].body).unwrap();
        let schema = if backend == "openai" {
            &body["text"]["format"]["schema"]
        } else {
            &body["output_config"]["format"]["schema"]
        };
        assert!(jsonschema::validator_for(schema).unwrap().is_valid(&wire));
        let green = &schema["properties"]["result"]["anyOf"][0]["properties"]["verdictFields"];
        assert_eq!(green["required"].as_array().unwrap().len(), 7);
        assert!(green["properties"]["constant"].get("enum").is_none());
    }
}

#[tokio::test]
async fn unsupported_constraints_are_locally_enforced_and_repaired_with_the_same_format() {
    for backend in ["openai", "anthropic"] {
        let mut fixture = Fixture::new(backend);
        *fixture.config.evals[0].declaration.pass_schema_mut() = json!({
            "type":"object", "additionalProperties":false,
            "properties":{"summary":{"type":"string","minLength":2}}, "required":["summary"]
        })
        .as_object()
        .cloned();
        let good = json!({"verdict":"GREEN","summary":"ok"});
        let (review, http) = fixture
            .run(vec![
                response(backend, &native(json!({"verdict":"GREEN","summary":""}))),
                response(backend, &native(good.clone())),
            ])
            .await;
        assert_eq!(review.result.unwrap().into_json(), good);
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
        assert!(repair.to_string().contains("instancePath"));
        assert!(repair.to_string().contains("/summary"));
    }
}

#[tokio::test]
async fn responses_schema_limits_keep_valid_owner_results_representable() {
    let nested = (0..12).fold((json!({"type":"string"}), json!("leaf")), |(schema, value), _| (
        json!({"type":"object","additionalProperties":false,"properties":{"child":schema},"required":["child"]}),
        json!({"child":value}),
    ));
    let enum_entries: Vec<_> = (0..1000).map(|index| format!("choice-{index}")).collect();
    let enum_names: Vec<_> = (0..251)
        .map(|index| format!("{}-{index}", "x".repeat(64)))
        .collect();
    for (detail, value) in [
        nested,
        (
            json!({"type":"string","enum":enum_entries}),
            json!("choice-999"),
        ),
        (
            json!({"type":"string","enum":enum_names}),
            json!(format!("{}-250", "x".repeat(64))),
        ),
    ] {
        let mut fixture = Fixture::new("openai");
        *fixture.config.evals[0].declaration.pass_schema_mut() = json!({
            "type":"object", "additionalProperties":false,
            "properties":{"detail":detail}, "required":["detail"]
        })
        .as_object()
        .cloned();
        let result = json!({"verdict":"GREEN","detail":value});
        let wire = native(json!(result.to_string()));
        let (review, http) = fixture.run(vec![response("openai", &wire)]).await;
        assert_eq!(review.result.unwrap().into_json(), result);
        let body: Value = serde_json::from_slice(&http.requests()[0].body).unwrap();
        let schema = &body["text"]["format"]["schema"];
        assert_eq!(
            schema["properties"]["result"]["anyOf"][0]["properties"]["verdictFields"]["type"],
            "string"
        );
        assert!(jsonschema::validator_for(schema).unwrap().is_valid(&wire));
    }
}
