use serde_json::json;

use super::*;
use serde_json::Value;

#[test]
fn typed_picker_catalog_preserves_skips_defaults_and_malformed_listed_entry_errors() {
    let body = json!({"extra":true,"models":[
        null, 17, [], {"visibility":"hide","slug":null}, {"visibility":17,"slug":"skip"},
        {"visibility":"list","slug":"first","display_name":null,"extra":17},
        {"visibility":"list","slug":"second","display_name":42},
        {"visibility":"list","slug":"third","display_name":""}
    ]});
    assert_eq!(
        serde_json::to_value(picker_models(&serde_json::to_vec(&body).unwrap()).unwrap()).unwrap(),
        json!([
            {"slug":"first","display_name":"first"},
            {"slug":"second","display_name":"second"},
            {"slug":"third","display_name":""},
        ])
    );
    for body in [
        json!({}),
        json!({"models":null}),
        json!({"models":{}}),
        json!([]),
        json!({"models":[{"visibility":"list"}]}),
        json!({"models":[{"visibility":"list","slug":""}]}),
        json!({"models":[{"visibility":"list","slug":17}]}),
    ] {
        assert_eq!(
            picker_models(&serde_json::to_vec(&body).unwrap())
                .err()
                .unwrap(),
            "Invalid Codex models response."
        );
    }
    for body in [
        br#"{"models":[],"extra":1e400}"#.as_slice(),
        br#"{"models":[],"extra":{"nested":[1e400]}}"#.as_slice(),
        br#"{"models":[],"extra":1e400,"extra":null}"#.as_slice(),
    ] {
        assert!(serde_json::from_slice::<Value>(body).is_err());
        assert_eq!(
            picker_models(body).err().unwrap(),
            "Invalid Codex models response."
        );
    }
    assert!(
        picker_models(br#"{"models":[],"extra":18446744073709551616}"#)
            .unwrap()
            .is_empty()
    );
    let duplicates =
        br#"{"models":null,"models":[{"visibility":"hide","visibility":"list","slug":17,"slug":"last","display_name":"old","display_name":null}]}"#;
    assert_eq!(
        serde_json::to_value(picker_models(duplicates).unwrap()).unwrap(),
        json!([{"slug":"last","display_name":"last"}])
    );
}

#[test]
fn usage_limits_name_the_plan_and_reset_and_never_retry() {
    let resets = 1_800 + 30 * 60;
    let body = json!({
        "error":{
            "type":"usage_limit_reached",
            "message":"The usage limit has been reached",
            "plan_type":"PRO",
            "resets_at":resets,
        },
    });
    let message = describe(
        Some(429),
        parsed(&body).as_ref(),
        "fallback",
        Some(crate::auth::codex::Timestamp::from_seconds(1_800)),
    );
    assert!(
        message.starts_with(
            "usage_limit_reached: You have hit your ChatGPT usage limit (pro plan). Try again in ~30 min."
        ),
        "{message}"
    );
    assert!(message.ends_with("(HTTP 429)"), "{message}");
    assert!(
        parsed(&json!({"error":{"code":"usage_not_included"}}))
            .unwrap()
            .usage_limit(None)
    );
    assert!(
        parsed(&json!({"error":{"resets_at":1}}))
            .unwrap()
            .usage_limit(Some(429))
    );
    // A plain 429 is a rate limit, worded by the provider.
    let plain = json!({"error":{"code":"rate_limit_exceeded","message":"Slow down"}});
    assert!(!parsed(&plain).unwrap().usage_limit(Some(429)));
    assert_eq!(
        describe(
            Some(429),
            parsed(&plain).as_ref(),
            "fallback",
            Some(crate::auth::codex::Timestamp::from_seconds(1_800))
        ),
        "rate_limit_exceeded: Slow down (HTTP 429)"
    );
    assert!(
        !parsed(&json!({"error":{"code":"server_error"}}))
            .unwrap()
            .usage_limit(Some(500))
    );
}

#[test]
fn rejected_credentials_say_how_to_sign_in_again() {
    let body = json!({"detail":"Unauthorized"});
    let message = describe(
        Some(401),
        parsed(&body).as_ref(),
        "fallback",
        Some(crate::auth::codex::Timestamp::from_seconds(1_800)),
    );
    assert!(
        message.starts_with("Unauthorized (HTTP 401) Sign in again"),
        "{message}"
    );
    assert!(message.contains("artifactize login codex"), "{message}");
    let failed = json!({
        "type":"response.failed",
        "response":{"error":{"code":"server_error","message":"Model failed"}},
    });
    assert_eq!(
        describe(
            None,
            parsed(&failed).as_ref(),
            "fallback",
            Some(crate::auth::codex::Timestamp::from_seconds(1_800))
        ),
        "server_error: Model failed"
    );
    assert_eq!(
        describe(
            Some(502),
            None,
            "fallback",
            Some(crate::auth::codex::Timestamp::from_seconds(1_800))
        ),
        "fallback (HTTP 502)"
    );
}

#[test]
fn requests_name_artifactize_as_the_caller() {
    let agent = user_agent();
    assert!(agent.starts_with(&format!("artifactize/{} (", env!("CARGO_PKG_VERSION"))));
    assert!(agent.ends_with("; artifactize)"));
}

fn parsed(value: &serde_json::Value) -> Option<super::errors::Error> {
    super::errors::Error::parse(&serde_json::to_vec(value).unwrap())
}
