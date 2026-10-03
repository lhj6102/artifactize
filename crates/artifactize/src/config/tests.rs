use serde_json::{Value, json};

use super::*;

fn critic(profile: Value) -> Value {
    json!({
        "id": "check", "title": "Check the artifact", "profile": profile,
        "payload": { "instruction": "Inspect {input}.", "ownerData": [true, null, {"value": 42}] }
    })
}

fn parse(value: Value) -> Result<ArtifactDeclaration, String> {
    parse_declaration(&value.to_string())
}

#[test]
fn defaults_and_payload_are_preserved_without_interpolation() {
    let empty = parse(json!({ "name": "input" })).unwrap();
    assert!(empty.critics.is_empty());
    assert!(empty.views.agent_tools.is_empty());
    assert!(empty.views.human_tools.is_empty());
    assert!(empty.mounts.is_empty());
    assert_eq!(empty.basis, None);

    let original = critic(json!({ "kind": "human" }));
    let declaration = parse(json!({"name": "review", "critics": [original.clone()]})).unwrap();
    assert_eq!(
        declaration.critics[0].payload,
        *original["payload"].as_object().unwrap()
    );
}

#[test]
fn unknown_fields_and_explicit_nulls_are_not_ignored() {
    assert!(
        parse(json!({"name": "a", "target": "b"}))
            .unwrap_err()
            .contains("unknown field")
    );
    assert!(parse(json!({"name": "a", "basis": null})).is_err());
    assert!(parse(json!({"name": "a", "critics": null})).is_err());
    assert!(parse(json!({"name": "a", "views": null})).is_err());
    assert!(parse(json!({"name": "a", "mounts": null})).is_err());
    assert!(parse(json!({"name": "a", "stale": null})).is_err());
    let mut declared = critic(json!({"kind": "human"}));
    declared["deps"] = json!([]);
    assert!(
        parse(json!({"name": "a", "critics": [declared]}))
            .unwrap_err()
            .contains("deps")
    );
    let invalid_human = critic(json!({"kind": "human", "timeoutMs": 100}));
    assert!(parse(json!({"name": "a", "critics": [invalid_human]})).is_err());
    assert!(parse(json!({"name": "a", "stale": {"kind": "always", "paths": ["input"]}})).is_err());
}

#[test]
fn identifiers_and_critic_requirements_are_strict() {
    assert!(parse(json!({"name": "_invalid"})).is_err());
    assert!(parse(json!({"name": "a".repeat(65)})).is_err());
    assert!(parse(json!({"name": "a".repeat(64)})).is_ok());
    assert!(parse(json!({"name": "a/b"})).is_err());
    let mut declared = critic(json!({"kind": "human"}));
    declared["id"] = json!("owner/check");
    assert!(parse(json!({"name": "a", "critics": [declared.clone()]})).is_err());
    declared["id"] = json!("check");
    declared["title"] = json!(" \n");
    assert!(parse(json!({"name": "a", "critics": [declared.clone()]})).is_err());
    declared["title"] = json!("Check");
    declared["payload"]["instruction"] = json!(" \t");
    assert!(
        parse(json!({"name": "a", "critics": [declared]}))
            .unwrap_err()
            .contains("payload.instruction")
    );
    assert!(parse(json!({"name": "a", "mounts": {"bad/alias": "b"}})).is_err());
}

#[test]
fn duplicate_critics_and_basis_with_critics_are_rejected() {
    let declared = critic(json!({"kind": "human"}));
    let error =
        parse(json!({"name": "a", "critics": [declared.clone(), declared.clone()]})).unwrap_err();
    assert!(error.contains("Duplicate local Critic in a: check"));
    assert!(
        parse(json!({"name": "a", "basis": true, "critics": [declared.clone()]}))
            .unwrap_err()
            .contains("cannot own Critics")
    );
    assert!(parse(json!({"name": "a", "basis": false, "critics": [declared]})).is_ok());
}

#[test]
fn profile_fields_and_numeric_limits_match_declarations() {
    let profile = json!({"kind": "agent", "provider": "provider", "model": "model", "reasoning": "high", "timeoutMs": 2147483647, "maxToolCalls": 9007199254740991_u64, "maxTokens": 1.0});
    assert!(parse(json!({"name": "a", "critics": [critic(profile.clone())]})).is_ok());
    let mut invalid = profile.clone();
    invalid["timeoutMs"] = json!(2147483648_u64);
    assert!(
        parse(json!({"name": "a", "critics": [critic(invalid)]}))
            .unwrap_err()
            .contains("timeoutMs")
    );
    let mut invalid = profile.clone();
    invalid["maxToolCalls"] = json!(9007199254740992_u64);
    assert!(parse(json!({"name": "a", "critics": [critic(invalid)]})).is_err());
    let mut invalid = profile.clone();
    invalid["maxTokens"] = json!(0);
    assert!(parse(json!({"name": "a", "critics": [critic(invalid)]})).is_err());
    let mut invalid = profile.clone();
    invalid["timeoutMs"] = json!(1.5);
    assert!(parse(json!({"name": "a", "critics": [critic(invalid)]})).is_err());
    let mut invalid = profile.clone();
    invalid["model"] = json!(" ");
    assert!(parse(json!({"name": "a", "critics": [critic(invalid)]})).is_err());
    let mut invalid = profile;
    invalid["extra"] = json!(true);
    assert!(parse(json!({"name": "a", "critics": [critic(invalid)]})).is_err());
}

