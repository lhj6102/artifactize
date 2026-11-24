use std::time::Duration;

use super::*;
use crate::config::{Profile, Reasoning, edit, read_sites, read_workspace_config};

fn write_workspace(settings: Option<&str>, definitions: &[(&str, &str)]) -> tempfile::TempDir {
    let root = crate::test_os::tempdir();
    let directory = root.path().join(DEFINITION_DIR);
    std::fs::create_dir(&directory).unwrap();
    if let Some(settings) = settings {
        std::fs::write(directory.join(SETTINGS_FILE), settings).unwrap();
    }
    for (name, source) in definitions {
        std::fs::write(directory.join(format!("{name}.artf")), source).unwrap();
    }
    root
}

#[test]
fn parse_settings_accepts_empty_and_agent_profiles_and_rejects_invalid_entries() {
    assert!(parse_settings("").unwrap().profiles.is_empty());

    let parsed = parse_settings(
        r#"
[profiles.sol]
kind = "agent"
backend = "codex"
model = "gpt-6.2-sol"
reasoning = "high"
timeout_ms = 1800000
max_tool_calls = 40
max_tokens = 200000

[profiles]
custom = { kind = "agent", backend = "my-endpoint", model = "custom/v1", reasoning = "max" }
"#,
    )
    .unwrap();
    assert_eq!(parsed.profiles.len(), 2);
    match &parsed.profiles["sol"] {
        Profile::Agent {
            backend,
            model,
            reasoning,
            timeout_ms,
            max_tool_calls,
            max_tokens,
        } => {
            assert_eq!(backend.as_str(), "codex");
            assert_eq!(model.as_str(), "gpt-6.2-sol");
            assert_eq!(*reasoning, Some(Reasoning::High));
            assert_eq!(*timeout_ms, Some(Duration::from_millis(1_800_000)));
            assert_eq!(*max_tool_calls, Some(40));
            assert_eq!(*max_tokens, Some(200_000));
        }
        other => panic!("expected agent profile, got {other:?}"),
    }

    for (source, expected) in [
        ("extra = true\n", "unknown field `extra`"),
        (
            "[profiles.run]\nkind = \"runtime\"\ncommand = \"cargo\"\nargs = []\n",
            "only agent profiles",
        ),
        (
            "[profiles.person]\nkind = \"human\"\n",
            "only agent profiles",
        ),
        (
            "[profiles._bad]\nkind = \"agent\"\nbackend = \"codex\"\nmodel = \"m\"\n",
            "Profile alias name",
        ),
        (
            "[profiles.bad]\nkind = \"agent\"\nbackend = \"anthropic\"\nmodel = \"m\"\nreasoning = \"xhigh\"\n",
            "Unsupported reasoning",
        ),
        (
            "[profiles.blank]\nkind = \"agent\"\nbackend = \"codex\"\nmodel = \"  \"\n",
            "nonblank",
        ),
        (
            "[profiles.bad]\nkind = \"agent\"\nbackend = \"codex\"\nmodel = \"m\"\nextra = 1\n",
            "unknown field",
        ),
        (
            "[profiles.date]\nkind = \"agent\"\nbackend = \"codex\"\nmodel = 2026-01-01\n",
            "TOML date/time",
        ),
    ] {
        let error = parse_settings(source).unwrap_err();
        assert!(
            error.contains(expected),
            "expected {expected:?} in error for {source:?}, got {error}"
        );
    }

    let too_many = (0..=MAX_DECLARED_ITEMS)
        .map(|i| format!("p{i} = {{ kind = \"agent\", backend = \"codex\", model = \"m\" }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = parse_settings(&format!("[profiles]\n{too_many}\n")).unwrap_err();
    assert!(error.contains("at most 64"), "{error}");
}

#[test]
fn workspace_resolves_profile_aliases_overrides_and_variants() {
    let settings = r#"
[profiles.sol]
kind = "agent"
backend = "codex"
model = "gpt-6.2-sol"
reasoning = "xhigh"
timeout_ms = 1800000
max_tool_calls = 50
max_tokens = 120000

[profiles.luna]
kind = "agent"
backend = "codex"
model = "gpt-6-luna"
reasoning = "max"
timeout_ms = 1800000
"#;
    let app = r#"
path = "app/"
type = "files"

[evals.exact]
title = "Uses alias as is"
profile = !sol
red_at = "P1"
payload.instruction = "Review {app}."

[evals.exact.profile_variants]
fast = !sol(reasoning = "low", timeout_ms = 600000)
empty = !luna()
inline = { kind = "agent", backend = "openai", model = "gpt-5", reasoning = "medium" }

[evals.overridden]
title = "Overrides alias fields across lines"
profile = !sol(
  # override backend and effort together
  backend = "anthropic",
  model = "claude-opus-4-6",
  reasoning = "high",
  max_tokens = 80000,
)
profile_variants = { luna-fast = !luna(reasoning = "low") }
payload.instruction = "Review {app}."
"#;
    let root = write_workspace(Some(settings), &[("app", app)]);
    std::fs::create_dir(root.path().join("app")).unwrap();

    let config = read_workspace_config(root.path()).unwrap();
    let exact = config
        .evals
        .iter()
        .find(|eval| eval.declaration.id == "exact")
        .unwrap();
    match exact.declaration.profile() {
        Profile::Agent {
            backend,
            model,
            reasoning,
            timeout_ms,
            max_tool_calls,
            max_tokens,
        } => {
            assert_eq!(backend.as_str(), "codex");
            assert_eq!(model.as_str(), "gpt-6.2-sol");
            assert_eq!(*reasoning, Some(Reasoning::Xhigh));
            assert_eq!(*timeout_ms, Some(Duration::from_millis(1_800_000)));
            assert_eq!(*max_tool_calls, Some(50));
            assert_eq!(*max_tokens, Some(120_000));
        }
        other => panic!("expected agent profile, got {other:?}"),
    }
    match &exact.declaration.profile_variants()["fast"] {
        Profile::Agent {
            backend,
            model,
            reasoning,
            timeout_ms,
            max_tool_calls,
            max_tokens,
        } => {
            assert_eq!(backend.as_str(), "codex");
            assert_eq!(model.as_str(), "gpt-6.2-sol");
            assert_eq!(*reasoning, Some(Reasoning::Low));
            assert_eq!(*timeout_ms, Some(Duration::from_millis(600_000)));
            assert_eq!(*max_tool_calls, Some(50));
            assert_eq!(*max_tokens, Some(120_000));
        }
        other => panic!("expected agent variant, got {other:?}"),
    }
    match &exact.declaration.profile_variants()["empty"] {
        Profile::Agent { model, .. } => assert_eq!(model.as_str(), "gpt-6-luna"),
        other => panic!("expected luna variant, got {other:?}"),
    }

    let overridden = config
        .evals
        .iter()
        .find(|eval| eval.declaration.id == "overridden")
        .unwrap();
    match overridden.declaration.profile() {
        Profile::Agent {
            backend,
            model,
            reasoning,
            timeout_ms,
            max_tool_calls,
            max_tokens,
        } => {
            assert_eq!(backend.as_str(), "anthropic");
            assert_eq!(model.as_str(), "claude-opus-4-6");
            assert_eq!(*reasoning, Some(Reasoning::High));
            assert_eq!(*timeout_ms, Some(Duration::from_millis(1_800_000)));
            assert_eq!(*max_tool_calls, Some(50));
            assert_eq!(*max_tokens, Some(80_000));
        }
        other => panic!("expected overridden agent profile, got {other:?}"),
    }
    match &overridden.declaration.profile_variants()["luna-fast"] {
        Profile::Agent {
            model, reasoning, ..
        } => {
            assert_eq!(model.as_str(), "gpt-6-luna");
            assert_eq!(*reasoning, Some(Reasoning::Low));
        }
        other => panic!("expected inline table variant, got {other:?}"),
    }

    // Site and eval scope readers also resolve aliases cleanly.
    let sites = read_sites(root.path()).unwrap();
    assert!(sites.contains_key("app"));
    let scope = crate::config::read_eval_scope(root.path(), &exact.id, &sites)
        .unwrap()
        .unwrap();
    assert_eq!(scope.evals.len(), 1);
}

#[test]
fn changing_profile_alias_settings_does_not_change_eval_hash_or_graph_fingerprint() {
    let app = r#"
path = "app/"
type = "files"

[evals.review]
title = "Review"
profile = !sol(reasoning = "high")
red_at = "P1"
payload.instruction = "Check {app}."
"#;
    let first = write_workspace(
        Some(
            "[profiles.sol]\nkind = \"agent\"\nbackend = \"codex\"\nmodel = \"gpt-6.1-sol\"\nreasoning = \"xhigh\"\n",
        ),
        &[("app", app)],
    );
    let second = write_workspace(
        Some(
            "[profiles.sol]\nkind = \"agent\"\nbackend = \"anthropic\"\nmodel = \"claude-opus-4-6\"\nreasoning = \"low\"\ntimeout_ms = 60000\n",
        ),
        &[("app", app)],
    );
    let first_config = read_workspace_config(first.path()).unwrap();
    let second_config = read_workspace_config(second.path()).unwrap();
    assert_eq!(
        crate::cache::eval_definition_hash(&first_config.evals[0].declaration),
        crate::cache::eval_definition_hash(&second_config.evals[0].declaration)
    );
    assert_eq!(
        crate::graph_structure::extract(&first_config).fingerprint(),
        crate::graph_structure::extract(&second_config).fingerprint()
    );
}

#[test]
fn alias_errors_report_exact_file_line_and_column() {
    let settings = r#"
[profiles.sol]
kind = "agent"
backend = "codex"
model = "gpt-6.2-sol"
reasoning = "xhigh"
"#;

    // Unknown alias points to its exact line and mentions settings.toml.
    let unknown = write_workspace(
        Some(settings),
        &[(
            "app",
            "path = \"app/\"\ntype = \"files\"\n[evals.review]\ntitle = \"Review\"\nprofile = !missing\npayload.instruction = \"Check {app}.\"\n",
        )],
    );
    let error = read_workspace_config(unknown.path()).unwrap_err();
    assert!(error.path.ends_with(".artifactize/app.artf"), "{error}");
    assert!(
        error.message.contains("line 5, column 12")
            && error.message.contains("Unknown profile alias `missing`")
            && error.message.contains(".artifactize/settings.toml"),
        "{error}"
    );

    // Overriding kind is rejected at the kind key's line.
    let kind_override = write_workspace(
        Some(settings),
        &[(
            "app",
            "path = \"app/\"\ntype = \"files\"\n[evals.review]\ntitle = \"Review\"\nprofile = !sol(\n  kind = \"runtime\",\n)\npayload.instruction = \"Check {app}.\"\n",
        )],
    );
    let error = read_workspace_config(kind_override.path()).unwrap_err();
    assert!(
        error.message.contains("line 6, column 3") && error.message.contains("cannot set `kind`"),
        "{error}"
    );

    // Overriding backend to one incompatible with inherited reasoning fails at the backend override.
    let incompatible = write_workspace(
        Some(settings),
        &[(
            "app",
            "path = \"app/\"\ntype = \"files\"\n[evals.review]\ntitle = \"Review\"\nprofile = !sol(\n  backend = \"anthropic\",\n)\npayload.instruction = \"Check {app}.\"\n",
        )],
    );
    let error = read_workspace_config(incompatible.path()).unwrap_err();
    assert!(
        error.message.contains("line 6, column 13")
            && error.message.contains("Unsupported reasoning"),
        "{error}"
    );

    // Error on a line AFTER a multiline alias invocation keeps its exact original line number.
    let after_multiline = write_workspace(
        Some(settings),
        &[(
            "app",
            "path = \"app/\"\ntype = \"files\"\n[evals.review]\ntitle = \"Review\"\nprofile = !sol(\n  reasoning = \"low\",\n  timeout_ms = 60000,\n)\npayload.instruction = \"   \"\n",
        )],
    );
    let error = read_workspace_config(after_multiline.path()).unwrap_err();
    assert!(
        error.message.contains("line 9, column 23"),
        "expected line 9 column 23 after multiline alias, got {error}"
    );

    // Using an alias outside profile / profile_variants is rejected.
    for (field, source) in [
        ("path", "path = !sol\nbasis = true\n"),
        ("tags", "path = \"app/\"\nbasis = true\ntags = [!sol]\n"),
        (
            "title",
            "path = \"app/\"\ntype = \"files\"\n[evals.review]\ntitle = !sol\nprofile = !sol\npayload.instruction = \"Check {app}.\"\n",
        ),
        (
            "annotations",
            "path = \"app/\"\nbasis = true\n[annotations.comment]\ntext = !sol\n",
        ),
    ] {
        let root = write_workspace(Some(settings), &[("app", source)]);
        let error = read_workspace_config(root.path()).unwrap_err();
        assert!(
            error
                .message
                .contains("can be used only as an eval profile"),
            "expected alias-position refusal for {field}, got {error}"
        );
    }

    // Runtime eval cannot use an agent alias in profile_variants.
    let kind_mismatch = write_workspace(
        Some(settings),
        &[(
            "app",
            "path = \"app/\"\n[evals.check]\ntitle = \"Check\"\nprofile = { kind = \"runtime\", command = \"cargo\", args = [] }\n[evals.check.profile_variants]\nalt = !sol\n[evals.check.payload]\ninstruction = \"Check {app}.\"\n",
        )],
    );
    let error = read_workspace_config(kind_mismatch.path()).unwrap_err();
    assert!(
        error.message.contains("retain the declared reviewer kind"),
        "{error}"
    );

    // Syntax errors report exact locations.
    for (source, expected) in [
        (
            "path = \"app/\"\n[evals.review]\ntitle = \"Review\"\nprofile = !\npayload.instruction = \"Check {app}.\"\n",
            "line 4, column 11",
        ),
        (
            "path = \"app/\"\n[evals.review]\ntitle = \"Review\"\nprofile = !sol(reasoning = \"low\"\npayload.instruction = \"Check {app}.\"\n",
            "line 4, column 15",
        ),
        (
            "path = \"app/\"\n[evals.review]\ntitle = \"Review\"\nprofile = !sol(} #)\npayload.instruction = \"Check {app}.\"\n",
            "line 4, column 16",
        ),
    ] {
        let root = write_workspace(Some(settings), &[("app", source)]);
        let error = read_workspace_config(root.path()).unwrap_err();
        assert!(error.message.contains(expected), "{error}");
    }

    // Standalone parse_declaration without workspace settings rejects unresolved aliases.
    let standalone = crate::config::parse_declaration(
        "name = \"app\"\n[evals.review]\ntitle = \"Review\"\nprofile = !sol\npayload.instruction = \"Check {app}.\"\n",
    )
    .unwrap_err();
    assert!(
        standalone.contains("Unknown profile alias `sol`"),
        "{standalone}"
    );

    // A directory at .artifactize/settings.toml is refused as non-regular.
    let dir_settings = write_workspace(None, &[("app", "path = \"app/\"\nbasis = true\n")]);
    std::fs::create_dir(dir_settings.path().join(DEFINITION_DIR).join(SETTINGS_FILE)).unwrap();
    let error = read_workspace_config(dir_settings.path()).unwrap_err();
    assert!(
        error.path.ends_with(".artifactize/settings.toml")
            && error.message.contains("regular file"),
        "{error}"
    );
}

#[test]
fn editor_operations_preserve_alias_syntax_verbatim() {
    let settings = r#"
[profiles.sol]
kind = "agent"
backend = "codex"
model = "gpt-6.2-sol"
reasoning = "high"
"#;
    let app = concat!(
        "path = \"app/\"\n",
        "type = \"files\"\n",
        "\n",
        "[evals.review]\n",
        "title = \"Review\"\n",
        "profile = !sol(\n",
        "  # keep this comment\n",
        "  reasoning = \"low\",\n",
        ")\n",
        "payload.instruction = \"Review {app} against {spec}.\"\n",
        "\n",
        "[evals.review.profile_variants]\n",
        "full = !sol\n",
    );
    let spec = "path = \"spec/\"\ntype = \"files\"\nbasis = true\n";
    let root = write_workspace(Some(settings), &[("app", app), ("spec", spec)]);

    // Read project snapshot and save canvas position + comment.
    let project = edit::read_project(root.path()).unwrap();
    let mut based_on = project.files;
    let delta = edit::apply(
        root.path(),
        &based_on,
        edit::Operation::SavePlaces {
            places: [("app".parse().unwrap(), [120.0, 40.0])].into(),
        },
    )
    .unwrap();
    based_on.extend(delta);

    let delta = edit::apply(
        root.path(),
        &based_on,
        edit::Operation::SetComment {
            name: "app".parse().unwrap(),
            text: "Uses shared alias".into(),
        },
    )
    .unwrap();
    based_on.extend(delta);

    // Rename referenced Artifact and verify alias syntax survives untouched.
    let mut rename_base = based_on.clone();
    rename_base.insert(
        std::path::PathBuf::from(".artifactize/requirements.artf"),
        None,
    );
    let delta = edit::apply(
        root.path(),
        &rename_base,
        edit::Operation::Rename {
            old: "spec".parse().unwrap(),
            new: "requirements".parse().unwrap(),
        },
    )
    .unwrap();
    based_on.extend(delta);

    let updated = std::fs::read_to_string(root.path().join(".artifactize/app.artf")).unwrap();
    assert!(
        updated.contains("profile = !sol(\n  # keep this comment\n  reasoning = \"low\",\n)"),
        "{updated}"
    );
    assert!(updated.contains("full = !sol\n"), "{updated}");
    assert!(
        updated.contains("Review {app} against {requirements}."),
        "{updated}"
    );
    assert!(updated.contains("at = [120.0, 40.0]"), "{updated}");
    assert!(
        updated.contains("text = \"Uses shared alias\""),
        "{updated}"
    );

    // Delete refusal inspects references in files using aliases without panicking.
    let refusal = edit::apply(
        root.path(),
        &based_on,
        edit::Operation::Delete {
            name: "requirements".parse().unwrap(),
        },
    )
    .unwrap_err();
    assert!(
        refusal.to_string().contains("still named by: app"),
        "{refusal}"
    );

    // SaveSource validates alias references and returns line/column on failure.
    let refusal = edit::apply(
        root.path(),
        &based_on,
        edit::Operation::SaveSource {
            name: "app".parse().unwrap(),
            source: "path = \"app/\"\ntype = \"files\"\n[evals.review]\ntitle = \"Review\"\nprofile = !nope\npayload.instruction = \"Check {app}.\"\n".into(),
        },
    )
    .unwrap_err();
    match refusal {
        edit::Refusal::Invalid {
            line,
            column,
            message,
            ..
        } => {
            assert_eq!(line, Some(5));
            assert_eq!(column, Some(12));
            assert!(
                message.contains("Unknown profile alias `nope`"),
                "{message}"
            );
        }
        other => panic!("expected Invalid refusal, got {other:?}"),
    }
}

#[test]
fn config_migrate_type_pass_preserves_profile_aliases() {
    let settings = "[profiles.sol]\nkind = \"agent\"\nbackend = \"codex\"\nmodel = \"gpt-6.2-sol\"\nreasoning = \"high\"\n";
    let app = concat!(
        "path = \"app/\"\n",
        "\n",
        "[evals.review]\n",
        "title = \"Review\"\n",
        "profile = !sol(reasoning = \"low\")\n",
        "payload.instruction = \"Check {app}.\"\n",
    );
    let root = write_workspace(Some(settings), &[("app", app)]);
    std::fs::create_dir(root.path().join("app")).unwrap();

    let report = crate::config::migrate::migrate(root.path()).unwrap();
    assert_eq!(
        report.typed,
        vec!["app".parse::<crate::types::ArtifactName>().unwrap()]
    );
    let updated = std::fs::read_to_string(root.path().join(".artifactize/app.artf")).unwrap();
    assert!(updated.contains("type = \"files\""), "{updated}");
    assert!(
        updated.contains("profile = !sol(reasoning = \"low\")"),
        "{updated}"
    );
}
