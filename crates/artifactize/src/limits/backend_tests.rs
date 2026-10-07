//! Pin accepted backend names, previous diagnostics and lexicographic JSON/display order.
use super::*;

#[test]
fn limits_keep_typed_backend_keys_and_existing_wire_order() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join(FILE),
        r#"{"backends":{"openai":8,"codex":4,"anthropic":2}}"#,
    )
    .unwrap();
    let limits = Limits::read(directory.path()).unwrap();
    assert_eq!(limits.limit(Backend::Openai), Some(8));
    assert_eq!(limits.limit(Backend::Codex), Some(4));
    assert_eq!(limits.limit(Backend::Anthropic), Some(2));
    assert_eq!(
        serde_json::to_string(limits.backends()).unwrap(),
        r#"{"anthropic":2,"codex":4,"openai":8}"#
    );
    let display = limits
        .backends()
        .iter()
        .map(|(backend, slots)| format!("{backend} {slots}"))
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(display, "anthropic 2, codex 4, openai 8");
    fs::write(directory.path().join(FILE), "{}").unwrap();
    let empty = Limits::read(directory.path()).unwrap();
    assert!(empty.backends().is_empty());
    assert_eq!(empty.limit(Backend::Openai), None);
}

#[test]
fn backend_errors_keep_previously_emitted_messages() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(FILE);
    for (wire, expected) in [
        (
            r#"{"backends":{"opneai":2}}"#,
            "backends: unknown variant `opneai`, expected one of `openai`, `anthropic`, `codex`",
        ),
        (
            r#"{"backends":{"claude":2}}"#,
            "backends: backend \"claude\" was removed in 0.5.0; use \"openai\" or \"anthropic\" with an API key, or \"codex\" with a ChatGPT/Codex sign-in",
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
            format!("{}: {expected}", path.display())
        );
    }
    for wire in [
        r#"{"backends":null}"#,
        r#"{"backends":{"openai":null}}"#,
        r#"{"backends":{"openai":1.0}}"#,
        r#"{"backends":{"Openai":1}}"#,
    ] {
        fs::write(&path, wire).unwrap();
        assert!(Limits::read(directory.path()).is_err());
    }
    // Keep old sorted validation precedence when multiple keys are invalid.
    fs::write(&path, r#"{"backends":{"opneai":1,"anthropic":0}}"#).unwrap();
    assert_eq!(
        Limits::read(directory.path()).unwrap_err(),
        format!(
            "{}: backends.anthropic must be between 1 and 100000.",
            path.display()
        )
    );
}
