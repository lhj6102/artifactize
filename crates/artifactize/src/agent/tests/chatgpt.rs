use super::*;
use crate::llm::{Server, stored_credentials};

async fn run(fixture: &Fixture, responses: Vec<MockHttpResponse>) -> (Review, Server) {
    let state = fixture._directory.path().join("state");
    stored_credentials(&state, "stored-siwc-token");
    let server = Server::new(responses).await;
    let client = server.client(&state, &fixture.config.root);
    let review = review(
        &client,
        &fixture.config,
        &fixture.config.evals[0],
        &fixture.output,
        CancellationToken::new(),
    )
    .await;
    (review, server)
}

fn assert_contract(body: &Value) {
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["model"], "exact-model");
    assert_eq!(body["reasoning"]["effort"], "high");
    assert!(
        body["instructions"]
            .as_str()
            .unwrap()
            .contains("untrusted evidence")
    );
    assert!(
        body["input"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["role"] != "system")
    );
    // An allowlist also catches future rig defaults outside the SIWC contract.
    for key in body.as_object().unwrap().keys() {
        assert!(
            [
                "model",
                "instructions",
                "input",
                "tools",
                "tool_choice",
                "store",
                "stream",
                "reasoning",
                "include",
                "parallel_tool_calls"
            ]
            .contains(&key.as_str()),
            "unexpected request field: {key}"
        );
    }
}

#[tokio::test]
async fn stored_bearer_stateless_tool_round_trip_and_wire_contract() {
    let mut fixture = Fixture::new("chatgpt");
    let Profile::Agent {
        max_tokens,
        max_tool_calls,
        ..
    } = &mut fixture.config.evals[0].declaration.profile
    else {
        unreachable!()
    };
    *max_tokens = Some(14);
    *max_tool_calls = Some(1);
    let (review, server) = run(
        &fixture,
        vec![
            openai_response(
                "exact-model",
                "completed",
                vec![call("c1", "inspect_a")],
                Value::Null,
            ),
            final_openai(),
        ],
    )
    .await;
    assert_eq!(review.result.unwrap(), json!({"verdict":"GREEN"}));
    assert_eq!(review.tool_calls.len(), 1);
    assert_eq!(review.tool_calls[0]["isError"], false);
    assert_eq!(review.attempts[1].usage["totalTokens"], 14);
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    for request in &requests {
        assert_eq!(request.line, "POST /v1/responses HTTP/1.1");
        assert_eq!(request.headers["authorization"], "Bearer stored-siwc-token");
        assert_contract(&request.body);
    }
    let first = &requests[0].body;
    let second = &requests[1].body;
    assert_eq!(first["instructions"], second["instructions"]);
    assert_eq!(first["input"][0], second["input"][0]);
    let input = second["input"].as_array().unwrap();
    assert!(
        input
            .iter()
            .any(|item| item["type"] == "function_call" && item["call_id"] == "c1")
    );
    assert!(
        input
            .iter()
            .any(|item| item["type"] == "function_call_output"
                && item["call_id"] == "c1"
                && item.to_string().contains("tool evidence"))
    );
}

