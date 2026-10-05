use serde_json::json;

use super::*;

#[test]
fn usage_limits_name_the_plan_and_reset_and_never_retry() {
    let resets = crate::auth::codex::now().unwrap() + 30 * 60;
    let body = json!({"error":{"type":"usage_limit_reached","message":"The usage limit has been reached","plan_type":"PRO","resets_at":resets}});
    let message = describe(Some(429), &body, "fallback");
    assert!(
        message.starts_with("usage_limit_reached: You have hit your ChatGPT usage limit (pro plan). Try again in ~30 min."),
        "{message}"
    );
    assert!(message.ends_with("(HTTP 429)"), "{message}");
    assert!(usage_limit(None, Some("usage_not_included")));
    assert!(!usage_limit(Some(500), Some("server_error")));
}

#[test]
fn rejected_credentials_say_how_to_sign_in_again() {
    let body = json!({"detail":"Unauthorized"});
    let message = describe(Some(401), &body, "fallback");
    assert!(
        message.starts_with("Unauthorized (HTTP 401) Sign in again"),
        "{message}"
    );
    assert!(message.contains("artifactize login codex"), "{message}");
    let failed = json!({"type":"response.failed","response":{"error":{"code":"server_error","message":"Model failed"}}});
    assert_eq!(
        describe(None, &failed, "fallback"),
        "server_error: Model failed"
    );
    assert_eq!(
        describe(Some(502), &json!("bad gateway"), "fallback"),
        "fallback (HTTP 502)"
    );
}

#[test]
fn requests_name_artifactize_as_the_caller() {
    let agent = user_agent();
    assert!(agent.starts_with(&format!("artifactize/{} (", env!("CARGO_PKG_VERSION"))));
    assert!(agent.ends_with("; artifactize)"));
}
