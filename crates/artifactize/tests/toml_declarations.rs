use std::{fs, path::Path};

use artifactize::config::{Profile, parse_declaration, read_workspace_config};
use serde_json::{Value, json};

mod support;

const FEATURES: &str = r#"
name = "app"
tags = ["type:code", "한글"]
mounts = { input = "source" }
fingerprint = { script = { command = "hash", args = ["{input}"], files = ["rules"], timeout_ms = 1000 } }
review_policy = { dependency_gates = "ignore" }

[views.agent_tools]
read = { builtin = "read", description = "Read {artifactName}" }
inspect = { description = "Inspect", protocol = "plain", command = "inspect", args = ["{path}"], timeout_ms = 1000, execution_paths = ["bin/inspect"], input_schema = { type = "object", properties = { path = { type = "string", minLength = 1 } }, required = ["path"], additionalProperties = false } }
[views.human_tools]
open = { description = "Open", kind = "launch", command = "open", args = ["{artifactPath}"], timeout_ms = 1000 }
print = { description = "Print", kind = "output", command = "cat", args = ["{artifactPath}/file"] }

[evals.z-runtime]
title = "Runtime"
profile = { kind = "runtime", command = "check", args = ["{input}"], timeout_ms = 2000 }
payload.instruction = "Run."

[evals.a-agent]
title = "Agent"
profile = { kind = "agent", backend = "codex", model = "fixture", reasoning = "high", timeout_ms = 3000, max_tool_calls = 10, max_tokens = 1000 }
profile_variants.careful = { kind = "agent", backend = "codex", model = "fixture", reasoning = "max", timeout_ms = 4000, max_tool_calls = 20, max_tokens = 2000 }
payload = { instruction = "Inspect {input}.", ownerData = { camelCase = true, array = [1, 2] } }
pass_schema = { type = "object", properties = { summary = { type = "string", minLength = 1 } }, required = ["summary"], additionalProperties = false }
fail_schema = { type = "object", properties = { reason = { type = "string", minLength = 1 } }, required = ["reason"], additionalProperties = false }

[evals.h-human]
title = "Human"
profile = { kind = "human" }
payload.instruction = """
Inspect the result.
Then sign off.
"""
"#;

#[test]
fn every_declaration_feature_is_standard_toml_with_owner_data_untouched() {
    let declaration = parse_declaration(FEATURES).unwrap();
    assert_eq!(declaration.tags, ["type:code", "한글"]);
    assert_eq!(declaration.mounts["input"], "source");
    assert_eq!(
        declaration
            .evals
            .iter()
            .map(|eval| eval.id.as_str())
            .collect::<Vec<_>>(),
        ["a-agent", "h-human", "z-runtime"]
    );
    assert!(matches!(
        declaration.evals[0].profile,
        Profile::Agent { .. }
    ));
    assert!(matches!(declaration.evals[1].profile, Profile::Human {}));
    assert!(matches!(
        declaration.evals[2].profile,
        Profile::Runtime { .. }
    ));
    assert_eq!(declaration.evals[0].profile_variants.len(), 1);
    assert_eq!(
        declaration.evals[0].payload.as_ref().unwrap().extra["ownerData"]["camelCase"],
        true
    );
    assert_eq!(
        declaration.evals[0].pass_schema.as_ref().unwrap()["properties"]["summary"]["minLength"],
        1
    );
    assert!(
        parse_declaration(
            "name = 'basis'\nbasis = true\nfingerprint = { files = ['src'], ignore = ['*.log'] }"
        )
        .is_ok()
    );
    assert!(
        parse_declaration("name = 'unkeyed'\nfingerprint = false")
            .unwrap()
            .fingerprint
            .is_none()
    );
}

