use serde_json::{Value, json};

use super::*;

fn eval(profile: Value) -> Value {
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
    assert!(empty.evals.is_empty());
    assert!(empty.views.agent_tools.is_empty());
    assert!(empty.views.human_tools.is_empty());
    assert!(empty.mounts.is_empty());
    assert_eq!(empty.basis, None);

    let original = eval(json!({ "kind": "human" }));
    let declaration = parse(json!({"name": "review", "evals": [original.clone()]})).unwrap();
    assert_eq!(
        declaration.evals[0].payload,
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
    assert!(parse(json!({"name": "a", "evals": null})).is_err());
    assert!(parse(json!({"name": "a", "views": null})).is_err());
    assert!(parse(json!({"name": "a", "mounts": null})).is_err());
    assert!(parse(json!({"name": "a", "fingerprint": null})).is_err());
    let mut declared = eval(json!({"kind": "human"}));
    declared["deps"] = json!([]);
    assert!(
        parse(json!({"name": "a", "evals": [declared]}))
            .unwrap_err()
            .contains("deps")
    );
    let invalid_human = eval(json!({"kind": "human", "timeoutMs": 100}));
    assert!(parse(json!({"name": "a", "evals": [invalid_human]})).is_err());
    assert!(
        parse(json!({"name": "a", "fingerprint": {"kind": "always", "paths": ["input"]}})).is_err()
    );
}

#[test]
fn identifiers_and_eval_requirements_are_strict() {
    assert!(parse(json!({"name": "_invalid"})).is_err());
    assert!(parse(json!({"name": "a".repeat(65)})).is_err());
    assert!(parse(json!({"name": "a".repeat(64)})).is_ok());
    assert!(parse(json!({"name": "a/b"})).is_err());
    let mut declared = eval(json!({"kind": "human"}));
    declared["id"] = json!("owner/check");
    assert!(parse(json!({"name": "a", "evals": [declared.clone()]})).is_err());
    declared["id"] = json!("check");
    declared["title"] = json!(" \n");
    assert!(parse(json!({"name": "a", "evals": [declared.clone()]})).is_err());
    declared["title"] = json!("Check");
    declared["payload"]["instruction"] = json!(" \t");
    assert!(
        parse(json!({"name": "a", "evals": [declared]}))
            .unwrap_err()
            .contains("payload.instruction")
    );
    assert!(parse(json!({"name": "a", "mounts": {"bad/alias": "b"}})).is_err());
}

#[test]
fn duplicate_evals_and_basis_with_evals_are_rejected() {
    let declared = eval(json!({"kind": "human"}));
    let error =
        parse(json!({"name": "a", "evals": [declared.clone(), declared.clone()]})).unwrap_err();
    assert!(error.contains("Duplicate local Eval in a: check"));
    assert!(
        parse(json!({"name": "a", "basis": true, "evals": [declared.clone()]}))
            .unwrap_err()
            .contains("cannot own Evals")
    );
    assert!(parse(json!({"name": "a", "basis": false, "evals": [declared]})).is_ok());
}

#[test]
fn profile_fields_and_numeric_limits_match_declarations() {
    let profile = json!({"kind": "agent", "backend":"openai", "model": "model", "reasoning": "high", "timeoutMs": 2147483647, "maxToolCalls": 9007199254740991_u64, "maxTokens": 1.0});
    assert!(parse(json!({"name": "a", "evals": [eval(profile.clone())]})).is_ok());
    let mut invalid = profile.clone();
    invalid["timeoutMs"] = json!(2147483648_u64);
    assert!(
        parse(json!({"name": "a", "evals": [eval(invalid)]}))
            .unwrap_err()
            .contains("timeoutMs")
    );
    let mut invalid = profile.clone();
    invalid["maxToolCalls"] = json!(9007199254740992_u64);
    assert!(parse(json!({"name": "a", "evals": [eval(invalid)]})).is_err());
    let mut invalid = profile.clone();
    invalid["maxTokens"] = json!(0);
    assert!(parse(json!({"name": "a", "evals": [eval(invalid)]})).is_err());
    let mut invalid = profile.clone();
    invalid["timeoutMs"] = json!(1.5);
    assert!(parse(json!({"name": "a", "evals": [eval(invalid)]})).is_err());
    let mut invalid = profile.clone();
    invalid["model"] = json!(" ");
    assert!(parse(json!({"name": "a", "evals": [eval(invalid)]})).is_err());
    let mut invalid = profile;
    invalid["extra"] = json!(true);
    assert!(parse(json!({"name": "a", "evals": [eval(invalid)]})).is_err());
}

#[test]
fn fixed_scripts_preserve_literal_args_but_reject_invalid_process_fields() {
    let args = json!(["$HOME", "{input}", "; touch marker", "line\nbreak"]);
    let profile =
        json!({"kind": "runtime", "command": "arbitrary-program", "args": args, "timeoutMs": 1e3});
    let declaration = parse(json!({"name": "a", "evals": [eval(profile)]})).unwrap();
    let Profile::Runtime {
        args: actual,
        timeout_ms,
        ..
    } = &declaration.evals[0].profile
    else {
        panic!()
    };
    assert_eq!(serde_json::to_value(actual).unwrap(), args);
    assert_eq!(*timeout_ms, Some(1000));
    let bad_command = eval(json!({"kind": "runtime", "command": "sh\n", "args": []}));
    assert!(parse(json!({"name": "a", "evals": [bad_command]})).is_err());
    let bad_arg = eval(json!({"kind": "runtime", "command": "sh", "args": ["\u{0}"]}));
    assert!(parse(json!({"name": "a", "evals": [bad_arg]})).is_err());
    assert!(parse(json!({"name": "a", "fingerprint": {"script":{"command": "fingerprint.sh", "args": [], "shell": true}}})).is_err());
}

#[test]
fn all_hook_timeouts_are_checked_without_opening_scripts() {
    let valid = json!({
        "name": "a",
        "fingerprint": {"script":{"command": "fingerprint.sh", "args": [],"timeoutMs": 2147483647}},
        "views": {"agentTools": {"read": {"description": "Read {artifactName}", "inputSchema": {"type": "object"}, "timeoutMs": 1000, "protocol": "json", "command": "sh", "args": ["view.sh"]}}},
        "evals": [{"id": "review", "title": "Review", "profile": {"kind": "agent", "backend":"openai", "model": "m", "reasoning": "high"}, "payload": {"instruction": "Review"}}]
    });
    assert!(parse(valid.clone()).is_ok());
    let mut invalid = valid.clone();
    invalid["fingerprint"]["script"]["timeoutMs"] = json!(null);
    assert!(parse(invalid).is_err());
    let mut invalid = valid.clone();
    invalid["views"]["agentTools"]["read"]["timeoutMs"] = json!(2147483648_u64);
    assert!(parse(invalid).is_err());
    let mut invalid = valid;
    invalid["evals"][0]["profile"]["timeoutMs"] = json!("30000");
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
        assert!(parse(json!({"name":"a","fingerprint":{"script":{"command":"entry","args":[],"files":[path]}}})).is_err());
        assert!(parse(json!({"name":"a","views":{"agentTools":{"read":{"description":"Read","inputSchema":{"type":"object"},"protocol":"json","executionPaths":[path],"command":"sh","args":[]}}}})).is_err());
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
fn response_schemas_are_validated_without_rewriting_owner_fields() {
    let schema = json!({"type":"object","properties":{"ownerField":{"anyOf":[{"type":"string"},{"type":"integer"}]}}, "ownerKeyword":true});
    let mut declared = eval(json!({"kind": "human"}));
    declared["passSchema"] = schema.clone();
    let declaration = parse(json!({"name": "a", "evals": [declared.clone()]})).unwrap();
    assert_eq!(
        declaration.evals[0].pass_schema.as_ref().unwrap(),
        schema.as_object().unwrap()
    );
    declared["failSchema"] = Value::Null;
    assert!(parse(json!({"name": "a", "evals": [declared]})).is_err());
}

#[test]
fn fingerprint_scripts_are_inert_declarations() {
    let script = json!({"script":{"command":"missing.sh","args":["literal"],"files":["missing-input"],"timeoutMs":1000}});
    let declaration = parse(json!({"name":"a","fingerprint":script})).unwrap();
    let Some(Fingerprint::Script {
        command,
        args,
        files,
        timeout_ms,
    }) = declaration.fingerprint
    else {
        panic!()
    };
    assert_eq!(command, "missing.sh");
    assert_eq!(args, ["literal"]);
    assert_eq!(files, ["missing-input"]);
    assert_eq!(timeout_ms, Some(1000));
    let minimum = json!({"script":{"command":"missing.sh","args":[]}});
    let declaration = parse(json!({"name":"a","fingerprint":minimum})).unwrap();
    let fingerprint = declaration.fingerprint.unwrap();
    let Fingerprint::Script {
        files, timeout_ms, ..
    } = &fingerprint
    else {
        panic!()
    };
    assert!(files.is_empty());
    assert_eq!(*timeout_ms, None);
    // Saved definitions keep the declared shape, with every field present.
    assert_eq!(
        serde_json::to_value(&fingerprint).unwrap(),
        json!({"script":{"command":"missing.sh","args":[],"files":[],"timeoutMs":null}})
    );
    for fingerprint in [
        json!({"kind":"always"}),
        json!({"kind":"file-hash"}),
        json!({"kind":"file-hash","paths":["input"]}),
        json!({"script":"missing.sh"}),
        json!({"script":{"command":"x","args":[]},"files":["."]}),
        json!({"script":{"command":"x","args":[]},"content":{}}),
    ] {
        assert!(parse(json!({"name":"a","fingerprint":fingerprint})).is_err());
    }
    for (key, value) in [
        ("paths", json!(["input"])),
        ("weight", json!(1)),
        ("weight", json!(101)),
        ("weight", json!(null)),
        ("files", json!(null)),
        ("inputs", json!(["input"])),
    ] {
        let mut fingerprint = minimum.clone();
        fingerprint["script"][key] = value;
        assert!(parse(json!({"name":"a","fingerprint":fingerprint})).is_err());
    }
}

#[test]
fn renamed_fingerprint_keys_fail_with_the_new_shape() {
    let shape =
        r#"use "fingerprint": {"files": ["."], "ignore": []} or "fingerprint": {"script": {...}}."#;
    for (key, value) in [
        ("staleKey", json!({"content":{}})),
        ("staleKey", json!({"script":{"command":"x","args":[]}})),
        ("stale", json!({"kind":"content"})),
    ] {
        let mut declaration = json!({"name":"a"});
        declaration[key] = value;
        assert_eq!(
            parse(declaration).unwrap_err(),
            format!("{key} was renamed to fingerprint: {shape}")
        );
    }
}

#[test]
fn content_fingerprint_defaults_to_the_owner_folder() {
    let declaration = parse(json!({"name":"a","fingerprint":{}})).unwrap();
    let fingerprint = declaration.fingerprint.unwrap();
    let Fingerprint::Content { files, ignore } = &fingerprint else {
        panic!()
    };
    assert_eq!(files, &["."]);
    assert!(ignore.is_empty());
    assert_eq!(
        serde_json::to_value(&fingerprint).unwrap(),
        json!({"files":["."],"ignore":[]})
    );
    let declared = json!({"files":["src","docs/a.md"],"ignore":["*.log","build/"]});
    let Some(Fingerprint::Content { files, ignore }) =
        parse(json!({"name":"a","fingerprint":declared}))
            .unwrap()
            .fingerprint
    else {
        panic!()
    };
    assert_eq!(files, ["src", "docs/a.md"]);
    assert_eq!(ignore, ["*.log", "build/"]);
    // The 0.4 dependency scope is gone in every form, with a message that says why.
    for scope in ["none", "direct", "transitive"] {
        let error = parse(json!({"name":"a","fingerprint":{"dependencies":scope}})).unwrap_err();
        assert!(
            error.contains("fingerprint.dependencies was removed in 0.5"),
            "{error}"
        );
    }
    for (key, value) in [
        ("dependencies", json!("all")),
        ("files", json!([])),
        ("files", json!(["/abs"])),
        ("files", json!(["a/../b"])),
        ("files", json!(["a", "a"])),
        ("ignore", json!(["!keep"])),
        ("ignore", json!(["x", "x"])),
        ("inputs", json!(["."])),
        ("content", json!({})),
        ("paths", json!(["input"])),
    ] {
        let mut fingerprint = json!({});
        fingerprint[key] = value;
        assert!(
            parse(json!({"name":"a","fingerprint":fingerprint})).is_err(),
            "{key}"
        );
    }
}

#[test]
fn dropped_configuration_fields_are_rejected() {
    for declaration in [
        json!({"name":"a","critics":[]}),
        json!({"name":"a","envRequirements":{}}),
        json!({"name":"a","reviewPolicy":{"maxConcurrentExecutors":1}}),
    ] {
        assert!(parse(declaration).unwrap_err().contains("unknown field"));
    }
    let mut declared =
        eval(json!({"kind":"agent","backend":"openai","model":"m","reasoning":"high"}));
    declared["resultCheck"] = json!({"script":{"command":"check.sh","args":[]}});
    assert!(
        parse(json!({"name":"a","evals":[declared]}))
            .unwrap_err()
            .contains("resultCheck")
    );
    assert!(
        parse(
            json!({"name":"a","views":{"agentTools":{"read":{
                "description":"Read", "protocol":"json", "command":"read.sh", "args":[], "observation":"content"
            }}}})
        )
        .is_err()
    );
}

#[test]
fn agent_backends_are_explicit_and_optional_reasoning_is_exact() {
    for backend in ["openai", "anthropic"] {
        let profile = json!({"kind":"agent","backend":backend,"model":"owner-chosen-model"});
        assert!(parse(json!({"name":"a","evals":[eval(profile)]})).is_ok());
    }
    for backend in ["chatgpt", "claude"] {
        let profile = json!({"kind":"agent","backend":backend,"model":"owner-chosen-model"});
        let error = parse(json!({"name":"a","evals":[eval(profile)]}))
            .err()
            .unwrap();
        for named in [
            format!(r#""{backend}" was removed in 0.5.0"#),
            r#""openai""#.into(),
            r#""anthropic""#.into(),
            r#""codex""#.into(),
        ] {
            assert!(error.contains(&named), "{error}");
        }
    }
    let unknown = json!({"kind":"agent","backend":"unknown","model":"m"});
    let error = parse(json!({"name":"a","evals":[eval(unknown)]}))
        .err()
        .unwrap();
    assert!(error.contains("unknown variant `unknown`"), "{error}");
    for profile in [
        json!({"kind":"agent","provider":"openai","model":"m"}),
        json!({"kind":"agent","backend":"unknown","model":"m"}),
        json!({"kind":"agent","backend":"openai","model":"m","reasoning":null}),
        json!({"kind":"agent","backend":"openai","model":"m","reasoning":"off"}),
        json!({"kind":"agent","backend":"anthropic","model":"m","reasoning":"xhigh"}),
        json!({"kind":"agent","backend":"openai","model":"m","effort":"high"}),
    ] {
        assert!(parse(json!({"name":"a","evals":[eval(profile)]})).is_err());
    }
}

#[test]
fn response_schema_contract_rejects_reserved_fields_and_open_envelopes() {
    let mut schemas = vec![
        json!({"type":"array"}),
        json!({"type":"object","properties":{"reason":{"type":"bogus"}}}),
        json!({"type":"object","additionalProperties":true}),
        json!({"type":"object","additionalProperties":{}}),
        json!({"type":"object","required":"reason"}),
        json!({"type":"object","properties":[]}),
        json!({"type":"object","$ref":"https://example.invalid/schema"}),
    ];
    for keyword in ["allOf", "anyOf", "oneOf", "not"] {
        schemas.push(json!({"type":"object",keyword:[]}));
    }
    for field in [
        "verdict",
        "reference",
        "reusedFrom",
        "executionProvenance",
        "attemptId",
        "provider",
        "model",
        "stdout",
        "stderr",
        "durationMs",
        "exitCode",
        "toolCalls",
    ] {
        schemas.push(json!({"type":"object","properties":{field:{}}}));
        schemas.push(json!({"type":"object","required":[field]}));
    }
    for schema in schemas {
        for branch in ["passSchema", "failSchema"] {
            let mut declaration = eval(json!({"kind":"human"}));
            declaration[branch] = schema.clone();
            assert!(
                parse(json!({"name":"a","evals":[declaration]})).is_err(),
                "{schema}"
            );
        }
    }
}
