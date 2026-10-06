use super::*;

fn final_text(text: &str) -> MockHttpResponse {
    openai_response("exact-model", "completed", vec![message(text)], Value::Null)
}

#[tokio::test]
async fn fenced_json_is_repaired_with_same_settings_and_no_tools() {
    let (review, http) = Fixture::new("openai")
        .run(vec![
            final_text("```json\n{\"verdict\":\"GREEN\"}\n```"),
            final_text(r#"{"verdict":"RED"}"#),
        ])
        .await;
    assert_eq!(review.result.unwrap(), json!({"verdict":"RED"}));
    let requests = http.requests();
    assert_eq!(requests.len(), 2);
    let first: Value = serde_json::from_slice(&requests[0].body).unwrap();
    let repair: Value = serde_json::from_slice(&requests[1].body).unwrap();
    for field in ["model", "reasoning", "store", "instructions"] {
        assert_eq!(repair[field], first[field]);
    }
    assert!(repair["tools"].as_array().is_none_or(Vec::is_empty));
    assert_eq!(repair["tool_choice"], "none");
    assert!(repair.to_string().contains("```json"));
    let prompt = repair["input"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .to_string();
    assert!(prompt.contains("not_json"));
    assert!(prompt.contains("Return only one JSON object"));
    assert!(!prompt.contains("GREEN"));
    assert!(!prompt.contains("RED"));
    assert_eq!(review.attempts[1].turn, 2);
}

#[tokio::test]
async fn schema_invalid_response_repairs_and_preserves_owner_fields() {
    let mut fixture = Fixture::new("openai");
    fixture.config.evals[0].declaration.pass_schema = json!({"type":"object","properties":{"reason":{"type":"string","minLength":1}},"required":["reason"]}).as_object().cloned();
    let result = json!({"verdict":"GREEN","reason":"  Owner text stays unchanged.  "});
    let (review, http) = fixture
        .run(vec![
            final_text(r#"{"verdict":"GREEN","reason":false}"#),
            final_text(&result.to_string()),
        ])
        .await;
    assert_eq!(review.result.unwrap(), result);
    assert_eq!(http.requests().len(), 2);
    let repair: Value = serde_json::from_slice(&http.requests()[1].body).unwrap();
    let prompt = repair["input"].as_array().unwrap().last().unwrap()["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        prompt,
        "Your final response did not match the required schema: schema_mismatch: result must match the selected verdict's owner schema\n- instancePath \"/reason\": false is not of type \"string\"\nReturn only one JSON object matching the schema."
    );
}

#[tokio::test]
async fn schemas_are_selected_by_the_final_verdict() {
    let mut fixture = Fixture::new("openai");
    fixture.config.evals[0].declaration.pass_schema =
        json!({"type":"object","properties":{"reason":{"const":"pass"}},"required":["reason"]})
            .as_object()
            .cloned();
    fixture.config.evals[0].declaration.fail_schema =
        json!({"type":"object","properties":{"reason":{"const":"fail"}},"required":["reason"]})
            .as_object()
            .cloned();
    for (verdict, wrong, right) in [("GREEN", "fail", "pass"), ("RED", "pass", "fail")] {
        let result = json!({"verdict":verdict,"reason":right});
        let (review, http) = fixture
            .run(vec![
                final_text(&json!({"verdict":verdict,"reason":wrong}).to_string()),
                final_text(&result.to_string()),
            ])
            .await;
        assert_eq!(review.result.unwrap(), result);
        assert_eq!(http.requests().len(), 2);
    }
}

#[tokio::test]
async fn second_failure_is_error_without_response_text_or_third_turn() {
    let (review, http) = Fixture::new("openai")
        .run(vec![
            final_text("PRIVATE_SECRET"),
            final_text(r#"{"verdict":"GREEN","PRIVATE_SECRET":"PRIVATE_SECRET"}"#),
            final_openai(),
        ])
        .await;
    let error = review.result.unwrap_err().message;
    assert!(error.contains("after one format repair"));
    assert!(error.contains("schema_mismatch"));
    assert!(!error.contains("PRIVATE_SECRET"));
    assert_eq!(http.requests().len(), 2);
    assert!(
        !json!(review.attempts)
            .to_string()
            .contains("PRIVATE_SECRET")
    );
}

#[tokio::test]
async fn unsolicited_repair_tool_calls_never_execute_or_continue() {
    let fixture = Fixture::new("openai");
    let (review, http) = fixture
        .run(vec![
            final_text("not JSON"),
            openai_response(
                "exact-model",
                "completed",
                vec![call("c1", "inspect_a")],
                Value::Null,
            ),
            final_openai(),
        ])
        .await;
    assert!(
        review
            .result
            .unwrap_err()
            .message
            .contains("tools are disabled")
    );
    assert!(fixture.calls().iter().all(|(_, answer)| answer.is_none()));
    assert_eq!(http.requests().len(), 2);
}

#[tokio::test]
async fn repair_retains_model_and_completion_checks() {
    for response in [
        openai_response(
            "wrong-model",
            "completed",
            vec![message(r#"{"verdict":"GREEN"}"#)],
            Value::Null,
        ),
        openai_response(
            "exact-model",
            "incomplete",
            vec![message(r#"{"verdict":"GREEN"}"#)],
            Value::Null,
        ),
    ] {
        let (review, http) = Fixture::new("openai")
            .run(vec![final_text("invalid"), response, final_openai()])
            .await;
        assert!(review.result.is_err());
        assert_eq!(http.requests().len(), 2);
    }
}

#[tokio::test]
async fn anthropic_repair_after_tools_exposes_no_tools() {
    let mut fixture = Fixture::new("anthropic");
    fixture.config.evals[0].declaration.fail_schema =
        json!({"type":"object","properties":{"reason":{"type":"string"}},"required":["reason"]})
            .as_object()
            .cloned();
    let (review, http) = fixture
        .run(vec![
            anthropic_response(true, "tool_use"),
            anthropic_response(false, "end_turn"),
            anthropic_response(false, "end_turn"),
        ])
        .await;
    let failure = review.result.unwrap_err();
    assert!(failure.message.contains("after one format repair"));
    assert_eq!(failure.code, crate::agent::error::Code::InvalidResult);
    assert_eq!(fixture.calls().len(), 1);
    let requests = http.requests();
    assert_eq!(requests.len(), 3);
    let body: Value = serde_json::from_slice(&requests[2].body).unwrap();
    assert!(body["tools"].as_array().is_none_or(Vec::is_empty));
    // rig omits Anthropic tool_choice when there are no tool definitions.
    assert!(body["tool_choice"].is_null());
    assert!(body.to_string().contains("tool evidence"));
}

#[derive(Clone)]
struct DelayResponses(Duration);

impl rig_core::http_client::HttpMiddleware for DelayResponses {
    fn after_response<'a>(
        &'a self,
        _: &'a rig_core::http_client::Method,
        _: &'a rig_core::http_client::Uri,
        _: StatusCode,
        _: &'a rig_core::http_client::HeaderMap,
    ) -> rig_core::wasm_compat::WasmBoxedFuture<'a, rig_core::http_client::Result<()>> {
        Box::pin(async move {
            tokio::time::sleep(self.0).await;
            Ok(())
        })
    }
}

#[tokio::test(start_paused = true)]
async fn repair_shares_original_deadline_and_cancellation_precedes_budget() {
    for cancel in [false, true] {
        let mut fixture = Fixture::new("openai");
        let Profile::Agent {
            timeout_ms,
            max_tokens,
            ..
        } = &mut fixture.config.evals[0].declaration.profile
        else {
            unreachable!()
        };
        *timeout_ms = Some(100);
        *max_tokens = Some(1);
        let http = SequencedHttpClient::new(vec![final_text("invalid"), final_openai()]);
        let client = Client::Openai(Box::new(
            OpenAIConfig::new("fake-key")
                .connect(
                    rig_core::http_client::DynHttpClient::new(http.clone())
                        .with_middleware(DelayResponses(Duration::from_millis(60))),
                )
                .responses("exact-model"),
        ));
        let cancellation = CancellationToken::new();
        if cancel {
            let token = cancellation.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(90)).await;
                token.cancel();
            });
        }
        let started = Instant::now();
        let review = review(
            &client,
            &fixture.config,
            &fixture.config.evals[0],
            &fixture.output,
            &mut session::Recorder::off(SESSION),
            cancellation,
        )
        .await;
        assert_eq!(
            review.result.unwrap_err().message,
            if cancel {
                "Agent review was cancelled."
            } else {
                "Agent review timed out."
            }
        );
        assert_eq!(http.requests().len(), 2);
        assert!(started.elapsed() <= Duration::from_millis(100));
        assert_eq!(review.attempts.len(), 2);
    }
}