#[test]
fn declaration_names_are_snake_case_at_every_typed_boundary() {
    for (snake, camel) in [
        ("review_policy", "reviewPolicy"),
        ("dependency_gates", "dependencyGates"),
        ("agent_tools", "agentTools"),
        ("human_tools", "humanTools"),
        ("input_schema", "inputSchema"),
        ("execution_paths", "executionPaths"),
        ("timeout_ms", "timeoutMs"),
        ("max_tool_calls", "maxToolCalls"),
        ("max_tokens", "maxTokens"),
        ("profile_variants", "profileVariants"),
        ("pass_schema", "passSchema"),
        ("fail_schema", "failSchema"),
    ] {
        let error = parse_declaration(&FEATURES.replace(snake, camel)).unwrap_err();
        assert!(
            error.contains("unknown field") && error.contains(camel),
            "{camel}: {error}"
        );
    }
    for (source, camel) in [
        (
            "name = 'a'\nfingerprint.script = { command = 'hash', args = [], timeoutMs = 1 }",
            "timeoutMs",
        ),
        (
            "name = 'a'\nviews.human_tools.open = { description = 'Open', kind = 'launch', command = 'open', args = [], timeoutMs = 1 }",
            "timeoutMs",
        ),
        (
            "name = 'a'\nevals.check = { title = 'Check', profile = { kind = 'runtime', command = 'check', args = [], timeoutMs = 1 }, payload = { instruction = 'Check.' } }",
            "timeoutMs",
        ),
        ("name = 'a'\nstaleKey = {}", "staleKey"),
        (
            "name = 'a'\nevals.check = { title = 'Check', profile = { kind = 'human' }, payload = { instruction = 'Check.' }, resultCheck = false }",
            "resultCheck",
        ),
    ] {
        let error = parse_declaration(source).unwrap_err();
        assert!(
            error.contains("unknown field") && error.contains(camel),
            "{error}"
        );
    }
}

#[test]
fn eval_tables_reject_array_shape_id_fields_and_invalid_identifiers() {
    for source in [
        "name = 'a'\n[[evals]]\nid = 'check'\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }",
        "name = 'a'\n[evals.check]\nid = 'check'\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }",
        "name = 'a'\n[evals.'bad/id']\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }",
        "name = 'a'\n[evals._bad]\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }",
    ] {
        assert!(parse_declaration(source).is_err(), "{source}");
    }
    let error = parse_declaration(
        "name = 'a'\n[evals.check]\nid = 'check'\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }",
    )
    .unwrap_err();
    assert!(error.contains("unknown field `id`"), "{error}");
    let duplicate = "name = 'a'\n[evals.check]\ntitle = 'Check'\n[evals.check]\ntitle = 'Again'";
    assert!(
        parse_declaration(duplicate)
            .unwrap_err()
            .contains("duplicate key")
    );
}

#[test]
fn datetimes_are_rejected_anywhere_with_the_full_owner_key_path() {
    for datetime in [
        "1979-05-27T07:32:00Z",
        "1979-05-27T07:32:00",
        "1979-05-27",
        "07:32:00",
    ] {
        for (field, path) in [
            ("stamp", "stamp"),
            (
                "evals.check.payload.owner.stamp",
                "evals.check.payload.owner.stamp",
            ),
            (
                "evals.check.pass_schema.properties.stamp.const",
                "evals.check.pass_schema.properties.stamp.const",
            ),
            (
                "views.agent_tools.read.input_schema.properties.stamp.const",
                "views.agent_tools.read.input_schema.properties.stamp.const",
            ),
        ] {
            let error =
                parse_declaration(&format!("name = 'a'\n{field} = {datetime}")).unwrap_err();
            assert!(
                error.contains(path) && error.contains("no JSON equivalent"),
                "{error}"
            );
        }
        let error = parse_declaration(&format!(
            "name = 'a'\nevals.check.payload.stamps = [{datetime}]"
        ))
        .unwrap_err();
        assert!(error.contains("evals.check.payload.stamps[0]"), "{error}");
    }
    let quoted = "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.', date = '1979-05-27' }";
    assert_eq!(
        parse_declaration(quoted).unwrap().evals[0]
            .payload
            .as_ref()
            .unwrap()
            .extra["date"],
        "1979-05-27"
    );
}

fn write(root: &Path, path: &str, source: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, source).unwrap();
}

#[test]
fn errors_name_the_declaration_and_parser_or_serde_positions() {
    let root = support::os::tempdir();
    for (source, detail, position) in [
        ("name = 'a'\nbroken = [", "unclosed", "line 2"),
        (
            "name = 'a'\nreviewPolicy = {}",
            "unknown field `reviewPolicy`",
            "line 2",
        ),
        ("name = '_bad'", "Artifact name must match", "line 1"),
        (
            "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'human' }",
            "Eval payload is required",
            "line 2",
        ),
    ] {
        write(root.path(), "index.artf", source);
        let error = read_workspace_config(root.path()).unwrap_err().to_string();
        assert!(
            error.contains("index.artf") && error.contains(detail) && error.contains(position),
            "{error}"
        );
        if !position.is_empty() {
            assert!(error.contains("column"), "{error}");
        }
    }
}

