use super::*;

#[tokio::test]
async fn tool_budget_counts_unknown_and_invalid_calls_before_validation() {
    let mut invalid = call("first", "inspect_a");
    invalid["arguments"] = json!("{}");
    for first in [
        call("first", "inspect_a"),
        call("first", "unknown"),
        invalid,
    ] {
        let mut fixture = Fixture::new("openai");
        let Profile::Agent { max_tool_calls, .. } =
            &mut fixture.config.evals[0].declaration.profile
        else {
            unreachable!()
        };
        *max_tool_calls = Some(1);
        let (review, http) = fixture
            .run(vec![
                openai_response(
                    "exact-model",
                    "completed",
                    vec![
                        first.clone(),
                        call("second", "inspect_a"),
                        call("third", "inspect_a"),
                    ],
                    Value::Null,
                ),
                final_openai(),
            ])
            .await;
        assert!(review.result.unwrap_err().message.contains("maxToolCalls"));
        assert_eq!(http.requests().len(), 1);
        // The first call ran and keeps its result; the budget stopped the others.
        let calls = fixture.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[0].1.as_ref().unwrap().1,
            first["name"] == "unknown" || first["arguments"] == "{}"
        );
        assert!(calls[1].1.is_none() && calls[2].1.is_none());
    }
}

#[tokio::test]
async fn tool_budget_and_duplicate_ids_span_turns() {
    for duplicate in [false, true] {
        let mut fixture = Fixture::new("openai");
        let Profile::Agent { max_tool_calls, .. } =
            &mut fixture.config.evals[0].declaration.profile
        else {
            unreachable!()
        };
        *max_tool_calls = if duplicate { None } else { Some(1) };
        let mut second = call(if duplicate { "first" } else { "second" }, "inspect_a");
        second["id"] = json!("different_item_id");
        let (review, http) = fixture
            .run(vec![
                openai_response(
                    "exact-model",
                    "completed",
                    vec![call("first", "inspect_a")],
                    Value::Null,
                ),
                openai_response("exact-model", "completed", vec![second], Value::Null),
                final_openai(),
            ])
            .await;
        let error = review.result.unwrap_err().message;
        assert!(
            error.contains(if duplicate {
                "tool-call ID"
            } else {
                "maxToolCalls"
            }),
            "{error}"
        );
        assert_eq!(http.requests().len(), 2);
        let calls = fixture.calls();
        assert_eq!(calls.len(), 2);
        assert!(!calls[0].1.as_ref().unwrap().1);
        assert!(calls[1].1.is_none());
    }
}

#[tokio::test]
async fn token_budget_is_cumulative_and_crosses_before_tools() {
    let mut fixture = Fixture::new("openai");
    let Profile::Agent { max_tokens, .. } = &mut fixture.config.evals[0].declaration.profile else {
        unreachable!()
    };
    *max_tokens = Some(14);
    let (review, http) = fixture
        .run(vec![
            openai_response(
                "exact-model",
                "completed",
                vec![call("c1", "inspect_a")],
                json!({"input_tokens":5,"output_tokens":2,"total_tokens":7}),
            ),
            openai_response(
                "exact-model",
                "completed",
                vec![call("c2", "inspect_a")],
                json!({"input_tokens":6,"output_tokens":2,"total_tokens":8}),
            ),
            final_openai(),
        ])
        .await;
    assert!(review.result.unwrap_err().message.contains("maxTokens"));
    assert_eq!(http.requests().len(), 2);
    // The second turn crossed the budget before its call ran.
    let calls = fixture.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].1.is_some() && calls[1].1.is_none());
    assert_eq!(review.attempts[1].usage["totalTokens"], 8);

    let (review, _) = fixture
        .run(vec![
            openai_response(
                "exact-model",
                "completed",
                vec![call("c1", "inspect_a")],
                Value::Null,
            ),
            final_openai(),
        ])
        .await;
    assert!(review.result.is_ok());
    assert!(review.attempts[0].usage.is_empty());
    assert_eq!(review.attempts[1].usage["totalTokens"], 14);
}

#[tokio::test]
async fn token_budget_includes_anthropic_cache_reads_and_writes() {
    let mut fixture = Fixture::new("anthropic");
    let Profile::Agent { max_tokens, .. } = &mut fixture.config.evals[0].declaration.profile else {
        unreachable!()
    };
    // 10 uncached + 2 cache reads + 5 cache writes + 4 output = 21.
    *max_tokens = Some(20);
    let (review, http) = fixture
        .run(vec![anthropic_response(true, "tool_use")])
        .await;
    assert!(review.result.unwrap_err().message.contains("maxTokens"));
    assert!(fixture.calls().iter().all(|(_, answer)| answer.is_none()));
    assert_eq!(http.requests().len(), 1);
    assert_eq!(review.attempts[0].usage["cacheReadTokens"], 2);
    assert_eq!(review.attempts[0].usage["cacheWriteTokens"], 5);
}

#[tokio::test]
async fn repair_tokens_can_exceed_budget_even_with_a_valid_verdict() {
    let mut fixture = Fixture::new("openai");
    let Profile::Agent { max_tokens, .. } = &mut fixture.config.evals[0].declaration.profile else {
        unreachable!()
    };
    *max_tokens = Some(14);
    let (review, http) = fixture
        .run(vec![
            openai_response(
                "exact-model",
                "completed",
                vec![message("not JSON")],
                json!({"input_tokens":1,"output_tokens":1,"total_tokens":2}),
            ),
            final_openai(),
        ])
        .await;
    assert!(review.result.unwrap_err().message.contains("maxTokens"));
    assert_eq!(http.requests().len(), 2);
    assert_eq!(review.attempts[1].usage["totalTokens"], 14);
}