#[tokio::test]
async fn repair_retains_contract_and_disables_tools() {
    let (review, server) = run(
        &Fixture::new("chatgpt"),
        vec![
            openai_response(
                "exact-model",
                "completed",
                vec![message("```json\n{\"verdict\":\"GREEN\"}\n```")],
                Value::Null,
            ),
            final_openai(),
        ],
    )
    .await;
    assert!(review.result.is_ok());
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_contract(&requests[1].body);
    assert_eq!(requests[1].body["tool_choice"], "none");
    assert!(
        requests[1].body["tools"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
}

#[tokio::test]
async fn incomplete_wrong_model_and_interrupted_stream_are_errors_before_tools() {
    let fixture = Fixture::new("chatgpt");
    for response in [
        openai_response(
            "exact-model",
            "incomplete",
            vec![message("{\"verdict\":\"GREEN\"}")],
            Value::Null,
        ),
        openai_response(
            "other-model",
            "completed",
            vec![call("c1", "inspect_a")],
            Value::Null,
        ),
        sse(vec![
            json!({"type":"response.output_item.added","output_index":0,"item":call("c1", "inspect_a")}),
        ]),
        sse(vec![]),
    ] {
        let (review, server) = run(&fixture, vec![response, final_openai()]).await;
        assert!(review.result.is_err(), "{:?}", review.result);
        assert!(review.tool_calls.is_empty());
        assert_eq!(server.requests().len(), 1);
    }
    for reason in ["max_output_tokens", "content_filter"] {
        let event = json!({"type":"response.incomplete","response":{
            "id":"resp_1", "object":"response", "created_at":1, "model":"exact-model", "status":"incomplete",
            "incomplete_details":{"reason":reason}, "output":[call("c1", "inspect_a")], "usage":null,
        }});
        let (review, server) = run(&fixture, vec![sse(vec![event])]).await;
        assert!(review.result.unwrap_err().contains("incomplete"));
        assert!(review.tool_calls.is_empty());
        assert_eq!(server.requests().len(), 1);
    }
}

fn failed(code: &str) -> MockHttpResponse {
    sse(vec![json!({"type":"response.failed", "response":{
        "id":"resp_1", "object":"response", "created_at":1, "model":"exact-model", "status":"failed", "output":[],
        "error":{"code":code,"message":"Provider explains the failure"},
    }})])
}

#[tokio::test]
async fn subscription_errors_preserve_body_codes_and_never_switch_billing() {
    let fixture = Fixture::new("chatgpt");
    for code in [
        "subscription_sharing_usage_limit_exceeded",
        "subscription_sharing_user_not_eligible",
        "subscription_sharing_invalid_user",
    ] {
        let (review, server) = run(&fixture, vec![failed(code), final_openai()]).await;
        let error = review.result.unwrap_err();
        assert!(error.contains(code), "{error}");
        assert!(error.contains("Provider explains the failure"), "{error}");
        assert_eq!(server.requests().len(), 1);
        assert_eq!(review.attempts.len(), 1);
        if code == "subscription_sharing_invalid_user" {
            assert!(error.contains("artifactize login chatgpt"));
        }
    }
    let code = "subscription_sharing_usage_unavailable";
    let (review, server) = run(
        &fixture,
        vec![failed(code), failed(code), failed(code), final_openai()],
    )
    .await;
    assert!(review.result.unwrap_err().contains(code));
    assert_eq!(server.requests().len(), 3);
    assert_eq!(review.attempts.len(), 3);
}

#[tokio::test]
async fn failure_after_partial_output_never_retries_and_http_auth_explains_login() {
    let partial_failure = sse(vec![
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":"partial"}),
        json!({"type":"response.failed","response":{"id":"resp_1","object":"response","created_at":1,"model":"exact-model","status":"failed","output":[],"error":{"code":"subscription_sharing_usage_unavailable","message":"Try later"}}}),
    ]);
    let fixture = Fixture::new("chatgpt");
    let (review, server) = run(&fixture, vec![partial_failure, final_openai()]).await;
    assert!(
        review
            .result
            .unwrap_err()
            .contains("subscription_sharing_usage_unavailable: Try later")
    );
    assert_eq!(server.requests().len(), 1);
    let (review, server) = run(
        &fixture,
        vec![
            MockHttpResponse::error(
                StatusCode::UNAUTHORIZED,
                json!({"detail":"Identity rejected"}).to_string(),
            ),
            final_openai(),
        ],
    )
    .await;
    let error = review.result.unwrap_err();
    assert!(error.contains("Identity rejected"), "{error}");
    assert!(error.contains("artifactize login chatgpt"), "{error}");
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn untyped_event_stream_passes_and_other_replies_keep_status_and_code() {
    let fixture = Fixture::new("chatgpt");
    // The real SIWC route answers 200 with an event stream and no Content-Type.
    let MockHttpResponse::SuccessWithHeaders(stream, _) = final_openai() else {
        unreachable!()
    };
    let (review, _) = run(&fixture, vec![MockHttpResponse::success_typed(stream, "")]).await;
    assert_eq!(review.result.unwrap(), json!({"verdict":"GREEN"}));
    let refusal =
        json!({"error":{"code":"subscription_sharing_user_not_eligible","message":"Not eligible"}})
            .to_string();
    for (response, expected) in [
        (
            MockHttpResponse::error(StatusCode::FORBIDDEN, refusal.clone()),
            "subscription_sharing_user_not_eligible: Not eligible (HTTP 403)",
        ),
        (
            MockHttpResponse::success_typed(refusal, ""),
            "subscription_sharing_user_not_eligible: Not eligible (HTTP 200)",
        ),
        (
            MockHttpResponse::success_typed("<html>busy</html>", "text/html"),
            "content type \"text/html\": <html>busy</html> (HTTP 200)",
        ),
    ] {
        let (review, server) = run(&fixture, vec![response, final_openai()]).await;
        let error = review.result.unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!(server.requests().len(), 1);
    }
}

#[tokio::test]
async fn each_turn_rereads_auth_and_all_system_messages_become_instructions() {
    let fixture = Fixture::new("chatgpt");
    let state = fixture._directory.path().join("state");
    stored_credentials(&state, "first-token");
    let server = Server::new(vec![final_openai(), final_openai()]).await;
    let client = server.client(&state, &fixture.config.root);
    let mut request = CompletionRequest::new("review").preamble("untrusted evidence");
    request
        .chat_history
        .push(Message::system("mid-history instruction"));
    request.chat_history.push(Message::user("continue"));
    request.additional_params = Some(Client::parameters(Backend::Chatgpt, Some("high")).unwrap());
    let cancellation = CancellationToken::new();
    let mut attempts = Vec::new();
    for number in 1..=2 {
        if number == 2 {
            stored_credentials(&state, "rotated-token");
        }
        client
            .turn(
                &request,
                llm::Turn {
                    number,
                    prior_output: number > 1,
                    deadline: Instant::now() + Duration::from_secs(5),
                    cancellation: &cancellation,
                },
                &mut attempts,
            )
            .await
            .unwrap();
    }
    let requests = server.requests();
    assert_eq!(requests[0].headers["authorization"], "Bearer first-token");
    assert_eq!(requests[1].headers["authorization"], "Bearer rotated-token");
    for request in &requests {
        assert_contract(&request.body);
        assert!(
            request.body["instructions"]
                .as_str()
                .unwrap()
                .contains("mid-history instruction")
        );
    }
}

#[tokio::test]
async fn auth_lock_is_within_review_deadline() {
    let mut fixture = Fixture::new("chatgpt");
    let Profile::Agent { timeout_ms, .. } = &mut fixture.config.evals[0].declaration.profile else {
        unreachable!()
    };
    *timeout_ms = Some(30);
    let state = fixture._directory.path().join("state");
    stored_credentials(&state, "test-token");
    use std::os::unix::fs::OpenOptionsExt;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(state.join("auth/chatgpt.lock"))
        .unwrap();
    lock.lock().unwrap();
    let server = Server::new(vec![final_openai()]).await;
    let client = server.client(&state, &fixture.config.root);
    let review = review(
        &client,
        &fixture.config,
        &fixture.config.evals[0],
        &fixture.output,
        CancellationToken::new(),
    )
    .await;
    assert!(review.result.unwrap_err().contains("timed out"));
    assert!(server.requests().is_empty());
}