#[test]
fn fixed_scripts_preserve_literal_args_but_reject_invalid_process_fields() {
    let args = json!(["$HOME", "{input}", "; touch marker", "line\nbreak"]);
    let profile =
        json!({"kind": "runtime", "command": "arbitrary-program", "args": args, "timeoutMs": 1e3});
    let declaration = parse(json!({"name": "a", "critics": [critic(profile)]})).unwrap();
    let Profile::Runtime {
        args: actual,
        timeout_ms,
        ..
    } = &declaration.critics[0].profile
    else {
        panic!()
    };
    assert_eq!(serde_json::to_value(actual).unwrap(), args);
    assert_eq!(*timeout_ms, Some(1000));
    let bad_command = critic(json!({"kind": "runtime", "command": "sh\n", "args": []}));
    assert!(parse(json!({"name": "a", "critics": [bad_command]})).is_err());
    let bad_arg = critic(json!({"kind": "runtime", "command": "sh", "args": ["\u{0}"]}));
    assert!(parse(json!({"name": "a", "critics": [bad_arg]})).is_err());
    assert!(parse(json!({"name": "a", "stale": {"kind": "identity", "script": {"command": "identity.sh", "args": [], "shell": true}}})).is_err());
}

#[test]
fn all_hook_timeouts_are_checked_without_opening_scripts() {
    let valid = json!({
        "name": "a",
        "stale": {"kind": "identity", "script": {"command": "identity.sh", "args": []}, "timeoutMs": 2147483647},
        "envRequirements": {"ready": {"description": "Check readiness", "script": {"command": "sh", "args": ["ready.sh"]}, "timeoutMs": 1}},
        "views": {"agentTools": {"read": {"metadata": {"description": "Read {artifactName}", "inputSchema": {"type": "object"}, "resultKinds": ["text"], "observation": "content", "timeoutMs": 1000}, "script": {"command": "sh", "args": ["view.sh"]}}}},
        "critics": [{"id": "review", "title": "Review", "profile": {"kind": "agent", "provider": "p", "model": "m", "reasoning": "high"}, "payload": {"instruction": "Review"}, "resultCheck": {"script": {"command": "sh", "args": ["check.sh"]}, "timeoutMs": 30000}}]
    });
    assert!(parse(valid.clone()).is_ok());
    let mut invalid = valid.clone();
    invalid["stale"]["timeoutMs"] = json!(null);
    assert!(parse(invalid).is_err());
    let mut invalid = valid.clone();
    invalid["envRequirements"]["ready"]["timeoutMs"] = json!(0);
    assert!(parse(invalid).is_err());
    let mut invalid = valid.clone();
    invalid["views"]["agentTools"]["read"]["metadata"]["timeoutMs"] = json!(2147483648_u64);
    assert!(parse(invalid).is_err());
    let mut invalid = valid;
    invalid["critics"][0]["resultCheck"]["timeoutMs"] = json!("30000");
    assert!(parse(invalid).is_err());
}

#[test]
fn declared_paths_share_the_posix_and_windows_safe_grammar() {
    for path in [
        "",
        "/absolute",
        "C:/absolute",
        "C:\\absolute",
        "../escape",
        "a/../b",
        "a/./b",
        "a//b",
        "a/",
        "a\\b",
        "a\nb",
        "a\u{7f}b",
    ] {
        assert!(validation::path(path).is_err(), "{path:?}");
        assert!(parse(json!({"name":"a","stale":{"kind":"file-hash","paths":[path]}})).is_err());
        assert!(parse(json!({"name":"a","stale":{"kind":"identity","script":{"command":"entry","args":[]},"inputs":[path]}})).is_err());
        assert!(parse(json!({"name":"a","envRequirements":{"ready":{"description":"Ready","script":{"command":"sh","args":[]},"inputs":[path]}}})).is_err());
        assert!(parse(json!({"name":"a","views":{"agentTools":{"read":{"metadata":{"description":"Read","inputSchema":{},"resultKinds":["text"],"observation":"content","executionPaths":[path]},"script":{"command":"sh","args":[]}}}}})).is_err());
    }
    for path in ["file", "nested/file", "C:relative", "1:/relative", "a:b"] {
        assert!(validation::path(path).is_ok(), "{path:?}");
    }
    assert!(validation::path(&"a".repeat(1024)).is_ok());
    assert!(validation::path(&"a".repeat(1025)).is_err());
    assert!(validation::path(&"🦀".repeat(512)).is_ok());
    assert!(validation::path(&"🦀".repeat(513)).is_err());
}

#[test]
fn later_schema_semantics_are_kept_as_inert_objects() {
    let schema = json!({"anyOf": [{"properties": {"ownerField": {"type": "string"}}}], "ownerKeyword": true});
    let mut declared = critic(json!({"kind": "human"}));
    declared["passSchema"] = schema.clone();
    let declaration = parse(json!({"name": "a", "critics": [declared.clone()]})).unwrap();
    assert_eq!(
        declaration.critics[0].pass_schema.as_ref().unwrap(),
        schema.as_object().unwrap()
    );
    declared["failSchema"] = Value::Null;
    assert!(parse(json!({"name": "a", "critics": [declared]})).is_err());
}