#[test]
fn legacy_files_fail_but_ignored_files_and_non_declarations_are_not_read() {
    let root = support::os::tempdir();
    write(root.path(), "index.artf", "name = 'a'");
    write(root.path(), "nested/artifactize.json", "not JSON");
    let error = read_workspace_config(root.path()).unwrap_err().to_string();
    assert!(
        error.contains("nested")
            && error.ends_with(
                "artifactize.json is no longer read; declare this Artifact in index.artf (TOML)."
            ),
        "{error}"
    );
    write(root.path(), ".artfignore", "nested/\n");
    write(root.path(), "other/index.artf.backup", "not TOML");
    assert_eq!(
        read_workspace_config(root.path()).unwrap().artifacts.len(),
        1
    );
    write(root.path(), ".artifactizeignore", "nested/\n");
    let error = read_workspace_config(root.path()).unwrap_err().to_string();
    assert!(
        error.ends_with(".artifactizeignore was renamed to .artfignore."),
        "{error}"
    );
}

#[test]
fn serialized_and_saved_definitions_keep_camel_case_output_names() {
    let declaration = parse_declaration(FEATURES).unwrap();
    let root = support::os::tempdir();
    write(root.path(), "index.artf", FEATURES);
    write(
        root.path(),
        "source/index.artf",
        "name = 'source'\nbasis = true",
    );
    let config = read_workspace_config(root.path()).unwrap();
    let view = artifactize::query::graph(&config, &artifactize::project::selection::Selection::All)
        .unwrap();
    let wire = serde_json::to_value(&view).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--repo")
        .arg(root.path())
        .args(["config", "graph", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        wire
    );
    let saved = artifactize::store::definitions::Definitions::from_view(&view).unwrap();
    assert_eq!(serde_json::to_value(saved).unwrap(), wire);
    let artifact = &wire["artifacts"]["app"];
    assert!(artifact.get("reviewPolicy").is_some() && artifact.get("review_policy").is_none());
    assert_eq!(artifact["reviewPolicy"]["dependencyGates"], "ignore");
    assert_eq!(artifact["fingerprint"]["script"]["timeoutMs"], 1000);
    let tool = &artifact["views"]["agentTools"]["inspect"];
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(tool["executionPaths"], json!(["bin/inspect"]));
    assert_eq!(tool["timeoutMs"], 1000);
    assert!(artifact["views"].get("humanTools").is_some());
    let eval = &wire["evals"][0]["declaration"];
    for (camel, snake) in [
        ("profileVariants", "profile_variants"),
        ("passSchema", "pass_schema"),
        ("failSchema", "fail_schema"),
    ] {
        assert!(
            eval.get(camel).is_some() && eval.get(snake).is_none(),
            "{eval}"
        );
    }
    assert_eq!(eval["id"], "a-agent");
    assert_eq!(eval["profile"]["timeoutMs"], 3000);
    assert_eq!(eval["profile"]["maxToolCalls"], 10);
    assert_eq!(eval["profile"]["maxTokens"], 1000);
    let stored = artifactize::config::StoredProfile::from(&declaration.evals[0].profile);
    let value = serde_json::to_value(stored).unwrap();
    assert_eq!(value["timeoutMs"], 3000);
    assert_eq!(
        serde_json::from_value::<artifactize::config::StoredProfile>(value.clone())
            .map(|profile| serde_json::to_value(profile).unwrap())
            .unwrap(),
        value
    );
}

#[test]
fn non_finite_numbers_are_not_silently_converted_to_json_null() {
    for number in ["inf", "+inf", "-inf", "nan", "+nan", "-nan"] {
        let error = parse_declaration(&format!(
            "name = 'a'\nevals.check.payload.owner.value = {number}"
        ))
        .unwrap_err();
        assert!(
            error.contains("evals.check.payload.owner.value")
                && error.contains("no JSON equivalent"),
            "{error}"
        );
    }
}

#[test]
fn individually_ignored_legacy_markers_do_not_fail_discovery() {
    let root = support::os::tempdir();
    write(root.path(), "index.artf", "name = 'a'");
    write(root.path(), "nested/index.artf", "name = 'b'");
    write(root.path(), "nested/artifactize.json", "not JSON");
    write(root.path(), ".artfignore", "artifactize.json\n");
    assert_eq!(
        read_workspace_config(root.path()).unwrap().artifacts.len(),
        2
    );
}

#[test]
fn dependency_eval_uses_toml_depends_on_and_keeps_saved_and_cli_depends_on_camel_case() {
    let source = "name = 'release'\n[evals.ready]\ntitle = 'Inputs are ready'\nprofile = { kind = 'dependency', depends_on = ['input'] }";
    let declaration = parse_declaration(source).unwrap();
    assert_eq!(declaration.evals[0].id, "ready");
    assert!(declaration.evals[0].payload.is_none());
    assert!(matches!(
        &declaration.evals[0].profile,
        Profile::Dependency { depends_on } if depends_on == &["input"]
    ));
    let expected = json!({"kind":"dependency","dependsOn":["input"]});
    assert_eq!(
        serde_json::to_value(&declaration.evals[0].profile).unwrap(),
        expected
    );
    let stored = artifactize::config::StoredProfile::from(&declaration.evals[0].profile);
    let stored_value = serde_json::to_value(&stored).unwrap();
    assert_eq!(stored_value, expected);
    assert_eq!(
        serde_json::to_value(
            serde_json::from_value::<artifactize::config::StoredProfile>(stored_value).unwrap()
        )
        .unwrap(),
        expected
    );
    let error = parse_declaration(&source.replace("depends_on", "dependsOn")).unwrap_err();
    assert!(error.contains("unknown field `dependsOn`"), "{error}");
    for extra in [
        "payload = { instruction = 'Inspect.' }",
        "pass_schema = {}",
        "fail_schema = {}",
        "profile_variants = {}",
    ] {
        let error = parse_declaration(&format!("{source}\n{extra}")).unwrap_err();
        assert!(error.contains("Dependency Evals cannot declare"), "{error}");
    }
    let root = support::os::tempdir();
    write(root.path(), "index.artf", source);
    write(
        root.path(),
        "input/index.artf",
        "name = 'input'\nbasis = true",
    );
    let config = read_workspace_config(root.path()).unwrap();
    let view = artifactize::query::graph(&config, &artifactize::project::selection::Selection::All)
        .unwrap();
    let wire = serde_json::to_value(&view).unwrap();
    assert_eq!(wire["evals"][0]["declaration"]["profile"], expected);
    assert!(wire["evals"][0]["declaration"].get("payload").is_none());
    let saved = artifactize::store::definitions::Definitions::from_view(&view).unwrap();
    assert_eq!(serde_json::to_value(saved).unwrap(), wire);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--repo")
        .arg(root.path())
        .args(["config", "graph", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        wire
    );
}

#[test]
fn legacy_named_directories_follow_directory_ignore_and_discovery_rules() {
    let root = support::os::tempdir();
    write(root.path(), "index.artf", "name = 'root'");
    write(
        root.path(),
        "artifactize.json/index.artf",
        "name = 'nested'",
    );
    write(root.path(), ".artfignore", "artifactize.json/\n");
    let run_graph = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_artifactize"))
            .arg("--repo")
            .arg(root.path())
            .args(["config", "graph", "--json"])
            .output()
            .unwrap()
    };
    let ignored = run_graph();
    assert!(ignored.status.success(), "{ignored:?}");
    let ignored: Value = serde_json::from_slice(&ignored.stdout).unwrap();
    assert_eq!(ignored["artifacts"].as_object().unwrap().len(), 1);
    write(root.path(), ".artfignore", "");
    let walked = run_graph();
    assert!(walked.status.success(), "{walked:?}");
    let walked: Value = serde_json::from_slice(&walked.stdout).unwrap();
    assert_eq!(walked["artifacts"].as_object().unwrap().len(), 2);
    assert_eq!(
        walked["artifacts"]["root"]["children"]["artifactize.json"],
        "nested"
    );
}

#[test]
fn unsupported_toml_values_report_the_value_position_and_full_key_path() {
    let root = support::os::tempdir();
    for (source, path, line, column) in [
        ("name = 'a'\n# Comment\nstamp = 1979-05-27\n", "stamp", 3, 9),
        (
            "name = 'a'\n[evals.check.payload]\nowner = { stamp = 07:32:00 }\n",
            "evals.check.payload.owner.stamp",
            3,
            19,
        ),
        (
            "name = 'a'\n[evals.check.payload]\nstamps = [\n  'quoted',\n  1979-05-27T07:32:00Z,\n]\n",
            "evals.check.payload.stamps[1]",
            5,
            3,
        ),
        ("name = 'a'\n\"한글\" = 1979-05-27\n", "한글", 2, 8),
        (
            "name = 'a'\r\n[evals.check.payload]\r\nstamp = 1979-05-27T07:32:00\r\n",
            "evals.check.payload.stamp",
            3,
            9,
        ),
        (
            "name = 'a'\n[evals.check.payload]\nvalue = nan\n",
            "evals.check.payload.value",
            3,
            9,
        ),
    ] {
        write(root.path(), "index.artf", source);
        let error = read_workspace_config(root.path()).unwrap_err().to_string();
        assert!(
            error.contains("index.artf")
                && error.contains(path)
                && error.contains("no JSON equivalent"),
            "{error}"
        );
        assert!(
            error.contains(&format!("line {line}, column {column}")),
            "{error}"
        );
    }
}

#[test]
fn semantic_errors_include_the_typed_key_path_and_source_position() {
    let root = support::os::tempdir();
    for (source, path, line) in [
        ("name = '_bad'", "name", 1),
        ("name = 'a'\ntags = ['same', 'same']", "tags", 2),
        (
            "name = 'a'\n[evals._bad]\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }",
            "evals._bad",
            2,
        ),
        (
            "name = 'a'\n[evals.check]\ntitle = ' ' \nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }",
            "evals.check.title",
            3,
        ),
        (
            "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = ' ' }",
            "evals.check.payload.instruction",
            5,
        ),
        (
            "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'agent', backend = 'openai', model = ' ' }\npayload = { instruction = 'Check.' }",
            "evals.check.profile.model",
            4,
        ),
        (
            "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }\npass_schema = { type = 'array' }",
            "evals.check.pass_schema",
            6,
        ),
        (
            "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }\nfail_schema = { type = 'object', additionalProperties = true }",
            "evals.check.fail_schema",
            6,
        ),
        (
            "name = 'a'\nmounts = { '_bad' = 'input' }",
            "mounts._bad",
            2,
        ),
        (
            "name = 'a'\nfingerprint = { files = [] }",
            "fingerprint.files",
            2,
        ),
        (
            "name = 'a'\nfingerprint = { ignore = ['!keep'] }",
            "fingerprint.ignore",
            2,
        ),
        (
            "name = 'a'\nviews.agent_tools.read = { builtin = 'read', description = ' ' }",
            "views.agent_tools.read",
            2,
        ),
        (
            "name = 'a'\nviews.human_tools.open = { kind = 'launch', command = 'open', args = [], description = ' ' }",
            "views.human_tools.open",
            2,
        ),
        ("name = 'a'\nfamily = false", "family", 2),
        ("name = 'a'\nstale_key = false", "stale_key", 2),
        (
            "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Check.' }\nresult_check = false",
            "evals.check.result_check",
            6,
        ),
    ] {
        write(root.path(), "index.artf", source);
        let error = read_workspace_config(root.path()).unwrap_err().to_string();
        assert!(
            error.contains("index.artf")
                && error.contains(path)
                && error.contains(&format!("line {line}, column")),
            "{error}"
        );
    }
    let error = parse_declaration("name = '_bad'").unwrap_err();
    assert!(error.contains("line 1, column 8: name:"), "{error}");
}

#[test]
fn null_syntax_is_rejected_by_the_production_toml_parser_at_every_boundary() {
    for field in [
        "basis",
        "evals",
        "views",
        "mounts",
        "fingerprint",
        "tags",
        "tags.items",
        "fingerprint.script.timeout_ms",
        "fingerprint.script.files",
        "fingerprint.script.weight",
        "views.agent_tools.read.description",
        "views.agent_tools.read.input_schema",
        "views.agent_tools.read.timeout_ms",
        "views.human_tools.open.timeout_ms",
        "evals.check.profile.reasoning",
        "evals.check.profile.depends_on",
        "evals.check.payload",
        "evals.check.payload.instruction",
        "evals.check.pass_schema",
        "evals.check.fail_schema",
        "evals.check.profile_variants",
    ] {
        let error = parse_declaration(&format!("name = 'a'\n{field} = null")).unwrap_err();
        assert!(
            error.contains("TOML parse error at line 2, column") && error.contains("null"),
            "{field}: {error}"
        );
    }
    let error = parse_declaration("name = 'a'\ntags = [null]").unwrap_err();
    assert!(
        error.contains("TOML parse error at line 2, column"),
        "{error}"
    );
}

#[test]
fn owner_payload_keys_do_not_collide_with_toml_datetime_serde_internals() {
    let source = "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'human' }\n[evals.check.payload]\ninstruction = 'Check.'\n'$__toml_private_datetime' = '1979-05-27'\nowner = { '$__toml_private_datetime' = '07:32:00', other = true }";
    let declaration = parse_declaration(source).unwrap();
    let extra = &declaration.evals[0].payload.as_ref().unwrap().extra;
    assert_eq!(extra["$__toml_private_datetime"], "1979-05-27");
    assert_eq!(
        extra["owner"],
        json!({"$__toml_private_datetime":"07:32:00", "other":true})
    );
}

#[test]
fn cross_artifact_validation_errors_keep_the_referencing_field_position() {
    let root = support::os::tempdir();
    for (source, path, line) in [
        (
            "name = 'a'\nmounts = { input = 'missing' }",
            "mounts.input",
            2,
        ),
        (
            "name = 'a'\nfingerprint.script = { command = 'hash', args = ['{missing}'] }",
            "fingerprint.script.args",
            2,
        ),
        (
            "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'human' }\npayload = { instruction = 'Read {missing}.' }",
            "evals.check.payload.instruction",
            5,
        ),
        (
            "name = 'a'\n[evals.check]\ntitle = 'Check'\nprofile = { kind = 'runtime', command = 'check', args = ['{missing}'] }\npayload = { instruction = 'Check.' }",
            "evals.check.profile.args",
            4,
        ),
        (
            "name = 'a'\n[evals.ready]\ntitle = 'Ready'\nprofile = { kind = 'dependency', depends_on = ['missing'] }",
            "evals.ready.profile.depends_on",
            4,
        ),
        (
            "name = 'a'\nviews.human_tools.open = { description = 'Open', kind = 'launch', command = 'open', args = ['{missing}'] }",
            "views.human_tools.open.args",
            2,
        ),
    ] {
        write(root.path(), "index.artf", source);
        let error = read_workspace_config(root.path()).unwrap_err().to_string();
        assert!(
            error.contains("index.artf")
                && error.contains(path)
                && error.contains(&format!("line {line}, column")),
            "{error}"
        );
    }
    write(root.path(), "index.artf", "name = 'a'");
    write(
        root.path(),
        "child/index.artf",
        "name = 'child'\nreview_policy = {}",
    );
    let error = read_workspace_config(root.path()).unwrap_err().to_string();
    assert!(
        error.contains("line 2, column") && error.contains("review_policy"),
        "{error}"
    );
    write(root.path(), "child/index.artf", "name = 'a'");
    let error = read_workspace_config(root.path()).unwrap_err().to_string();
    assert!(
        error.contains("line 1, column 8: name") && error.contains("Duplicate Artifact name"),
        "{error}"
    );
}

#[test]
fn json_backed_tool_and_fingerprint_fields_report_the_offending_field_span() {
    for (source, line, column) in [
        (
            "name = 'a'\n[views.agent_tools.inspect]\ndescription = 'Inspect'\nprotocol = 'plain'\ncommand = 'inspect'\nargs = []\ntimeoutMs = 1",
            7,
            1,
        ),
        (
            "name = 'a'\n[views.agent_tools.inspect]\ndescription = 'Inspect'\nprotocol = 'plain'\ncommand = 'inspect'\nargs = []\ntimeout_ms = 0",
            7,
            14,
        ),
        (
            "name = 'a'\n[fingerprint.script]\ncommand = 'hash'\nargs = []\ntimeoutMs = 1",
            5,
            1,
        ),
        (
            "name = 'a'\n[fingerprint.script]\ncommand = 'hash'\nargs = []\ntimeout_ms = 0",
            5,
            14,
        ),
    ] {
        let error = parse_declaration(source).unwrap_err();
        assert!(
            error.contains(&format!("line {line}, column {column}")),
            "{error}"
        );
    }
}

#[test]
fn explicitly_listed_artf_files_are_rejected_before_opening_for_artifactsum() {
    let root = support::os::tempdir();
    for input in ["index.artf", "missing.png.artf", "existing.png.artf"] {
        write(root.path(), "existing.png", "pixels");
        write(root.path(), "existing.png.artf", "name = 'image'");
        write(
            root.path(),
            "index.artf",
            &format!("name = 'a'\nfingerprint = {{ files = ['{input}'] }}"),
        );
        let error = read_workspace_config(root.path()).unwrap_err().to_string();
        assert!(
            error.contains("line 2, column")
                && error.contains("fingerprint.files")
                && error.contains("cannot be explicit artifactsum inputs"),
            "{error}"
        );
    }
}
