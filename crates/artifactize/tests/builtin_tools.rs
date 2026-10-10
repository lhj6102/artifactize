mod support;

use artifactize::{cache, config::read_workspace_config, tools};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tokio_util::sync::CancellationToken;

fn declaration(repo: &Path, agent: Value, human: Value) {
    fs::create_dir_all(repo.join("a")).unwrap();
    fs::write(
        repo.join("a/notes.md"),
        "# First\nbody\n## Nested\nchild\n# Next\nlast\n",
    )
    .unwrap();
    fs::write(repo.join("a/other.md"), "other content\n").unwrap();
    support::declaration::write(repo.join("a/index.artf"), json!({
        "name":"a", "fingerprint":false,
        "views":{"agent_tools":agent,"human_tools":human},
        "evals":[{"id":"review","title":"Review","profile":{"kind":"human"},"payload":{"instruction":"Review."}}]
    }).to_string()).unwrap();
}
fn command(repo: &Path, state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
    command
        .arg("--repo")
        .arg(repo)
        .arg("--state-dir")
        .arg(state)
        .env("ARTIFACTIZE_REMOTE", "off")
        .env("USER", "reviewer");
    command
}
fn parsed(output: Output, code: i32) -> Value {
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn config_check_accepts_fixed_and_dynamic_forms_and_positions_refusals() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    declaration(
        &repo,
        json!({
            "read":{"builtin":"read"},"list":{"builtin":"list"},"glob":{"builtin":"glob"},"grep":{"builtin":"grep"},"image":{"builtin":"view_image"},
            "fixed":{"builtin":"read","args":["notes.md"]},"folder":{"builtin":"list","args":[""]},
            "section":{"builtin":"section","args":["notes.md"]},"fixed_section":{"builtin":"section","args":["notes.md","First"]},
            "help":{"builtin":"help","args":["missing-program","sub"]}
        }),
        json!({
            "read":{"builtin":"read","args":["{artifactPath}/notes.md"]},"list":{"builtin":"list","args":["{artifactPath}"]},
            "section":{"builtin":"section","args":["notes.md","First"],"kind":"output"},
            "help":{"builtin":"help","args":["missing-program"]},
            "open":{"builtin":"open","args":["{artifactPath}/notes.md"],"kind":"launch"},
            "url":{"builtin":"open","args":["https://example.org/notes"]}
        }),
    );
    let output = command(&repo, &state)
        .args(["config", "check", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    for (audience, invalid) in [
        ("agent_tools", json!({"builtin":"unknown"})),
        ("agent_tools", json!({"builtin":"glob","args":["notes.md"]})),
        ("agent_tools", json!({"builtin":"grep","args":[]})),
        (
            "agent_tools",
            json!({"builtin":"view_image","args":["notes.md"]}),
        ),
        ("agent_tools", json!({"builtin":"open","args":["notes.md"]})),
        ("agent_tools", json!({"builtin":"read","args":[]})),
        ("agent_tools", json!({"builtin":"section"})),
        ("agent_tools", json!({"builtin":"help"})),
        (
            "agent_tools",
            json!({"builtin":"read","args":["../outside"]}),
        ),
        ("human_tools", json!({"builtin":"unknown","args":[]})),
        (
            "human_tools",
            json!({"builtin":"open","args":["notes.md"],"kind":"output"}),
        ),
        (
            "human_tools",
            json!({"builtin":"read","args":["notes.md"],"kind":"launch"}),
        ),
        (
            "human_tools",
            json!({"builtin":"section","args":["notes.md"]}),
        ),
        ("human_tools", json!({"builtin":"read"})),
        (
            "human_tools",
            json!({"builtin":"read","args":["{artifactPath}/../outside"]}),
        ),
        (
            "human_tools",
            json!({"builtin":"open","args":["file:///etc/passwd"]}),
        ),
    ] {
        let mut value = json!({"name":"a","views":{}});
        value["views"][audience] = json!({"bad":invalid});
        support::declaration::write(repo.join("a/index.artf"), value.to_string()).unwrap();
        let output = command(&repo, &state)
            .args(["config", "check", "--json"])
            .output()
            .unwrap();
        let report = parsed(output, 2);
        let error = report["error"].as_str().unwrap();
        assert!(
            error.contains("a/index.artf") || error.contains("a\\index.artf"),
            "{error}"
        );
        assert!(error.contains(&format!("views.{audience}.bad")), "{error}");
        assert!(error.contains(" --> ") || error.contains("line"), "{error}");
    }
}

#[tokio::test]
async fn fixed_agent_reads_sections_mounts_and_schemas() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    declaration(
        &repo,
        json!({
            "fixed":{"builtin":"read","args":["notes.md"]},
            "section":{"builtin":"section","args":["notes.md"]},
            "fixed_section":{"builtin":"section","args":["notes.md","First"]},
            "help":{"builtin":"help","args":["missing-program"]}
        }),
        json!({}),
    );
    let config = read_workspace_config(&repo).unwrap();
    let registry = tools::Registry::for_artifact(&config, "a").unwrap();
    let schema = |name: &str| {
        registry
            .list()
            .find(|tool| tool.name == name)
            .unwrap()
            .input_schema
            .clone()
    };
    assert_eq!(schema("fixed_a")["properties"], json!({}));
    assert_eq!(schema("help_a")["properties"], json!({}));
    assert_eq!(schema("fixed_section_a")["properties"], json!({}));
    assert_eq!(schema("section_a")["required"], json!(["heading"]));
    assert_eq!(
        schema("section_a")["properties"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        vec!["heading"]
    );
    let call =
        |name: &'static str, args| registry.call(name, args, root.path(), CancellationToken::new());
    let fixed = call("fixed_a", json!({})).await;
    assert_eq!(
        fixed.content,
        vec![tools::Content::Text {
            text: "# First\nbody\n## Nested\nchild\n# Next\nlast\n".into()
        }]
    );
    assert!(call("fixed_a", json!({"path":"other.md"})).await.is_error);
    let section = call("section_a", json!({"heading":"First"})).await;
    assert_eq!(
        section.content,
        vec![tools::Content::Text {
            text: "# First\nbody\n## Nested\nchild\n".into()
        }]
    );
    assert_eq!(call("fixed_section_a", json!({})).await, section);
    let missing = call("section_a", json!({"heading":"missing"})).await;
    assert!(missing.is_error);
    assert!(
        serde_json::to_string(&missing)
            .unwrap()
            .contains("First\\nNested\\nNext")
    );
    // A mount prefix follows the same read rule, even for fixed args.
    let mut value =
        support::declaration::read(fs::read(repo.join("a/index.artf")).unwrap()).unwrap();
    value["mounts"] = json!({"reference":"b"});
    value["views"]["agent_tools"]["mounted"] =
        json!({"builtin":"read","args":["reference/reference.md"]});
    support::declaration::write(repo.join("a/index.artf"), value.to_string()).unwrap();
    fs::create_dir_all(repo.join("b")).unwrap();
    fs::write(repo.join("b/reference.md"), "mounted text\n").unwrap();
    support::declaration::write(
        repo.join("b/index.artf"),
        json!({"name":"b","basis":true}).to_string(),
    )
    .unwrap();
    let config = read_workspace_config(&repo).unwrap();
    let result = tools::Registry::for_artifact(&config, "a")
        .unwrap()
        .call(
            "mounted_a",
            json!({}),
            root.path(),
            CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error, "{result:?}");
    assert!(
        serde_json::to_string(&result)
            .unwrap()
            .contains("mounted text")
    );
}

#[test]
fn changing_only_builtin_args_preserves_reuse_identity_like_command_views() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    declaration(
        &repo,
        json!({"fixed":{"builtin":"read","args":["notes.md"]}}),
        json!({"fixed":{"builtin":"read","args":["notes.md"]}}),
    );
    let first = read_workspace_config(&repo).unwrap();
    declaration(
        &repo,
        json!({"fixed":{"builtin":"read","args":["other.md"]}}),
        json!({"fixed":{"builtin":"read","args":["other.md"]}}),
    );
    let second = read_workspace_config(&repo).unwrap();
    assert_eq!(
        cache::eval_definition_hash(&first.evals[0].declaration),
        cache::eval_definition_hash(&second.evals[0].declaration)
    );
    let fingerprints = std::collections::BTreeMap::from([(
        "a",
        cache::PreparedFingerprint {
            value: "fixture-fingerprint".parse().unwrap(),
            manifest: None,
        },
    )]);
    assert_eq!(
        cache::eval_key(&first, &first.evals[0], &fingerprints).unwrap(),
        cache::eval_key(&second, &second.evals[0], &fingerprints).unwrap()
    );
}

#[test]
fn request_tools_execute_builtins_in_process_with_bounded_text() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    declaration(
        &repo,
        json!({}),
        json!({
            "read":{"builtin":"read","args":["{artifactPath}/notes.md"]},
            "section":{"builtin":"section","args":["{artifactPath}/notes.md","First"]},
            "list":{"builtin":"list","args":["{artifactPath}"]},
            "large":{"builtin":"read","args":["large.txt"]}
        }),
    );
    fs::write(repo.join("a/large.txt"), "x".repeat(80_000)).unwrap();
    let run = parsed(
        command(&repo, &state)
            .args(["verify", "--all", "--timeout-ms", "1", "--json"])
            .output()
            .unwrap(),
        3,
    );
    let id = run["requests"][0]["id"].as_str().unwrap();
    parsed(
        command(&repo, &state)
            .args(["request", "claim", id, "--json"])
            .output()
            .unwrap(),
        0,
    );
    for (name, expected) in [
        ("read_a", "# First"),
        ("section_a", "## Nested"),
        ("list_a", "notes.md"),
        ("large_a", "[output truncated]"),
    ] {
        let output = command(&repo, &state)
            .args(["request", "tool", id, name])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(expected), "{text}");
        assert!(text.len() <= 64 * 1024 + 1, "{}", text.len());
        if name == "section_a" {
            assert!(!text.contains("# Next"));
        }
    }
}

