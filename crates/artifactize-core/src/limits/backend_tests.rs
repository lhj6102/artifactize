//! Pin accepted backend names, previous diagnostics and lexicographic JSON/display order.
use super::*;
use crate::config::Provider;
use std::fs;

#[tokio::test]
async fn limits_keep_typed_backend_keys_and_existing_wire_order() {
    let directory = crate::test_os::tempdir();
    fs::write(
        directory.path().join(FILE),
        r#"{"backends":{"openai":8,"codex":4,"anthropic":2}}"#,
    )
    .unwrap();
    let limits = Limits::read(directory.path()).unwrap();
    assert_eq!(limits.limit(&Provider::Openai.into()), Some(8));
    assert_eq!(limits.limit(&Provider::Codex.into()), Some(4));
    assert_eq!(limits.limit(&Provider::Anthropic.into()), Some(2));
    assert_eq!(
        serde_json::to_string(limits.backends()).unwrap(),
        r#"{"anthropic":2,"codex":4,"openai":8}"#
    );
    let report = crate::diagnostics::doctor(Some(directory.path()), None)
        .await
        .unwrap();
    let check = report
        .checks
        .iter()
        .find(|check| check.name == "limits")
        .unwrap();
    assert_eq!(
        check.message,
        "Backend capacity on this machine: anthropic 2, codex 4, openai 8."
    );
    fs::write(directory.path().join(FILE), "{}").unwrap();
    let empty = Limits::read(directory.path()).unwrap();
    assert!(empty.backends().is_empty());
    assert_eq!(empty.limit(&Provider::Openai.into()), None);
}

#[test]
fn backend_errors_keep_previously_emitted_messages() {
    let directory = crate::test_os::tempdir();
    let path = directory.path().join(FILE);
    for (wire, expected) in [
        (
            r#"{"backends":{"my provider":2}}"#,
            "backends: backend must match [A-Za-z0-9][A-Za-z0-9_-]{0,63}.",
        ),
        (
            r#"{"backends":{"claude":2}}"#,
            "backends: backend \"claude\" was removed in 0.5.0; use \"openai\", \"anthropic\" or \"gemini\" with an API key, or \"codex\" with a ChatGPT/Codex sign-in",
        ),
        (
            r#"{"backends":{"openai":0}}"#,
            "backends.openai must be between 1 and 100000.",
        ),
        (
            r#"{"backends":{"openai":100001}}"#,
            "backends.openai must be between 1 and 100000.",
        ),
    ] {
        fs::write(&path, wire).unwrap();
        assert_eq!(
            Limits::read(directory.path()).unwrap_err(),
            format!("{}: {expected}", crate::platform::path_text(&path))
        );
    }
    for wire in [
        r#"{"backends":null}"#,
        r#"{"backends":{"openai":null}}"#,
        r#"{"backends":{"openai":1.0}}"#,
        r#"{"backends":{"-openai":1}}"#,
    ] {
        fs::write(&path, wire).unwrap();
        assert!(Limits::read(directory.path()).is_err());
    }
    // Keep old sorted validation precedence when multiple keys are invalid.
    fs::write(&path, r#"{"backends":{"zz-name!":1,"anthropic":0}}"#).unwrap();
    assert_eq!(
        Limits::read(directory.path()).unwrap_err(),
        format!(
            "{}: backends.anthropic must be between 1 and 100000.",
            crate::platform::path_text(&path)
        )
    );
}