#[cfg(unix)]
#[test]
fn request_open_hands_one_target_to_the_shared_opener() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    declaration(
        &repo,
        json!({}),
        json!({"open":{"builtin":"open","args":["{artifactPath}/notes.md"]}}),
    );
    let bin = root.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let stub = bin.join(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    });
    fs::write(
        &stub,
        "#!/bin/sh\nprintf '%s\\n%s\\n' \"$#\" \"$1\" > \"$RECORD\"\n",
    )
    .unwrap();
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o700)).unwrap();
    let run = parsed(
        command(&repo, &state)
            .args(["verify", "--all", "--timeout-ms", "1", "--json"])
            .output()
            .unwrap(),
        3,
    );
    let id = run["requests"][0]["id"].as_str().unwrap();
    parsed(
        command(&repo, &state)
            .args(["request", "claim", id, "--json"])
            .output()
            .unwrap(),
        0,
    );
    let record = root.path().join("record");
    let output = command(&repo, &state)
        .env("PATH", bin)
        .env("RECORD", &record)
        .args(["request", "tool", id, "open_a", "--json"])
        .output()
        .unwrap();
    let result = parsed(output, 0);
    assert_eq!(result["content"][0]["launched"], true);
    assert_eq!(
        fs::read_to_string(record).unwrap(),
        format!(
            "1\n{}\n",
            fs::canonicalize(repo.join("a/notes.md")).unwrap().display()
        )
    );
}
